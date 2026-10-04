# reclaude

Reclaude captures Claude Code session events for local browsing, search, and
analysis. It stores prompts, responses, tool activity, file changes, and
transcripts in SQLite, with a CLI and web dashboard for reviewing them.

## Capabilities

- Capture session events through Claude Code hooks.
- Extract summaries, plans, and thinking blocks from archived transcripts.
- Search with SQLite FTS5 or local ONNX embeddings (`nomic-embed-text-v1.5`).
- Browse sessions and events in the CLI, with optional `fzf` selection.
- View captured activity and saved focus summaries in the web dashboard.
- Generate hour, day, or week summaries through Google's Gemini API.

## Privacy and data flow

### Local records

Installing the hooks captures full prompts, assistant responses, tool I/O, file
changes, plans, thinking blocks, and session context locally. SessionStart also
archives available Claude transcripts in the background. These records can
contain credentials, confidential code, paths, or private conversations. The
SQLite database and compressed transcript records are plaintext rather than
encrypted, and there is no automatic retention policy.

On Unix, state/log directories use `0700`. Database files and sidecars, logs, and
chat pager files use `0600`, including existing files opened through these paths.
Final-path symlinks are rejected. Unix helpers also refuse files owned by another
user or with multiple hardlinks before changing their permissions or contents.
Copies and backups made elsewhere are not restricted by the application. Unix
modes do not apply on other platforms; configure native owner-only ACLs there.
Keep your provider key file owner-only as well.

### External services

Normal capture does not post to Mattermost. Posting requires
`RECLAUDE_MMCTL_ENABLED=1` and an explicit destination, either
`RECLAUDE_MM_CHANNEL=team:channel` or both `RECLAUDE_MM_TEAM` and
`RECLAUDE_MM_CHANNEL`. It sends raw user/assistant content plus repository,
branch, working directory, and session context through the configured `mmctl`
account. Set `RECLAUDE_MMCTL_BIN` if the binary is not on PATH. Command diagnostics
omit message arguments and subprocess output. A saved session thread with a
different destination is refused; migrate or remove its local mapping before
posting to a newly configured destination.

`reclaude focus` sends selected session content to Google's Gemini API: prompts,
assistant text, plans, compaction/thinking, code diffs or file summaries, tool
summaries, timestamps, and project/path context. It uses a key from
`~/.config/gemini-api-key`. `reclaude focus hour --dry-run` previews only the first
3,000 characters of the request content, not the full export. The request uses a
key header; diagnostics omit credential-bearing URLs and provider error bodies.

### UI access

The UI/API binds to `127.0.0.1` by default and has no authentication. Browser
access is same-origin; Vite development uses a local `/api` proxy. Unrelated
origins are not granted CORS access. Keep the server on loopback or use a
separately managed authenticated proxy before binding it more widely. Local
processes and code running in the same UI origin can still access stored records.

## Installation

You need Rust and Cargo, plus Claude Code for the hook integration. Run the build
commands from the repository root:

```bash
cargo build --release
mkdir -p ~/.local/bin
cp target/release/reclaude ~/.local/bin/reclaude
```

Ensure `~/.local/bin` is on PATH, then register the plugin:

```bash
claude plugin marketplace add JamieAP/reclaude
claude plugin install reclaude@reclaude
```

The plugin registers hooks through `scripts/capture-hook.sh`. You can also run
`/reclaude:setup` in Claude Code for guided installation.

### Web dashboard

The dashboard needs Node.js and npm. Build it from the repository root:

```bash
cd frontend
npm ci
npm run build
cd ..
reclaude ui --no-open
```

Open `http://127.0.0.1:8420`. Run this command from the checkout so the server can
find `frontend/dist`, or install the built files under
`~/.reclaude/frontend/dist`. Without a frontend build, the server provides only
the API. See the [frontend guide](frontend/README.md) for development setup.

## CLI reference

### Browse and search

```bash
reclaude status
reclaude sessions -n 10
reclaude events -n 20
reclaude files "src/"
reclaude chat
reclaude search "query"
reclaude search "fix" --and "bug"
reclaude search --semantic "query"
reclaude search "query" --fzf
```

Sessions and searches default to the current directory; use `--all` to query
across projects. Interactive selection requires `fzf` on PATH. Semantic search
requires a downloaded model and embedded events:

```bash
reclaude embed download
reclaude embed status
reclaude embed backfill
```

### Transcripts and summaries

```bash
reclaude transcripts sync
reclaude transcripts list
reclaude transcripts extract
reclaude focus hour --dry-run
reclaude focus hour
reclaude focus day
reclaude focus week
```

The `focus` commands send content to Gemini; see the data-flow details above.

### Utilities

```bash
reclaude ui --no-open
reclaude log
reclaude tag-session
reclaude get-session <tag>
```

Use `reclaude <command> --help` for filters and options.

## Event types

| Type | Category | Description |
|---|---|---|
| `user_prompt` | Conversation | User prompts |
| `assistant` | Conversation | Assistant responses |
| `plan` | Conversation | Planning text before tool use |
| `thinking` | Conversation | Thinking blocks |
| `tool_use` | Action | Tool invocations with input and output |
| `file_diff` | Action | Edit/Write activity as unified diffs |
| `compaction` | System | Session summaries |
| `session_start` | Lifecycle | Session startup or resume |
| `session_end` | Lifecycle | Session completion |
| `subagent_spawn` | Lifecycle | Subagent creation |
| `subagent_stop` | Lifecycle | Subagent completion |
| `plan_file` | System | Plans from `~/.claude/plans/` |

## Data locations

| Path | Contents |
|---|---|
| `~/.reclaude/metadata.db` | Events, sessions, archived transcripts, search indexes, and vectors |
| `~/.reclaude/log/reclaude.jsonl` | Structured log |
| `~/.reclaude/models/` | ONNX embedding model |

## Development

```bash
cargo build --release
cargo test
```

The [frontend guide](frontend/README.md) describes running the API and Vite server.
With frontend dependencies installed and `reclaude` on PATH, `just serve` starts
both. `just install` builds and copies the binary to `~/.local/bin`.

## Precursor to Gently

Reclaude is the conceptual precursor to [Gently](https://github.com/JamieAP/gently).
Development began on 18 January 2026 with Claude Code
hooks, local session and event storage, transcript extraction, CLI queries, and a
dashboard. Later work added full-text search and local ONNX embeddings.

Gently followed on 31 May 2026, extending the aim of making agent activity
inspectable into OpenTelemetry spans for sessions, turns, tools, and subagents,
a durable export pipeline and collector, and CLI/MCP queries. Codex support
followed in June. The projects have separate Git histories; Reclaude is the
earlier conceptual foundation.

## License

[MIT](LICENSE). Dependencies retain their own licenses and notices.
The license does not grant rights to third-party session data.
