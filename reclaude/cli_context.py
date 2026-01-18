from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime

from .cli_utils import _parse_since, _resolve_session, _short_sid
from .db import CaptureDB, SemanticEventType


def cmd_context(args: argparse.Namespace) -> int:
    db = CaptureDB()
    session_id = _resolve_session(db, args.session)
    if not session_id:
        print("No session found", file=sys.stderr)
        return 2

    session_info = next(
        ((ts, cnt, cwd) for sid, ts, cnt, cwd in db.get_sessions_with_info() if sid == session_id),
        None,
    )
    cwd = session_info[2] if session_info else None

    since: datetime | None = None
    if args.since:
        try:
            since = _parse_since(args.since)
        except ValueError as e:
            print(str(e), file=sys.stderr)
            return 2

    compactions = db.query_events(
        event_type=SemanticEventType.COMPACTION,
        session_id=session_id,
        limit=200,
    )
    post_summaries = [
        e
        for e in compactions
        if e.metadata.get("phase") == "post" and e.metadata.get("subtype") == "summary"
    ]
    post_contexts = [
        e
        for e in compactions
        if e.metadata.get("phase") == "post" and e.metadata.get("subtype") == "context"
    ]

    latest_summary = post_summaries[0] if post_summaries else None
    latest_context = post_contexts[0] if post_contexts else None
    boundary = since or (latest_context.timestamp if latest_context else None) or (
        latest_summary.timestamp if latest_summary else None
    )

    events = db.query_events(
        session_id=session_id,
        since=boundary,
        limit=args.limit,
    )
    events_sorted = sorted(
        (e for e in events if e.event_type != SemanticEventType.COMPACTION),
        key=lambda e: e.timestamp,
    )

    if args.format == "json":
        payload = {
            "session_id": session_id,
            "cwd": cwd,
            "since": boundary.isoformat() if boundary else None,
            "compaction": {
                "summary": {
                    "id": latest_summary.id,
                    "timestamp": latest_summary.timestamp.isoformat(),
                    "content": latest_summary.content,
                    "metadata": latest_summary.metadata,
                }
                if latest_summary
                else None,
                "context": {
                    "id": latest_context.id,
                    "timestamp": latest_context.timestamp.isoformat(),
                    "content": latest_context.content,
                    "metadata": latest_context.metadata,
                }
                if latest_context
                else None,
            },
            "events": [
                {
                    "id": e.id,
                    "timestamp": e.timestamp.isoformat(),
                    "event_type": e.event_type,
                    "session_id": e.session_id,
                    "content": e.content if args.full else e.content[:1000],
                    "metadata": e.metadata,
                }
                for e in events_sorted
            ],
        }
        print(json.dumps(payload, indent=2, default=str))
        return 0

    # Markdown-ish, LLM-friendly text
    print("# reclaude context\n")
    print(f"- session_id: {session_id}")
    if cwd:
        print(f"- cwd: {cwd}")
    print(f"- since: {boundary.isoformat() if boundary else 'start'}")

    if latest_summary:
        print("\n## compaction_summary\n")
        print(latest_summary.content.strip())

    if latest_context:
        print("\n## post_compaction_context\n")
        print(latest_context.content.strip())

    print("\n## events\n")
    for e in events_sorted:
        ts = e.timestamp.strftime("%Y-%m-%d %H:%M:%S")
        sid = _short_sid(e.session_id)

        if e.event_type == SemanticEventType.TOOL_USE and not args.full:
            meta = e.metadata or {}
            tool = meta.get("tool_name", "unknown")
            ok = meta.get("success", True)
            dur = meta.get("duration_ms")
            dur_str = f"{dur}ms" if dur is not None else "?"
            status = "ok" if ok else "FAIL"
            print(f"- [{ts}] {sid} tool_use {tool} ({status}, {dur_str})")
            continue

        if e.event_type == SemanticEventType.FILE_DIFF and not args.full:
            meta = e.metadata or {}
            file_path = meta.get("file_path", "")
            op = meta.get("operation", "?")
            added = meta.get("lines_added", 0)
            removed = meta.get("lines_removed", 0)
            print(f"- [{ts}] {sid} file_diff {op} {file_path} (+{added}/-{removed})")
            continue

        content = e.content if args.full else e.content[:400]
        content = content.replace("\n", " ").strip()
        if not args.full and len(e.content) > 400:
            content += "..."
        print(f"- [{ts}] {sid} {e.event_type}: {content}")

    return 0
