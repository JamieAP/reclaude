"""Focus snapshot API routes."""

from fastapi import APIRouter, Depends, Query

from reclaude.db import CaptureDB

from ..deps import get_db
from ..models import FocusSnapshotResponse

router = APIRouter(prefix="/focus", tags=["focus"])


@router.get("", response_model=list[FocusSnapshotResponse])
def list_focus_snapshots(
    project: str | None = Query(None, description="Filter by project"),
    time_scale: str | None = Query(None, description="Filter by time scale (minute, hour, day, week)"),
    limit: int = Query(50, ge=1, le=200, description="Maximum results"),
    db: CaptureDB = Depends(get_db),
) -> list[FocusSnapshotResponse]:
    """Query focus snapshots with optional filters."""
    snapshots = db.query_focus_snapshots(
        project=project,
        time_scale=time_scale,
        limit=limit,
    )
    return [
        FocusSnapshotResponse(
            id=s.id,
            timestamp=s.timestamp,
            project=s.project,
            time_scale=s.time_scale,
            period_start=s.period_start,
            period_end=s.period_end,
            focus_summary=s.focus_summary,
            top_topics=s.top_topics,
            event_count=s.event_count,
            metadata=s.metadata,
        )
        for s in snapshots
    ]
