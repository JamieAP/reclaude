from __future__ import annotations

import argparse
import os
import subprocess
from pathlib import Path

from .cli_constants import LOG_FILE
from .db import CaptureDB, DEFAULT_DB_PATH


def cmd_log(args: argparse.Namespace) -> int:
    if not LOG_FILE.exists():
        print(f"Log file not found: {LOG_FILE}")
        return 1

    import json
    import sys

    LEVEL_COLORS = {
        "debug": "\033[2m",      # dim
        "info": "\033[36m",      # cyan
        "warning": "\033[33m",   # yellow
        "error": "\033[31m",     # red
    }
    RESET = "\033[0m"
    DIM = "\033[2m"

    def fmt_line(raw: str) -> str:
        try:
            d = json.loads(raw)
        except (json.JSONDecodeError, ValueError):
            return raw.rstrip()

        ts = d.pop("timestamp", "")[:19]  # trim to seconds
        level = d.pop("level", "info")
        event = d.pop("event", "?")
        session = d.pop("session", "")

        color = LEVEL_COLORS.get(level, "")
        parts = [f"{DIM}{ts}{RESET}"]
        if session:
            parts.append(f"\033[35m{session}{RESET}")
        parts.append(f"{color}{level.upper():7s} {event}{RESET}")

        # remaining keys as key=val
        extras = " ".join(f"{k}={v}" for k, v in d.items())
        if extras:
            parts.append(f"{DIM}{extras}{RESET}")

        return " ".join(parts)

    tail_args = [str(getattr(args, "lines", 20))]
    try:
        proc = subprocess.Popen(
            ["tail", "-n", tail_args[0], "-f", str(LOG_FILE)],
            stdout=subprocess.PIPE,
            text=True,
        )
        for line in proc.stdout:
            sys.stdout.write(fmt_line(line) + "\n")
            sys.stdout.flush()
    except KeyboardInterrupt:
        pass
    finally:
        if proc.poll() is None:
            proc.terminate()

    return 0


def cmd_path(args: argparse.Namespace) -> int:
    db = CaptureDB()
    override = os.environ.get("RECLAUDE_DB_PATH")
    if override:
        print(f"Database: {db.path} (from RECLAUDE_DB_PATH)")
    else:
        print(f"Database: {DEFAULT_DB_PATH}")
    print(f"Log: {LOG_FILE}")
    return 0


def cmd_ui(args: argparse.Namespace) -> int:
    return _run_api_server(args)


def _run_api_server(args: argparse.Namespace) -> int:
    import uvicorn

    host = getattr(args, "host", "127.0.0.1")
    port = getattr(args, "port", 8420)
    open_browser = getattr(args, "open", True)
    reload = getattr(args, "reload", False)

    # Check if frontend is built
    frontend_dist = Path(__file__).parent / "frontend" / "dist"
    has_spa = frontend_dist.exists() and (frontend_dist / "index.html").exists()

    print(f"Starting reclaude {'web UI' if has_spa else 'API'} at http://{host}:{port}")
    if has_spa:
        print("API docs available at /docs")
    else:
        print("Frontend not built. Run: cd reclaude/frontend && npm run build")
        print(f"API docs available at http://{host}:{port}/docs")

    if open_browser:
        import threading
        import webbrowser

        def open_after_delay():
            import time

            time.sleep(1)
            # Open SPA root if available, otherwise API docs
            url = f"http://{host}:{port}" if has_spa else f"http://{host}:{port}/docs"
            webbrowser.open(url)

        threading.Thread(target=open_after_delay, daemon=True).start()

    uvicorn.run(
        "reclaude.api:app",
        host=host,
        port=port,
        reload=reload,
    )
    return 0
