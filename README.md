# reclaude

Semantic event capture and analysis for Claude Code sessions.

## Features

- **Event Capture** - Prompts, responses, tool usage, file diffs, session lifecycle
- **Transcript Extraction** - Batch extraction of summaries, plans, and thinking from session transcripts
- **Full-Text Search** - FTS5-powered instant text search with boolean operators
- **Semantic Search** - Vector similarity via local ONNX embeddings (nomic-embed-text-v1.5)
- **Focus Synthesis** - Gemini-powered activity summaries at hour/day/week scales
- **Web Dashboard** - Browse sessions, events with search
- **fzf Integration** - Interactive browsing for events, sessions, and search results

## Installation

**Prerequisites:** Rust toolchain (cargo)

```bash
# Add marketplace and install plugin
claude plugin marketplace add JamieAP/reclaude
claude plugin install reclaude@reclaude
```

Hooks auto-register via `scripts/capture-hook.sh`.

### Install CLI

Build and install the binary:

```bash
cargo build --release
cp target/release/reclaude ~/.local/bin/reclaude
```

Or run `/reclaude:setup` in Claude Code for guided installation.

Data is stored in `~/.reclaude/metadata.db` (SQLite with FTS5 and sqlite-vec).

## CLI Reference

### Core Commands

```bash
reclaude status              # Capture statistics
reclaude sessions [N]        # List recent sessions
reclaude events [N]          # Recent events (filterable)
reclaude files [pattern]     # Files touched by Claude
reclaude chat                # View conversation around events
```

### Search

```bash
reclaude search "query"             # Full-text search
reclaude search "fix" --and "bug"   # Boolean operators
reclaude search --semantic "query"  # Vector similarity search
reclaude search --fzf               # Interactive results
```

### Transcripts

```bash
reclaude transcripts sync    # Archive all transcripts
reclaude transcripts list    # List archived
reclaude transcripts extract # Extract semantic events
```

### Focus

```bash
reclaude focus hour          # Last hour summary
reclaude focus day           # Last 24h summary
reclaude focus week          # Last 7d summary
```

### Embeddings

```bash
reclaude embed download      # Download ONNX model (~131MB)
reclaude embed status        # Show model status
reclaude embed backfill      # Vectorize historical events
```

### Utilities

```bash
reclaude ui                  # Launch web dashboard (localhost:8420)
reclaude log                 # Tail capture log
reclaude tag-session         # Tag current session
reclaude get-session <tag>   # Retrieve session by tag
```

## Event Types

| Type | Category | Description |
|------|----------|-------------|
| `user_prompt` | conversation | User prompts to Claude |
| `assistant` | conversation | Claude's text responses |
| `plan` | conversation | Planning text before tool use |
| `thinking` | conversation | Claude's reasoning blocks |
| `tool_use` | action | Tool invocations with I/O |
| `file_diff` | action | Edit/Write as unified diffs |
| `compaction` | system | Session summaries |
| `session_start` | lifecycle | Session startup/resume |
| `session_end` | lifecycle | Session completion |
| `subagent_spawn` | lifecycle | Subagent creation |
| `subagent_stop` | lifecycle | Subagent completion |
| `plan_file` | system | Implementation plans from ~/.claude/plans/ |

## Data Locations

| Path | Contents |
|------|----------|
| `~/.reclaude/metadata.db` | SQLite database (events, sessions, FTS, vectors) |
| `~/.reclaude/log/reclaude.jsonl` | Structured log |
| `~/.reclaude/models/` | ONNX embedding model |

## Development

```bash
cargo build --release        # Build release binary
cargo test                   # Run tests
just serve                   # API + Vite dev server
just install                 # Build and install to ~/.local/bin
```

## License

MIT
