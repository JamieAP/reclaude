---
name: reclaude:context
description: Use when confused about prior work, missing context, picking up a task, debugging something that was working before, wondering "what changed?", "what was decided?", "what happened last session?", or needing history about any file, feature, or decision. Also use proactively at session start to orient yourself.
version: 0.1.0
---

# Context Discovery

You have access to `reclaude` - a searchable database of captured Claude Code session events. Prompts, responses, tool calls, file diffs, plans, thinking, session lifecycle - all captured and indexed.

**Use it.** Don't guess what happened. Don't assume. Look it up.

## When to Use

| Situation | What to run |
|-----------|-------------|
| Starting a new session | `reclaude events --all -n 10` - what happened recently? |
| Picking up previous work | `reclaude search "feature name" --all` - find prior context |
| "What changed in this file?" | `reclaude search "filename" -t diff --all` |
| "What was decided about X?" | `reclaude search --semantic "decision about X" --all` |
| "Why does this code look like this?" | `reclaude search --semantic "why X was implemented" --all` |
| Debugging a regression | `reclaude files "path/to/file" --all` - who touched it, when? |
| "What did I do last session?" | `reclaude events -s <session_prefix> --full` |
| Lost after context compaction | `reclaude chat --fzf` - browse full conversation history |

## Commands

### Search (your primary tool)

```bash
# Full-text search - exact keyword matching with stemming
reclaude search "migration" --all -n 10

# Boolean operators
reclaude search "sqlite" --and "migration" --not "postgres" --all

# Semantic search - finds conceptually similar content
reclaude search --semantic "replacing the database layer" --all -n 10

# Scoped to current project (default) or specific session
reclaude search "bug fix" -n 10
reclaude search "auth" -s abc123 --all

# Filter by event type
reclaude search "schema" -t diff --all        # file changes only
reclaude search "should we" -t prompt --all    # user prompts only
reclaude search "decided" -t assistant --all   # Claude responses only

# Interactive results with fzf
reclaude search "config" --fzf --all
```

### Browse Events

```bash
# Recent events (current project)
reclaude events -n 20

# Recent events (all projects)
reclaude events --all -n 20

# Filter by type: prompt, diff, plan, tool, compaction
reclaude events -t diff -n 10
reclaude events -t plan --all

# Full content (no truncation)
reclaude events --full -n 5

# Single event by ID
reclaude events --id 42000
```

### Conversation Replay

```bash
# Browse sessions interactively
reclaude chat --fzf

# Replay a specific session's conversation
reclaude chat -s abc123

# Jump to context around a specific event
reclaude chat 42000
```

### File History

```bash
# Files touched by Claude in current project
reclaude files

# Search for a specific file
reclaude files "capture.rs" --all

# Combined: find diffs to a file
reclaude search "src/db/events.rs" -t diff --all -n 10
```

### Session Context

```bash
# Recent sessions
reclaude sessions -n 10

# What's the overall status?
reclaude status

# Files touched recently (with recency + touch count)
reclaude files
reclaude files --all
```

## Search Strategy

**Start broad, narrow down:**

1. **Semantic first** when you have a concept but not exact words:
   `reclaude search --semantic "why we chose sqlite over postgres" --all`

2. **FTS when you know keywords:**
   `reclaude search "WAL" --and "pragma" --all`

3. **Add filters to reduce noise:**
   `-t diff` for code changes, `-t prompt` for user intent, `-t plan` for reasoning

4. **Use `--all`** to search across projects - context often spans repos.

5. **Use `--fzf`** when you're exploring and don't know what you're looking for.

## Anti-Patterns

| Don't | Do instead |
|-------|-----------|
| Guess what happened last session | `reclaude events --all -n 15` |
| Assume a file's history | `reclaude files "filename" --all` |
| Ask the user "what were we working on?" | `reclaude search --semantic "recent work" --all -n 5` |
| Re-derive a decision that was already made | `reclaude search --semantic "decided to..." --all` |
| Wonder why code looks a certain way | `reclaude search "filename" -t diff --all` |
| Repeat work from a prior session | `reclaude search "feature name" --all -n 10` |

## Output Formats

- Default: colored terminal table (time, session, type, content preview)
- `--json`: machine-readable JSON lines (pipe to jq, python, etc.)
- `--full` / `--verbose`: no truncation
- `--fzf`: interactive browser with preview pane
