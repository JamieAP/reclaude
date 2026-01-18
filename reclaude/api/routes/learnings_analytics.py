"""Analytics API routes for learnings."""

from collections import defaultdict
from datetime import datetime

from fastapi import APIRouter, Depends, Query

from reclaude.db import CaptureDB

from ..deps import get_db
from ..models import LearningsAnalytics, LearningsTimeSeriesPoint

router = APIRouter(prefix="/learnings/analytics", tags=["learnings-analytics"])


@router.get("", response_model=LearningsAnalytics)
def get_analytics(
    since: datetime | None = Query(None),
    remote_url: str | None = Query(None),
    db: CaptureDB = Depends(get_db),
) -> LearningsAnalytics:
    """Get analytics summary for learnings."""
    # Get true total count (separate from the limited query for aggregations)
    with db.connection() as conn:
        count_query = "SELECT COUNT(*) as cnt FROM learnings WHERE 1=1"
        params: list[str] = []
        if since:
            count_query += " AND timestamp >= ?"
            params.append(since.isoformat())
        if remote_url:
            count_query += " AND remote_url = ?"
            params.append(remote_url)
        total_count = conn.execute(count_query, params).fetchone()["cnt"]

    # Sample up to 500 learnings for aggregation charts
    learnings = db.query_learnings(
        since=since,
        remote_url=remote_url,
        limit=500,
    )

    if not learnings:
        return LearningsAnalytics(
            total_count=0,
            by_repo={},
            by_branch={},
            time_series=[],
            avg_content_length=0.0,
            date_range_start=None,
            date_range_end=None,
        )

    by_repo: dict[str, int] = defaultdict(int)
    by_branch: dict[str, int] = defaultdict(int)
    by_date: dict[str, int] = defaultdict(int)
    total_content_len = 0

    for l in learnings:
        repo_key = l.repo_name or "unknown"
        by_repo[repo_key] += 1

        branch_key = l.branch or "unknown"
        by_branch[branch_key] += 1

        # Truncate to date
        date_key = l.timestamp.isoformat()[:10]
        by_date[date_key] += 1

        total_content_len += len(l.content)

    # Sort time series by date
    time_series = [
        LearningsTimeSeriesPoint(date=d, count=c)
        for d, c in sorted(by_date.items())
    ]

    timestamps = [l.timestamp.isoformat() for l in learnings]

    return LearningsAnalytics(
        total_count=total_count,
        by_repo=dict(by_repo),
        by_branch=dict(by_branch),
        time_series=time_series,
        avg_content_length=total_content_len / len(learnings),
        date_range_start=min(timestamps),
        date_range_end=max(timestamps),
    )
