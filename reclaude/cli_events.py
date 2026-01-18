from __future__ import annotations

import argparse
import json
import os
import sys
from typing import TYPE_CHECKING

from .cli_constants import EVENT_TYPE_CONFIG, EventTypeConfig
from .cli_utils import _event_to_dict, _session_filter, _short_sid
from .db import CaptureDB
from .log import get_logger

if TYPE_CHECKING:
    from .db import SemanticEvent


def _format_event_line(
    event: SemanticEvent,
    config: EventTypeConfig,
    full: bool = False,
) -> str:
    """Format a single event line for CLI output.

    Handles different event types with their specific formatting:
    - prompt/plan: [ts] sid preview
    - diff: [ts] sid op file_path (+added/-removed) - metadata only
    - tool: [ts] sid tool_name (status, duration) preview
    - compaction: [ts] sid (phase/subtype) preview

    Args:
        event: The semantic event to format
        config: EventTypeConfig for this event type
        full: If True, show full content instead of preview

    Returns:
        Formatted single-line string for display
    """
    ts = event.timestamp.strftime("%Y-%m-%d %H:%M:%S")
    sid = _short_sid(event.session_id)
    meta = event.metadata or {}

    # Determine event type key from config
    event_type_key = None
    for key, cfg in EVENT_TYPE_CONFIG.items():
        if cfg is config:
            event_type_key = key
            break

    # Handle diff events (metadata-only, no content preview)
    if event_type_key == "diff":
        file_path = meta.get("file_path", "unknown")
        op = meta.get("operation", "?")
        added = meta.get("lines_added", 0)
        removed = meta.get("lines_removed", 0)
        return f"[{ts}] {sid} {op} {file_path} (+{added}/-{removed})"

    # Handle tool events (metadata prefix + content)
    if event_type_key == "tool":
        tool_name = meta.get("tool_name", "unknown")
        success = meta.get("success", True)
        duration = meta.get("duration_ms")
        status = "ok" if success else "FAIL"
        duration_str = f"{duration}ms" if duration is not None else "?"

        if full:
            # Full mode: header line only, caller handles content separately
            return f"[{ts}] {sid} {tool_name} ({status}, {duration_str})"
        preview = event.content[:config.preview_len].replace("\n", " ")
        if len(event.content) > config.preview_len:
            preview += "..."
        return f"[{ts}] {sid} {tool_name} ({status}, {duration_str}) {preview}"

    # Handle compaction events (metadata prefix + variable preview length)
    if event_type_key == "compaction":
        phase = meta.get("phase", "?")
        subtype = meta.get("subtype", "")
        label = f"{phase}/{subtype}" if subtype else phase

        # Variable preview length: 500 for context, 100 otherwise
        preview_len = 500 if subtype == "context" else config.preview_len

        if full:
            return f"[{ts}] {sid} ({label})"
        preview = event.content[:preview_len].replace("\n", " ")
        if len(event.content) > preview_len:
            preview += "..."
        return f"[{ts}] {sid} ({label}) {preview}"

    # Handle prompt/plan events (simple preview)
    if full:
        return f"[{ts}] {sid}"
    preview = event.content[:config.preview_len].replace("\n", " ")
    if len(event.content) > config.preview_len:
        preview += "..."
    return f"[{ts}] {sid} {preview}"


def _get_config_for_event(event: SemanticEvent) -> EventTypeConfig | None:
    """Get the EventTypeConfig for a given event based on its event_type."""
    event_type_str = event.event_type.value if hasattr(event.event_type, "value") else str(event.event_type)
    for config in EVENT_TYPE_CONFIG.values():
        db_type_str = config.db_type.value if hasattr(config.db_type, "value") else str(config.db_type)
        if db_type_str == event_type_str:
            return config
    return None


def cmd_events(args: argparse.Namespace) -> int:
    """Show recent events with optional type filtering and semantic search.

    Supports:
    - No --type: all events (original behavior)
    - --type prompt: only prompts
    - --type prompt,plan: prompts and plans
    - --type tool --semantic "query": semantic search over tools
    """
    db = CaptureDB()
    session_id = _session_filter(db, args.session)

    # Parse --type argument
    type_arg = getattr(args, "type", None)
    requested_types: list[str] = []
    if type_arg:
        requested_types = [t.strip().lower() for t in type_arg.split(",") if t.strip()]
        # Validate type names
        invalid_types = [t for t in requested_types if t not in EVENT_TYPE_CONFIG]
        if invalid_types:
            valid_types = ", ".join(EVENT_TYPE_CONFIG.keys())
            print(f"Invalid event type(s): {', '.join(invalid_types)}", file=sys.stderr)
            print(f"Valid types: {valid_types}", file=sys.stderr)
            return 2

    # Check for semantic search mode
    semantic_query = getattr(args, "semantic", None)
    if semantic_query:
        if not requested_types:
            # Default to all types for semantic search
            semantic_types: tuple[str, ...] = tuple(
                st for cfg in EVENT_TYPE_CONFIG.values() for st in cfg.semantic_types
            )
        else:
            # Gather semantic types from requested type configs
            semantic_types = tuple(
                st
                for t in requested_types
                for st in EVENT_TYPE_CONFIG[t].semantic_types
            )
        return _cmd_events_semantic(db, args, semantic_query, semantic_types)

    # Determine limit and empty message
    if requested_types:
        if len(requested_types) == 1:
            config = EVENT_TYPE_CONFIG[requested_types[0]]
            default_limit = config.default_limit
            empty_message = config.empty_message
        else:
            # Multiple types: use max of their default limits
            default_limit = max(EVENT_TYPE_CONFIG[t].default_limit for t in requested_types)
            empty_message = "No events found"
    else:
        default_limit = 20
        empty_message = "No events found"

    # Use provided limit or fall back to default
    limit = args.limit if args.limit is not None else default_limit

    # Query events
    if len(requested_types) == 1:
        # Single type: use DB-level filtering
        config = EVENT_TYPE_CONFIG[requested_types[0]]
        events = db.query_events(
            event_type=config.db_type,
            limit=limit,
            session_id=session_id,
        )
    elif requested_types:
        # Multiple types: query all, filter post-query
        db_types = {EVENT_TYPE_CONFIG[t].db_type for t in requested_types}
        db_type_values = {
            dt.value if hasattr(dt, "value") else str(dt)
            for dt in db_types
        }
        # Query more than needed since we'll filter
        events = db.query_events(limit=limit * 3, session_id=session_id)
        events = [
            e
            for e in events
            if (e.event_type.value if hasattr(e.event_type, "value") else str(e.event_type))
            in db_type_values
        ][:limit]
    else:
        # No type filter: all events
        events = db.query_events(limit=limit, session_id=session_id)

    get_logger().info(
        "query_events",
        count=len(events),
        limit=limit,
        types=requested_types or "all",
    )

    if not events:
        print(empty_message)
        return 0

    # JSON output
    if args.json:
        print(json.dumps([_event_to_dict(e, full=args.full) for e in reversed(events)], indent=2, default=str))
        return 0

    # Human-readable output
    for event in reversed(events):
        config = _get_config_for_event(event)

        if config:
            # Use type-specific formatting
            line = _format_event_line(event, config, full=args.full)
            print(line)

            # Full mode: print content after header for types that support it
            if args.full and config.preview_len > 0:
                print(event.content)
                print()
            elif config.preview_len > 0:
                # Add spacing for readability (prompts, plans, compactions)
                event_type_key = None
                for key, cfg in EVENT_TYPE_CONFIG.items():
                    if cfg is config:
                        event_type_key = key
                        break
                if event_type_key in ("prompt", "plan"):
                    print()
                elif event_type_key == "compaction":
                    meta = event.metadata or {}
                    if meta.get("subtype") == "context":
                        print()
        else:
            # Fallback for unknown event types
            ts = event.timestamp.strftime("%Y-%m-%d %H:%M:%S")
            etype = event.event_type.value if hasattr(event.event_type, "value") else str(event.event_type)
            preview = event.content[:200].replace("\n", " ")
            if len(event.content) > 200:
                preview += "..."
            print(f"[{ts}] {_short_sid(event.session_id)} {etype}: {preview}")

    return 0


def _cmd_events_semantic(
    db: CaptureDB,
    args: argparse.Namespace,
    query: str,
    event_types: tuple[str, ...],
) -> int:
    """Unified semantic search over events using Gemini embeddings.

    Args:
        db: Database connection
        args: Parsed CLI arguments (expects --all, --full, --json, --limit)
        query: Search query string
        event_types: Tuple of event type strings to search (e.g., ("user_prompt",))

    Returns:
        Exit code (0 for success)
    """
    from reclaude.embeddings import embed_text
    from reclaude.git import get_git_context

    # Find config for these event types (for preview_len and empty_message)
    config: EventTypeConfig | None = None
    for cfg in EVENT_TYPE_CONFIG.values():
        if cfg.semantic_types == event_types:
            config = cfg
            break

    # Get context filter (unless --all)
    remote_url = None
    context_label = ""
    if not getattr(args, "all", False):
        ctx = get_git_context(os.getcwd())
        remote_url = ctx.get("remote_url")
        if remote_url:
            repo_name = remote_url.rstrip("/").split("/")[-1].replace(".git", "")
            context_label = f" (in {repo_name})"

    # Build search label based on event types
    type_label = "/".join(event_types)
    print(f"Searching {type_label} for: {query}{context_label}", file=sys.stderr)

    query_embedding = embed_text(query)

    results = db.query_events_semantic(
        query_embedding=query_embedding,
        event_types=event_types,
        limit=args.limit,
        remote_url=remote_url,
    )

    get_logger().info(
        "semantic_event_search",
        query=query,
        event_types=event_types,
        results=len(results),
    )

    if not results:
        if config:
            # Use config empty message, modified for semantic context
            print(f"No matching {config.empty_message.lower().replace('no ', '')}")
        else:
            print("No matching events found")
        return 0

    # JSON output
    if args.json:
        payload = []
        for event, dist in results:
            etype = event.event_type.value if hasattr(event.event_type, "value") else event.event_type
            entry = {
                "id": event.id,
                "distance": round(dist, 4),
                "similarity": round(1 - dist, 4),
                "timestamp": event.timestamp.isoformat(),
                "session_id": event.session_id,
                "event_type": etype,
                "content": event.content if args.full else event.content[:500],
            }
            # Include relevant metadata based on event type
            meta = event.metadata or {}
            # Compaction-specific fields
            if "phase" in meta:
                entry["phase"] = meta.get("phase")
            if "subtype" in meta:
                entry["subtype"] = meta.get("subtype")
            # Tool-specific fields
            if "tool_name" in meta:
                entry["tool_name"] = meta.get("tool_name")
            if "success" in meta:
                entry["success"] = meta.get("success")
            if "duration_ms" in meta:
                entry["duration_ms"] = meta.get("duration_ms")
            # Diff-specific fields
            if "file_path" in meta:
                entry["file_path"] = meta.get("file_path")
            if "operation" in meta:
                entry["operation"] = meta.get("operation")
            if "lines_added" in meta:
                entry["lines_added"] = meta.get("lines_added")
            if "lines_removed" in meta:
                entry["lines_removed"] = meta.get("lines_removed")
            payload.append(entry)
        print(json.dumps(payload, indent=2, default=str))
        return 0

    # Human-readable output with similarity scores
    for event, dist in results:
        ts = event.timestamp.strftime("%Y-%m-%d %H:%M:%S")
        score = f"[{1 - dist:.2f}]"  # Convert distance to similarity score
        etype = event.event_type.value if hasattr(event.event_type, "value") else event.event_type
        meta = event.metadata or {}

        # Build type/metadata label based on event type
        label_parts = []

        # Plan events: show plan vs plan_file
        if etype in ("plan", "plan_file"):
            label_parts.append(etype)

        # Compaction events: show phase/subtype
        if meta.get("phase"):
            phase = meta.get("phase", "?")
            subtype = meta.get("subtype", "")
            label_parts.append(f"{phase}/{subtype}" if subtype else phase)

        # Tool events: show tool name and status
        if meta.get("tool_name"):
            tool_name = meta.get("tool_name", "unknown")
            success = meta.get("success", True)
            duration = meta.get("duration_ms")
            status = "ok" if success else "FAIL"
            duration_str = f"{duration}ms" if duration is not None else "?"
            label_parts.append(f"{tool_name} ({status}, {duration_str})")

        # Diff events: show file info
        if meta.get("file_path"):
            file_path = meta.get("file_path", "unknown")
            op = meta.get("operation", "?")
            added = meta.get("lines_added", 0)
            removed = meta.get("lines_removed", 0)
            label_parts.append(f"{op} {file_path} (+{added}/-{removed})")

        label = f"({', '.join(label_parts)})" if label_parts else ""

        if args.full:
            print(f"{score} [{ts}] {_short_sid(event.session_id)} {label}".rstrip())
            print(event.content)
            print()
        else:
            # Determine preview length from config or use default
            preview_len = 200
            if config and config.preview_len > 0:
                preview_len = config.preview_len
            # Special case: compaction context events get more space
            if meta.get("subtype") == "context":
                preview_len = 500

            preview = event.content[:preview_len].replace("\n", " ")
            if len(event.content) > preview_len:
                preview += "..."
            print(f"{score} [{ts}] {_short_sid(event.session_id)} {label} {preview}".rstrip())
            print()

    return 0
