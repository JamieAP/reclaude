#!/usr/bin/env bash
# reclaude capture hook - forwards hook events to the Rust binary
set -euo pipefail

HOOK_TYPE="${1:-}"
if [[ -z "$HOOK_TYPE" ]]; then
    echo "Usage: capture-hook.sh <hook_type>" >&2
    exit 1
fi

# Resolve reclaude binary location
RECLAUDE=""
if command -v reclaude &>/dev/null; then
    RECLAUDE="reclaude"
elif [[ -x "$HOME/.local/bin/reclaude" ]]; then
    RECLAUDE="$HOME/.local/bin/reclaude"
elif [[ -x "$HOME/.cargo/bin/reclaude" ]]; then
    RECLAUDE="$HOME/.cargo/bin/reclaude"
else
    # Binary not found - silently exit so Claude isn't blocked
    exit 0
fi

# On SessionStart, archive transcripts in background before Claude
# rotates them (~7 day retention). Runs once per session start/resume/compact.
if [[ "$HOOK_TYPE" == SessionStart:* ]]; then
    "$RECLAUDE" transcripts sync &>/dev/null &
fi

exec "$RECLAUDE" capture "$HOOK_TYPE"
