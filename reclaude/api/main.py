"""FastAPI application for reclaude REST API."""

from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles
from fastapi.responses import FileResponse

from .routes import events_router, focus_router, learnings_router, learnings_analytics_router, learnings_semantic_router, personas_router, repos_router, sessions_router, statistics_router


# Path to frontend dist directory (relative to this file)
FRONTEND_DIST = Path(__file__).parent.parent / "frontend" / "dist"


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan handler."""
    # Startup
    yield
    # Shutdown


def create_app() -> FastAPI:
    """Create and configure the FastAPI application."""
    app = FastAPI(
        title="reclaude API",
        description="REST API for reclaude semantic event capture",
        version="0.1.0",
        lifespan=lifespan,
    )

    # CORS middleware for local development
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],  # Allow all origins for local dev
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    # Register routers
    app.include_router(events_router, prefix="/api")
    app.include_router(focus_router, prefix="/api")
    app.include_router(learnings_router, prefix="/api")
    app.include_router(learnings_analytics_router, prefix="/api")
    app.include_router(learnings_semantic_router, prefix="/api")
    app.include_router(personas_router, prefix="/api")
    app.include_router(repos_router, prefix="/api")
    app.include_router(sessions_router, prefix="/api")
    app.include_router(statistics_router, prefix="/api")

    @app.get("/api/health")
    def health_check():
        """Health check endpoint."""
        return {"status": "ok"}

    # Serve SPA static files if frontend is built
    if FRONTEND_DIST.exists() and (FRONTEND_DIST / "index.html").exists():
        # Mount static assets
        app.mount("/assets", StaticFiles(directory=FRONTEND_DIST / "assets"), name="assets")

        # Serve index.html for all non-API routes (SPA routing)
        @app.get("/{full_path:path}")
        async def serve_spa(full_path: str):
            """Serve SPA for all non-API routes."""
            # Check if it's a static file that exists
            file_path = FRONTEND_DIST / full_path
            if file_path.exists() and file_path.is_file():
                return FileResponse(file_path)
            # Otherwise serve index.html for SPA routing
            return FileResponse(FRONTEND_DIST / "index.html")

    return app


# Default app instance
app = create_app()
