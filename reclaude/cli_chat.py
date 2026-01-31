from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile

from .cli_context import _find_latest_session_for_cwd
from .cli_utils import _relative_time, _session_filter
from .db import CaptureDB, SemanticEvent

DIM = "\033[2m"
RESET = "\033[0m"
CYAN = "\033[36m"
PURPLE = "\033[35m"
YELLOW = "\033[33m"
GREEN = "\033[32m"
BOLD = "\033[1m"
WHITE = "\033[37m"

TYPE_ICON = {
    "Bash": "λ", "Edit": "∂", "Write": "✎", "Read": "◉", "Glob": "⊛", "Grep": "/",
    "Task": "◆", "TaskCreate": "◆", "TaskUpdate": "◆", "TaskGet": "◆", "TaskList": "◆",
    "WebFetch": "⇣", "WebSearch": "⊕",
}

TARGET_MARKER = "«TARGET»"


def _tool_summary(tool: str, content: str) -> str:
    """One-line summary of a tool invocation."""
    # Extract input JSON from content
    marker = "--- INPUT ---"
    idx = content.find(marker)
    if idx < 0:
        return ""
    inp = content[idx + len(marker):]
    end = inp.find("--- ")
    if end > 0:
        inp = inp[:end]
    inp = inp.strip()

    try:
        d = json.loads(inp)
    except (json.JSONDecodeError, ValueError):
        return inp[:80]

    if tool == "Read":
        return d.get("file_path", "")
    if tool == "Write":
        path = d.get("file_path", "")
        size = len(d.get("content", ""))
        return f"{path} ({size} chars)"
    if tool == "Edit":
        return d.get("file_path", "")
    if tool == "Bash":
        return d.get("command", "")[:100]
    if tool == "Glob":
        return d.get("pattern", "")
    if tool == "Grep":
        return f'/{d.get("pattern", "")}/ {d.get("path", "")}'
    if tool in ("Task", "TaskCreate", "TaskUpdate"):
        return d.get("description", d.get("subject", ""))[:80]
    if tool == "WebFetch":
        return d.get("url", "")[:80]
    if tool == "WebSearch":
        return d.get("query", "")[:80]

    for v in d.values():
        if isinstance(v, str) and len(v) > 5:
            return v[:80]
    return ""


def _indent(text: str, prefix: str = "  ") -> str:
    return "\n".join(prefix + line for line in text.splitlines())


def _render_event(e: SemanticEvent, is_target: bool) -> list[str]:
    """Render a single event as chat lines."""
    ts = e.timestamp.strftime("%H:%M")
    meta = e.metadata if isinstance(e.metadata, dict) else {}
    marker = f" {TARGET_MARKER}" if is_target else ""
    highlight = YELLOW + BOLD if is_target else ""
    end_hl = RESET if is_target else ""
    arrow = "→ " if is_target else "  "

    etype = e.event_type
    lines: list[str] = []

    if etype == "user_prompt":
        lines.append(f"{arrow}{highlight}{GREEN}▹ You {DIM}({ts}){RESET}{end_hl}{marker}")
        lines.append(_indent(e.content.strip()))
        lines.append("")

    elif etype in ("assistant", "plan"):
        label = "Claude" if etype == "assistant" else "Claude [plan]"
        lines.append(f"{arrow}{highlight}{CYAN}▸ {label} {DIM}({ts}){RESET}{end_hl}{marker}")
        lines.append(_indent(e.content.strip()))
        lines.append("")

    elif etype == "thinking":
        n = len(e.content)
        lines.append(f"  {DIM}… [thinking, {n} chars]{RESET}")

    elif etype == "tool_use":
        tool = meta.get("tool_name", meta.get("tool", "?"))
        ok = meta.get("success", True)
        dur = meta.get("duration_ms")
        icon = TYPE_ICON.get(tool, "·")
        status = f"{GREEN}✓{RESET}" if ok else f"\033[31m✗{RESET}"
        dur_s = f" {DIM}{dur}ms{RESET}" if dur else ""
        summary = _tool_summary(tool, e.content)
        lines.append(f"  {status} {CYAN}{icon} {tool}{RESET}{dur_s} {DIM}{summary}{RESET}")

    elif etype == "file_diff":
        diff_lines = e.content.split("\n")
        header = next((l for l in diff_lines if l.startswith("+++") or l.startswith("---")), "")
        path = header.split(" ", 1)[1].strip() if " " in header else header
        adds = sum(1 for l in diff_lines if l.startswith("+") and not l.startswith("+++"))
        dels = sum(1 for l in diff_lines if l.startswith("-") and not l.startswith("---"))
        lines.append(f"  {DIM}± {path} +{adds}/-{dels}{RESET}")

    elif etype == "compaction":
        lines.append(f"  {DIM}⊘ [context compacted]{RESET}")

    elif etype == "session_start":
        cwd = meta.get("cwd", "")
        proj = cwd.rstrip("/").rsplit("/", 1)[-1] if cwd else ""
        lines.append(f"\n  {DIM}{'─' * 60}")
        lines.append(f"  ↳ session start {ts} {proj}")
        lines.append(f"  {'─' * 60}{RESET}\n")

    elif etype == "session_end":
        lines.append(f"\n  {DIM}↲ session end ({ts}){RESET}\n")

    elif etype in ("permission_request", "notification", "sys_msg"):
        # Collapse to one-liner
        preview = e.content.replace("\n", " ")[:80]
        lines.append(f"  {DIM}· {etype}: {preview}{RESET}")

    else:
        # Anything else: dim one-liner
        preview = e.content.replace("\n", " ")[:60]
        lines.append(f"  {DIM}· {etype}: {preview}{RESET}")

    return lines


def _fzf_chat_picker(db: CaptureDB) -> int:
    """Pick a session interactively with fzf, then open its chat."""
    sessions = db.get_sessions_with_info()
    if not sessions:
        print("No sessions found", file=sys.stderr)
        return 1

    # Filter to cwd
    here = os.getcwd()
    cwd_sessions = [s for s in sessions if (s[4] or s[3] or "").rstrip("/") == here.rstrip("/")]
    if cwd_sessions:
        sessions = cwd_sessions

    sessions = sessions[:50]

    if not shutil.which("fzf"):
        print("fzf not found in PATH", file=sys.stderr)
        return 1

    reclaude = f"{sys.executable} -m reclaude.cli"

    lines = []
    for sid, ts, cnt, cwd, start_cwd, is_active in sessions:
        short_id = sid.split("-", 1)[0]
        age = _relative_time(ts)
        project = (start_cwd or cwd or "").rstrip("/").rsplit("/", 1)[-1]
        dot = f"{GREEN}\u25cf{RESET}" if is_active else f"{DIM}\u25cb{RESET}"
        line = (
            f"{sid}\t"
            f"{DIM}{age:>8s}{RESET}  "
            f"{CYAN}{short_id}{RESET}  "
            f"{dot} {BOLD}{project:<18s}{RESET} "
            f"{DIM}{cnt:>5d}{RESET}"
        )
        lines.append(line)

    fzf_input = "\n".join(lines)
    preview_cmd = f"{reclaude} chat --session {{1}} --no-pager 2>/dev/null | head -60"

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
             "--header=  chat  \u21b5 open session  esc quit",
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

    session_id = selected.split("\t")[0]
    subprocess.run(
        f"{reclaude} chat --session {session_id}",
        shell=True,
    )
    return 0


def cmd_chat(args: argparse.Namespace) -> int:
    db = CaptureDB()

    if args.fzf:
        return _fzf_chat_picker(db)

    show_all = args.all
    session_arg = args.session
    event_id = args.event_id

    if not show_all and not event_id and not session_arg:
        # Default to latest session for cwd
        cwd_session = _find_latest_session_for_cwd(db, os.getcwd())
        if not cwd_session:
            print("No session found for current directory", file=sys.stderr)
            return 1
        session_arg = cwd_session

    CHAT_TYPES = ["user_prompt", "assistant", "plan"]

    if show_all:
        # All chat events across every session and repo
        events = db.query_events(event_type=CHAT_TYPES, limit=50_000)
        events.reverse()  # chronological
    elif session_arg:
        session_id = _session_filter(db, session_arg)
        if not session_id:
            return 1
        events = db.query_events(
            session_id=session_id,
            event_type=CHAT_TYPES,
            limit=50_000,
        )
        events.reverse()
    else:
        # Single session centered on target event
        target = db.get_event_by_id(event_id)
        if not target:
            print(f"No event with id {event_id}", file=sys.stderr)
            return 1
        if not target.session_id:
            print(f"Event {event_id} has no session_id", file=sys.stderr)
            return 1
        events = db.query_events(
            session_id=target.session_id,
            event_type=CHAT_TYPES,
            limit=50_000,
        )
        events.reverse()

    if not events:
        print("No events found", file=sys.stderr)
        return 1

    # Render events, tracking target line
    output_lines: list[str] = []
    target_line = 0
    prev_session = None

    for e in events:
        # In --all mode, insert session separators
        if show_all and e.session_id != prev_session:
            meta = e.metadata if isinstance(e.metadata, dict) else {}
            cwd = meta.get("cwd", "")
            proj = cwd.rstrip("/").rsplit("/", 1)[-1] if cwd else ""
            ts = e.timestamp.strftime("%Y-%m-%d %H:%M")
            if prev_session is not None:
                output_lines.append("")
            output_lines.append(f"  {DIM}{'─' * 60}")
            output_lines.append(f"  ↳ {proj or '?'} · {ts} · {(e.session_id or '?')[:12]}")
            output_lines.append(f"  {'─' * 60}{RESET}")
            output_lines.append("")
            prev_session = e.session_id

        is_target = event_id and e.id == event_id
        if is_target:
            target_line = len(output_lines) + 1

        rendered = _render_event(e, is_target)
        output_lines.extend(rendered)

    text = "\n".join(output_lines) + "\n"

    # Output
    no_pager = args.no_pager
    if no_pager or not sys.stdout.isatty():
        sys.stdout.write(text.replace(TARGET_MARKER, ""))
        return 0

    # Pager: write to temp file, open less at target line
    with tempfile.NamedTemporaryFile(mode="w", suffix=".ans", delete=False) as f:
        f.write(text.replace(TARGET_MARKER, ""))
        tmp = f.name

    less_args = ["less", "-R"]
    if target_line:
        less_args.append(f"+{target_line}g")
    else:
        less_args.append("+G")  # --all: start at bottom (most recent)

    try:
        subprocess.run(less_args + [tmp])
    except KeyboardInterrupt:
        pass

    return 0
