use std::collections::HashMap;

use anyhow::Context;
use rusqlite::Connection;

use crate::cli::BackfillArgs;
use crate::cmd;
use crate::db::Database;
use crate::models::Event;

/// Default path to the legacy capture.db.
fn default_capture_db() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    format!("{home}/.reclaude/capture.db")
}

/// Map event_type string to category using the EventType enum.
/// Falls back to "system" for unknown types.
fn category_for(event_type: &str) -> &'static str {
    use std::str::FromStr;
    match crate::models::EventType::from_str(event_type) {
        Ok(et) => et.category().as_str(),
        Err(_) => "system",
    }
}

/// Extract an optional string from a serde_json::Value map.
fn meta_str(meta: &serde_json::Value, key: &str) -> Option<String> {
    meta.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

pub async fn run(args: &BackfillArgs, db: &Database) -> anyhow::Result<()> {
    #[allow(non_snake_case)]
    let (BOLD, CYAN, DIM, GREEN, RED, RESET) =
        (cmd::bold(), cmd::cyan(), cmd::dim(), cmd::green(), cmd::red(), cmd::reset());
    let capture_path = args
        .from
        .clone()
        .unwrap_or_else(default_capture_db);

    if !std::path::Path::new(&capture_path).exists() {
        anyhow::bail!("capture.db not found at: {capture_path}");
    }

    eprintln!("{BOLD}Backfill from legacy capture.db{RESET}");
    eprintln!("  {DIM}Source:{RESET} {capture_path}");

    // Open legacy SQLite DB (read-only)
    let conn = Connection::open_with_flags(
        &capture_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .context("failed to open capture.db")?;

    // Count events
    let total: usize = conn.query_row(
        "SELECT COUNT(*) FROM semantic_events",
        [],
        |row| row.get(0),
    )?;

    // Count by type
    let mut stmt = conn.prepare(
        "SELECT event_type, COUNT(*) FROM semantic_events GROUP BY event_type ORDER BY COUNT(*) DESC"
    )?;
    let type_counts: Vec<(String, usize)> = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    eprintln!("  {DIM}Events:{RESET} {total:>8}");
    for (et, count) in &type_counts {
        eprintln!("    {CYAN}{et:25}{RESET} {count:>8}");
    }

    let existing = db.events.count().await?;
    eprintln!("\n  {DIM}Current events:{RESET} {existing}");

    if args.dry_run {
        eprintln!("\n  {DIM}Dry run - no changes made.{RESET}");
        return Ok(());
    }

    // Confirm before destructive operation
    eprintln!("\n  {RED}This will DROP the existing events table and replace it.{RESET}");
    eprintln!("  Press Ctrl+C to abort, or wait 3 seconds to continue...");
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    eprint!("  Recreating events table... ");
    db.events.recreate_table().await?;
    eprintln!("{GREEN}done{RESET}");

    // 2. Read all events from legacy DB and insert in batches
    let batch_size = 5000;
    let mut offset = 0usize;
    let mut inserted = 0usize;
    let mut session_map: HashMap<String, (String, Option<String>, Option<String>, Option<String>, Option<String>)> = HashMap::new();

    let mut stmt = conn.prepare(
        "SELECT id, timestamp, event_type, session_id, content, metadata \
         FROM semantic_events ORDER BY id ASC LIMIT ?1 OFFSET ?2"
    )?;

    eprint!("  Inserting events ");

    loop {
        let events: Vec<Event> = {
            let rows = stmt.query_map(
                rusqlite::params![batch_size as i64, offset as i64],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )?;

            let mut batch = Vec::new();
            for row in rows {
                let (id, timestamp, event_type, session_id, content, metadata_str) = row?;
                let meta: serde_json::Value =
                    serde_json::from_str(&metadata_str).unwrap_or(serde_json::json!({}));

                let category = category_for(&event_type).to_string();
                let cwd = meta_str(&meta, "cwd");
                let remote_url = meta_str(&meta, "remote_url");
                let repo_name = meta_str(&meta, "repo_name");
                let branch = meta_str(&meta, "branch");
                let tool_name = meta_str(&meta, "tool_name");
                let file_path = meta_str(&meta, "file_path");

                // Track session metadata for upsert
                if let Some(ref sid) = session_id {
                    session_map.entry(sid.clone()).or_insert_with(|| {
                        (
                            timestamp.clone(),
                            cwd.clone(),
                            repo_name.clone(),
                            remote_url.clone(),
                            branch.clone(),
                        )
                    });
                }

                batch.push(Event {
                    id,
                    timestamp,
                    event_type,
                    category,
                    session_id,
                    content,
                    cwd,
                    remote_url,
                    repo_name,
                    branch,
                    tool_name,
                    file_path,
                    metadata_json: metadata_str,
                    vector: None,
                });
            }
            batch
        };

        if events.is_empty() {
            break;
        }

        let count = events.len();
        db.events.bulk_insert(&events).await?;
        inserted += count;
        offset += count;

        eprint!("\r  Inserting events [{inserted}/{total}]");

        if count < batch_size {
            break;
        }
    }
    eprintln!(" {GREEN}done{RESET}");

    // 3. Optimize FTS index (merge b-tree segments after bulk insert)
    eprint!("  Optimizing FTS index... ");
    db.events.optimize_fts().await?;
    eprintln!("{GREEN}done{RESET}");

    // 4. Upsert sessions into metadata DB
    eprint!("  Syncing sessions... ");
    let mut session_count = 0;
    for (sid, (ts, cwd, repo_name, remote_url, branch)) in &session_map {
        let _ = db.meta.upsert_session(
            sid,
            ts,
            cwd.as_deref(),
            repo_name.as_deref(),
            remote_url.as_deref(),
            branch.as_deref(),
        );
        session_count += 1;
    }
    eprintln!("{GREEN}{session_count} sessions{RESET}");

    // 5. Verify
    let final_count = db.events.count().await?;
    eprintln!("\n  {BOLD}Backfill complete{RESET}");
    eprintln!("  {DIM}Events:{RESET} {GREEN}{final_count}{RESET}");

    if final_count != total {
        eprintln!("  {RED}Warning: expected {total}, got {final_count}{RESET}");
    }

    Ok(())
}
