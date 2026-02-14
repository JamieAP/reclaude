from __future__ import annotations

import argparse
import json
import os
import sys

from .cli_utils import _resolve_session
from .db import CaptureDB
from .log import get_logger


def cmd_learn(args: argparse.Namespace) -> int:
    from .capture import get_git_context

    db = CaptureDB()

    if args.content:
        content = args.content
    elif not sys.stdin.isatty():
        content = sys.stdin.read()
    else:
        print("No content provided. Pass as argument or pipe via stdin.", file=sys.stderr)
        return 2

    content = content.strip()
    if not content:
        print("Empty content.", file=sys.stderr)
        return 2

    session_id = args.session or os.environ.get("RECLAUDE_SESSION_ID")
    cwd = os.getcwd()
    zellij_session = os.environ.get("ZELLIJ_SESSION_NAME")

    # Get git context for worktree reconciliation
    git_ctx = get_git_context(cwd)

    learning_id = db.insert_learning(
        content,
        session_id=session_id,
        cwd=cwd,
        zellij_session=zellij_session,
        repo_root=str(git_ctx.get("repo_root")) if git_ctx.get("repo_root") else None,
        remote_url=str(git_ctx.get("remote_url")) if git_ctx.get("remote_url") else None,
        repo_name=str(git_ctx.get("repo_name")) if git_ctx.get("repo_name") else None,
        branch=str(git_ctx.get("branch")) if git_ctx.get("branch") else None,
        is_worktree=bool(git_ctx.get("is_worktree")) if git_ctx.get("is_worktree") is not None else None,
    )

    # Log learning
    get_logger().info(
        "learning_stored",
        project=os.path.basename(cwd),
        content_chars=len(content),
        learning_id=learning_id,
    )

    print(f"Saved learning id={learning_id}")
    return 0


def _ensure_learnings_embedded(db: CaptureDB) -> None:
    """Lazy embedding: batch embed any learnings without embeddings."""
    unembedded_ids = db.get_unembedded_learning_ids(limit=500)
    if not unembedded_ids:
        return

    from reclaude.embeddings import embed_batch

    # Get learning content
    learnings_to_embed = []
    with db.connection() as conn:
        for lid in unembedded_ids:
            row = conn.execute("SELECT * FROM learnings WHERE id = ?", (lid,)).fetchone()
            if row:
                learnings_to_embed.append((lid, row["content"]))

    if not learnings_to_embed:
        return

    print(f"Embedding {len(learnings_to_embed)} new learnings...", file=sys.stderr)

    try:
        texts = [content for _, content in learnings_to_embed]
        embeddings = embed_batch(texts)

        # Store embeddings
        for (lid, _), embedding in zip(learnings_to_embed, embeddings):
            if embedding:
                db.insert_learning_embedding(lid, embedding)

        print(f"Embedded {len(embeddings)} learnings", file=sys.stderr)
    except Exception as e:
        print(f"Embedding failed: {e}", file=sys.stderr)


def _cmd_learnings_semantic(db: CaptureDB, args: argparse.Namespace, query: str) -> int:
    """Semantic search over learnings using Gemini embeddings."""
    from reclaude.embeddings import embed_text
    from reclaude.git import get_git_context

    # Get optional repo filter from current context
    remote_url = None
    repo_root = None
    if not args.all:
        ctx = get_git_context(os.getcwd())
        remote_url = ctx.get("remote_url")
        repo_root = ctx.get("repo_root")

    # Generate embedding for query
    print(f"Searching for: {query}", file=sys.stderr)
    query_embedding = embed_text(query)

    # Semantic search
    results = db.query_learnings_semantic(
        query_embedding=query_embedding,
        limit=args.limit,
        remote_url=remote_url if not args.all else None,
        repo_root=repo_root if not args.all else None,
    )

    get_logger().info("semantic_search", query=query, results=len(results))

    if not results:
        print("No matching learnings found")
        return 0

    # Output formatting
    use_human = args.human
    use_json = args.json

    if use_json:
        payload = [
            {
                "id": l.id,
                "distance": round(dist, 4),
                "timestamp": l.timestamp.isoformat(),
                "content": l.content if args.full else l.content[:500],
                "remote_url": l.remote_url,
                "repo_name": l.repo_name,
            }
            for l, dist in results
        ]
        print(json.dumps(payload, indent=2, default=str))
        return 0

    # Default: compact or human output with distance scores
    for l, dist in results:
        ts = l.timestamp.strftime("%Y-%m-%d %H:%M")
        score = f"[{1 - dist:.2f}]"  # Convert distance to similarity score

        if use_human or args.full:
            print(f"{score} [{ts}] id={l.id}")
            print(l.content)
            print()
        else:
            preview = l.content[:150].replace("\n", " ")
            if len(l.content) > 150:
                preview += "..."
            print(f"{score} {preview}")

    return 0


def _cmd_learnings_extract(db: CaptureDB, session_id: str, commit: bool = False) -> int:
    """Extract learnings from a session using Gemini."""
    import json as json_module
    from datetime import datetime

    print(f"Loading session {session_id}...", file=sys.stderr)

    # Load all events for the session
    with db.connection() as conn:
        rows = conn.execute(
            "SELECT timestamp, event_type, content FROM semantic_events WHERE session_id = ? ORDER BY timestamp",
            (session_id,),
        ).fetchall()

    if not rows:
        print(f"No events found for session {session_id}", file=sys.stderr)
        return 1

    # Get session time range
    first_ts = datetime.fromisoformat(rows[0]["timestamp"].replace("Z", "+00:00"))
    last_ts = datetime.fromisoformat(rows[-1]["timestamp"].replace("Z", "+00:00"))

    # Format events for Gemini (truncate very long content)
    formatted_events = []
    total_chars = 0
    max_chars = 800_000  # Leave room for prompt (~1M context)

    for row in rows:
        content = row["content"]
        if len(content) > 2000:
            content = content[:2000] + "... [truncated]"
        line = f"[{row['timestamp']}] {row['event_type']}: {content}"
        if total_chars + len(line) > max_chars:
            formatted_events.append("[... remaining events truncated for context limit ...]")
            break
        formatted_events.append(line)
        total_chars += len(line)

    events_text = "\n".join(formatted_events)
    print(f"Loaded {len(rows)} events ({total_chars:,} chars)", file=sys.stderr)

    # Gemini extraction prompt
    prompt = """You are analyzing a Claude Code session to extract learnings.

A "learning" is a concise, reusable insight such as:
- Technical discoveries (how something works, why something failed)
- Pattern recognition (this approach works well for X)
- Tool/API quirks worth remembering
- Debugging insights that would help future sessions
- Architecture decisions and their rationale
- Gotchas or edge cases discovered

Extract 3-10 learnings from this session. For each learning:
1. Use a timestamp from the relevant part of the session
2. Write a concise 1-3 sentence summary
3. Include relevant context (file names, commands, error messages)

Output JSON array with objects containing:
- "timestamp": ISO timestamp from session
- "content": the learning text

SESSION EVENTS:
"""

    try:
        import google.generativeai as genai
    except ImportError:
        print("google-generativeai not installed. Run: uv pip install google-generativeai", file=sys.stderr)
        return 1

    api_key = os.environ.get("GEMINI_API_KEY")
    if not api_key:
        print("GEMINI_API_KEY not set", file=sys.stderr)
        return 1

    genai.configure(api_key=api_key)
    model = genai.GenerativeModel("gemini-1.5-flash")

    print("Extracting learnings with Gemini...", file=sys.stderr)
    response = model.generate_content(
        prompt + events_text,
        generation_config=genai.GenerationConfig(
            response_mime_type="application/json",
            temperature=0.3,
        ),
    )

    try:
        learnings = json_module.loads(response.text)
    except json_module.JSONDecodeError:
        print(f"Failed to parse Gemini response: {response.text[:500]}", file=sys.stderr)
        return 1

    if not learnings:
        print("No learnings extracted", file=sys.stderr)
        return 0

    print(f"\nExtracted {len(learnings)} learnings:\n")

    for i, learning in enumerate(learnings, 1):
        ts = learning.get("timestamp", "?")
        content = learning.get("content", "")
        print(f"{i}. [{ts}]")
        print(f"   {content}\n")

    if commit:
        from .capture import get_git_context

        cwd = os.getcwd()
        git_ctx = get_git_context(cwd)

        print("Saving learnings to database...", file=sys.stderr)
        for learning in learnings:
            db.insert_learning(
                learning.get("content", ""),
                session_id=session_id,
                cwd=cwd,
                repo_root=str(git_ctx.get("repo_root")) if git_ctx.get("repo_root") else None,
                remote_url=str(git_ctx.get("remote_url")) if git_ctx.get("remote_url") else None,
                repo_name=str(git_ctx.get("repo_name")) if git_ctx.get("repo_name") else None,
                branch=str(git_ctx.get("branch")) if git_ctx.get("branch") else None,
            )
        print(f"Saved {len(learnings)} learnings")
    else:
        print("(use --commit to save these learnings)")

    return 0


def cmd_learnings(args: argparse.Namespace) -> int:
    db = CaptureDB()

    # Check for extract mode
    extract_session = args.extract
    if extract_session:
        session_id = _resolve_session(db, extract_session)
        if not session_id:
            return 2
        return _cmd_learnings_extract(db, session_id, commit=args.commit)

    # Ensure embeddings are up to date for semantic search
    _ensure_learnings_embedded(db)

    # Check for semantic search mode
    semantic_query = args.semantic
    if semantic_query:
        return _cmd_learnings_semantic(db, args, semantic_query)

    # Session filtering
    session_id = None
    if args.session:
        from .cli_utils import _session_filter
        session_id = _session_filter(db, args.session)
        if not session_id:
            return 1

    # CWD scoping: filter by remote_url unless --all
    remote_url = None
    if not args.all:
        from reclaude.git import get_git_context
        ctx = get_git_context(os.getcwd())
        remote_url = ctx.get("remote_url")

    # Regular listing
    learnings = db.query_learnings(limit=args.limit, session_id=session_id, remote_url=remote_url)

    if not learnings:
        print("No learnings found")
        return 0

    if args.json:
        payload = [
            {
                "id": l.id,
                "timestamp": l.timestamp.isoformat(),
                "content": l.content if args.full else l.content[:500],
                "session_id": l.session_id,
                "cwd": l.cwd,
                "remote_url": l.remote_url,
                "repo_name": l.repo_name,
            }
            for l in learnings
        ]
        print(json.dumps(payload, indent=2, default=str))
        return 0

    for l in learnings:
        ts = l.timestamp.strftime("%Y-%m-%d %H:%M")
        repo = l.repo_name or ""

        if args.full:
            print(f"[{ts}] id={l.id} {repo}")
            print(l.content)
            print()
        else:
            preview = l.content[:150].replace("\n", " ")
            if len(l.content) > 150:
                preview += "..."
            print(f"[{ts}] {preview}")

    return 0
