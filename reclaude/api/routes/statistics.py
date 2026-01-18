"""Statistics API routes."""

from fastapi import APIRouter, Depends

from reclaude.db import CaptureDB

from ..deps import get_db
from ..models import RepoInfo, StatisticsResponse

router = APIRouter(prefix="/statistics", tags=["statistics"])


@router.get("", response_model=StatisticsResponse)
def get_statistics(
    db: CaptureDB = Depends(get_db),
) -> StatisticsResponse:
    """Get overall statistics for the dashboard."""
    total_events = db.count_events()
    events_by_type = db.event_counts_by_type()
    sessions = db.get_sessions_with_info()
    event_types = db.get_distinct_event_types()

    # Count learnings efficiently with COUNT query
    total_learnings = db.count_learnings()

    # Get repos
    repos_data = db.get_repos_with_stats()
    repos = [RepoInfo.from_db_dict(r) for r in repos_data]

    return StatisticsResponse(
        total_events=total_events,
        events_by_type=events_by_type,
        total_sessions=len(sessions),
        total_learnings=total_learnings,
        event_types=event_types,
        repos=repos,
    )
