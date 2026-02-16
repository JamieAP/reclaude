use std::path::{Path, PathBuf};

use tracing::debug;

use crate::cli::{TranscriptsCommand, TranscriptsExtractArgs, TranscriptsListArgs};
use crate::db::Database;

const CLAUDE_PROJECTS_DIR: &str = ".claude/projects";
const ZSTD_COMPRESSION_LEVEL: i32 = 19;

/// Dispatch transcript subcommands.
pub async fn run(command: &TranscriptsCommand, db: &Database) -> anyhow::Result<()> {
    match command {
        TranscriptsCommand::Sync => cmd_sync(db).await,
        TranscriptsCommand::List(args) => cmd_list(args, db).await,
        TranscriptsCommand::Show { session_id } => cmd_show(session_id, db).await,
        TranscriptsCommand::Extract(args) => cmd_extract(args, db).await,
        TranscriptsCommand::Stats => cmd_stats(db).await,
    }
}

/// Discovered transcript file info.
struct TranscriptInfo {
    path: PathBuf,
    session_id: String,
    parent_session_id: Option<String>,
    size_bytes: i64,
}

/// Scan and archive all transcripts.
async fn cmd_sync(db: &Database) -> anyhow::Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let projects_dir = PathBuf::from(&home).join(CLAUDE_PROJECTS_DIR);

    eprintln!("Scanning {}...", projects_dir.display());
    let transcripts = discover_transcripts(&projects_dir)?;

    if transcripts.is_empty() {
        println!("No transcripts found");
        return Ok(());
    }

    let main_count = transcripts.iter().filter(|t| t.parent_session_id.is_none()).count();
    let sub_count = transcripts.len() - main_count;
    eprintln!(
        "Found {} transcripts ({} main, {} subagents)",
        transcripts.len(),
        main_count,
        sub_count
    );

    // Get existing archived sessions for change detection
    let archived = db.get_archived_session_ids().await?;
    let archived_lookup: std::collections::HashMap<&str, i64> =
        archived.iter().map(|(sid, size)| (sid.as_str(), *size)).collect();

    let mut new_count = 0usize;
    let mut changed_count = 0usize;
    let mut skipped_count = 0usize;
    let mut total_compressed = 0usize;

    for t in &transcripts {
        if let Some(&existing_size) = archived_lookup.get(t.session_id.as_str()) {
            if existing_size == t.size_bytes {
                skipped_count += 1;
                continue;
            }
        }

        // Compress and archive
        let content = std::fs::read(&t.path)?;
        let compressed = zstd::encode_all(content.as_slice(), ZSTD_COMPRESSION_LEVEL)?;

        // Extract metadata from first JSONL line
        let metadata = extract_metadata(&t.path);

        let is_new = db.upsert_transcript(
            &t.session_id,
            &compressed,
            t.size_bytes,
            compressed.len() as i64,
            Some(&t.path.to_string_lossy()),
            t.parent_session_id.as_deref(),
            metadata.as_deref(),
        ).await?;

        if is_new {
            new_count += 1;
        } else {
            changed_count += 1;
        }
        total_compressed += compressed.len();
    }

    println!("  New: {new_count}");
    println!("  Updated: {changed_count}");
    println!("  Skipped: {skipped_count}");

    if new_count + changed_count > 0 {
        println!("Archived: {} compressed", format_size(total_compressed as i64));
    }

    Ok(())
}

/// List archived transcripts.
async fn cmd_list(args: &TranscriptsListArgs, db: &Database) -> anyhow::Result<()> {
    let transcripts = db.list_transcripts(args.subagents, args.limit).await?;

    if transcripts.is_empty() {
        println!("No archived transcripts");
        return Ok(());
    }

    for t in &transcripts {
        let ts = &t.archived_at[..16];
        let size = format_size(t.size_bytes);
        let compressed = t
            .compressed_bytes
            .map(|b| format_size(b))
            .unwrap_or_else(|| "?".to_string());
        let parent = t
            .parent_session_id
            .as_ref()
            .map(|p| format!(" (sub of {})", &p[..8.min(p.len())]))
            .unwrap_or_default();
        let short_id: String = t.session_id.chars().take(12).collect();

        println!("[{ts}] {short_id}... {size} -> {compressed}{parent}");
    }

    Ok(())
}

/// Show decompressed transcript content.
async fn cmd_show(session_id: &str, db: &Database) -> anyhow::Result<()> {
    // Find by prefix match
    let transcripts = db.list_transcripts(true, 1000).await?;
    let matches: Vec<_> = transcripts
        .iter()
        .filter(|t| t.session_id.starts_with(session_id))
        .collect();

    if matches.is_empty() {
        eprintln!("No transcript found matching: {session_id}");
        return Ok(());
    }
    if matches.len() > 1 {
        eprintln!("Multiple matches for '{session_id}':");
        for t in matches.iter().take(5) {
            eprintln!("  {}", t.session_id);
        }
        return Ok(());
    }

    let sid = &matches[0].session_id;
    let compressed = db.get_transcript_content(sid).await?;
    match compressed {
        Some(data) => {
            let decompressed = zstd::decode_all(data.as_slice())?;
            let text = String::from_utf8_lossy(&decompressed);

            // Pretty print JSONL
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    println!("{}", serde_json::to_string_pretty(&obj)?);
                    println!();
                } else {
                    println!("{trimmed}");
                }
            }
        }
        None => {
            eprintln!("No content found for: {sid}");
        }
    }

    Ok(())
}

/// Extract semantic events from transcripts.
async fn cmd_extract(args: &TranscriptsExtractArgs, db: &Database) -> anyhow::Result<()> {
    // Single file extraction
    if let Some(ref file_path) = args.file {
        let path = std::path::PathBuf::from(file_path);
        if !path.exists() {
            anyhow::bail!("File not found: {file_path}");
        }
        if args.dry_run {
            let size = std::fs::metadata(&path)?.len();
            println!("Would extract from: {file_path} ({} bytes)", size);
            return Ok(());
        }
        eprintln!("Extracting from: {file_path}");
        let result = crate::extract::extract_file(&path, args.force, db).await?;
        println!("  Scanned: {} entries", result.entries_scanned);
        println!("  Created: {} events", result.events_created);
        if result.events_created > 0 {
            db.compact().await?;
        }
        if !result.errors.is_empty() {
            for err in &result.errors {
                eprintln!("  Error: {err}");
            }
        }
        return Ok(());
    }

    // Batch extraction from archived transcripts
    let transcripts = db.list_transcripts(true, 10000).await?;
    if transcripts.is_empty() {
        println!("No archived transcripts. Run: reclaude transcripts sync");
        return Ok(());
    }

    let cutoff = if let Some(hours) = args.since {
        let cutoff_time = chrono::Utc::now() - chrono::Duration::hours(hours as i64);
        Some(cutoff_time.to_rfc3339())
    } else {
        None
    };

    let to_process: Vec<&crate::models::SessionTranscript> = transcripts.iter()
        .filter(|t| {
            if let Some(ref cutoff) = cutoff {
                t.archived_at.as_str() >= cutoff.as_str()
            } else {
                true
            }
        })
        .collect();

    if args.dry_run {
        println!("Would extract from {} transcripts", to_process.len());
        for t in to_process.iter().take(10) {
            let short: String = t.session_id.chars().take(12).collect();
            println!("  {short}... ({} bytes)", t.size_bytes);
        }
        if to_process.len() > 10 {
            println!("  ... and {} more", to_process.len() - 10);
        }
        return Ok(());
    }

    eprintln!("Extracting from {} transcripts...", to_process.len());
    let mut total_events = 0usize;
    let mut total_errors = 0usize;

    for (i, t) in to_process.iter().enumerate() {
        let short: String = t.session_id.chars().take(12).collect();
        eprint!("\r  [{}/{}] {short}...          ", i + 1, to_process.len());

        match crate::extract::extract_archived(&t.session_id, db).await {
            Ok(Some(result)) => {
                total_events += result.events_created;
                total_errors += result.errors.len();
            }
            Ok(None) => {}
            Err(e) => {
                total_errors += 1;
                debug!("extract_error: {}: {e}", t.session_id);
            }
        }
    }
    eprintln!();

    if total_events > 0 {
        eprintln!("Optimizing database...");
        db.compact().await?;
    }

    println!("Extraction complete:");
    println!("  Transcripts: {}", to_process.len());
    println!("  Events created: {total_events}");
    if total_errors > 0 {
        println!("  Errors: {total_errors}");
    }

    Ok(())
}

/// Show transcript archive statistics.
async fn cmd_stats(db: &Database) -> anyhow::Result<()> {
    let stats = db.get_transcript_stats().await?;

    if stats.total == 0 {
        println!("No archived transcripts");
        println!("Run: reclaude transcripts sync");
        return Ok(());
    }

    println!("Archived: {} transcripts", stats.total);
    println!("  Main sessions: {}", stats.main_count);
    println!("  Subagents: {}", stats.subagent_count);
    println!();

    let ratio = if stats.total_compressed_bytes > 0 {
        stats.total_size_bytes as f64 / stats.total_compressed_bytes as f64
    } else {
        0.0
    };

    println!(
        "Storage: {} -> {} ({ratio:.1}x compression)",
        format_size(stats.total_size_bytes),
        format_size(stats.total_compressed_bytes),
    );
    if !stats.oldest.is_empty() {
        println!("Oldest: {}", &stats.oldest[..16.min(stats.oldest.len())]);
        println!("Newest: {}", &stats.newest[..16.min(stats.newest.len())]);
    }

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Find all transcript JSONL files in Claude Code projects directory.
fn discover_transcripts(projects_dir: &Path) -> anyhow::Result<Vec<TranscriptInfo>> {
    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();

    for entry in std::fs::read_dir(projects_dir)? {
        let entry = entry?;
        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }

        // Main session transcripts: <project>/<uuid>.jsonl
        for jsonl in std::fs::read_dir(&project_dir)?.flatten() {
            let path = jsonl.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let session_id = path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());
            if let Some(session_id) = session_id {
                let size = std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0);
                results.push(TranscriptInfo {
                    path,
                    session_id,
                    parent_session_id: None,
                    size_bytes: size,
                });
            }
        }

        // Subagent transcripts: <project>/<parent-uuid>/subagents/agent-*.jsonl
        for parent_entry in std::fs::read_dir(&project_dir)?.flatten() {
            let parent_path = parent_entry.path();
            if !parent_path.is_dir() {
                continue;
            }
            let subagents_dir = parent_path.join("subagents");
            if !subagents_dir.exists() {
                continue;
            }
            let parent_id = parent_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            for jsonl in std::fs::read_dir(&subagents_dir)?.flatten() {
                let path = jsonl.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let session_id = path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());
                if let Some(session_id) = session_id {
                    let size = std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0);
                    results.push(TranscriptInfo {
                        path,
                        session_id,
                        parent_session_id: Some(parent_id.clone()),
                        size_bytes: size,
                    });
                }
            }
        }
    }

    Ok(results)
}

/// Extract metadata from first line of transcript JSONL.
fn extract_metadata(path: &Path) -> Option<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    let first_line = reader.lines().next()?.ok()?;
    let obj: serde_json::Value = serde_json::from_str(&first_line).ok()?;
    let meta = serde_json::json!({
        "cwd": obj["cwd"],
        "version": obj["version"],
        "gitBranch": obj["gitBranch"],
    });
    Some(meta.to_string())
}

/// Format bytes as human-readable size.
fn format_size(bytes: i64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    for unit in &units {
        if size < 1024.0 {
            return format!("{size:.1} {unit}");
        }
        size /= 1024.0;
    }
    format!("{size:.1} TB")
}
