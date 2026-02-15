use crate::cli::SearchArgs;
use crate::cmd;
use crate::db::Database;
use crate::fzf;
use crate::models::Event;

/// Full-text search over captured events.
pub async fn run(args: &SearchArgs, db: &Database) -> anyhow::Result<()> {
    // Rebuild FTS index if requested
    if args.rebuild {
        eprint!("Rebuilding FTS index... ");
        db.events.rebuild_fts_index().await?;
        eprintln!("done");
        if args.query.is_empty() {
            return Ok(());
        }
    }

    let has_query = args.query.iter().any(|t| !t.trim().is_empty());
    if !has_query {
        eprintln!("Usage: reclaude search <query>");
        return Ok(());
    }

    // Build FTS query with boolean operators.
    // Quote each term to prevent FTS5 syntax errors from special chars
    // like dots (capture.rs), slashes (src/db), etc.
    let quoted_query = args.query.iter().map(|t| fts_quote(t)).collect::<Vec<_>>().join(" ");
    let mut parts: Vec<String> = vec![quoted_query];
    for t in &args.and_terms {
        parts.push(format!("AND {}", fts_quote(t)));
    }
    for t in &args.or_terms {
        parts.push(format!("OR {}", fts_quote(t)));
    }
    for t in &args.not_terms {
        parts.push(format!("NOT {}", fts_quote(t)));
    }
    let query = parts.join(" ");

    // Resolve filters
    let session_id = if let Some(ref s) = args.session {
        let sessions = db.meta.list_sessions(None, 100)?;
        cmd::resolve_session(&sessions, Some(s))
    } else {
        None
    };

    let cwd = if args.all {
        None
    } else {
        Some(args.cwd.clone().unwrap_or_else(cmd::current_dir))
    };

    // Resolve event type filter (supports aliases like "diff" → "file_diff")
    let event_types: Vec<&str> = if let Some(ref et) = args.event_type {
        if et == "chat" {
            vec!["user_prompt", "assistant", "plan"]
        } else if let Some(config) = crate::models::resolve_type_alias(et) {
            config.semantic_types.iter().map(|t| t.as_str()).collect()
        } else {
            vec![et.as_str()]
        }
    } else {
        Vec::new()
    };

    // Semantic search path
    if args.semantic {
        let mut embedder = match crate::embed::NomicEmbedder::load()? {
            Some(e) => e,
            None => {
                eprintln!("Embedding model not downloaded. Run: reclaude embed download");
                return Ok(());
            }
        };

        let query_text = args.query.join(" ");
        let query_vec = embedder.embed_query(&query_text)?;

        let results = db.events.search_vector(
            &query_vec,
            &event_types,
            session_id.as_deref(),
            cwd.as_deref(),
            args.limit,
        ).await?;

        if results.is_empty() {
            println!("No results.");
            return Ok(());
        }

        return display_results(&results, args);
    }

    // Execute FTS search (auto-rebuilds stale index on first failure)
    let results = db
        .events
        .search_fts(
            &query,
            &event_types,
            session_id.as_deref(),
            cwd.as_deref(),
            args.limit,
        )
        .await?;

    if results.is_empty() {
        println!("No results.");
        return Ok(());
    }

    display_results(&results, args)
}

fn display_results(results: &[Event], args: &SearchArgs) -> anyhow::Result<()> {
    #[allow(non_snake_case)]
    let (DIM, RESET, CYAN, PURPLE) = (cmd::dim(), cmd::reset(), cmd::cyan(), cmd::purple());

    // JSON output
    if args.json {
        let payload: Vec<serde_json::Value> = results
            .iter()
            .map(|e| {
                let content = if args.verbose {
                    e.content.clone()
                } else {
                    e.content.chars().take(500).collect()
                };
                serde_json::json!({
                    "id": e.id,
                    "timestamp": e.timestamp,
                    "event_type": e.event_type,
                    "session_id": e.session_id,
                    "content": content,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    // FZF mode
    if args.fzf {
        return fzf_results(results);
    }

    // Human-readable output
    let full_sid = args.full_sid;

    for e in results {
        let ts = if e.timestamp.len() >= 16 {
            e.timestamp[5..16].replace('T', " ") // "02-15 12:34"
        } else {
            e.timestamp.clone()
        };
        let sid = cmd::format_sid(e.session_id.as_deref(), full_sid);
        let meta = e.metadata();

        let (icon, etype) = if e.event_type == "tool_use" {
            let tool = meta["tool_name"].as_str().unwrap_or("tool_use");
            (tool_icon(tool), tool.to_string())
        } else {
            (type_icon(&e.event_type), e.event_type.clone())
        };

        let preview = clean_content(e, &meta, args.verbose);

        println!(
            "{DIM}{ts}{RESET} {PURPLE}{sid}{RESET} {CYAN}{icon} {etype}{RESET} {preview}"
        );
    }

    let mode = if args.semantic { "semantic" } else { "FTS" };
    println!("{} results ({mode})", results.len());

    Ok(())
}

fn clean_content(e: &Event, meta: &serde_json::Value, verbose: bool) -> String {
    match e.event_type.as_str() {
        "tool_use" => {
            let tool = meta["tool_name"].as_str().unwrap_or("?");
            let ok = meta["success"].as_bool().unwrap_or(true);
            let status = if ok { "\u{2713}" } else { "\u{2717}" };
            let summary = crate::cmd::events::tool_preview(tool, &e.content);
            format!("{status} {summary}")
        }
        "file_diff" => {
            let path = meta["file_path"].as_str().unwrap_or("");
            let added = meta["lines_added"].as_i64().unwrap_or(0);
            let removed = meta["lines_removed"].as_i64().unwrap_or(0);
            format!("{path}  +{added}/-{removed}")
        }
        _ => {
            if verbose {
                e.content.clone()
            } else {
                let preview: String = e.content.replace('\n', " ").chars().take(300).collect();
                preview
            }
        }
    }
}

/// Quote a search term for FTS5 MATCH to avoid syntax errors from
/// special characters (dots, slashes, hyphens, etc.).
/// Preserves trailing `*` for FTS5 prefix queries (e.g., `auth*`).
fn fts_quote(term: &str) -> String {
    let (body, suffix) = if let Some(stripped) = term.strip_suffix('*') {
        (stripped, "*")
    } else {
        (term, "")
    };
    if body.contains(|c: char| !c.is_alphanumeric() && c != '_') {
        format!("\"{}\"{suffix}", body.replace('"', "\"\""))
    } else {
        format!("{body}{suffix}")
    }
}

fn tool_icon(tool: &str) -> &'static str {
    match tool {
        "Bash" => "\u{03bb}",
        "Edit" => "\u{2202}",
        "Write" => "\u{270e}",
        "Read" => "\u{25c9}",
        "Glob" => "\u{229b}",
        "Grep" => "/",
        "Task" | "TaskCreate" | "TaskUpdate" => "\u{25c6}",
        "WebFetch" => "\u{21e3}",
        "WebSearch" => "\u{2295}",
        _ => "\u{00b7}",
    }
}

fn type_icon(event_type: &str) -> &'static str {
    match event_type {
        "assistant" => "\u{25b8}",
        "user_prompt" => "\u{25b9}",
        "plan" | "plan_file" => "\u{25c8}",
        "file_diff" => "\u{00b1}",
        "thinking" => "\u{2026}",
        "compaction" => "\u{2298}",
        "session_start" => "\u{21b3}",
        "session_end" => "\u{21b2}",
        _ => "\u{00b7}",
    }
}

fn fzf_results(results: &[Event]) -> anyhow::Result<()> {
    #[allow(non_snake_case)]
    let (DIM, RESET, CYAN) = (cmd::dim(), cmd::reset(), cmd::cyan());
    let lines: Vec<String> = results
        .iter()
        .map(|e| {
            let ts = if e.timestamp.len() >= 16 {
                &e.timestamp[5..16]
            } else {
                &e.timestamp
            };
            let preview: String = e.content.replace('\n', " ").chars().take(120).collect();
            format!(
                "{}\t{DIM}{ts}{RESET}  {CYAN}{:12}{RESET}  {DIM}{preview}{RESET}",
                e.id, e.event_type
            )
        })
        .collect();

    let reclaude = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "reclaude".to_string());

    let preview_cmd = format!("{reclaude} chat {{1}} --no-pager 2>/dev/null");

    if let Some(id) = fzf::select_with_id(&lines, &preview_cmd, "  search results  \u{21b5} open chat  esc quit")? {
        std::process::Command::new(&reclaude)
            .args(["chat", &id])
            .status()?;
    }

    Ok(())
}
