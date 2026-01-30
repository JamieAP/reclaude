from __future__ import annotations

import argparse
import json
import os
import re
import sys
from datetime import datetime, timedelta

from .cli_utils import _parse_since, _resolve_session, _short_sid
from .db import CaptureDB, SemanticEventType

DEFAULT_BUDGET = 16000

# Max gap between session_end and session_start to consider them linked
CHAIN_THRESHOLD = timedelta(seconds=5)


def _squeeze(text: str) -> str:
    """Compress text for token efficiency without losing meaning."""
    # Strip decorative unicode (box drawing, ★, bullets, emoji-style)
    text = re.sub(r"[─═┄┈━┃│┌┐└┘├┤┬┴┼╔╗╚╝╠╣╦╩╬★●○◆◇▶▷►▸•·…→←↑↓⇒⇐✓✗✔✘]+", "", text)
    # Strip lines that are only backticks/whitespace (with optional "Insight")
    text = re.sub(r"^[`\s]*Insight[`\s]*$", "", text, flags=re.MULTILINE)
    text = re.sub(r"^`[`\s]*$", "", text, flags=re.MULTILINE)
    # Strip markdown table separators
    text = re.sub(r"^\|[-: |]+\|$", "", text, flags=re.MULTILINE)
    # Strip code fences (keep content)
    text = re.sub(r"^```\w*\s*$", "", text, flags=re.MULTILINE)
    # Strip bold/italic markers (content stays)
    text = re.sub(r"\*{1,2}([^*]+)\*{1,2}", r"\1", text)
    # Collapse task notifications to one-liners
    text = re.sub(
        r"<task-notification>\s*<task-id>([^<]+)</task-id>.*?<status>([^<]+)</status>\s*<summary>([^<]+)</summary>.*?</task-notification>\s*(?:Read the output[^\n]*)?",
        r"[task \1: \2 - \3]",
        text,
        flags=re.DOTALL,
    )
    # Collapse whitespace runs
    text = re.sub(r"\n{3,}", "\n\n", text)
    text = re.sub(r"[ \t]{2,}", " ", text)
    return text.strip()


def _budget_events(
    events: list, budget: int
) -> list[tuple]:
    """Walk events newest-first, fitting within a character budget.

    Returns list of (event, content, truncated) tuples in chronological order.
    User prompts get full content; assistant responses get squeezed
    and truncated, with older events truncated more aggressively.
    """
    if budget <= 0:
        return [(e, _squeeze(e.content), False) for e in events]

    # Newest-first pass
    reversed_events = list(reversed(events))
    selected: list[tuple] = []
    remaining = budget

    for i, e in enumerate(reversed_events):
        if remaining <= 0:
            break

        squeezed = _squeeze(e.content)

        truncated = False
        if e.event_type == SemanticEventType.USER_PROMPT:
            # User prompts: usually short. Cap at 500 to handle pastes.
            if len(squeezed) > 500:
                content = squeezed[:500]
                truncated = True
            else:
                content = squeezed
        else:
            # Assistant: cap based on recency. Recent = more, older = less.
            cap = max(200, remaining // (i + 1))
            if len(squeezed) > cap:
                content = squeezed[:cap]
                truncated = True
            else:
                content = squeezed

        remaining -= len(content)
        selected.append((e, content, truncated))

    selected.reverse()
    return selected


def _find_latest_session_for_cwd(db: CaptureDB, cwd: str) -> str | None:
    """Find the most recent session that has events in this cwd."""
    sessions = db.get_sessions_with_info()
    for sid, _ts, _cnt, session_cwd, start_cwd, _active in sessions:
        # Match on either current or start cwd
        if session_cwd == cwd or (start_cwd and start_cwd == cwd):
            return sid
        if session_cwd and session_cwd.startswith(cwd + "/"):
            return sid
        if start_cwd and start_cwd.startswith(cwd + "/"):
            return sid
    return None


def _cwds_match(meta_a: dict, meta_b: dict) -> bool:
    """Check if two event metadata dicts refer to the same project."""
    cwd_a = meta_a.get("cwd") or meta_a.get("repo_root")
    cwd_b = meta_b.get("cwd") or meta_b.get("repo_root")
    if cwd_a and cwd_b:
        if (cwd_a == cwd_b
                or cwd_a.startswith(cwd_b + "/")
                or cwd_b.startswith(cwd_a + "/")):
            return True
    # Fallback: same transcript project directory
    tp_a = meta_a.get("transcript_path", "")
    tp_b = meta_b.get("transcript_path", "")
    if tp_a and tp_b and tp_a.rsplit("/", 1)[0] == tp_b.rsplit("/", 1)[0]:
        return True
    return False


def _find_session_chain(db: CaptureDB, session_id: str) -> list[dict]:
    """Walk both directions from session_id, finding sessions linked by clear transitions.

    Returns a list of dicts (oldest first):
        [{"session_id": ..., "link": ..., "gap_ms": ...}, ...]
    The target session has link="target".
    """
    chain = [{"session_id": session_id, "link": "target", "gap_ms": 0}]

    # --- Walk backwards (find predecessors) ---
    current_sid = session_id
    for _ in range(20):
        starts = db.query_events(
            event_type=SemanticEventType.SESSION_START,
            session_id=current_sid,
            limit=100,
        )
        if not starts:
            break

        earliest_start = min(starts, key=lambda e: e.timestamp)
        if earliest_start.metadata.get("trigger") != "clear":
            break

        # Find a session_end just before this start
        window_start = earliest_start.timestamp - CHAIN_THRESHOLD
        candidates = db.query_events(
            event_type=SemanticEventType.SESSION_END,
            since=window_start,
            limit=50,
        )

        predecessor = None
        best_gap = CHAIN_THRESHOLD
        for e in candidates:
            if e.session_id == current_sid:
                continue
            gap = earliest_start.timestamp - e.timestamp
            if timedelta(0) <= gap < best_gap and _cwds_match(e.metadata, earliest_start.metadata):
                predecessor = e
                best_gap = gap

        if not predecessor:
            break

        gap_ms = int(best_gap.total_seconds() * 1000)
        chain.insert(0, {"session_id": predecessor.session_id, "link": f"-> clear ({gap_ms}ms)", "gap_ms": gap_ms})
        current_sid = predecessor.session_id

    # --- Walk forwards (find successors) ---
    current_sid = session_id
    for _ in range(20):
        ends = db.query_events(
            event_type=SemanticEventType.SESSION_END,
            session_id=current_sid,
            limit=100,
        )
        if not ends:
            break

        latest_end = max(ends, key=lambda e: e.timestamp)

        # Find a session_start(clear) just after this end
        window_end = latest_end.timestamp + CHAIN_THRESHOLD
        candidates = db.query_events(
            event_type=SemanticEventType.SESSION_START,
            since=latest_end.timestamp,
            limit=50,
        )
        # Filter to within window
        candidates = [e for e in candidates if e.timestamp <= window_end]

        successor = None
        best_gap = CHAIN_THRESHOLD
        for e in candidates:
            if e.session_id == current_sid:
                continue
            if e.metadata.get("trigger") != "clear":
                continue
            gap = e.timestamp - latest_end.timestamp
            if timedelta(0) <= gap < best_gap and _cwds_match(e.metadata, latest_end.metadata):
                successor = e
                best_gap = gap

        if not successor:
            break

        gap_ms = int(best_gap.total_seconds() * 1000)
        chain.append({"session_id": successor.session_id, "link": f"<- clear ({gap_ms}ms)", "gap_ms": gap_ms})
        current_sid = successor.session_id

    return chain


def cmd_context(args: argparse.Namespace) -> int:
    db = CaptureDB()
    cwd = os.getcwd()

    since: datetime | None = None
    if args.since:
        try:
            since = _parse_since(args.since)
        except ValueError as e:
            print(str(e), file=sys.stderr)
            return 2

    # Explicit session or latest for this cwd
    if args.session:
        session_id = _resolve_session(db, args.session)
    else:
        session_id = _find_latest_session_for_cwd(db, cwd)
    if not session_id:
        print("No session found", file=sys.stderr)
        return 2

    # Determine session list (single or chained)
    chain_mode = args.chain
    if chain_mode:
        chain = _find_session_chain(db, session_id)
        session_ids = [link["session_id"] for link in chain]
    else:
        chain = None
        session_ids = [session_id]

    # Collect events across all sessions in the chain
    event_types = [SemanticEventType.USER_PROMPT, SemanticEventType.ASSISTANT]
    all_events = []
    for sid in session_ids:
        events = db.query_events(
            event_type=event_types,
            session_id=sid,
            since=since,
            limit=args.limit,
        )
        all_events.extend(events)

    if not all_events:
        print("No events found", file=sys.stderr)
        return 2

    events_sorted = sorted(all_events, key=lambda e: e.timestamp)
    budget = 0 if args.full else args.budget
    selected = _budget_events(events_sorted, budget)

    if args.format == "json":
        payload = {
            "cwd": cwd,
            "session_id": session_id,
            "chain": chain,
            "since": since.isoformat() if since else None,
            "budget": budget,
            "events": [
                {
                    "id": e.id,
                    "timestamp": e.timestamp.isoformat(),
                    "event_type": e.event_type,
                    "session_id": e.session_id,
                    "content": content,
                    "metadata": e.metadata,
                }
                for e, content, truncated in selected
            ],
        }
        print(json.dumps(payload, indent=2, default=str))
        return 0

    # Markdown conversation transcript
    sid_short = _short_sid(session_id)
    has_truncated = any(t for _, _, t in selected)
    if has_truncated:
        print("_truncated events: use `reclaude events --id <id>` for full content_\n")

    # Chain stitching header
    if chain and len(chain) > 1:
        print(f"## session chain ({len(chain)} sessions)\n")
        for link in chain:
            link_short = _short_sid(link["session_id"])
            if link["link"] == "target":
                print(f"  {link_short} (target)")
            else:
                print(f"  {link_short} {link['link']}")
        print()

    print(f"# reclaude context – {cwd} [{sid_short}]\n")
    if since:
        print(f"_since {since.isoformat()}_\n")

    prev_sid = None
    for e, content, truncated in selected:
        ts = e.timestamp.strftime("%H:%M:%S")
        role = "user" if e.event_type == SemanticEventType.USER_PROMPT else "assistant"

        # Mark session boundaries in chained output
        if chain and len(chain) > 1 and e.session_id != prev_sid:
            e_short = _short_sid(e.session_id)
            print(f"--- session {e_short} ---\n")
            prev_sid = e.session_id

        print(f"### [{ts}] {role}\n")
        print(content)
        if truncated:
            print(f"… [e/{e.id}]")
        print()

    return 0
