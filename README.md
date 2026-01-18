# reclaude

Semantic event capture and learnings system for Claude Code sessions.

## Features

- **Event Capture** - Prompts, responses, tool usage, file diffs, session lifecycle
- **Learnings** - Persistent cross-session memory for patterns and discoveries
- **Transcript Extraction** - Batch extraction of summaries, plans, and thinking from session transcripts
- **Focus Synthesis** - Gemini-powered activity summaries at hour/day/week scales
- **Web Dashboard** - Browse sessions, events, learnings with semantic search
- **Semantic Search** - Vector similarity via Gemini embeddings

## Installation

**Prerequisites:** [uv](https://docs.astral.sh/uv/) (recommended) or Python 3.11+

```bash
# Add marketplace and install
claude plugin marketplace add JamieAP/reclaude
claude plugin install reclaude@reclaude
```

Hooks auto-register and dependencies auto-install on first use via `uv run`.

### Install CLI

To query captured data from your terminal, install the CLI:

```bash
uv tool install git+https://github.com/JamieAP/reclaude
```

Or run `/reclaude:setup` in Claude Code for guided installation.

Both the plugin hooks and CLI use the same database at `~/.reclaude/capture.db`.

## CLI Reference

### Core Commands

```bash
reclaude status              # Capture statistics
reclaude sessions [N]        # List recent sessions
reclaude events [N]          # Recent events (filterable)
reclaude files [pattern]     # Files touched by Claude
reclaude context             # LLM-ready session context
```

### Learnings

```bash
reclaude learn "insight"     # Store a learning
reclaude learnings [N]       # Query learnings (current project)
reclaude learnings --all     # All projects
reclaude learnings --semantic "query"  # Semantic search
```

### Transcripts

```bash
reclaude transcripts sync    # Archive all transcripts
reclaude transcripts list    # List archived
reclaude transcripts extract # Extract semantic events
reclaude transcripts extract --file <path>  # Single file
reclaude transcripts export <session-id>    # Export to JSONL
```

### Focus

```bash
reclaude focus hour          # Last hour summary
reclaude focus day           # Last 24h summary
reclaude focus week          # Last 7d summary
```

### Utilities

```bash
reclaude ui                  # Launch web dashboard (localhost:8420)
reclaude path                # Show database/log paths
reclaude log                 # Tail capture log
```

## Slash Commands

| Command | Description |
|---------|-------------|
| `/reclaude:learn <content>` | Store a learning |
| `/reclaude:learnings` | Query learnings for current project |

## Event Types

| Type | Description |
|------|-------------|
| `user_prompt` | User prompts to Claude |
| `assistant` | Claude's text responses |
| `plan` | Planning text before tool use |
| `thinking` | Claude's reasoning blocks |
| `tool_use` | Tool invocations with I/O |
| `file_diff` | Edit/Write as unified diffs |
| `compaction` | Session summaries |
| `session_start` | Session startup/resume |
| `session_end` | Session completion |
| `subagent_stop` | Subagent completion |
| `plan_file` | Implementation plans from ~/.claude/plans/ |

## Programmatic Access

```python
from reclaude.db import CaptureDB, SemanticEventType

db = CaptureDB()

# Query events
events = db.query_events(
    event_type=SemanticEventType.USER_PROMPT,
    limit=10
)

# Get counts
counts = db.event_counts_by_type()
```

## Data Locations

| Path | Contents |
|------|----------|
| `~/.reclaude/capture.db` | SQLite database |
| `~/.reclaude/capture.log` | Structured log |
| `~/.reclaude/transcripts/` | Archived transcripts (zstd) |

## Development

```bash
uv sync              # Install dependencies
uv run pytest        # Run tests
uv run reclaude ui   # Start dev server
```

## License

MIT
