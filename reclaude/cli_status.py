from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import subprocess
import sys

from .cli_utils import _relative_time, _session_filter
from .db import CaptureDB


def _short_path(path: str) -> str:
    """Replace $HOME with ~ for display."""
    home = os.path.expanduser("~")
    if path.startswith(home):
        return "~" + path[len(home):]
    return path


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

    if getattr(args, "heal", False):
        from .capture import heal_orphaned_sessions
        healed = heal_orphaned_sessions(db)
        print(f"Healed {healed} orphaned sessions")
        if healed == 0:
            return 0

    sessions = db.get_sessions_with_info()

    if not sessions:
        print("No sessions found", file=sys.stderr)
        return 0

    # Filter to cwd by default, --all shows all directories
    if not getattr(args, "all", False):
        here = os.getcwd()
        sessions = [
            s for s in sessions
            if (s[4] or s[3] or "").rstrip("/") == here.rstrip("/")
        ]

    sessions = sessions[: args.limit]

    if getattr(args, "fzf", False):
        return _fzf_select(sessions)

    if args.json:
        payload = [
            {
                "session_id": sid,
                "last_timestamp": ts.isoformat(),
                "event_count": cnt,
                "cwd": cwd,
                "start_cwd": start_cwd,
                "is_active": is_active,
            }
            for sid, ts, cnt, cwd, start_cwd, is_active in sessions
        ]
        print(json.dumps(payload, indent=2, default=str))
        return 0

    for sid, ts, cnt, cwd, start_cwd, is_active in sessions:
        short_id = sid.split("-", 1)[0]
        age = _relative_time(ts)
        project = (start_cwd or cwd or "").rstrip("/").rsplit("/", 1)[-1]
        active_marker = " *" if is_active else ""
        dir_display = _short_path(start_cwd or cwd or "")
        print(f"{age:>8s} {short_id} ({cnt} events){active_marker} {project} {dir_display}".rstrip())

    return 0


def _fzf_select(sessions: list) -> int:
    """Pipe sessions to fzf with preview, output cd+cb command for selected session."""
    DIM = "\033[2m"
    CYAN = "\033[36m"
    BOLD = "\033[1m"
    GREEN = "\033[32m"
    RST = "\033[0m"

    lines = []
    session_map = {}  # short_id -> (full_sid, cwd)
    for sid, ts, cnt, cwd, start_cwd, is_active in sessions:
        short_id = sid.split("-", 1)[0]
        age = _relative_time(ts)
        project = (start_cwd or cwd or "").rstrip("/").rsplit("/", 1)[-1]
        dot = f"{GREEN}●{RST}" if is_active else f"{DIM}○{RST}"
        full_path = start_cwd or cwd or ""

        line = (
            f"{DIM}{age:>8s}{RST}  "
            f"{CYAN}{short_id}{RST}  "
            f"{dot} {BOLD}{project:<18s}{RST} "
            f"{DIM}{cnt:>5d}{RST}"
        )
        lines.append(line)
        session_map[short_id] = (sid, full_path)

    fzf_input = "\n".join(lines)

    # Use same Python to ensure --compact flag is available
    reclaude = f"{sys.executable} -m reclaude.cli"

    # Preview: events render immediately, summary appends async via temp file
    preview_cmd = (
        "SID=$(echo {} | sed 's/\\x1b\\[[0-9;]*m//g' | awk '{print $3}'); "
        "SFILE=/tmp/reclaude-summary-$SID; "
        # Show cached summary at top if available
        "if [ -f \"$SFILE\" ]; then "
        "echo \"\\033[1;35m\" ; cat \"$SFILE\"; echo \"\\033[0m\"; "
        "echo '\\033[2m───────────────────────────────\\033[0m'; echo; "
        "fi; "
        # Events (instant)
        f"{reclaude} events 20 --compact --session $SID; "
        # Fire off summary generation in background for next preview
        f"({reclaude} session-summary $SID > \"$SFILE\" 2>/dev/null &)"
    )

    # Dark theme: muted bg, cyan accents, dim borders
    color_scheme = ",".join([
        "bg+:#1a1a2e",        # selected row bg
        "fg+:#e0e0e0",        # selected row fg
        "hl:#56b6c2",         # highlight match
        "hl+:#56b6c2",        # highlight match (selected)
        "pointer:#c678dd",    # pointer arrow
        "marker:#98c379",     # marker
        "border:#3b3b5c",     # border lines
        "header:#888888",      # header text
        "info:#555555",        # match count
        "prompt:#c678dd",     # prompt
        "gutter:#0e0e1a",     # gutter bg
        "preview-bg:#0e0e1a", # preview bg
    ])

    try:
        result = subprocess.run(
            [
                "fzf", "--ansi",
                "--height=80%", "--reverse",
                "--preview", preview_cmd,
                "--preview-window=right,55%,wrap,border-left",
                f"--color={color_scheme}",
                "--border=rounded",
                "--margin=1,2",
                "--padding=1,0",
                "--header=  sessions",
                "--header-first",
            ],
            input=fzf_input,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        print("fzf not found in PATH", file=sys.stderr)
        return 1

    if result.returncode != 0:
        if result.stderr:
            print(result.stderr.strip(), file=sys.stderr)
        return 1

    selected = result.stdout.strip()
    if not selected:
        return 1

    # Strip ANSI and parse short ID (3rd field: age_val ago SHORT_ID ...)
    clean = re.sub(r"\x1b\[[0-9;]*m", "", selected)
    parts = clean.split()
    if len(parts) < 3:
        return 1
    short_id = parts[2]

    entry = session_map.get(short_id)
    if not entry:
        return 1
    full_sid, cwd = entry

    if cwd:
        print(f'cd {shlex.quote(cwd)} && cb --resume {full_sid}')
    else:
        print(f'cb --resume {full_sid}')

    return 0


def cmd_session_summary(args: argparse.Namespace) -> int:
    """Generate a Gemini summary of a session using the focus pipeline."""
    from datetime import timedelta

    from .cli_focus import _build_focus_content
    from .cli_gemini import _call_gemini_sdk

    db = CaptureDB()
    session_id = _session_filter(db, args.session_id)
    if not session_id:
        return 1

    # Fetch last 15 minutes of semantic events for this session
    events = db.query_events(session_id=session_id, limit=500)
    if not events:
        print("No events found")
        return 0

    # Scope to last 15 minutes of activity
    latest = max(e.timestamp for e in events)
    cutoff = latest - timedelta(minutes=15)
    recent = [e for e in events if e.timestamp >= cutoff]
    if not recent:
        recent = events[-20:]  # fallback: last 20 events

    content = _build_focus_content(recent, "15min")

    prompt = (
        "Summarize this Claude Code session in 2-3 sentences. "
        "Focus on WHAT was accomplished (features built, bugs fixed, files changed). "
        "Be specific and concise. No preamble.\n\n"
        "The input is structured with sections for code changes, tool use, "
        "plans, and assistant responses."
    )
    summary, ok = _call_gemini_sdk(
        "gemini-3-flash-preview",
        prompt,
        content,
        verbose=False,
        respect_rate_limit=False,
        max_output_tokens=300,
    )
    if ok:
        print(summary)
    else:
        print("(summary unavailable)")
    return 0
