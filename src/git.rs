use std::process::Command;

use crate::models::GitContext;

/// Normalize git remote URL to canonical `host/owner/repo` form.
///
/// Handles SSH (`git@host:owner/repo.git`), SSH protocol
/// (`ssh://git@host/owner/repo`), and HTTPS URLs.
pub fn normalize_remote_url(url: &str) -> String {
    if url.is_empty() {
        return url.to_string();
    }

    let url = url.strip_suffix(".git").unwrap_or(url);

    // SSH: git@host:owner/repo
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return format!("{host}/{path}");
        }
    }

    // SSH protocol: ssh://git@host/owner/repo
    if let Some(rest) = url.strip_prefix("ssh://git@") {
        if let Some((host, path)) = rest.split_once('/') {
            return format!("{host}/{path}");
        }
    }

    // HTTPS: https://host/owner/repo
    if url.starts_with("http://") || url.starts_with("https://") {
        let without_scheme = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .unwrap_or(url);
        return without_scheme.to_string();
    }

    url.to_string()
}

/// Extract git repository context for a working directory.
///
/// Returns repo_root, remote_url (normalized), branch, repo_name,
/// and whether we're inside a worktree.
pub fn get_git_context(cwd: &str) -> GitContext {
    let mut ctx = GitContext {
        cwd: cwd.to_string(),
        ..Default::default()
    };

    let git = |args: &[&str]| -> Option<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .ok()?;

        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        } else {
            None
        }
    };

    // Detect worktree vs main repo
    if let Some(git_common) = git(&["rev-parse", "--git-common-dir"]) {
        if git_common != ".git" {
            // We're in a worktree
            let common_path = std::path::Path::new(cwd).join(&git_common);
            if let Some(parent) = common_path.canonicalize().ok().and_then(|p| {
                p.parent().map(|p| p.to_string_lossy().to_string())
            }) {
                ctx.repo_root = Some(parent);
            }
            ctx.is_worktree = true;
        } else {
            ctx.repo_root = git(&["rev-parse", "--show-toplevel"]);
            ctx.is_worktree = false;
        }
    }

    // Remote URL (canonical cross-machine identifier)
    if let Some(raw_remote) = git(&["remote", "get-url", "origin"]) {
        ctx.remote_url = Some(normalize_remote_url(&raw_remote));
    }

    // Current branch
    ctx.branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]);

    // Repo name from remote URL or directory name
    if let Some(ref remote) = ctx.remote_url {
        let url = remote.strip_suffix(".git").unwrap_or(remote);
        ctx.repo_name = url.rsplit('/').next().map(String::from);
    } else if let Some(ref root) = ctx.repo_root {
        ctx.repo_name = std::path::Path::new(root)
            .file_name()
            .map(|n| n.to_string_lossy().to_string());
    }

    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_ssh() {
        assert_eq!(
            normalize_remote_url("git@github.com:owner/repo.git"),
            "github.com/owner/repo"
        );
    }

    #[test]
    fn test_normalize_https() {
        assert_eq!(
            normalize_remote_url("https://github.com/owner/repo.git"),
            "github.com/owner/repo"
        );
    }

    #[test]
    fn test_normalize_ssh_protocol() {
        assert_eq!(
            normalize_remote_url("ssh://git@github.com/owner/repo"),
            "github.com/owner/repo"
        );
    }

    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize_remote_url(""), "");
    }

    #[test]
    fn test_normalize_no_git_suffix() {
        assert_eq!(
            normalize_remote_url("git@github.com:owner/repo"),
            "github.com/owner/repo"
        );
    }
}
