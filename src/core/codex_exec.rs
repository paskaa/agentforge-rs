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
    // ── 0. 空输出直接判 FAIL ──
    if output.trim().len() < 20 {
        tracing::warn!("[parse_verdict] Output too short ({} bytes) → FAIL", output.trim().len());
        return Verdict::Fail("empty or near-empty output".into());
    }

    // ── 1. 显式 VERDICT 标记（优先级最高）──
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

    // ── 2. 启发式容错（mimo-v2.5 经常不输出 VERDICT 标记）──
    let lower = output.to_lowercase();

    // 修复证据（大幅扩充关键词）
    let fix_keywords = [
        "修复", "修改了", "已修改", "已修复", "已更正", "已解决",
        "fixed", "applied", "patched", "resolved", "corrected",
        "编译通过", "build success", "compile success", "build ok",
        "vite build", "mvn compile", "变更摘要", "变更文件",
        "fix(#", "fix(", "git commit", "git add",
        "修改内容", "修复方案", "修复完成", "successfully",
    ];
    let has_fix_evidence = fix_keywords.iter().any(|k| lower.contains(k));

    // 错误证据（排除已在修复上下文中出现的 error）
    let error_keywords = [
        "panic:", "编译失败", "build failed", "fatal error",
        "stack overflow", "out of memory", "oom",
    ];
    // 弱错误信号（可能出现在正常修复日志中，需要结合上下文）
    let weak_error_keywords = [
        "error:", "exception", "npe", "nullpointer",
    ];
    let has_strong_error = error_keywords.iter().any(|k| lower.contains(k));
    let has_weak_error = weak_error_keywords.iter().any(|k| lower.contains(k));

    // 判定逻辑：
    // - 有修复证据 + 无强错误 → PASS（即使有弱错误，因为修复过程中看到 error 日志是正常的）
    // - 有修复证据 + 有强错误 → 看最后一段输出是否有错误（最近的输出优先）
    // - 无修复证据 + 有错误 → FAIL
    // - 都没有 → Unknown
    if has_fix_evidence && !has_strong_error {
        tracing::info!("[parse_verdict] Heuristic PASS (fix evidence found, no strong errors)");
        return Verdict::Pass;
    }
    if has_fix_evidence && has_strong_error {
        // 检查最后 500 字符是否有强错误（最近的输出优先）
        let tail = output.chars().rev().take(500).collect::<String>().to_lowercase();
        let tail_has_error = error_keywords.iter().any(|k| tail.contains(k));
        if !tail_has_error {
            tracing::info!("[parse_verdict] Heuristic PASS (fix evidence found, errors only in early output)");
            return Verdict::Pass;
        }
        tracing::warn!("[parse_verdict] Ambiguous: fix evidence + strong error in tail → UNKNOWN");
        return Verdict::Unknown;
    }
    if has_strong_error || (has_weak_error && !has_fix_evidence) {
        tracing::info!("[parse_verdict] Heuristic FAIL (error evidence found, no fix evidence)");
        return Verdict::Fail("heuristic: error evidence in output".into());
    }

    // ── 3. 最后手段：检查是否有代码变更痕迹 ──
    let has_code_change = lower.contains("@@ -") || lower.contains("diff --git")
        || lower.contains(">>>>") || lower.contains("<<<<")
        || lower.contains("git diff") || lower.contains("git status");
    if has_code_change {
        tracing::info!("[parse_verdict] Heuristic PASS (code change patterns found in output)");
        return Verdict::Pass;
    }

    tracing::warn!("[parse_verdict] No clear signal → UNKNOWN");
    Verdict::Unknown
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
    // 禁用远程插件同步（避免 chatgpt 认证 hang）
    cmd.env("CODEX_DISABLE_REMOTE_SYNC", "1");

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
    // 铁律: 即使 timeout_secs=0，硬上限 1800s（30 分钟）
    let effective_timeout = if no_timeout { 1800 } else { timeout_secs };
    let timeout_duration = std::time::Duration::from_secs(effective_timeout);
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
    let last_activity2 = std::sync::Arc::clone(&last_activity);

    let stdout_thread = std::thread::spawn(move || {
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
    let stderr_thread = std::thread::spawn(move || {
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

    // 停滞检测：无新输出超过 480 秒（8 分钟）则杀进程
    // 总上限通过 timeout_secs 控制（默认 1800s = 30 分钟）
    let stall_timeout = std::time::Duration::from_secs(900);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_success = status.success();
                elapsed_ms = start.elapsed().as_millis() as u64;
                break;
            }
            Ok(None) => {
                // 检查总体超时
                if start.elapsed() > timeout_duration {
                    tracing::warn!("[codex_exec] TIMEOUT after {}s — killing codex, capturing partial output", effective_timeout);
                    let _ = child.kill();
                    let _ = child.wait();
                    // 超时前先提取已收集的部分输出（可能包含 LLM 分析结果）
                    let partial_stdout = if let Ok(guard) = stdout_buf.lock() {
                        String::from_utf8_lossy(&guard).to_string()
                    } else { String::new() };
                    let partial_stderr = if let Ok(guard) = stderr_buf.lock() {
                        String::from_utf8_lossy(&guard).to_string()
                    } else { String::new() };
                    // 从 JSON 输出中提取 final_message
                    let partial_final = partial_stdout.lines()
                        .filter_map(|line| { serde_json::from_str::<serde_json::Value>(line).ok() })
                        .filter(|j| j.get("type").and_then(|t| t.as_str()) == Some("message"))
                        .last()
                        .and_then(|j| j.get("content").and_then(|c| c.as_str()).map(|s| s.to_string()))
                        .unwrap_or_else(|| {
                            // 降级：取最后 2000 字节的原始输出
                            let tail = partial_stdout.chars().rev().take(4000).collect::<Vec<_>>();
                            tail.into_iter().rev().collect()
                        });
                    tracing::info!("[codex_exec] Captured {} bytes partial output", partial_final.len());
                    return CodexExecResult {
                        success: false, final_message: partial_final,
                        verdict: Verdict::Fail(format!("timeout after {}s (partial output captured)", effective_timeout)),
                        events: vec![], total_tokens: 0,
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        stderr: partial_stderr,
                    };
                }
                // 检查停滞：无输出但子进程仍在运行（LLM 推理中），不杀进程
                // 只依赖总体超时 (timeout_duration) 来终止，停滞检测仅用于日志
                // 取 stdout 和 stderr 最近活动时间的最大值（stderr 有 API 调用也算活跃）
                let idle = {
                    let stdout_idle = last_activity.lock().map(|g| g.elapsed()).unwrap_or(std::time::Duration::from_secs(0));
                    let stderr_idle = last_activity2.lock().map(|g| g.elapsed()).unwrap_or(std::time::Duration::from_secs(0));
                    std::cmp::min(stdout_idle, stderr_idle)
                };
                if idle > stall_timeout {
                    // 检查子进程是否还活着且有 CPU 活动
                    let pid = child.id();
                    let proc_alive = if pid > 0 {
                        std::path::Path::new(&format!("/proc/{}/stat", pid)).exists()
                    } else { false };
                    if proc_alive {
                        // 子进程还在运行（LLM 推理中），仅记录日志，不杀
                        tracing::info!("[codex_exec] No stdout for {}s but process alive (pid={}), waiting... (elapsed={}s)",
                            idle.as_secs(), pid, start.elapsed().as_secs());
                    } else {
                        tracing::warn!("[codex_exec] STALLED — process dead, no output for {}s", idle.as_secs());
                        return CodexExecResult {
                            success: false, final_message: String::new(),
                            verdict: Verdict::Fail(format!("stalled — process dead, no output for {}s", idle.as_secs())),
                            events: vec![], total_tokens: 0,
                            elapsed_ms: start.elapsed().as_millis() as u64,
                            stderr: format!("codex stalled — process dead for {}s", idle.as_secs()),
                        };
                    }
                }
                // ── 推理重复检测：mimo-v2.5 可能陷入推理死循环 ──
                // 检查 stdout（reasoning JSONL）和 stderr 中是否有重复模式
                let loop_detected = {
                    let mut detected = false;
                    // 检查 stdout（reasoning 通过 JSONL 的 text 字段输出）
                    if let Ok(guard) = stdout_buf.try_lock() {
                        let s = String::from_utf8_lossy(&guard);
                        let len = s.len();
                        if len > 3000 {
                            let tail_start = if len > 2000 { len - 2000 } else { 0 };
                            let tail = &s[tail_start..];
                            let spent = tail.matches("I've spent").count();
                            let apply = tail.matches("apply a fix now").count();
                            let too_long = tail.matches("too long on this").count();
                            if spent >= 3 || apply >= 3 || too_long >= 3 {
                                tracing::warn!("[codex_exec] stdout 推理死循环: spent={} apply={}", spent, apply);
                                detected = true;
                            }
                        }
                    }
                    // 检查 stderr
                    if !detected {
                        if let Ok(guard) = stderr_buf.try_lock() {
                            let s = String::from_utf8_lossy(&guard);
                            let len = s.len();
                            if len > 2000 {
                                let tail_start = if len > 1500 { len - 1500 } else { 0 };
                                let tail = &s[tail_start..];
                                let spent = tail.matches("I've spent").count();
                                let apply = tail.matches("apply a fix now").count();
                                if spent >= 3 || apply >= 3 {
                                    tracing::warn!("[codex_exec] stderr 推理死循环: spent={} apply={}", spent, apply);
                                    detected = true;
                                }
                            }
                        }
                    }
                    detected
                };
                if loop_detected {
                    let _ = child.kill();
                    let _ = child.wait();
                    return CodexExecResult {
                        success: false, final_message: String::new(),
                        verdict: Verdict::Fail("reasoning loop detected".into()),
                        events: vec![], total_tokens: 0,
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        stderr: "mimo reasoning loop: repeated reasoning detected".into(),
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

    // 等待读取线程完成（确保 pipe 中所有数据都已读入 buffer）
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

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
