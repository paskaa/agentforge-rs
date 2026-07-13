//! Agent Exec — 调用 opencode CLI 的执行层
//!
//! 使用 opencode run 管道
//! opencode run --agent <agent> --pure --print-logs < prompt

use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::{Mutex, Condvar, atomic::{AtomicU32, Ordering}};
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

struct SemGuard;
impl Drop for SemGuard { fn drop(&mut self) { sem_release(); } }

/// Agent 模型映射（使用 claude 可用模型）
fn get_agent_model(agent_name: &str) -> String {
    match agent_name {
        "zhugeliang" => "claude-sonnet-4-5".to_string(),
        "zhouyu"     => "claude-sonnet-4-5".to_string(),
        "zhaoyun"    => "claude-sonnet-4-5".to_string(),
        "guanyu"     => "claude-sonnet-4-5".to_string(),
        "zhangfei"   => "claude-sonnet-4-5".to_string(),
        "simayi"     => "claude-sonnet-4-5".to_string(),
        "lusu"       => "claude-sonnet-4-5".to_string(),
        "huanggai"   => "claude-sonnet-4-5".to_string(),
        "gaoshun"    => "claude-sonnet-4-5".to_string(),
        "chendao"    => "claude-sonnet-4-5".to_string(),
        "simashi"    => "claude-sonnet-4-5".to_string(),
        "huatuo"     => "claude-sonnet-4-5".to_string(),
        "chenlin"    => "claude-sonnet-4-5".to_string(),
        "liubei"     => "claude-sonnet-4-5".to_string(),
        "xunyu"      => "claude-sonnet-4-5".to_string(),
        "guanping"   => "claude-sonnet-4-5".to_string(),
        "leixu"      => "claude-sonnet-4-5".to_string(),
        "wulan"      => "claude-sonnet-4-5".to_string(),
        "machao"     => "claude-sonnet-4-5".to_string(),
        "madai"      => "claude-sonnet-4-5".to_string(),
        "pangde"     => "claude-sonnet-4-5".to_string(),
        _            => "claude-sonnet-4-5".to_string(), // 默认 fallback
    }
}

/// Agent 角色上下文（简化版，用于 opencode prompt）
fn get_agent_role_context(n: &str) -> String {
    match n {
        "guanyu"    => "你是关羽，后端修复工程师。Java/Spring/MyBatis。修复后 mvn compile 验证。".into(),
        "zhaoyun"   => "你是赵云，前端修复工程师。Vue3/ElementUI/TypeScript。修复后 vite build 验证。".into(),
        "xunyu"     => "你是荀彧，数据库工程师。PostgreSQL/DDL/DML。修复后检查迁移脚本。".into(),
        "zhangfei"  => "你是张飞，QA测试工程师。Playwright回归测试。".into(),
        "huatuo"    => "你是华佗，产品验收员。验证修复是否满足需求。".into(),
        "chenlin"   => "你是陈琳，文档工程师。生成修复文档。".into(),
        "zhugeliang"=> "你是诸葛亮，架构师。分析Bug、拆解任务。".into(),
        _           => "代码修复助手。".into()
    }
}

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

/// 从 opencode 输出中解析 VERDICT
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

/// 从 opencode 输出中提取有效消息
fn extract_opencode_message(stdout: &str) -> String {
    // opencode run 输出包含大量调试信息，提取最后的 AI 响应
    let lines: Vec<&str> = stdout.lines().collect();
    // 从后往前找有意义的行
    for line in lines.iter().rev() {
        let t = line.trim();
        if t.len() > 50 && !t.contains("DEBUG") && !t.contains("TRACE") && !t.contains("token") {
            return t.to_string();
        }
    }
    // fallback: 返回最后 500 字符
    let tail_len = std::cmp::min(500, stdout.len());
    stdout[stdout.len() - tail_len..].to_string()
}

/// opencode exec — 替代 codex_exec，使用 opencode run CLI
///
/// # 参数
/// - `task`: 任务描述/prompt
/// - `sandbox`: 沙箱权限（目前 opencode 不支持，保留接口）
/// - `schema_path`: 模式文件路径（目前 opencode 不支持，保留接口）
/// - `agent_name`: Agent 名称，决定使用哪个 agent 配置和模型
/// - `timeout_secs`: 超时秒数（0 表示无超时）
pub fn codex_exec(task: &str, _sandbox: &str, _schema_path: Option<&str>, 
                  agent_name: Option<&str>, timeout_secs: u64) -> CodexExecResult {
    let start = Instant::now();
    
    // 确定 agent 和模型
    let agent = agent_name.unwrap_or("guanyu");
    let model = get_agent_model(agent);
    let agent_ctx = get_agent_role_context(agent);
    let full_task = if agent_ctx.is_empty() { 
        task.to_string() 
    } else { 
        format!("{}\n\n{}", agent_ctx, task) 
    };
    
    // 确定工作目录（使用 agent worktree）
    let work_dir = agent_name.map(|a| { 
        let d = format!("/tmp/agentforge-worktrees/{}", a); 
        std::fs::create_dir_all(&d).ok(); 
        d 
    }).map(std::path::PathBuf::from)
    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    
    tracing::info!("[opencode] spawning: agent={} dir={:?}", agent, work_dir);
    
    sem_acquire();
    let _sem_guard = SemGuard;
    
    // 构建 opencode 命令
    let mut cmd = Command::new("opencode");
    cmd.arg("run")
       .arg("--agent")
       .arg(agent)
       .arg("--pure")
       .arg("--print-logs")
       .arg("-")  // read prompt from stdin
       .current_dir(&work_dir)
       .stdin(Stdio::piped())
       .stdout(Stdio::piped())
       .stderr(Stdio::piped());
    
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            sem_release();
            return CodexExecResult {
                success: false,
                final_message: format!("opencode run failed: {}", e),
                verdict: Verdict::Fail(format!("spawn: {}", e)),
                total_tokens: 0,
                elapsed_ms: start.elapsed().as_millis() as u64,
                stderr: e.to_string(),
            };
        }
    };
    
    // 写入 prompt 到 stdin
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(full_task.as_bytes());
        // opencode 需要关闭 stdin 才能开始处理
        drop(stdin);
    }
    
    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            sem_release();
            return CodexExecResult {
                success: false,
                final_message: format!("opencode run wait failed: {}", e),
                verdict: Verdict::Fail(format!("wait: {}", e)),
                total_tokens: 0,
                elapsed_ms: start.elapsed().as_millis() as u64,
                stderr: e.to_string(),
            };
        }
    };
    
    let elapsed_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_success = output.status.success();
    let final_message = extract_opencode_message(&stdout);
    let total_tokens = 0u64; // opencode 不直接暴露 token 数
    let verdict = parse_verdict(if final_message.is_empty() { &stdout } else { &final_message });
    let success = exit_success && verdict.is_pass();
    
    tracing::info!("[opencode] completed: exit={} elapsed={}ms verdict={:?}", 
                   output.status, elapsed_ms, verdict);
    
    if !stderr.is_empty() {
        let snippet: String = stderr.chars().take(500).collect();
        tracing::warn!("[opencode] stderr: {}", snippet);
    }
    
    CodexExecResult {
        success,
        final_message: if final_message.is_empty() { stdout.clone() } else { final_message },
        verdict,
        total_tokens,
        elapsed_ms,
        stderr,
    }
}

/// 批量执行多个任务（使用 opencode run）
pub fn codex_exec_pipeline(tasks: &[(String, String)], _sandbox: &str, _timeout_secs: u64) -> Vec<CodexExecResult> {
    tasks.iter()
         .map(|(agent, task)| codex_exec(task, "read-only", None, Some(agent), 3600))
         .collect()
}
