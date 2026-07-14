//! Agent Exec — 直接调用 Sensenova API（绕过 UltraWork 插件）
//!
//! 直接 HTTP POST → https://token.sensenova.cn/v1/chat/completions
//! 模型映射: zhugeliang → glm-5.2, 其余 → deepseek-v4-flash

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Mutex, Condvar, atomic::{AtomicU32, Ordering}};
use std::time::{Duration, Instant};

const MAX_CONCURRENT: u32 = 1;
static SEM_COUNTER: AtomicU32 = AtomicU32::new(0);
static SEM_MTX: Mutex<()> = Mutex::new(());
static SEM_CV: Condvar = Condvar::new();

const API_BASE: &str = "https://token.sensenova.cn/v1";
const API_KEY: &str = "sk-bXHx8SgfDdENopLA2SxPbsb5pusnhGor";

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

/// Agent → 模型映射（sensenova）
fn get_agent_model(agent_name: &str) -> &'static str {
    match agent_name {
        "zhugeliang" => "deepseek-v4-flash",  // glm-5.2 频繁 429，改用 deepseek-v4-flash
        "zhouyu" | "simayi" | "lusu" => "deepseek-v4-flash",
        _ => "deepseek-v4-flash", // 默认: guanyu, zhaoyun, zhangfei, etc.
    }
}

/// Agent 角色上下文（简化版，注入 system prompt）
fn get_agent_role_context(n: &str) -> String {
    match n {
        "guanyu"    => "你是关羽，后端修复工程师。精通 Java/Spring/MyBatis。修复后必须用 mvn compile 验证。".into(),
        "zhaoyun"   => "你是赵云，前端修复工程师。精通 Vue3/ElementUI/TypeScript。修复后必须用 vite build 验证。".into(),
        "xunyu"     => "你是荀彧，数据库工程师。精通 PostgreSQL/DDL/DML。修复后检查迁移脚本。".into(),
        "zhangfei"  => "你是张飞，QA测试工程师。负责 Playwright 回归测试。".into(),
        "huatuo"    => "你是华佗，产品验收员。验证修复是否满足需求。".into(),
        "chenlin"   => "你是陈琳，文档工程师。生成修复文档。".into(),
        "zhugeliang"=> "你是诸葛亮，架构师。分析Bug、拆解任务、路由给正确的修复人员。".into(),
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

/// 从输出中解析 VERDICT
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

/// 直接调用 API（替代 opencode）
pub fn codex_exec(task: &str, _sandbox: &str, _schema_path: Option<&str>, 
                  agent_name: Option<&str>, timeout_secs: u64) -> CodexExecResult {
    let start = Instant::now();
    
    // 确定 agent 和模型
    let agent = agent_name.unwrap_or("guanyu");
    let model = get_agent_model(agent);
    let agent_ctx = get_agent_role_context(agent);
    
    // 构建 system + user prompt
    let system = if agent_ctx.is_empty() { "你是一个中文编程助手。使用简体中文思考和回复。".into() } else { agent_ctx };
    let user = task.to_string();
    
    tracing::info!("[api] calling: agent={} model={}", agent, model);
    
    sem_acquire();
    let _sem_guard = SemGuard;
    
    // 直接 HTTP 调用 API
    let url = format!("{}/chat/completions", API_BASE);
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "max_tokens": 4096,
        "temperature": 0.3,
    });
    
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(if timeout_secs > 0 { timeout_secs } else { 300 }))
        .build();
    
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            sem_release();
            return CodexExecResult {
                success: false, final_message: String::new(),
                verdict: Verdict::Fail(format!("client build: {}", e)),
                total_tokens: 0, elapsed_ms: start.elapsed().as_millis() as u64,
                stderr: e.to_string(),
            };
        }
    };
    
    let resp = client.post(&url)
        .header("Authorization", format!("Bearer {}", API_KEY))
        .header("Content-Type", "application/json")
        .json(&body)
        .send();
    
    let elapsed_ms = start.elapsed().as_millis() as u64;
    
    match resp {
        Ok(r) => {
            let status = r.status();
            match r.json::<serde_json::Value>() {
                Ok(json_body) => {
                    // 提取 content
                    let content = json_body["choices"][0]["message"]["content"]
                        .as_str().unwrap_or("")
                        .to_string();
                    let reasoning = json_body["choices"][0]["message"]["reasoning_content"]
                        .as_str().unwrap_or("")
                        .to_string();
                    
                    // 提取 token 数
                    let total_tokens = json_body["usage"]["total_tokens"].as_u64().unwrap_or(0);
                    
                    // 组合输出
                    let final_message = if !reasoning.is_empty() {
                        format!("{}\n\n{}", reasoning, content)
                    } else {
                        content.clone()
                    };
                    
                    let success = status.is_success() && !content.is_empty();
                    let verdict = parse_verdict(&final_message);
                    
                    tracing::info!("[api] completed: status={} elapsed={}ms tokens={} verdict={:?}", 
                                   status, elapsed_ms, total_tokens, verdict);
                    
                    CodexExecResult {
                        success,
                        final_message: if final_message.is_empty() { content } else { final_message },
                        verdict,
                        total_tokens,
                        elapsed_ms,
                        stderr: String::new(),
                    }
                }
                Err(e) => {
                    let stderr = format!("JSON parse error: {} (status={})", e, status);
                    tracing::warn!("[api] JSON error: {}", stderr);
                    CodexExecResult {
                        success: false, final_message: String::new(),
                        verdict: Verdict::Fail(format!("json: {}", e)),
                        total_tokens: 0, elapsed_ms,
                        stderr,
                    }
                }
            }
        }
        Err(e) => {
            let stderr = format!("HTTP error: {}", e);
            tracing::warn!("[api] HTTP error: {}", stderr);
            CodexExecResult {
                success: false, final_message: String::new(),
                verdict: Verdict::Fail(format!("http: {}", e)),
                total_tokens: 0, elapsed_ms,
                stderr,
            }
        }
    }
}

/// 批量执行多个任务
pub fn codex_exec_pipeline(tasks: &[(String, String)], _sandbox: &str, _timeout_secs: u64) -> Vec<CodexExecResult> {
    tasks.iter()
         .map(|(agent, task)| codex_exec(task, "read-only", None, Some(agent), 3600))
         .collect()
}