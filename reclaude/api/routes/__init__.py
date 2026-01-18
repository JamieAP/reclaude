"""API route modules."""

from .events import router as events_router
from .focus import router as focus_router
from .learnings import router as learnings_router
from .learnings_analytics import router as learnings_analytics_router
from .learnings_semantic import router as learnings_semantic_router
from .personas import router as personas_router
from .repos import router as repos_router
from .sessions import router as sessions_router
from .statistics import router as statistics_router

__all__ = [
    "events_router",
    "focus_router",
    "learnings_router",
    "learnings_analytics_router",
    "learnings_semantic_router",
    "personas_router",
    "repos_router",
    "sessions_router",
    "statistics_router",
]
