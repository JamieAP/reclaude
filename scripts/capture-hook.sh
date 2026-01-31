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

# Fast path: use venv directly if it exists (avoids uv lockfile resolution)
if [[ -x "$PLUGIN_ROOT/.venv/bin/python" ]]; then
    exec "$PLUGIN_ROOT/.venv/bin/python" -m reclaude.capture "$HOOK_TYPE"
elif command -v uv &>/dev/null; then
    # Cold start: uv will create venv + install deps
    exec uv run --project "$PLUGIN_ROOT" python -m reclaude.capture "$HOOK_TYPE"
else
    exit 0
fi
