//! Sub-agent pool — manages Claude Code fix invocations.
//!
//! Each agent gets its own Git worktree for isolated fixes.

use std::process::Command;
use std::time::Instant;

/// Result of invoking Claude Code for a bug fix.
#[derive(Debug, Clone)]
pub struct ClaudeResult {
    pub success: bool,
    pub bug_id: String,
    pub elapsed_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub changes: u32,
}

/// Synchronous version — safe for `tokio::task::block_in_place`.
pub fn run_claude_fix_sync(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    claude_fix_script: &str,
    timeout_secs: u64,
) -> ClaudeResult {
    run_claude_fix_impl(agent_name, bug_id, bug_title, claude_fix_script, timeout_secs)
}

/// Async wrapper (uses sync internally, safe for spawn_blocking or block_in_place).
pub async fn run_claude_fix(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    claude_fix_script: &str,
    timeout_secs: u64,
) -> ClaudeResult {
    run_claude_fix_impl(agent_name, bug_id, bug_title, claude_fix_script, timeout_secs)
}

/// Invoke Claude Code to fix a bug using the agent's isolated worktree.
fn run_claude_fix_impl(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    claude_fix_script: &str,
    _timeout_secs: u64,
) -> ClaudeResult {
    let start = Instant::now();

    // Build shell command
    let output = Command::new("bash")
        .arg(claude_fix_script)
        .arg(bug_id)
        .arg(bug_title)
        .arg(agent_name)
        .output();

    let elapsed = start.elapsed().as_millis() as u64;

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let exit_code = o.status.code().unwrap_or(-1);
            // Success: exit 0 AND (fix commit found OR already fixed)
            // NOTE: FILES_MODIFIED: 0 means Claude Code found nothing to fix — NOT success
            let success = exit_code == 0 && (
                stdout.contains(&format!("Fix Bug #{}", bug_id))
                || stdout.contains("已在之前修复")
                || stdout.contains("已包含完整修复")
            );

            // Count diff changes from worktree
            let changes = count_worktree_changes(agent_name);

            ClaudeResult {
                success,
                bug_id: bug_id.to_string(),
                elapsed_ms: elapsed,
                stdout,
                stderr,
                exit_code,
                changes,
            }
        }
        Err(e) => ClaudeResult {
            success: false,
            bug_id: bug_id.to_string(),
            elapsed_ms: elapsed,
            stdout: String::new(),
            stderr: format!("{:?}", e),
            exit_code: -1,
            changes: 0,
        },
    }
}

/// Count changed lines in the agent's worktree since last commit.
fn count_worktree_changes(agent_name: &str) -> u32 {
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    let output = Command::new("git")
        .args(["diff", "HEAD~1", "--stat"])
        .current_dir(&worktree)
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let mut total = 0u32;
            for line in stdout.lines() {
                if let Some(pipe_pos) = line.find('|') {
                    let nums = &line[pipe_pos+1..];
                    total += nums.chars().filter(|&c| c == '+' || c == '-').count() as u32;
                }
            }
            total
        }
        Err(_) => 0,
    }
}

/// Verify that a fix diff is meaningful (≥3 changed lines).
pub fn is_meaningful_fix(changes: u32) -> bool {
    changes >= 3
}

/// Compute elapsed duration as HH:MM:SS string.
pub fn fmt_duration(seconds: f64) -> String {
    let s = seconds as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let ss = s % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, ss)
    } else {
        format!("{:02}:{:02}", m, ss)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_meaningful_fix() {
        assert!(!is_meaningful_fix(0));
        assert!(!is_meaningful_fix(2));
        assert!(is_meaningful_fix(3));
        assert!(is_meaningful_fix(100));
    }

    #[test]
    fn test_fmt_duration() {
        assert_eq!(fmt_duration(0.0), "00:00");
        assert_eq!(fmt_duration(45.0), "00:45");
        assert_eq!(fmt_duration(125.0), "02:05");
        assert_eq!(fmt_duration(3661.0), "01:01:01");
    }
}
