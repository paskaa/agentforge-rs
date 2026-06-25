//! Agent Exec — 调用 mimo-code CLI 的执行层
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::{Mutex, Condvar, atomic::{AtomicU32, AtomicBool, Ordering}};
use std::time::Duration;
use std::time::Instant;

const MAX_CONCURRENT: u32 = 1;
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

/// Redis-based global API lock — ensures only 1 agent calls mimo-code across all processes
fn api_lock_acquire(agent: &str) -> String {
    let client = redis::Client::open("redis://127.0.0.1:16379/").ok();
    let mut conn = match client.and_then(|c| c.get_connection().ok()) {
        Some(c) => c,
        None => { tracing::warn!("[api_lock] Redis unavailable, proceeding without lock"); return "bypass".into(); }
    };
    let lock_key = "api_lock:mimo";
    // Unique lock value per invocation: agent + random suffix
    let lock_id = format!("{}:{}", agent, rand_id());
    let ttl: u64 = 1800; // 30 min TTL
    for wait_round in 0..120 { // max wait 60 min
        let acquired: bool = redis::cmd("SET")
            .arg(lock_key).arg(&lock_id)
            .arg("NX").arg("EX").arg(ttl)
            .query(&mut conn).unwrap_or(false);
        if acquired {
            tracing::info!("[api_lock] {} acquired API lock (id={})", agent, lock_id);
            return lock_id;
        }
        // Check who holds it
        let holder: String = redis::cmd("GET").arg(lock_key).query(&mut conn).unwrap_or_default();
        let holder_agent = holder.split(':').next().unwrap_or("?");
        tracing::info!("[api_lock] {} waiting... lock held by {} (round {}/120)", agent, holder_agent, wait_round + 1);
        std::thread::sleep(Duration::from_secs(30));
    }
    tracing::warn!("[api_lock] {} gave up waiting after 60min", agent);
    "gave_up".into()
}

fn api_lock_release(lock_id: &str) {
    if lock_id == "bypass" || lock_id == "gave_up" { return; }
    let client = redis::Client::open("redis://127.0.0.1:16379/").ok();
    let mut conn = match client.and_then(|c| c.get_connection().ok()) {
        Some(c) => c,
        None => return,
    };
    let lock_key = "api_lock:mimo";
    let holder: String = redis::cmd("GET").arg(lock_key).query(&mut conn).unwrap_or_default();
    if holder == lock_id {
        let _: Result<(), _> = redis::cmd("DEL").arg(lock_key).query(&mut conn);
        tracing::info!("[api_lock] released API lock (id={})", lock_id);
    }
}

fn rand_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{:x}", t)
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
    // Redis global lock — only 1 agent at a time across all processes
    let agent = agent_name.unwrap_or("unknown");
    let mut _lock_id = api_lock_acquire(agent);
    sem_acquire();
    let _sem_guard = SemGuard;
    let mimo_bin = std::env::var("MIMO_CODE_PATH").unwrap_or_else(|_| "mimo-code".into());
    let agent_ctx = agent_name.map(|a| get_agent_role_context(a)).unwrap_or_default();
    let full_task = if agent_ctx.is_empty() { task.to_string() } else { format!("{}\n\n{}", agent_ctx, task) };
    let work_dir = agent_name.map(|a| { let d = format!("/tmp/agentforge-worktrees/{}", a); std::fs::create_dir_all(&d).ok(); d }).map(std::path::PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    tracing::info!("[agent_exec] spawning: {} in {:?}", mimo_bin, work_dir);
    let mut attempt = 0u32;
    let max_attempts = 10u32;
    let result = loop {
        attempt += 1;
        let mut cmd = Command::new(&mimo_bin);
        cmd.arg("run").arg("-y").arg("--no-tui").arg("--sandbox").arg("danger-full-access").arg("--max-iterations").arg("8").arg(&full_task);
        cmd.current_dir(&work_dir); cmd.stdin(Stdio::null()); cmd.stdout(Stdio::piped()); cmd.stderr(Stdio::piped());
        let child = match cmd.spawn() { Ok(c) => c, Err(e) => { break CodexExecResult { success: false, final_message: format!("spawn failed: {}", e), verdict: Verdict::Fail(format!("spawn: {}", e)), total_tokens: 0, elapsed_ms: start.elapsed().as_millis() as u64, stderr: e.to_string() }; } };
        let output = match child.wait_with_output() { Ok(o) => o, Err(e) => { break CodexExecResult { success: false, final_message: format!("wait failed: {}", e), verdict: Verdict::Fail(format!("wait: {}", e)), total_tokens: 0, elapsed_ms: start.elapsed().as_millis() as u64, stderr: e.to_string() }; } };
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_success = output.status.success();
        let final_message = extract_mimo_response(&stdout);
        let total_tokens = parse_mimo_tokens(&stdout);
        let verdict = parse_verdict(if final_message.is_empty() { &stdout } else { &final_message });
        let success = exit_success && verdict.is_pass();
        tracing::info!("[agent_exec] completed: attempt={} exit={} elapsed={}ms tokens={}", attempt, output.status, elapsed_ms, total_tokens);
        if !stderr.is_empty() {
            let snippet: String = stderr.chars().take(500).collect();
            tracing::warn!("[agent_exec] stderr: {}", snippet);
        }
        if !exit_success && (stderr.contains("429") || stdout.contains("429")) && attempt < max_attempts {
            let delay = std::time::Duration::from_secs(120 * (1u64 << attempt.min(5)));
            tracing::warn!("[agent_exec] 429 rate limit, retry {} in {}s — releasing lock during wait", attempt + 1, delay.as_secs());
            // Release lock during retry — unique lock_id prevents self-deadlock
            api_lock_release(&_lock_id);
            std::thread::sleep(delay);
            // Re-acquire lock before next attempt
            _lock_id = api_lock_acquire(agent);
            continue;
        }
        break CodexExecResult { success, final_message: if final_message.is_empty() { stdout.clone() } else { final_message }, verdict, total_tokens, elapsed_ms, stderr };
    };
    api_lock_release(&_lock_id);
    result
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
