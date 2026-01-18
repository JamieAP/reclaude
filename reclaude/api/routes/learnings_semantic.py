"""Semantic search API routes for learnings."""

from fastapi import APIRouter, Depends, HTTPException, Query

from reclaude.db import CaptureDB
from reclaude.embeddings import embed_text, get_learning_embedding

from ..deps import get_db
from ..models import (
    LearningResponse,
    SemanticLearningResult,
    SemanticSearchRequest,
)

router = APIRouter(prefix="/learnings/semantic", tags=["learnings-semantic"])


@router.post("/search", response_model=list[SemanticLearningResult])
def semantic_search(
    request: SemanticSearchRequest,
    db: CaptureDB = Depends(get_db),
) -> list[SemanticLearningResult]:
    """Search learnings by semantic similarity."""
    try:
        query_embedding = embed_text(request.query)
    except Exception as e:
        raise HTTPException(status_code=503, detail=f"Embedding service error: {e}")

    results = db.query_learnings_semantic(
        query_embedding=query_embedding,
        limit=request.limit,
        distance_threshold=request.distance_threshold,
        remote_url=request.remote_url,
        repo_root=request.repo_root,
        since=request.since,
    )

    if not results:
        return []

    # Convert distance to similarity percentage (cosine distance: 0=identical, 2=opposite)
    return [
        SemanticLearningResult(
            learning=LearningResponse.from_db(learning),
            distance=dist,
            similarity_pct=max(0, min(100, (1 - dist / 2) * 100)),
        )
        for learning, dist in results
    ]


@router.get("/{learning_id}/similar", response_model=list[SemanticLearningResult])
def find_similar(
    learning_id: int,
    limit: int = Query(10, ge=1, le=50),
    db: CaptureDB = Depends(get_db),
) -> list[SemanticLearningResult]:
    """Find learnings similar to a given learning."""
    embedding = get_learning_embedding(db, learning_id)
    if embedding is None:
        raise HTTPException(
            status_code=404, detail="Learning not found or not embedded"
        )

    results = db.query_learnings_semantic(
        query_embedding=embedding,
        limit=limit + 1,  # +1 to exclude self
    )

    # Filter out the source learning and convert
    return [
        SemanticLearningResult(
            learning=LearningResponse.from_db(learning),
            distance=dist,
            similarity_pct=max(0, min(100, (1 - dist / 2) * 100)),
        )
        for learning, dist in results
        if learning.id != learning_id
    ][:limit]
