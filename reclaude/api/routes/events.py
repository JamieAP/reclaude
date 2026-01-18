"""Event-related API routes."""

from datetime import datetime

from fastapi import APIRouter, Depends, HTTPException, Query

from reclaude.db import CaptureDB

from ..deps import get_db
from ..models import SemanticEventResponse

router = APIRouter(prefix="/events", tags=["events"])


@router.get("", response_model=list[SemanticEventResponse])
def list_events(
    event_type: str | None = Query(None, description="Filter by event type"),
    session_id: str | None = Query(None, description="Filter by session ID"),
    since: datetime | None = Query(None, description="Only events after this timestamp"),
    cwd: str | None = Query(None, description="Filter by working directory"),
    cwd_prefix: bool = Query(True, description="Match cwd as prefix (includes subdirs)"),
    limit: int = Query(100, ge=1, le=1000, description="Maximum results"),
    offset: int = Query(0, ge=0, description="Pagination offset"),
    db: CaptureDB = Depends(get_db),
) -> list[SemanticEventResponse]:
    """Query semantic events with optional filters."""
    events = db.query_events(
        event_type=event_type,
        session_id=session_id,
        since=since,
        cwd=cwd,
        cwd_prefix=cwd_prefix,
        limit=limit,
        offset=offset,
    )
    return [SemanticEventResponse.from_db(e) for e in events]


@router.get("/since/{since_id}", response_model=list[SemanticEventResponse])
def list_events_since(
    since_id: int,
    event_type: str | None = Query(None, description="Filter by event type"),
    cwd: str | None = Query(None, description="Filter by working directory"),
    cwd_prefix: bool = Query(True, description="Match cwd as prefix"),
    limit: int = Query(100, ge=1, le=1000, description="Maximum results"),
    db: CaptureDB = Depends(get_db),
) -> list[SemanticEventResponse]:
    """Query events with ID greater than since_id (for polling/incremental updates)."""
    events = db.query_events_since_id(
        since_id=since_id,
        event_type=event_type,
        cwd=cwd,
        cwd_prefix=cwd_prefix,
        limit=limit,
    )
    return [SemanticEventResponse.from_db(e) for e in events]


@router.get("/{event_id}", response_model=SemanticEventResponse)
def get_event(
    event_id: int,
    db: CaptureDB = Depends(get_db),
) -> SemanticEventResponse:
    """Get a single event by ID."""
    event = db.get_event_by_id(event_id)
    if not event:
        raise HTTPException(status_code=404, detail="Event not found")
    return SemanticEventResponse.from_db(event)
