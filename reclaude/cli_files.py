"""Find files touched by Claude across sessions."""

from __future__ import annotations

import argparse
import json
import re
import signal
import sys
import time

from .cli_utils import _relative_time, _session_filter
from .db import CaptureDB, SemanticEventType


def _setup_pipe_handling() -> None:
    """Configure graceful handling of pipe closure (e.g., head, grep)."""
    # Ignore SIGPIPE - let write() raise BrokenPipeError instead
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)


# Tools that touch files (all have file_path parameter)
FILE_TOOLS = frozenset({"Read", "Write", "Edit", "NotebookEdit"})

# Polling interval for stream mode (seconds)
POLL_INTERVAL = 1.0


def _extract_file_path(content: str) -> str | None:
    """Extract file_path from tool_use event content.

    Content format:
        Tool: {name}
        Success: {bool}

        --- INPUT ---
        {json}

        --- OUTPUT ---
        ...
    """
    # Find the INPUT section
    match = re.search(r"--- INPUT ---\n(.+?)(?:\n--- OUTPUT ---|$)", content, re.DOTALL)
    if not match:
        return None

    input_json = match.group(1).strip()
    # Handle truncation marker
    if input_json.endswith("... (truncated)"):
        # Try to parse what we have
        input_json = input_json.rsplit("\n... (truncated)", 1)[0]

    try:
        tool_input = json.loads(input_json)
        return tool_input.get("file_path")
    except json.JSONDecodeError:
        return None


def cmd_files(args: argparse.Namespace) -> int:
    """Find files touched by Claude, sorted by last touch time."""
    db = CaptureDB()

    # Resolve session filter if provided
    session_id = None
    if args.session:
        session_id = _session_filter(db, args.session)

    # Query tool_use events
    events = db.query_events(
        event_type=SemanticEventType.TOOL_USE,
        session_id=session_id,
        limit=args.scan_limit,
    )

    # Stream mode: output paths as discovered, poll for new events
    if args.stream:
        _setup_pipe_handling()
        seen: set[str] = set()
        last_id = 0

        # Initial batch: get recent events sorted oldest-first for chronological output
        initial_events = list(events)
        initial_events.reverse()  # Convert DESC to ASC (chronological)

        try:
            # Output initial batch
            for event in initial_events:
                tool_name = event.metadata.get("tool_name", "")
                if tool_name not in FILE_TOOLS:
                    continue

                file_path = _extract_file_path(event.content)
                if not file_path or file_path in seen:
                    continue

                if args.pattern and args.pattern not in file_path:
                    continue

                seen.add(file_path)
                last_id = max(last_id, event.id)
                print(file_path, flush=True)

            # Track highest ID for incremental queries
            if initial_events:
                last_id = max(e.id for e in initial_events)

            # Poll loop for new events
            while True:
                time.sleep(POLL_INTERVAL)

                new_events = db.query_events_since_id(
                    since_id=last_id,
                    event_type=SemanticEventType.TOOL_USE,
                    limit=100,
                )

                for event in new_events:
                    last_id = max(last_id, event.id)

                    tool_name = event.metadata.get("tool_name", "")
                    if tool_name not in FILE_TOOLS:
                        continue

                    file_path = _extract_file_path(event.content)
                    if not file_path or file_path in seen:
                        continue

                    if args.pattern and args.pattern not in file_path:
                        continue

                    seen.add(file_path)
                    print(file_path, flush=True)

        except (BrokenPipeError, KeyboardInterrupt):
            pass
        return 0

    # Batch mode: aggregate then sort by last touch time
    file_touches: dict[str, dict] = {}

    for event in events:
        tool_name = event.metadata.get("tool_name", "")
        if tool_name not in FILE_TOOLS:
            continue

        file_path = _extract_file_path(event.content)
        if not file_path:
            continue

        # Apply pattern filter if specified
        if args.pattern and args.pattern not in file_path:
            continue

        if file_path not in file_touches:
            file_touches[file_path] = {
                "last_ts": event.timestamp,
                "sessions": set(),
                "tools": set(),
                "count": 0,
            }

        touch = file_touches[file_path]
        # Events come in DESC order, so first seen is most recent
        touch["sessions"].add(event.session_id)
        touch["tools"].add(tool_name)
        touch["count"] += 1

    if not file_touches:
        if args.pattern:
            print(f"No files matching '{args.pattern}' found", file=sys.stderr)
        else:
            print("No file touches found", file=sys.stderr)
        return 0

    # Sort by last touch time (most recent first)
    sorted_files = sorted(
        file_touches.items(),
        key=lambda x: x[1]["last_ts"],
        reverse=True,
    )

    # Apply limit
    sorted_files = sorted_files[: args.limit]

    # Output
    if args.json:
        output = []
        for path, info in sorted_files:
            output.append({
                "path": path,
                "last_touched": info["last_ts"].isoformat(),
                "sessions": len(info["sessions"]),
                "tools": sorted(info["tools"]),
                "count": info["count"],
            })
        print(json.dumps(output, indent=2))
    else:
        for path, info in sorted_files:
            rel_time = _relative_time(info["last_ts"])
            tools = ",".join(sorted(info["tools"]))
            sessions = len(info["sessions"])
            count = info["count"]

            if args.full:
                print(f"{path}")
                print(f"  {rel_time} | {count} touches | {sessions} sessions | {tools}")
            else:
                # Compact format: path (relative_time, count touches)
                print(f"{path}  ({rel_time}, {count}x)")

    return 0
