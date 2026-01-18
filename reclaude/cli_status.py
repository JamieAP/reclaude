from __future__ import annotations

import argparse
import json

from .db import CaptureDB


def cmd_status(args: argparse.Namespace) -> int:
    db = CaptureDB()
    counts = db.event_counts_by_type()
    total = db.count_events()

    print(f"Database: {db.path}")
    print(f"Total events: {total}")
    print()
    print("By type:")
    for event_type, count in sorted(counts.items(), key=lambda x: -x[1]):
        print(f"  {event_type}: {count}")

    return 0


def cmd_sessions(args: argparse.Namespace) -> int:
    db = CaptureDB()
    sessions = db.get_sessions_with_info()

    if not sessions:
        print("No sessions found")
        return 0

    sessions = sessions[: args.limit]

    if args.json:
        payload = [
            {
                "session_id": sid,
                "last_timestamp": ts.isoformat(),
                "event_count": cnt,
                "cwd": cwd,
            }
            for sid, ts, cnt, cwd in sessions
        ]
        print(json.dumps(payload, indent=2, default=str))
        return 0

    for sid, ts, cnt, cwd in sessions:
        ts_str = ts.strftime("%Y-%m-%d %H:%M:%S")
        project = (cwd or "").rstrip("/").rsplit("/", 1)[-1] if cwd else ""
        suffix = f" {cwd}" if cwd else ""
        label = f"{project} " if project else ""
        print(f"[{ts_str}] {sid} ({cnt} events) {label}".rstrip() + suffix)

    return 0
