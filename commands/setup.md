---
description: Install the reclaude CLI binary for terminal use
---

# Setup reclaude CLI

The user wants to install the `reclaude` CLI so they can run commands like `reclaude status`, `reclaude events`, etc. from their terminal.

## Install the CLI

Run this command to install reclaude as a uv tool:

```bash
uv tool install git+https://github.com/JamieAP/reclaude
```

This makes the `reclaude` command available system-wide.

## Verify Installation

After installing, verify it works:

```bash
reclaude --help
reclaude status
```

## Alternative: Install from plugin cache

If you prefer to use the exact version from your installed plugin:

```bash
uv pip install -e ~/.claude/plugins/cache/reclaude/*/
```

Note: The cache path may vary. Use `claude plugin list` to find the exact location.
