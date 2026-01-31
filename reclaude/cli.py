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
from .cli_chat import cmd_chat
from .cli_plans import cmd_plans_sync
from .cli_search import cmd_search
from .cli_files import cmd_files
from .cli_server import cmd_log, cmd_path, cmd_ui
from .cli_status import cmd_session_summary, cmd_sessions, cmd_status
from .cli_transcripts import cmd_transcripts
from .cli_utils import (
    add_json_flag, add_full_flag, add_session_flag,
    add_all_flag, add_fzf_flag, add_limit_flag, add_limit_positional,
)


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
    add_limit_positional(sp_sessions, default=10)
    add_all_flag(sp_sessions, help="Show sessions from all directories (default: cwd only)")
    add_json_flag(sp_sessions)
    add_fzf_flag(sp_sessions, help="Interactive select with fzf, outputs cd+cb command")
    sp_sessions.add_argument("--heal", action="store_true", help="Emit synthetic session_end for crashed sessions")

    # events
    sp_events = subparsers.add_parser("events", help="Show recent events")
    sp_events.add_argument("limit", type=int, nargs="?", default=None, help="Number of events (default varies by type)")
    add_session_flag(sp_events)
    sp_events.add_argument("--type", "-t", metavar="TYPES", help="Filter by event type(s): prompt,diff,plan,tool,compaction")
    add_json_flag(sp_events)
    add_full_flag(sp_events)
    sp_events.add_argument("--semantic", metavar="QUERY", help="Semantic search query")
    sp_events.add_argument("--cwd", help="Scope to this directory (default: current directory)")
    add_all_flag(sp_events, help="Search all repos (not just current)")
    sp_events.add_argument("--compact", action="store_true", help="Compact output for preview panes")
    add_fzf_flag(sp_events, help="Interactive event browser with fzf")
    sp_events.add_argument("--id", type=int, help="Show a single event by ID")

    # chat - view conversation around an event
    p_chat = subparsers.add_parser("chat", help="View conversation around an event")
    p_chat.add_argument("event_id", type=int, nargs="?", help="Event ID to navigate to")
    add_session_flag(p_chat)
    p_chat.add_argument("--no-pager", action="store_true", help="Print to stdout instead of less")
    add_all_flag(p_chat, help="Show all sessions across all repos")
    add_fzf_flag(p_chat, help="Pick session interactively with fzf")

    # files - files touched by Claude
    sp_files = subparsers.add_parser("files", help="Find files touched by Claude")
    sp_files.add_argument("pattern", nargs="?", help="Filter by file path substring")
    add_session_flag(sp_files)
    add_limit_flag(sp_files, default=50)
    sp_files.add_argument("--scan-limit", type=int, default=5000, help="Max events to scan (default 5000)")
    add_json_flag(sp_files)
    add_full_flag(sp_files)
    sp_files.add_argument("--stream", action="store_true", help="Stream paths live, poll for new (Ctrl+C to stop)")

    # context
    sp_context = subparsers.add_parser("context", help="Emit LLM-ready session context for cwd")
    sp_context.add_argument("--session", "-s", help="Session ID or prefix (default: latest in cwd)")
    sp_context.add_argument("--since", help="Start from timestamp (1h, 24h, 7d, or ISO-8601)")
    sp_context.add_argument("--format", choices=["text", "json"], default="text", help="Output format")
    sp_context.add_argument("--budget", type=int, default=16000, help="Character budget (default: 16000, 0=unlimited)")
    sp_context.add_argument("--full", action="store_true", help="No budget limit (equivalent to --budget 0)")
    sp_context.add_argument("--chain", action="store_true", help="Follow session chain through clear transitions")
    sp_context.add_argument("limit", type=int, nargs="?", default=100, help="Max events to include")

    # learn
    sp_learn = subparsers.add_parser("learn", help="Store a learning note")
    sp_learn.add_argument("content", nargs="?", help="Learning content (or pipe via stdin)")
    sp_learn.add_argument("--session", "-s", help="Associate with session ID")

    # learnings
    sp_learnings = subparsers.add_parser("learnings", help="List stored learnings")
    add_limit_positional(sp_learnings, default=10)
    add_json_flag(sp_learnings)
    add_full_flag(sp_learnings)
    sp_learnings.add_argument("--semantic", metavar="QUERY", help="Semantic search query")
    add_all_flag(sp_learnings, help="Search all repos (not just current)")
    add_session_flag(sp_learnings)
    sp_learnings.add_argument("--human", action="store_true", help="Human-readable semantic output")
    sp_learnings.add_argument("--extract", metavar="SESSION", help="Extract learnings from session")
    sp_learnings.add_argument("--commit", action="store_true", help="Save extracted learnings")

    # focus
    p_focus = subparsers.add_parser("focus", help="Synthesize focus summary via Gemini")
    p_focus.add_argument(
        "scale",
        nargs="?",
        default="hour",
        choices=["15min", "hour", "8hour", "day", "week"],
        help="Time window: 15min, hour (1h), 8hour (8h), day (24h), week (7d)",
    )
    p_focus.add_argument("--model", default="gemini-3-pro-preview", help="Gemini model (default: gemini-3-pro-preview)")
    p_focus.add_argument("--dry-run", action="store_true", help="Show what would be sent without calling Gemini")
    p_focus.add_argument("--budget", type=int, default=120000, help="Character budget per synthesis call (default: 120000, 0=unlimited)")

    # search
    p_search = subparsers.add_parser("search", help="Full-text search over events")
    p_search.add_argument("query", nargs="+", help="Search terms")
    p_search.add_argument("--and", dest="and_terms", action="append", default=[], metavar="TERM", help="Require additional term (repeatable)")
    p_search.add_argument("--or", dest="or_terms", action="append", default=[], metavar="TERM", help="Include alternative term (repeatable)")
    p_search.add_argument("--not", dest="not_terms", action="append", default=[], metavar="TERM", help="Exclude term (repeatable)")
    p_search.add_argument("-t", "--type", dest="event_type", help="Filter by event type")
    p_search.add_argument("-v", "--verbose", action="store_true", help="Show full event content")
    add_limit_flag(p_search, default=20)
    add_fzf_flag(p_search, help="Open results in fzf with chat preview")
    p_search.add_argument("--rebuild", action="store_true", help="Rebuild FTS index first")
    p_search.add_argument("--cwd", help="Scope search to this directory (default: current directory)")
    add_all_flag(p_search, help="Search all projects, not just current directory")
    add_session_flag(p_search)
    add_json_flag(p_search)

    # log
    log_parser = subparsers.add_parser("log", help="Tail the event log (pretty JSONL)")
    log_parser.add_argument("-n", "--lines", type=int, default=20, help="Initial lines to show (default: 20)")

    # path
    subparsers.add_parser("path", help="Show database and log paths")

    # ui
    sp_ui = subparsers.add_parser("ui", help="Launch web UI server")
    sp_ui.add_argument("--host", default="127.0.0.1", help="Host to bind (default: 127.0.0.1)")
    sp_ui.add_argument("--port", type=int, default=8420, help="Port to bind (default: 8420)")
    sp_ui.add_argument("--no-open", dest="open", action="store_false", help="Don't open browser")
    sp_ui.add_argument("--reload", action="store_true", help="Enable auto-reload (dev mode)")

    # plans
    sp_plans = subparsers.add_parser("plans", help="Manage plan file capture")
    plans_subs = sp_plans.add_subparsers(dest="plans_command")
    plans_subs.add_parser("sync", help="Discover and ingest ~/.claude/plans/*.md")

    # transcripts
    sp_transcripts = subparsers.add_parser("transcripts", help="Archive session transcripts")
    transcripts_subs = sp_transcripts.add_subparsers(dest="transcripts_command")

    transcripts_subs.add_parser("sync", help="Scan and archive all transcripts")

    sp_tr_list = transcripts_subs.add_parser("list", help="List archived transcripts")
    add_limit_flag(sp_tr_list, default=20)
    sp_tr_list.add_argument("--subagents", action="store_true", help="Include subagent transcripts")
    add_fzf_flag(sp_tr_list, help="Interactive select with fzf, enter opens show")

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

    # session-summary (used by fzf preview)
    sp_ss = subparsers.add_parser("session-summary", help="Gemini summary of a session")
    sp_ss.add_argument("session_id", help="Session ID (prefix match)")

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
        "session-summary": cmd_session_summary,
    }

    if args.command == "search":
        return cmd_search(args)

    if args.command == "chat":
        return cmd_chat(args)

    # Subcommand routing for nested commands
    if args.command == "plans":
        if args.plans_command == "sync":
            return cmd_plans_sync(args)
        sp_plans.print_help()
        return 0

    handler = commands.get(args.command)
    if handler:
        return handler(args)

    parser.print_help()
    return 1


if __name__ == "__main__":
    sys.exit(main())
