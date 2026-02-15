use crate::cli::EventsArgs;
use crate::cmd;
use crate::db::Database;
use crate::fzf;
use crate::models::{resolve_type_alias, Event};

/// Show recent events with optional type filtering and output modes.
pub async fn run(args: &EventsArgs, db: &Database) -> anyhow::Result<()> {
    // Single event by ID
    if let Some(id) = args.id {
        return show_single_event(id, args.json, db).await;
    }

    // Resolve session filter
    let session_id = if let Some(ref s) = args.session {
        let sessions = db.meta.list_sessions(None, 100)?;
        cmd::resolve_session(&sessions, Some(s))
    } else {
        None
    };

    // CWD scoping
    let cwd = if args.all {
        None
    } else {
        Some(args.cwd.clone().unwrap_or_else(cmd::current_dir))
    };

    // Parse --type argument
    let type_arg = args.event_type.as_deref();
    let requested_types: Vec<&str> = match type_arg {
        Some(t) => t.split(',').map(|s| s.trim()).collect(),
        None => Vec::new(),
    };

    // Validate type names
    for t in &requested_types {
        if resolve_type_alias(t).is_none() {
            let valid: Vec<&str> = crate::models::event_type_configs()
                .iter()
                .map(|(name, _)| *name)
                .collect();
            eprintln!("Invalid event type: {t}");
            eprintln!("Valid types: {}", valid.join(", "));
            return Ok(());
        }
    }

    // Determine limit and db event types
    let (limit, db_types) = if requested_types.is_empty() {
        let limit = args.limit.unwrap_or(20);
        (limit, Vec::new())
    } else {
        let limit = args.limit.unwrap_or_else(|| {
            requested_types
                .iter()
                .filter_map(|t| resolve_type_alias(t))
                .map(|c| c.default_limit)
                .max()
                .unwrap_or(20)
        });
        let db_types: Vec<String> = requested_types
            .iter()
            .filter_map(|t| resolve_type_alias(t))
            .flat_map(|c| c.semantic_types.iter())
            .map(|et| et.to_string())
            .collect();
        (limit, db_types)
    };

    // Query events
    let db_type_refs: Vec<&str> = db_types.iter().map(|s| s.as_str()).collect();
    let events = db
        .events
        .query(
            &db_type_refs,
            session_id.as_deref(),
            cwd.as_deref(),
            limit,
        )
        .await?;

    if events.is_empty() {
        let msg = if !requested_types.is_empty() {
            requested_types
                .first()
                .and_then(|t| resolve_type_alias(t))
                .map(|c| c.empty_message)
                .unwrap_or("No events found")
        } else {
            "No events found"
        };
        println!("{msg}");
        return Ok(());
    }

    // Reverse for chronological display (events come in desc order)
    let events_chrono: Vec<&Event> = events.iter().rev().collect();

    // FZF interactive mode
    if args.fzf {
        return fzf_events(&events_chrono);
    }

    // JSON output
    if args.json {
        let payload: Vec<serde_json::Value> = events_chrono
            .iter()
            .map(|e| event_to_json(e, args.full))
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    // Compact output (for fzf preview panes)
    if args.compact {
        for e in &events_chrono {
            println!("{}", format_compact(e));
        }
        return Ok(());
    }

    // Human-readable output
    for e in &events_chrono {
        println!("{}", format_event_line(e, args.full));
    }

    Ok(())
}

async fn show_single_event(id: i64, json: bool, db: &Database) -> anyhow::Result<()> {
    let event = db.events.get_by_id(id).await?;
    match event {
        Some(e) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&event_to_json(&e, true))?);
            } else {
                let sid = cmd::short_sid(e.session_id.as_deref());
                println!(
                    "[{}] {} (id={}, session={sid})\n",
                    e.timestamp, e.event_type, e.id
                );
                println!("{}", e.content);
            }
        }
        None => {
            eprintln!("No event with id {id}");
        }
    }
    Ok(())
}

fn event_to_json(e: &Event, full: bool) -> serde_json::Value {
    let content = if full {
        e.content.clone()
    } else {
        e.content.chars().take(1000).collect()
    };
    serde_json::json!({
        "id": e.id,
        "timestamp": e.timestamp,
        "event_type": e.event_type,
        "session_id": e.session_id,
        "content": content,
        "metadata": e.metadata(),
    })
}

/// Format a single event line for CLI output.
fn format_event_line(e: &Event, full: bool) -> String {
    let ts = if e.timestamp.len() >= 16 {
        e.timestamp[5..16].replace('T', " ") // "02-15 12:34"
    } else {
        e.timestamp.clone()
    };
    let sid = cmd::short_sid(e.session_id.as_deref());
    let meta = e.metadata();

    match e.event_type.as_str() {
        "file_diff" => {
            let file_path = meta["file_path"].as_str().unwrap_or("unknown");
            let op = meta["operation"].as_str().unwrap_or("?");
            let added = meta["lines_added"].as_i64().unwrap_or(0);
            let removed = meta["lines_removed"].as_i64().unwrap_or(0);
            format!("{ts} {sid} {op} {file_path} (+{added}/-{removed})")
        }
        "tool_use" => {
            let tool = meta["tool_name"].as_str().unwrap_or("?");
            let success = meta["success"].as_bool().unwrap_or(true);
            let status = if success { "ok" } else { "FAIL" };
            let summary = tool_preview(tool, &e.content);
            format!("{ts} {sid} {tool} ({status}) {summary}")
        }
        "pre_tool_use" => {
            let tool = meta["tool_name"].as_str().unwrap_or("?");
            format!("{ts} {sid} pre_tool_use: {tool}")
        }
        "user_prompt" | "plan" => {
            if full {
                format!("{ts} {sid}")
            } else {
                let preview: String = e.content.chars().take(300).collect();
                let preview = preview.replace('\n', " ");
                let ellipsis = if e.content.chars().count() > 300 { "..." } else { "" };
                format!("{ts} {sid} {preview}{ellipsis}")
            }
        }
        _ => {
            let preview: String = e.content.chars().take(300).collect();
            let preview = preview.replace('\n', " ");
            let ellipsis = if e.content.chars().count() > 300 { "..." } else { "" };
            format!("{ts} {sid} {}: {preview}{ellipsis}", e.event_type)
        }
    }
}

/// Compact single-line format for fzf preview panes.
fn format_compact(e: &Event) -> String {
    #[allow(non_snake_case)]
    let (DIM, RESET, CYAN, GREEN, RED, YELLOW, BOLD) =
        (cmd::dim(), cmd::reset(), cmd::cyan(), cmd::green(), cmd::red(), cmd::yellow(), cmd::bold());
    let age = cmd::relative_time(&e.timestamp);
    let meta = e.metadata();

    match e.event_type.as_str() {
        "file_diff" => {
            let file_path = meta["file_path"].as_str().unwrap_or("?");
            let fname = file_path.rsplit('/').next().unwrap_or(file_path);
            let op = meta["operation"].as_str().unwrap_or("edit");
            let added = meta["lines_added"].as_i64().unwrap_or(0);
            let removed = meta["lines_removed"].as_i64().unwrap_or(0);
            format!(
                "{DIM}{age:>8}{RESET}  {YELLOW}{op:<6}{RESET} {BOLD}{fname}{RESET} \
                 {GREEN}+{added}{RESET}/{RED}-{removed}{RESET}"
            )
        }
        "tool_use" => {
            let tool = meta["tool_name"].as_str().unwrap_or("?");
            let success = meta["success"].as_bool().unwrap_or(true);
            let dot = if success {
                format!("{GREEN}\u{2713}{RESET}")
            } else {
                format!("{RED}\u{2717}{RESET}")
            };
            let preview = tool_preview(tool, &e.content);
            format!(
                "{DIM}{age:>8}{RESET}  {dot} {CYAN}{tool:<8}{RESET} {DIM}{preview}{RESET}"
            )
        }
        "user_prompt" => {
            let preview: String = e.content.chars().take(100).collect();
            let preview = preview.replace('\n', " ");
            format!("{DIM}{age:>8}{RESET}  {BOLD}\u{25b6} {preview}{RESET}")
        }
        "plan" => {
            let preview: String = e.content.chars().take(100).collect();
            let preview = preview.replace('\n', " ");
            format!("{DIM}{age:>8}{RESET}  {YELLOW}\u{25c6}{RESET} {preview}")
        }
        _ => {
            let preview: String = e.content.chars().take(80).collect();
            let preview = preview.replace('\n', " ");
            let label = e.event_type.replace('_', " ");
            format!("{DIM}{age:>8}{RESET}  {DIM}{label}: {preview}{RESET}")
        }
    }
}

/// Extract a short preview from tool_use content.
pub fn tool_preview(tool: &str, content: &str) -> String {
    // Find the INPUT section
    let marker = "--- INPUT ---";
    let idx = match content.find(marker) {
        Some(i) => i + marker.len(),
        None => return String::new(),
    };
    let inp = &content[idx..];
    let end = inp.find("--- ");
    let inp = match end {
        Some(e) => &inp[..e],
        None => inp,
    };
    let inp = inp.trim();

    // Try to parse as JSON
    if let Ok(d) = serde_json::from_str::<serde_json::Value>(inp) {
        match tool {
            "Read" => return d["file_path"].as_str().unwrap_or("").to_string(),
            "Write" => {
                let path = d["file_path"].as_str().unwrap_or("");
                let size = d["content"].as_str().map(|s| s.len()).unwrap_or(0);
                return format!("{path} ({size} chars)");
            }
            "Edit" => return d["file_path"].as_str().unwrap_or("").to_string(),
            "Bash" => {
                let desc = d["description"].as_str().unwrap_or("");
                if !desc.is_empty() {
                    return desc.chars().take(60).collect();
                }
                let cmd = d["command"].as_str().unwrap_or("");
                return cmd.chars().take(100).collect();
            }
            "Glob" => return d["pattern"].as_str().unwrap_or("").to_string(),
            "Grep" => {
                let pattern = d["pattern"].as_str().unwrap_or("");
                let path = d["path"].as_str().unwrap_or("");
                return format!("/{pattern}/ {path}");
            }
            _ => {
                // Show first string value
                if let Some(obj) = d.as_object() {
                    for v in obj.values() {
                        if let Some(s) = v.as_str() {
                            if s.len() > 5 {
                                return s.chars().take(80).collect();
                            }
                        }
                    }
                }
            }
        }
    }

    inp.chars().take(80).collect::<String>().replace('\n', " ")
}

fn fzf_events(events: &[&Event]) -> anyhow::Result<()> {
    #[allow(non_snake_case)]
    let (DIM, RESET, CYAN) = (cmd::dim(), cmd::reset(), cmd::cyan());
    let lines: Vec<String> = events
        .iter()
        .map(|e| {
            let ts = &e.timestamp[5..16]; // MM-DD HH:MM
            let preview: String = e.content.chars().take(100).collect();
            let preview = preview.replace('\n', " ");
            format!(
                "{}\t{DIM}{ts}{RESET}  {CYAN}{:16}{RESET}  {DIM}{preview}{RESET}",
                e.id, e.event_type
            )
        })
        .collect();

    let reclaude = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "reclaude".to_string());

    let preview_cmd = format!("{reclaude} chat {{1}} --no-pager 2>/dev/null");

    if let Some(id) = fzf::select_with_id(&lines, &preview_cmd, "  events  \u{21b5} open chat  esc quit")? {
        // Open chat for the selected event
        std::process::Command::new(&reclaude)
            .args(["chat", &id])
            .status()?;
    }

    Ok(())
}
