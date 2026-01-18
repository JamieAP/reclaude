#!/usr/bin/env bash
# reclaude capture hook - auto-installs deps via uv on first run
set -euo pipefail

HOOK_TYPE="${1:-}"
if [[ -z "$HOOK_TYPE" ]]; then
    echo "Usage: capture-hook.sh <hook_type>" >&2
    exit 1
fi

# Find plugin root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_ROOT="$(dirname "$SCRIPT_DIR")"

# Prefer uv (auto-installs deps), fall back to direct venv python
if command -v uv &>/dev/null; then
    exec uv run --project "$PLUGIN_ROOT" python -m reclaude.capture "$HOOK_TYPE"
elif [[ -x "$PLUGIN_ROOT/.venv/bin/python" ]]; then
    exec "$PLUGIN_ROOT/.venv/bin/python" -m reclaude.capture "$HOOK_TYPE"
else
    # Silent exit - don't break Claude if not set up
    exit 0
fi
