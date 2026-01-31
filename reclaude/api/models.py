"""Pydantic models for API request/response schemas."""

from __future__ import annotations

from datetime import datetime
from typing import TYPE_CHECKING

from pydantic import BaseModel, Field

if TYPE_CHECKING:
    from reclaude.db import Learning, SemanticEvent


class SemanticEventResponse(BaseModel):
    """Response model for a semantic event."""

    id: int
    timestamp: datetime
    event_type: str
    session_id: str | None
    content: str
    metadata: dict

    class Config:
        from_attributes = True

    @classmethod
    def from_db(cls, event: SemanticEvent) -> SemanticEventResponse:
        """Create response from database model."""
        return cls(
            id=event.id,
            timestamp=event.timestamp,
            event_type=event.event_type,
            session_id=event.session_id,
            content=event.content,
            metadata=event.metadata,
        )


class EventsQueryParams(BaseModel):
    """Query parameters for events endpoint."""

    event_type: str | None = None
    session_id: str | None = None
    since: datetime | None = None
    cwd: str | None = None
    cwd_prefix: bool = True
    limit: int = Field(default=100, ge=1, le=1000)
    offset: int = Field(default=0, ge=0)


class LearningResponse(BaseModel):
    """Response model for a learning."""

    id: int
    timestamp: datetime
    session_id: str | None
    cwd: str | None
    zellij_session: str | None
    content: str
    repo_root: str | None = None
    remote_url: str | None = None
    repo_name: str | None = None
    branch: str | None = None
    is_worktree: bool | None = None

    class Config:
        from_attributes = True

    @classmethod
    def from_db(cls, learning: Learning) -> LearningResponse:
        """Create response from database model."""
        return cls(
            id=learning.id,
            timestamp=learning.timestamp,
            session_id=learning.session_id,
            cwd=learning.cwd,
            zellij_session=learning.zellij_session,
            content=learning.content,
            repo_root=learning.repo_root,
            remote_url=learning.remote_url,
            repo_name=learning.repo_name,
            branch=learning.branch,
            is_worktree=learning.is_worktree,
        )


class LearningCreate(BaseModel):
    """Request model for creating a learning."""

    content: str
    session_id: str | None = None
    cwd: str | None = None
    zellij_session: str | None = None
    repo_root: str | None = None
    remote_url: str | None = None
    repo_name: str | None = None
    branch: str | None = None
    is_worktree: bool | None = None


class LearningsQueryParams(BaseModel):
    """Query parameters for learnings endpoint."""

    cwd: str | None = None
    cwd_prefix: bool = True
    zellij_session: str | None = None
    since: datetime | None = None
    repo_root: str | None = None
    remote_url: str | None = None
    limit: int = Field(default=50, ge=1, le=500)


class SessionInfo(BaseModel):
    """Response model for session information."""

    session_id: str
    last_event_at: datetime
    event_count: int
    cwd: str | None
    start_cwd: str | None = None  # Directory session started in (for resuming)
    is_active: bool = False  # Whether session is currently active
    # Extended stats
    repos: list[str] = []
    branches: list[str] = []
    lines_added: int = 0
    lines_removed: int = 0
    learnings_count: int = 0
    tool_use_count: int = 0
    file_diff_count: int = 0
    compaction_count: int = 0
    user_prompt_count: int = 0
    # Summary fields
    first_prompt: str | None = None
    started_at: datetime | None = None
    duration_minutes: int | None = None
    files_modified: list[str] = []


class BaseUnitResponse(BaseModel):
    """Response model for a base unit."""

    id: int
    timestamp: datetime
    project: str | None
    session_start: datetime
    session_end: datetime
    session_gap_minutes: int | None
    event_ids: list[int]
    learnings_ids: list[int]
    event_count: int
    model: str
    content: str
    metadata: dict
    input_chars: int | None = None
    output_chars: int | None = None

    class Config:
        from_attributes = True


class FocusSnapshotResponse(BaseModel):
    """Response model for a focus snapshot."""

    id: int
    timestamp: datetime
    project: str | None
    time_scale: str
    period_start: datetime
    period_end: datetime
    focus_summary: str
    top_topics: list[str]
    event_count: int
    metadata: dict

    class Config:
        from_attributes = True


class RepoInfo(BaseModel):
    """Response model for repository information."""

    remote_url: str
    repo_name: str | None
    event_count: int
    last_event_at: datetime

    @classmethod
    def from_db_dict(cls, data: dict) -> RepoInfo:
        """Create response from database dict result."""
        return cls(
            remote_url=data["remote_url"],
            repo_name=data["repo_name"],
            event_count=data["event_count"],
            last_event_at=data["last_event_at"],
        )


class StatisticsResponse(BaseModel):
    """Response model for statistics."""

    total_events: int
    events_by_type: dict[str, int]
    total_sessions: int
    total_learnings: int
    event_types: list[str]
    repos: list[RepoInfo] = []


class PaginatedResponse(BaseModel):
    """Generic paginated response wrapper."""

    items: list
    total: int
    limit: int
    offset: int
    has_more: bool


class SemanticSearchRequest(BaseModel):
    """Request for semantic search."""

    query: str = Field(..., min_length=1, max_length=2000)
    limit: int = Field(20, ge=1, le=100)
    distance_threshold: float | None = Field(None, ge=0.0, le=2.0)
    remote_url: str | None = None
    repo_root: str | None = None
    since: datetime | None = None


class SemanticLearningResult(BaseModel):
    """Learning with similarity score."""

    learning: LearningResponse
    distance: float
    similarity_pct: float  # 0-100, derived from distance


class LearningsTimeSeriesPoint(BaseModel):
    """Single point in time series."""

    date: str  # ISO date (day granularity)
    count: int


class LearningsAnalytics(BaseModel):
    """Analytics summary for learnings."""

    total_count: int
    by_repo: dict[str, int]
    by_branch: dict[str, int]
    time_series: list[LearningsTimeSeriesPoint]
    avg_content_length: float
    date_range_start: str | None
    date_range_end: str | None
