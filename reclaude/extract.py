"""
Transcript extraction for semantic events.

Extracts SUMMARY, ASSISTANT, PLAN, THINKING events from Claude Code transcripts.
FILE_DIFF is extracted inline via PostToolUse hook (not here).
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterator

from .db import CaptureDB, SemanticEventType
from .git import get_git_context
from .log import get_logger

log = get_logger()

# Content limits
MAX_PROMPT_CONTENT = 10000
MAX_ASSISTANT_CONTENT = 50000


def _content_hash(entry: dict) -> str:
    """Generate a short hash from entry content for deduplication."""
    # Use type + content-ish fields for uniqueness
    parts = [
        entry.get("type", ""),
        entry.get("summary", ""),  # for summary entries
        entry.get("timestamp", ""),
    ]
    # For user/assistant, include message content
    msg = entry.get("message", {})
    if isinstance(msg, dict):
        content = msg.get("content", "")
        if isinstance(content, str):
            parts.append(content[:500])
        elif isinstance(content, list) and content:
            # First block preview
            first = content[0]
            if isinstance(first, dict):
                parts.append(str(first.get("text", first.get("thinking", "")))[:500])

    combined = "|".join(str(p) for p in parts)
    return hashlib.sha256(combined.encode()).hexdigest()[:16]


@dataclass
class ExtractResult:
    """Result of extracting from a single transcript."""
    transcript_path: str
    entries_scanned: int
    events_created: int
    by_type: dict[str, int]
    errors: list[str]


@dataclass
class BatchResult:
    """Result of batch extraction across multiple transcripts."""
    transcripts_processed: int
    total_events: int
    by_type: dict[str, int]
    errors: list[str]


class TranscriptExtractor:
    """Extracts semantic events from Claude Code transcripts."""

    def __init__(self, db: CaptureDB):
        self.db = db
        # Cache of known dedup_keys for this transcript (loaded per-transcript)
        self._known_dedup_keys: set[str] = set()

    def extract_transcript(self, path: Path, force_full: bool = False) -> ExtractResult:
        """Extract events from a single transcript, resuming from last position.

        Args:
            path: Path to transcript JSONL file
            force_full: If True, ignore scan state and process from beginning

        Returns:
            ExtractResult with counts and any errors
        """
        path = Path(path)
        result = ExtractResult(
            transcript_path=str(path),
            entries_scanned=0,
            events_created=0,
            by_type={},
            errors=[],
        )

        if not path.exists():
            result.errors.append(f"File not found: {path}")
            return result

        # Get scan state
        start_offset = 0
        if not force_full:
            state = self.db.get_scan_state(str(path))
            if state:
                start_offset = state.get("last_byte_offset", 0)

        # Parse entries from offset
        entries = list(self._parse_entries(path, start_offset))
        result.entries_scanned = len(entries)

        if not entries:
            return result

        # Extract session_id from first entry with one
        session_id = None
        for e in entries:
            if e.get("sessionId"):
                session_id = e["sessionId"]
                break

        # Get git context from first entry's cwd
        cwd = None
        for e in entries:
            if e.get("cwd"):
                cwd = e["cwd"]
                break
        git_ctx = get_git_context(cwd) if cwd else {}

        # Group assistant entries by msg_id for PLAN detection
        msg_groups = self._group_by_msg_id(entries)

        # Load existing dedup keys for this transcript to avoid re-inserting
        self._load_dedup_keys(str(path))

        # Process each entry
        last_uuid = None
        for i, entry in enumerate(entries):
            uuid = entry.get("uuid") or entry.get("leafUuid")
            last_uuid = uuid or last_uuid

            # Generate dedup key: prefer uuid, fall back to content hash
            entry_key = uuid if uuid else f"hash:{_content_hash(entry)}"

            # Check for duplicate (already extracted)
            if self._is_duplicate(str(path), entry_key):
                continue

            event = self._extract_entry(entry, entries, i, msg_groups, session_id, git_ctx, str(path), entry_key)
            if event:
                event_type_str = event["event_type"]
                self.db.insert_event(**event)
                # Add to cache to prevent duplicates within same batch
                self._known_dedup_keys.add(f"{path}:{entry_key}")
                result.events_created += 1
                result.by_type[event_type_str] = result.by_type.get(event_type_str, 0) + 1

        # Update scan state
        file_size = path.stat().st_size
        self.db.update_scan_state(
            transcript_path=str(path),
            last_byte_offset=file_size,
            last_entry_uuid=last_uuid,
            entries_extracted=result.events_created,
        )

        return result

    def extract_all(self, since_hours: int = 24, force_full: bool = False) -> BatchResult:
        """Extract from all transcripts modified in last N hours.

        Args:
            since_hours: Only process transcripts modified within this window
            force_full: If True, reprocess from beginning (ignore scan state)

        Returns:
            BatchResult with aggregate counts
        """
        from .cli_transcripts import discover_transcripts

        result = BatchResult(
            transcripts_processed=0,
            total_events=0,
            by_type={},
            errors=[],
        )

        cutoff = datetime.now(timezone.utc).timestamp() - (since_hours * 3600)
        transcripts = discover_transcripts()

        for info in transcripts:
            # Check modification time
            try:
                mtime = info.path.stat().st_mtime
                if mtime < cutoff:
                    continue
            except OSError:
                continue

            extract_result = self.extract_transcript(info.path, force_full=force_full)
            result.transcripts_processed += 1
            result.total_events += extract_result.events_created
            result.errors.extend(extract_result.errors)

            for etype, count in extract_result.by_type.items():
                result.by_type[etype] = result.by_type.get(etype, 0) + count

        return result

    def _parse_entries(self, path: Path, start_offset: int) -> Iterator[dict]:
        """Stream-parse JSONL from byte offset."""
        try:
            with open(path, "rb") as fb:
                if start_offset > 0:
                    # Check if we're at a line boundary (previous char is newline)
                    fb.seek(start_offset - 1)
                    prev_char = fb.read(1)
                    at_line_start = prev_char == b"\n"
                else:
                    at_line_start = True

            with open(path, "r", encoding="utf-8", errors="replace") as f:
                if start_offset > 0:
                    f.seek(start_offset)
                    # Only skip partial line if we landed mid-line
                    if not at_line_start:
                        f.readline()

                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        yield json.loads(line)
                    except json.JSONDecodeError:
                        continue
        except IOError as e:
            log.warning("transcript_read_error", path=str(path), error=str(e))

    def _group_by_msg_id(self, entries: list[dict]) -> dict[str, list[int]]:
        """Group assistant entry indices by API message ID."""
        groups: dict[str, list[int]] = {}
        for i, entry in enumerate(entries):
            if entry.get("type") == "assistant":
                msg_id = entry.get("message", {}).get("id")
                if msg_id:
                    if msg_id not in groups:
                        groups[msg_id] = []
                    groups[msg_id].append(i)
        return groups

    def _load_dedup_keys(self, transcript_path: str) -> None:
        """Load existing dedup keys for this transcript into cache."""
        self._known_dedup_keys.clear()
        # Only query events that have a dedup_key starting with this transcript path
        prefix = f"{transcript_path}:"
        with self.db.connection() as conn:
            rows = conn.execute(
                """
                SELECT json_extract(metadata, '$.dedup_key') as dedup_key
                FROM semantic_events
                WHERE json_extract(metadata, '$.dedup_key') LIKE ?
                """,
                (prefix + "%",)
            ).fetchall()
            for row in rows:
                if row["dedup_key"]:
                    self._known_dedup_keys.add(row["dedup_key"])

    def _is_duplicate(self, transcript_path: str, entry_uuid: str) -> bool:
        """Check if we've already extracted this entry (uses in-memory cache)."""
        dedup_key = f"{transcript_path}:{entry_uuid}"
        return dedup_key in self._known_dedup_keys

    def _extract_entry(
        self,
        entry: dict,
        all_entries: list[dict],
        index: int,
        msg_groups: dict[str, list[int]],
        session_id: str | None,
        git_ctx: dict,
        transcript_path: str,
        entry_key: str,
    ) -> dict | None:
        """Extract a semantic event from a transcript entry."""
        entry_type = entry.get("type")
        uuid = entry.get("uuid") or entry.get("leafUuid")
        timestamp = entry.get("timestamp")

        # Parse timestamp
        ts = None
        if timestamp:
            try:
                ts = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
            except ValueError:
                ts = datetime.now(timezone.utc)
        else:
            ts = datetime.now(timezone.utc)

        base_metadata = {
            "dedup_key": f"{transcript_path}:{entry_key}",
            "transcript_path": transcript_path,
            "transcript_uuid": uuid,
            **git_ctx,
        }

        # SUMMARY extraction
        if entry_type == "summary":
            summary_text = entry.get("summary", "")
            if summary_text:
                return {
                    "event_type": SemanticEventType.COMPACTION,
                    "content": summary_text,
                    "session_id": session_id,
                    "timestamp": ts,
                    "metadata": {
                        **base_metadata,
                        "phase": "post",
                        "subtype": "summary",
                        "leaf_uuid": entry.get("leafUuid"),
                    },
                }

        # USER_PROMPT extraction
        elif entry_type == "user":
            if entry.get("isMeta"):
                return None  # Skip meta messages

            message = entry.get("message", {})
            content = message.get("content", "") if isinstance(message, dict) else str(message)

            # Skip tool results (content is a list or starts with JSON array)
            if isinstance(content, list):
                return None
            if isinstance(content, str) and content.strip().startswith("[{"):
                return None

            # Skip very short or empty
            if not content or len(content.strip()) < 5:
                return None

            return {
                "event_type": SemanticEventType.USER_PROMPT,
                "content": content[:MAX_PROMPT_CONTENT],  # Truncate
                "session_id": session_id,
                "timestamp": ts,
                "metadata": {
                    **base_metadata,
                    "prompt_length": len(content),
                    "truncated": len(content) > MAX_PROMPT_CONTENT,
                },
            }

        # ASSISTANT extraction (includes THINKING and PLAN detection)
        elif entry_type == "assistant":
            message = entry.get("message", {})
            content_blocks = message.get("content", [])
            msg_id = message.get("id")

            if not content_blocks or not isinstance(content_blocks, list):
                return None

            # Get block types in this entry
            block_types = [b.get("type") for b in content_blocks if isinstance(b, dict)]

            # Extract thinking content (if any)
            thinking_text = ""
            if "thinking" in block_types:
                for block in content_blocks:
                    if block.get("type") == "thinking":
                        thinking_text += block.get("thinking", "") + "\n"
                thinking_text = thinking_text.strip()

            # Extract text content (if any)
            text_content = ""
            if "text" in block_types:
                for block in content_blocks:
                    if block.get("type") == "text":
                        text_content += block.get("text", "") + "\n"
                text_content = text_content.strip()

            # Priority: TEXT (could be PLAN) > THINKING-only
            # If both present, return TEXT with thinking noted in metadata
            if text_content:
                # Check if this is a PLAN (text followed by tool_use in same msg_id)
                is_plan = False
                if msg_id and msg_id in msg_groups:
                    indices = msg_groups[msg_id]
                    my_pos = indices.index(index) if index in indices else -1
                    if my_pos >= 0:
                        # Check subsequent entries in same msg_id
                        for later_idx in indices[my_pos + 1:]:
                            later_entry = all_entries[later_idx]
                            later_blocks = later_entry.get("message", {}).get("content", [])
                            later_types = [b.get("type") for b in later_blocks if isinstance(b, dict)]
                            if "tool_use" in later_types:
                                is_plan = True
                                break

                event_type = SemanticEventType.PLAN if is_plan else SemanticEventType.ASSISTANT

                return {
                    "event_type": event_type,
                    "content": text_content[:MAX_ASSISTANT_CONTENT],
                    "session_id": session_id,
                    "timestamp": ts,
                    "metadata": {
                        **base_metadata,
                        "msg_id": msg_id,
                        "is_plan": is_plan,
                        "has_thinking": bool(thinking_text),
                    },
                }

            # Thinking without text (thinking-only or thinking + tool_use)
            elif thinking_text:
                return {
                    "event_type": SemanticEventType.THINKING,
                    "content": thinking_text[:MAX_ASSISTANT_CONTENT],
                    "session_id": session_id,
                    "timestamp": ts,
                    "metadata": {
                        **base_metadata,
                        "msg_id": msg_id,
                        "has_tool_use": "tool_use" in block_types,
                    },
                }

        return None
