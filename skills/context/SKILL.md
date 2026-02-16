---
name: reclaude:context
description: Use when confused about prior work, missing context, picking up a task, debugging a regression, or at session start to orient. Also use proactively when user's CLAUDE.md says to check reclaude.
---

# Context Recovery with reclaude

`reclaude` is a searchable database of every Claude Code session - prompts, responses, diffs, plans, tool calls. **Don't guess. Look it up.**

## Session Start Drill

Run this when picking up work or starting a session in a project with prior history:

```bash
reclaude events -n 10              # what happened recently in this project?
reclaude sessions -n 5             # recent sessions here
```

If the user mentions prior work, search for it before acting.

## The Drill-Down Pattern

Context recovery is a funnel: **search → find event → replay conversation**.

**1. Search** - find relevant events:
```bash
reclaude search "migration" --all          # FTS: exact keywords
reclaude search --semantic "why we chose sqlite" --all  # vector: conceptual match
reclaude search "schema" -t diff --all     # scoped to diffs only
```

**2. Identify** - note the event ID from search results (leftmost column).

**3. Replay** - see the full conversation around that event:
```bash
reclaude chat <event_id>           # conversation context around the event
reclaude chat -s <session_prefix>  # full session replay
```

This is how you recover *decisions*, not just *facts*. The conversation shows reasoning.

## When to Search What

| Question | Command |
|----------|---------|
| What changed in a file? | `reclaude search "filename" -t diff --all` |
| What was decided? | `reclaude search --semantic "decision about X" --all` |
| Who touched this file? | `reclaude files "path/to/file" --all` |
| What did we do last session? | `reclaude events -s <prefix> --full` |
| Lost after compaction? | `reclaude chat --fzf` |

## Key Flags

All subcommands support `--help` for full flag reference. The critical ones:

- `--all` - search across all projects (default scopes to cwd)
- `-t <type>` - filter: `prompt`, `diff`, `plan`, `tool`, `compaction`
- `--semantic` - vector similarity search (conceptual, not keyword)
- `--fzf` - interactive browser with preview
- `--full` - no content truncation
- `-s <prefix>` - filter by session ID prefix

## Anti-Patterns

- **Don't guess** what happened last session - `reclaude events -n 10`
- **Don't ask the user** "what were we working on?" - search first
- **Don't re-derive** a decision already made - search for it
- **Don't repeat** `--help` output as context - just run the command
