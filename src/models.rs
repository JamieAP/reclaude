use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

// ── Event Types ─────────────────────────────────────────────────────

/// All 22 semantic event types captured from Claude Code sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    UserPrompt,
    Assistant,
    Plan,
    Thinking,
    Compaction,
    FileDiff,
    ToolUse,
    SysMsg,
    PlanFile,
    SessionStart,
    SessionEnd,
    SubagentStop,
    PreToolUse,
    PermissionRequest,
    Notification,
    TaskCreate,
    TaskUpdate,
    TaskGet,
    TaskList,
    TodoWrite,
    SubagentSpawn,
    SubagentOutput,
}

impl EventType {
    /// Get the display category for timeline/UI filtering.
    pub fn category(self) -> EventCategory {
        match self {
            Self::UserPrompt | Self::Assistant | Self::Plan | Self::Thinking => {
                EventCategory::Conversation
            }
            Self::ToolUse | Self::FileDiff | Self::PlanFile | Self::TodoWrite | Self::PreToolUse => {
                EventCategory::Action
            }
            Self::SessionStart
            | Self::SessionEnd
            | Self::SubagentSpawn
            | Self::SubagentStop
            | Self::SubagentOutput => EventCategory::Lifecycle,
            Self::Compaction
            | Self::SysMsg
            | Self::PermissionRequest
            | Self::Notification
            | Self::TaskCreate
            | Self::TaskUpdate
            | Self::TaskGet
            | Self::TaskList => EventCategory::System,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserPrompt => "user_prompt",
            Self::Assistant => "assistant",
            Self::Plan => "plan",
            Self::Thinking => "thinking",
            Self::Compaction => "compaction",
            Self::FileDiff => "file_diff",
            Self::ToolUse => "tool_use",
            Self::SysMsg => "sys_msg",
            Self::PlanFile => "plan_file",
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::SubagentStop => "subagent_stop",
            Self::PreToolUse => "pre_tool_use",
            Self::PermissionRequest => "permission_request",
            Self::Notification => "notification",
            Self::TaskCreate => "task_create",
            Self::TaskUpdate => "task_update",
            Self::TaskGet => "task_get",
            Self::TaskList => "task_list",
            Self::TodoWrite => "todo_write",
            Self::SubagentSpawn => "subagent_spawn",
            Self::SubagentOutput => "subagent_output",
        }
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user_prompt" => Ok(Self::UserPrompt),
            "assistant" => Ok(Self::Assistant),
            "plan" => Ok(Self::Plan),
            "thinking" => Ok(Self::Thinking),
            "compaction" => Ok(Self::Compaction),
            "file_diff" => Ok(Self::FileDiff),
            "tool_use" => Ok(Self::ToolUse),
            "sys_msg" => Ok(Self::SysMsg),
            "plan_file" => Ok(Self::PlanFile),
            "session_start" => Ok(Self::SessionStart),
            "session_end" => Ok(Self::SessionEnd),
            "subagent_stop" => Ok(Self::SubagentStop),
            "pre_tool_use" => Ok(Self::PreToolUse),
            "permission_request" => Ok(Self::PermissionRequest),
            "notification" => Ok(Self::Notification),
            "task_create" => Ok(Self::TaskCreate),
            "task_update" => Ok(Self::TaskUpdate),
            "task_get" => Ok(Self::TaskGet),
            "task_list" => Ok(Self::TaskList),
            "todo_write" => Ok(Self::TodoWrite),
            "subagent_spawn" => Ok(Self::SubagentSpawn),
            "subagent_output" => Ok(Self::SubagentOutput),
            _ => Err(format!("unknown event type: {s}")),
        }
    }
}

// ── Event Categories ────────────────────────────────────────────────

/// Display tier categories for filtering event noise in timelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventCategory {
    /// user_prompt, assistant, plan, thinking
    Conversation,
    /// tool_use, file_diff, plan_file, todo_write
    Action,
    /// session_start, session_end, subagent_spawn/stop/output
    Lifecycle,
    /// compaction, sys_msg, permission_request, notification, task_*
    System,
}

impl EventCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Action => "action",
            Self::Lifecycle => "lifecycle",
            Self::System => "system",
        }
    }
}

impl fmt::Display for EventCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EventCategory {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "conversation" => Ok(Self::Conversation),
            "action" => Ok(Self::Action),
            "lifecycle" => Ok(Self::Lifecycle),
            "system" => Ok(Self::System),
            _ => Err(format!("unknown category: {s}")),
        }
    }
}

// ── Core Data Structures ────────────────────────────────────────────

/// A semantic event stored in the events table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub timestamp: String, // ISO-8601
    pub event_type: String,
    pub category: String,
    pub session_id: Option<String>,
    pub content: String,
    pub cwd: Option<String>,
    pub remote_url: Option<String>,
    pub repo_name: Option<String>,
    pub branch: Option<String>,
    pub tool_name: Option<String>,
    pub file_path: Option<String>,
    pub metadata_json: String,
    pub vector: Option<Vec<f32>>,
}

impl Event {
    pub fn metadata(&self) -> serde_json::Value {
        serde_json::from_str(&self.metadata_json).unwrap_or_default()
    }
}

/// A session record stored in SQLite (first-class, upserted on each event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub cwd: Option<String>,
    pub repo_name: Option<String>,
    pub remote_url: Option<String>,
    pub branch: Option<String>,
    pub event_count: i64,
    pub is_active: bool,
}

/// Archived session transcript (zstd-compressed JSONL).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTranscript {
    pub id: i64,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub archived_at: String,
    pub transcript_path: Option<String>,
    pub size_bytes: i64,
    pub compressed_bytes: Option<i64>,
    pub metadata_json: Option<String>,
}

/// Scan state for incremental transcript processing.
#[derive(Debug, Clone)]
pub struct ScanState {
    pub last_byte_offset: i64,
}

/// Git repository context for an event or working directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitContext {
    pub cwd: String,
    pub repo_root: Option<String>,
    pub is_worktree: bool,
    pub remote_url: Option<String>,
    pub branch: Option<String>,
    pub repo_name: Option<String>,
}

// ── CLI Display Configuration ───────────────────────────────────────

/// Configuration for how an event type alias maps to display in CLI commands.
pub struct EventTypeConfig {
    pub semantic_types: &'static [EventType],
    pub default_limit: usize,
    pub empty_message: &'static str,
}

/// CLI type aliases for `--type` flag (maps shorthand to event types).
pub fn event_type_configs() -> &'static [(&'static str, EventTypeConfig)] {
    static CONFIGS: &[(&str, EventTypeConfig)] = &[
        (
            "prompt",
            EventTypeConfig {
                semantic_types: &[EventType::UserPrompt],
                default_limit: 10,
                empty_message: "No prompts found",
            },
        ),
        (
            "diff",
            EventTypeConfig {
                semantic_types: &[EventType::FileDiff],
                default_limit: 20,
                empty_message: "No diffs found",
            },
        ),
        (
            "plan",
            EventTypeConfig {
                semantic_types: &[EventType::Plan, EventType::PlanFile],
                default_limit: 10,
                empty_message: "No plans found",
            },
        ),
        (
            "tool",
            EventTypeConfig {
                semantic_types: &[EventType::ToolUse],
                default_limit: 20,
                empty_message: "No tool usage events found",
            },
        ),
        (
            "compaction",
            EventTypeConfig {
                semantic_types: &[EventType::Compaction],
                default_limit: 10,
                empty_message: "No compaction events found",
            },
        ),
    ];
    CONFIGS
}

/// Resolve a `--type` alias (e.g., "prompt") to its config.
pub fn resolve_type_alias(alias: &str) -> Option<&'static EventTypeConfig> {
    event_type_configs()
        .iter()
        .find(|(name, _)| *name == alias)
        .map(|(_, config)| config)
}
