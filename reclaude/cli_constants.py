from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from .db import SemanticEventType

LOG_FILE = Path.home() / ".reclaude" / "reclaude.jsonl"

SID_NONE = "--------"

# Gemini rate limiting config
GEMINI_RPM_LIMIT = 60  # Allow burst up to 60 RPM
GEMINI_MIN_INTERVAL = 60.0 / GEMINI_RPM_LIMIT  # 1 second between calls


# Event type configuration for unified --type filtering
@dataclass(frozen=True, slots=True)
class EventTypeConfig:
    """Configuration for an event type in the CLI."""
    db_type: str  # SemanticEventType value for DB queries
    semantic_types: tuple[str, ...]  # Event types for semantic search
    default_limit: int  # Default --limit value
    preview_len: int  # Default content preview length (0 = metadata-based formatting)
    empty_message: str  # Message when no events found


EVENT_TYPE_CONFIG: dict[str, EventTypeConfig] = {
    "prompt": EventTypeConfig(
        db_type=SemanticEventType.USER_PROMPT,
        semantic_types=("user_prompt",),
        default_limit=10,
        preview_len=200,
        empty_message="No prompts found",
    ),
    "diff": EventTypeConfig(
        db_type=SemanticEventType.FILE_DIFF,
        semantic_types=("file_diff",),
        default_limit=20,
        preview_len=0,  # Uses metadata: file_path, operation, lines_added, lines_removed
        empty_message="No diffs found",
    ),
    "plan": EventTypeConfig(
        db_type=SemanticEventType.PLAN,
        semantic_types=("plan", "plan_file"),
        default_limit=10,
        preview_len=300,
        empty_message="No plans found",
    ),
    "tool": EventTypeConfig(
        db_type=SemanticEventType.TOOL_USE,
        semantic_types=("tool_use",),
        default_limit=20,
        preview_len=150,  # Also uses metadata: tool_name, success, duration_ms
        empty_message="No tool usage events found",
    ),
    "compaction": EventTypeConfig(
        db_type=SemanticEventType.COMPACTION,
        semantic_types=("compaction",),
        default_limit=10,
        preview_len=100,  # Variable: 500 for context subtype, 100 otherwise
        empty_message="No compaction events found",
    ),
}


FOCUS_PROMPTS = {
    "15min": """Summarize the developer's work in the LAST 15 MINUTES.

The input is organized into sections:
- SESSION SUMMARIES: Pre-synthesized summaries of completed work (high signal)
- PLANS: Implementation plans being followed (goals/context)
- CODE CHANGES: Files modified with diffs and structural trees
- CLAUDE'S RESPONSES: What Claude accomplished (outcomes)
- TOOL USE: Commands and tools executed
- RAW ACTIVITY: Prompts, plans, and thinking (fill gaps)

What exactly happened in this short window? Be precise and granular - every action matters at this timescale.

Be specific and concise (2-4 bullets). Include file names, function names, commands, error messages where relevant.""",

    "hour": """Summarize the developer's work in the LAST HOUR.

The input is organized into sections:
- SESSION SUMMARIES: Pre-synthesized summaries of completed work (high signal)
- ACTIVE PLANS: Implementation plans being followed (goals/context)
- CODE CHANGES: Files modified with line counts
- CLAUDE'S RESPONSES: What Claude accomplished (outcomes)
- RAW ACTIVITY: Prompts, plans, and thinking (fill gaps)

What specific task(s) were they working on? What progress was made?
Note any blockers, decisions, or discoveries.

Be specific and concise (3-5 bullets). Include file names, function names, error messages where relevant.""",

    "8hour": """Summarize the developer's work over the LAST 8 HOURS (a work session).

The input is organized into sections:
- SESSION SUMMARIES: Pre-synthesized summaries of completed work (high signal)
- ACTIVE PLANS: Implementation plans being followed (goals/context)
- CODE CHANGES: Files modified with line counts
- CLAUDE'S RESPONSES: What Claude accomplished (outcomes)
- RAW ACTIVITY: Prompts, plans, and thinking (fill gaps)

What were the main tasks tackled? What was accomplished vs still in progress?
Note significant decisions, blockers overcome, or patterns discovered.

Be thorough but concise (5-10 bullets). Group by task/feature if multiple threads of work.""",

    "day": """Summarize the developer's work over the LAST 24 HOURS.

The input is organized into sections:
- SESSION SUMMARIES: Pre-synthesized summaries (prioritize these)
- ACTIVE PLANS: Implementation plans (high-level goals)
- CODE CHANGES: Aggregated file modifications
- CLAUDE'S RESPONSES: Key accomplishments from Claude
- RAW ACTIVITY: Recent prompts (supplement summaries)

What features, bugs, or research got attention? What meaningful progress was made?
Note any recurring themes, decisions made, or insights gained.

Structured summary (8-15 bullets). Group by project/feature area.""",

    "week": """Summarize the developer's work over the LAST 7 DAYS.

The input is organized into sections:
- SESSION SUMMARIES: Pre-synthesized summaries (primary source - trust these)
- ACTIVE PLANS: Implementation plans worked on this week
- CODE CHANGES: Aggregated file change statistics
- CLAUDE'S RESPONSES: Key outcomes (supplement summaries)
- RAW ACTIVITY: Key prompts only (minimal)

What were the major themes and sustained efforts vs one-off tasks?
What progress was made toward larger goals? Any patterns in the work?

High-level summary (10-20 bullets). Focus on outcomes and evolution of understanding.""",
}
