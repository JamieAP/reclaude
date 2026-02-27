use std::process::Command;

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;
use tracing::debug;

use crate::{db::Database, models::Event};

const EVENT_SESSION_START: &str = "session_start";
const EVENT_USER_PROMPT: &str = "user_prompt";
const EVENT_ASSISTANT: &str = "assistant";
const DEFAULT_MMCTL_BIN: &str = "mmctl";
const DEFAULT_TEAM: &str = "demo";
const DEFAULT_CHANNEL: &str = "sessions";
const MAX_MM_PAYLOAD_CHARS: usize = 12_000;

#[derive(Debug, Clone)]
struct MmctlConfig {
    enabled: bool,
    bin: String,
    team: String,
    channel: String,
    channel_spec: String,
}

impl MmctlConfig {
    fn from_env() -> Self {
        let enabled = parse_bool_env("RECLAUDE_MMCTL_ENABLED", true);
        let bin =
            std::env::var("RECLAUDE_MMCTL_BIN").unwrap_or_else(|_| DEFAULT_MMCTL_BIN.to_string());

        let channel_env =
            std::env::var("RECLAUDE_MM_CHANNEL").unwrap_or_else(|_| DEFAULT_CHANNEL.to_string());

        if let Some((team, channel)) = channel_env.split_once(':') {
            let team = team.trim().to_string();
            let channel = channel.trim().to_string();
            return Self {
                enabled,
                bin,
                team: team.clone(),
                channel: channel.clone(),
                channel_spec: format!("{team}:{channel}"),
            };
        }

        let team = std::env::var("RECLAUDE_MM_TEAM").unwrap_or_else(|_| DEFAULT_TEAM.to_string());
        let channel = channel_env.trim().to_string();

        Self {
            enabled,
            bin,
            team: team.clone(),
            channel: channel.clone(),
            channel_spec: format!("{team}:{channel}"),
        }
    }
}

#[derive(Debug)]
struct SessionThread {
    channel_spec: String,
    root_post_id: String,
}

#[derive(Debug, Clone, Copy)]
enum SessionMessageKind {
    UserPrompt,
    Assistant,
}

impl SessionMessageKind {
    fn from_event_type(event_type: &str) -> Option<Self> {
        match event_type {
            EVENT_USER_PROMPT => Some(Self::UserPrompt),
            EVENT_ASSISTANT => Some(Self::Assistant),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::UserPrompt => "User Prompt",
            Self::Assistant => "Assistant",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::UserPrompt => ":bust_in_silhouette:",
            Self::Assistant => ":robot_face:",
        }
    }
}

#[derive(Debug, Deserialize)]
struct MmPost {
    id: String,
    #[serde(default)]
    create_at: i64,
    #[serde(default)]
    root_id: String,
    #[serde(default)]
    message: String,
}

/// Hook entry point for Mattermost posting from capture events.
///
/// Behavior:
/// - On `session_start`, ensure per-session root thread exists in the configured session channel.
/// - On `user_prompt` and `assistant`, post a formatted foldable reply in that thread.
/// - Other event types are ignored.
pub async fn maybe_post_hook_event(db: &Database, event: &Event) -> anyhow::Result<()> {
    let config = MmctlConfig::from_env();
    if !config.enabled {
        return Ok(());
    }

    let session_id = match event.session_id.as_deref() {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(()),
    };

    if event.event_type == EVENT_SESSION_START {
        ensure_session_thread(db, &config, session_id, event).await?;
        return Ok(());
    }

    let Some(kind) = SessionMessageKind::from_event_type(&event.event_type) else {
        return Ok(());
    };

    let thread = ensure_session_thread(db, &config, session_id, event).await?;
    let message = format_event_reply(kind, event, session_id);

    let output = run_mmctl(
        &config,
        &[
            "--suppress-warnings",
            "post",
            "create",
            &thread.channel_spec,
            "--reply-to",
            &thread.root_post_id,
            "--message",
            &message,
        ],
    )?;

    debug!(
        session_id,
        event_type = event.event_type,
        stdout = output.trim(),
        "mattermost_reply_posted"
    );
    Ok(())
}

async fn ensure_session_thread(
    db: &Database,
    config: &MmctlConfig,
    session_id: &str,
    event: &Event,
) -> anyhow::Result<SessionThread> {
    if let Some((channel_spec, root_post_id)) = db.get_mm_thread(session_id).await? {
        return Ok(SessionThread {
            channel_spec,
            root_post_id,
        });
    }

    ensure_channel_exists(config)?;

    let marker = format!(
        "reclaude-thread:{}:{}:{}",
        session_id,
        std::process::id(),
        chrono::Utc::now().timestamp_micros()
    );
    let root_message = format!(
        "{}\n\n<!-- {} -->",
        format_session_root(event, session_id),
        marker
    );

    run_mmctl(
        config,
        &[
            "--suppress-warnings",
            "post",
            "create",
            &config.channel_spec,
            "--message",
            &root_message,
        ],
    )?;

    // mmctl post create does not reliably return the created post ID, so resolve
    // by listing recent posts and matching our unique marker.
    let list_output = run_mmctl(
        config,
        &[
            "--suppress-warnings",
            "--json",
            "post",
            "list",
            &config.channel_spec,
            "--number",
            "40",
        ],
    )?;

    let root_post_id = find_post_id_by_marker(&list_output, &marker)
        .ok_or_else(|| anyhow!("failed to resolve root post id for session thread"))?;

    db.upsert_mm_thread(
        session_id,
        &config.channel_spec,
        &root_post_id,
        &event.timestamp,
    )
    .await?;

    Ok(SessionThread {
        channel_spec: config.channel_spec.clone(),
        root_post_id,
    })
}

fn ensure_channel_exists(config: &MmctlConfig) -> anyhow::Result<()> {
    let search = run_mmctl(
        config,
        &[
            "--suppress-warnings",
            "channel",
            "search",
            "--team",
            &config.team,
            &config.channel,
        ],
    );

    if search.is_ok() {
        return Ok(());
    }

    let display_name = channel_display_name(&config.channel);
    run_mmctl(
        config,
        &[
            "--suppress-warnings",
            "channel",
            "create",
            "--team",
            &config.team,
            "--name",
            &config.channel,
            "--display-name",
            &display_name,
            "--purpose",
            "Claude session threads captured by reclaude hooks",
        ],
    )
    .context("unable to create Mattermost channel")?;

    Ok(())
}

fn run_mmctl(config: &MmctlConfig, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new(&config.bin)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {}", config.bin))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let detail = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        bail!("{} {:?} failed: {}", config.bin, args, detail);
    }

    Ok(stdout)
}

fn format_session_root(event: &Event, session_id: &str) -> String {
    let repo = event.repo_name.as_deref().unwrap_or("unknown");
    let branch = event.branch.as_deref().unwrap_or("unknown");
    let cwd = event.cwd.as_deref().unwrap_or("unknown");
    let short_session = shorten(session_id, 12);

    let context = format!(
        "- Full session ID: `{session_id}`\n- Repo: `{repo}`\n- Branch: `{branch}`\n- CWD: `{cwd}`"
    );

    format!(
        "## :thread: Claude Session `{short_session}`\n\
`{}`\n\
\n{}",
        event.timestamp,
        format_foldable_block("Session context", &context)
    )
}

fn format_event_reply(kind: SessionMessageKind, event: &Event, session_id: &str) -> String {
    let chars = event.content.chars().count();
    let (payload, truncated) = truncate_at_char_boundary(&event.content, MAX_MM_PAYLOAD_CHARS);
    let mut body = fenced_text_block(&payload);
    if truncated {
        body.push_str("\n\n_... truncated to fit Mattermost post size limits._");
    }

    let summary = format!("{} payload ({chars} chars)", kind.label());
    let foldable = format_foldable_block(&summary, &body);
    let short_session = shorten(session_id, 12);

    format!(
        "### {} {}\n\
`{}` | session `{short_session}`\n\
\n{}",
        kind.icon(),
        kind.label(),
        event.timestamp,
        foldable
    )
}

fn format_foldable_block(summary: &str, content: &str) -> String {
    format!("/details {summary}\n{content}")
}

fn fenced_text_block(content: &str) -> String {
    let safe = content.replace("~~~", "~ ~ ~");
    format!("~~~text\n{safe}\n~~~")
}

fn truncate_at_char_boundary(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_string(), false);
    }

    let end = s
        .char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);

    (s[..end].to_string(), true)
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let prefix: String = s.chars().take(max).collect();
    format!("{prefix}...")
}

fn channel_display_name(channel: &str) -> String {
    channel
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_bool_env(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let normalized = v.trim().to_ascii_lowercase();
            !matches!(normalized.as_str(), "0" | "false" | "off" | "no")
        }
        Err(_) => default,
    }
}

fn extract_json_payload(stdout: &str) -> Option<&str> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "null" {
        return Some("null");
    }

    let start = trimmed.find(|c| c == '[' || c == '{')?;
    Some(trimmed[start..].trim())
}

fn find_post_id_by_marker(stdout: &str, marker: &str) -> Option<String> {
    let payload = extract_json_payload(stdout)?;
    let posts: Vec<MmPost> = serde_json::from_str(payload).ok()?;

    posts
        .into_iter()
        .filter(|post| post.root_id.is_empty() && post.message.contains(marker))
        .max_by_key(|post| post.create_at)
        .map(|post| post.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_payload_skips_mmctl_preamble() {
        let input = "There are 5 posts on https://mattermost.example/\n[{\"id\":\"p1\"}]";
        let payload = extract_json_payload(input);
        assert_eq!(payload, Some("[{\"id\":\"p1\"}]"));
    }

    #[test]
    fn find_post_id_by_marker_uses_latest_root_post() {
        let input = "There are 3 posts on https://mattermost.example/\n\
[{\"id\":\"p1\",\"create_at\":10,\"root_id\":\"\",\"message\":\"hello <!-- reclaude:m1 -->\"},\
{\"id\":\"p2\",\"create_at\":20,\"root_id\":\"thread\",\"message\":\"hello <!-- reclaude:m1 -->\"},\
{\"id\":\"p3\",\"create_at\":30,\"root_id\":\"\",\"message\":\"hello <!-- reclaude:m1 -->\"}]";

        let id = find_post_id_by_marker(input, "reclaude:m1");
        assert_eq!(id, Some("p3".to_string()));
    }

    #[test]
    fn foldable_block_uses_details_markdown() {
        let block = format_foldable_block("Prompt payload", "hello");
        assert!(block.starts_with("/details Prompt payload\n"));
        assert!(block.contains("hello"));
    }

    #[test]
    fn format_event_reply_includes_foldable_payload() {
        let event = Event {
            id: 1,
            timestamp: "2026-02-27T12:00:00Z".to_string(),
            event_type: EVENT_USER_PROMPT.to_string(),
            category: "conversation".to_string(),
            session_id: Some("sess-1234567890".to_string()),
            content: "ship it".to_string(),
            cwd: None,
            remote_url: None,
            repo_name: None,
            branch: None,
            tool_name: None,
            file_path: None,
            metadata_json: "{}".to_string(),
            vector: None,
        };

        let msg = format_event_reply(SessionMessageKind::UserPrompt, &event, "sess-1234567890");
        assert!(msg.contains("### :bust_in_silhouette: User Prompt"));
        assert!(msg.contains("/details User Prompt payload (7 chars)"));
        assert!(msg.contains("~~~text"));
        assert!(msg.contains("ship it"));
    }
}
