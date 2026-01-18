"""CLI commands for session transcript archival."""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path

import zstandard as zstd

from .db import CaptureDB


CLAUDE_PROJECTS_DIR = Path.home() / ".claude" / "projects"
ZSTD_COMPRESSION_LEVEL = 19  # High compression for archival


@dataclass
class TranscriptInfo:
    """Discovered transcript file info."""
    path: Path
    session_id: str
    parent_session_id: str | None
    size_bytes: int


def discover_transcripts() -> list[TranscriptInfo]:
    """Find all transcript JSONL files in Claude Code projects directory."""
    if not CLAUDE_PROJECTS_DIR.exists():
        return []

    results: list[TranscriptInfo] = []

    for project_dir in CLAUDE_PROJECTS_DIR.iterdir():
        if not project_dir.is_dir():
            continue

        # Main session transcripts: <project>/<uuid>.jsonl
        for jsonl in project_dir.glob("*.jsonl"):
            if jsonl.name.startswith("agent-"):
                # Top-level agent files (legacy?)
                results.append(TranscriptInfo(
                    path=jsonl,
                    session_id=jsonl.stem,
                    parent_session_id=None,
                    size_bytes=jsonl.stat().st_size,
                ))
            else:
                # Main session
                results.append(TranscriptInfo(
                    path=jsonl,
                    session_id=jsonl.stem,
                    parent_session_id=None,
                    size_bytes=jsonl.stat().st_size,
                ))

        # Subagent transcripts: <project>/<parent-uuid>/subagents/agent-*.jsonl
        for jsonl in project_dir.glob("*/subagents/agent-*.jsonl"):
            parent_id = jsonl.parent.parent.name
            results.append(TranscriptInfo(
                path=jsonl,
                session_id=jsonl.stem,
                parent_session_id=parent_id,
                size_bytes=jsonl.stat().st_size,
            ))

    return results


def compress_transcript(path: Path) -> tuple[bytes, int]:
    """Read and compress a transcript file with zstd.

    Returns:
        (compressed_bytes, uncompressed_size)
    """
    content = path.read_bytes()
    cctx = zstd.ZstdCompressor(level=ZSTD_COMPRESSION_LEVEL)
    compressed = cctx.compress(content)
    return compressed, len(content)


def decompress_transcript(compressed: bytes) -> bytes:
    """Decompress a zstd-compressed transcript."""
    dctx = zstd.ZstdDecompressor()
    return dctx.decompress(compressed)


def extract_metadata(path: Path) -> dict:
    """Extract metadata from first line of transcript JSONL."""
    try:
        with open(path, "r") as f:
            first_line = f.readline().strip()
            if first_line:
                data = json.loads(first_line)
                return {
                    "cwd": data.get("cwd"),
                    "version": data.get("version"),
                    "gitBranch": data.get("gitBranch"),
                }
    except (json.JSONDecodeError, OSError):
        pass
    return {}


def format_size(size_bytes: int) -> str:
    """Format bytes as human-readable size."""
    for unit in ("B", "KB", "MB", "GB"):
        if size_bytes < 1024:
            return f"{size_bytes:.1f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.1f} TB"


def cmd_transcripts(args: argparse.Namespace) -> int:
    """Main transcripts command dispatcher."""
    subcommand = args.transcripts_command

    if subcommand == "sync":
        return cmd_sync(args)
    elif subcommand == "list":
        return cmd_list(args)
    elif subcommand == "stats":
        return cmd_stats(args)
    elif subcommand == "export":
        return cmd_export(args)
    elif subcommand == "show":
        return cmd_show(args)
    elif subcommand == "extract":
        return cmd_extract(args)
    else:
        print("Usage: reclaude transcripts {sync|list|stats|export|show|extract}")
        return 1


def cmd_sync(args: argparse.Namespace) -> int:
    """Scan and archive all transcripts."""
    db = CaptureDB()

    print(f"Scanning {CLAUDE_PROJECTS_DIR}...")
    transcripts = discover_transcripts()

    if not transcripts:
        print("No transcripts found")
        return 0

    main_count = sum(1 for t in transcripts if t.parent_session_id is None)
    sub_count = len(transcripts) - main_count
    print(f"Found {len(transcripts)} transcripts ({main_count} main, {sub_count} subagents)")

    # Get existing archived sessions with sizes
    archived = db.get_archived_session_ids()
    archived_lookup = {sid: size for sid, size in archived}

    new_count = 0
    changed_count = 0
    skipped_count = 0
    total_compressed = 0

    for t in transcripts:
        existing_size = archived_lookup.get(t.session_id)

        if existing_size is not None and existing_size == t.size_bytes:
            skipped_count += 1
            continue

        # Compress and archive
        compressed, uncompressed_size = compress_transcript(t.path)
        metadata = extract_metadata(t.path)

        _, is_new = db.upsert_transcript(
            session_id=t.session_id,
            content=compressed,
            size_bytes=uncompressed_size,
            compressed_bytes=len(compressed),
            transcript_path=str(t.path),
            parent_session_id=t.parent_session_id,
            metadata=metadata,
        )

        if is_new:
            new_count += 1
        else:
            changed_count += 1

        total_compressed += len(compressed)

    print(f"  New: {new_count}")
    print(f"  Updated: {changed_count}")
    print(f"  Skipped: {skipped_count}")

    if new_count + changed_count > 0:
        print(f"Archived: {format_size(total_compressed)} compressed")

    return 0


def cmd_list(args: argparse.Namespace) -> int:
    """List archived transcripts."""
    db = CaptureDB()
    transcripts = db.query_transcripts(
        include_subagents=args.subagents,
        limit=args.limit,
    )

    if not transcripts:
        print("No archived transcripts")
        return 0

    for t in transcripts:
        ts_str = t.archived_at.strftime("%Y-%m-%d %H:%M")
        size_str = format_size(t.size_bytes)
        compressed_str = format_size(t.compressed_bytes) if t.compressed_bytes else "?"

        parent_marker = f" (sub of {t.parent_session_id[:8]})" if t.parent_session_id else ""
        project = ""
        if t.metadata.get("cwd"):
            project = t.metadata["cwd"].rstrip("/").rsplit("/", 1)[-1]

        print(f"[{ts_str}] {t.session_id[:12]}... {size_str} → {compressed_str}{parent_marker} {project}")

    return 0


def cmd_stats(args: argparse.Namespace) -> int:
    """Show transcript archive statistics."""
    db = CaptureDB()
    stats = db.get_transcript_stats()

    if stats["total"] == 0:
        print("No archived transcripts")
        print("Run: reclaude transcripts sync")
        return 0

    print(f"Archived: {stats['total']} transcripts")
    print(f"  Main sessions: {stats['main_count']}")
    print(f"  Subagents: {stats['subagent_count']}")
    print()

    total_size = stats["total_size_bytes"]
    compressed_size = stats["total_compressed_bytes"]
    ratio = total_size / compressed_size if compressed_size else 0

    print(f"Storage: {format_size(total_size)} → {format_size(compressed_size)} ({ratio:.1f}x compression)")
    print(f"Oldest: {stats['oldest']}")
    print(f"Newest: {stats['newest']}")

    return 0


def cmd_export(args: argparse.Namespace) -> int:
    """Export a transcript back to JSONL."""
    db = CaptureDB()

    # Find transcript by prefix match
    session_id = args.session_id
    transcripts = db.query_transcripts(limit=1000)
    matches = [t for t in transcripts if t.session_id.startswith(session_id)]

    if not matches:
        print(f"No transcript found matching: {session_id}")
        return 1

    if len(matches) > 1:
        print(f"Multiple matches for '{session_id}':")
        for t in matches[:5]:
            print(f"  {t.session_id}")
        return 1

    transcript = matches[0]
    compressed = db.get_transcript_content(transcript.session_id)

    if not compressed:
        print(f"No content found for: {transcript.session_id}")
        return 1

    decompressed = decompress_transcript(compressed)

    if args.output:
        output_path = Path(args.output)
        output_path.write_bytes(decompressed)
        print(f"Exported to: {output_path}")
    else:
        sys.stdout.buffer.write(decompressed)

    return 0


def cmd_show(args: argparse.Namespace) -> int:
    """Show decompressed transcript content."""
    db = CaptureDB()

    session_id = args.session_id
    transcripts = db.query_transcripts(limit=1000)
    matches = [t for t in transcripts if t.session_id.startswith(session_id)]

    if not matches:
        print(f"No transcript found matching: {session_id}")
        return 1

    if len(matches) > 1:
        print(f"Multiple matches for '{session_id}':")
        for t in matches[:5]:
            print(f"  {t.session_id}")
        return 1

    transcript = matches[0]
    compressed = db.get_transcript_content(transcript.session_id)

    if not compressed:
        print(f"No content found for: {transcript.session_id}")
        return 1

    decompressed = decompress_transcript(compressed)

    # Pretty print JSONL
    for line in decompressed.decode("utf-8").splitlines():
        if line.strip():
            try:
                obj = json.loads(line)
                print(json.dumps(obj, indent=2))
                print()
            except json.JSONDecodeError:
                print(line)

    return 0


def cmd_extract(args: argparse.Namespace) -> int:
    """Extract semantic events from transcripts."""
    from datetime import datetime, timezone

    from .extract import TranscriptExtractor

    db = CaptureDB()
    extractor = TranscriptExtractor(db)

    if args.file:
        # Single file extraction
        path = Path(args.file)
        print(f"Extracting from {path}...")
        result = extractor.extract_transcript(path, force_full=args.force)

        print(f"  Entries scanned: {result.entries_scanned}")
        print(f"  Events created: {result.events_created}")
        if result.by_type:
            for etype, count in sorted(result.by_type.items()):
                print(f"    {etype}: {count}")
        if result.errors:
            for err in result.errors:
                print(f"  Error: {err}", file=sys.stderr)

        return 1 if result.errors else 0

    else:
        # Batch extraction
        since = args.since or 24
        print(f"Extracting from transcripts modified in last {since}h...")

        if args.dry_run:
            cutoff = datetime.now(timezone.utc).timestamp() - (since * 3600)
            transcripts = discover_transcripts()
            count = sum(1 for t in transcripts if t.path.stat().st_mtime >= cutoff)
            print(f"Would process {count} transcripts (dry run)")
            return 0

        result = extractor.extract_all(since_hours=since, force_full=args.force)

        print(f"Transcripts processed: {result.transcripts_processed}")
        print(f"Total events created: {result.total_events}")
        if result.by_type:
            for etype, count in sorted(result.by_type.items()):
                print(f"  {etype}: {count}")
        if result.errors:
            print(f"Errors: {len(result.errors)}", file=sys.stderr)
            for err in result.errors[:5]:
                print(f"  {err}", file=sys.stderr)

        return 1 if result.errors else 0
