pub mod chat;
pub mod embed;
pub mod events;
pub mod files;
pub mod focus;
pub mod search;
pub mod sessions;
pub mod status;
pub mod transcripts;

use chrono::{DateTime, Utc};

// ── ANSI Colors ──────────────────────────────────────────────────────

pub const DIM: &str = "\x1b[2m";
pub const RESET: &str = "\x1b[0m";
pub const CYAN: &str = "\x1b[36m";
pub const PURPLE: &str = "\x1b[35m";
pub const YELLOW: &str = "\x1b[33m";
pub const GREEN: &str = "\x1b[32m";
pub const RED: &str = "\x1b[31m";
pub const BOLD: &str = "\x1b[1m";

// ── Shared Formatting Utilities ──────────────────────────────────────

/// Format session ID - first segment before first dash, or full if requested.
pub fn format_sid(session_id: Option<&str>, full: bool) -> String {
    let sid = session_id.unwrap_or("").trim();
    if sid.is_empty() {
        return "--------".to_string();
    }
    if full {
        sid.to_string()
    } else {
        sid.split('-').next().unwrap_or(sid).to_string()
    }
}

/// Short 8-char session ID.
pub fn short_sid(session_id: Option<&str>) -> String {
    let sid = session_id.unwrap_or("").trim();
    if sid.is_empty() {
        "--------".to_string()
    } else {
        sid.chars().take(8).collect()
    }
}

/// Human-readable relative time (e.g., "5m ago", "2h ago").
pub fn relative_time(timestamp: &str) -> String {
    let dt = match DateTime::parse_from_rfc3339(timestamp) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => return timestamp.to_string(),
    };

    let now = Utc::now();
    let delta = now.signed_duration_since(dt);
    let secs = delta.num_seconds();

    if secs < 0 {
        return "now".to_string();
    }
    if secs < 60 {
        return format!("{secs}s ago");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    format!("{days}d ago")
}

/// Replace $HOME with ~ for display.
pub fn short_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = path.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

/// Extract project name from a path (last component).
pub fn project_name(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Get current working directory as string.
pub fn current_dir() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Resolve a session ID prefix to full session ID.
pub fn resolve_session(
    sessions: &[crate::models::Session],
    value: Option<&str>,
) -> Option<String> {
    if sessions.is_empty() {
        return None;
    }

    match value {
        None | Some("latest") | Some("@latest") => {
            Some(sessions[0].session_id.clone())
        }
        Some(needle) => {
            let needle = needle.trim();
            let matches: Vec<&crate::models::Session> = sessions
                .iter()
                .filter(|s| s.session_id == needle || s.session_id.starts_with(needle))
                .collect();

            match matches.len() {
                1 => Some(matches[0].session_id.clone()),
                0 => {
                    eprintln!("No session matches: {needle:?}");
                    eprintln!("Tip: run `reclaude sessions` to list session ids.");
                    None
                }
                _ => {
                    eprintln!("Ambiguous session prefix {needle:?} matches:");
                    for s in matches.iter().take(10) {
                        eprintln!("  {}", s.session_id);
                    }
                    None
                }
            }
        }
    }
}
