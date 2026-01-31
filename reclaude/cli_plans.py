from __future__ import annotations

import argparse
import hashlib
import sys
from datetime import datetime, timezone
from pathlib import Path

from .db import CaptureDB, SemanticEventType

CLAUDE_PLANS_DIR = Path.home() / ".claude" / "plans"


def _content_hash(content: str) -> str:
    return hashlib.sha256(content.encode()).hexdigest()[:16]


def cmd_plans_sync(args: argparse.Namespace) -> int:
    """Discover and ingest ~/.claude/plans/*.md as PLAN_FILE events."""
    db = CaptureDB()
    plans_dir = CLAUDE_PLANS_DIR

    if not plans_dir.is_dir():
        print(f"No plans directory at {plans_dir}", file=sys.stderr)
        return 0

    plan_files = sorted(plans_dir.glob("*.md"))
    if not plan_files:
        print("No plan files found.")
        return 0

    # Get existing plan file events keyed by slug → content_hash
    existing = db.query_events(
        event_type=SemanticEventType.PLAN_FILE,
        limit=10_000,
    )
    # Collect all known hashes per slug - skip if current hash already exists
    existing_hashes: dict[str, set[str]] = {}
    for e in existing:
        slug = e.metadata.get("slug", "")
        content_hash = e.metadata.get("content_hash", "")
        if slug:
            existing_hashes.setdefault(slug, set()).add(content_hash)

    new_count = 0
    updated_count = 0
    skipped_count = 0

    for path in plan_files:
        slug = path.stem
        content = path.read_text()
        chash = _content_hash(content)

        known = existing_hashes.get(slug, set())
        if chash in known:
            skipped_count += 1
            continue

        mtime = datetime.fromtimestamp(path.stat().st_mtime, tz=timezone.utc)

        db.insert_event(
            event_type=SemanticEventType.PLAN_FILE,
            content=content,
            timestamp=mtime,
            metadata={
                "slug": slug,
                "path": str(path),
                "size_bytes": len(content.encode()),
                "content_hash": chash,
            },
        )

        if not known:
            new_count += 1
        else:
            updated_count += 1

    print(f"Plans: {len(plan_files)} found, {new_count} new, {updated_count} updated, {skipped_count} unchanged")
    return 0
