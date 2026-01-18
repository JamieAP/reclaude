"""
Embedding utilities for semantic search using Gemini.

Uses gemini-embedding-001 model (3072 dimensions).
"""

from __future__ import annotations

import struct
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from google.genai.types import EmbedContentResponse

# Model config
EMBEDDING_MODEL = "gemini-embedding-001"
EMBEDDING_DIM = 3072  # Full dimension output from gemini-embedding-001


def get_client():
    """Get Gemini client (lazy init to avoid import cost).

    Reads API key from ~/.gemini/api_key (same as intelligence commands).
    """
    from google import genai
    from pathlib import Path

    api_key_file = Path.home() / ".config" / "gemini-api-key"
    if api_key_file.exists():
        api_key = api_key_file.read_text().strip()
        return genai.Client(api_key=api_key)
    else:
        # Fall back to env var or default
        return genai.Client()


def embed_text(text: str) -> list[float]:
    """
    Generate embedding for text using Gemini.

    Args:
        text: The text to embed

    Returns:
        List of 3072 floats representing the embedding vector
    """
    client = get_client()
    response: EmbedContentResponse = client.models.embed_content(
        model=EMBEDDING_MODEL,
        contents=text,
    )
    return list(response.embeddings[0].values)


def embed_batch(texts: list[str], batch_size: int = 100) -> list[list[float]]:
    """
    Generate embeddings for multiple texts.

    Uses batched API calls (max 100 per batch) for efficiency.

    Args:
        texts: List of texts to embed
        batch_size: Max items per API call (Gemini limit is 100)

    Returns:
        List of embedding vectors (each 3072 floats)
    """
    if not texts:
        return []

    client = get_client()
    all_embeddings = []

    # Process in batches of 100 (Gemini API limit)
    for i in range(0, len(texts), batch_size):
        batch = texts[i:i + batch_size]
        response: EmbedContentResponse = client.models.embed_content(
            model=EMBEDDING_MODEL,
            contents=batch,
        )
        all_embeddings.extend([list(emb.values) for emb in response.embeddings])

    return all_embeddings


def embedding_to_blob(embedding: list[float]) -> bytes:
    """
    Convert embedding to binary blob for sqlite-vec storage.

    sqlite-vec expects vectors as packed floats in little-endian format.
    """
    return struct.pack(f"<{len(embedding)}f", *embedding)


def blob_to_embedding(blob: bytes) -> list[float]:
    """Convert binary blob back to embedding list."""
    count = len(blob) // 4  # 4 bytes per float
    return list(struct.unpack(f"<{count}f", blob))


def get_learning_embedding(db, learning_id: int) -> list[float] | None:
    """Get the stored embedding for a learning, or generate and store it.

    Args:
        db: CaptureDB instance
        learning_id: ID of the learning to get/generate embedding for

    Returns:
        Embedding vector (list of floats) or None if learning not found
        or sqlite-vec is unavailable
    """
    from reclaude.db import HAS_SQLITE_VEC

    if not HAS_SQLITE_VEC:
        return None

    # Check if embedding exists
    with db.connection() as conn:
        row = conn.execute(
            "SELECT embedding FROM vec_learnings WHERE rowid = ?",
            (learning_id,),
        ).fetchone()

        if row:
            return blob_to_embedding(row["embedding"])

        # Get learning content and generate embedding
        learning_row = conn.execute(
            "SELECT content FROM learnings WHERE id = ?",
            (learning_id,),
        ).fetchone()

        if not learning_row:
            return None

    # Generate and store embedding
    embedding = embed_text(learning_row["content"])
    db.insert_learning_embedding(learning_id, embedding)
    return embedding
