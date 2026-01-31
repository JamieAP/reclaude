from __future__ import annotations

import argparse
import sys
from datetime import datetime, timedelta, timezone

from .cli_constants import FOCUS_PROMPTS
from .cli_context import _squeeze
from .cli_gemini import _call_gemini_sdk
from .db import CaptureDB, SemanticEvent, SemanticEventType

# Character budget for a single Gemini synthesis call.
FOCUS_CHAR_BUDGET = 120_000

# Don't bisect windows smaller than this.
MIN_WINDOW = timedelta(minutes=5)

_TREE_CMDS = {".py": "py-tree", ".rs": "rs-tree"}


def _code_tree(filename: str, content: str) -> str | None:
    """Run rs-tree/py-tree on file content, return tree output or None."""
    import subprocess

    ext = "." + filename.rsplit(".", 1)[-1] if "." in filename else ""
    cmd = _TREE_CMDS.get(ext)
    if not cmd:
        return None

    try:
        result = subprocess.run(
            [cmd, "-"],
            input=content,
            capture_output=True,
            text=True,
            timeout=5,
        )
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    return None


# Section definitions: (section_title, event_types, metadata_filter_or_None)
def _tool_use_summary(e: SemanticEvent) -> str:
    """Extract a compact one-line summary from a tool_use event."""
    import json as _json

    content = e.content
    tool = e.metadata.get("tool_name", "")

    # Try to parse the INPUT JSON block
    input_start = content.find("--- INPUT ---")
    if input_start == -1:
        return content[:80]

    json_start = content.find("{", input_start)
    output_marker = content.find("--- OUTPUT ---", json_start) if json_start != -1 else -1
    json_end = content.rfind("}", json_start, output_marker) if output_marker != -1 else content.rfind("}", json_start)

    if json_start == -1 or json_end == -1:
        return content[:80]

    try:
        inp = _json.loads(content[json_start : json_end + 1])
    except _json.JSONDecodeError:
        return content[:80]

    # Tool-specific compact summaries
    if tool == "Bash":
        cmd = inp.get("command", "")
        # Trim to first line, cap length
        first_line = cmd.split("\n")[0][:120]
        return first_line
    elif tool == "Read":
        path = inp.get("file_path", "")
        parts = []
        if path:
            parts.append(path.split("/")[-1])
        if inp.get("offset"):
            parts.append(f"L{inp['offset']}")
        return " ".join(parts) or path
    elif tool == "Write":
        path = inp.get("file_path", "")
        filename = path.split("/")[-1]
        file_content = inp.get("content", "")
        tree = _code_tree(filename, file_content)
        if tree:
            return f"{filename}\n{tree}"
        return f"{filename} ({len(file_content)} chars)"
    elif tool == "Edit":
        path = inp.get("file_path", "")
        return f"{path.split('/')[-1]}"
    elif tool in ("Grep", "Glob"):
        pat = inp.get("pattern", "")
        path = inp.get("path", "")
        short_path = path.split("/")[-1] if path else ""
        return f'"{pat}" in {short_path}' if short_path else f'"{pat}"'
    elif tool == "Task":
        desc = inp.get("description", "")
        agent = inp.get("subagent_type", "")
        return f"{desc} [{agent}]" if agent else desc
    elif tool in ("TaskUpdate", "TaskCreate", "TaskGet", "TaskList"):
        task_id = inp.get("taskId", "")
        status = inp.get("status", "")
        return f"#{task_id} → {status}" if status else f"#{task_id}"
    else:
        # Generic: first key=value pair
        for k, v in inp.items():
            return f"{k}={str(v)[:60]}"
        return tool


SECTIONS = [
    ("SESSION SUMMARIES", [SemanticEventType.COMPACTION], {"phase": "post", "subtype": "summary"}),
    ("PLANS", [SemanticEventType.PLAN_FILE], None),
    ("CODE CHANGES", [SemanticEventType.FILE_DIFF], None),
    ("CLAUDE'S RESPONSES", [SemanticEventType.ASSISTANT], None),
    ("TOOL USE", [SemanticEventType.TOOL_USE], None),
    ("TASK MANAGEMENT", [SemanticEventType.TASK_CREATE, SemanticEventType.TASK_UPDATE, SemanticEventType.TASK_GET, SemanticEventType.TASK_LIST, SemanticEventType.TODO_WRITE], None),
    ("RAW ACTIVITY", [SemanticEventType.USER_PROMPT, SemanticEventType.PLAN, SemanticEventType.THINKING], None),
]

# Scale → tile duration and lookback
SCALE_CONFIG = {
    "15min": {"tile": timedelta(minutes=15), "lookback": timedelta(minutes=15)},
    "hour": {"tile": timedelta(hours=1), "lookback": timedelta(hours=1)},
    "8hour": {"tile": timedelta(hours=8), "lookback": timedelta(hours=8)},
    "day": {"tile": timedelta(days=1), "lookback": timedelta(days=1)},
    "week": {"tile": timedelta(weeks=1), "lookback": timedelta(weeks=1)},
}


def _snap_to_tile(dt: datetime, tile_duration: timedelta) -> datetime:
    """Snap a datetime down to the nearest tile boundary (UTC)."""
    secs = int(tile_duration.total_seconds())
    # Seconds since midnight UTC of the epoch week start (Monday)
    # For week tiles, align to Monday 00:00
    if secs == 7 * 86400:
        # ISO weekday: Monday=1
        days_since_monday = dt.weekday()
        midnight = dt.replace(hour=0, minute=0, second=0, microsecond=0)
        return midnight - timedelta(days=days_since_monday)
    # For sub-week tiles, align to midnight then snap within the day
    midnight = dt.replace(hour=0, minute=0, second=0, microsecond=0)
    seconds_into_day = int((dt - midnight).total_seconds())
    tile_secs = min(secs, 86400)
    snapped_offset = (seconds_into_day // tile_secs) * tile_secs
    return midnight + timedelta(seconds=snapped_offset)


def _clock_tiles(scale: str, now: datetime) -> list[tuple[datetime, datetime]]:
    """Generate clock-snapped, non-overlapping tiles covering the lookback window."""
    cfg = SCALE_CONFIG[scale]
    lookback_start = now - cfg["lookback"]
    tile_dur = cfg["tile"]

    # Snap to tile boundary at or before lookback_start
    tile_start = _snap_to_tile(lookback_start, tile_dur)

    tiles = []
    while tile_start < now:
        tile_end = tile_start + tile_dur
        # Cap the final tile at now
        actual_end = min(tile_end, now)
        tiles.append((tile_start, actual_end))
        tile_start = tile_end

    return tiles


def _fetch_all_events(
    db: CaptureDB, since: datetime, until: datetime
) -> list[SemanticEvent]:
    """Fetch ALL events in [since, until) with no type filtering or limits."""
    return db.query_events(since=since, until=until, limit=50_000)


def _group_events(
    events: list[SemanticEvent],
) -> tuple[dict[str, list[SemanticEvent]], dict[str, int]]:
    """Group events by section. Returns (groups, other_counts)."""
    type_to_section: dict[str, str] = {}
    for title, types, _ in SECTIONS:
        for t in types:
            type_to_section[t] = title

    groups: dict[str, list[SemanticEvent]] = {title: [] for title, _, _ in SECTIONS}
    other_counts: dict[str, int] = {}
    for e in events:
        etype = e.event_type.value if hasattr(e.event_type, "value") else e.event_type
        section = type_to_section.get(etype)
        if section:
            if etype == SemanticEventType.COMPACTION:
                if e.metadata.get("phase") != "post" or e.metadata.get("subtype") != "summary":
                    other_counts[etype] = other_counts.get(etype, 0) + 1
                    continue
            groups[section].append(e)
        else:
            other_counts[etype] = other_counts.get(etype, 0) + 1

    return groups, other_counts

    return groups


def _build_focus_content(events: list[SemanticEvent], scale: str) -> str:
    """Build structured Gemini input from all events. No truncation."""
    groups, other_counts = _group_events(events)
    sections = []

    for title, _, _ in SECTIONS:
        group = sorted(groups.get(title, []), key=lambda e: e.timestamp)
        if not group:
            continue

        if title == "CODE CHANGES" and scale in ("day", "week"):
            # Aggregate diffs for larger scales
            file_stats: dict[str, dict] = {}
            for e in group:
                path = e.metadata.get("file_path", "unknown")
                if path not in file_stats:
                    file_stats[path] = {"added": 0, "removed": 0, "count": 0}
                file_stats[path]["added"] += e.metadata.get("lines_added", 0)
                file_stats[path]["removed"] += e.metadata.get("lines_removed", 0)
                file_stats[path]["count"] += 1
            sorted_files = sorted(
                file_stats.items(), key=lambda x: x[1]["count"], reverse=True
            )
            lines = [
                f"- {path}: +{s['added']}/-{s['removed']} ({s['count']} edits)"
                for path, s in sorted_files
            ]
            sections.append(f"## {title} ({len(group)} total edits)\n" + "\n".join(lines))
        elif title == "CODE CHANGES":
            lines = []
            for e in group:
                ts = e.timestamp.strftime("%H:%M")
                path = e.metadata.get("file_path", "unknown")
                op = e.metadata.get("operation", "edit")
                filename = path.split("/")[-1]
                added = e.metadata.get("lines_added", 0)
                removed = e.metadata.get("lines_removed", 0)

                if op == "write" and e.content:
                    # Full file write - show structural tree
                    # Content is a unified diff; extract the new file from + lines
                    new_lines = []
                    for dl in e.content.split("\n"):
                        if dl.startswith("+") and not dl.startswith("+++"):
                            new_lines.append(dl[1:])
                    new_content = "\n".join(new_lines)
                    tree = _code_tree(filename, new_content) if new_content else None
                    if tree:
                        lines.append(f"[{ts}] {path} (new file)\n{tree}")
                    else:
                        lines.append(f"[{ts}] {path} (+{added} new)")
                elif e.content:
                    # Edit - include the diff hunk inline
                    lines.append(f"[{ts}] {path}\n{e.content}")
                else:
                    lines.append(f"[{ts}] {path} (+{added}/-{removed})")
            sections.append(f"## {title}\n" + "\n".join(lines))
        elif title == "PLANS":
            # Dedupe plans by slug (keep most recent)
            seen_slugs: set[str] = set()
            unique = []
            for e in reversed(group):
                slug = e.metadata.get("slug", "")
                if slug and slug not in seen_slugs:
                    seen_slugs.add(slug)
                    unique.append(e)
            unique.reverse()
            lines = []
            for e in unique:
                slug = e.metadata.get("slug", "unknown")
                ts = e.timestamp.strftime("%Y-%m-%d %H:%M")
                lines.append(f"### {slug} ({ts})\n{_squeeze(e.content)}")
            sections.append(f"## {title}\n" + "\n".join(lines))
        elif title == "TOOL USE":
            lines = []
            for e in group:
                ts = e.timestamp.strftime("%H:%M")
                tool = e.metadata.get("tool_name", "?")
                ok = "✓" if e.metadata.get("success") else "✗"
                summary = _tool_use_summary(e)
                lines.append(f"[{ts}] {tool}: {summary} {ok}")
            sections.append(f"## {title} ({len(group)} calls)\n" + "\n".join(lines))
        else:
            lines = []
            for e in group:
                ts = e.timestamp.strftime("%Y-%m-%d %H:%M")
                etype = e.event_type.value if hasattr(e.event_type, "value") else e.event_type
                content = _squeeze(e.content)
                lines.append(f"[{ts}] ({etype}) {content}")
            sections.append(f"## {title}\n" + "\n---\n".join(lines))

    # Rolled-up count for dropped sections (agent activity, system, etc.)
    if other_counts:
        rollup = ", ".join(f"{count} {etype}" for etype, count in sorted(other_counts.items()))
        sections.append(f"## OTHER ({sum(other_counts.values())} events)\n{rollup}")

    return "\n\n".join(sections)


def _synthesize_window(
    db: CaptureDB,
    args: argparse.Namespace,
    scale: str,
    start: datetime,
    end: datetime,
    budget: int,
) -> int:
    """Recursively synthesize a time window, bisecting if content exceeds budget."""
    events = _fetch_all_events(db, start, end)

    if not events:
        return 0

    content = _build_focus_content(events, scale)

    if len(content) <= budget or (end - start) <= MIN_WINDOW:
        return _do_synthesis(db, args, scale, start, end, events, content)

    mid = start + (end - start) / 2
    print(
        f"Window {start.strftime('%H:%M')}-{end.strftime('%H:%M')} "
        f"exceeds budget ({len(content):,} > {budget:,} chars), splitting...",
        file=sys.stderr,
    )
    rc1 = _synthesize_window(db, args, scale, start, mid, budget)
    rc2 = _synthesize_window(db, args, scale, mid, end, budget)
    return rc1 or rc2


def _do_synthesis(
    db: CaptureDB,
    args: argparse.Namespace,
    scale: str,
    start: datetime,
    end: datetime,
    events: list[SemanticEvent],
    content: str,
) -> int:
    """Run a single Gemini synthesis call and store the result."""
    groups, other_counts = _group_events(events)
    section_counts = ", ".join(
        f"{len(evts)} {title.lower()}"
        for title, _, _ in SECTIONS
        if (evts := groups.get(title, []))
    )
    total = len(events)
    period_str = f"{start.strftime('%Y-%m-%d %H:%M')} to {end.strftime('%H:%M')}"

    if args.dry_run:
        print(f"\n=== DRY RUN: {period_str} ===")
        print(f"Scale: {scale}")
        print(f"Events: {total} total - {section_counts}")
        print(f"Input size: {len(content):,} chars")
        print("\n--- STRUCTURED INPUT PREVIEW ---")
        print(content[:3000] + ("..." if len(content) > 3000 else ""))
        return 0

    print(
        f"Synthesizing {period_str}: {total} events ({len(content):,} chars)...",
        file=sys.stderr,
    )

    prompt = FOCUS_PROMPTS.get(scale, FOCUS_PROMPTS["hour"])
    output, success = _call_gemini_sdk(
        model_name=args.model,
        prompt=prompt,
        data=content,
        verbose=True,
        temperature=0.05,
    )

    if not success:
        print(f"Synthesis failed: {output}", file=sys.stderr)
        return 1

    lines = [l.strip() for l in output.split("\n") if l.strip() and not l.startswith("#")]
    top_topics = [l[:50] for l in lines[:5]]

    db.insert_focus_snapshot(
        project="all",
        time_scale=scale,
        period_start=start,
        period_end=end,
        focus_summary=output,
        top_topics=top_topics,
        event_count=total,
        metadata={
            "model": args.model,
            "input_chars": len(content),
            "section_counts": {
                title: len(groups.get(title, []))
                for title, _, _ in SECTIONS
            },
        },
    )

    print(f"\n## Focus: {scale}\n*{period_str} - {total} events ({section_counts})*\n")
    print(output)
    return 0


def _focus_snapshot_exists(db: CaptureDB, scale: str, start: datetime, end: datetime) -> bool:
    """Check if a focus snapshot already covers this exact tile."""
    snapshots = db.query_focus_snapshots(time_scale=scale, limit=500)
    for s in snapshots:
        if s.period_start == start and s.period_end == end:
            return True
    return False


def cmd_focus(args: argparse.Namespace) -> int:
    """Generate on-demand focus summary via Gemini synthesis."""
    db = CaptureDB()
    scale = args.scale
    budget = args.budget if args.budget > 0 else FOCUS_CHAR_BUDGET

    now = datetime.now(timezone.utc)

    if scale not in SCALE_CONFIG:
        print(f"Unknown scale: {scale}", file=sys.stderr)
        return 1

    tiles = _clock_tiles(scale, now)
    if not tiles:
        print(f"No tiles for {scale}.", file=sys.stderr)
        return 0

    print(
        f"Focus {scale}: {len(tiles)} tile(s) covering "
        f"{tiles[0][0].strftime('%Y-%m-%d %H:%M')} to {tiles[-1][1].strftime('%H:%M')}",
        file=sys.stderr,
    )

    rc = 0
    for tile_start, tile_end in tiles:
        cfg = SCALE_CONFIG[scale]
        tile_complete = tile_end == tile_start + cfg["tile"]

        # Skip complete tiles that already have snapshots
        if tile_complete and not args.dry_run and _focus_snapshot_exists(db, scale, tile_start, tile_end):
            print(
                f"  Tile {tile_start.strftime('%H:%M')}-{tile_end.strftime('%H:%M')}: "
                f"already synthesized, skipping.",
                file=sys.stderr,
            )
            continue

        result = _synthesize_window(db, args, scale, tile_start, tile_end, budget)
        if result != 0:
            rc = result

    return rc
