//! Coordinator — scans all agent bugs and dispatches them to fixer queues.
//!
//! Runs every 5 minutes under zhugeliang (架构师).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use redis::AsyncCommands;
use serde_json;


/// All 8 agent IDs.
pub const ALL_AGENTS: &[&str] = &[
    "zhugeliang", "liubei", "guanyu", "zhaoyun",
    "xunyu", "zhangfei", "huatuo", "chenlin",
];

/// Expertise-based keyword routing (priority order).
const EXPERTISE: &[(&str, &[&str])] = &[
    ("xunyu", &["数据库", "sql", "慢查询", "索引", "表", "字段缺失", "ddl", "schema"]),
    ("guanyu", &["后端", "java", "api", "接口", "服务", "spring", "service", "controller",
                  "mapper", "后端报错", "保存失败", "事务", "缓存", "校验", "签发",
                  "退回", "撤回", "错误提示", "审计", "完诊", "操作失败", "div_log", "SQL", "执行科室", "库存",
                  "发药", "计费"]),
    ("zhaoyun", &["前端", "vue", "界面", "显示", "弹窗", "按钮", "列表", "回显",
                  "刷新", "不规范", "缺失", "操作项", "字段", "命名", "加载",
                  "过滤", "查询", "提示语", "样式", "组件", "渲染", "模板",
                  "提交申请", "检查申请", "报卡", "手术安排"]),
];

pub struct Coordinator {
    pub zentao_dir: PathBuf,
    pub agent_accounts: HashMap<String, String>,
    pub last_scan: Instant,
}

impl Coordinator {
    pub fn new(zentao_dir: PathBuf) -> Self {
        let mut accounts = HashMap::new();
        for id in ALL_AGENTS {
            accounts.insert(id.to_string(), id.to_string());
        }
        Self {
            zentao_dir,
            agent_accounts: accounts,
            last_scan: Instant::now() - Duration::from_secs(3600),
        }
    }

    /// Refresh Zentao token by calling the refresh script.
    pub fn refresh_token(&self) {
        let script = self.zentao_dir.join("zentao-token-refresh.sh");
        let _ = Command::new("bash")
            .arg(script)
            .arg("zhangfei")
            .output();
    }
}
#[allow(dead_code)]
fn parse_bug_line(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    // "1. #455 [一般] Title..."
    line.split('#').nth(1)
        .and_then(|s| s.split(char::is_whitespace).next())
        .map(|s| s.to_string())
}

/// Route a bug to a fixer agent based on title keywords.
pub fn route_bug(title: &str) -> &str {
    let t = title.to_lowercase();
    for (agent, keywords) in EXPERTISE {
        for kw in *keywords {
            if t.contains(kw) {
                return agent;
            }
        }
    }
    "zhaoyun" // default: frontend
}

// ── CLI entry points (called by Hermes bridge via `agentforge <subcommand>`) ──

const ZENTAO_DIR: &str = "/root/.openclaw/extensions/zentao-token-refresh";

/// Scan all agent bugs and print summary to stdout (using Rust API client).
pub async fn scan_bugs_cli() -> anyhow::Result<()> {
    let agents = ["zhugeliang", "liubei", "guanyu", "zhaoyun", "xunyu", "zhangfei", "huatuo", "chenlin"];
    
    // Map agent names to Zentao accounts
    let agent_accounts = [
        ("zhugeliang", "wangyizhe"),
        ("liubei", "liubei"),
        ("guanyu", "guanyu"),
        ("zhaoyun", "zhaoyun"),
        ("xunyu", "xunyu"),
        ("zhangfei", "zhangfei"),
        ("huatuo", "huatuo"),
        ("chenlin", "chenlin"),
    ];
    
    let cfg = crate::config::Config::load()?;
    let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
    
    let mut found = false;
    for (agent, account) in &agent_accounts {
        match client.get_my_bugs(account).await {
            Ok(bugs) if !bugs.is_empty() => {
                println!("【{}】", agent);
                for b in &bugs {
                    let sev = match b.severity.unwrap_or(3) {
                        1 => "致命",
                        2 => "严重",
                        3 => "一般",
                        4 => "轻微",
                        _ => "未知",
                    };
                    println!("  #{} [{}] {} — {}", b.id, sev, b.title, b.moduleTitle.as_deref().unwrap_or(""));
                }
                found = true;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("Failed to fetch bugs for {}: {}", agent, e);
            }
        }
    }
    if !found {
        println!("当前没有活跃的 Bug。");
    }
    Ok(())
}

/// Query a single bug and print detail to stdout — 使用 Rust API 客户端
pub async fn query_bug_cli(bug_id: &str) -> anyhow::Result<()> {
    let cfg = match crate::config::Config::load() {
        Ok(c) => c,
        Err(e) => {
            println!("加载配置失败: {}", e);
            return Ok(());
        }
    };
    let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
    match client.get_bug(bug_id).await {
        Ok(detail) => {
            println!("{}", detail.format_for_prompt());
        }
        Err(e) => {
            tracing::warn!("Zentao API 查询失败: {}, 尝试 shell 脚本回退", e);
            let Ok(out) = Command::new("bash")
                .arg(format!("{}/zentao-bug-query.sh", ZENTAO_DIR))
                .arg(bug_id)
                .output()
            else {
                println!("查询 Bug #{} 失败（API 和 shell 均不可用）", bug_id);
                return Ok(());
            };
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.trim().is_empty() {
                println!("Bug #{} 查询结果为空。", bug_id);
            } else {
                println!("{}", stdout.trim());
            }
        }
    }
    Ok(())
}

/// Submit a fix task to the Redis queue and print ack to stdout.
pub async fn submit_fix_cli(bug_id: &str, bug_title: &str, fixer: &str) -> anyhow::Result<()> {
    let cfg = crate::config::Config::load()?;
    let redis_url = cfg.redis_url();
    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    
    // 铁律：所有 Bug 必须先经过诸葛亮分析，再路由给修复 Agent
    let zhugeliang_queue = "agent-work-queue:fix:zhugeliang".to_string();
    
    // 去重：检查该 bug 是否已在诸葛亮队列中
    let existing_zg: Vec<String> = conn.lrange(&zhugeliang_queue, 0, -1).await.unwrap_or_default();
    if existing_zg.iter().any(|s| s.contains(&format!("Bug #{}", bug_id))) {
        println!("⏭️   Bug #{} 已在诸葛亮分析队列中，跳过重复分派", bug_id);
        return Ok(());
    }
    
    // 也检查是否已在最终修复 Agent 队列中
    let fixer_queue = format!("agent-work-queue:fix:{}", fixer);
    let existing_fixer: Vec<String> = conn.lrange(&fixer_queue, 0, -1).await.unwrap_or_default();
    if existing_fixer.iter().any(|s| s.contains(&format!("Bug #{}", bug_id))) {
        println!("⏭️   Bug #{} 已在 {} 队列中，跳过重复分派", bug_id, fixer);
        return Ok(());
    }
    
    // 发送给诸葛亮分析，携带建议的修复 Agent
    let task = serde_json::json!({
        "agent_id": "zhugeliang",
        "message": format!("请分析 Bug #{} 并设计修复方案，然后路由给合适的修复 Agent 执行。\n建议修复 Agent: {}\nBug 标题: {}", bug_id, fixer, bug_title),
        "source": "pipeline_pre_analyze",
        "sender_id": "cli",
        "suggested_fixer": fixer,
        "chat_id": "",
        "is_dm": "true",
        "msg_id": format!("pipeline-pre-analyze-{}-{}", bug_id, chrono::Local::now().timestamp()),
        "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    
    let _: redis::RedisResult<i64> = conn.rpush(&zhugeliang_queue, task.to_string()).await;
    
    println!("✅ Bug #{} 已提交给诸葛亮分析（建议修复 Agent: {}）", bug_id, fixer);
    Ok(())
}

/// Assign a bug to a specific fixer and print ack.
pub async fn assign_bug_cli(bug_id: &str, fixer: &str) -> anyhow::Result<()> {
    let valid_fixers = ["zhaoyun", "guanyu", "xunyu"];
    if !valid_fixers.contains(&fixer) {
        println!("无效的修复智能体：{}。可选：zhaoyun(前端), guanyu(后端), xunyu(数据库)", fixer);
        return Ok(());
    }
    
    let cfg = crate::config::Config::load()?;
    let redis_url = cfg.redis_url();
    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    
    let task = serde_json::json!({
        "agent_id": fixer,
        "message": format!("请修复 Bug #{}", bug_id),
        "source": "hermes_assign",
        "sender_id": "hermes",
        "chat_id": "",
        "is_dm": "true",
        "msg_id": format!("hermes-assign-{}", chrono::Local::now().timestamp()),
        "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    
    let queue = format!("agent-work-queue:fix:{}", fixer);
    // 去重：检查该 bug 是否已在队列中
    let existing: Vec<String> = conn.lrange(&queue, 0, -1).await.unwrap_or_default();
    let already_queued = existing.iter().any(|s| s.contains(&format!("Bug #{}", bug_id)));
    if already_queued {
        println!("⏭️   Bug #{} 已在 {} 队列中，跳过重复分派", bug_id, fixer);
    } else {
        let _: redis::RedisResult<i64> = conn.rpush(&queue, task.to_string()).await;
    }
    
    let names = [("zhaoyun", "赵云（前端）"), ("guanyu", "关羽（后端）"), ("xunyu", "荀彧（数据库）")];
    let display = names.iter().find(|(id,_)| *id == fixer).map(|(_,n)| *n).unwrap_or(fixer);
    println!("已将 Bug #{} 分派给 {}。", bug_id, display);
    Ok(())
}

/// Print agent list to stdout.
pub fn list_agents_cli() {
    println!("可用智能体：");
    println!("  诸葛亮(zhugeliang): 架构师/协调者");
    println!("  刘备(liubei): PM/项目经理");
    println!("  关羽(guanyu): 后端修复（Java/Spring/API）");
    println!("  赵云(zhaoyun): 前端修复（Vue/界面）");
    println!("  荀彧(xunyu): 数据库修复（SQL/索引/DDL）");
    println!("  张飞(zhangfei): QA 测试");
    println!("  华佗(huatuo): 产品验收");
    println!("  陈琳(chenlin): 文档归档");
}

/// Download bug attachments and analyze via LLM vision.
pub async fn analyze_bug_cli(bug_id: &str) -> anyhow::Result<()> {
    // 优先走 Rust API + Vision/OCR 链路（内置，不依赖外部脚本）
    match crate::config::Config::load() {
        Ok(cfg) => {
            let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
            match client.get_bug(bug_id).await {
                Ok(detail) => {
                    let text_prompt = detail.format_for_prompt();
                    let mut images: Vec<Vec<u8>> = Vec::new();
                    for fid in extract_file_ids(&detail.raw_steps_html) {
                        if let Ok(bytes) = download_zentao_file(&cfg, &fid).await {
                            if bytes.len() > 100 {
                                images.push(bytes);
                            }
                        }
                    }

                    if images.is_empty() {
                        println!("{}", text_prompt);
                    } else {
                        let llm = crate::core::llm::LlmClient::from_config(&cfg);
                        let system = "你是 HIS 系统的 Bug 分析专家。根据禅道截图与文本信息，输出可执行修复要点。";
                        let user = format!("以下是 Bug 信息与附件截图。请结合截图识别关键界面问题，并给出修复优先级与前端改动建议。

{}", text_prompt);
                        match llm.vision(system, &user, &images, Some(&llm.vision_model), None, Some(2048)).await {
                            Ok(ans) => println!("{}", ans),
                            Err(e) => {
                                tracing::warn!("analyze_bug_cli vision 失败，回退到文本: {}", e);
                                println!("{}", text_prompt);
                            }
                        }
                    }
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("analyze_bug_cli API 回退到 shell: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::warn!("analyze_bug_cli 加载配置失败，回退到 shell: {}", e);
        }
    }

    // 兜底：兼容已部署的外部脚本路径
    let analyze_script = format!("{}/zentao-bug-analyze.sh", ZENTAO_DIR);
    if !std::path::Path::new(&analyze_script).exists() {
        println!("分析 Bug #{} 失败：API 不可用且缺少脚本 {}", bug_id, analyze_script);
        return Ok(());
    }

    let out = Command::new("bash")
        .arg(analyze_script)
        .arg(bug_id)
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    println!("{}", stdout.trim());
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.trim().is_empty() {
            eprintln!("stderr: {}", stderr.trim());
        }
    }
    Ok(())
}


/// Parse a bug line like "1. #503 [严重] Title..." into (bug_id, title).
fn parse_bug_line_full(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if !line.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    // "1. #503 [严重] 发药明细与发药汇总单..."
    let after_hash = line.split('#').nth(1)?;
    // after_hash = "503 [严重] 发药明细..."
    let bug_id = after_hash.split(char::is_whitespace).next()?.to_string();
    // Extract title: skip "#503 " then remove "[严重] " prefix
    let after_id = after_hash.trim_start_matches(|c: char| !c.is_whitespace()).trim_start();
    // after_id = "[严重] 发药明细..."
    // Remove "[XXX] " prefix
    let title = if let Some(bracket_end) = after_id.find(']') {
        after_id[bracket_end+1..].trim().to_string()
    } else {
        after_id.to_string()
    };
    if title.is_empty() { None } else { Some((bug_id, title)) }
}

/// Result of a single pipeline fix attempt.
#[derive(Debug, Clone)]
pub struct PipelineFixResult {
    pub bug_id: String,
    pub bug_title: String,
    pub fixer: String,
    pub success: bool,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

/// Run the full pipeline: scan all active bugs → fix each one sequentially.
pub async fn pipeline_cli(max_bugs: usize, _default_fixer: &str) -> anyhow::Result<()> {
    // Step 1: Connect to Redis
    let cfg = crate::config::Config::load()?;
    let redis_url = cfg.redis_url();
    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;

    // Step 2: Refresh Zentao token
    let _ = std::process::Command::new("bash")
        .arg(format!("{}/zentao-token-refresh.sh", ZENTAO_DIR))
        .arg("zhangfei")
        .output();

    // Step 3: Scan all active bugs via Zentao API (handles ALL pages, not just first 50)
    let zentao_client = crate::core::zentao::ZentaoClient::from_config(&cfg);
    let mut all_bugs: Vec<(String, String, String)> = Vec::new();
    match zentao_client.get_all_active_bugs().await {
        Ok(bugs) => {
            for b in bugs {
                if b.id == 613 { continue; } // skip already fixed
                // 铁律 18: 跳过已解决/已关闭的 bug
                if b.status.as_deref() == Some("resolved") || b.status.as_deref() == Some("closed") {
                    println!("⏭️   Bug #{} 状态={}，跳过", b.id, b.status.as_deref().unwrap_or("?"));
                    continue;
                }
                // Categorize by area based on title/module
                let combined = format!("{:?} {:?}", b.title, b.moduleTitle).to_lowercase();
                let fixer = if combined.contains("报错") || combined.contains("保存") || combined.contains("接口") || combined.contains("sql") || combined.contains("数据") {
                    "guanyu"
                } else {
                    "zhaoyun" // frontend handles most UI/display bugs
                };
                all_bugs.push((b.id.to_string(), b.title, fixer.to_string()));
            }
        }
        Err(e) => {
            tracing::error!("Failed to scan bugs via Zentao API: {}", e);
            println!("❌ 无法扫描 Bug：{}", e);
            return Ok(());
        }
    }

    if all_bugs.is_empty() {
        println!("🔍 当前没有活跃的 Bug。");
        return Ok(());
    }

    let total = all_bugs.len().min(max_bugs);
    println!("🔍 扫描完成：共 {} 个活跃 Bug（本次处理 {} 个）", all_bugs.len(), total);
    println!("{}", "=".repeat(60));

    // Step 4: Fix each bug sequentially via Redis
    let mut results: Vec<PipelineFixResult> = Vec::new();
    for (i, (bug_id, bug_title, fixer)) in all_bugs.iter().take(max_bugs).enumerate() {
        println!("[{}/{}] 🛠️  修复 Bug #{}: {}", i + 1, total, bug_id, bug_title);
        println!("    队列: agent-work-queue:fix:zhugeliang (诸葛亮分析)");
        println!("{}", "-".repeat(40));

        let start = std::time::Instant::now();

        // 铁律：所有 Bug 先发给诸葛亮分析，再路由给修复 Agent
        let task = serde_json::json!({
            "agent_id": "zhugeliang",
            "message": format!("请分析 Bug #{} 并设计修复方案，然后路由给合适的修复 Agent 执行。\n建议修复 Agent: {}\nBug 标题: {}", bug_id, fixer, bug_title),
            "source": "pipeline_pre_analyze",
            "sender_id": "pipeline",
            "suggested_fixer": fixer,
            "chat_id": "",
            "is_dm": "true",
            "msg_id": format!("pipeline-pre-analyze-{}-{}", bug_id, chrono::Local::now().timestamp()),
            "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        });

        let queue = "agent-work-queue:fix:zhugeliang".to_string();
        // ── 铁律 18: 入列前三重检查 ──
        let (skip, reason) = crate::core::pipeline::should_skip_bug(
            bug_id, fixer, &mut conn, &zentao_client,
        ).await;
        if skip {
            println!("⏭️   Bug #{} 跳过: {}", bug_id, reason);
        } else {
            let _: redis::RedisResult<i64> = conn.rpush(&queue, task.to_string()).await;
            println!("📥  Bug #{} 已入列 → {}", bug_id, fixer);
        }

        // 轮询等待修复结果（最大等待 30 分钟）
        let result_key = format!("pipeline:result:{}", bug_id);
        let mut success = false;
        let mut elapsed_ms = 0u64;
        let mut error_msg = String::new();
        let mut changes = 0u32;
        let mut polls = 0u32;

        loop {
            polls += 1;
            if polls > 720 {  // 720 * 10s = 2小时安全上限
                tracing::warn!("Bug #{} 轮询超时（{} 小时），跳过", bug_id, polls * 10 / 3600);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let val: Option<String> = conn.get(&result_key).await.unwrap_or(None);
            if let Some(val) = val {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&val) {
                    success = parsed.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    elapsed_ms = parsed.get("elapsed_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    changes = parsed.get("changes").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    error_msg = parsed.get("error").and_then(|v| v.as_str()).unwrap_or("").to_string();
                }
                // 删除结果 key，释放空间
                let _: redis::RedisResult<()> = conn.del(&result_key).await;
                break;
            }
        }

        let elapsed = start.elapsed().as_secs();
        if success {
            println!("✅ Bug #{} 修复完成（{} 秒，{} 个文件变更）", bug_id, elapsed, changes);
        } else if error_msg.is_empty() {
            println!("⏰ Bug #{} 超时（{} 秒，超过 2 小时限制）", bug_id, elapsed);
            error_msg = "Pipeline 轮询超时（2 小时）".to_string();
        } else {
            println!("❌ Bug #{} 修复失败（{} 秒）", bug_id, elapsed);
        }

        results.push(PipelineFixResult {
            bug_id: bug_id.clone(),
            bug_title: bug_title.clone(),
            fixer: fixer.clone(),
            success,
            elapsed_ms,
            error: if success { None } else { Some(error_msg) },
        });

        println!("{}", "=".repeat(60));
    }

    // Step 5: Summary
    let success_count = results.iter().filter(|r| r.success).count();
    let fail_count = results.iter().filter(|r| !r.success).count();
    println!("📊 执行汇总");
    println!("  总数: {} / 成功: {} / 失败: {}", results.len(), success_count, fail_count);
    if fail_count > 0 {
        println!("
❌ 失败的 Bug:");
        for r in results.iter().filter(|r| !r.success) {
            println!("  Bug #{} [{}]: {}", r.bug_id, r.fixer, r.bug_title);
            if let Some(ref err) = r.error {
                println!("    原因: {}", err);
            }
        }
    }
    if success_count > 0 {
        println!("
✅ 成功的 Bug:");
        for r in results.iter().filter(|r| r.success) {
            println!("  Bug #{} [{}]: {} ({}ms)", r.bug_id, r.fixer, r.bug_title, r.elapsed_ms);
        }
    }

    // L5: batch 结束后自动运行自优化
    if !results.is_empty() {
        println!("\n🔧 L5: 运行自优化分析...");
        match tokio::process::Command::new("agentforge").arg("optimize").output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // 只打印最后几行摘要
                let summary: String = stdout.lines().rev().take(5).collect::<Vec<_>>().join("\n");
                if !summary.is_empty() { println!("{}", summary); }
            }
            Err(e) => tracing::warn!("L5 optimize failed: {}", e),
        }
    }

    Ok(())
}


#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bug_line() {
        assert_eq!(parse_bug_line("  1. #455 [一般] 测试标题"), Some("455".into()));
        assert_eq!(parse_bug_line("No bug here"), None);
    }

    #[test]
    fn test_route_bug() {
        assert_eq!(route_bug("前端vue界面显示异常"), "zhaoyun");
        assert_eq!(route_bug("后端api接口报500错误"), "guanyu");
        assert_eq!(route_bug("数据库查询慢性能优化"), "xunyu");
        assert_eq!(route_bug("前端vue组件渲染问题"), "zhaoyun");
    }
}


async fn download_zentao_file(cfg: &crate::config::Config, fid: &str) -> anyhow::Result<Vec<u8>> {
    let token = load_zentao_token(&cfg.zentao.token_file);
    let url = format!("{}/api.php/v1/files/{}", cfg.zentao.base_url.trim_end_matches('/'), fid);
    let resp = reqwest::Client::new()
        .get(&url)
        .header("Token", token)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("Zentao file API error: HTTP {}", resp.status());
    }
    Ok(resp.bytes().await?.to_vec())
}

fn extract_file_ids(steps: &str) -> Vec<String> {
    let mut file_ids = Vec::new();
    let mut pos = 0;
    while let Some(idx) = steps[pos..].find("fileID=") {
        let start = pos + idx + 7;
        let mut end = start;
        while end < steps.len() && steps.as_bytes()[end].is_ascii_digit() {
            end += 1;
        }
        if end > start {
            file_ids.push(steps[start..end].to_string());
        }
        pos = end;
    }
    file_ids
}

fn load_zentao_token(path: &std::path::Path) -> String {
    if let Ok(content) = std::fs::read_to_string(path) {
        for line in content.lines() {
            if let Some(v) = line.strip_prefix("ZENTAO_TOKEN=") {
                return v.trim().to_string();
            }
        }
    }
    if let Ok(content) = std::fs::read_to_string("/root/.config/zentao/.env") {
        for line in content.lines() {
            if let Some(v) = line.strip_prefix("ZENTAO_TOKEN=") {
                return v.trim().to_string();
            }
        }
    }
    String::new()
}
