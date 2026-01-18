"""Session-related API routes."""

from fastapi import APIRouter, Depends

from reclaude.db import CaptureDB

from ..deps import get_db
from ..models import SessionInfo

router = APIRouter(prefix="/sessions", tags=["sessions"])


@router.get("", response_model=list[SessionInfo])
def list_sessions(
    db: CaptureDB = Depends(get_db),
) -> list[SessionInfo]:
    """List all sessions with metadata and extended stats."""
    sessions = db.get_sessions_with_info()
    result = []
    for session_id, last_ts, count, cwd in sessions:
        stats = db.get_session_extended_stats(session_id)
        result.append(
            SessionInfo(
                session_id=session_id,
                last_event_at=last_ts,
                event_count=count,
                cwd=cwd,
                repos=stats["repos"],
                branches=stats["branches"],
                lines_added=stats["lines_added"],
                lines_removed=stats["lines_removed"],
                learnings_count=stats["learnings_count"],
                tool_use_count=stats["tool_use_count"],
                file_diff_count=stats["file_diff_count"],
                compaction_count=stats["compaction_count"],
                user_prompt_count=stats["user_prompt_count"],
                first_prompt=stats["first_prompt"],
                started_at=stats["started_at"],
                duration_minutes=stats["duration_minutes"],
                files_modified=stats["files_modified"],
            )
        )
    return result
