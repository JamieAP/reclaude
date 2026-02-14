//! Transcript extraction: parses JSONL transcripts into semantic events.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;
use tracing::debug;

use crate::db::Database;
use crate::models::{Event, EventType};

const MAX_PROMPT_CONTENT: usize = 10_000;
const MAX_ASSISTANT_CONTENT: usize = 50_000;
const MAX_TOOL_CONTENT: usize = 100_000;

/// Result of extracting events from a single transcript.
pub struct ExtractResult {
    pub entries_scanned: usize,
    pub events_created: usize,
    pub errors: Vec<String>,
}

// ── JSONL Parsing ────────────────────────────────────────────────────

/// Parse JSONL entries from a byte slice, starting at a given byte offset.
///
/// If `start_offset > 0` and doesn't land on a line boundary, skips to the
/// next complete line.
pub fn parse_entries(data: &[u8], start_offset: usize) -> Vec<Value> {
    let slice = if start_offset >= data.len() {
        return Vec::new();
    } else {
        &data[start_offset..]
    };

    let text = std::str::from_utf8(slice).unwrap_or("");

    // If resuming mid-file, skip partial first line
    let skip_first = start_offset > 0
        && start_offset < data.len()
        && data[start_offset - 1] != b'\n';

    let mut entries = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 && skip_first {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(obj) => entries.push(obj),
            Err(_) => continue,
        }
    }
    entries
}

/// Group assistant entry indices by API message ID (for PLAN detection).
///
/// When text precedes tool_use in the same msg_id, the text is classified
/// as a PLAN rather than a plain ASSISTANT response.
pub fn group_by_msg_id(entries: &[Value]) -> HashMap<String, Vec<usize>> {
    let mut groups = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if entry.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        if let Some(msg_id) = entry
            .get("message")
            .and_then(|m| m.get("id"))
            .and_then(|id| id.as_str())
        {
            groups
                .entry(msg_id.to_string())
                .or_insert_with(Vec::new)
                .push(i);
        }
    }
    groups
}

// ── Helpers ──────────────────────────────────────────────────────────

fn val_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .take_while(|(i, _)| *i < max)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}... (truncated)", &s[..end])
    }
}

// ── Event Extraction ─────────────────────────────────────────────────

/// Extract semantic events from a single JSONL entry.
///
/// Returns 0-N events depending on entry type and content blocks.
pub fn extract_entry(
    entry: &Value,
    all_entries: &[Value],
    index: usize,
    msg_groups: &HashMap<String, Vec<usize>>,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: &str,
) -> Vec<Event> {
    let entry_type = match entry.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let timestamp = val_str(entry, "timestamp")
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let uuid = val_str(entry, "uuid").or_else(|| val_str(entry, "leafUuid"));
    let git_branch = val_str(entry, "gitBranch");

    let base_metadata = serde_json::json!({
        "transcript_path": transcript_path,
        "transcript_uuid": uuid,
        "source": "transcript",
    });

    match entry_type {
        "summary" => extract_summary(
            entry, session_id, cwd, &timestamp, git_branch.as_deref(), &base_metadata,
        ),
        "user" => extract_user(
            entry, session_id, cwd, &timestamp, git_branch.as_deref(), &base_metadata,
        ),
        "assistant" => extract_assistant(
            entry, all_entries, index, msg_groups,
            session_id, cwd, &timestamp, git_branch.as_deref(), &base_metadata,
        ),
        _ => Vec::new(), // Skip progress, system, file-history-snapshot, queue-operation
    }
}

fn make_extract_event(
    event_type: EventType,
    content: &str,
    session_id: Option<&str>,
    cwd: Option<&str>,
    timestamp: &str,
    branch: Option<&str>,
    metadata: Value,
) -> Event {
    Event {
        id: 0, // assigned by EventStore::insert
        timestamp: timestamp.to_string(),
        event_type: event_type.to_string(),
        category: event_type.category().to_string(),
        session_id: session_id.map(String::from),
        content: content.to_string(),
        cwd: cwd.map(String::from),
        remote_url: None,
        repo_name: None,
        branch: branch.map(String::from),
        tool_name: None,
        file_path: None,
        metadata_json: metadata.to_string(),
        vector: None,
    }
}

fn extract_summary(
    entry: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    timestamp: &str,
    branch: Option<&str>,
    base_metadata: &Value,
) -> Vec<Event> {
    let summary = match val_str(entry, "summary") {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };
    let mut meta = base_metadata.clone();
    meta["phase"] = serde_json::json!("post");
    meta["subtype"] = serde_json::json!("summary");
    vec![make_extract_event(
        EventType::Compaction, &summary, session_id, cwd, timestamp, branch, meta,
    )]
}

fn extract_user(
    entry: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    timestamp: &str,
    branch: Option<&str>,
    base_metadata: &Value,
) -> Vec<Event> {
    // Skip meta messages
    if entry.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Vec::new();
    }

    let message = match entry.get("message") {
        Some(m) => m,
        None => return Vec::new(),
    };

    let content = match message.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(_)) => return Vec::new(), // Tool results
        _ => return Vec::new(),
    };

    let trimmed = content.trim();
    if trimmed.len() < 5 {
        return Vec::new();
    }

    // Skip task XML injected as prompts
    if trimmed.starts_with("<task-notification") {
        return Vec::new();
    }

    let truncated_content = truncate(&content, MAX_PROMPT_CONTENT);
    let mut meta = base_metadata.clone();
    meta["prompt_length"] = serde_json::json!(content.len());
    meta["truncated"] = serde_json::json!(content.len() > MAX_PROMPT_CONTENT);

    vec![make_extract_event(
        EventType::UserPrompt, &truncated_content, session_id, cwd, timestamp, branch, meta,
    )]
}

fn extract_assistant(
    entry: &Value,
    all_entries: &[Value],
    index: usize,
    msg_groups: &HashMap<String, Vec<usize>>,
    session_id: Option<&str>,
    cwd: Option<&str>,
    timestamp: &str,
    branch: Option<&str>,
    base_metadata: &Value,
) -> Vec<Event> {
    let message = match entry.get("message") {
        Some(m) => m,
        None => return Vec::new(),
    };
    let content_blocks = match message.get("content").and_then(|c| c.as_array()) {
        Some(blocks) => blocks,
        None => return Vec::new(),
    };
    let msg_id = message.get("id").and_then(|id| id.as_str());

    // Collect block types
    let block_types: Vec<&str> = content_blocks
        .iter()
        .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
        .collect();

    let has_text = block_types.contains(&"text");
    let has_thinking = block_types.contains(&"thinking");
    let has_tool_use = block_types.contains(&"tool_use");

    // Extract text content
    let text_content: String = if has_text {
        content_blocks
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    // Extract thinking content
    let thinking_content: String = if has_thinking {
        content_blocks
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("thinking"))
            .filter_map(|b| b.get("thinking").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    let mut events = Vec::new();

    // Priority: TEXT (could be PLAN) > THINKING-only
    if !text_content.trim().is_empty() {
        // Check if this is a PLAN (text followed by tool_use)
        let is_plan = has_tool_use || is_plan_by_msg_group(msg_id, index, all_entries, msg_groups);

        let event_type = if is_plan { EventType::Plan } else { EventType::Assistant };
        let truncated = truncate(&text_content, MAX_ASSISTANT_CONTENT);

        let mut meta = base_metadata.clone();
        meta["msg_id"] = serde_json::json!(msg_id);
        meta["is_plan"] = serde_json::json!(is_plan);
        meta["has_thinking"] = serde_json::json!(!thinking_content.is_empty());
        meta["response_length"] = serde_json::json!(text_content.len());

        events.push(make_extract_event(
            event_type, &truncated, session_id, cwd, timestamp, branch, meta,
        ));
    } else if !thinking_content.trim().is_empty() {
        let truncated = truncate(&thinking_content, MAX_ASSISTANT_CONTENT);
        let mut meta = base_metadata.clone();
        meta["msg_id"] = serde_json::json!(msg_id);
        meta["has_tool_use"] = serde_json::json!(has_tool_use);

        events.push(make_extract_event(
            EventType::Thinking, &truncated, session_id, cwd, timestamp, branch, meta,
        ));
    }

    // Extract tool_use blocks
    if has_tool_use {
        for block in content_blocks {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                continue;
            }
            let tool_name = block.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let tool_input = block.get("input").cloned().unwrap_or(serde_json::json!({}));
            let tool_id = block.get("id").and_then(|id| id.as_str()).unwrap_or("");

            // Find matching tool_result in next few entries
            let tool_output = find_tool_result(all_entries, index, tool_id);

            let input_str = truncate(
                &serde_json::to_string_pretty(&tool_input).unwrap_or_default(),
                MAX_TOOL_CONTENT,
            );
            let output_str = truncate(&tool_output, MAX_TOOL_CONTENT);
            let tool_content = format!(
                "Tool: {tool_name}\n\n--- INPUT ---\n{input_str}\n\n--- OUTPUT ---\n{output_str}"
            );

            let mut meta = base_metadata.clone();
            meta["tool_name"] = serde_json::json!(tool_name);
            meta["tool_id"] = serde_json::json!(tool_id);
            meta["msg_id"] = serde_json::json!(msg_id);
            meta["success"] = serde_json::json!(true);
            meta["input_length"] = serde_json::json!(input_str.len());
            meta["output_length"] = serde_json::json!(output_str.len());

            let mut event = make_extract_event(
                EventType::ToolUse, &tool_content, session_id, cwd, timestamp, branch, meta,
            );
            event.tool_name = Some(tool_name.to_string());
            events.push(event);
        }
    }

    events
}

/// Check if text in this entry is a PLAN by looking for tool_use in same msg_id group.
fn is_plan_by_msg_group(
    msg_id: Option<&str>,
    index: usize,
    all_entries: &[Value],
    msg_groups: &HashMap<String, Vec<usize>>,
) -> bool {
    let msg_id = match msg_id {
        Some(id) => id,
        None => return false,
    };
    let indices = match msg_groups.get(msg_id) {
        Some(v) => v,
        None => return false,
    };
    let my_pos = match indices.iter().position(|&i| i == index) {
        Some(p) => p,
        None => return false,
    };
    // Check later entries in same msg_id for tool_use
    for &later_idx in &indices[my_pos + 1..] {
        if later_idx >= all_entries.len() {
            continue;
        }
        let later = &all_entries[later_idx];
        if let Some(blocks) = later
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        {
            for block in blocks {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    return true;
                }
            }
        }
    }
    false
}

/// Find tool_result matching a tool_use ID in subsequent entries.
fn find_tool_result(all_entries: &[Value], tool_use_index: usize, tool_id: &str) -> String {
    for later_idx in (tool_use_index + 1)..std::cmp::min(tool_use_index + 5, all_entries.len()) {
        let later = &all_entries[later_idx];
        if later.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        let content = match later
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        {
            Some(c) => c,
            None => continue,
        };
        for block in content {
            if block.get("tool_use_id").and_then(|id| id.as_str()) == Some(tool_id) {
                let result_content = block.get("content");
                return match result_content {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => serde_json::to_string(other).unwrap_or_default(),
                    None => String::new(),
                };
            }
        }
    }
    String::new()
}

// ── Dedup ────────────────────────────────────────────────────────────

/// Generate a dedup key for an entry.
///
/// Prefers UUID (unique per entry). Falls back to a content hash
/// combining type + timestamp + first 500 chars of content.
fn dedup_key(entry: &Value, transcript_path: &str) -> String {
    let uuid = entry
        .get("uuid")
        .and_then(|v| v.as_str())
        .or_else(|| entry.get("leafUuid").and_then(|v| v.as_str()));

    if let Some(uuid) = uuid {
        return format!("{transcript_path}:{uuid}");
    }

    // Content hash fallback
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    entry
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .hash(&mut hasher);
    entry
        .get("timestamp")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .hash(&mut hasher);
    entry
        .get("summary")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .hash(&mut hasher);
    if let Some(msg) = entry.get("message") {
        let content = msg.get("content");
        match content {
            Some(Value::String(s)) => s[..std::cmp::min(500, s.len())].hash(&mut hasher),
            Some(Value::Array(arr)) if !arr.is_empty() => {
                serde_json::to_string(&arr[0])
                    .unwrap_or_default()
                    .hash(&mut hasher);
            }
            _ => {}
        }
    }
    format!("{transcript_path}:hash:{:016x}", hasher.finish())
}

// ── Orchestrator ─────────────────────────────────────────────────────

/// Extract semantic events from transcript JSONL content.
///
/// `data`: raw JSONL bytes (decompressed if from archive)
/// `transcript_path`: used for dedup keys and metadata
/// `start_offset`: byte offset for incremental processing
/// `known_keys`: set of already-extracted dedup keys (skip these)
pub async fn extract_from_bytes(
    data: &[u8],
    transcript_path: &str,
    start_offset: usize,
    known_keys: &HashSet<String>,
    db: &Database,
) -> anyhow::Result<ExtractResult> {
    let entries = parse_entries(data, start_offset);

    let mut result = ExtractResult {
        entries_scanned: entries.len(),
        events_created: 0,
        errors: Vec::new(),
    };

    if entries.is_empty() {
        return Ok(result);
    }

    // Extract session_id and cwd from first entry that has them
    let session_id = entries.iter().find_map(|e| val_str(e, "sessionId"));
    let cwd = entries.iter().find_map(|e| val_str(e, "cwd"));

    let msg_groups = group_by_msg_id(&entries);

    let mut seen_keys = HashSet::new();
    let mut last_uuid: Option<String> = None;

    for (i, entry) in entries.iter().enumerate() {
        let uuid = val_str(entry, "uuid").or_else(|| val_str(entry, "leafUuid"));
        if uuid.is_some() {
            last_uuid = uuid.clone();
        }

        let key = dedup_key(entry, transcript_path);
        if known_keys.contains(&key) || seen_keys.contains(&key) {
            continue;
        }

        let events = extract_entry(
            entry,
            &entries,
            i,
            &msg_groups,
            session_id.as_deref(),
            cwd.as_deref(),
            transcript_path,
        );

        for event in events {
            match db.events.insert(&event).await {
                Ok(id) => {
                    if let Some(sid) = &event.session_id {
                        let _ = db.meta.upsert_session(
                            sid,
                            &event.timestamp,
                            event.cwd.as_deref(),
                            event.repo_name.as_deref(),
                            event.remote_url.as_deref(),
                            event.branch.as_deref(),
                        );
                    }
                    result.events_created += 1;
                    debug!(event_id = id, event_type = %event.event_type, "extracted");
                }
                Err(e) => {
                    result.errors.push(format!("insert error: {e}"));
                }
            }
        }

        seen_keys.insert(key);
    }

    // Update scan state
    db.meta.update_scan_state(
        transcript_path,
        data.len() as i64,
        last_uuid.as_deref(),
        result.events_created as i64,
    )?;

    Ok(result)
}

/// Extract from a transcript file on disk.
pub async fn extract_file(
    path: &Path,
    force: bool,
    db: &Database,
) -> anyhow::Result<ExtractResult> {
    let path_str = path.to_string_lossy().to_string();

    let start_offset = if force {
        0
    } else {
        db.meta
            .get_scan_state(&path_str)?
            .map(|s| s.last_byte_offset as usize)
            .unwrap_or(0)
    };

    let data = std::fs::read(path)?;
    let known_keys = HashSet::new();
    extract_from_bytes(&data, &path_str, start_offset, &known_keys, db).await
}

/// Extract from a compressed transcript stored in the archive database.
pub async fn extract_archived(
    session_id: &str,
    db: &Database,
) -> anyhow::Result<Option<ExtractResult>> {
    let compressed = match db.meta.get_transcript_content(session_id)? {
        Some(data) => data,
        None => return Ok(None),
    };

    let decompressed = zstd::decode_all(compressed.as_slice())?;
    let transcript_path = format!("archive:{session_id}");

    let start_offset = db
        .meta
        .get_scan_state(&transcript_path)?
        .map(|s| s.last_byte_offset as usize)
        .unwrap_or(0);

    let known_keys = HashSet::new();
    let result =
        extract_from_bytes(&decompressed, &transcript_path, start_offset, &known_keys, db).await?;
    Ok(Some(result))
}

// ── Plans Sync ───────────────────────────────────────────────────────

/// Sync plan files from ~/.claude/plans/ as plan_file events.
pub async fn sync_plans(db: &Database) -> anyhow::Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let plans_dir = std::path::PathBuf::from(&home).join(".claude/plans");

    if !plans_dir.exists() {
        println!("No plans directory at {}", plans_dir.display());
        return Ok(());
    }

    let mut plan_files: Vec<std::path::PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&plans_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            plan_files.push(path);
        }
    }

    if plan_files.is_empty() {
        println!("No plan files found in {}", plans_dir.display());
        return Ok(());
    }

    eprintln!("Found {} plan files", plan_files.len());
    let mut created = 0usize;
    let mut skipped = 0usize;

    for path in &plan_files {
        let path_str = path.to_string_lossy().to_string();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        // Use scan state to track if already ingested (by file size)
        let file_size = std::fs::metadata(path)?.len() as i64;
        if let Some(state) = db.meta.get_scan_state(&path_str)? {
            if state.last_byte_offset == file_size {
                skipped += 1;
                continue;
            }
        }

        let content = std::fs::read_to_string(path)?;
        if content.trim().is_empty() {
            continue;
        }

        let metadata = serde_json::json!({
            "file_path": path_str,
            "file_name": file_name,
            "source": "plans_sync",
        });

        // Use file modification time as timestamp
        let mtime = std::fs::metadata(path)?
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        let event = Event {
            id: 0,
            timestamp: mtime,
            event_type: EventType::PlanFile.to_string(),
            category: EventType::PlanFile.category().to_string(),
            session_id: None,
            content,
            cwd: None,
            remote_url: None,
            repo_name: None,
            branch: None,
            tool_name: None,
            file_path: Some(path_str.clone()),
            metadata_json: metadata.to_string(),
            vector: None,
        };

        db.events.insert(&event).await?;
        db.meta
            .update_scan_state(&path_str, file_size, None, 1)?;
        created += 1;
    }

    println!("Plans sync: {created} new, {skipped} unchanged");
    Ok(())
}
