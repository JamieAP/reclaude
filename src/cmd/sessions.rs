use crate::cli::SessionsArgs;
use crate::cmd;
use crate::db::Database;
use crate::fzf;

/// List recent sessions with age, event count, and project info.
pub async fn run(args: &SessionsArgs, db: &Database) -> anyhow::Result<()> {
    let cwd = if args.all { None } else { Some(cmd::current_dir()) };
    let sessions = db.list_sessions(cwd.as_deref(), args.limit).await?;

    if sessions.is_empty() {
        eprintln!("No sessions found");
        return Ok(());
    }

    if args.fzf {
        return fzf_select(&sessions);
    }

    if args.json {
        let payload: Vec<serde_json::Value> = sessions
            .iter()
            .map(|s| {
                serde_json::json!({
                    "session_id": s.session_id,
                    "started_at": s.started_at,
                    "ended_at": s.ended_at,
                    "cwd": s.cwd,
                    "repo_name": s.repo_name,
                    "event_count": s.event_count,
                    "is_active": s.is_active,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    for s in &sessions {
        let display_id = cmd::format_sid(Some(&s.session_id), args.full_sid);
        let age = cmd::relative_time(&s.started_at);
        let project = s.cwd.as_deref().map(cmd::project_name).unwrap_or_default();
        let dir = s.cwd.as_deref().map(cmd::short_path).unwrap_or_default();
        let active = if s.is_active { " *" } else { "" };

        println!(
            "{age} {display_id} {}ev{active} {project} {dir}",
            s.event_count
        );
    }

    Ok(())
}

fn fzf_select(sessions: &[crate::models::Session]) -> anyhow::Result<()> {
    #[allow(non_snake_case)]
    let (DIM, RESET, CYAN, BOLD, GREEN) =
        (cmd::dim(), cmd::reset(), cmd::cyan(), cmd::bold(), cmd::green());
    let mut lines = Vec::new();

    for s in sessions {
        let short_id = s.session_id.split('-').next().unwrap_or(&s.session_id);
        let age = cmd::relative_time(&s.started_at);
        let project = s.cwd.as_deref().map(cmd::project_name).unwrap_or_default();
        let dot = if s.is_active {
            format!("{GREEN}\u{25cf}{RESET}")
        } else {
            format!("{DIM}\u{25cb}{RESET}")
        };

        lines.push(format!(
            "{DIM}{age:>8}{RESET}  {CYAN}{short_id}{RESET}  {dot} {BOLD}{project:<18}{RESET} {DIM}{:>5}{RESET}",
            s.event_count
        ));
    }

    let reclaude = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "reclaude".to_string());

    let preview_cmd = format!(
        "SID=$(echo {{}} | sed 's/\\x1b\\[[0-9;]*m//g' | awk '{{print $3}}'); \
         {reclaude} events 20 --compact --all --session $SID"
    );

    let selected = fzf::run_fzf(
        &lines.join("\n"),
        &preview_cmd,
        "  sessions",
    )?;

    if let Some(line) = selected {
        // Strip ANSI and parse short ID
        let clean = fzf::strip_ansi(&line);
        let parts: Vec<&str> = clean.split_whitespace().collect();
        if parts.len() >= 3 {
            let short_id = parts[2];
            // Find matching session
            for s in sessions {
                if s.session_id.starts_with(short_id) {
                    let cwd = s.cwd.as_deref().unwrap_or("");
                    if cwd.is_empty() {
                        println!("cb --resume {}", s.session_id);
                    } else {
                        println!("cd {} && cb --resume {}", cwd, s.session_id);
                    }
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}
