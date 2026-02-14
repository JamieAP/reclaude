use std::io::Write;
use std::process::{Command, Stdio};

/// Dark color scheme for fzf.
const FZF_COLOR_SCHEME: &str = "bg+:#1a1a2e,fg+:#e0e0e0,hl:#56b6c2,hl+:#56b6c2,\
    pointer:#c678dd,marker:#98c379,border:#3b3b5c,\
    header:#888888,info:#555555,prompt:#c678dd,\
    gutter:#0e0e1a,preview-bg:#0e0e1a";

/// Run fzf with input text, a preview command, and a header.
///
/// Returns the selected line, or None if user cancelled.
pub fn run_fzf(
    input: &str,
    preview_cmd: &str,
    header: &str,
) -> anyhow::Result<Option<String>> {
    if input.is_empty() {
        return Ok(None);
    }

    let mut child = Command::new("fzf")
        .args([
            "--ansi",
            "--height=80%",
            "--reverse",
            "--preview",
            preview_cmd,
            "--preview-window=right,55%,wrap,border-left",
            &format!("--color={FZF_COLOR_SCHEME}"),
            "--border=rounded",
            "--margin=1,2",
            "--padding=1,0",
            &format!("--header={header}"),
            "--header-first",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes())?;
    }
    // Drop stdin to signal EOF
    drop(child.stdin.take());

    let output = child.wait_with_output()?;

    if output.status.success() {
        let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if selected.is_empty() {
            Ok(None)
        } else {
            Ok(Some(selected))
        }
    } else {
        Ok(None)
    }
}

/// Run fzf with tab-separated lines where first column is hidden ID.
pub fn select_with_id(
    lines: &[String],
    preview_cmd: &str,
    header: &str,
) -> anyhow::Result<Option<String>> {
    if lines.is_empty() {
        return Ok(None);
    }

    let input = lines.join("\n");

    let mut child = Command::new("fzf")
        .args([
            "--ansi",
            "--delimiter=\t",
            "--with-nth=2..",
            "--height=80%",
            "--reverse",
            "--preview",
            preview_cmd,
            "--preview-window=right,55%,wrap,border-left",
            &format!("--color={FZF_COLOR_SCHEME}"),
            "--border=rounded",
            "--margin=1,2",
            "--padding=1,0",
            &format!("--header={header}"),
            "--header-first",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes())?;
    }
    drop(child.stdin.take());

    let output = child.wait_with_output()?;

    if output.status.success() {
        let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if selected.is_empty() {
            Ok(None)
        } else {
            // Extract the hidden ID (first tab-separated field)
            let id = selected.split('\t').next().unwrap_or(&selected);
            Ok(Some(id.to_string()))
        }
    } else {
        Ok(None)
    }
}

/// Strip ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;

    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            result.push(c);
        }
    }

    result
}
