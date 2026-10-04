---
name: reclaude:context
description: Use when confused about prior work, missing context, picking up a task, debugging a regression, or at session start to orient. Also use proactively when user's CLAUDE.md says to check reclaude.
---

# Context Recovery with reclaude

`reclaude` is a searchable database of every Claude Code session - prompts, responses, diffs, plans, tool calls. **Don't guess. Look it up.**

## Session Start Drill

Run this when picking up work or starting a session:

```bash
reclaude recap                     # 24h activity: dirs, sessions, subagent trees, prompts
reclaude events -n 10              # what happened recently in this project?
```

`recap` defaults to a 24h zoom across all repos - directories touched (with file counts and edit counts), session trees showing subagent spawns, and recent prompts. Use `recap 2h` or `recap 7d` to change window. Use `recap --overview` for the compact multi-bucket view.

If the user mentions prior work, search for it before acting.

## The Drill-Down Pattern

Context recovery is a funnel: **search → find event → replay conversation**.

**1. Search** - find relevant events (scoped to cwd by default, add `--all` to widen):
```bash
reclaude search "migration"                # FTS: current project
reclaude search --semantic "why we chose sqlite"  # vector: conceptual match
reclaude search "schema" -t diff           # scoped to diffs only
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
| What changed in a file? | `reclaude search "filename" -t diff` |
| What was decided? | `reclaude search --semantic "decision about X"` |
| Who touched this file? | `reclaude files "path/to/file"` |
| Markdown files by recency? | `reclaude files -e md --all` |
| What did we do last session? | `reclaude events -s <prefix> --full` |
| Cross-project history? | Add `--all` to any of the above |
| Lost after compaction? | `reclaude chat --fzf` |

## Key Flags

All subcommands support `--help` for full flag reference. The critical ones:

- `--all` - search across all projects (default scopes to cwd)
- `-t <type>` - filter: `prompt`, `diff`, `plan`, `tool`, `compaction`
- `--semantic` - vector similarity search (conceptual, not keyword)
- `--fzf` - interactive browser with preview
- `-e <ext>` - filter files by extension (e.g. `-e md`, `-e rs`)
- `--full` - no content truncation
- `-s <prefix>` - filter by session ID prefix

## Scoping: Time > Location

Recent events are often more relevant. Broad `--all` FTS searches can surface textually similar but unrelated results from other projects and months.

**Scope narrowly first, widen only if needed:**
1. Default (no `--all`) - current cwd only
2. Add `-s <session_prefix>` - specific session
3. Add `--all` - only when you need cross-project history

Use `recap` to know which time periods and repos are relevant *before* searching.

## Anti-Patterns

- **Don't guess** what happened last session - `reclaude recap` then search
- **Don't `--all` by default** - start scoped, widen if needed
- **Don't ask the user** "what were we working on?" - search first
- **Don't re-derive** a decision already made - search for it
