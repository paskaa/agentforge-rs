//! Coordinator — scans all agent bugs and dispatches them to fixer queues.
//!
//! Runs every 5 minutes under zhugeliang (架构师).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use redis::AsyncCommands;
use serde_json;

use crate::core::pipeline::HUMAN_ACCOUNTS;

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
                  "退回", "审计", "完诊", "div_log", "SQL", "执行科室", "库存",
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

    /// Scan all agent bugs and dispatch to fixer queues.
    ///
    /// Returns count of dispatched bugs.
    pub async fn scan_and_dispatch(
        &mut self,
        redis: &mut redis::aio::MultiplexedConnection,
        min_interval: u64,
    ) -> u64 {
        let now = Instant::now();
        if now.duration_since(self.last_scan).as_secs() < min_interval {
            return 0;
        }
        self.last_scan = now;

        tracing::info!("[coordinator] Scanning all agent bugs...");

        let mut total = 0u64;
        let mut liubei_bugs: Vec<(String, String)> = vec![];

        for account in ALL_AGENTS {
            self.refresh_token();
            let script = self.zentao_dir.join("zentao-my-bugs.sh");
            let output = Command::new("bash")
                .arg(&script)
                .arg(account)
                .arg("active")
                .output();

            let stdout = match output {
                Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
                Err(_) => continue,
            };

            // Skip if no bugs
            if stdout.contains("没有未解决的 Bug") {
                continue;
            }

            // Parse: "  1. #455 [一般] Title..."
            for line in stdout.lines() {
                if let Some(bid) = parse_bug_line(line) {
                    let title = line.split(']').nth(1).unwrap_or("").trim().to_string();
                    if *account == "liubei" {
                        liubei_bugs.push((bid, title));
                    } else {
                        // Route to fixer
                        let fixer = route_bug(&title);
                        let msg = serde_json::json!({
                            "agent_id": fixer,
                            "message": format!("请修复 Bug #{}：{}", bid, title),
                            "source": "coordinator_scan",
                            "sender_id": "coordinator",
                            "chat_id": "",
                            "is_dm": "true",
                            "msg_id": format!("coord-{}-{}", bid, chrono::Utc::now().timestamp()),
                            "timestamp": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                        });

                        let queue = format!("agent-work-queue:fix:{}", fixer);
                        let _: redis::RedisResult<i64> = redis
                            .rpush(&queue, msg.to_string())
                            .await;

                        total += 1;
                    }
                }
            }
        }

        // Batch PM analysis: send all liubei bugs at once
        if !liubei_bugs.is_empty() {
            let bug_lines: Vec<String> = liubei_bugs
                .iter()
                .map(|(bid, title)| format!("  #{}：{}", bid, title))
                .collect();
            let msg = serde_json::json!({
                "agent_id": "liubei",
                "message": format!("请分析并分派以下 {} 个 Bug：\n{}", liubei_bugs.len(), bug_lines.join("\n")),
                "source": "pm_analyze",
                "sender_id": "coordinator",
                "chat_id": "",
                "is_dm": "true",
                "msg_id": format!("pm-batch-{}", chrono::Utc::now().timestamp()),
                "timestamp": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            });

            let _: redis::RedisResult<i64> = redis
                .rpush("agent-work-queue", msg.to_string())
                .await;
            total += liubei_bugs.len() as u64;
        }

        tracing::info!("[coordinator] Dispatched {} bugs", total);
        total
    }
}

/// Parse bug ID from zentao-my-bugs output line.
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

/// Scan all agent bugs and print summary to stdout.
pub async fn scan_bugs_cli() -> anyhow::Result<()> {
    let agents = ["zhugeliang", "liubei", "guanyu", "zhaoyun", "xunyu", "zhangfei", "huatuo", "chenlin"];
    
    // Refresh token
    let _ = Command::new("bash")
        .arg(format!("{}/zentao-token-refresh.sh", ZENTAO_DIR))
        .arg("zhangfei")
        .output();
    
    let mut found = false;
    for agent in &agents {
        let Ok(out) = Command::new("bash")
            .arg(format!("{}/zentao-my-bugs.sh", ZENTAO_DIR))
            .arg(agent)
            .arg("active")
            .output()
        else { continue; };
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !stdout.contains("没有未解决的 Bug") && !stdout.trim().is_empty() {
            println!("【{}】\n{}", agent, stdout.trim());
            found = true;
        }
    }
    if !found {
        println!("当前没有活跃的 Bug。");
    }
    Ok(())
}

/// Query a single bug and print detail to stdout.
pub async fn query_bug_cli(bug_id: &str) -> anyhow::Result<()> {
    let Ok(out) = Command::new("bash")
        .arg(format!("{}/zentao-bug-detail.sh", ZENTAO_DIR))
        .arg(bug_id)
        .output()
    else {
        println!("查询 Bug #{} 失败", bug_id);
        return Ok(());
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.trim().is_empty() {
        println!("Bug #{} 查询结果为空。", bug_id);
    } else {
        println!("{}", stdout.trim());
    }
    Ok(())
}

/// Submit a fix task to the Redis queue and print ack to stdout.
pub async fn submit_fix_cli(bug_id: &str, bug_title: &str, fixer: &str) -> anyhow::Result<()> {
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    
    let task = serde_json::json!({
        "agent_id": fixer,
        "message": format!("请修复 Bug #{}：{}", bug_id, bug_title),
        "source": "hermes_action",
        "sender_id": "hermes",
        "chat_id": "",
        "is_dm": "true",
        "msg_id": format!("hermes-fix-{}", chrono::Utc::now().timestamp()),
        "timestamp": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    
    let queue = format!("agent-work-queue:fix:{}", fixer);
    let _: redis::RedisResult<i64> = conn.rpush(&queue, task.to_string()).await;
    
    println!("已提交 Bug #{} 的修复任务给 {}。修复由 Claude Code 异步执行。", bug_id, fixer);
    Ok(())
}

/// Assign a bug to a specific fixer and print ack.
pub async fn assign_bug_cli(bug_id: &str, fixer: &str) -> anyhow::Result<()> {
    let valid_fixers = ["zhaoyun", "guanyu", "xunyu"];
    if !valid_fixers.contains(&fixer) {
        println!("无效的修复智能体：{}。可选：zhaoyun(前端), guanyu(后端), xunyu(数据库)", fixer);
        return Ok(());
    }
    
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    
    let task = serde_json::json!({
        "agent_id": fixer,
        "message": format!("请修复 Bug #{}", bug_id),
        "source": "hermes_assign",
        "sender_id": "hermes",
        "chat_id": "",
        "is_dm": "true",
        "msg_id": format!("hermes-assign-{}", chrono::Utc::now().timestamp()),
        "timestamp": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    
    let queue = format!("agent-work-queue:fix:{}", fixer);
    let _: redis::RedisResult<i64> = conn.rpush(&queue, task.to_string()).await;
    
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
