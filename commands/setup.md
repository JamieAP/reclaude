---
description: Install the reclaude CLI binary for terminal use
---

# Setup reclaude CLI

The user wants to install the `reclaude` CLI so they can run commands like `reclaude status`, `reclaude events`, etc. from their terminal.

## Install the CLI

Build and install the release binary:

```bash
cd /path/to/reclaude
cargo build --release
cp target/release/reclaude ~/.local/bin/reclaude
```

Ensure `~/.local/bin` is on your PATH.

## Verify Installation

After installing, verify it works:

```bash
reclaude --help
reclaude status
```

## Optional: Download Embedding Model

For semantic (vector) search:

```bash
reclaude embed download
reclaude embed backfill --limit 5000
```
