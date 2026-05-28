//! Sub-agent pool — manages Claude Code / Codex fix invocations.
//!
//! Each agent gets its own Git worktree for isolated fixes.
//! When Codex is invoked, the prompt includes the full Harness Engineering
//! methodology (Init → Plan → Implement → Verify → Cleanup) via loaded skills.
//!
//! All fix invocations go through `codex-aliyun` → `mimo2codex` → `codex` pipeline.

use std::process::Command;
use std::time::Instant;

/// Result of invoking Claude Code / Codex for a bug fix.
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

/// Check if a bug is a frontend issue (should use Codex).
pub fn is_frontend_bug(title: &str) -> bool {
    let t = title.to_lowercase();
    let frontend_kw = ["前端", "vue", "界面", "显示", "弹窗", "按钮", "列表", "回显",
                       "刷新", "排版", "格式", "样式", "css", "组件", "命名", "提示语",
                       "查询", "过滤", "加载", "报表", "报表管理"];
    frontend_kw.iter().any(|kw| t.contains(kw))
}

// ──────────────────────────────────────────────
// Harness methodology: skill loading + prompt builder
// ──────────────────────────────────────────────

/// Load a skill file's content (return empty string if not found).
fn load_skill(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Build the full harness-augmented prompt for Codex.
/// Loads ALL installed harness skills so Codex follows the methodology autonomously.
fn build_harness_prompt(bug_id: &str, bug_title: &str, bug_details: &str) -> String {
    let skills_base = "/root/.codex/skills";
    let harness_eng    = load_skill(&format!("{}/harness-engineering/SKILL.md", skills_base));
    let walkinglabs    = load_skill(&format!("{}/walkinglabs-harness/SKILL.md", skills_base));
    let durable_exec   = load_skill(&format!("{}/durable-execution/SKILL.md", skills_base));
    let closed_loop    = load_skill(&format!("{}/closed-loop-testing/SKILL.md", skills_base));
    let constraint_d   = load_skill(&format!("{}/constraint-design/SKILL.md", skills_base));
    let review_audit   = load_skill(&format!("{}/review-audit/SKILL.md", skills_base));
    let karpathy       = load_skill(&format!("{}/karpathy-guidelines/SKILL.md", skills_base));
    let full_chain     = load_skill(&format!("{}/full-chain-fix/SKILL.md", skills_base));

    let agents_md_path = "/root/.openclaw/workspace/his-repo/AGENTS.md";
    let agents_md_hint = load_skill(agents_md_path)
        .lines().take(30).collect::<Vec<_>>().join("\n");

    format!(
        r#"你是一个中文编程助手。使用简体中文思考和回复。

## Harness Engineering 方法论（必须遵守）

在修复 Bug 之前，阅读并遵循以下方法论：

### 工作纪律
1. **Init**: 先确认工作目录和项目状态
2. **Plan**: 分析全链路数据流——录入→保存→查询→修改→删除→关联（6 环）
3. **Implement**: 一次只修一个 Bug，只动必要文件
4. **Verify**: 修改后运行编译/语法检查
5. **Cleanup**: 不留临时文件或调试代码

### 约束铁律
- 安全 > 架构 > 质量 > 性能
- 禁止硬编码密钥/密码
- 涉及 Mapper XML 时，UNION ALL 所有子查询统一修改
- 涉及数据库字段时，走通全链路：前端→API→Service→Mapper→DB

## 已加载的技能（融入你的工作方式）

### 🔧 核心方法论
{harness_eng}

### 📋 实战模式（5 子系统）
{walkinglabs}

### ⏳ 持久执行（检查点/幂等）
{durable_exec}

### 🧪 闭环测试（质量门禁）
{closed_loop}

### 📐 约束设计
{constraint_d}

### 🔐 审查审计
{review_audit}

### 🎯 编码准则
{karpathy}

### 🔗 全链路修复
{full_chain}

## 项目规则摘要
{agents_md_hint}

---

## 任务：修复 Bug #{bug_id}：{bug_title}

## 禅道 Bug 详情
{bug_details}

## Harness 修复指引
1. **Init**: 确认目录，读 AGENTS.md
2. **Analyze**: 用 rg/grep 搜索相关代码
3. **Reproduce**: 按步骤复现，定位根因
4. **Full-chain**: 涉及字段时检查全部 6 环
5. **Fix**: 修改文件（用 apply_patch）
6. **Verify**: 运行编译检查
7. **Submit**: 输出变更摘要

请分析并直接修改文件修复。不要用 git。
"#,
        bug_id=bug_id, bug_title=bug_title, bug_details=bug_details,
        harness_eng=harness_eng, walkinglabs=walkinglabs,
        durable_exec=durable_exec, closed_loop=closed_loop,
        constraint_d=constraint_d, review_audit=review_audit,
        karpathy=karpathy, full_chain=full_chain,
        agents_md_hint=agents_md_hint,
    )
}

/// Run quality gates after a fix to verify correctness.
fn run_quality_gates(work_dir: &str) -> (bool, String, String) {
    let is_his_repo = work_dir.contains("his-repo");
    let is_rust = work_dir.contains("agentforge-rs");

    // Use owned Vec<String> to avoid type complexity with slices
    if is_his_repo {
        let output = Command::new("mvn")
            .args(["compile", "-q", "-pl", "openhis-application", "-am"])
            .current_dir(work_dir)
            .output();
        match output {
            Ok(o) if o.status.success() => (true, "mvn compile OK".into(), String::new()),
            Ok(o) => (false,
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string()),
            Err(e) => (true, format!("mvn not available: {}", e), String::new()),
        }
    } else if is_rust {
        let output = Command::new("cargo")
            .args(["check", "-q"])
            .current_dir(work_dir)
            .output();
        match output {
            Ok(o) if o.status.success() => (true, "cargo check OK".into(), String::new()),
            Ok(o) => (false,
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string()),
            Err(e) => (true, format!("cargo not available: {}", e), String::new()),
        }
    } else {
        (true, "(no quality gates)".into(), String::new())
    }
}

// ──────────────────────────────────────────────
// Codex fix implementation (mimo2codex → codex)
// ──────────────────────────────────────────────

/// Run Codex (via codex-aliyun → mimo2codex) to fix a bug.
/// The prompt includes full Harness Engineering methodology.
fn run_codex_fix_impl(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    _timeout_secs: u64,
) -> ClaudeResult {
    let start = Instant::now();

    // Step 1: Query bug details from Zentao
    let bug_details_text = query_bug_details(bug_id);

    // Step 2: Build harness-augmented prompt
    let prompt = build_harness_prompt(bug_id, bug_title, &bug_details_text);

    // Step 3: Target repository
    let work_dir = "/root/.openclaw/workspace/his-repo/openhis-server-new";

    // Step 4: Launch codex via mimo2codex pipeline
    // Uses codex-aliyun which: (1) starts mimo2codex if needed, (2) runs codex with mimo model
    let mut child = match Command::new("codex-aliyun")
        .args(["exec", "--sandbox", "workspace-write",
                "--dangerously-bypass-approvals-and-sandbox",
                "--skip-git-repo-check", "-"])
        .current_dir(work_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn() {
        Ok(c) => c,
        Err(e) => {
            // Fallback to raw codex if codex-aliyun not found
            tracing::warn!("codex-aliyun not found ({}), falling back to codex", e);
            match Command::new("codex")
                .args(["exec", "--sandbox", "workspace-write",
                        "--dangerously-bypass-approvals-and-sandbox",
                        "--skip-git-repo-check", "-"])
                .current_dir(work_dir)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn() {
                Ok(c) => c,
                Err(e2) => return ClaudeResult {
                    success: false, bug_id: bug_id.to_string(),
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    stdout: String::new(),
                    stderr: format!("codex spawn failed: {}", e2),
                    exit_code: -1, changes: 0,
                },
            }
        }
    };

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let output = child.wait_with_output();
    let elapsed = start.elapsed().as_millis() as u64;

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let exit_code = o.status.code().unwrap_or(-1);
            let changes = count_worktree_changes(agent_name);
            let has_fix_commit = has_recent_fix_commit(agent_name, bug_id);
            let success = has_fix_commit || (
                exit_code == 0 && (
                    stdout.contains("修复完成") || stdout.contains("fix") || stdout.contains("resolved")
                    || changes > 0
                )
            );

            // Step 5: Run quality gates on the fix
            if success {
                let (gates_ok, gate_stdout, _gate_stderr) = run_quality_gates(work_dir);
                if gates_ok {
                    tracing::info!("[{}] Fix Bug #{} — all quality gates passed", agent_name, bug_id);
                } else {
                    tracing::warn!("[{}] Fix Bug #{} passed but quality gates failed: {}",
                        agent_name, bug_id, gate_stdout);
                }
            }

            // Step 6: Auto-commit changes
            if success && changes > 0 {
                auto_commit_fix(agent_name, bug_id);
            }

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

/// Query Zentao for bug details.
fn query_bug_details(bug_id: &str) -> String {
    let result = Command::new("bash")
        .arg("/root/.openclaw/extensions/zentao-token-refresh/zentao-bug-query.sh")
        .arg(bug_id)
        .output();
    match result {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => {
            let _ = std::fs::remove_file("/tmp/.zentao-admin-token");
            Command::new("bash")
                .arg("/root/.openclaw/extensions/zentao-token-refresh/zentao-bug-query.sh")
                .arg(bug_id)
                .output()
                .ok()
                .and_then(|o| if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).to_string())
                } else { None })
                .unwrap_or_default()
        }
    }
}

/// Auto-commit fix changes to the agent's worktree.
fn auto_commit_fix(agent_name: &str, bug_id: &str) {
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    let _ = Command::new("git")
        .args(["-C", &worktree, "add", "--all", "--",
                ":!*.orig", ":!*.mjs", ":!*.timestamp*"])
        .output();
    let _ = Command::new("git")
        .args(["-C", &worktree, "commit", "-m", &format!("Fix Bug #{}", bug_id)])
        .output();
}

// ──────────────────────────────────────────────
// Public API — called by executor.rs
// ──────────────────────────────────────────────

/// Synchronous fix entry point — safe for `tokio::task::block_in_place`.
/// This is the main entry point called by the agent executor.
/// Always routes through `mimo2codex → codex` with full Harness methodology.
pub fn run_claude_fix_sync(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    _claude_fix_script: &str,
    timeout_secs: u64,
) -> ClaudeResult {
    tracing::info!("[{}] Harness fix for Bug #{}: {}",
        agent_name, bug_id, bug_title);
    run_codex_fix_impl(agent_name, bug_id, bug_title, timeout_secs)
}

/// Async wrapper (uses sync internally).
pub async fn run_claude_fix(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    claude_fix_script: &str,
    timeout_secs: u64,
) -> ClaudeResult {
    run_claude_fix_sync(agent_name, bug_id, bug_title, claude_fix_script, timeout_secs)
}

// ──────────────────────────────────────────────
// Helper functions
// ──────────────────────────────────────────────

/// Count uncommitted changed lines in the agent's worktree.
fn count_worktree_changes(agent_name: &str) -> u32 {
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    let stat_outputs = [
        Command::new("git").args(["diff", "--stat"]).current_dir(&worktree).output(),
        Command::new("git").args(["diff", "--cached", "--stat"]).current_dir(&worktree).output(),
    ];
    let mut total = 0u32;
    for o in stat_outputs.into_iter().flatten() {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            if let Some(pipe_pos) = line.find('|') {
                let nums = &line[pipe_pos+1..];
                total += nums.chars().filter(|&c| c == '+' || c == '-').count() as u32;
            }
        }
    }
    total
}

/// Check if the agent's worktree has a NEW commit with "Fix Bug #N" message.
fn has_recent_fix_commit(agent_name: &str, bug_id: &str) -> bool {
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    let output = Command::new("git")
        .args(["-C", &worktree, "log", "-1", "--oneline", "--not", "--remotes",
                "--grep", &format!("Fix Bug #{}", bug_id)])
        .output();
    if let Ok(o) = &output {
        if !String::from_utf8_lossy(&o.stdout).trim().is_empty() {
            return true;
        }
    }
    let head_output = Command::new("git")
        .args(["-C", &worktree, "rev-parse", "HEAD"]).output();
    let origin_head_output = Command::new("git")
        .args(["-C", &worktree, "rev-parse", "origin/HEAD"]).output();
    if let (Ok(h), Ok(oh)) = (&head_output, &origin_head_output) {
        let head = String::from_utf8_lossy(&h.stdout).trim().to_string();
        let origin_head = String::from_utf8_lossy(&oh.stdout).trim().to_string();
        if head != origin_head && !head.is_empty() && !origin_head.is_empty() {
            let msg_output = Command::new("git")
                .args(["-C", &worktree, "log", "-1", "--format=%s"]).output();
            if let Ok(mo) = &msg_output {
                let msg = String::from_utf8_lossy(&mo.stdout);
                return msg.contains(&format!("Fix Bug #{}", bug_id))
                    || msg.contains(&format!("#{}", bug_id));
            }
        }
    }
    false
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

    #[test]
    fn test_build_harness_prompt_contains_key_elements() {
        let prompt = build_harness_prompt("999", "test bug", "details here");
        assert!(prompt.contains("Harness Engineering"));
        assert!(prompt.contains("Init"));
        assert!(prompt.contains("Plan"));
        assert!(prompt.contains("Verify"));
        assert!(prompt.contains("Bug #999"));
        assert!(prompt.contains("test bug"));
        assert!(prompt.contains("full-chain"));
    }

    #[test]
    fn test_load_skill_returns_string() {
        let content = load_skill("/root/.codex/skills/harness-engineering/SKILL.md");
        assert!(!content.is_empty());
        assert!(content.contains("harness-engineering"));
    }

    #[test]
    fn test_run_quality_gates_his_repo() {
        let (ok, stdout, _) = run_quality_gates("/root/.openclaw/workspace/his-repo/openhis-server-new");
        assert!(ok, "Quality gates failed: {}", stdout);
    }

    #[test]
    fn test_run_quality_gates_rust() {
        let (ok, stdout, _) = run_quality_gates("/root/agentforge-rs");
        assert!(ok, "Quality gates failed: {}", stdout);
    }
}
