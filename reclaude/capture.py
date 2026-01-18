#!/usr/bin/env python3
"""
Semantic capture hook for Claude Code.

Captures Claude events:

Usage:
    python -m reclaude.capture <hook_type>
    # Or via installed entry point in hook config
"""

from __future__ import annotations

import difflib
import json
import os
import sys
from pathlib import Path

from .db import CaptureDB, SemanticEventType, _normalize_session_id
from .git import get_git_context
from .log import get_logger, log_event


# Cache git context per cwd to avoid repeated git calls in same hook
_git_context_cache: dict[str, dict] = {}


def enrich_metadata(metadata: dict, cwd: str | None = None) -> dict:
    """Add git context to metadata for repo-aware querying.

    Adds: repo_root, remote_url, repo_name, branch, is_worktree
    """
    cwd = cwd or metadata.get("cwd") or os.getcwd()

    # Use cached context if available
    if cwd not in _git_context_cache:
        _git_context_cache[cwd] = get_git_context(cwd)

    git_ctx = _git_context_cache[cwd]

    # Add git context fields (don't overwrite existing)
    for key in ("repo_root", "remote_url", "repo_name", "branch", "is_worktree"):
        if key not in metadata and git_ctx.get(key) is not None:
            metadata[key] = git_ctx[key]

    return metadata


LOG_DIR = Path.home() / ".reclaude"

MAX_PROMPT_LENGTH = 10000
MAX_CONTEXT_LENGTH = 100000

# Module-level logger - initialized lazily
log = get_logger()


def capture_user_prompt(payload: dict, db: CaptureDB) -> int | None:
    """Capture user prompts from UserPromptSubmit hook.

    All UserPromptSubmit events are treated as user prompts.
    System messages are extracted separately via batch transcript extraction.
    """
    prompt = payload.get("prompt", "")
    session_id = payload.get("session_id")
    sid_short = (session_id or "")[:12]
    cwd = payload.get("cwd")
    transcript_path = payload.get("transcript_path")

    if not prompt or len(prompt) < 5:
        log.debug("prompt_skipped", session=sid_short, reason="too_short")
        return None

    content = prompt[:MAX_PROMPT_LENGTH]

    # Get git context for worktree-aware storage
    git_ctx = get_git_context(cwd)

    metadata = {
        "prompt_length": len(prompt),
        "truncated": len(prompt) > MAX_PROMPT_LENGTH,
        "cwd": cwd,
        "transcript_path": transcript_path,
        "repo_root": git_ctx.get("repo_root"),
        "remote_url": git_ctx.get("remote_url"),
        "repo_name": git_ctx.get("repo_name"),
        "branch": git_ctx.get("branch"),
        "is_worktree": git_ctx.get("is_worktree"),
    }

    event_id = db.insert_event(
        event_type=SemanticEventType.USER_PROMPT,
        content=content,
        session_id=session_id,
        metadata=metadata,
    )

    log.info("capture_prompt", session=sid_short, event_id=event_id, chars=len(prompt))

    log_event(
        "capture_prompt",
        event_id=event_id,
        session_id=sid_short,
        chars=len(prompt),
        cwd=os.path.basename(cwd) if cwd else None,
    )

    return event_id


def capture_pre_tool_use(payload: dict, db: CaptureDB) -> list[int]:
    """Pre-tool hook, currently a no-op."""
    return []


MAX_TOOL_INPUT_LENGTH = 100000
MAX_TOOL_OUTPUT_LENGTH = 100000


def _generate_edit_diff(file_path: str, old_string: str, new_string: str) -> tuple[str, int, int]:
    """Generate unified diff from Edit tool strings.

    Returns: (diff_content, lines_added, lines_removed)
    """
    diff = difflib.unified_diff(
        old_string.splitlines(keepends=True),
        new_string.splitlines(keepends=True),
        fromfile=file_path,
        tofile=file_path,
    )
    diff_content = "".join(diff)

    lines_added = sum(1 for line in diff_content.split("\n") if line.startswith("+") and not line.startswith("+++"))
    lines_removed = sum(1 for line in diff_content.split("\n") if line.startswith("-") and not line.startswith("---"))

    return diff_content, lines_added, lines_removed


def capture_post_tool_use(payload: dict, db: CaptureDB) -> list[int]:
    """Capture all tool uses with full I/O."""
    tool_name = payload.get("tool_name", "")
    tool_input = payload.get("tool_input", {})
    tool_result = payload.get("tool_result", {})
    session_id = payload.get("session_id")
    sid_short = (session_id or "")[:12]
    cwd = payload.get("cwd")
    transcript_path = payload.get("transcript_path")

    log.debug("post_tool_use", session=sid_short, tool=tool_name)

    event_ids = []

    # Determine success/failure from tool_result
    is_error = tool_result.get("is_error", False)

    # Serialize input and output
    input_str = json.dumps(tool_input, indent=2, default=str)
    if len(input_str) > MAX_TOOL_INPUT_LENGTH:
        input_str = input_str[:MAX_TOOL_INPUT_LENGTH] + "\n... (truncated)"

    # Extract output content from tool_result
    output_content = tool_result.get("content", "")
    if isinstance(output_content, list):
        # Handle content blocks
        output_parts = []
        for block in output_content:
            if isinstance(block, dict):
                if block.get("type") == "text":
                    output_parts.append(block.get("text", ""))
                else:
                    output_parts.append(json.dumps(block, default=str))
            else:
                output_parts.append(str(block))
        output_str = "\n".join(output_parts)
    else:
        output_str = str(output_content) if output_content else ""

    if len(output_str) > MAX_TOOL_OUTPUT_LENGTH:
        output_str = output_str[:MAX_TOOL_OUTPUT_LENGTH] + "\n... (truncated)"

    # Create combined content for tool_use event
    tool_content = f"Tool: {tool_name}\n"
    tool_content += f"Success: {not is_error}\n"
    tool_content += f"\n--- INPUT ---\n{input_str}\n"
    tool_content += f"\n--- OUTPUT ---\n{output_str}"

    # Build metadata for tool_use event
    tool_metadata = {
        "tool_name": tool_name,
        "success": not is_error,
        "input_length": len(input_str),
        "output_length": len(output_str),
        "input_truncated": len(json.dumps(tool_input, default=str))
        > MAX_TOOL_INPUT_LENGTH,
        "output_truncated": len(str(output_content)) > MAX_TOOL_OUTPUT_LENGTH,
        "cwd": cwd,
        "transcript_path": transcript_path,
    }

    # Extract subagent_type for Task tool calls (persona/agent tracking)
    if tool_name == "Task":
        subagent_type = tool_input.get("subagent_type")
        if subagent_type:
            tool_metadata["subagent_type"] = subagent_type
            # Extract helper name (e.g., "example-tools:python-helper" -> "python-helper")
            if ":" in subagent_type:
                tool_metadata["persona"] = subagent_type.split(":")[-1]

    tool_event_id = db.insert_event(
        event_type=SemanticEventType.TOOL_USE,
        content=tool_content,
        session_id=session_id,
        metadata=enrich_metadata(tool_metadata),
    )
    event_ids.append(tool_event_id)

    log.info(
        "capture_tool_use",
        session=sid_short,
        event_id=tool_event_id,
        tool=tool_name,
        success=not is_error,
    )

    # Inline FILE_DIFF extraction for Edit tool
    if tool_name == "Edit" and not is_error:
        file_path = tool_input.get("file_path")
        old_string = tool_input.get("old_string", "")
        new_string = tool_input.get("new_string", "")

        if file_path and old_string != new_string:
            diff_content, lines_added, lines_removed = _generate_edit_diff(
                file_path, old_string, new_string
            )

            if diff_content.strip():
                diff_metadata = {
                    "file_path": file_path,
                    "operation": "edit",
                    "lines_added": lines_added,
                    "lines_removed": lines_removed,
                    "replace_all": tool_input.get("replace_all", False),
                    "tool_event_id": tool_event_id,
                    "cwd": cwd,
                    "transcript_path": transcript_path,
                }

                diff_event_id = db.insert_event(
                    event_type=SemanticEventType.FILE_DIFF,
                    content=diff_content,
                    session_id=session_id,
                    metadata=enrich_metadata(diff_metadata),
                )
                event_ids.append(diff_event_id)

                log.info(
                    "capture_file_diff",
                    session=sid_short,
                    event_id=diff_event_id,
                    file=file_path,
                    added=lines_added,
                    removed=lines_removed,
                )

    # Inline FILE_DIFF extraction for Write tool
    elif tool_name == "Write" and not is_error:
        file_path = tool_input.get("file_path")
        content = tool_input.get("content", "")

        if file_path and content:
            lines = content.splitlines()
            diff_lines = [f"--- /dev/null\n", f"+++ {file_path}\n"]
            diff_lines.append(f"@@ -0,0 +1,{len(lines)} @@\n")
            for line in lines:
                diff_lines.append(f"+{line}\n")
            diff_content = "".join(diff_lines)

            diff_metadata = {
                "file_path": file_path,
                "operation": "write",
                "lines_added": len(lines),
                "lines_removed": 0,
                "tool_event_id": tool_event_id,
                "cwd": cwd,
                "transcript_path": transcript_path,
            }

            diff_event_id = db.insert_event(
                event_type=SemanticEventType.FILE_DIFF,
                content=diff_content,
                session_id=session_id,
                metadata=enrich_metadata(diff_metadata),
            )
            event_ids.append(diff_event_id)

            log.info(
                "capture_file_diff",
                session=sid_short,
                event_id=diff_event_id,
                file=file_path,
                added=len(lines),
                removed=0,
            )

    return event_ids


def capture_pre_compact(payload: dict, db: CaptureDB) -> int | None:
    """Capture pre-compaction state."""
    session_id = payload.get("session_id")
    sid_short = (session_id or "")[:12]
    trigger = payload.get("trigger", "auto")
    custom_instructions = payload.get("custom_instructions", "")
    cwd = payload.get("cwd")
    transcript_path = payload.get("transcript_path")

    log.info("pre_compact", session=sid_short, trigger=trigger)

    content = f"Compaction triggered: {trigger}"
    if custom_instructions:
        content += f"\nCustom instructions: {custom_instructions}"

    event_id = db.insert_event(
        event_type=SemanticEventType.COMPACTION,
        content=content,
        session_id=session_id,
        metadata=enrich_metadata(
            {
                "trigger": trigger,
                "has_custom_instructions": bool(custom_instructions),
                "phase": "pre",
                "cwd": cwd,
                "transcript_path": transcript_path,
            }
        ),
    )

    log.info("capture_compaction", session=sid_short, event_id=event_id, phase="pre")

    log_event(
        "capture_compaction",
        event_id=event_id,
        session_id=sid_short,
        trigger=trigger,
    )
    return event_id


def capture_stop(payload: dict, db: CaptureDB) -> int | None:
    """Capture assistant text responses from the Stop hook.

    Extracts text blocks from the most recent assistant message.
    Skips responses that only contain tool_use (no text).
    """
    session_id = _normalize_session_id(payload.get("session_id"))
    if not session_id:
        log.debug("stop_skipped", reason="no_session")
        return None

    sid_short = session_id[:8]
    transcript_path = payload.get("transcript_path")
    if not transcript_path or not Path(transcript_path).exists():
        log.debug("stop_skipped", session=sid_short, reason="no_transcript")
        return None

    # Read last assistant message from JSONL
    event_id = None

    try:
        with open(transcript_path, "rb") as f:
            last_assistant_line = None
            for line in f:
                try:
                    entry = json.loads(line.decode("utf-8", errors="replace").strip())
                    if entry.get("type") == "assistant":
                        last_assistant_line = entry
                except json.JSONDecodeError:
                    continue

            if last_assistant_line:
                # Extract text and thinking blocks
                content_blocks = last_assistant_line.get("message", {}).get(
                    "content", []
                )
                text_parts = []
                thinking_parts = []
                has_tool_use = False

                for block in content_blocks:
                    block_type = block.get("type")
                    if block_type == "text":
                        text_parts.append(block.get("text", ""))
                    elif block_type == "thinking":
                        thinking_parts.append(block.get("thinking", ""))
                    elif block_type == "tool_use":
                        has_tool_use = True

                # Skip if only tool use (no text)
                if text_parts or thinking_parts:
                    # Combine text (thinking is captured separately in plan events)
                    assistant_text = "\n\n".join(text_parts)
                    if assistant_text.strip():
                        # Get git context
                        git_ctx = get_git_context()

                        # Create event
                        metadata = {
                            "response_length": len(assistant_text),
                            "has_thinking": len(thinking_parts) > 0,
                            "has_tool_use": has_tool_use,
                            "text_blocks": len(text_parts),
                            "transcript_path": str(transcript_path),
                            **git_ctx,
                        }

                        event_id = db.insert_event(
                            event_type=SemanticEventType.ASSISTANT,
                            session_id=session_id,
                            content=assistant_text,
                            metadata=metadata,
                        )

                        log.info(
                            "capture_assistant",
                            session=sid_short,
                            event_id=event_id,
                            chars=len(assistant_text),
                        )

    except (IOError, FileNotFoundError) as e:
        log.warning("stop_transcript_error", session=sid_short, error=str(e))

    return event_id


def capture_session_start(payload: dict, db: CaptureDB, trigger: str) -> int | None:
    """Capture session start events (startup, resume, clear, compact)."""
    session_id = _normalize_session_id(payload.get("session_id"))
    if not session_id:
        return None

    sid_short = session_id[:8]
    git_ctx = get_git_context()

    metadata = {
        "trigger": trigger,
        "transcript_path": payload.get("transcript_path"),
        **git_ctx,
    }

    event_id = db.insert_event(
        event_type=SemanticEventType.SESSION_START,
        session_id=session_id,
        content=f"Session started: {trigger}",
        metadata=metadata,
    )

    log.info("session_start", session=sid_short, trigger=trigger)
    return event_id


def capture_session_end(payload: dict, db: CaptureDB) -> int | None:
    """Capture session end events with reason."""
    session_id = _normalize_session_id(payload.get("session_id"))
    if not session_id:
        return None

    sid_short = session_id[:8]
    reason = payload.get("reason", "unknown")
    git_ctx = get_git_context()

    metadata = {
        "reason": reason,
        "transcript_path": payload.get("transcript_path"),
        **git_ctx,
    }

    event_id = db.insert_event(
        event_type=SemanticEventType.SESSION_END,
        session_id=session_id,
        content=f"Session ended: {reason}",
        metadata=metadata,
    )

    log.info("session_end", session=sid_short, reason=reason)
    return event_id


def capture_subagent_stop(payload: dict, db: CaptureDB) -> int | None:
    """Capture when Task tool subagents complete."""
    session_id = _normalize_session_id(payload.get("session_id"))
    if not session_id:
        return None

    sid_short = session_id[:8]
    git_ctx = get_git_context()

    # Extract subagent info from payload
    subagent_result = payload.get("result", "")
    subagent_type = payload.get("subagent_type", "unknown")

    metadata = {
        "subagent_type": subagent_type,
        "transcript_path": payload.get("transcript_path"),
        **git_ctx,
    }

    # Truncate result if too long
    content = (
        subagent_result[:MAX_CONTEXT_LENGTH]
        if subagent_result
        else "Subagent completed"
    )

    event_id = db.insert_event(
        event_type=SemanticEventType.SUBAGENT_STOP,
        session_id=session_id,
        content=content,
        metadata=metadata,
    )

    log.info("subagent_stop", session=sid_short, subagent_type=subagent_type)
    return event_id


def capture_permission_request(payload: dict, db: CaptureDB) -> int | None:
    """Capture permission request events."""
    session_id = _normalize_session_id(payload.get("session_id"))
    if not session_id:
        return None

    sid_short = session_id[:8]
    git_ctx = get_git_context()

    tool_name = payload.get("tool_name", "unknown")
    tool_input = payload.get("tool_input", {})

    metadata = {
        "tool_name": tool_name,
        "tool_input": tool_input,
        "transcript_path": payload.get("transcript_path"),
        **git_ctx,
    }

    event_id = db.insert_event(
        event_type=SemanticEventType.PERMISSION_REQUEST,
        session_id=session_id,
        content=f"Permission requested for: {tool_name}",
        metadata=metadata,
    )

    log.info("permission_request", session=sid_short, tool=tool_name)
    return event_id


def capture_notification(payload: dict, db: CaptureDB) -> int | None:
    """Capture notification events."""
    session_id = _normalize_session_id(payload.get("session_id"))
    if not session_id:
        return None

    sid_short = session_id[:8]
    git_ctx = get_git_context()

    notification_type = payload.get("type", "unknown")
    message = payload.get("message", "")

    metadata = {
        "notification_type": notification_type,
        "transcript_path": payload.get("transcript_path"),
        **git_ctx,
    }

    event_id = db.insert_event(
        event_type=SemanticEventType.NOTIFICATION,
        session_id=session_id,
        content=message or f"Notification: {notification_type}",
        metadata=metadata,
    )

    log.info("notification", session=sid_short, notification_type=notification_type)
    return event_id


def _open_capture_db() -> CaptureDB | None:
    """Open the capture database, optionally overridden by RECLAUDE_DB_PATH."""
    db_path = os.environ.get("RECLAUDE_DB_PATH")
    try:
        if db_path:
            try:
                return CaptureDB(db_path)
            except TypeError:
                # Some tests monkeypatch CaptureDB with a no-arg callable.
                return CaptureDB()
        return CaptureDB()
    except Exception as e:
        log.warning("db_open_error", error=str(e))
        return None


def process_hook(hook_type: str, payload: dict) -> None:
    """Process a hook event and capture semantic data."""
    db = _open_capture_db()
    if not db:
        return

    if hook_type == "UserPromptSubmit":
        capture_user_prompt(payload, db)
    elif hook_type == "PreToolUse":
        capture_pre_tool_use(payload, db)
    elif hook_type == "PostToolUse":
        capture_post_tool_use(payload, db)
    elif hook_type == "PreCompact":
        capture_pre_compact(payload, db)
    elif hook_type == "Stop":
        capture_stop(payload, db)
    # Session lifecycle
    elif hook_type == "SessionStart:startup":
        capture_session_start(payload, db, "startup")
    elif hook_type == "SessionStart:resume":
        capture_session_start(payload, db, "resume")
    elif hook_type == "SessionStart:clear":
        capture_session_start(payload, db, "clear")
    elif hook_type == "SessionStart:compact":
        capture_session_start(payload, db, "compact")
    elif hook_type == "SessionEnd":
        capture_session_end(payload, db)
    # Agent and permission tracking
    elif hook_type == "SubagentStop":
        capture_subagent_stop(payload, db)
    elif hook_type == "PermissionRequest":
        capture_permission_request(payload, db)
    elif hook_type == "Notification":
        capture_notification(payload, db)
    else:
        log.warning("unknown_hook", hook_type=hook_type)


def main() -> None:
    """CLI entry point for hook processing."""
    if len(sys.argv) < 2:
        log.error("usage_error", message="reclaude.capture <hook_type>")
        sys.exit(1)

    hook_type = sys.argv[1]
    log.debug("hook_invoked", hook_type=hook_type)

    try:
        raw_input = sys.stdin.read()
        payload = json.loads(raw_input) if raw_input.strip() else {}
    except json.JSONDecodeError as e:
        log.warning("malformed_json", hook_type=hook_type, error=str(e))
        sys.exit(0)

    try:
        process_hook(hook_type, payload)
    except Exception as e:
        log.exception("hook_error", hook_type=hook_type, error=str(e))

    sys.exit(0)


if __name__ == "__main__":
    main()
