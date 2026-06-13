//! Agent Exec — 调用 mimo-code CLI 的执行层
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::{Mutex, Condvar, atomic::{AtomicU32, AtomicBool, Ordering}};
use std::time::Instant;

const MAX_CONCURRENT: u32 = 2;
static SEM_COUNTER: AtomicU32 = AtomicU32::new(0);
static SEM_MTX: Mutex<()> = Mutex::new(());
static SEM_CV: Condvar = Condvar::new();

fn sem_acquire() {
    loop {
        let current = SEM_COUNTER.load(Ordering::SeqCst);
        if current < MAX_CONCURRENT {
            SEM_COUNTER.fetch_add(1, Ordering::SeqCst);
            return;
        }
        let guard = SEM_MTX.lock().unwrap();
        let _guard = SEM_CV.wait(guard).unwrap();
    }
}
fn sem_release() {
    SEM_COUNTER.fetch_sub(1, Ordering::SeqCst);
    if let Ok(guard) = SEM_MTX.lock() { SEM_CV.notify_one(); }
}
struct SemGuard;
impl Drop for SemGuard { fn drop(&mut self) { sem_release(); } }

#[derive(Debug, Clone)]
pub struct CodexExecResult {
    pub success: bool,
    pub final_message: String,
    pub verdict: Verdict,
    pub total_tokens: u64,
    pub elapsed_ms: u64,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict { Pass, Fail(String), Unknown }
impl Verdict {
    pub fn is_pass(&self) -> bool { matches!(self, Verdict::Pass) }
    pub fn is_fail(&self) -> bool { matches!(self, Verdict::Fail(_)) }
}

pub fn parse_verdict(output: &str) -> Verdict {
    if output.trim().len() < 20 { return Verdict::Fail("empty output".into()); }
    for line in output.lines() {
        let line = line.trim();
        if line.contains("VERDICT:") || line.contains("VERDICT：") {
            if line.contains("PASS") || line.contains("通过") { return Verdict::Pass; }
            if line.contains("FAIL") || line.contains("失败") {
                let reason = line.split(&['：', ':'][..]).nth(1).map(|s| s.trim().to_string()).unwrap_or_default();
                return Verdict::Fail(reason);
            }
        }
    }
    Verdict::Unknown
}

pub fn codex_exec(task: &str, _sandbox: &str, _schema_path: Option<&str>, agent_name: Option<&str>, timeout_secs: u64) -> CodexExecResult {
    let start = Instant::now();
    sem_acquire();
    let _sem_guard = SemGuard;
    let mimo_bin = std::env::var("MIMO_CODE_PATH").unwrap_or_else(|_| "mimo-code".into());
    let agent_ctx = agent_name.map(|a| get_agent_role_context(a)).unwrap_or_default();
    let full_task = if agent_ctx.is_empty() { task.to_string() } else { format!("{}\n\n{}", agent_ctx, task) };
    let work_dir = agent_name.map(|a| { let d = format!("/tmp/agentforge-worktrees/{}", a); std::fs::create_dir_all(&d).ok(); d }).map(std::path::PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    tracing::info!("[agent_exec] spawning: {} in {:?}", mimo_bin, work_dir);
    let mut cmd = Command::new(&mimo_bin);
    cmd.arg("run").arg("-y").arg("--no-tui").arg("--max-iterations").arg("20").arg(&full_task);
    cmd.current_dir(&work_dir); cmd.stdin(Stdio::null()); cmd.stdout(Stdio::piped()); cmd.stderr(Stdio::piped());
    let child = match cmd.spawn() { Ok(c) => c, Err(e) => { return CodexExecResult { success: false, final_message: format!("spawn failed: {}", e), verdict: Verdict::Fail(format!("spawn: {}", e)), total_tokens: 0, elapsed_ms: start.elapsed().as_millis() as u64, stderr: e.to_string() }; } };
    let output = match child.wait_with_output() { Ok(o) => o, Err(e) => { return CodexExecResult { success: false, final_message: format!("wait failed: {}", e), verdict: Verdict::Fail(format!("wait: {}", e)), total_tokens: 0, elapsed_ms: start.elapsed().as_millis() as u64, stderr: e.to_string() }; } };
    let elapsed_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_success = output.status.success();
    let final_message = extract_mimo_response(&stdout);
    let total_tokens = parse_mimo_tokens(&stdout);
    let verdict = parse_verdict(if final_message.is_empty() { &stdout } else { &final_message });
    let success = exit_success && verdict.is_pass();
    tracing::info!("[agent_exec] completed: exit={} elapsed={}ms tokens={}", output.status, elapsed_ms, total_tokens);
    CodexExecResult { success, final_message: if final_message.is_empty() { stdout.clone() } else { final_message }, verdict, total_tokens, elapsed_ms, stderr }
}

fn extract_mimo_response(stdout: &str) -> String {
    for line in stdout.lines().rev() { if let Some(resp) = line.trim().strip_prefix("MiMo: ") { return resp.to_string(); } }
    let mut r = Vec::new();
    for line in stdout.lines() { let t = line.trim(); if t.starts_with("Token usage:") || t.starts_with("MiMo Code CLI") { break; } if !t.is_empty() && !t.starts_with("model=") && !t.starts_with("workspace=") && !t.starts_with("- Thinking") && !t.starts_with("·") { r.push(t); } }
    r.join("\n")
}

fn parse_mimo_tokens(stdout: &str) -> u64 {
    for line in stdout.lines() { if let Some(u) = line.strip_prefix("Token usage:") { let mut t = 0u64; for p in u.split(|c: char| c.is_alphabetic() || c == ',' || c == '·') { if let Ok(n) = p.trim().parse::<u64>() { t += n; } } return t; } } 0
}

fn get_agent_role_context(n: &str) -> String {
    match n { "guanyu" => "你是关羽，后端修复工程师。Java/Spring/MyBatis。修复后 mvn compile 验证。".into(), "zhaoyun" => "你是赵云，前端修复工程师。Vue3/ElementUI/TypeScript。修复后 vite build 验证。".into(), "xunyu" => "你是荀彧，数据库工程师。PostgreSQL/DDL/DML。修复后检查迁移脚本。".into(), "zhangfei" => "你是张飞，QA测试工程师。Playwright回归测试。".into(), "huatuo" => "你是华佗，产品验收员。验证修复是否满足需求。".into(), "chenlin" => "你是陈琳，文档工程师。生成修复文档。".into(), "zhugeliang" => "你是诸葛亮，架构师。分析Bug、拆解任务。".into(), _ => "代码修复助手。".into() }
}

pub fn codex_exec_pipeline(tasks: &[(String, String)], sandbox: &str, timeout_secs: u64) -> Vec<CodexExecResult> {
    tasks.iter().map(|(a, t)| codex_exec(t, sandbox, None, Some(a), timeout_secs)).collect()
}
