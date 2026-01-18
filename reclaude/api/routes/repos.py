"""Repository API routes."""

from datetime import datetime

from fastapi import APIRouter, Depends, Query

from reclaude.db import CaptureDB

from ..deps import get_db
from ..models import RepoInfo, SemanticEventResponse

router = APIRouter(prefix="/repos", tags=["repos"])


@router.get("", response_model=list[RepoInfo])
def list_repos(
    db: CaptureDB = Depends(get_db),
) -> list[RepoInfo]:
    """List all repositories with event counts."""
    repos = db.get_repos_with_stats()
    return [RepoInfo.from_db_dict(r) for r in repos]


@router.get("/{remote_url:path}/events", response_model=list[SemanticEventResponse])
def list_repo_events(
    remote_url: str,
    event_type: str | None = Query(None, description="Filter by event type"),
    since: datetime | None = Query(None, description="Only events after this timestamp"),
    limit: int = Query(100, ge=1, le=1000, description="Maximum results"),
    offset: int = Query(0, ge=0, description="Pagination offset"),
    db: CaptureDB = Depends(get_db),
) -> list[SemanticEventResponse]:
    """Get events for a specific repository."""
    # Query all events and filter by remote_url in metadata
    # Over-fetch to account for filtering, then limit
    events = db.query_events(
        event_type=event_type,
        since=since,
        limit=limit * 3,  # Over-fetch since we'll filter
        offset=0,
    )

    # Filter by remote_url in metadata
    filtered = [e for e in events if e.metadata.get("remote_url") == remote_url]

    # Apply offset and limit after filtering
    paginated = filtered[offset:offset + limit]

    return [SemanticEventResponse.from_db(e) for e in paginated]
