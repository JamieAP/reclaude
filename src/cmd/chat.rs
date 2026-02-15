use std::process::Command;

use crate::cli::ChatArgs;
use crate::cmd;
use crate::db::Database;
use crate::models::Event;

/// Chat event types shown in conversation view.
const CHAT_TYPES: &[&str] = &[
    "user_prompt",
    "assistant",
    "plan",
    "thinking",
    "tool_use",
    "file_diff",
    "compaction",
    "session_start",
    "session_end",
    "permission_request",
    "notification",
];

/// View conversation around events, rendered as a chat timeline.
pub async fn run(args: &ChatArgs, db: &Database) -> anyhow::Result<()> {
    let show_all = args.all;
    let effective_limit = if args.limit > 0 { args.limit } else { 50_000 };

    let events = if show_all {
        // All chat events across every session and repo
        db.events.query(CHAT_TYPES, None, None, effective_limit).await?
    } else if let Some(ref session_arg) = args.session {
        let sessions = db.meta.list_sessions(None, 100)?;
        let session_id = cmd::resolve_session(&sessions, Some(session_arg));
        match session_id {
            Some(sid) => {
                db.events
                    .query(CHAT_TYPES, Some(&sid), None, effective_limit)
                    .await?
            }
            None => return Ok(()),
        }
    } else if let Some(event_id) = args.event_id {
        // Find the event, then get its session
        let target = db.events.get_by_id(event_id).await?;
        match target {
            Some(e) => {
                if let Some(ref sid) = e.session_id {
                    db.events
                        .query(CHAT_TYPES, Some(sid), None, effective_limit)
                        .await?
                } else {
                    eprintln!("Event {event_id} has no session_id");
                    return Ok(());
                }
            }
            None => {
                eprintln!("No event with id {event_id}");
                return Ok(());
            }
        }
    } else {
        // Default: all sessions in current cwd
        let cwd = cmd::current_dir();
        db.events.query(CHAT_TYPES, None, Some(&cwd), effective_limit).await?
    };

    if events.is_empty() {
        eprintln!("No events found");
        return Ok(());
    }

    // Reverse for chronological display
    let events: Vec<&Event> = events.iter().rev().collect();

    #[allow(non_snake_case)]
    let (DIM, RESET) = (cmd::dim(), cmd::reset());

    // Render events
    let mut output_lines: Vec<String> = Vec::new();
    let mut target_line: usize = 0;
    let mut prev_session: Option<&str> = None;
    let full_sid = args.full_sid;

    for e in &events {
        // Insert session separators in multi-session views
        let e_sid = e.session_id.as_deref();
        if (show_all || args.session.is_none()) && e_sid != prev_session {
            let project = e.cwd.as_deref().map(cmd::project_name).unwrap_or_default();
            let ts = &e.timestamp[..16]; // YYYY-MM-DD HH:MM
            let display_sid = cmd::format_sid(e_sid, full_sid);

            if prev_session.is_some() {
                output_lines.push(String::new());
            }
            output_lines.push(format!("  {DIM}{}", "\u{2500}".repeat(60)));
            output_lines.push(format!(
                "  \u{21b3} {} \u{00b7} {} \u{00b7} {}",
                if project.is_empty() { "?" } else { &project },
                ts,
                display_sid
            ));
            output_lines.push(format!("  {}{RESET}", "\u{2500}".repeat(60)));
            output_lines.push(String::new());
            prev_session = e_sid;
        }

        let is_target = args.event_id.map_or(false, |id| e.id == id);
        if is_target {
            target_line = output_lines.len() + 1;
        }

        let rendered = render_event(e, is_target);
        output_lines.extend(rendered);
    }

    let text = output_lines.join("\n") + "\n";

    // Output
    if args.no_pager || !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        print!("{text}");
        return Ok(());
    }

    // Pager: write to temp file, open less at target line
    let tmp = std::env::temp_dir().join(format!("reclaude-chat-{}.ans", std::process::id()));
    std::fs::write(&tmp, &text)?;

    let mut less_args = vec!["-R".to_string()];
    if target_line > 0 {
        less_args.push(format!("+{target_line}g"));
    } else {
        less_args.push("+G".to_string()); // start at bottom
    }
    less_args.push(tmp.to_string_lossy().to_string());

    let _ = Command::new("less").args(&less_args).status();
    let _ = std::fs::remove_file(&tmp);

    Ok(())
}

/// Render a single event as chat lines.
fn render_event(e: &Event, is_target: bool) -> Vec<String> {
    #[allow(non_snake_case)]
    let (DIM, RESET, CYAN, GREEN, YELLOW, BOLD) =
        (cmd::dim(), cmd::reset(), cmd::cyan(), cmd::green(), cmd::yellow(), cmd::bold());
    let ts = if e.timestamp.len() >= 16 {
        &e.timestamp[11..16] // HH:MM
    } else {
        &e.timestamp
    };
    let meta = e.metadata();
    let highlight = if is_target {
        format!("{YELLOW}{BOLD}")
    } else {
        String::new()
    };
    let end_hl = if is_target { RESET } else { "" };
    let arrow = if is_target { "\u{2192} " } else { "  " };

    let mut lines: Vec<String> = Vec::new();

    match e.event_type.as_str() {
        "user_prompt" => {
            let content = e.content.trim();
            // Skip system noise
            if content.starts_with("<task-notification") || content.starts_with("<system-reminder>") {
                return lines;
            }
            lines.push(format!(
                "{arrow}{highlight}{GREEN}\u{25b9} You {DIM}({ts}){RESET}{end_hl}"
            ));
            lines.push(indent(content));
            lines.push(String::new());
        }
        "assistant" | "plan" => {
            let label = if e.event_type == "assistant" {
                "Claude"
            } else {
                "Claude [plan]"
            };
            lines.push(format!(
                "{arrow}{highlight}{CYAN}\u{25b8} {label} {DIM}({ts}){RESET}{end_hl}"
            ));
            lines.push(indent(e.content.trim()));
            lines.push(String::new());
        }
        "thinking" => {
            let n = e.content.len();
            lines.push(format!("  {DIM}\u{2026} [thinking, {n} chars]{RESET}"));
        }
        "tool_use" => {
            let tool = meta["tool_name"].as_str().unwrap_or("?");
            let ok = meta["success"].as_bool().unwrap_or(true);
            let status = if ok {
                format!("{GREEN}\u{2713}{RESET}")
            } else {
                format!("{}\u{2717}{RESET}", cmd::red())
            };
            let icon = tool_icon(tool);
            let summary = crate::cmd::events::tool_preview(tool, &e.content);
            lines.push(format!(
                "  {status} {CYAN}{icon} {tool}{RESET} {DIM}{summary}{RESET}"
            ));
        }
        "file_diff" => {
            let file_path = meta["file_path"].as_str().unwrap_or("");
            let added = meta["lines_added"].as_i64().unwrap_or(0);
            let removed = meta["lines_removed"].as_i64().unwrap_or(0);
            lines.push(format!(
                "  {DIM}\u{00b1} {file_path} +{added}/-{removed}{RESET}"
            ));
        }
        "compaction" => {
            lines.push(format!("  {DIM}\u{2298} [context compacted]{RESET}"));
        }
        "session_start" => {
            let project = e.cwd.as_deref().map(cmd::project_name).unwrap_or_default();
            lines.push(format!("\n  {DIM}{}", "\u{2500}".repeat(60)));
            lines.push(format!("  \u{21b3} session start {ts} {project}"));
            lines.push(format!("  {}{RESET}\n", "\u{2500}".repeat(60)));
        }
        "session_end" => {
            lines.push(format!("\n  {DIM}\u{21b2} session end ({ts}){RESET}\n"));
        }
        "permission_request" | "notification" | "sys_msg" => {
            let preview: String = e.content.replace('\n', " ").chars().take(80).collect();
            lines.push(format!("  {DIM}\u{00b7} {}: {preview}{RESET}", e.event_type));
        }
        _ => {
            let preview: String = e.content.replace('\n', " ").chars().take(60).collect();
            lines.push(format!("  {DIM}\u{00b7} {}: {preview}{RESET}", e.event_type));
        }
    }

    lines
}

fn indent(text: &str) -> String {
    text.lines().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n")
}

fn tool_icon(tool: &str) -> &'static str {
    match tool {
        "Bash" => "\u{03bb}",       // λ
        "Edit" => "\u{2202}",       // ∂
        "Write" => "\u{270e}",      // ✎
        "Read" => "\u{25c9}",       // ◉
        "Glob" => "\u{229b}",       // ⊛
        "Grep" => "/",
        "Task" | "TaskCreate" | "TaskUpdate" | "TaskGet" | "TaskList" => "\u{25c6}", // ◆
        "WebFetch" => "\u{21e3}",   // ⇣
        "WebSearch" => "\u{2295}",  // ⊕
        _ => "\u{00b7}",            // ·
    }
}
