#!/usr/bin/env bash
# reclaude capture hook - forwards hook events to the Rust binary
set -euo pipefail

HOOK_TYPE="${1:-}"
if [[ -z "$HOOK_TYPE" ]]; then
    echo "Usage: capture-hook.sh <hook_type>" >&2
    exit 1
fi

# Look for reclaude binary in common locations
if command -v reclaude &>/dev/null; then
    exec reclaude capture "$HOOK_TYPE"
elif [[ -x "$HOME/.local/bin/reclaude" ]]; then
    exec "$HOME/.local/bin/reclaude" capture "$HOOK_TYPE"
elif [[ -x "$HOME/.cargo/bin/reclaude" ]]; then
    exec "$HOME/.cargo/bin/reclaude" capture "$HOOK_TYPE"
else
    # Binary not found - silently exit so Claude isn't blocked
    exit 0
fi
