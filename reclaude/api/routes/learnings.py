"""Learning-related API routes."""

from datetime import datetime

from fastapi import APIRouter, Depends, HTTPException, Query

from reclaude.db import CaptureDB

from ..deps import get_db
from ..models import LearningCreate, LearningResponse

router = APIRouter(prefix="/learnings", tags=["learnings"])


@router.get("", response_model=list[LearningResponse])
def list_learnings(
    cwd: str | None = Query(None, description="Filter by working directory"),
    cwd_prefix: bool = Query(True, description="Match cwd as prefix (includes subdirs)"),
    zellij_session: str | None = Query(None, description="Filter by Zellij session"),
    since: datetime | None = Query(None, description="Only learnings after this timestamp"),
    repo_root: str | None = Query(None, description="Filter by repo root"),
    remote_url: str | None = Query(None, description="Filter by git remote URL"),
    limit: int = Query(50, ge=1, le=500, description="Maximum results"),
    db: CaptureDB = Depends(get_db),
) -> list[LearningResponse]:
    """Query learnings with optional filters."""
    learnings = db.query_learnings(
        cwd=cwd,
        cwd_prefix=cwd_prefix,
        zellij_session=zellij_session,
        since=since,
        repo_root=repo_root,
        remote_url=remote_url,
        limit=limit,
    )
    return [LearningResponse.from_db(learning) for learning in learnings]


@router.post("", response_model=LearningResponse, status_code=201)
def create_learning(
    learning: LearningCreate,
    db: CaptureDB = Depends(get_db),
) -> LearningResponse:
    """Create a new learning."""
    learning_id = db.insert_learning(
        content=learning.content,
        session_id=learning.session_id,
        cwd=learning.cwd,
        zellij_session=learning.zellij_session,
        repo_root=learning.repo_root,
        remote_url=learning.remote_url,
        repo_name=learning.repo_name,
        branch=learning.branch,
        is_worktree=learning.is_worktree,
    )

    # Fetch the created learning to return it
    learnings = db.query_learnings(limit=1)
    if not learnings or learnings[0].id != learning_id:
        raise HTTPException(status_code=500, detail="Failed to create learning")

    return LearningResponse.from_db(learnings[0])
