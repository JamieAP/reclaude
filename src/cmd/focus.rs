use chrono::{DateTime, Datelike, Duration, Utc};

use crate::cli::FocusArgs;
use crate::db::Database;
use crate::gemini::GeminiClient;
use crate::models::Event;

/// Character budget for a single Gemini synthesis call.
const FOCUS_CHAR_BUDGET: usize = 120_000;

/// Don't bisect windows smaller than this.
const MIN_WINDOW_MINUTES: i64 = 5;

/// Scale → tile duration in minutes and prompt text.
struct ScaleConfig {
    tile_minutes: i64,
    prompt: &'static str,
}

fn scale_config(scale: &str) -> Option<ScaleConfig> {
    Some(match scale {
        "15min" => ScaleConfig { tile_minutes: 15, prompt: PROMPT_15MIN },
        "hour" => ScaleConfig { tile_minutes: 60, prompt: PROMPT_HOUR },
        "8hour" => ScaleConfig { tile_minutes: 480, prompt: PROMPT_8HOUR },
        "day" => ScaleConfig { tile_minutes: 1440, prompt: PROMPT_DAY },
        "week" => ScaleConfig { tile_minutes: 10080, prompt: PROMPT_WEEK },
        _ => return None,
    })
}

/// Generate on-demand focus summary via Gemini synthesis.
pub async fn run(args: &FocusArgs, db: &Database) -> anyhow::Result<()> {
    let scale = &args.scale;
    let budget = if args.budget > 0 { args.budget } else { FOCUS_CHAR_BUDGET };

    let cfg = match scale_config(scale) {
        Some(c) => c,
        None => {
            eprintln!("Unknown scale: {scale} (expected: 15min, hour, 8hour, day, week)");
            return Ok(());
        }
    };

    let now = Utc::now();
    let tiles = clock_tiles(cfg.tile_minutes, now);

    if tiles.is_empty() {
        eprintln!("No tiles for {scale}.");
        return Ok(());
    }

    eprintln!(
        "Focus {scale}: {} tile(s) covering {} to {}",
        tiles.len(),
        tiles[0].0.format("%Y-%m-%d %H:%M"),
        tiles.last().unwrap().1.format("%H:%M"),
    );

    // Create Gemini client (unless dry-run)
    let mut gemini = if args.dry_run {
        None
    } else {
        Some(GeminiClient::new(Some(&args.model))?)
    };

    for (tile_start, tile_end) in &tiles {
        synthesize_window(
            db,
            gemini.as_mut(),
            &cfg,
            scale,
            *tile_start,
            *tile_end,
            budget,
            args.dry_run,
        )
        .await?;
    }

    Ok(())
}

/// Generate clock-snapped tiles covering the lookback window.
fn clock_tiles(tile_minutes: i64, now: DateTime<Utc>) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    let lookback = Duration::minutes(tile_minutes);
    let lookback_start = now - lookback;
    let tile_dur = Duration::minutes(tile_minutes);

    let tile_start = snap_to_tile(lookback_start, tile_minutes);

    let mut tiles = Vec::new();
    let mut start = tile_start;
    while start < now {
        let end = std::cmp::min(start + tile_dur, now);
        tiles.push((start, end));
        start = start + tile_dur;
    }
    tiles
}

/// Snap a datetime down to the nearest tile boundary.
fn snap_to_tile(dt: DateTime<Utc>, tile_minutes: i64) -> DateTime<Utc> {
    if tile_minutes == 10080 {
        // Week: snap to Monday 00:00 UTC
        let weekday = dt.weekday().num_days_from_monday() as i64;
        let midnight = dt.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let snapped = midnight - chrono::Duration::days(weekday);
        DateTime::from_naive_utc_and_offset(snapped, Utc)
    } else {
        // Snap within day
        let midnight = dt.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let secs_into_day = (dt.naive_utc() - midnight).num_seconds();
        let tile_secs = tile_minutes.min(1440) * 60;
        let snapped_secs = (secs_into_day / tile_secs) * tile_secs;
        let snapped = midnight + chrono::Duration::seconds(snapped_secs);
        DateTime::from_naive_utc_and_offset(snapped, Utc)
    }
}

/// Recursively synthesize a time window, bisecting if content exceeds budget.
async fn synthesize_window(
    db: &Database,
    mut gemini: Option<&mut GeminiClient>,
    cfg: &ScaleConfig,
    scale: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    budget: usize,
    dry_run: bool,
) -> anyhow::Result<()> {
    let since = start.to_rfc3339();
    let until = end.to_rfc3339();

    let events = db
        .events
        .query_range(&[], None, None, Some(&since), Some(&until), 50_000)
        .await?;

    if events.is_empty() {
        return Ok(());
    }

    // Events come desc, reverse for chronological
    let events: Vec<&Event> = events.iter().rev().collect();
    let content = build_focus_content(&events, scale);

    if content.len() <= budget || (end - start).num_minutes() <= MIN_WINDOW_MINUTES {
        return do_synthesis(gemini, cfg, scale, start, end, &events, &content, dry_run).await;
    }

    // Bisect
    let mid = start + (end - start) / 2;
    eprintln!(
        "Window {}-{} exceeds budget ({} > {} chars), splitting...",
        start.format("%H:%M"),
        end.format("%H:%M"),
        content.len(),
        budget,
    );

    // Bisect oversized sections and fetch events separately for each half.
    let since_half = start.to_rfc3339();
    let mid_str = mid.to_rfc3339();
    let until_half = end.to_rfc3339();

    // First half
    let events1 = db
        .events
        .query_range(&[], None, None, Some(&since_half), Some(&mid_str), 50_000)
        .await?;
    if !events1.is_empty() {
        let events1_ref: Vec<&Event> = events1.iter().rev().collect();
        let content1 = build_focus_content(&events1_ref, scale);
        do_synthesis(gemini.as_deref_mut(), cfg, scale, start, mid, &events1_ref, &content1, dry_run).await?;
    }

    // Second half
    let events2 = db
        .events
        .query_range(&[], None, None, Some(&mid_str), Some(&until_half), 50_000)
        .await?;
    if !events2.is_empty() {
        let events2_ref: Vec<&Event> = events2.iter().rev().collect();
        let content2 = build_focus_content(&events2_ref, scale);
        do_synthesis(gemini, cfg, scale, mid, end, &events2_ref, &content2, dry_run).await?;
    }

    Ok(())
}

/// Run a single synthesis call.
async fn do_synthesis(
    gemini: Option<&mut GeminiClient>,
    cfg: &ScaleConfig,
    scale: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    events: &[&Event],
    content: &str,
    dry_run: bool,
) -> anyhow::Result<()> {
    let period = format!(
        "{} to {}",
        start.format("%Y-%m-%d %H:%M"),
        end.format("%H:%M")
    );

    if dry_run {
        println!("\n=== DRY RUN: {period} ===");
        println!("Scale: {scale}");
        println!("Events: {} total", events.len());
        println!("Input size: {} chars", content.len());
        println!("\n--- STRUCTURED INPUT PREVIEW ---");
        let preview: String = content.chars().take(3000).collect();
        println!("{preview}");
        if content.len() > 3000 {
            println!("...");
        }
        return Ok(());
    }

    let gemini = match gemini {
        Some(g) => g,
        None => anyhow::bail!("Gemini client not available"),
    };

    eprintln!(
        "Synthesizing {period}: {} events ({} chars)...",
        events.len(),
        content.len()
    );

    let output = gemini.generate(cfg.prompt, content).await?;

    println!("\n## Focus: {scale}\n*{period} -- {} events*\n", events.len());
    println!("{output}");

    Ok(())
}

/// Build structured Gemini input from events.
fn build_focus_content(events: &[&Event], scale: &str) -> String {
    // Group events by category
    let mut code_changes: Vec<&Event> = Vec::new();
    let mut assistants: Vec<&Event> = Vec::new();
    let mut tool_uses: Vec<&Event> = Vec::new();
    let mut prompts: Vec<&Event> = Vec::new();
    let mut plans: Vec<&Event> = Vec::new();
    let mut other_count: usize = 0;

    for e in events {
        match e.event_type.as_str() {
            "file_diff" => code_changes.push(e),
            "assistant" => assistants.push(e),
            "tool_use" => tool_uses.push(e),
            "user_prompt" => prompts.push(e),
            "plan" | "plan_file" => plans.push(e),
            "compaction" | "thinking" => prompts.push(e),
            _ => other_count += 1,
        }
    }

    let mut sections = Vec::new();

    // Plans
    if !plans.is_empty() {
        let mut lines = Vec::new();
        for e in &plans {
            let ts = &e.timestamp[..16];
            let content = squeeze(&e.content);
            lines.push(format!("[{ts}] {content}"));
        }
        sections.push(format!("## PLANS\n{}", lines.join("\n")));
    }

    // Code changes
    if !code_changes.is_empty() {
        if scale == "day" || scale == "week" {
            // Aggregate for larger scales
            let mut file_stats: std::collections::HashMap<String, (i64, i64, usize)> =
                std::collections::HashMap::new();
            for e in &code_changes {
                let meta = e.metadata();
                let path = meta["file_path"].as_str().unwrap_or("unknown").to_string();
                let added = meta["lines_added"].as_i64().unwrap_or(0);
                let removed = meta["lines_removed"].as_i64().unwrap_or(0);
                let entry = file_stats.entry(path).or_default();
                entry.0 += added;
                entry.1 += removed;
                entry.2 += 1;
            }
            let mut sorted: Vec<_> = file_stats.into_iter().collect();
            sorted.sort_by(|a, b| b.1 .2.cmp(&a.1 .2));
            let lines: Vec<String> = sorted
                .iter()
                .map(|(path, (added, removed, count))| {
                    format!("- {path}: +{added}/-{removed} ({count} edits)")
                })
                .collect();
            sections.push(format!(
                "## CODE CHANGES ({} total edits)\n{}",
                code_changes.len(),
                lines.join("\n")
            ));
        } else {
            let mut lines = Vec::new();
            for e in &code_changes {
                let meta = e.metadata();
                let ts = &e.timestamp[11..16];
                let path = meta["file_path"].as_str().unwrap_or("unknown");
                let added = meta["lines_added"].as_i64().unwrap_or(0);
                let removed = meta["lines_removed"].as_i64().unwrap_or(0);
                lines.push(format!("[{ts}] {path} (+{added}/-{removed})"));
            }
            sections.push(format!("## CODE CHANGES\n{}", lines.join("\n")));
        }
    }

    // Assistant responses
    if !assistants.is_empty() {
        let mut lines = Vec::new();
        for e in &assistants {
            let ts = &e.timestamp[..16];
            let content = squeeze(&e.content);
            lines.push(format!("[{ts}] {content}"));
        }
        sections.push(format!("## CLAUDE'S RESPONSES\n{}", lines.join("\n---\n")));
    }

    // Tool use
    if !tool_uses.is_empty() {
        let mut lines = Vec::new();
        for e in &tool_uses {
            let meta = e.metadata();
            let ts = &e.timestamp[11..16];
            let tool = meta["tool_name"].as_str().unwrap_or("?");
            let ok = if meta["success"].as_bool().unwrap_or(true) {
                "\u{2713}"
            } else {
                "\u{2717}"
            };
            let summary = crate::cmd::events::tool_preview(tool, &e.content);
            lines.push(format!("[{ts}] {tool}: {summary} {ok}"));
        }
        sections.push(format!(
            "## TOOL USE ({} calls)\n{}",
            tool_uses.len(),
            lines.join("\n")
        ));
    }

    // Raw activity (prompts, thinking)
    if !prompts.is_empty() {
        let mut lines = Vec::new();
        for e in &prompts {
            let ts = &e.timestamp[..16];
            let content = squeeze(&e.content);
            lines.push(format!("[{ts}] ({}) {content}", e.event_type));
        }
        sections.push(format!("## RAW ACTIVITY\n{}", lines.join("\n---\n")));
    }

    if other_count > 0 {
        sections.push(format!("## OTHER ({other_count} events)"));
    }

    sections.join("\n\n")
}

/// Compress whitespace in content for compact display.
fn squeeze(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Focus Prompts ──────────────────────────────────────────────────

const PROMPT_15MIN: &str = "Summarize the developer's work in the LAST 15 MINUTES.\n\n\
The input is organized into sections:\n\
- PLANS: Implementation plans being followed (goals/context)\n\
- CODE CHANGES: Files modified with diffs\n\
- CLAUDE'S RESPONSES: What Claude accomplished (outcomes)\n\
- TOOL USE: Commands and tools executed\n\
- RAW ACTIVITY: Prompts, plans, and thinking (fill gaps)\n\n\
What exactly happened in this short window? Be precise and granular.\n\n\
Be specific and concise (2-4 bullets). Include file names, function names, commands where relevant.";

const PROMPT_HOUR: &str = "Summarize the developer's work in the LAST HOUR.\n\n\
The input is organized into sections:\n\
- PLANS: Implementation plans being followed\n\
- CODE CHANGES: Files modified with line counts\n\
- CLAUDE'S RESPONSES: What Claude accomplished\n\
- RAW ACTIVITY: Prompts, plans, and thinking\n\n\
What specific task(s) were they working on? What progress was made?\n\
Note any blockers, decisions, or discoveries.\n\n\
Be specific and concise (3-5 bullets). Include file names, function names where relevant.";

const PROMPT_8HOUR: &str = "Summarize the developer's work over the LAST 8 HOURS.\n\n\
What were the main tasks tackled? What was accomplished vs still in progress?\n\
Note significant decisions, blockers overcome, or patterns discovered.\n\n\
Be thorough but concise (5-10 bullets). Group by task/feature if multiple threads.";

const PROMPT_DAY: &str = "Summarize the developer's work over the LAST 24 HOURS.\n\n\
What features, bugs, or research got attention? What meaningful progress was made?\n\
Note any recurring themes, decisions made, or insights gained.\n\n\
Structured summary (8-15 bullets). Group by project/feature area.";

const PROMPT_WEEK: &str = "Summarize the developer's work over the LAST 7 DAYS.\n\n\
What were the major themes and sustained efforts vs one-off tasks?\n\
What progress was made toward larger goals? Any patterns in the work?\n\n\
High-level summary (10-20 bullets). Focus on outcomes and evolution.";
