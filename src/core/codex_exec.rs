//! Codex Exec — 直接调用 `codex exec` 的执行层
//!
//! 替代 codex-aliyun → mimo2codex → codex 管道，直接使用 Codex CLI 的
//! 非交互模式 (codex exec --json)。
//!
//! 核心原理 (来自 HuanCode Agent Loop):
//!   while True:
//!       response = model.call(messages, tools)
//!       if stop_reason != "tool_use": return
//!       execute_tools(response.tool_calls)
//!
//! Codex CLI 内置了完整的 Agent Loop，我们只需要:
//!   1. 传入 task prompt
//!   2. 解析 JSONL 事件流
//!   3. 提取最终消息和 VERDICT

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Instant;

/// Codex JSONL 事件类型
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum CodexEvent {
    #[serde(rename = "thread.started")]
    ThreadStarted { thread_id: String },
    #[serde(rename = "turn.started")]
    TurnStarted {},
    #[serde(rename = "turn.completed")]
    TurnCompleted { usage: Option<TurnUsage> },
    #[serde(rename = "turn.failed")]
    TurnFailed { error: Option<String> },
    #[serde(rename = "item.started")]
    ItemStarted { item: CodexItem },
    #[serde(rename = "item.completed")]
    ItemCompleted { item: CodexItem },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(other)]
    Unknown,
}

/// Codex 事件中的 item
#[derive(Debug, Clone, Deserialize)]
pub struct CodexItem {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub item_type: String,
    pub text: Option<String>,
    pub command: Option<String>,
    pub status: Option<String>,
}

/// Codex 使用量统计
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TurnUsage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
}

/// VERDICT 二元输出
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Pass,
    Fail(String),
    Unknown,
}

impl Verdict {
    pub fn is_pass(&self) -> bool { matches!(self, Verdict::Pass) }
    pub fn is_fail(&self) -> bool { matches!(self, Verdict::Fail(_)) }
}

/// Codex 执行结果
#[derive(Debug, Clone)]
pub struct CodexExecResult {
    pub success: bool,
    pub final_message: String,
    pub verdict: Verdict,
    pub events: Vec<CodexEvent>,
    pub total_tokens: u64,
    pub elapsed_ms: u64,
    pub stderr: String,
}

/// 从输出中解析 VERDICT
pub fn parse_verdict(output: &str) -> Verdict {
    for line in output.lines() {
        let line = line.trim();
        if line.contains("VERDICT:") || line.contains("VERDICT：") {
            if line.contains("PASS") || line.contains("通过") {
                return Verdict::Pass;
            }
            if line.contains("FAIL") || line.contains("失败") {
                let reason = line
                    .split(&['：', ':'][..])
                    .nth(1)
                    .map(|s| {
                        s.trim()
                            .trim_start_matches("FAIL")
                            .trim_start_matches("失败")
                            .trim_start_matches(&['[', '（', '('][..])
                            .trim_end_matches(&[']', '）', ')'][..])
                            .trim()
                            .to_string()
                    })
                    .unwrap_or_else(|| "未提供原因".to_string());
                return Verdict::Fail(reason);
            }
        }
    }

    // ── 启发式容错：codex 可能不输出 VERDICT 标记 ──
    let lower = output.to_lowercase();
    let has_fix_evidence = lower.contains("修复") || lower.contains("修改了")
        || lower.contains("fixed") || lower.contains("applied")
        || lower.contains("patch") || lower.contains("已修复")
        || lower.contains("编译通过") || lower.contains("build success")
        || lower.contains("compile success");
    let has_error_evidence = lower.contains("error:") || lower.contains("panic:")
        || lower.contains("编译失败") || lower.contains("build failed")
        || lower.contains("exception") || lower.contains("npe");

    if has_fix_evidence && !has_error_evidence {
        tracing::info!("[parse_verdict] Heuristic PASS (no VERDICT marker but fix evidence found)");
        Verdict::Pass
    } else if has_error_evidence && !has_fix_evidence {
        tracing::info!("[parse_verdict] Heuristic FAIL (no VERDICT marker, error evidence found)");
        Verdict::Fail("heuristic: error evidence in output".into())
    } else {
        Verdict::Unknown
    }
}

/// 执行 codex exec 命令
///
/// 等价于 HuanCode 的:
///   response = client.messages.create(model=MODEL, messages=messages, tools=TOOLS)
///   if response.stop_reason != "tool_use": return
///
/// Codex CLI 内部已经实现了完整的 Agent Loop，
/// 我们只需要传入 task 和接收 JSONL 输出。
pub fn codex_exec(
    task: &str,
    sandbox: &str,          // "workspace-write" | "read-only" | "danger-full-access"
    schema_path: Option<&str>,
    agent_name: Option<&str>,
    timeout_secs: u64,
) -> CodexExecResult {
    let start = Instant::now();

    // 构建命令
    let mut cmd = Command::new("codex");
    cmd.arg("exec")
       .arg("--sandbox").arg(sandbox)
       .arg("--dangerously-bypass-approvals-and-sandbox")
       .arg("--json");

    // 添加 agent 角色上下文
    let full_task = if let Some(agent) = agent_name {
        let role_context = get_agent_role_context(agent);
        format!("{}\n\n{}", role_context, task)
    } else {
        task.to_string()
    };

    // 添加 output schema
    if let Some(schema) = schema_path {
        cmd.arg("--output-schema").arg(schema);
    }

    cmd.arg(&full_task);

    // 设置工作目录 — 使用 agent worktree（不是主仓库）
    // 铁律: Codex 必须在 worktree 中工作，不能在主仓库中修改文件
    let work_dir = if let Some(agent) = agent_name {
        format!("/tmp/agentforge-worktrees/{}", agent)
    } else {
        "/root/.openclaw/workspace/his-repo".to_string()
    };
    cmd.current_dir(&work_dir);

    // 执行（带超时保护：spawn + try_wait 循环）
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return CodexExecResult {
                success: false, final_message: String::new(),
                verdict: Verdict::Fail(format!("spawn error: {}", e)),
                events: vec![], total_tokens: 0,
                elapsed_ms: start.elapsed().as_millis() as u64,
                stderr: format!("spawn error: {}", e),
            };
        }
    };

    let no_timeout = timeout_secs == 0;
    let timeout_duration = if no_timeout { std::time::Duration::from_secs(u64::MAX) } else { std::time::Duration::from_secs(timeout_secs) };
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_success = true;
    let elapsed_ms;

    // 增量读取 stdout 的分离线程
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let stderr_buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let last_activity = std::sync::Arc::new(std::sync::Mutex::new(Instant::now()));
    let stdout_clone = std::sync::Arc::clone(&stdout_buf);
    let stderr_clone = std::sync::Arc::clone(&stderr_buf);
    let activity_clone = std::sync::Arc::clone(&last_activity);
    let activity_clone2 = std::sync::Arc::clone(&last_activity);

    let _stdout_thread = std::thread::spawn(move || {
        use std::io::Read;
        if let Some(mut pipe) = stdout_pipe {
            let mut buf = [0u8; 4096];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(mut guard) = stdout_clone.lock() {
                            guard.extend_from_slice(&buf[..n]);
                        }
                        if let Ok(mut guard) = activity_clone.lock() {
                            *guard = Instant::now();
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    });
    let _stderr_thread = std::thread::spawn(move || {
        use std::io::Read;
        if let Some(mut pipe) = stderr_pipe {
            let mut buf = [0u8; 4096];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(mut guard) = stderr_clone.lock() {
                            guard.extend_from_slice(&buf[..n]);
                        }
                        // stderr 也有输出，说明进程活跃
                        if let Ok(mut guard) = activity_clone2.lock() {
                            *guard = Instant::now();
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    });

    // 停滞检测：无新输出超过 300 秒（5 分钟）则杀进程
    let stall_timeout = std::time::Duration::from_secs(600);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_success = status.success();
                elapsed_ms = start.elapsed().as_millis() as u64;
                break;
            }
            Ok(None) => {
                // 检查总体超时
                if !no_timeout && start.elapsed() > timeout_duration {
                    tracing::warn!("[codex_exec] TIMEOUT after {}s — killing codex", timeout_secs);
                    let _ = child.kill();
                    let _ = child.wait();
                    return CodexExecResult {
                        success: false, final_message: String::new(),
                        verdict: Verdict::Fail(format!("timeout after {}s", timeout_secs)),
                        events: vec![], total_tokens: 0,
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        stderr: format!("codex timed out after {}s", timeout_secs),
                    };
                }
                // 检查停滞：有输出活动则不杀
                let idle = if let Ok(guard) = last_activity.lock() {
                    guard.elapsed()
                } else {
                    stall_timeout // 保守处理
                };
                if idle > stall_timeout {
                    tracing::warn!("[codex_exec] STALLED — no output for {}s, killing codex (elapsed={}s)",
                        idle.as_secs(), start.elapsed().as_secs());
                    let _ = child.kill();
                    let _ = child.wait();
                    return CodexExecResult {
                        success: false, final_message: String::new(),
                        verdict: Verdict::Fail(format!("stalled — no output for {}s", idle.as_secs())),
                        events: vec![], total_tokens: 0,
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        stderr: format!("codex stalled — no output for {}s", idle.as_secs()),
                    };
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Err(e) => {
                return CodexExecResult {
                    success: false, final_message: String::new(),
                    verdict: Verdict::Fail(format!("try_wait error: {}", e)),
                    events: vec![], total_tokens: 0,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    stderr: format!("process error: {}", e),
                };
            }
        }
    }

    // 从缓冲区读取 stdout/stderr
    stdout = stdout_buf.lock().map(|g| String::from_utf8_lossy(&g).to_string()).unwrap_or_default();
    stderr = stderr_buf.lock().map(|g| String::from_utf8_lossy(&g).to_string()).unwrap_or_default();


    // 解析 JSONL 事件流
    let mut events = Vec::new();
    let mut final_message = String::new();
    let mut total_tokens: u64 = 0;

    for line in stdout.lines() {
        if line.trim().is_empty() { continue; }
        if let Ok(event) = serde_json::from_str::<CodexEvent>(line) {
            match &event {
                CodexEvent::ItemCompleted { item } => {
                    if item.item_type == "agent_message" {
                        if let Some(text) = &item.text {
                            final_message = text.clone();
                        }
                    }
                }
                CodexEvent::TurnCompleted { usage } => {
                    if let Some(u) = usage {
                        total_tokens += u.output_tokens.unwrap_or(0);
                        total_tokens += u.input_tokens.unwrap_or(0);
                    }
                }
                _ => {}
            }
            events.push(event);
        }
    }

    // 如果没有 JSONL 事件，把 stdout 当作最终消息
    if final_message.is_empty() && !stdout.trim().is_empty() {
        final_message = stdout.trim().to_string();
    }

    let verdict = parse_verdict(&final_message);
    let success = exit_success && verdict.is_pass();

    tracing::info!(
        "[codex_exec] completed: verdict={} tokens={} elapsed={}ms",
        if verdict.is_pass() { "PASS" } else if verdict.is_fail() { "FAIL" } else { "UNKNOWN" },
        total_tokens, elapsed_ms
    );

    CodexExecResult {
        success,
        final_message,
        verdict,
        events,
        total_tokens,
        elapsed_ms,
        stderr,
    }
}

/// 获取 Agent 角色上下文 (注入到 prompt 中)
fn get_agent_role_context(agent_name: &str) -> String {
    match agent_name {
        "guanyu" => "你是关羽，后端修复工程师。负责 Java/Spring 后端修复。精通 MyBatis-Plus、Spring Boot、REST API、Maven。修复后运行 mvn compile 验证。".to_string(),
        "zhaoyun" => "你是赵云，前端修复工程师。负责 Vue3 前端修复。精通 ElementUI、TypeScript、Axios、Vite。修复后运行 vue-tsc --noEmit && vite build 验证。".to_string(),
        "xunyu" => "你是荀彧，数据库工程师。负责 SQL/数据库修复。精通 PostgreSQL、DDL、DML、索引优化。修复后检查迁移脚本规范。".to_string(),
        "zhangfei" => "你是张飞，QA 测试工程师。负责运行回归测试验证修复质量。使用 Playwright 运行测试。".to_string(),
        "huatuo" => "你是华佗，产品验收员。负责验证修复是否满足业务需求。检查测试文档和修复符合度。".to_string(),
        "chenlin" => "你是陈琳，文档工程师。负责生成和归档修复文档。".to_string(),
        "zhugeliang" => "你是诸葛亮，架构师。负责分析 Bug、拆解任务、分派给合适的修复 Agent。".to_string(),
        _ => "你是一个代码修复助手。".to_string(),
    }
}

/// 批量执行 codex exec (用于 pipeline)
pub fn codex_exec_pipeline(
    tasks: &[(String, String)],  // (agent_name, task_prompt)
    sandbox: &str,
    timeout_secs: u64,
) -> Vec<CodexExecResult> {
    tasks.iter().map(|(agent, task)| {
        codex_exec(task, sandbox, None, Some(agent), timeout_secs)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_verdict_pass() {
        let output = "所有检查通过\nVERDICT: PASS";
        assert!(parse_verdict(output).is_pass());
    }

    #[test]
    fn test_parse_verdict_fail() {
        let output = "测试失败\nVERDICT: FAIL [编译错误]";
        assert!(parse_verdict(output).is_fail());
    }

    #[test]
    fn test_parse_verdict_unknown() {
        let output = "没有VERDICT标记";
        assert!(matches!(parse_verdict(output), Verdict::Unknown));
    }

    #[test]
    fn test_parse_verdict_pass_chinese() {
        let output = "VERDICT：通过";
        assert!(parse_verdict(output).is_pass());
    }

    #[test]
    fn test_parse_verdict_fail_chinese() {
        let output = "VERDICT：失败 [功能不完整]";
        assert!(parse_verdict(output).is_fail());
    }
}
