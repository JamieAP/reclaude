# reclaude task runner

# Default: show status
default: status

# Show capture statistics
status:
    reclaude status

# Show recent sessions
sessions n="10":
    reclaude sessions {{n}}

# Show recent events
events n="20" *args:
    reclaude events {{n}} {{args}}

# Search events (FTS or semantic)
search *args:
    reclaude search {{args}}

# Focus synthesis via Gemini
focus scale="hour":
    reclaude focus {{scale}}

# Transcript management
transcripts cmd="sync":
    reclaude transcripts {{cmd}}

# Tail log
log:
    tail -f ~/.reclaude/log/reclaude.jsonl

# Build release binary
build:
    cargo build --release

# Build and install to ~/.local/bin
install: build
    cp target/release/reclaude ~/.local/bin/reclaude

# Run tests
test *args:
    cargo test {{args}}

# Backfill events from Python capture.db
backfill *args:
    reclaude backfill {{args}}

# Launch web UI (serves built frontend + API)
ui:
    reclaude ui

# Serve API only
api host="127.0.0.1" port="8420":
    reclaude ui --host {{host}} --port {{port}} --no-open

# Local dev: API + Vite dev server (with HMR)
serve:
    #!/usr/bin/env bash
    set -euo pipefail

    # Start API server in background
    echo "Starting API server on http://127.0.0.1:8420..."
    reclaude ui --no-open &
    API_PID=$!

    # Give API a moment to start
    sleep 1

    # Start Vite dev server in foreground
    echo "Starting Vite dev server..."
    cd frontend && npm run dev &
    VITE_PID=$!

    # Trap to kill both on exit
    trap "kill $API_PID $VITE_PID 2>/dev/null" EXIT

    # Wait for either to exit
    wait
