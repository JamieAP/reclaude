"""
Git context utilities for reclaude.

Provides repository identification for worktree reconciliation and smart queries.
"""

from __future__ import annotations

import os
import re
import subprocess


def normalize_remote_url(url: str) -> str:
    """Normalize git remote URL to canonical form.

    Converts SSH and HTTPS URLs to a common format: host/owner/repo
    Examples:
        git@github.com:owner/repo.git -> github.com/owner/repo
        https://github.com/owner/repo.git -> github.com/owner/repo
        ssh://git@github.com/owner/repo -> github.com/owner/repo
    """
    if not url:
        return url

    # Remove .git suffix
    if url.endswith(".git"):
        url = url[:-4]

    # SSH format: git@host:owner/repo
    ssh_match = re.match(r"^git@([^:]+):(.+)$", url)
    if ssh_match:
        return f"{ssh_match.group(1)}/{ssh_match.group(2)}"

    # SSH protocol: ssh://git@host/owner/repo
    ssh_proto_match = re.match(r"^ssh://git@([^/]+)/(.+)$", url)
    if ssh_proto_match:
        return f"{ssh_proto_match.group(1)}/{ssh_proto_match.group(2)}"

    # HTTPS: https://host/owner/repo
    https_match = re.match(r"^https?://([^/]+)/(.+)$", url)
    if https_match:
        return f"{https_match.group(1)}/{https_match.group(2)}"

    # Unknown format, return as-is
    return url


def get_git_context(cwd: str | None = None) -> dict[str, str | bool | None]:
    """Extract all git identifiers for a directory.

    Returns dict with: cwd, repo_root, is_worktree, remote_url, branch, repo_name.
    Greedy capture for flexible querying/merging later.
    """
    working_dir = cwd or os.getcwd()
    ctx: dict[str, str | bool | None] = {"cwd": working_dir}

    def git(*args: str) -> str | None:
        try:
            result = subprocess.run(
                ["git", "-C", working_dir] + list(args),
                capture_output=True, text=True, timeout=5
            )
            return result.stdout.strip() if result.returncode == 0 else None
        except Exception:
            return None

    # Main worktree / repo root
    git_common = git("rev-parse", "--git-common-dir")
    if git_common and git_common != ".git":
        # We're in a worktree, resolve main repo
        ctx["repo_root"] = os.path.dirname(os.path.abspath(os.path.join(working_dir, git_common)))
        ctx["is_worktree"] = True
    else:
        ctx["repo_root"] = git("rev-parse", "--show-toplevel")
        ctx["is_worktree"] = False

    # Remote URL (canonical cross-machine ID, normalized for SSH/HTTPS equivalence)
    raw_remote = git("remote", "get-url", "origin")
    ctx["remote_url"] = normalize_remote_url(raw_remote) if raw_remote else None

    # Current branch
    ctx["branch"] = git("rev-parse", "--abbrev-ref", "HEAD")

    # Repo name from remote or directory
    remote_url = ctx.get("remote_url")
    repo_root = ctx.get("repo_root")
    if remote_url and isinstance(remote_url, str):
        url = remote_url
        if url.endswith(".git"):
            url = url[:-4]
        ctx["repo_name"] = url.split("/")[-1].split(":")[-1]
    elif repo_root and isinstance(repo_root, str):
        ctx["repo_name"] = os.path.basename(repo_root)

    return ctx
