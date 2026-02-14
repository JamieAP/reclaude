use std::path::Path;

use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::db::Database;
use crate::git::get_git_context;
use crate::models::{Event, EventType, GitContext};

const MAX_PROMPT_LENGTH: usize = 10_000;
const MAX_TOOL_INPUT_LENGTH: usize = 100_000;
const MAX_TOOL_OUTPUT_LENGTH: usize = 100_000;
const MAX_CONTEXT_LENGTH: usize = 100_000;

/// Process a Claude Code hook event.
///
/// Entry point for `reclaude capture <hook_type>`. Reads JSON payload
/// from stdin, routes to the appropriate handler, inserts events,
/// and upserts the session record.
pub async fn process_hook(hook_type: &str) -> anyhow::Result<()> {
    let payload: Value = {
        let stdin = std::io::stdin();
        serde_json::from_reader(stdin.lock())?
    };

    info!(hook_type, "capture");

    let db = Database::open().await?;

    match hook_type {
        "UserPromptSubmit" => capture_user_prompt(&payload, &db).await,
        "PostToolUse" => capture_post_tool_use(&payload, &db).await,
        "Stop" => capture_stop(&payload, &db).await,
        "PreCompact" => capture_pre_compact(&payload, &db).await,
        "PermissionRequest" => capture_permission_request(&payload, &db).await,
        "Notification" => capture_notification(&payload, &db).await,
        "SubagentStop" => capture_subagent_stop(&payload, &db).await,
        "SessionEnd" => capture_session_end(&payload, &db).await,
        s if s.starts_with("SessionStart:") => {
            let trigger = &s["SessionStart:".len()..];
            capture_session_start(&payload, trigger, &db).await
        }
        s if s.starts_with("PreToolUse") => {
            let _ = s;
            Ok(()) // No-op (kept for hook registration compatibility)
        }
        _ => {
            warn!(hook_type, "unknown hook type");
            Ok(())
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn val_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn val_bool(v: &Value, key: &str) -> bool {
    v.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Find a valid UTF-8 boundary at or before `max` to avoid panicking
        // on multi-byte characters (emoji, CJK, etc.)
        let end = s.char_indices()
            .take_while(|(i, _)| *i < max)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}... (truncated)", &s[..end])
    }
}

fn cwd_from_payload(payload: &Value) -> String {
    val_str(payload, "cwd").unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    })
}

fn git_for_payload(payload: &Value) -> GitContext {
    let cwd = cwd_from_payload(payload);
    get_git_context(&cwd)
}

/// Try to generate a vector embedding for embeddable event types.
/// Returns None silently if model isn't downloaded or embedding fails.
fn try_embed(event_type: &EventType, content: &str) -> Option<Vec<f32>> {
    if !crate::embed::should_embed(event_type.as_str()) {
        return None;
    }
    let text: String = content.chars().take(4000).collect();
    match crate::embed::NomicEmbedder::load() {
        Ok(Some(mut embedder)) => match embedder.embed_document(&text) {
            Ok(vec) => Some(vec),
            Err(e) => {
                debug!("embed failed: {e}");
                None
            }
        },
        _ => None,
    }
}

fn make_event(
    event_type: EventType,
    content: &str,
    session_id: Option<&str>,
    cwd: Option<&str>,
    git: &GitContext,
    tool_name: Option<&str>,
    file_path: Option<&str>,
    metadata: Value,
    vector: Option<Vec<f32>>,
) -> Event {
    Event {
        id: 0, // assigned by EventStore::insert
        timestamp: now_iso(),
        event_type: event_type.to_string(),
        category: event_type.category().to_string(),
        session_id: session_id.map(String::from),
        content: content.to_string(),
        cwd: cwd.map(String::from).or_else(|| {
            if git.cwd.is_empty() {
                None
            } else {
                Some(git.cwd.clone())
            }
        }),
        remote_url: git.remote_url.clone(),
        repo_name: git.repo_name.clone(),
        branch: git.branch.clone(),
        tool_name: tool_name.map(String::from),
        file_path: file_path.map(String::from),
        metadata_json: metadata.to_string(),
        vector,
    }
}

/// Insert event into the events table and upsert session metadata.
async fn insert_and_upsert(db: &Database, event: &Event) -> anyhow::Result<i64> {
    let id = db.events.insert(event).await?;
    if let Some(sid) = &event.session_id {
        db.meta.upsert_session(
            sid,
            &event.timestamp,
            event.cwd.as_deref(),
            event.repo_name.as_deref(),
            event.remote_url.as_deref(),
            event.branch.as_deref(),
        )?;
    }
    Ok(id)
}

// ── UserPromptSubmit ─────────────────────────────────────────────────

async fn capture_user_prompt(payload: &Value, db: &Database) -> anyhow::Result<()> {
    let prompt = val_str(payload, "prompt").unwrap_or_default();
    let session_id = val_str(payload, "session_id");
    let cwd = val_str(payload, "cwd");

    // Skip empty or very short prompts
    if prompt.len() < 5 {
        debug!("prompt_skipped: too short");
        return Ok(());
    }

    // Skip task/system XML injected as user prompts
    if prompt.trim_start().starts_with("<task-notification") {
        debug!("prompt_skipped: task XML");
        return Ok(());
    }

    let content = truncate(&prompt, MAX_PROMPT_LENGTH);
    let git = git_for_payload(payload);

    let metadata = json!({
        "prompt_length": prompt.len(),
        "truncated": prompt.len() > MAX_PROMPT_LENGTH,
        "transcript_path": val_str(payload, "transcript_path"),
    });

    let vector = try_embed(&EventType::UserPrompt, &content);
    let event = make_event(
        EventType::UserPrompt,
        &content,
        session_id.as_deref(),
        cwd.as_deref(),
        &git,
        None,
        None,
        metadata,
        vector,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, chars = prompt.len(), "capture_prompt");
    Ok(())
}

// ── PostToolUse ──────────────────────────────────────────────────────

async fn capture_post_tool_use(payload: &Value, db: &Database) -> anyhow::Result<()> {
    let tool_name = val_str(payload, "tool_name").unwrap_or_default();
    let tool_input = payload.get("tool_input").cloned().unwrap_or(json!({}));
    let tool_result = payload.get("tool_result").cloned().unwrap_or(json!({}));
    let session_id = val_str(payload, "session_id");
    let cwd = val_str(payload, "cwd");
    let transcript_path = val_str(payload, "transcript_path");
    let is_error = val_bool(&tool_result, "is_error");

    let git = git_for_payload(payload);

    // Serialize input
    let input_str = {
        let s = serde_json::to_string_pretty(&tool_input).unwrap_or_default();
        truncate(&s, MAX_TOOL_INPUT_LENGTH)
    };

    // Extract output content (handles both string and content-block formats)
    let output_str = extract_tool_output(&tool_result);
    let output_str = truncate(&output_str, MAX_TOOL_OUTPUT_LENGTH);

    // Build combined content for tool_use event
    let tool_content = format!(
        "Tool: {tool_name}\nSuccess: {}\n\n--- INPUT ---\n{input_str}\n\n--- OUTPUT ---\n{output_str}",
        !is_error
    );

    let mut tool_metadata = json!({
        "tool_name": tool_name,
        "success": !is_error,
        "input_length": input_str.len(),
        "output_length": output_str.len(),
        "transcript_path": transcript_path,
    });

    // Extract subagent_type for Task tool calls
    if tool_name == "Task" {
        if let Some(subagent_type) = val_str(&tool_input, "subagent_type") {
            tool_metadata["subagent_type"] = json!(subagent_type);
            if let Some(persona) = subagent_type.split(':').last() {
                if persona != subagent_type {
                    tool_metadata["persona"] = json!(persona);
                }
            }
        }
    }

    let event = make_event(
        EventType::ToolUse,
        &tool_content,
        session_id.as_deref(),
        cwd.as_deref(),
        &git,
        Some(&tool_name),
        None,
        tool_metadata,
        None,
    );

    let tool_event_id = insert_and_upsert(db, &event).await?;
    info!(event_id = tool_event_id, tool = %tool_name, success = !is_error, "capture_tool_use");

    // ── File diff extraction ──────────────────────────────────────
    if tool_name == "Edit" && !is_error {
        capture_edit_diff(
            &tool_input,
            session_id.as_deref(),
            cwd.as_deref(),
            transcript_path.as_deref(),
            tool_event_id,
            &git,
            db,
        )
        .await?;
    } else if tool_name == "Write" && !is_error {
        capture_write_diff(
            &tool_input,
            session_id.as_deref(),
            cwd.as_deref(),
            transcript_path.as_deref(),
            tool_event_id,
            &git,
            db,
        )
        .await?;
    }

    // ── Task semantic events ──────────────────────────────────────
    match tool_name.as_str() {
        "TaskCreate" => {
            capture_task_create(
                &tool_input,
                &tool_result,
                session_id.as_deref(),
                cwd.as_deref(),
                transcript_path.as_deref(),
                tool_event_id,
                &git,
                db,
            )
            .await?;
        }
        "TaskUpdate" => {
            capture_task_update(
                &tool_input,
                session_id.as_deref(),
                cwd.as_deref(),
                transcript_path.as_deref(),
                tool_event_id,
                &git,
                db,
            )
            .await?;
        }
        "TaskGet" => {
            capture_task_get(
                &tool_input,
                &tool_result,
                session_id.as_deref(),
                cwd.as_deref(),
                transcript_path.as_deref(),
                tool_event_id,
                &git,
                db,
            )
            .await?;
        }
        "TaskList" => {
            capture_task_list(
                &tool_result,
                session_id.as_deref(),
                cwd.as_deref(),
                transcript_path.as_deref(),
                tool_event_id,
                &git,
                db,
            )
            .await?;
        }
        "TodoWrite" => {
            capture_todo_write(
                &tool_input,
                session_id.as_deref(),
                cwd.as_deref(),
                transcript_path.as_deref(),
                tool_event_id,
                &git,
                db,
            )
            .await?;
        }
        "Task" => {
            capture_subagent_spawn(
                &tool_input,
                session_id.as_deref(),
                cwd.as_deref(),
                transcript_path.as_deref(),
                tool_event_id,
                &git,
                db,
            )
            .await?;
        }
        "TaskOutput" => {
            capture_subagent_output(
                &tool_input,
                &tool_result,
                session_id.as_deref(),
                cwd.as_deref(),
                transcript_path.as_deref(),
                tool_event_id,
                &git,
                db,
            )
            .await?;
        }
        _ => {}
    }

    Ok(())
}

/// Extract output string from tool_result, handling content block format.
fn extract_tool_output(tool_result: &Value) -> String {
    let content = &tool_result["content"];
    match content {
        Value::Array(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    parts.push(text.to_string());
                } else if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    parts.push(block.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string());
                } else {
                    parts.push(serde_json::to_string(block).unwrap_or_default());
                }
            }
            parts.join("\n")
        }
        Value::String(s) => s.clone(),
        _ => content.to_string(),
    }
}

// ── File Diff Helpers ────────────────────────────────────────────────

async fn capture_edit_diff(
    tool_input: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: Option<&str>,
    tool_event_id: i64,
    git: &GitContext,
    db: &Database,
) -> anyhow::Result<()> {
    let file_path = match val_str(tool_input, "file_path") {
        Some(p) => p,
        None => return Ok(()),
    };
    let old_string = val_str(tool_input, "old_string").unwrap_or_default();
    let new_string = val_str(tool_input, "new_string").unwrap_or_default();

    if old_string == new_string {
        return Ok(());
    }

    let (diff_content, lines_added, lines_removed) =
        generate_edit_diff(&file_path, &old_string, &new_string);

    if diff_content.trim().is_empty() {
        return Ok(());
    }

    let metadata = json!({
        "file_path": file_path,
        "operation": "edit",
        "lines_added": lines_added,
        "lines_removed": lines_removed,
        "replace_all": val_bool(tool_input, "replace_all"),
        "tool_event_id": tool_event_id,
        "transcript_path": transcript_path,
    });

    let event = make_event(
        EventType::FileDiff,
        &diff_content,
        session_id,
        cwd,
        git,
        None,
        Some(&file_path),
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(
        event_id = id,
        file = %file_path,
        added = lines_added,
        removed = lines_removed,
        "capture_file_diff"
    );
    Ok(())
}

async fn capture_write_diff(
    tool_input: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: Option<&str>,
    tool_event_id: i64,
    git: &GitContext,
    db: &Database,
) -> anyhow::Result<()> {
    let file_path = match val_str(tool_input, "file_path") {
        Some(p) => p,
        None => return Ok(()),
    };
    let content = val_str(tool_input, "content").unwrap_or_default();

    if content.is_empty() {
        return Ok(());
    }

    let lines: Vec<&str> = content.lines().collect();
    let line_count = lines.len();

    let mut diff = format!("--- /dev/null\n+++ {file_path}\n@@ -0,0 +1,{line_count} @@\n");
    for line in &lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }

    let metadata = json!({
        "file_path": file_path,
        "operation": "write",
        "lines_added": line_count,
        "lines_removed": 0,
        "tool_event_id": tool_event_id,
        "transcript_path": transcript_path,
    });

    let event = make_event(
        EventType::FileDiff,
        &diff,
        session_id,
        cwd,
        git,
        None,
        Some(&file_path),
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(
        event_id = id,
        file = %file_path,
        added = line_count,
        "capture_file_diff"
    );
    Ok(())
}

/// Generate a unified-style diff from Edit tool's old/new strings.
fn generate_edit_diff(file_path: &str, old: &str, new: &str) -> (String, usize, usize) {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut diff = format!("--- {file_path}\n+++ {file_path}\n");
    diff.push_str(&format!(
        "@@ -{},{}  +{},{} @@\n",
        1,
        old_lines.len(),
        1,
        new_lines.len()
    ));

    let mut lines_removed = 0;
    for line in &old_lines {
        diff.push('-');
        diff.push_str(line);
        diff.push('\n');
        lines_removed += 1;
    }
    let mut lines_added = 0;
    for line in &new_lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
        lines_added += 1;
    }

    (diff, lines_added, lines_removed)
}

// ── Task Semantic Event Helpers ──────────────────────────────────────

async fn capture_task_create(
    tool_input: &Value,
    tool_result: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: Option<&str>,
    tool_event_id: i64,
    git: &GitContext,
    db: &Database,
) -> anyhow::Result<()> {
    let subject = val_str(tool_input, "subject").unwrap_or_default();
    let description = val_str(tool_input, "description").unwrap_or_default();

    // Try to extract task_id from result
    let task_id = extract_task_id_from_result(tool_result);

    let content = if description.is_empty() {
        format!("Task: {subject}")
    } else {
        format!("Task: {subject}\n\n{description}")
    };

    let metadata = json!({
        "task_id": task_id,
        "subject": subject,
        "description": truncate(&description, 500),
        "status": "pending",
        "tool_event_id": tool_event_id,
        "transcript_path": transcript_path,
    });

    let event = make_event(
        EventType::TaskCreate,
        &content,
        session_id,
        cwd,
        git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, "capture_task_create");
    Ok(())
}

async fn capture_task_update(
    tool_input: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: Option<&str>,
    tool_event_id: i64,
    git: &GitContext,
    db: &Database,
) -> anyhow::Result<()> {
    let task_id = val_str(tool_input, "taskId").unwrap_or_default();
    let new_status = val_str(tool_input, "status");
    let new_subject = val_str(tool_input, "subject");

    let mut changes = Vec::new();
    if let Some(ref s) = new_status {
        changes.push(format!("status={s}"));
    }
    if new_subject.is_some() {
        changes.push("subject".to_string());
    }
    if tool_input.get("description").is_some() {
        changes.push("description".to_string());
    }

    let content = if changes.is_empty() {
        format!("Task {task_id}: updated")
    } else {
        format!("Task {task_id}: {}", changes.join(", "))
    };

    let metadata = json!({
        "task_id": task_id,
        "status": new_status,
        "changes": changes,
        "tool_event_id": tool_event_id,
        "transcript_path": transcript_path,
    });

    let event = make_event(
        EventType::TaskUpdate,
        &content,
        session_id,
        cwd,
        git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, "capture_task_update");
    Ok(())
}

async fn capture_task_get(
    tool_input: &Value,
    tool_result: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: Option<&str>,
    tool_event_id: i64,
    git: &GitContext,
    db: &Database,
) -> anyhow::Result<()> {
    let task_id = val_str(tool_input, "taskId").unwrap_or_default();
    let is_error = val_bool(tool_result, "is_error");

    let content = if is_error {
        format!("Get task {task_id} (not found)")
    } else {
        format!("Get task {task_id}")
    };

    let metadata = json!({
        "task_id": task_id,
        "found": !is_error,
        "tool_event_id": tool_event_id,
        "transcript_path": transcript_path,
    });

    let event = make_event(
        EventType::TaskGet,
        &content,
        session_id,
        cwd,
        git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, "capture_task_get");
    Ok(())
}

async fn capture_task_list(
    tool_result: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: Option<&str>,
    tool_event_id: i64,
    git: &GitContext,
    db: &Database,
) -> anyhow::Result<()> {
    let result_content = val_str(tool_result, "content").unwrap_or_default();

    // Count tasks from output
    let task_count = result_content
        .lines()
        .filter(|l| {
            let trimmed = l.trim();
            trimmed.starts_with('-') || trimmed.starts_with('*')
        })
        .count();

    let content = if task_count > 0 {
        format!("Listed {task_count} tasks")
    } else {
        "Listed tasks".to_string()
    };

    let metadata = json!({
        "task_count": task_count,
        "tool_event_id": tool_event_id,
        "transcript_path": transcript_path,
    });

    let event = make_event(
        EventType::TaskList,
        &content,
        session_id,
        cwd,
        git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, "capture_task_list");
    Ok(())
}

async fn capture_todo_write(
    tool_input: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: Option<&str>,
    tool_event_id: i64,
    git: &GitContext,
    db: &Database,
) -> anyhow::Result<()> {
    let todos = tool_input
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let content_lines: Vec<String> = todos
        .iter()
        .map(|todo| {
            if let Some(obj) = todo.as_object() {
                let status = obj
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("pending");
                let text = obj
                    .get("content")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                format!("[{status}] {text}")
            } else {
                todo.to_string()
            }
        })
        .collect();

    let content = if content_lines.is_empty() {
        "Updated todos".to_string()
    } else {
        content_lines.join("\n")
    };

    let metadata = json!({
        "todo_count": todos.len(),
        "tool_event_id": tool_event_id,
        "transcript_path": transcript_path,
    });

    let event = make_event(
        EventType::TodoWrite,
        &content,
        session_id,
        cwd,
        git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, "capture_todo_write");
    Ok(())
}

async fn capture_subagent_spawn(
    tool_input: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: Option<&str>,
    tool_event_id: i64,
    git: &GitContext,
    db: &Database,
) -> anyhow::Result<()> {
    let subagent_type = val_str(tool_input, "subagent_type").unwrap_or_default();
    let description = val_str(tool_input, "description").unwrap_or_default();
    let prompt = val_str(tool_input, "prompt").unwrap_or_default();
    let run_in_background = val_bool(tool_input, "run_in_background");

    let persona = if subagent_type.contains(':') {
        subagent_type.split(':').last().map(String::from)
    } else {
        None
    };

    let mut content = if description.is_empty() {
        format!("Spawn {subagent_type}")
    } else {
        format!("Spawn {subagent_type}: {description}")
    };
    if !prompt.is_empty() {
        content.push_str("\n\n");
        content.push_str(&truncate(&prompt, 500));
    }

    let metadata = json!({
        "subagent_type": subagent_type,
        "persona": persona,
        "description": description,
        "run_in_background": run_in_background,
        "prompt_length": prompt.len(),
        "tool_event_id": tool_event_id,
        "transcript_path": transcript_path,
    });

    let event = make_event(
        EventType::SubagentSpawn,
        &content,
        session_id,
        cwd,
        git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, "capture_subagent_spawn");
    Ok(())
}

async fn capture_subagent_output(
    tool_input: &Value,
    tool_result: &Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
    transcript_path: Option<&str>,
    tool_event_id: i64,
    git: &GitContext,
    db: &Database,
) -> anyhow::Result<()> {
    let task_id = val_str(tool_input, "task_id").unwrap_or_default();
    let is_error = val_bool(tool_result, "is_error");

    let content = if is_error {
        format!("Output from task {task_id} (error)")
    } else {
        format!("Output from task {task_id}")
    };

    let result_content = val_str(tool_result, "content").unwrap_or_default();

    let metadata = json!({
        "task_id": task_id,
        "status": if is_error { "error" } else { "success" },
        "has_output": !result_content.is_empty() && !is_error,
        "output_length": result_content.len(),
        "tool_event_id": tool_event_id,
        "transcript_path": transcript_path,
    });

    let event = make_event(
        EventType::SubagentOutput,
        &content,
        session_id,
        cwd,
        git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, "capture_subagent_output");
    Ok(())
}

fn extract_task_id_from_result(tool_result: &Value) -> Option<String> {
    // Handle both string content and content-block format (array of {type, text})
    let content = extract_tool_output(tool_result);
    if content.is_empty() {
        return None;
    }
    for line in content.lines() {
        let lower = line.to_lowercase();
        if lower.contains("id:") {
            if let Some(id_part) = line.split(':').last() {
                let trimmed = id_part.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}

// ── Stop ─────────────────────────────────────────────────────────────

async fn capture_stop(payload: &Value, db: &Database) -> anyhow::Result<()> {
    let session_id = match val_str(payload, "session_id") {
        Some(sid) => sid,
        None => {
            debug!("stop_skipped: no session_id");
            return Ok(());
        }
    };

    let transcript_path = match val_str(payload, "transcript_path") {
        Some(p) if Path::new(&p).exists() => p,
        _ => {
            debug!("stop_skipped: no transcript");
            return Ok(());
        }
    };

    let git = git_for_payload(payload);
    let cwd = val_str(payload, "cwd");

    // Read last assistant message from JSONL transcript
    let data = match std::fs::read(&transcript_path) {
        Ok(d) => d,
        Err(e) => {
            warn!(error = %e, "stop_transcript_error");
            return Ok(());
        }
    };

    let mut last_assistant: Option<Value> = None;
    for line in data.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_slice::<Value>(line) {
            if entry.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                last_assistant = Some(entry);
            }
        }
    }

    let assistant = match last_assistant {
        Some(a) => a,
        None => return Ok(()),
    };

    // Extract text and thinking blocks from content
    let content_blocks = assistant
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let mut text_parts = Vec::new();
    let mut has_thinking = false;
    let mut has_tool_use = false;

    for block in &content_blocks {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
            Some("thinking") => {
                has_thinking = true;
            }
            Some("tool_use") => {
                has_tool_use = true;
            }
            _ => {}
        }
    }

    // Skip if only tool use (no text)
    if text_parts.is_empty() {
        return Ok(());
    }

    let assistant_text = text_parts.join("\n\n");
    if assistant_text.trim().is_empty() {
        return Ok(());
    }

    let metadata = json!({
        "response_length": assistant_text.len(),
        "has_thinking": has_thinking,
        "has_tool_use": has_tool_use,
        "text_blocks": text_parts.len(),
        "transcript_path": transcript_path,
    });

    let vector = try_embed(&EventType::Assistant, &assistant_text);
    let event = make_event(
        EventType::Assistant,
        &assistant_text,
        Some(&session_id),
        cwd.as_deref(),
        &git,
        None,
        None,
        metadata,
        vector,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(
        event_id = id,
        chars = assistant_text.len(),
        "capture_assistant"
    );
    Ok(())
}

// ── Session Lifecycle ────────────────────────────────────────────────

async fn capture_session_start(
    payload: &Value,
    trigger: &str,
    db: &Database,
) -> anyhow::Result<()> {
    let session_id = match val_str(payload, "session_id") {
        Some(sid) => sid,
        None => return Ok(()),
    };

    let git = git_for_payload(payload);
    let cwd = val_str(payload, "cwd");

    let metadata = json!({
        "trigger": trigger,
        "transcript_path": val_str(payload, "transcript_path"),
    });

    let event = make_event(
        EventType::SessionStart,
        &format!("Session started: {trigger}"),
        Some(&session_id),
        cwd.as_deref(),
        &git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, %trigger, "session_start");
    Ok(())
}

async fn capture_session_end(payload: &Value, db: &Database) -> anyhow::Result<()> {
    let session_id = match val_str(payload, "session_id") {
        Some(sid) => sid,
        None => return Ok(()),
    };

    let reason = val_str(payload, "reason").unwrap_or_else(|| "unknown".to_string());
    let git = git_for_payload(payload);
    let cwd = val_str(payload, "cwd");

    let metadata = json!({
        "reason": reason,
        "transcript_path": val_str(payload, "transcript_path"),
    });

    let event = make_event(
        EventType::SessionEnd,
        &format!("Session ended: {reason}"),
        Some(&session_id),
        cwd.as_deref(),
        &git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;

    // Also mark session inactive in SQLite
    db.meta.end_session(&session_id, &event.timestamp)?;

    info!(event_id = id, %reason, "session_end");
    Ok(())
}

// ── Other Hook Types ─────────────────────────────────────────────────

async fn capture_pre_compact(payload: &Value, db: &Database) -> anyhow::Result<()> {
    let session_id = val_str(payload, "session_id");
    let trigger = val_str(payload, "trigger").unwrap_or_else(|| "auto".to_string());
    let custom_instructions = val_str(payload, "custom_instructions").unwrap_or_default();
    let cwd = val_str(payload, "cwd");
    let git = git_for_payload(payload);

    let mut content = format!("Compaction triggered: {trigger}");
    if !custom_instructions.is_empty() {
        content.push_str("\nCustom instructions: ");
        content.push_str(&custom_instructions);
    }

    let metadata = json!({
        "trigger": trigger,
        "has_custom_instructions": !custom_instructions.is_empty(),
        "phase": "pre",
        "transcript_path": val_str(payload, "transcript_path"),
    });

    let event = make_event(
        EventType::Compaction,
        &content,
        session_id.as_deref(),
        cwd.as_deref(),
        &git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, %trigger, "capture_compaction");
    Ok(())
}

async fn capture_permission_request(payload: &Value, db: &Database) -> anyhow::Result<()> {
    let session_id = match val_str(payload, "session_id") {
        Some(sid) => sid,
        None => return Ok(()),
    };

    let tool_name = val_str(payload, "tool_name").unwrap_or_else(|| "unknown".to_string());
    let git = git_for_payload(payload);
    let cwd = val_str(payload, "cwd");

    let metadata = json!({
        "tool_name": tool_name,
        "transcript_path": val_str(payload, "transcript_path"),
    });

    let event = make_event(
        EventType::PermissionRequest,
        &format!("Permission requested for: {tool_name}"),
        Some(&session_id),
        cwd.as_deref(),
        &git,
        Some(&tool_name),
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, %tool_name, "permission_request");
    Ok(())
}

async fn capture_notification(payload: &Value, db: &Database) -> anyhow::Result<()> {
    let session_id = match val_str(payload, "session_id") {
        Some(sid) => sid,
        None => return Ok(()),
    };

    let notification_type = val_str(payload, "type").unwrap_or_else(|| "unknown".to_string());
    let message = val_str(payload, "message").unwrap_or_default();
    let git = git_for_payload(payload);
    let cwd = val_str(payload, "cwd");

    let metadata = json!({
        "notification_type": notification_type,
        "transcript_path": val_str(payload, "transcript_path"),
    });

    let content = if message.is_empty() {
        format!("Notification: {notification_type}")
    } else {
        message
    };

    let event = make_event(
        EventType::Notification,
        &content,
        Some(&session_id),
        cwd.as_deref(),
        &git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, %notification_type, "notification");
    Ok(())
}

async fn capture_subagent_stop(payload: &Value, db: &Database) -> anyhow::Result<()> {
    let session_id = match val_str(payload, "session_id") {
        Some(sid) => sid,
        None => return Ok(()),
    };

    let subagent_type = val_str(payload, "subagent_type").unwrap_or_else(|| "unknown".to_string());
    let result = val_str(payload, "result").unwrap_or_else(|| "Subagent completed".to_string());
    let git = git_for_payload(payload);
    let cwd = val_str(payload, "cwd");

    let content = truncate(&result, MAX_CONTEXT_LENGTH);

    let metadata = json!({
        "subagent_type": subagent_type,
        "transcript_path": val_str(payload, "transcript_path"),
    });

    let event = make_event(
        EventType::SubagentStop,
        &content,
        Some(&session_id),
        cwd.as_deref(),
        &git,
        None,
        None,
        metadata,
        None,
    );

    let id = insert_and_upsert(db, &event).await?;
    info!(event_id = id, %subagent_type, "subagent_stop");
    Ok(())
}
