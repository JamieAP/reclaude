#!/usr/bin/env python3
"""
CLI for querying reclaude captured events.

Usage:
    reclaude status          Show capture statistics
    reclaude sessions [N]    List recent sessions
    reclaude events [N]      Show recent events (default 20)
    reclaude events --type prompt,plan  Filter by event types
    reclaude files [pattern] Find files touched by Claude
    reclaude context         Emit LLM-ready session context
    reclaude learn <content> Store a learning note
    reclaude learnings [N]   List stored learnings
    reclaude focus [scale]   Synthesize focus summary via Gemini
    reclaude log             Tail the capture log
    reclaude path            Show database path
    reclaude ui              Launch web UI server
"""

from __future__ import annotations

import argparse
import sys

from .cli_context import cmd_context
from .cli_events import cmd_events
from .cli_focus import cmd_focus
from .cli_learnings import cmd_learn, cmd_learnings
from .cli_files import cmd_files
from .cli_server import cmd_log, cmd_path, cmd_ui
from .cli_status import cmd_sessions, cmd_status
from .cli_transcripts import cmd_transcripts


def main() -> int:
    parser = argparse.ArgumentParser(
        description="reclaude CLI - query captured Claude Code events",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # status
    subparsers.add_parser("status", help="Show capture statistics")

    # sessions
    sp_sessions = subparsers.add_parser("sessions", help="List recent sessions")
    sp_sessions.add_argument("limit", type=int, nargs="?", default=10, help="Number of sessions")
    sp_sessions.add_argument("--json", action="store_true", help="Output as JSON")

    # events
    sp_events = subparsers.add_parser("events", help="Show recent events")
    sp_events.add_argument("limit", type=int, nargs="?", default=None, help="Number of events (default varies by type)")
    sp_events.add_argument("--session", "-s", help="Filter by session ID (prefix match)")
    sp_events.add_argument("--type", "-t", metavar="TYPES", help="Filter by event type(s): prompt,diff,plan,tool,compaction")
    sp_events.add_argument("--json", action="store_true", help="Output as JSON")
    sp_events.add_argument("--full", action="store_true", help="Show full content")
    sp_events.add_argument("--semantic", metavar="QUERY", help="Semantic search query")
    sp_events.add_argument("--all", action="store_true", help="Search all repos (not just current)")

    # files - files touched by Claude
    sp_files = subparsers.add_parser("files", help="Find files touched by Claude")
    sp_files.add_argument("pattern", nargs="?", help="Filter by file path substring")
    sp_files.add_argument("--session", "-s", help="Filter by session ID (prefix match)")
    sp_files.add_argument("--limit", "-n", type=int, default=50, help="Max files to show (default 50)")
    sp_files.add_argument("--scan-limit", type=int, default=5000, help="Max events to scan (default 5000)")
    sp_files.add_argument("--json", action="store_true", help="Output as JSON")
    sp_files.add_argument("--full", action="store_true", help="Show detailed info per file")
    sp_files.add_argument("--stream", action="store_true", help="Stream paths live, poll for new (Ctrl+C to stop)")

    # context
    sp_context = subparsers.add_parser("context", help="Emit LLM-ready session context")
    sp_context.add_argument("--session", "-s", help="Session ID (default: latest)")
    sp_context.add_argument("--since", help="Start from timestamp (1h, 24h, 7d, or ISO-8601)")
    sp_context.add_argument("--format", choices=["text", "json"], default="text", help="Output format")
    sp_context.add_argument("--full", action="store_true", help="Include full event content")
    sp_context.add_argument("limit", type=int, nargs="?", default=100, help="Max events to include")

    # learn
    sp_learn = subparsers.add_parser("learn", help="Store a learning note")
    sp_learn.add_argument("content", nargs="?", help="Learning content (or pipe via stdin)")
    sp_learn.add_argument("--session", "-s", help="Associate with session ID")

    # learnings
    sp_learnings = subparsers.add_parser("learnings", help="List stored learnings")
    sp_learnings.add_argument("limit", type=int, nargs="?", default=10, help="Number of learnings")
    sp_learnings.add_argument("--json", action="store_true", help="Output as JSON")
    sp_learnings.add_argument("--full", action="store_true", help="Show full content")
    sp_learnings.add_argument("--semantic", metavar="QUERY", help="Semantic search query")
    sp_learnings.add_argument("--all", action="store_true", help="Search all repos (not just current)")
    sp_learnings.add_argument("--human", action="store_true", help="Human-readable semantic output")
    sp_learnings.add_argument("--extract", metavar="SESSION", help="Extract learnings from session")
    sp_learnings.add_argument("--commit", action="store_true", help="Save extracted learnings")

    # focus
    p_focus = subparsers.add_parser("focus", help="Synthesize focus summary via Gemini")
    p_focus.add_argument(
        "scale",
        nargs="?",
        default="hour",
        choices=["hour", "8hour", "day", "week"],
        help="Time window: hour (1h), 8hour (8h), day (24h), week (7d)",
    )
    p_focus.add_argument("--model", default="gemini-3-pro-preview", help="Gemini model (default: gemini-3-pro-preview)")
    p_focus.add_argument("--dry-run", action="store_true", help="Show what would be sent without calling Gemini")

    # log
    subparsers.add_parser("log", help="Tail the capture log")

    # path
    subparsers.add_parser("path", help="Show database and log paths")

    # ui
    sp_ui = subparsers.add_parser("ui", help="Launch web UI server")
    sp_ui.add_argument("--host", default="127.0.0.1", help="Host to bind (default: 127.0.0.1)")
    sp_ui.add_argument("--port", type=int, default=8420, help="Port to bind (default: 8420)")
    sp_ui.add_argument("--no-open", dest="open", action="store_false", help="Don't open browser")
    sp_ui.add_argument("--reload", action="store_true", help="Enable auto-reload (dev mode)")

    # transcripts
    sp_transcripts = subparsers.add_parser("transcripts", help="Archive session transcripts")
    transcripts_subs = sp_transcripts.add_subparsers(dest="transcripts_command")

    transcripts_subs.add_parser("sync", help="Scan and archive all transcripts")

    sp_tr_list = transcripts_subs.add_parser("list", help="List archived transcripts")
    sp_tr_list.add_argument("--limit", "-n", type=int, default=20, help="Number to show")
    sp_tr_list.add_argument("--subagents", action="store_true", help="Include subagent transcripts")

    transcripts_subs.add_parser("stats", help="Show archive statistics")

    sp_tr_export = transcripts_subs.add_parser("export", help="Export transcript to JSONL")
    sp_tr_export.add_argument("session_id", help="Session ID (prefix match)")
    sp_tr_export.add_argument("--output", "-o", help="Output file path")

    sp_tr_show = transcripts_subs.add_parser("show", help="Show transcript content")
    sp_tr_show.add_argument("session_id", help="Session ID (prefix match)")

    sp_tr_extract = transcripts_subs.add_parser("extract", help="Extract semantic events from transcripts")
    sp_tr_extract.add_argument("--file", "-f", help="Extract from specific file")
    sp_tr_extract.add_argument("--since", type=int, help="Hours to look back (default: 24)")
    sp_tr_extract.add_argument("--force", action="store_true", help="Reprocess from beginning")
    sp_tr_extract.add_argument("--dry-run", action="store_true", help="Show what would be processed")

    args = parser.parse_args()

    if not args.command:
        parser.print_help()
        return 0

    commands = {
        "status": cmd_status,
        "sessions": cmd_sessions,
        "events": cmd_events,
        "files": cmd_files,
        "context": cmd_context,
        "log": cmd_log,
        "path": cmd_path,
        "ui": cmd_ui,
        "learn": cmd_learn,
        "learnings": cmd_learnings,
        "focus": cmd_focus,
        "transcripts": cmd_transcripts,
    }

    handler = commands.get(args.command)
    if handler:
        return handler(args)

    parser.print_help()
    return 1


if __name__ == "__main__":
    sys.exit(main())
