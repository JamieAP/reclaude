"""Dependency injection for FastAPI routes."""

from functools import lru_cache

from reclaude.db import CaptureDB


@lru_cache
def get_db() -> CaptureDB:
    """Get the shared database instance."""
    return CaptureDB()
