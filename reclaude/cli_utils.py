from __future__ import annotations

import sys
from datetime import datetime, timedelta, timezone

from .cli_constants import SID_NONE
from .db import CaptureDB


def _short_sid(session_id: str | None) -> str:
    sid = (session_id or "").strip()
    return sid[:8] if sid else SID_NONE


def _relative_time(dt: datetime) -> str:
    now = datetime.now(timezone.utc)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    delta = now - dt
    secs = int(delta.total_seconds())
    if secs < 60:
        return f"{secs}s ago"
    mins = secs // 60
    if mins < 60:
        return f"{mins}m ago"
    hours = mins // 60
    if hours < 24:
        return f"{hours}h ago"
    days = hours // 24
    return f"{days}d ago"


def _parse_since(value: str) -> datetime:
    now = datetime.now(timezone.utc)
    v = value.strip()
    if v == "1h":
        return now - timedelta(hours=1)
    if v == "24h":
        return now - timedelta(hours=24)
    if v == "7d":
        return now - timedelta(days=7)
    if v == "30d":
        return now - timedelta(days=30)

    # ISO-8601 (accept "Z")
    try:
        v = v.replace("Z", "+00:00")
        dt = datetime.fromisoformat(v)
    except ValueError as e:
        raise ValueError(f"Invalid --since value: {value!r}") from e

    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt


def _resolve_session(db: CaptureDB, value: str | None) -> str | None:
    """Resolve a session id/prefix, or choose the latest session when value is None."""
    sessions = db.get_sessions_with_info()
    if not sessions:
        return None

    if value is None or value.strip().lower() in {"latest", "@latest"}:
        return sessions[0][0]

    needle = value.strip()
    matches = [sid for sid, _, _, _ in sessions if sid == needle or sid.startswith(needle)]
    if len(matches) == 1:
        return matches[0]

    if len(matches) == 0:
        print(f"No session matches: {value!r}", file=sys.stderr)
        print("Tip: run `reclaude sessions` to list session ids.", file=sys.stderr)
        return None

    print(f"Ambiguous session prefix {value!r} matches:", file=sys.stderr)
    for sid in matches[:10]:
        print(f"  {sid}", file=sys.stderr)
    return None


def _session_filter(db: CaptureDB, session_arg: str | None) -> str | None:
    if session_arg is None:
        return None
    session_id = _resolve_session(db, session_arg)
    if not session_id:
        raise SystemExit(2)
    return session_id


def _event_to_dict(event, full: bool = False) -> dict:
    content = event.content if full else event.content[:1000]
    return {
        "id": event.id,
        "timestamp": event.timestamp.isoformat(),
        "event_type": event.event_type,
        "session_id": event.session_id,
        "content": content,
        "metadata": event.metadata,
    }
