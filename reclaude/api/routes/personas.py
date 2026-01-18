"""Persona/agent usage API routes."""

from datetime import datetime, timedelta, timezone

from fastapi import APIRouter, Depends, Query

from reclaude.db import CaptureDB

from ..deps import get_db

router = APIRouter(prefix="/personas", tags=["personas"])


@router.get("/usage")
def get_persona_usage(
    days: int = Query(7, ge=1, le=90, description="Days to look back"),
    db: CaptureDB = Depends(get_db),
) -> dict:
    """Get aggregated persona/agent usage statistics.

    Extracts subagent_type from Task tool_use events and aggregates usage.
    Works with both new structured metadata and legacy content parsing.
    """
    since = datetime.now(timezone.utc) - timedelta(days=days)

    with db.connection() as conn:
        # Try structured metadata first (new captures)
        structured_query = """
            SELECT
                json_extract(metadata, '$.subagent_type') as agent_type,
                json_extract(metadata, '$.persona') as persona,
                COUNT(*) as count,
                SUM(CASE WHEN json_extract(metadata, '$.success') = 1 THEN 1 ELSE 0 END) as success_count,
                AVG(json_extract(metadata, '$.duration_ms')) as avg_duration_ms
            FROM semantic_events
            WHERE event_type = 'tool_use'
              AND json_extract(metadata, '$.tool_name') = 'Task'
              AND json_extract(metadata, '$.subagent_type') IS NOT NULL
              AND timestamp >= ?
            GROUP BY agent_type, persona
            ORDER BY count DESC
        """
        structured_rows = conn.execute(structured_query, (since.isoformat(),)).fetchall()

        # Also parse legacy content for older Task events without structured metadata
        legacy_query = """
            SELECT content
            FROM semantic_events
            WHERE event_type = 'tool_use'
              AND json_extract(metadata, '$.tool_name') = 'Task'
              AND json_extract(metadata, '$.subagent_type') IS NULL
              AND timestamp >= ?
        """
        legacy_rows = conn.execute(legacy_query, (since.isoformat(),)).fetchall()

    # Build results from structured data
    usage_by_type: dict[str, dict] = {}
    for row in structured_rows:
        agent_type = row["agent_type"]
        if agent_type:
            usage_by_type[agent_type] = {
                "count": row["count"],
                "success_count": row["success_count"],
                "avg_duration_ms": round(row["avg_duration_ms"]) if row["avg_duration_ms"] else None,
                "persona": row["persona"],
            }

    # Parse legacy content for subagent_type
    import json
    import re
    for row in legacy_rows:
        content = row["content"] or ""
        # Try to extract subagent_type from JSON in content
        match = re.search(r'"subagent_type":\s*"([^"]+)"', content)
        if match:
            agent_type = match.group(1)
            if agent_type not in usage_by_type:
                usage_by_type[agent_type] = {
                    "count": 0,
                    "success_count": 0,
                    "avg_duration_ms": None,
                    "persona": agent_type.split(":")[-1] if ":" in agent_type else None,
                }
            usage_by_type[agent_type]["count"] += 1

    # Categorize into built-in vs custom personas
    builtin_agents = []
    custom_personas = []

    for agent_type, stats in sorted(usage_by_type.items(), key=lambda x: -x[1]["count"]):
        entry = {
            "agent_type": agent_type,
            **stats,
        }
        if ":" in agent_type:
            custom_personas.append(entry)
        else:
            builtin_agents.append(entry)

    return {
        "days": days,
        "total_task_calls": sum(s["count"] for s in usage_by_type.values()),
        "builtin_agents": builtin_agents,
        "custom_personas": custom_personas,
    }


@router.get("/timeline")
def get_persona_timeline(
    days: int = Query(7, ge=1, le=30, description="Days to look back"),
    db: CaptureDB = Depends(get_db),
) -> list[dict]:
    """Get daily persona usage timeline.

    Returns daily counts of each persona/agent type.
    """
    since = datetime.now(timezone.utc) - timedelta(days=days)

    with db.connection() as conn:
        query = """
            SELECT
                date(timestamp) as day,
                COALESCE(
                    json_extract(metadata, '$.subagent_type'),
                    'unknown'
                ) as agent_type,
                COUNT(*) as count
            FROM semantic_events
            WHERE event_type = 'tool_use'
              AND json_extract(metadata, '$.tool_name') = 'Task'
              AND timestamp >= ?
            GROUP BY day, agent_type
            ORDER BY day DESC, count DESC
        """
        rows = conn.execute(query, (since.isoformat(),)).fetchall()

    # Group by day
    timeline: dict[str, dict[str, int]] = {}
    for row in rows:
        day = row["day"]
        agent_type = row["agent_type"]
        if day not in timeline:
            timeline[day] = {}
        if agent_type and agent_type != "unknown":
            timeline[day][agent_type] = row["count"]

    return [
        {"date": day, "agents": agents}
        for day, agents in sorted(timeline.items(), reverse=True)
    ]
