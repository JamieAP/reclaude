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


def _format_compact(event: SemanticEvent) -> str:
    """Compact single-line format for fzf preview panes."""
    from .cli_utils import _relative_time

    DIM = "\033[2m"
    CYAN = "\033[36m"
    GREEN = "\033[32m"
    RED = "\033[31m"
    YELLOW = "\033[33m"
    BOLD = "\033[1m"
    RST = "\033[0m"

    age = _relative_time(event.timestamp)
    meta = event.metadata or {}
    etype = event.event_type.value if hasattr(event.event_type, "value") else str(event.event_type)

    if etype == "file_diff":
        file_path = meta.get("file_path", "?")
        # Just the filename
        fname = file_path.rsplit("/", 1)[-1]
        op = meta.get("operation", "edit")
        added = meta.get("lines_added", 0)
        removed = meta.get("lines_removed", 0)
        return f"{DIM}{age:>8s}{RST}  {YELLOW}{op:<6s}{RST} {BOLD}{fname}{RST} {GREEN}+{added}{RST}/{RED}-{removed}{RST}"

    if etype == "tool_use":
        tool = meta.get("tool_name", "?")
        success = meta.get("success", True)
        dot = f"{GREEN}✓{RST}" if success else f"{RED}✗{RST}"
        # Extract meaningful preview from content
        content = event.content
        preview = ""
        if "--- INPUT ---" in content:
            inp = content.split("--- INPUT ---")[1]
            if "--- OUTPUT ---" in inp:
                inp = inp.split("--- OUTPUT ---")[0]
            inp = inp.strip()
            # For Bash, show the command; for others, show a short summary
            if tool == "Bash":
                import json as _json
                try:
                    data = _json.loads(inp)
                    cmd = data.get("command", data.get("description", ""))
                    desc = data.get("description", "")
                    preview = desc or cmd[:60]
                except (ValueError, AttributeError):
                    preview = inp[:60].replace("\n", " ")
            elif tool == "Edit":
                import json as _json2
                try:
                    data = _json2.loads(inp)
                    fp = data.get("file_path", "")
                    fname = fp.rsplit("/", 1)[-1] if fp else ""
                    preview = fname
                except (ValueError, AttributeError):
                    preview = inp[:60].replace("\n", " ")
            elif tool == "Read":
                import json as _json3
                try:
                    data = _json3.loads(inp)
                    fp = data.get("file_path", "")
                    preview = fp.rsplit("/", 1)[-1] if fp else ""
                except (ValueError, AttributeError):
                    preview = inp[:60].replace("\n", " ")
            else:
                # Strip JSON wrapper noise for other tools
                inp_clean = inp.replace("\n", " ").strip()
                if inp_clean.startswith("{"):
                    import json as _json4
                    try:
                        data = _json4.loads(inp_clean)
                        # Show first string value as preview
                        for v in data.values():
                            if isinstance(v, str) and v:
                                preview = v[:60]
                                break
                    except (ValueError, AttributeError):
                        pass
                if not preview:
                    preview = inp_clean[:60]
        if not preview:
            preview = content[:60].replace("\n", " ").strip()
        return f"{DIM}{age:>8s}{RST}  {dot} {CYAN}{tool:<8s}{RST} {DIM}{preview}{RST}"

    if etype == "user_prompt":
        preview = event.content[:100].replace("\n", " ").strip()
        return f"{DIM}{age:>8s}{RST}  {BOLD}▶ {preview}{RST}"

    if etype == "plan":
        preview = event.content[:100].replace("\n", " ").strip()
        return f"{DIM}{age:>8s}{RST}  {YELLOW}◆{RST} {preview}"

    # Fallback for compaction, notification, etc.
    preview = event.content[:80].replace("\n", " ").strip()
    label = etype.replace("_", " ")
    return f"{DIM}{age:>8s}{RST}  {DIM}{label}: {preview}{RST}"


def _tool_key(event: SemanticEvent) -> str | None:
    """Return tool name if this is a tool_use event, else None."""
    etype = event.event_type.value if hasattr(event.event_type, "value") else str(event.event_type)
    if etype == "tool_use":
        return (event.metadata or {}).get("tool_name", "?")
    return None


def _format_compact_rolled(events: list[SemanticEvent], count: int) -> str:
    """Format a rolled-up group of same-tool events."""
    from .cli_utils import _relative_time

    DIM = "\033[2m"
    CYAN = "\033[36m"
    GREEN = "\033[32m"
    RED = "\033[31m"
    RST = "\033[0m"

    last = events[-1]
    age = _relative_time(last.timestamp)
    tool = (last.metadata or {}).get("tool_name", "?")
    all_ok = all((e.metadata or {}).get("success", True) for e in events)
    dot = f"{GREEN}✓{RST}" if all_ok else f"{RED}✗{RST}"
    return f"{DIM}{age:>8s}{RST}  {dot} {CYAN}{tool:<8s}{RST} {DIM}×{count}{RST}"


def _print_compact_rolled(events: list[SemanticEvent]) -> None:
    """Print compact timeline with consecutive same-tool events rolled up."""
    i = 0
    while i < len(events):
        key = _tool_key(events[i])
        if key is None:
            print(_format_compact(events[i]))
            i += 1
            continue

        # Collect consecutive events with same tool
        group = [events[i]]
        j = i + 1
        while j < len(events) and _tool_key(events[j]) == key:
            group.append(events[j])
            j += 1

        if len(group) >= 3:
            print(_format_compact_rolled(group, len(group)))
        else:
            for e in group:
                print(_format_compact(e))
        i = j


def _get_config_for_event(event: SemanticEvent) -> EventTypeConfig | None:
    """Get the EventTypeConfig for a given event based on its event_type."""
    event_type_str = event.event_type.value if hasattr(event.event_type, "value") else str(event.event_type)
    for config in EVENT_TYPE_CONFIG.values():
        db_type_str = config.db_type.value if hasattr(config.db_type, "value") else str(config.db_type)
        if db_type_str == event_type_str:
            return config
    return None


def _fzf_events(events: list[SemanticEvent]) -> int:
    """Interactive event browser with fzf."""
    import shutil
    import subprocess

    if not shutil.which("fzf"):
        print("fzf not found in PATH", file=sys.stderr)
        return 1

    DIM = "\033[2m"
    CYAN = "\033[36m"
    RST = "\033[0m"

    reclaude = f"{sys.executable} -m reclaude.cli"

    lines = []
    for e in events:
        ts = e.timestamp.strftime("%m-%d %H:%M")
        etype = e.event_type.value if hasattr(e.event_type, "value") else str(e.event_type)
        preview = e.content.replace("\n", " ")[:100]
        line = f"{e.id}\t{DIM}{ts}{RST}  {CYAN}{etype:16s}{RST}  {DIM}{preview}{RST}"
        lines.append(line)

    fzf_input = "\n".join(lines)

    preview_cmd = f"{reclaude} chat {{1}} --no-pager 2>/dev/null"

    color_scheme = ",".join([
        "bg+:#1a1a2e", "fg+:#e0e0e0", "hl:#56b6c2", "hl+:#56b6c2",
        "pointer:#c678dd", "marker:#98c379", "border:#3b3b5c",
        "header:#888888", "info:#555555", "prompt:#c678dd",
        "gutter:#0e0e1a", "preview-bg:#0e0e1a",
    ])

    try:
        result = subprocess.run(
            ["fzf", "--ansi",
             "--delimiter=\t", "--with-nth=2..",
             "--height=80%", "--reverse",
             "--preview", preview_cmd,
             "--preview-window=right,55%,wrap,border-left",
             f"--color={color_scheme}",
             "--border=rounded", "--margin=1,2", "--padding=1,0",
             "--header=  events  \u21b5 open chat  esc quit",
             "--header-first"],
            input=fzf_input, capture_output=True, text=True,
        )
    except FileNotFoundError:
        print("fzf not found in PATH", file=sys.stderr)
        return 1

    if result.returncode != 0:
        return 0

    selected = result.stdout.strip()
    if not selected:
        return 0

    event_id = selected.split("\t")[0]
    parts = reclaude.split()
    subprocess.run([parts[0], *parts[1:], "chat", event_id])
    return 0


def cmd_events(args: argparse.Namespace) -> int:
    """Show recent events with optional type filtering and semantic search.

    Supports:
    - No --type: all events (original behavior)
    - --type prompt: only prompts
    - --type prompt,plan: prompts and plans
    - --type tool --semantic "query": semantic search over tools
    """
    db = CaptureDB()

    # Single event by ID
    event_id = args.id
    if event_id is not None:
        event = db.get_event_by_id(event_id)
        if not event:
            print(f"No event with id {event_id}", file=sys.stderr)
            return 2
        if args.json:
            print(json.dumps(_event_to_dict(event), indent=2, default=str))
        else:
            print(f"[{event.timestamp}] {event.event_type} (id={event.id}, session={_short_sid(event.session_id)})\n")
            print(event.content)
        return 0

    session_id = _session_filter(db, args.session)

    # CWD scoping: filter to current directory unless --all
    cwd = None if args.all else (args.cwd or os.getcwd())

    # Parse --type argument
    type_arg = args.type
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
    semantic_query = args.semantic
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
            cwd=cwd,
        )
    elif requested_types:
        # Multiple types: query all, filter post-query
        db_types = {EVENT_TYPE_CONFIG[t].db_type for t in requested_types}
        db_type_values = {
            dt.value if hasattr(dt, "value") else str(dt)
            for dt in db_types
        }
        # Query more than needed since we'll filter
        events = db.query_events(limit=limit * 3, session_id=session_id, cwd=cwd)
        events = [
            e
            for e in events
            if (e.event_type.value if hasattr(e.event_type, "value") else str(e.event_type))
            in db_type_values
        ][:limit]
    else:
        # No type filter: all events
        events = db.query_events(limit=limit, session_id=session_id, cwd=cwd)

    get_logger().info(
        "query_events",
        count=len(events),
        limit=limit,
        types=requested_types or "all",
    )

    if not events:
        print(empty_message)
        return 0

    # fzf interactive mode
    if args.fzf:
        return _fzf_events(list(reversed(events)))

    # JSON output
    if args.json:
        print(json.dumps([_event_to_dict(e, full=args.full) for e in reversed(events)], indent=2, default=str))
        return 0

    # Compact output (for fzf preview panes)
    if args.compact:
        DIM = "\033[2m"
        BOLD = "\033[1m"
        RST = "\033[0m"

        # Show session path as header if filtering by session
        if session_id:
            from .cli_status import _short_path
            start_cwd = db._get_session_start_cwd(session_id)
            cwd = start_cwd or db._get_session_cwd(session_id)
            if cwd:
                print(f"{BOLD}{_short_path(cwd)}{RST}")
                print()

        # Filter out user_prompts from main timeline (shown separately below)
        timeline_events = [
            e for e in reversed(events)
            if (e.event_type.value if hasattr(e.event_type, "value") else str(e.event_type))
            != "user_prompt"
        ]
        _print_compact_rolled(timeline_events)

        # Bottom section: last 4 user prompts
        if session_id:
            prompts = db.query_events(
                event_type="user_prompt", session_id=session_id, limit=4
            )
            if prompts:
                print()
                print(f"{DIM}{'─' * 44}{RST}")
                print(f"{DIM} prompts{RST}")
                print(f"{DIM}{'─' * 44}{RST}")
                for p in reversed(prompts):
                    print(_format_compact(p))

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
    if not args.all:
        ctx = get_git_context(args.cwd or os.getcwd())
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
