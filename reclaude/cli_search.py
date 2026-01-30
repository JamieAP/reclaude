from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile

from .db import CaptureDB

DIM = "\033[2m"
RESET = "\033[0m"
CYAN = "\033[36m"
PURPLE = "\033[35m"
YELLOW = "\033[33m"
GREEN = "\033[32m"
RED = "\033[31m"
BOLD = "\033[1m"
WHITE = "\033[37m"


def _clean_content(event_type: str, content: str, meta: dict, verbose: bool) -> str:
    """Extract human-readable content from an event."""
    if event_type == "tool_use":
        tool = meta.get("tool", "?")
        ok = meta.get("success", True)
        dur = meta.get("duration_ms")
        dur_s = f" {dur}ms" if dur else ""
        status = "✓" if ok else "✗"

        # Parse the content JSON for input/output
        try:
            # tool_use content is structured: Tool: X ... --- INPUT --- {...} --- OUTPUT --- ...
            inp = _extract_section(content, "INPUT")
            out = _extract_section(content, "OUTPUT")
        except Exception:
            inp, out = "", ""

        summary = _tool_summary(tool, inp)
        if verbose:
            lines = [f"{status}{dur_s} {summary}"]
            if out and not ok:
                lines.append(f"  {DIM}{out[:300]}{RESET}")
            return "\n".join(lines)
        return f"{status}{dur_s} {summary}"

    if event_type == "file_diff":
        # Show file path and change summary
        lines = content.split("\n")
        header = next((l for l in lines if l.startswith("---") or l.startswith("+++")), "")
        path = header.split(" ", 1)[1] if " " in header else header
        adds = sum(1 for l in lines if l.startswith("+") and not l.startswith("+++"))
        dels = sum(1 for l in lines if l.startswith("-") and not l.startswith("---"))
        if verbose:
            return f"{path}  +{adds}/-{dels}\n{content}"
        return f"{path}  +{adds}/-{dels}"

    if event_type in ("assistant", "user_prompt", "plan", "plan_file"):
        if verbose:
            return content
        return content.replace("\n", " ")[:200]

    if event_type in ("session_start", "session_end"):
        return content.replace("\n", " ")[:120]

    # Default: strip noise
    if verbose:
        return content
    return content.replace("\n", " ")[:200]


def _extract_section(content: str, section: str) -> str:
    marker = f"--- {section} ---"
    idx = content.find(marker)
    if idx < 0:
        return ""
    start = idx + len(marker)
    end = content.find("--- ", start)
    return content[start:end].strip() if end > 0 else content[start:].strip()


def _tool_summary(tool: str, inp: str) -> str:
    """One-line summary of what a tool did."""
    if not inp:
        return ""
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
        cmd = d.get("command", "")
        return cmd[:100]
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

    # Generic: show first string value
    for v in d.values():
        if isinstance(v, str) and len(v) > 5:
            return v[:80]
    return ""


def _fzf_mode(results: list) -> int:
    """Open search results in fzf with chat preview, enter opens full chat."""
    if not shutil.which("fzf"):
        print("fzf not found in PATH", file=sys.stderr)
        return 1

    reclaude = shutil.which("reclaude") or "reclaude"

    # Build fzf input: "ID\tvisible line" - fzf shows everything, we extract ID on select
    lines = []
    for e in results:
        ts = e.timestamp.strftime("%m-%d %H:%M")
        sid = (e.session_id or "")[:8]
        preview = e.content.replace("\n", " ")[:120]
        etype = e.event_type
        # ID is first field, tab-separated
        lines.append(f"{e.id}\t{ts}  {sid}  {etype:12s}  {preview}")

    fzf_input = "\n".join(lines)

    # fzf: preview runs reclaude chat on the selected event ID
    # --with-nth=2.. hides the ID column from display
    # --preview extracts field 1 (the ID) and passes to reclaude chat
    # enter: print the selected ID so we can open full chat after
    try:
        proc = subprocess.run(
            [
                "fzf",
                "--ansi",
                "--delimiter=\t",
                "--with-nth=2..",
                f"--preview={reclaude} chat {{1}} --no-pager",
                "--preview-window=right:60%:wrap",
                "--bind=enter:accept",
                "--header=↵ open chat  esc quit",
            ],
            input=fzf_input,
            capture_output=True,
            text=True,
        )
    except KeyboardInterrupt:
        return 0

    if proc.returncode != 0:
        return 0  # user pressed esc

    # Extract event ID from selected line
    selected = proc.stdout.strip()
    if not selected:
        return 0

    event_id = selected.split("\t")[0]
    # Open full chat in pager
    subprocess.run([reclaude, "chat", event_id])
    return 0


def cmd_search(args: argparse.Namespace) -> int:
    db = CaptureDB()

    if args.rebuild:
        print("Rebuilding FTS index...", end=" ", flush=True)
        n = db.rebuild_fts()
        print(f"done ({n} rows)")
        if not args.query:
            return 0

    parts = [" ".join(args.query)]
    for t in args.and_terms:
        parts.append(f"AND {t}")
    for t in args.or_terms:
        parts.append(f"OR {t}")
    for t in args.not_terms:
        parts.append(f"NOT {t}")
    query = " ".join(parts)
    event_type = args.event_type
    # "chat" is a shortcut for conversation types
    if event_type == "chat":
        event_type = ["user_prompt", "assistant", "plan"]
    limit = args.limit

    if args.all:
        cwd = None
    else:
        cwd = args.cwd or os.getcwd()

    try:
        results = db.search_events(query, event_type=event_type, cwd=cwd, limit=limit)
    except Exception as e:
        if "no such table" in str(e):
            print("FTS index not built yet. Run: reclaude search --rebuild", file=sys.stderr)
            return 1
        raise

    if not results:
        print("No results.")
        return 0

    verbose = args.verbose
    use_fzf = args.fzf

    if use_fzf:
        return _fzf_mode(results)

    # Type icons for visual scanning
    TYPE_ICON = {
        "Bash": "λ", "Edit": "∂", "Write": "✎", "Read": "◉", "Glob": "⊛", "Grep": "/",
        "Task": "◆", "TaskCreate": "◆", "TaskUpdate": "◆",
        "WebFetch": "⇣", "WebSearch": "⊕",
        "assistant": "▸", "user_prompt": "▹", "plan": "◈", "plan_file": "◈",
        "file_diff": "±", "thinking": "…", "compaction": "⊘",
        "session_start": "↳", "session_end": "↲",
    }

    # Prepare rows
    rows = []
    for e in results:
        ts = e.timestamp.strftime("%m-%d %H:%M")
        sid = (e.session_id or "")[:8]
        meta = json.loads(e.metadata) if isinstance(e.metadata, str) else (e.metadata or {})
        if not meta.get("tool") and meta.get("tool_name"):
            meta["tool"] = meta["tool_name"]
        cleaned = _clean_content(e.event_type, e.content, meta, verbose)

        etype = e.event_type
        if etype == "tool_use":
            etype = meta.get("tool", "tool_use")

        icon = TYPE_ICON.get(etype, "·")
        rows.append((ts, sid, icon, etype, cleaned, e.id))

    # Measure columns
    try:
        tw = os.get_terminal_size().columns
    except OSError:
        tw = 120
    type_w = max(len(r[3]) for r in rows) if rows else 10
    fixed = 10 + 2 + 8 + 2 + 2 + type_w + 3  # ts + gaps + sid + gaps + icon+space + type + sep
    content_w = max(tw - fixed - 4, 20)  # 4 for borders + padding

    # Header
    hdr_type = "TYPE".ljust(type_w)
    hdr_content = "CONTENT"
    print(f"\n  {DIM}{'TIME':10s}  {'SESSION':8s}  {'  ' + hdr_type}   {hdr_content}{RESET}")
    print(f"  {DIM}{'─' * 10}  {'─' * 8}  {'─' * (type_w + 2)}  {'─' * min(content_w, 50)}{RESET}")

    for ts, sid, icon, etype, cleaned, eid in rows:
        if verbose:
            preview = cleaned
        else:
            preview = cleaned[:content_w]

        print(f"  {DIM}{ts:10s}{RESET}  {PURPLE}{sid:8s}{RESET}  {CYAN}{icon} {etype:{type_w}s}{RESET}   {preview}")

        if verbose and "\n" in cleaned:
            print(f"  {DIM}{'':10s}  {'':8s}  {'':>{type_w + 2}s}{RESET}   {DIM}id={eid}{RESET}")

    print(f"\n  {DIM}{'─' * min(tw - 4, 70)}")
    print(f"  {len(results)} result(s) for \"{query}\"{RESET}\n")
    return 0
