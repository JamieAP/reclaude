use std::collections::{HashMap, HashSet};

use crate::cli::FilesArgs;
use crate::cmd::{self, DIM, RESET};
use crate::db::Database;

/// Tools that touch files (all have file_path parameter in their input).
const FILE_TOOLS: &[&str] = &["Read", "Write", "Edit", "NotebookEdit"];

/// Find files touched by Claude, sorted by last touch time.
pub async fn run(args: &FilesArgs, db: &Database) -> anyhow::Result<()> {
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
        Some(cmd::current_dir())
    };

    // Query tool_use events
    let events = db
        .events
        .query(
            &["tool_use"],
            session_id.as_deref(),
            cwd.as_deref(),
            args.scan_limit,
        )
        .await?;

    // Aggregate file touches
    let mut file_touches: HashMap<String, FileTouch> = HashMap::new();

    for event in &events {
        let tool = event.tool_name.as_deref().unwrap_or("");
        if !FILE_TOOLS.contains(&tool) {
            continue;
        }

        let file_path = match extract_file_path(&event.content) {
            Some(p) => p,
            None => continue,
        };

        // Apply pattern filter
        if let Some(ref pattern) = args.pattern {
            if !file_path.contains(pattern.as_str()) {
                continue;
            }
        }

        let touch = file_touches.entry(file_path).or_insert_with(|| FileTouch {
            last_ts: event.timestamp.clone(),
            sessions: HashSet::new(),
            tools: HashSet::new(),
            count: 0,
        });

        // Events come in DESC order, so first seen is most recent
        if let Some(ref sid) = event.session_id {
            touch.sessions.insert(sid.clone());
        }
        touch.tools.insert(tool.to_string());
        touch.count += 1;
    }

    if file_touches.is_empty() {
        if args.pattern.is_some() {
            eprintln!("No files matching pattern found");
        } else {
            eprintln!("No file touches found");
        }
        return Ok(());
    }

    // Sort by last touch time (most recent first)
    let mut sorted: Vec<(String, FileTouch)> = file_touches.into_iter().collect();
    sorted.sort_by(|a, b| b.1.last_ts.cmp(&a.1.last_ts));
    sorted.truncate(args.limit);

    // Output
    if args.json {
        let payload: Vec<serde_json::Value> = sorted
            .iter()
            .map(|(path, info)| {
                let mut tools: Vec<&str> = info.tools.iter().map(|s| s.as_str()).collect();
                tools.sort();
                serde_json::json!({
                    "path": path,
                    "last_touched": info.last_ts,
                    "sessions": info.sessions.len(),
                    "tools": tools,
                    "count": info.count,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        for (path, info) in &sorted {
            let age = cmd::relative_time(&info.last_ts);
            if args.full {
                let mut tools: Vec<&str> = info.tools.iter().map(|s| s.as_str()).collect();
                tools.sort();
                println!("{path}");
                println!(
                    "  {} | {} touches | {} sessions | {}",
                    age,
                    info.count,
                    info.sessions.len(),
                    tools.join(",")
                );
            } else {
                println!("{path}  {DIM}({age}, {}x){RESET}", info.count);
            }
        }
    }

    Ok(())
}

struct FileTouch {
    last_ts: String,
    sessions: HashSet<String>,
    tools: HashSet<String>,
    count: usize,
}

/// Extract file_path from tool_use event content.
///
/// Content format:
///   Tool: {name}
///   Success: {bool}
///
///   --- INPUT ---
///   {json}
///
///   --- OUTPUT ---
///   ...
fn extract_file_path(content: &str) -> Option<String> {
    let marker = "--- INPUT ---";
    let idx = content.find(marker)? + marker.len();
    let inp = &content[idx..];
    let end = inp.find("--- ");
    let inp = match end {
        Some(e) => &inp[..e],
        None => inp,
    };
    let inp = inp.trim();

    let d: serde_json::Value = serde_json::from_str(inp).ok()?;
    d["file_path"].as_str().map(|s| s.to_string())
}
