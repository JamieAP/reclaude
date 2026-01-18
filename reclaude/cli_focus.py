from __future__ import annotations

import argparse
import sys
from datetime import datetime, timedelta, timezone

from .cli_constants import FOCUS_PROMPTS
from .cli_gemini import _call_gemini_sdk
from .db import CaptureDB, SemanticEventType


def cmd_focus(args: argparse.Namespace) -> int:
    """Generate on-demand focus summary via Gemini synthesis."""
    db = CaptureDB()
    scale = args.scale

    # Rolling time windows
    now = datetime.now(timezone.utc)
    if scale == "hour":
        period_start = now - timedelta(hours=1)
    elif scale == "8hour":
        period_start = now - timedelta(hours=8)
    elif scale == "day":
        period_start = now - timedelta(hours=24)
    elif scale == "week":
        period_start = now - timedelta(days=7)
    else:
        print(f"Unknown scale: {scale}", file=sys.stderr)
        return 1

    # Scale-appropriate limits for each event type
    # assistant: Claude's responses (outcomes); plans: implementation context
    limits = {
        "hour": {"summaries": 10, "diffs": 15, "assistant": 10, "plans": 3, "raw": 25},
        "8hour": {"summaries": 25, "diffs": 30, "assistant": 20, "plans": 5, "raw": 40},
        "day": {"summaries": 50, "diffs": 50, "assistant": 30, "plans": 8, "raw": 60},
        "week": {"summaries": 100, "diffs": 100, "assistant": 40, "plans": 10, "raw": 30},
    }.get(scale, {"summaries": 20, "diffs": 20, "assistant": 15, "plans": 5, "raw": 40})

    # -------------------------------------------------------------------------
    # Tier 1: COMPACTION summaries (pre-synthesized session context - backbone)
    # -------------------------------------------------------------------------
    compaction_summaries = db.query_events(
        event_type=SemanticEventType.COMPACTION,
        since=period_start,
        limit=limits["summaries"],
        metadata_filter={"phase": "post", "subtype": "summary"},
    )
    compaction_summaries.sort(key=lambda e: e.timestamp)

    # -------------------------------------------------------------------------
    # Tier 1b: PLAN_FILE events (implementation plans - high-level context)
    # -------------------------------------------------------------------------
    plan_files = db.query_events(
        event_type=SemanticEventType.PLAN_FILE,
        since=period_start,
        limit=limits["plans"],
    )
    plan_files.sort(key=lambda e: e.timestamp)
    # Dedupe by slug (keep most recent version of each plan)
    seen_slugs: set[str] = set()
    unique_plans = []
    for p in reversed(plan_files):
        slug = p.metadata.get("slug", "")
        if slug and slug not in seen_slugs:
            seen_slugs.add(slug)
            unique_plans.append(p)
    plan_files = list(reversed(unique_plans))

    # -------------------------------------------------------------------------
    # Tier 2: FILE_DIFF events (what actually changed)
    # -------------------------------------------------------------------------
    file_diffs = db.query_events(
        event_type=SemanticEventType.FILE_DIFF,
        since=period_start,
        limit=limits["diffs"],
    )
    file_diffs.sort(key=lambda e: e.timestamp)

    # -------------------------------------------------------------------------
    # Tier 2b: ASSISTANT events (Claude's responses - what was accomplished)
    # -------------------------------------------------------------------------
    assistant_events = db.query_events(
        event_type=SemanticEventType.ASSISTANT,
        since=period_start,
        limit=limits["assistant"],
    )
    assistant_events.sort(key=lambda e: e.timestamp)

    # -------------------------------------------------------------------------
    # Tier 3: Raw events (prompts, plans, thinking) - fill gaps
    # -------------------------------------------------------------------------
    raw_event_types = [
        SemanticEventType.USER_PROMPT,
        SemanticEventType.PLAN,
        SemanticEventType.THINKING,
    ]

    raw_events = []
    for etype in raw_event_types:
        events = db.query_events(
            event_type=etype,
            since=period_start,
            limit=500,  # Fetch more, then trim
        )
        raw_events.extend(events)

    raw_events.sort(key=lambda e: e.timestamp)
    raw_events = raw_events[-limits["raw"]:]  # Keep most recent

    total_events = (
        len(compaction_summaries)
        + len(plan_files)
        + len(file_diffs)
        + len(assistant_events)
        + len(raw_events)
    )
    if total_events == 0:
        print(f"No events in last {scale}.", file=sys.stderr)
        return 0

    print(
        f"Synthesizing {scale} focus: {len(compaction_summaries)} summaries, "
        f"{len(plan_files)} plans, {len(file_diffs)} diffs, "
        f"{len(assistant_events)} assistant, {len(raw_events)} raw...",
        file=sys.stderr,
    )

    # -------------------------------------------------------------------------
    # Build structured input with sections
    # -------------------------------------------------------------------------
    sections = []

    # Section 1: Session summaries (high value - pre-synthesized)
    if compaction_summaries:
        summary_lines = []
        for e in compaction_summaries:
            ts = e.timestamp.strftime("%Y-%m-%d %H:%M")
            # Summaries can be longer since they're pre-digested
            content = e.content[:1500] if len(e.content) > 1500 else e.content
            summary_lines.append(f"[{ts}] {content}")
        sections.append("## SESSION SUMMARIES\n" + "\n---\n".join(summary_lines))

    # Section 2: Implementation plans (high-level context)
    if plan_files:
        plan_lines = []
        for e in plan_files:
            slug = e.metadata.get("slug", "unknown")
            ts = e.timestamp.strftime("%Y-%m-%d %H:%M")
            # Plans can be long - extract key info based on scale
            if scale == "week":
                # For week: just show plan names and first line
                first_line = e.content.split("\n")[0][:200] if e.content else ""
                plan_lines.append(f"- **{slug}** ({ts}): {first_line}")
            else:
                # For shorter scales: include more context
                content = e.content[:800] if len(e.content) > 800 else e.content
                plan_lines.append(f"### {slug} ({ts})\n{content}")
        sections.append("## ACTIVE PLANS\n" + "\n".join(plan_lines))

    # Section 3: Code changes
    if file_diffs:
        if scale in ("day", "week"):
            # Aggregate for larger scales
            file_stats: dict[str, dict] = {}
            for e in file_diffs:
                path = e.metadata.get("file_path", "unknown")
                if path not in file_stats:
                    file_stats[path] = {"added": 0, "removed": 0, "count": 0}
                file_stats[path]["added"] += e.metadata.get("lines_added", 0)
                file_stats[path]["removed"] += e.metadata.get("lines_removed", 0)
                file_stats[path]["count"] += 1
            # Sort by activity
            sorted_files = sorted(file_stats.items(), key=lambda x: x[1]["count"], reverse=True)
            diff_lines = [
                f"- {path}: +{stats['added']}/-{stats['removed']} ({stats['count']} edits)"
                for path, stats in sorted_files[:30]  # Cap at 30 files
            ]
            sections.append(f"## CODE CHANGES ({len(file_diffs)} total edits)\n" + "\n".join(diff_lines))
        else:
            # Detailed for shorter scales
            diff_lines = []
            for e in file_diffs:
                ts = e.timestamp.strftime("%H:%M")
                path = e.metadata.get("file_path", "unknown")
                added = e.metadata.get("lines_added", 0)
                removed = e.metadata.get("lines_removed", 0)
                diff_lines.append(f"[{ts}] {path} (+{added}/-{removed})")
            sections.append("## CODE CHANGES\n" + "\n".join(diff_lines))

    # Section 4: Claude's responses (what was accomplished)
    if assistant_events:
        assistant_lines = []
        for e in assistant_events:
            ts = e.timestamp.strftime("%Y-%m-%d %H:%M")
            # Truncate based on scale
            max_len = 300 if scale == "week" else 500 if scale == "day" else 700
            content = e.content[:max_len] if len(e.content) > max_len else e.content
            assistant_lines.append(f"[{ts}] {content}")
        sections.append("## CLAUDE'S RESPONSES\n" + "\n---\n".join(assistant_lines))

    # Section 5: Raw activity (prompts, thinking - fill gaps)
    if raw_events:
        raw_lines = []
        for e in raw_events:
            ts = e.timestamp.strftime("%Y-%m-%d %H:%M")
            etype = e.event_type.value if hasattr(e.event_type, "value") else e.event_type
            content = e.content[:400] if len(e.content) > 400 else e.content
            raw_lines.append(f"[{ts}] ({etype}) {content}")
        sections.append("## RAW ACTIVITY\n" + "\n---\n".join(raw_lines))

    event_content = "\n\n".join(sections)
    prompt = FOCUS_PROMPTS.get(scale, FOCUS_PROMPTS["hour"])

    if args.dry_run:
        print("\n=== DRY RUN ===")
        print(f"Scale: {scale}")
        print(f"Period: {period_start.strftime('%Y-%m-%d %H:%M')} to {now.strftime('%Y-%m-%d %H:%M')}")
        print(
            f"Events: {len(compaction_summaries)} summaries, {len(plan_files)} plans, "
            f"{len(file_diffs)} diffs, {len(assistant_events)} assistant, {len(raw_events)} raw"
        )
        print(f"Input size: {len(event_content)} chars")
        print("\n--- STRUCTURED INPUT PREVIEW ---")
        print(event_content[:3000] + ("..." if len(event_content) > 3000 else ""))
        return 0

    # Synthesize via Gemini
    output, success = _call_gemini_sdk(
        model_name=args.model,
        prompt=prompt,
        data=event_content,
        verbose=True,
        temperature=0.3,
    )

    if not success:
        print(f"Synthesis failed: {output}", file=sys.stderr)
        return 1

    # Extract topics (first few words of each bullet/line)
    lines = [l.strip() for l in output.split("\n") if l.strip() and not l.startswith("#")]
    top_topics = [l[:50] for l in lines[:5]]

    # Store snapshot for web UI
    db.insert_focus_snapshot(
        project="all",  # No longer project-scoped
        time_scale=scale,
        period_start=period_start,
        period_end=now,
        focus_summary=output,
        top_topics=top_topics,
        event_count=total_events,
        metadata={
            "model": args.model,
            "summaries_count": len(compaction_summaries),
            "plans_count": len(plan_files),
            "diffs_count": len(file_diffs),
            "assistant_count": len(assistant_events),
            "raw_count": len(raw_events),
        },
    )

    # Output
    period_str = f"{period_start.strftime('%Y-%m-%d %H:%M')} to {now.strftime('%H:%M')}"
    print(
        f"\n## Focus: {scale}\n*{period_str} - {total_events} events "
        f"({len(compaction_summaries)} summaries, {len(plan_files)} plans, "
        f"{len(file_diffs)} diffs, {len(assistant_events)} assistant)*\n"
    )
    print(output)
    return 0
