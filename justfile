# reclaude task runner

# Default: show status
default: status

# Show capture statistics
status:
    uv run reclaude status

# Show recent sessions
sessions n="10":
    uv run reclaude sessions {{n}}

# Show recent events (supports --type, --session, --semantic)
events n="20" *args:
    uv run reclaude events {{n}} {{args}}

# Query files touched by Claude
query *args:
    uv run reclaude query {{args}}

# Show LLM context bundle
context *args:
    uv run reclaude context {{args}}

# Store a learning
learn content:
    uv run reclaude learn "{{content}}"

# Query learnings
learnings n="30" *args:
    uv run reclaude learnings {{n}} {{args}}

# Focus synthesis via Gemini
focus scale="hour":
    uv run reclaude focus {{scale}}

# Transcript management
transcripts cmd="sync":
    uv run reclaude transcripts {{cmd}}

# Tail capture log
log:
    tail -f ~/.reclaude/capture.log

# Run tests
test *args:
    uv run --extra dev pytest {{args}}

# Run tests with coverage
test-cov:
    uv run --extra dev pytest --cov=reclaude --cov-report=term-missing

# Install package in dev mode
install:
    uv pip install -e ".[dev]"

# Show paths
path:
    uv run reclaude path

# Launch web UI (serves built frontend + API)
ui:
    uv run reclaude ui

# Serve API only (for production or testing built frontend)
api host="127.0.0.1" port="8420":
    uv run reclaude ui --host {{host}} --port {{port}} --no-open

# Local dev: API + Vite dev server (with HMR)
serve:
    #!/usr/bin/env bash
    set -euo pipefail

    # Start API server in background
    echo "Starting API server on http://127.0.0.1:8420..."
    uv run reclaude ui --no-open --reload &
    API_PID=$!

    # Give API a moment to start
    sleep 1

    # Start Vite dev server in foreground
    echo "Starting Vite dev server..."
    cd reclaude/frontend && npm run dev &
    VITE_PID=$!

    # Trap to kill both on exit
    trap "kill $API_PID $VITE_PID 2>/dev/null" EXIT

    # Wait for either to exit
    wait

# Full plugin reinstall (requires Claude restart)
build:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Removing cached plugin..."
    rm -rf ~/.claude/plugins/cache/reclaude
    echo "Installing from local marketplace..."
    TMPDIR=~/.claude/plugins/cache claude plugin install reclaude@reclaude
    echo "Done. Restart Claude to load."

# Fast sync to plugin cache (no restart needed)
sync:
    #!/usr/bin/env bash
    set -euo pipefail
    CACHE=~/.claude/plugins/cache/reclaude/reclaude/1.0.0
    cp -r commands/ skills/ hooks/ "$CACHE/"
    cp .claude-plugin/*.json "$CACHE/.claude-plugin/"
    echo "Synced."
