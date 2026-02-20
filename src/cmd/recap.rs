use crate::cli::RecapArgs;
use crate::db::sessions::RecapOptions;
use crate::db::Database;

use crate::cmd;

pub async fn run(args: &RecapArgs, db: &Database) -> anyhow::Result<()> {
    let zoomed = !args.overview;
    let zoom = if zoomed { Some(args.zoom.clone()) } else { None };
    let opts = RecapOptions {
        min_events: args.min_events,
        zoom,
        file_limit: if zoomed { 0 } else { 3 },
        dir_limit: if zoomed { 10 } else { 0 },
        session_limit: if zoomed && !args.compact { 15 } else { 0 },
        prompt_limit: if zoomed && !args.compact { 5 } else { 0 },
    };

    let rows = db.query_recap(&opts).await?;

    if rows.is_empty() {
        println!("No activity in the last 7 days.");
        return Ok(());
    }

    #[allow(non_snake_case)]
    let (DIM, RST, CYAN, GRN, YLW, PUR, BOLD) = (
        cmd::dim(), cmd::reset(), cmd::cyan(), cmd::green(),
        cmd::yellow(), cmd::purple(), cmd::bold(),
    );

    let mut current_bucket = String::new();

    for row in &rows {
        if row.bucket != current_bucket {
            if !current_bucket.is_empty() {
                println!();
            }
            let label = match row.bucket.as_str() {
                "2h" => "Last 2 hours",
                "24h" => "Last 24 hours",
                "7d" => "Last 7 days",
                _ => &row.bucket,
            };
            println!("{BOLD}{label}{RST}");
            current_bucket = row.bucket.clone();
        }

        // Stat chips
        let diff_info = if row.diffs > 0 {
            format!("{YLW}{}{RST} diffs", row.diffs)
        } else {
            String::new()
        };
        let prompt_info = if row.prompts > 0 {
            format!("{PUR}{}{RST} prompts", row.prompts)
        } else {
            String::new()
        };

        let stats: Vec<&str> = [diff_info.as_str(), prompt_info.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        let stats_str = if stats.is_empty() {
            String::new()
        } else {
            format!("  {}", stats.join(", "))
        };

        let repo_display = short_repo(&row.repo);

        if zoomed {
            // Zoomed: header-style repo line
            println!(
                "\n  {CYAN}{}{RST}  ({} sessions, {GRN}{}{RST} events{stats_str})",
                repo_display, row.sessions, row.events,
            );

            // Directory stats
            if !row.top_dirs.is_empty() && !args.compact {
                for d in &row.top_dirs {
                    let dir_display = short_dir(&d.dir);
                    let file_word = if d.unique_files == 1 { "file" } else { "files" };
                    println!(
                        "    {DIM}{:<30}{RST} {:>2} {file_word}  {:>3} edits",
                        dir_display, d.unique_files, d.total_edits,
                    );
                }
            }

            // Session tree
            if !row.session_info.is_empty() && !args.compact {
                println!();
                for si in &row.session_info {
                    let sid = recap_sid(&si.session_id);
                    let desc = truncate(&si.description, 80);
                    let child_tag = if si.child_count > 0 {
                        format!("  {DIM}({} subagents){RST}", si.child_count)
                    } else {
                        String::new()
                    };

                    if desc.is_empty() {
                        println!("    {CYAN}{sid}{RST}  ({GRN}{}{RST} events){child_tag}", si.event_count);
                    } else {
                        println!("    {CYAN}{sid}{RST}  {DIM}\"{desc}\"{RST}{child_tag}");
                    }

                    // Spawn descriptions
                    for (i, spawn) in si.spawns.iter().enumerate() {
                        let prefix = if i + 1 < si.spawns.len() { "├─" } else { "└─" };
                        let sdesc = truncate(spawn, 80);
                        println!("      {DIM}{prefix} {sdesc}{RST}");
                    }
                }
            }

            // User prompts
            if !row.top_prompts.is_empty() {
                println!();
                for prompt in &row.top_prompts {
                    let truncated = truncate(prompt, 100);
                    println!("    {PUR}>{RST} {DIM}\"{truncated}\"{RST}");
                }
            }
        } else {
            // Overview: compact one-liner per repo
            println!(
                "  {CYAN}{:<24}{RST} {GRN}{:>5}{RST} events  {:>2} sessions{stats_str}",
                repo_display, row.events, row.sessions,
            );

            // Top file paths (topic signal)
            if !row.top_files.is_empty() && !args.compact {
                let files: Vec<String> = row.top_files.iter().map(|f| short_file(f)).collect();
                println!("  {DIM}{:<24} {}{RST}", "", files.join(", "));
            }
        }
    }

    println!();
    Ok(())
}

/// Shorten repo name: /home/user -> ~, /home/user/Downloads -> ~/Downloads
fn short_repo(repo: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if repo == home {
            return "~".to_string();
        }
        if let Some(rest) = repo.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    repo.to_string()
}

/// Shorten a file path to just the meaningful tail.
/// /home/user/dev/reclaude/src/db/events.rs -> src/db/events.rs
fn short_file(path: &str) -> String {
    if let Some(idx) = path.find("/dev/") {
        let after_dev = &path[idx + 5..];
        if let Some(slash) = after_dev.find('/') {
            return after_dev[slash + 1..].to_string();
        }
    }
    // Fallback: last 3 components
    let parts: Vec<&str> = path.rsplitn(4, '/').collect();
    if parts.len() >= 3 {
        format!("{}/{}/{}", parts[2], parts[1], parts[0])
    } else {
        path.to_string()
    }
}

/// Shorten a directory path to repo-relative.
/// /home/user/dev/reclaude/src/cmd/ -> src/cmd/
fn short_dir(path: &str) -> String {
    if let Some(idx) = path.find("/dev/") {
        let after_dev = &path[idx + 5..];
        if let Some(slash) = after_dev.find('/') {
            let result = &after_dev[slash + 1..];
            if !result.is_empty() {
                return result.to_string();
            }
            // Project root
            return format!("{}/", &after_dev[..slash]);
        }
    }
    // Try $HOME -> ~
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = path.strip_prefix(&home) {
            let rest = rest.trim_start_matches('/');
            if rest.is_empty() {
                return "~/".to_string();
            }
            return format!("~/{rest}");
        }
    }
    // Fallback: last 3 components
    let trimmed = path.trim_end_matches('/');
    let parts: Vec<&str> = trimmed.rsplitn(4, '/').collect();
    if parts.len() >= 3 {
        format!("{}/{}/{}/", parts[2], parts[1], parts[0])
    } else {
        path.to_string()
    }
}

/// Session ID for recap: full for short agent IDs, truncated for UUIDs.
fn recap_sid(session_id: &str) -> String {
    if session_id.starts_with("agent-") {
        session_id.to_string()
    } else {
        session_id.chars().take(8).collect()
    }
}

/// Truncate string at char boundary.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}
