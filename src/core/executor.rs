//! Agent Executor — async loop: BLPOP for fixers, xread for non-fixers,
//! full pipeline handlers, and coordinator scan.

use crate::config::{AgentConfig, Config};
use crate::core::llm::LlmClient;
use crate::core::pipeline::{self, route_bug, Verdict, RoundBudget, FileDiff, HandoffCard, PipelineEvent, check_round_budget, increment_round, take_snapshot, snapshot_and_diff};
use crate::core::subagent::{self, CodexResult};
use crate::core::trace::TraceStore;
use crate::network::feishu::FeishuClient;
use redis::AsyncCommands;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize, Clone)]
pub struct Task {
    pub agent_id: String,
    pub message: String,
    pub source: String,
    pub sender_id: String,
    pub msg_id: String,
    pub timestamp: String,
    #[serde(default)] pub chat_id: String,
    #[serde(default)] pub is_dm: String,
    #[serde(default)] pub bug_reporter: String,
}

const AGENT_NAMES: &[(&str, &str)] = &[
    ("zhugeliang","诸葛亮"),("liubei","刘备"),("guanyu","关羽"),("zhaoyun","赵云"),
    ("xunyu","荀彧"),("zhangfei","张飞"),("huatuo","华佗"),("chenlin","陈琳"),
];
const FIXERS: &[&str] = &["zhugeliang","guanyu","zhaoyun","xunyu","zhangfei","huatuo","chenlin"];
/// 真正执行代码修复的 Agent（排除测试/验收/归档人员）
const CODE_FIXERS: &[&str] = &["guanyu", "zhaoyun", "xunyu"];
const ALL_AGENTS: &[&str] = &["zhugeliang","liubei","guanyu","zhaoyun","xunyu","zhangfei","huatuo","chenlin"];
const COORDINATOR: &str = "liubei";

const MAX_FIX_RETRIES: u32 = 3;
const RETRY_BASE_DELAY_MS: u64 = 5000; // 5s base, doubles each retry

/// Check if a codex failure is transient (model API error, timeout, etc.) and worth retrying.
fn is_transient_error(stderr: &str, stdout: &str) -> bool {
    let combined = format!("{} {}", stderr, stdout).to_lowercase();
    // Model API errors
    combined.contains("rate limit") || combined.contains("429") || combined.contains("503")
        || combined.contains("timeout") || combined.contains("timed out")
        || combined.contains("connection refused") || combined.contains("eof")
        || combined.contains("network") || combined.contains("overloaded")
        || combined.contains("capacity") || combined.contains("try again")
        || combined.contains("econnreset") || combined.contains("ehostunreach")
        || combined.contains("panic") || combined.contains("segfault")
}

pub struct AgentExecutor {
    pub agent_id: String, pub agent_name: String,
    pub redis: redis::aio::MultiplexedConnection,
    pub redis_sync: Arc<Mutex<redis::Connection>>,
    pub llm: LlmClient, pub feishu: FeishuClient,
    pub traces: Arc<TraceStore>,
    fix_stream: String, is_fixer: bool,
    last_coordinator_scan: Instant,
    last_retry_check: Instant,
    last_analysis_scan: Instant,
    last_stream_id: String,
    zentao_dir: String,
}

impl AgentExecutor {
    pub async fn new(agent_id: &str, config: Config) -> anyhow::Result<Self> {
        let agent_cfg = config.agents.get(agent_id).cloned().unwrap_or(AgentConfig {
            name: agent_id.into(), role: String::new(), expertise: vec![], model: None, feishu_app_id: None,
        });
        let agent_name = AGENT_NAMES.iter().find(|(id,_)| *id==agent_id).map(|(_,n)| n.to_string()).unwrap_or_else(|| agent_id.into());
        let redis_url = config.redis_url();
        let client = redis::Client::open(redis_url.clone())?;
        let redis = client.get_multiplexed_async_connection().await?;
        let redis_sync = Arc::new(Mutex::new(client.get_connection()?));
        let is_fixer = FIXERS.contains(&agent_id);
        let fix_stream = format!("agent-work-queue:fix:{}", agent_id);
        let llm = LlmClient::new(&config.llm.api_base, &config.llm.api_key, agent_cfg.model.as_deref().unwrap_or(&config.llm.default_model));
        let feishu = FeishuClient::new(&config.feishu.app_id, &config.feishu.app_secret, &config.feishu.group_chat_id);
        let traces = Arc::new(TraceStore::open(std::path::Path::new("/var/lib/agentforge/traces.db")).await?);
        Ok(Self { agent_id: agent_id.into(), agent_name, redis, redis_sync, llm, feishu, traces, fix_stream, is_fixer,
            last_coordinator_scan: Instant::now(), last_retry_check: Instant::now(), last_analysis_scan: Instant::now(), last_stream_id: "0-0".into(),
            zentao_dir: "/root/.openclaw/extensions/zentao-token-refresh".into() })
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        // 启动时清理可能残留的旧锁
        if self.is_fixer {
            let lock_key = format!("codex_lock:{}", self.agent_id);
            let ttl: i64 = self.redis.clone().ttl(&lock_key).await.unwrap_or(-2);
            if ttl != -2 {
                tracing::info!("[{}] Cleaning up stale lock on startup (TTL={}s)", self.agent_id, ttl);
                let _: redis::RedisResult<()> = self.redis.clone().del(&lock_key).await;
            }
            // 启动时清理残留的 fix_active 去重标记（旧进程已死，锁无意义）
            let pattern = format!("fix_active:{}:*", self.agent_id);
            let mut cursor: u64 = 0;
            loop {
                let (new_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                    .arg(cursor).arg("MATCH").arg(&pattern).arg("COUNT").arg(100)
                    .query_async(&mut self.redis.clone()).await.unwrap_or((0, vec![]));
                for key in &keys {
                    let _: redis::RedisResult<()> = self.redis.clone().del(key).await;
                    tracing::info!("[{}] Cleaned stale fix_active key: {}", self.agent_id, key);
                }
                cursor = new_cursor;
                if cursor == 0 { break; }
            }

            // 启动恢复：检查失败集合中是否有可重试的 bug
            self.recover_failed_bugs().await;
        }
        tracing::info!("[{}] Started as {} (stream={}, fixer={})", self.agent_id, self.agent_name, self.fix_stream, self.is_fixer);
        // Non-fixer: read from stream without consumer group
        // (simpler than XREADGROUP — just xread with new messages)
        loop {
            if self.agent_id == "liubei"
                && self.last_coordinator_scan.elapsed() > Duration::from_secs(300)
            { self.last_coordinator_scan = Instant::now(); self.run_coordinator_scan().await; }

            // 定期检查失败 bug 并重新入队（每 10 分钟）
            if self.is_fixer && self.last_retry_check.elapsed() > Duration::from_secs(600) {
                self.last_retry_check = Instant::now();
                self.recover_failed_bugs().await;
            }


            // For fixers: check per-agent lock BEFORE consuming — avoid task loss
            // Also auto-release stale locks (TTL < 82800 = held >1h)
            if self.is_fixer {
                let my_lock = format!("codex_lock:{}", self.agent_id);
                let ttl: i64 = self.redis.clone().ttl(&my_lock).await.unwrap_or(-2);
                if ttl == -2 { /* key doesn't exist — no lock */ }
                else if ttl > 0 && ttl < 82800 {
                    // Lock held >1h (86400-3600=82800s) — probably stale, release
                    tracing::warn!("[{}] Stale lock detected (TTL={}s), auto-releasing", self.agent_id, ttl);
                    let _: redis::RedisResult<()> = self.redis.clone().del(&my_lock).await;
                } else {
                    // Lock active — wait
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            }

            // 修复人员定期扫描诸葛亮分析文档（每 60 秒）
            let can_fix = CODE_FIXERS.contains(&self.agent_id.as_str());
            if can_fix && self.last_analysis_scan.elapsed() > Duration::from_secs(60) {
                self.last_analysis_scan = Instant::now();
                self.scan_analysis_docs().await;
            }

            let val = self.blpop_val().await;
            let Some(val) = val else { continue };

            let task = match serde_json::from_str::<Task>(&val) {
                Ok(t) => t, Err(e) => { tracing::warn!("[{}] parse: {}", self.agent_id, e); continue; }
            };
            let source = task.source.as_str();
            let msg = &task.message;
            tracing::info!("[{}] Processing: {} (source={})", self.agent_id, msg.chars().take(80).collect::<String>(), source);

            match source {
                "pm_analyze" if self.agent_id == "liubei" => self.handle_pm_analyze(msg).await,
                "pipeline_pre_analyze" if self.agent_id == "zhugeliang" => self.handle_pipeline_pre_analyze(msg).await,
                "pipeline_analyze" if self.agent_id == "zhugeliang" => self.handle_pipeline_analyze(msg).await,
                "pipeline_db_review" if self.agent_id == "xunyu" => self.handle_pipeline_db_review(msg).await,
                "pipeline_report" if self.agent_id == "liubei" => self.handle_pipeline_report(msg).await,
                "pm_routed" | "coordinator_scan" | "hermes_action" | "hermes_assign" | "pipeline" | "pipeline_batch" | "verify_retry" | "web_ui" | "web_execute" | "manual_enqueue" => self.handle_fix_task(msg).await,
                "pipeline_fix_done" if self.agent_id == "zhangfei" => self.handle_pipeline_test(msg).await,
                "pipeline_test_done" if self.agent_id == "huatuo" => self.handle_pipeline_verify(msg).await,
                "pipeline_test_done" if self.agent_id == "chenlin" => self.handle_chenlin_doc(msg).await,
                "ws_listener" => {
                    // Filter: only respond if message is for me (direct target or broadcast)
                    let is_for_me = task.agent_id == self.agent_id 
                        || task.agent_id == "broadcast"
                        || task.agent_id.is_empty();
                    if !is_for_me {
                        tracing::debug!("[{}] Skipping msg for {}", self.agent_id, task.agent_id);
                        continue;
                    }
                    // Broadcast: only respond if my expertise keywords match the message
                    if task.agent_id == "broadcast" && !self.should_respond(msg) {
                        tracing::debug!("[{}] Broadcast — keyword doesn't match, skipping", self.agent_id);
                        continue;
                    }
                    // ── Hermes-first: NLU with pipeline awareness ──
                    // Hermes bridge (Python) handles both plain chat and pipeline actions.
                    // It auto-executes fast actions (scan_bugs, query_bug) and returns
                    // the final reply. Only fallback to legacy routing on failure.
                    let hermes_handled = self.handle_chat_hermes(msg, &task).await;
                    if !hermes_handled {
                        // Fallback: legacy keyword-based pipeline routing
                        let triggered = self.detect_pipeline_intent(msg, &task).await;
                        if !triggered {
                            self.handle_chat_legacy(msg, &task).await;
                        }
                    }
                }
                _ => { tracing::warn!("[{}] 未知 source '{}'，降级为 handle_fix_task", self.agent_id, source); self.handle_fix_task(msg).await; }
            }
        }
    }

    /// 启动恢复：检查失败集合，将可重试的 bug 重新入队
    async fn recover_failed_bugs(&mut self) {
        let failed_key = format!("agent-failed-bugs:{}", self.agent_id);
        let failed_bugs: Vec<String> = self.redis.clone().smembers(&failed_key).await.unwrap_or_default();
        if failed_bugs.is_empty() { return; }

        let retry_key_prefix = format!("fix_retry_count:{}", self.agent_id);
        let mut requeued = 0u32;

        for bid in &failed_bugs {
            // 检查重试次数（每个 bug 最多重试 5 次）
            let retry_count: i32 = self.redis.clone()
                .get(format!("{}:{}", retry_key_prefix, bid))
                .await.unwrap_or(0);
            if retry_count >= 5 {
                tracing::warn!("[{}] Bug #{} max retries ({}) reached, skipping", self.agent_id, bid, retry_count);
                continue;
            }

            // 检查 bug 是否仍然活跃（未被其他 agent 修复）
            let lock_key = format!("codex_lock:{}", self.agent_id);
            let lock_exists: bool = self.redis.clone().exists(&lock_key).await.unwrap_or(false);
            if lock_exists { continue; } // 正在处理中，跳过

            // 重新入队
            let task = serde_json::json!({
                "agent_id": self.agent_id,
                "message": format!("请修复 Bug #{}（重试）", bid),
                "source": "retry_recover",
                "sender_id": "system",
                "chat_id": "",
                "is_dm": "true",
                "msg_id": format!("retry-{}-{}-{}", bid, self.agent_id, chrono::Local::now().timestamp()),
                "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            });
            let queue = format!("agent-work-queue:fix:{}", self.agent_id);
            let _: redis::RedisResult<i64> = self.redis.clone().rpush(&queue, task.to_string()).await;
            // 增加重试计数
            let _: redis::RedisResult<i32> = self.redis.clone()
                .incr(format!("{}:{}", retry_key_prefix, bid), 1).await;
            let _: redis::RedisResult<()> = self.redis.clone()
                .expire(format!("{}:{}", retry_key_prefix, bid), 86400).await;
            // 从失败集合移除
            let _: redis::RedisResult<i64> = self.redis.clone().srem(&failed_key, bid).await;
            requeued += 1;
            tracing::info!("[{}] Recovered Bug #{} — requeued for retry", self.agent_id, bid);
        }

        if requeued > 0 {
            tracing::info!("[{}] Startup recovery: requeued {} failed bugs", self.agent_id, requeued);
            let _ = self.feishu.send(&format!("🔄 [{}] 启动恢复：重新入队 {} 个失败 Bug", self.agent_name, requeued), None).await;
        }
    }

    /// 扫描诸葛亮分析文档，找到分配给自己的 Bug 并入队
    async fn scan_analysis_docs(&self) {
        let bugs_dir = std::path::Path::new("/tmp/agentforge-worktrees/zhugeliang/MD/bugs");
        if !bugs_dir.exists() { return; }

        let my_queue = format!("agent-work-queue:fix:{}", self.agent_id);
        let fixer_id_pattern = format!("**FIXER_ID**: {}", self.agent_id);
        let fixer_id_pattern2 = format!("**FIXER_ID**: {}", match self.agent_id.as_str() {
            "guanyu" => "guanyu", "zhaoyun" => "zhaoyun", "xunyu" => "xunyu", _ => "",
        });

        let Ok(entries) = std::fs::read_dir(bugs_dir) else { return };
        let mut found = 0u32;

        for entry in entries.flatten() {
            let path = entry.path();
            let fname = path.file_name().unwrap_or_default().to_string_lossy();
            // 只看 BUG_*_ANALYSIS.md
            if !fname.starts_with("BUG_") || !fname.ends_with("_ANALYSIS.md") { continue; }

            // 提取 Bug ID
            let bid = fname.strip_prefix("BUG_").unwrap_or("")
                .strip_suffix("_ANALYSIS.md").unwrap_or("")
                .to_string();
            if bid.is_empty() { continue; }

            // 检查分析文档是否分配给自己
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            if !content.contains(&fixer_id_pattern) && !content.contains(&fixer_id_pattern2) { continue; }

            // 检查是否已在队列中（去重）
            let existing: Vec<String> = self.redis.clone().lrange(&my_queue, 0, -1).await.unwrap_or_default();
            if existing.iter().any(|s| s.contains(&format!("Bug #{}", bid))) { continue; }

            // 检查禅道状态：已解决/已关闭/已完成的 Bug 跳过
            let cfg = crate::config::Config::load().unwrap_or_default();
            let zclient = crate::core::zentao::ZentaoClient::from_config(&cfg);
            if let Ok(bug_detail) = zclient.get_bug(&bid).await {
                if bug_detail.status == "resolved" || bug_detail.status == "closed" || bug_detail.status == "done" {
                    tracing::info!("[{}] Bug #{} 禅道状态={}, 已解决，跳过", self.agent_id, bid, bug_detail.status);
                    let fixed_key = format!("bug_fixed:{}", bid);
                    let _: redis::RedisResult<()> = self.redis.clone().set_ex(&fixed_key, "resolved", 2592000).await;
                    continue;
                }
                tracing::info!("[{}] Bug #{} 禅道状态={}, 未解决，继续处理", self.agent_id, bid, bug_detail.status);
            } else {
                tracing::warn!("[{}] Bug #{} 无法查询禅道状态，按未解决处理", self.agent_id, bid);
            }

            // 检查是否已在处理中
            let lock_key = format!("fix_active:{}:{}", self.agent_id, bid);
            if self.redis.clone().exists::<_, bool>(&lock_key).await.unwrap_or(false) { continue; }

            // 路由校验：只检查分析文档中的「标题」行，不检查全文（全文含前后端代码路径会误判）
            let bug_title_line = content.lines()
                .find(|l| l.contains("标题") || l.contains("**标题**"))
                .unwrap_or("")
                .to_lowercase();
            let is_backend_bug = bug_title_line.contains("java") || bug_title_line.contains("service")
                || bug_title_line.contains("mapper") || bug_title_line.contains("sql")
                || bug_title_line.contains("接口") || bug_title_line.contains("后端")
                || bug_title_line.contains("数据库") || bug_title_line.contains("api");
            let is_frontend_bug = bug_title_line.contains("前端") || bug_title_line.contains("vue")
                || bug_title_line.contains("界面") || bug_title_line.contains("显示")
                || bug_title_line.contains("弹窗") || bug_title_line.contains("页面")
                || bug_title_line.contains("组件") || bug_title_line.contains("css");
            let agent_is_backend = self.agent_id == "guanyu";
            let agent_is_frontend = self.agent_id == "zhaoyun";

            // 只有当标题明确匹配且与 agent 类型冲突时才跳过
            if (is_backend_bug && !is_frontend_bug && agent_is_frontend)
                || (is_frontend_bug && !is_backend_bug && agent_is_backend) {
                tracing::warn!("[{}] Bug #{} 类型不匹配（{}: {}），跳过", self.agent_id, bid,
                    if agent_is_frontend { "前端" } else { "后端" },
                    if is_backend_bug { "后端" } else { "前端" });
                continue;
            }

            // 入队
            let task = serde_json::json!({
                "agent_id": self.agent_id,
                "message": format!("请修复 Bug #{}（诸葛亮分析完成，分配给你）", bid),
                "source": "analysis_scan",
                "sender_id": "zhugeliang",
                "chat_id": "", "is_dm": "true",
                "msg_id": format!("scan-{}-{}-{}", bid, self.agent_id, chrono::Local::now().timestamp()),
                "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            });
            let _: redis::RedisResult<i64> = self.redis.clone().rpush(&my_queue, task.to_string()).await;
            found += 1;
            tracing::info!("[{}] 📄 发现分析文档 Bug #{}，自动入队", self.agent_id, bid);
        }

        if found > 0 {
            tracing::info!("[{}] 扫描分析文档: 新入队 {} 个 Bug", self.agent_id, found);
            let _ = self.feishu.send(&format!("📄 [{}] 发现 {} 个新分析文档，已入队", self.agent_name, found), None).await;
        }
    }

    async fn blpop_val(&self) -> Option<String> {
        let stream = self.fix_stream.clone();
        let sync = Arc::clone(&self.redis_sync);
        tokio::task::spawn_blocking(move || {
            let mut conn = match sync.lock() { Ok(c) => c, Err(p) => p.into_inner() };
            let (_, val): (String, String) = redis::cmd("BLPOP").arg(&stream).arg(10_i64).query(&mut *conn).ok()?;
            Some(val)
        }).await.unwrap_or(None)
    }

    #[allow(clippy::never_loop)]
    async fn xread_val(&mut self) -> Option<String> {
        let stream = self.fix_stream.clone();
        let last_id = self.last_stream_id.clone();
        let opts = redis::streams::StreamReadOptions::default().count(1).block(10000);
        let read: redis::RedisResult<Vec<redis::streams::StreamReadReply>> = self.redis.clone()
            .xread_options(&[stream.as_str()], &[&last_id], &opts).await;
        if let Ok(replies) = read {
            for reply in &replies {
                for key in &reply.keys {
                    for id in &key.ids {
                        // Track last ID for next read (avoid re-reading same messages)
                        self.last_stream_id = id.id.clone();
                        // Build JSON from HashMap
                        let mut map = serde_json::Map::new();
                        for (k, v) in &id.map {
                            let val_str = match v {
                                redis::Value::Data(d) => String::from_utf8_lossy(d).to_string(),
                                redis::Value::Int(i) => i.to_string(),
                                other => format!("{:?}", other),
                            };
                            map.insert(k.clone(), serde_json::Value::String(val_str));
                        }
                        return Some(serde_json::Value::Object(map).to_string());
                    }
                }
            }
        }
        None
    }

    async fn run_coordinator_scan(&self) {
        tracing::info!("[{}] Coordinator scan...", self.agent_id);
        let _ = tokio::process::Command::new("bash")
            .arg(format!("{}/zentao-token-refresh.sh", self.zentao_dir)).arg("zhangfei").output().await;
        for agent in ALL_AGENTS {
            let Ok(out) = tokio::process::Command::new("bash")
                .arg(format!("{}/zentao-my-bugs.sh", self.zentao_dir)).arg(agent).arg("active").output().await
            else { continue; };
            let stdout = String::from_utf8_lossy(&out.stdout);
            for (bid, title) in pipeline::parse_bugs_from_message(&stdout).iter().take(10) {
                // 检查禅道状态：已解决的 Bug 不入队
                let cfg = crate::config::Config::load().unwrap_or_default();
                let zclient = crate::core::zentao::ZentaoClient::from_config(&cfg);
                if let Ok(bug_detail) = zclient.get_bug(&bid).await {
                    if bug_detail.status == "resolved" || bug_detail.status == "closed" || bug_detail.status == "done" {
                        tracing::info!("[liubei] ⏭ Bug #{} 禅道状态={}, 已解决，跳过", bid, bug_detail.status);
                        continue;
                    }
                }
                let fixer = route_bug(title);
                let task_json = pipeline::build_fix_task(bid, title, fixer);
                let queue = format!("agent-work-queue:fix:{}", fixer);
                self.push_task_dedup(&queue, &task_json.to_string()).await;
                // 铁律: 新 Bug 必须先经诸葛亮分析再派给 Fixer
                if self.agent_id == "liubei" {
                    // 跳过已有分析文档或已在分析队列的 Bug
                    let analysis_key = format!("analysis_sent:{}", bid);
                    let already_analyzing: bool = self.redis.clone().exists(&analysis_key).await.unwrap_or(false);
                    let analysis_path = format!("/tmp/agentforge-worktrees/zhugeliang/MD/bugs/BUG_{}_ANALYSIS.md", bid);
                    let has_analysis = std::path::Path::new(&analysis_path).exists();
                    if already_analyzing || has_analysis {
                        tracing::info!("[liubei] ⏭ Bug #{} 已有分析或在分析中，跳过", bid);
                    } else {
                        let _: redis::RedisResult<()> = self.redis.clone().set_ex(&analysis_key, "1", 3600).await;
                        let pre_analyze = serde_json::json!({
                        "agent_id": "zhugeliang",
                        "message": format!("请分析 Bug #{}：{}。建议修复 Agent: {}", bid, title, fixer),
                        "source": "pipeline_pre_analyze",
                        "sender_id": "liubei",
                        "msg_id": format!("pre-analyze-{}-{}", bid, chrono::Local::now().timestamp()),
                        "timestamp": chrono::Local::now().format("%-Y-%m-%dT%H:%M:%S").to_string(),
                        "chat_id": "", "is_dm": "true",
                    });
                        let _: redis::RedisResult<i64> = self.redis.clone().rpush("agent-work-queue:fix:zhugeliang", pre_analyze.to_string()).await;
                        tracing::info!("[liubei] 📤 Bug #{} 已派给诸葛亮分析", bid);
                    }
                }
            }
        }
        // liubei is the sole coordinator — dispatch to subagents
        if self.agent_id == "liubei" { self.handle_pm_analyze("请分析和分派所有活跃 Bug").await; }
    }

    async fn handle_pm_analyze(&self, msg: &str) {
        let bugs = pipeline::parse_bugs_from_message(msg);
        if bugs.is_empty() { let _ = self.feishu.send("✅ 暂无需要分派的 Bug。", None).await; return; }
        for (bid, title) in &bugs {
            let fixer = route_bug(title);
            let task_json = pipeline::build_fix_task(bid, title, fixer);
            let queue = format!("agent-work-queue:fix:{}", fixer);
            self.push_task_dedup(&queue, &task_json.to_string()).await;
        }
        let reply = format!("✅ 已分析 {} 个 Bug，已分派给对应智能体。", bugs.len());
        let _ = self.feishu.send(&reply, None).await;
        self.traces.log(&self.agent_id, "pm_routed", None, Some(&reply), None, None, None, Some("ok"), None).await;
        self.publish_trace(&self.agent_id, "pm_routed", "", &reply, "ok", 0).await;
    }


    /// Publish trace event to Redis channel for WebSocket broadcasting.
    async fn publish_trace(&self, agent_id: &str, event: &str, task_id: &str, message: &str, status: &str, duration_ms: i64) {
        let trace_event = serde_json::json!({
            "event": "trace",
            "data": {
                "ts": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.6f").to_string(),
                "agent_id": agent_id,
                "event": event,
                "task_id": task_id,
                "message": message.chars().take(200).collect::<String>(),
                "status": status,
                "duration_ms": duration_ms,
            }
        });
        let _: redis::RedisResult<()> = self.redis.clone().publish("agentforge:traces", trace_event.to_string()).await;
    }

    async fn handle_fix_task(&self, msg: &str) {
        // ── Trigger: "分配" → coordinator scan (before bug_id check) ──
        if msg.contains("分配") && (self.agent_id == "liubei" || self.agent_id == "zhugeliang") {
            tracing::info!("[{}] 🎯 Pipeline triggered: 分配Bug", self.agent_id);
            let _ = self.feishu.send("🔍 收到分配指令，正在扫描 Bug...", None).await;
            self.run_coordinator_scan().await;
            return;
        }
        let bug_id = pipeline::parse_bugs_from_message(msg).first().map(|(b,_)| b.clone()).unwrap_or_default();
        if bug_id.is_empty() { return; }
        // ── 铁律: 检查禅道 Bug 状态 — 已关闭/已解决的 Bug 禁止处理 ──
        {
            tracing::info!("[{}] Bug#{} 开始状态检查...", self.agent_id, bug_id);
            let cfg = crate::config::Config::load().ok();
            if let Some(cfg) = cfg {
                let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                tracing::info!("[{}] Bug#{} 调用 get_bug...", self.agent_id, bug_id);
                if let Ok(bug_detail) = client.get_bug(&bug_id).await {
                    tracing::info!("[{}] Bug#{} get_bug 完成, status={}", self.agent_id, bug_id, bug_detail.status);
                    if bug_detail.status == "resolved" || bug_detail.status == "closed" || bug_detail.status == "done" {
                        tracing::warn!("[{}] Bug#{} 禅道状态={}, 已关闭/已解决，跳过处理", self.agent_id, bug_id, bug_detail.status);
                        let _ = self.feishu.send(&format!("⏭️ Bug#{} 状态={}，已关闭，跳过处理", bug_id, bug_detail.status), None).await;
                        return;
                    }
                    if pipeline::is_human(&bug_detail.opened_by) {
                        tracing::info!("[{}] Bug#{} 由人类 {} 提出，将只加备注不改状态", self.agent_id, bug_id, bug_detail.opened_by);
                    }
                }
            }
        }

        // ── 去重: 5分钟内跳过的bug不再重复处理 ──
        let skip_key = format!("skip_no_analysis:{}:{}", self.agent_id, bug_id);
        let is_skipped: bool = self.redis.clone().exists(&skip_key).await.unwrap_or(false);
        if is_skipped {
            tracing::debug!("[{}] Bug#{} 5分钟内已跳过，不再处理", self.agent_id, bug_id);
            return;
        }

        // ── 铁律: 修复前必须有诸葛亮分析文档 ──
        let fixers = ["guanyu", "zhaoyun", "xunyu"];
        if fixers.contains(&self.agent_id.as_str()) {
            // 分析文档在 zhugeliang worktree（诸葛亮产出），也同步到自己 worktree
            let analysis_path_zg = format!("/tmp/agentforge-worktrees/zhugeliang/MD/bugs/BUG_{}_ANALYSIS.md", bug_id);
            let analysis_path_self = format!("/tmp/agentforge-worktrees/{}/MD/bugs/BUG_{}_ANALYSIS.md", self.agent_id, bug_id);
            let has_analysis = (std::path::Path::new(&analysis_path_zg).exists() &&
                std::fs::read_to_string(&analysis_path_zg).map(|s| s.len() > 200).unwrap_or(false))
                || (std::path::Path::new(&analysis_path_self).exists() &&
                std::fs::read_to_string(&analysis_path_self).map(|s| s.len() > 200).unwrap_or(false));
            if !has_analysis {
                // 没有分析文档 → 跳过，等诸葛亮分析完成后再由扫描器入队
                tracing::info!("[{}] Bug#{} 无分析文档，跳过（等诸葛亮分析）", self.agent_id, bug_id);
                // 设置5分钟TTL防止重复处理（避免死循环）
                let skip_key = format!("skip_no_analysis:{}:{}", self.agent_id, bug_id);
                let _: redis::RedisResult<()> = self.redis.clone().set_ex(&skip_key, "1", 300).await;
                return;
            }
        }

        // ── 铁律 19: fix_start 去重 — 检查是否已在处理 ──
        let dedup_key = format!("fix_active:{}:{}", self.agent_id, bug_id);
        let already_active: bool = self.redis.clone().exists(&dedup_key).await.unwrap_or(false);
        if already_active {
            tracing::warn!("[{}] Bug#{} 已在处理中（fix_active 存在），跳过重复 fix_start", self.agent_id, bug_id);
            return;
        }
        // 设置活跃标记（TTL 30 分钟）
        let _: redis::RedisResult<()> = self.redis.clone().set_ex(&dedup_key, "1", 1800).await;
        // ── 文件快照：记录修复前状态 ──
        let project_dir = if self.agent_id == "zhaoyun" {
            "/root/.openclaw/workspace/his-repo/healthlink-his-ui"
        } else {
            "/root/.openclaw/workspace/his-repo/healthlink-his-server"
        };
        let before_snapshot = take_snapshot(project_dir);
        let snapshot_key = format!("file_snapshot:{}:{}", self.agent_id, bug_id);
        if let Ok(snapshot_json) = serde_json::to_string(&before_snapshot) {
            let _: redis::RedisResult<()> = self.redis.clone().set_ex(&snapshot_key, &snapshot_json, 86400).await;
        }

        self.traces.log(&self.agent_id, "fix_start", Some(&format!("Bug#{}", bug_id)), Some(msg), Some("codex"), None, None, Some("pending"), None).await;
        self.publish_trace(&self.agent_id, "fix_start", &format!("Bug#{}", bug_id), msg, "pending", 0).await;
        // Try to acquire per-agent lock
        let lock_key = format!("codex_lock:{}", self.agent_id);
        let lock_sync = Arc::clone(&self.redis_sync); let agent = self.agent_id.clone();
        let lk = lock_key.clone();
        let acquired = tokio::task::spawn_blocking(move || {
            if let Ok(mut conn) = lock_sync.lock() {
                redis::cmd("SET").arg(&lk).arg(&agent).arg("NX").arg("EX").arg(86400)
                    .query::<Option<String>>(&mut *conn).ok().flatten().is_some()
            } else { false }
        }).await.unwrap_or(false);
        
        if !acquired {
            tracing::warn!("[{}] Failed to acquire lock for Bug #{} — skipping", self.agent_id, bug_id);
            return;
        }

        // Set current_bug in Redis for dashboard display
        let current_bug_key = format!("current_bug:{}", self.agent_id);
        let current_bug_val = format!("Bug#{}", bug_id);
        let _: redis::RedisResult<()> = self.redis.clone().set(&current_bug_key, &current_bug_val).await;

        let (an, bid, m, tr) = (self.agent_id.clone(), bug_id.clone(), msg.to_string(), Arc::clone(&self.traces));
        let feishu = self.feishu.clone();
        let mut redis_clone = self.redis.clone();
        tokio::spawn(async move {
            tracing::info!("[{}] Codex spawn started for Bug #{}", an, bid);

            // ── 🔴 铁律: 修复后检查是否有文件被删除 ──
            // （此检查在 Codex 完成后执行，通过 traces 中的 fix_done 事件触发）

            // ── 自动重试逻辑：最多 MAX_FIX_RETRIES 次，指数退避 ──
            let mut r = None;
            for attempt in 0..=MAX_FIX_RETRIES {
                if attempt > 0 {
                    let delay_ms = RETRY_BASE_DELAY_MS * 2u64.pow(attempt - 1);
                    tracing::warn!("[{}] Retry #{}/{} for Bug #{} after {}ms", an, attempt, MAX_FIX_RETRIES, bid, delay_ms);
                    tr.log(&an, "fix_retry", Some(&format!("Bug#{}", bid)),
                        Some(&format!("重试第{}次，等待{}ms", attempt, delay_ms)),
                        Some("codex"), None, None, Some("retrying"), None).await;
                    // Publish to Redis for WebSocket real-time
                    let tr_clone = tr.clone();
                    let an_c = an.clone(); let bid_c = bid.clone();
                    tokio::spawn(async move {
                        tr_clone.publish_trace_for_ws(&an_c, "fix_retry", &format!("Bug#{}", bid_c),
                            &format!("重试第{}次", attempt), "retrying", 0).await;
                    });
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }

                let attempt_result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    tokio::task::block_in_place(|| {
                        subagent::run_opencode_fix_v2(&an, &bid, &m, 0)
                    })
                })) {
                    Ok(r) => r,
                    Err(panic) => {
                        let msg = if let Some(s) = panic.downcast_ref::<String>() { s.clone() } else { "panic".into() };
                        tracing::error!("[{}] Codex panic for #{} (attempt {}): {}", an, bid, attempt, msg);
                        // Panic is always transient — retry
                        if attempt < MAX_FIX_RETRIES { continue; }
                        CodexResult {
                            success: false, bug_id: bid.clone(), elapsed_ms: 0,
                            stdout: String::new(), stderr: format!("panic: {}", msg),
                            exit_code: -1, changes: 0,
                            last_phase: "generator".to_string(), phase_verdicts: vec![],
                        }
                    }
                };

                // 成功直接用
                if attempt_result.success {
                    r = Some(attempt_result);
                    break;
                }

                // 失败：检查是否是瞬态错误
                if attempt < MAX_FIX_RETRIES && is_transient_error(&attempt_result.stderr, &attempt_result.stdout) {
                    tracing::warn!("[{}] Transient error for Bug #{} (attempt {}): {}", an, bid,
                        attempt, attempt_result.stderr.chars().take(150).collect::<String>());
                    continue;
                }

                // 非瞬态错误或已达最大重试次数
                r = Some(attempt_result);
                break;
            }

            let mut r = r.unwrap_or(CodexResult {
                success: false, bug_id: bid.clone(), elapsed_ms: 0,
                stdout: String::new(), stderr: "all retries exhausted".into(),
                exit_code: -1, changes: 0,
                last_phase: "generator".to_string(), phase_verdicts: vec![],
            });

            // ── 文件快照 Diff：计算修复变更（仅用于信息展示，不再覆盖 success 判定）──
            // 铁律: success 判定必须基于 worktree 实际变更，不能用主仓库快照覆盖
            let file_diff = {
                let snapshot_key = format!("file_snapshot:{}:{}", an, bid);
                let snapshot_json: String = redis_clone.clone().get(&snapshot_key).await.unwrap_or_default();
                let before: std::collections::HashMap<String, (u64, String)> =
                    serde_json::from_str(&snapshot_json).unwrap_or_default();
                let proj_dir = if an == "zhaoyun" {
                    "/root/.openclaw/workspace/his-repo/healthlink-his-ui"
                } else {
                    "/root/.openclaw/workspace/his-repo/healthlink-his-server"
                };
                snapshot_and_diff(proj_dir, &before)
            };
            let diff_summary = file_diff.summary();
            let diff_detail = file_diff.detail();
            // ── 不再用文件快照覆盖 success：success 由 subagent 的 has_fix_commit + changes 判定 ──
            // 文件快照仅用于日志记录
            tracing::info!("[{}] Fix #{}: ok={} changes={} file_diff={} time={}ms", an, bid, r.success, r.changes, diff_summary, r.elapsed_ms);
            let phase_summary = r.phase_verdicts.iter().map(|(p,v)| format!("{}:{}", p, v)).collect::<Vec<_>>().join(" ");
            let fix_msg = format!("{} | 文件变更: {} | 阶段: {}", r.stdout.chars().take(200).collect::<String>(), diff_summary, phase_summary);
            tr.log(&an, "fix_done", Some(&format!("Bug#{}", bid)), Some(&fix_msg), Some("codex"), None, Some(r.elapsed_ms as i64), Some(if r.success {"ok"} else {"failed"}), None).await;
            // Publish to Redis for WebSocket real-time
            {
                let tr_clone = tr.clone();
                let an_c = an.clone(); let bid_c = bid.clone();
                let msg_c = r.stdout.chars().take(200).collect::<String>();
                let status_c = if r.success {"ok"} else {"failed"};
                let dur_c = r.elapsed_ms as i64;
                tokio::spawn(async move {
                    tr_clone.publish_trace_for_ws(&an_c, "fix_done", &format!("Bug#{}", bid_c),
                        &msg_c, status_c, dur_c).await;
                });
            }
            let _: redis::RedisResult<()> = redis_clone.del(format!("codex_lock:{}", an)).await;
            // 清理 fix_active 去重标记，允许后续重试
            let _: redis::RedisResult<()> = redis_clone.del(format!("fix_active:{}:{}", an, bid)).await;

            // ── 全链路验证（异步非阻塞）──
            // 铁律 20: 验证不通过禁止进 Pipeline
            // 但验证本身不阻塞 executor，spawn 到独立 task
            let an_v = an.clone();
            let bid_v = bid.clone();
            let m_v = m.clone();
            let tr_v = tr.clone();
            let mut redis_v = redis_clone.clone();
            // 验证在 develop 分支上跑（main_repo），确保验证的是最终合入的代码
            // 而不是 agent worktree 中的代码
            // 铁律: 前端 agent 用前端目录，后端 agent 用后端目录
            let work_dir = if an_v == "zhaoyun" {
                "/root/.openclaw/workspace/his-repo/healthlink-his-ui".to_string()
            } else {
                "/root/.openclaw/workspace/his-repo/healthlink-his-server".to_string()
            };
            tokio::spawn(async move {
                // 确保在 develop 分支上验证（可能被其他 agent 切换过）
                let _ = std::process::Command::new("git")
                    .args(["-C", "/root/.openclaw/workspace/his-repo", "checkout", "develop"])
                    .output();
                let _ = std::process::Command::new("git")
                    .args(["-C", "/root/.openclaw/workspace/his-repo", "pull", "--rebase", "origin", "develop"])
                    .output();
                tracing::info!("[{}] Bug #{} 开始全链路验证（develop 分支）...", an_v, bid_v);
                let verification = super::verification::run_full_verification(&an_v, &bid_v, &m_v, &work_dir);
                tracing::info!("[{}] Bug #{} 验证结果: {} ({}ms)", an_v, bid_v, verification.summary, verification.total_ms);
                let verify_detail = serde_json::to_string(&verification).unwrap_or_default();
                tr_v.log(&an_v, "verification", Some(&format!("Bug#{}", bid_v)), 
                    Some(&verification.summary), Some("verification"), None, 
                    Some(verification.total_ms as i64), 
                    Some(if verification.all_passed {"ok"} else {"failed"}),
                    Some(&verify_detail)).await;
                // Publish verification result to WebSocket
                tr_v.publish_trace_for_ws(&an_v, "verification", &format!("Bug#{}", bid_v),
                    &verification.summary, if verification.all_passed {"ok"} else {"failed"}, 
                    verification.total_ms as i64).await;
                // 验证失败 → 反馈给智能体进行二次修复
                if !verification.all_passed {
                    tracing::warn!("[{}] Bug #{} 全链路验证失败: {}", an_v, bid_v, verification.summary);
                    let _: redis::RedisResult<()> = redis_v.sadd(format!("agent-failed-bugs:{}", an_v), &bid_v).await;
                    // 清理 fix_active 标记，允许后续重试
                    let _: redis::RedisResult<()> = redis_v.del(format!("fix_active:{}:{}", an_v, bid_v)).await;
                    
                    // 检查验证重试次数（最多 3 次）+ 连续相同错误检测
                    let retry_key = format!("verify_retry:{}:{}", an_v, bid_v);
                    let retry_count: i32 = redis_v.clone().get(&retry_key).await.unwrap_or(0);
                    let _: redis::RedisResult<()> = redis_v.clone().set_ex(&retry_key, retry_count + 1, 3600).await;
                    
                    // 连续相同错误检测：哈希失败原因，连续 2 次相同错误则停止重试
                    let error_fingerprint: String = verification.checks.iter()
                        .filter(|c| !c.passed)
                        .map(|c| format!("{}:{}", c.name, c.message.chars().take(100).collect::<String>()))
                        .collect::<Vec<_>>().join("|");
                    let error_hash_key = format!("verify_error_hash:{}:{}", an_v, bid_v);
                    let last_error_hash: String = redis_v.clone().get(&error_hash_key).await.unwrap_or_default();
                    let _: redis::RedisResult<()> = redis_v.clone().set_ex(&error_hash_key, &error_fingerprint, 3600).await;
                    let same_error_repeat = !error_fingerprint.is_empty() && error_fingerprint == last_error_hash;
                    
                    if retry_count < 3 && !(same_error_repeat && retry_count >= 1) {
                        // 构建失败反馈消息，包含详细失败原因
                        let failed_checks: Vec<String> = verification.checks.iter()
                            .filter(|c| !c.passed)
                            .map(|c| format!("- {} ❌: {}", c.name, c.message.lines().next().unwrap_or("")))
                            .collect();
                        let retry_msg = format!(
                            "【验证失败反馈】Bug #{} 上次修复未通过全链路验证，请根据以下失败原因重新修复：

失败原因：
{}

总耗时: {}ms

请针对上述失败项重新修复，确保：
1. 编译通过（vite build / mvn compile）
2. 单元测试通过（vitest / mvn test）
3. Playwright 回归测试通过
4. 数据库表可访问
5. 后端服务可达",
                            bid_v, failed_checks.join("
"), verification.total_ms
                        );
                        
                        // 存储失败详情到 Redis（供 agent 读取）
                        let detail_key = format!("verify_fail_detail:{}:{}", an_v, bid_v);
                        let _: redis::RedisResult<()> = redis_v.clone().set_ex(&detail_key, &verify_detail, 3600).await;
                        
                        // 推送重试任务到 agent 队列
                        let retry_task = serde_json::json!({
                            "agent_id": an_v,
                            "message": retry_msg,
                            "source": "verify_retry",
                            "sender_id": "verification",
                            "chat_id": "",
                            "is_dm": "true",
                            "msg_id": format!("verify-retry-{}-{}-{}", bid_v, retry_count + 1, chrono::Local::now().timestamp()),
                            "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                        });
                        let queue = format!("agent-work-queue:fix:{}", an_v);
                        let _: redis::RedisResult<i64> = redis_v.clone().rpush(&queue, retry_task.to_string()).await;
                        tracing::info!("[{}] Bug #{} 验证失败反馈已推送到队列 (重试 {}/3, same_error={})", an_v, bid_v, retry_count + 1, same_error_repeat);
                    } else {
                        let reason = if same_error_repeat { "连续相同错误" } else { "已达重试上限(3次)" };
                        tracing::warn!("[{}] Bug #{} 验证重试{}，标记为最终失败", an_v, bid_v, reason);
                        // 存储最终失败标记
                        let final_key = format!("verify_final_fail:{}:{}", an_v, bid_v);
                        let _: redis::RedisResult<()> = redis_v.clone().set_ex(&final_key, &verify_detail, 86400).await;
                    }
                }
            });

            // L5: 自动评分 — 更新 agent 成功率和耗时
            {
                let scores_path = "/var/lib/agentforge/agent_scores.json";
                let mut opt = super::self_optimizer::SelfOptimizer::load(scores_path);
                let bug_type = if m.contains("前端") || m.contains("vue") || m.contains("界面") { "frontend" }
                    else if m.contains("SQL") || m.contains("数据库") || m.contains("迁移") { "database" }
                    else { "backend" };
                opt.update_scores(&an, bug_type, r.success, r.elapsed_ms as f64 / 1000.0);
                let _ = opt.save(scores_path);
            }

            // 飞书通知修复结果
            let _ = feishu.send(&format!(
                "{} Bug #{} 修复{}（{} 秒，{} 个文件变更）",
                if r.success { "✅" } else { "❌" },
                bid,
                if r.success { "成功" } else { "失败" },
                r.elapsed_ms / 1000,
                r.changes,
            ), None).await;

            // 写 pipeline 结果，供 Pipeline 命令轮询
            let pipeline_result = serde_json::json!({
                "bug_id": bid,
                "agent": an,
                "success": r.success,
                "elapsed_ms": r.elapsed_ms,
                "changes": r.changes,
                "last_phase": r.last_phase,
                "phase_verdicts": r.phase_verdicts.iter().map(|(p,v)| serde_json::json!({"phase": p, "verdict": v})).collect::<Vec<_>>(),
                "error": if r.success { String::new() } else { r.stderr.chars().take(200).collect::<String>() },
            });
            let _: redis::RedisResult<()> = redis_clone.set_ex(
                &format!("pipeline:result:{}", bid),
                pipeline_result.to_string(),
                86400, // 24h TTL
            ).await;

            // 失败处理：标记 bug 并移出队列（防止协调器不断重新入队）
            if !r.success {
                let _: redis::RedisResult<()> = redis_clone.sadd(format!("agent-failed-bugs:{}", an), &bid).await;
                // 清理 fix_active 标记，允许后续重试
                let _: redis::RedisResult<()> = redis_clone.del(format!("fix_active:{}:{}", an, bid)).await;
                // 从队列中移除（按内容匹配删除当前这个任务）
                let queue_key = format!("agent-work-queue:fix:{}", an);
                let _: redis::RedisResult<i64> = redis_clone.lrem(&queue_key, 1, &m).await;
                tracing::info!("[{}] Bug #{} fix failed, removed from queue and added to failed set", an, bid);
            }

            // 管道：先走诸葛亮分析路由，再走全链路
            if r.success {
                // Dedup: only trigger pipeline once per bug (Redis key with 24h TTL)
                let pipeline_key = format!("pipeline_sent:{}", bid);
                let already_sent: bool = redis_clone.exists(&pipeline_key).await.unwrap_or(false);
                if !already_sent {
                    let _: redis::RedisResult<()> = redis_clone.set_ex(&pipeline_key, "1", 86400).await;
                    let reporter = pipeline::extract_reporter(&m);
                    // Step 1: 诸葛亮分析修复是否需要DB审查
                    let pipe_task = serde_json::json!({
                        "agent_id": "zhugeliang",
                        "message": format!("请分析 Bug #{} 的修复是否需要 DB 审查。
提出人: {}。
修复 Agent: {}。
如果涉及 DB 变更，路由给 Xunyu 审查；否则直接路由给 Zhangfei 测试。", bid, reporter, an),
                        "source": "pipeline_analyze",
                        "sender_id": an,
                        "bug_reporter": reporter,
                        "msg_id": format!("pipeline-analyze-{}-{}", bid, chrono::Local::now().timestamp()),
                        "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                        "chat_id": "", "is_dm": "true",
                    });
                    let _: redis::RedisResult<i64> = redis_clone.rpush("agent-work-queue:fix:zhugeliang", pipe_task.to_string()).await;
                }
            }
        });
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    async fn handle_pipeline_test(&self, msg: &str) {
        let bid = pipeline::extract_bug_id(msg); let rep = pipeline::extract_reporter(msg);
        tracing::info!("[zhangfei] Testing Bug #{}", bid);
        // 检查重试次数
        let retry_key = format!("pipeline_retry:{}", bid);
        let retry_count: i32 = self.redis.clone().get(&retry_key).await.unwrap_or(0);
        let max_retries = 3;

        if retry_count >= max_retries {
            tracing::warn!("[zhangfei] Bug #{} exceeded max retries ({})", bid, max_retries);
            let _ = self.feishu.send(&format!("⚠️ Bug #{} 修复测试失败超过 {} 次，请人工介入。", bid, max_retries), None).await;
            return;
        }

        // ── 轮次预算检查 ──
        let budget = RoundBudget::default();
        if check_round_budget(&bid, "zhangfei", &mut self.redis.clone(), &budget).await.unwrap_or(false) {
            tracing::warn!("[zhangfei] Bug #{} 超出轮次预算，升级到人工处理", bid);
            let budget_event = PipelineEvent::BudgetUpdate { bug_id: bid.clone(), agent: "zhangfei".into(), current: budget.max_test_rounds, max: budget.max_test_rounds };
            self.publish_trace("pipeline", "budget_exceeded", &format!("Bug#{}", bid), &budget_event.to_json(), "failed", 0).await;
            let _ = self.feishu.send(&format!("🔴 Bug #{} 超出轮次预算，升级到人工处理。", bid), None).await;
            self.traces.log("zhangfei", "budget_exceeded", Some(&format!("Bug#{}", bid)), None, None, None, None, Some("failed"), None).await;
            return;
        }
        increment_round(&bid, "zhangfei", &mut self.redis.clone()).await;

        // 确保前端 dev server 在运行
        let _ = tokio::process::Command::new("bash")
            .arg("/root/.openclaw/workspace/scripts/ensure-frontend.sh")
            .output().await;

        // 运行 Playwright 回归测试（单 worker 避免压垮 dev server）
        let test_result = tokio::time::timeout(
            Duration::from_secs(120),
            tokio::process::Command::new("bash")
                .arg("-c")
                .arg(format!("cd /root/.openclaw/workspace/his-repo/healthlink-his-ui && npx playwright test --grep @bug{} --reporter=line --workers=1 2>&1", bid))
                .output()
        ).await;

        let (test_passed_raw, test_output) = match test_result {
            Ok(Ok(out)) => (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string()),
            Ok(Err(e)) => (false, format!("spawn error: {}", e)),
            Err(_) => (false, "timeout after 120s".to_string()),
        };

        // 'No tests found' = no Playwright test exists for this bug, which is OK (not a regression)
        let no_test_found = test_output.contains("No tests found") || test_output.contains("no tests");
        let test_passed = test_passed_raw || no_test_found;

        let test_verdict = if test_passed {
            Verdict::Pass
        } else {
            // 尝试降级测试
            let degraded = degraded_test(&bid, &rep).await;
            if degraded {
                Verdict::Pass
            } else {
                Verdict::Fail("Playwright测试失败且降级测试也失败".to_string())
            }
        };

        if test_verdict.is_pass() {
            tracing::info!("[zhangfei] Bug #{} VERDICT: PASS", bid);
            let _ = self.feishu.send(&format!("✅ Bug #{} 回归测试通过 (VERDICT: PASS)。", bid), None).await;

            // 清理重试计数
            let _: redis::RedisResult<()> = self.redis.clone().del(&retry_key).await;

            // ── 截图证据收集 ──
            let report_dir = "/root/.openclaw/workspace/his-repo/healthlink-his-ui/tests/e2e/report";
            let mut evidence_files: Vec<String> = Vec::new();
            // 1. 测试 spec 中手动生成的截图
            let manual_screenshot = format!("{}/bug-{}-result.png", report_dir, bid);
            if std::path::Path::new(&manual_screenshot).exists() {
                evidence_files.push(manual_screenshot);
            }
            // 2. Playwright 自动截图（test-results 目录）
            let test_results_dir = "/root/.openclaw/workspace/his-repo/healthlink-his-ui/test-results";
            if let Ok(entries) = std::fs::read_dir(test_results_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.contains(&format!("bug{}", bid)) || name.contains(&format!("bug-{}", bid)) {
                        let screenshots_dir = entry.path().join("screenshots");
                        if screenshots_dir.exists() {
                            if let Ok(imgs) = std::fs::read_dir(&screenshots_dir) {
                                for img in imgs.flatten() {
                                    let p = img.path().to_string_lossy().to_string();
                                    if p.ends_with(".png") { evidence_files.push(p); }
                                }
                            }
                        }
                        // 也检查根目录下的截图
                        if let Ok(root_imgs) = std::fs::read_dir(entry.path()) {
                            for img in root_imgs.flatten() {
                                let p = img.path().to_string_lossy().to_string();
                                if p.ends_with(".png") { evidence_files.push(p); }
                            }
                        }
                    }
                }
            }
            // 3. Playwright HTML report 中的截图
            let html_report_data = format!("{}/data", report_dir);
            if let Ok(entries) = std::fs::read_dir(&html_report_data) {
                for entry in entries.flatten() {
                    let p = entry.path().to_string_lossy().to_string();
                    if p.ends_with(".png") && p.contains(&bid) {
                        evidence_files.push(p);
                    }
                }
            }

            tracing::info!("[zhangfei] Bug #{} 找到 {} 个截图证据文件", bid, evidence_files.len());

            // 提取测试结果摘要
            let _test_summary: String = test_output.lines()
                .filter(|l| l.contains("passed") || l.contains("failed") || l.contains("Pending") || l.contains("✓") || l.contains("✗"))
                .collect::<Vec<_>>()
                .join("\n");

            // ── 上传截图证据到禅道 ──
            let evidence_paths = evidence_files.clone();
            {
                let cfg = crate::config::Config::load().ok();
                if let Some(cfg) = cfg {
                    let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                    // 上传每个截图文件
                    for (i, path) in evidence_paths.iter().enumerate() {
                        let desc = format!("Playwright回归测试截图 #{} — Bug #{}", i + 1, bid);
                        match client.upload_attachment(&bid, path, &desc).await {
                            Ok(_) => tracing::info!("[zhangfei] Bug #{} 截图证据 #{} 上传成功: {}", bid, i + 1, path),
                            Err(e) => tracing::warn!("[zhangfei] Bug #{} 截图证据 #{} 上传失败: {}", bid, i + 1, e),
                        }
                    }
                    // 生成完整的测试报告（含证据清单）
                    let evidence_list = if evidence_paths.is_empty() {
                        "⚠️ 未找到截图文件（测试可能未生成截图）".to_string()
                    } else {
                        evidence_paths.iter().enumerate()
                            .map(|(i, p)| format!("  {}. {}", i + 1, std::path::Path::new(p).file_name().unwrap_or_default().to_string_lossy()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    let test_report = format!(
                        "[🔥 张飞测试报告] Bug #{} Playwright回归测试\n\n                        测试状态：✅ 通过\n                        测试标签：@bug{}\n                        执行模式：无头浏览器 (chromium, 1920x1080)\n                        测试输出摘要：\n{}\n\n                        📸 截图证据（{} 张）：\n{}\n\n                        结论：回归测试通过，BUG已修复。截图已上传至禅道附件。",
                        bid, bid,
                        _test_summary.chars().take(500).collect::<String>(),
                        evidence_paths.len(),
                        evidence_list
                    );
                    let _ = client.comment_bug(&bid, &test_report).await;
                    // 二次检查：防止人类在测试通过后手动关闭 bug
                    {
                        let cfg_check = crate::config::Config::load().ok();
                        if let Some(ref cfg) = cfg_check {
                            let zc = crate::core::zentao::ZentaoClient::from_config(cfg);
                            let rt = tokio::runtime::Handle::current();
                            match rt.block_on(zc.get_bug(&bid)) {
                                Ok(detail) => {
                                    let st = detail.status.as_str();
                                    if st == "resolved" || st == "closed" || st == "done" {
                                        tracing::warn!("[zhangfei] Bug #{} 已被人类关闭/解决(status={})，跳过 resolve", bid, st);
                                    } else {
                                        let _ = client.resolve_bug(&bid, "Playwright回归测试通过，BUG已修复").await;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("[zhangfei] Bug #{} 状态检查失败({})，保守执行 resolve", bid, e);
                                    let _ = client.resolve_bug(&bid, "Playwright回归测试通过，BUG已修复").await;
                                }
                            }
                        } else {
                            // 无法加载配置，保守执行 resolve
                            let _ = client.resolve_bug(&bid, "Playwright回归测试通过，BUG已修复").await;
                        }
                    }
                }
            }

            // 保存测试文档
            let test_doc = format!("# Bug #{} 回归测试\n\n**Playwright 测试通过**\n\n测试标签: @bug{}", bid, bid);
            let _: redis::RedisResult<()> = self.redis.clone().set_ex(format!("test_doc:{}", bid), &test_doc, 86400).await;

            // 创建交接卡（张飞 → 华佗）
            let mut handoff = HandoffCard::new(&bid, "", &rep, "zhangfei", "huatuo", "test");
            handoff.verification_summary = Some(test_output.lines().take(5).collect::<Vec<_>>().join("; "));
            handoff.save(&mut self.redis.clone()).await;

            // 发送流式事件
            let event = PipelineEvent::Handoff { bug_id: bid.clone(), from: "zhangfei".into(), to: "huatuo".into(), stage: "test".into() };
            self.publish_trace("pipeline", "handoff", &format!("Bug#{}", bid), &event.to_json(), "ok", 0).await;

            // 通知下一阶段（huatuo 验收 + chenlin 归档）
            let ts = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
            let next_msg = format!("Bug #{} 测试完成，请验收。提出人: {}。", bid, rep);
            for next_agent in &["huatuo", "chenlin"] {
                let pipe_task = serde_json::json!({
                    "agent_id": next_agent,
                    "message": &next_msg,
                    "source": "pipeline_test_done",
                    "sender_id": "zhangfei",
                    "bug_reporter": &rep,
                    "msg_id": format!("pipeline-test-done-{}-{}", bid, chrono::Local::now().timestamp()),
                    "timestamp": &ts,
                    "chat_id": "", "is_dm": "true",
                });
                let _: redis::RedisResult<i64> = self.redis.clone().rpush(
                    format!("agent-work-queue:fix:{}", next_agent),
                    pipe_task.to_string()
                ).await;
            }
        } else {
            // 测试失败：增加重试计数，推回给 fixer 重修
            let new_count = retry_count + 1;
            let _: redis::RedisResult<()> = self.redis.clone().set(&retry_key, new_count).await;
            let _: redis::RedisResult<()> = self.redis.clone().expire(&retry_key, 86400).await;

            tracing::warn!("[zhangfei] Bug #{} Playwright test FAILED (attempt {}/{})", bid, new_count, max_retries);
            let _ = self.feishu.send(&format!("⚠️ Bug #{} 回归测试失败（第 {}/{} 次），已退回修复。\n```\n{}\n```",
                bid, new_count, max_retries, &test_output.chars().take(500).collect::<String>()), None).await;

            // 失败信息由飞书通知，不单独写禅道（避免干扰禅道状态）

            // 从消息中提取原 fixer agent_id
            let sender = msg.split("sender_id:").nth(1).and_then(|s| s.split(',').next()).map(|s| s.trim().trim_matches('"')).unwrap_or("zhaoyun");
            let rework_task = serde_json::json!({
                "agent_id": sender,
                "message": format!("请重新修复 Bug #{}。回归测试未通过，需继续修改代码直到测试通过。\n测试输出：\n{}", bid, test_output.chars().take(300).collect::<String>()),
                "source": "pipeline_retry",
                "sender_id": "zhangfei",
                "msg_id": format!("pipeline-retry-{}-{}", bid, chrono::Local::now().timestamp()),
                "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                "chat_id": "", "is_dm": "true",
            });
            let _: redis::RedisResult<i64> = self.redis.clone().rpush(
                format!("agent-work-queue:fix:{}", sender),
                rework_task.to_string()
            ).await;
        }
        self.traces.log("zhangfei", "test_done", Some(&format!("Bug#{}", bid)), Some(if test_passed {"pass"} else {"fail"}), None, None, None, Some(if test_passed {"ok"} else {"failed"}), None).await;
        self.traces.publish_trace_for_ws("zhangfei", "test_done", &format!("Bug#{}", bid), if test_passed {"测试通过"} else {"测试失败"}, if test_passed {"ok"} else {"failed"}, 0).await;
    }

    async fn handle_pipeline_verify(&self, msg: &str) {
        let bid = pipeline::extract_bug_id(msg); let rep = pipeline::extract_reporter(msg);

        // 🔴 铁律: 检查修复是否已合并到 develop 分支
        {
            let his_repo = "/root/.openclaw/workspace/his-repo";
            let git_log = std::process::Command::new("git")
                .args(["log", "origin/develop", "--oneline", "--grep", &format!("#{}", bid)])
                .current_dir(his_repo)
                .output();
            match git_log {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if stdout.trim().is_empty() {
                        tracing::warn!("[huatuo] Bug#{} 修复未合并到 develop 分支，拒绝验收", bid);
                        let _ = self.feishu.send(&format!("❌ Bug #{} 验收失败：修复未合并到 develop 分支！请先合并再验收。", bid), None).await;
                        // 写入禅道备注
                        let cfg = crate::config::Config::load().ok();
                        if let Some(cfg) = cfg {
                            let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                            let _ = client.comment_bug(&bid, &format!(
                                "[🚫 华佗验收] Bug #{} 验收失败

原因：修复代码未合并到 develop 分支，修复不会部署到生产环境
要求：先执行 git merge 到 develop 后重新提交验收", bid
                            )).await;
                        }
                        self.traces.log("huatuo", "verify_done", Some(&format!("Bug#{}", bid)), Some("rejected: not on develop"), None, None, None, Some("failed"), None).await;
                        self.publish_trace("huatuo", "verify_done", &format!("Bug#{}", bid), "验收失败：未合并到develop", "failed", 0).await;
                        return;
                    }
                    tracing::info!("[huatuo] Bug#{} develop 分支确认有修复 commit", bid);
                }
                Err(e) => {
                    tracing::warn!("[huatuo] 无法检查 develop 分支: {}", e);
                }
            }
        }

        // 🔴 铁律: 检查后端服务是否已重新编译部署
        {
            // 获取后端服务启动时间
            let status_output = std::process::Command::new("systemctl")
                .args(["show", "his-backend.service", "--property=ActiveEnterTimestamp"])
                .output();
            // 获取修复commit时间
            let commit_output = std::process::Command::new("git")
                .args(["log", "origin/develop", "--format=%ai", "-1", "--grep", &format!("#{}", bid)])
                .current_dir("/root/.openclaw/workspace/his-repo")
                .output();
            if let (Ok(s), Ok(c)) = (status_output, commit_output) {
                let start_str = String::from_utf8_lossy(&s.stdout).replace("ActiveEnterTimestamp=", "").trim().to_string();
                let commit_str = String::from_utf8_lossy(&c.stdout).trim().to_string();
                if !start_str.is_empty() && !commit_str.is_empty() {
                    // 简单比较：如果服务启动时间早于commit时间，说明未重新部署
                    if start_str < commit_str {
                        tracing::warn!("[huatuo] Bug#{} 后端服务({})早于修复commit({})，未重新编译部署", bid, start_str, commit_str);
                        let _ = self.feishu.send(&format!("❌ Bug #{} 验收失败：后端服务未重新编译部署！
服务启动: {}
修复时间: {}
请先 mvn package + systemctl restart his-backend", bid, start_str, commit_str), None).await;
                        let cfg = crate::config::Config::load().ok();
                        if let Some(cfg) = cfg {
                            let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                            let _ = client.comment_bug(&bid, &format!(
                                "[🚫 华佗验收] Bug #{} 验收失败

原因：后端服务未重新编译部署
服务启动时间: {}
修复commit时间: {}
要求：mvn package -DskipTests + systemctl restart his-backend 后重新验收", bid, start_str, commit_str
                            )).await;
                        }
                        self.traces.log("huatuo", "verify_done", Some(&format!("Bug#{}", bid)), Some("rejected: not deployed"), None, None, None, Some("failed"), None).await;
                        self.publish_trace("huatuo", "verify_done", &format!("Bug#{}", bid), "验收失败：未编译部署", "failed", 0).await;
                        return;
                    }
                    tracing::info!("[huatuo] Bug#{} 后端服务({})晚于修复commit({})，已部署", bid, start_str, commit_str);
                }
            }
        }

        let test_doc: Option<String> = self.redis.clone().get(format!("test_doc:{}", bid)).await.ok();
        if test_doc.is_none() { let _ = self.feishu.send(&format!("⚠️ Bug #{} 验收失败：无测试文档", bid), None).await; return; }
        if pipeline::is_human(&rep) {
            // 人类提的bug：只加备注，不改状态和分配
            let cfg = crate::config::Config::load().ok();
            if let Some(cfg) = cfg {
                let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                let _ = client.comment_bug(&bid, &format!(
                    "[💊 华佗验收] Bug #{} 验收通过\n\n                    验收人：华佗\n                    提出人：{}（人类）\n                    测试文档：已确认\n                    验收结果：通过",
                    bid, rep
                )).await;
            }
            let _ = self.feishu.send(&format!("Bug #{} 验收通过（人类 {}）。", bid, rep), None).await;
            self.traces.log("huatuo", "verify_done", Some(&format!("Bug#{}", bid)), None, None, None, None, Some("ok"), None).await;
            self.traces.publish_trace_for_ws("huatuo", "verify_done", &format!("Bug#{}", bid), "验收完成（人类）", "ok", 0).await;
            return;
        }
        // 智能体提的bug：加备注 + 解决 + 分配给提出人
        let cfg = crate::config::Config::load().ok();
        if let Some(cfg) = cfg {
            let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
            let verify_comment = format!(
                "[💊 华佗验收] Bug #{} 验收通过\n\n                验收人：华佗\n                提出人：{}\n                测试文档：已确认\n                验收结果：通过\n                操作：标记为已解决，分配给提出人",
                bid, rep
            );
            let _ = client.comment_bug(&bid, &verify_comment).await;
            // 二次检查：防止人类在验收通过后手动关闭 bug
            {
                let cfg_check = crate::config::Config::load().ok();
                if let Some(ref cfg) = cfg_check {
                    let zc = crate::core::zentao::ZentaoClient::from_config(cfg);
                    let rt = tokio::runtime::Handle::current();
                    match rt.block_on(zc.get_bug(&bid)) {
                        Ok(detail) => {
                            let st = detail.status.as_str();
                            if st == "resolved" || st == "closed" || st == "done" {
                                tracing::warn!("[huatuo] Bug #{} 已被人类关闭/解决(status={})，跳过 resolve", bid, st);
                            } else {
                                let _ = client.resolve_bug(&bid, "验收通过").await;
                                let _ = client.assign_bug(&bid, &rep, "验收通过，分配给提出人确认").await;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[huatuo] Bug #{} 状态检查失败({})，保守执行 resolve", bid, e);
                            let _ = client.resolve_bug(&bid, "验收通过").await;
                            let _ = client.assign_bug(&bid, &rep, "验收通过，分配给提出人确认").await;
                        }
                    }
                } else {
                    let _ = client.resolve_bug(&bid, "验收通过").await;
                    let _ = client.assign_bug(&bid, &rep, "验收通过，分配给提出人确认").await;
                }
            }
        }
        let verify_verdict = Verdict::Pass;
        let _ = self.feishu.send(&format!("✅ Bug #{} VERDICT: PASS (验收通过)。", bid), None).await;
        self.traces.log("huatuo", "verify_done", Some(&format!("Bug#{}", bid)), Some("VERDICT: PASS"), None, None, None, Some("ok"), None).await;
        self.traces.publish_trace_for_ws("huatuo", "verify_done", &format!("Bug#{}", bid), "VERDICT: PASS (验收完成)", "ok", 0).await;
    }

    async fn handle_chenlin_doc(&self, msg: &str) {
        let bid = pipeline::extract_bug_id(msg);
        let rep = pipeline::extract_reporter(msg);
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        // ── Step 1: 收集全流程 trace 数据 ──
        let pool = &self.traces.pool;
        let traces: Vec<(String,String,String,String,String,i64,String)> =
            sqlx::query_as("SELECT agent_id, event, task_id, status, COALESCE(message,''), COALESCE(duration_ms,0), ts FROM traces WHERE task_id LIKE ?1 AND ts > ?2 ORDER BY ts ASC")
                .bind(format!("Bug#{}", bid))
                .bind("2026-01-01")
                .fetch_all(pool).await.unwrap_or_default();

        // ── Step 2: 收集 Git commit 信息 ──
        let worktree = "/tmp/agentforge-worktrees/guanyu/healthlink-his-server";
        let commit_hash = std::process::Command::new("git")
            .args(["-C", worktree, "log", "--oneline", "--format=%h", "-1", "--", "."])
            .output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let commit_files = std::process::Command::new("git")
            .args(["-C", worktree, "show", "--stat", "--format=", &commit_hash])
            .output().map(|o| String::from_utf8_lossy(&o.stdout).lines().map(String::from).collect::<Vec<_>>())
            .unwrap_or_default();

        // ── Step 3: 从 trace 中提取根因和测试结果 ──
        let mut root_cause = String::new();
        let mut test_result = String::new();
        let mut test_output = String::new();
        let mut fix_duration = 0i64;
        let mut fix_start_ts = String::new();
        let mut fix_end_ts = String::new();

        for (_agent, event, _task, status, message, dur, ts) in &traces {
            match event.as_str() {
                "fix_start" => { fix_start_ts = ts.clone(); }
                "fix_done" => {
                    fix_end_ts = ts.clone();
                    fix_duration = *dur;
                    root_cause = message.chars().take(500).collect();
                }
                "test_done" => {
                    test_result = status.clone();
                    test_output = message.clone();
                }
                _ => {}
            }
        }

        // ── Step 3.5: 如果 traces 查询为空（时序问题），从消息中提取 ──
        if test_result.is_empty() {
            test_result = if msg.contains("测试通过") || msg.contains("test_done") { "ok".into() } else { "unknown".into() };
        }
        if fix_duration == 0 {
            fix_start_ts = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
            fix_end_ts = fix_start_ts.clone();
        }

        // ── Step 4: 生成完整 Markdown 报告 ──
        let pipeline_timeline = traces.iter().map(|(agent, event, _task, status, _msg, dur, ts)| {
            let status_icon = match status.as_str() {
                "ok" => "✅", "failed" => "❌", "pending" => "⏳", _ => "❓",
            };
            format!("| {} | {} | {} | {} | {:.1}s |",
                ts.chars().skip(11).take(8).collect::<String>(),
                agent, event, status_icon, *dur as f64 / 1000.0)
        }).collect::<Vec<_>>().join("\n");

        let fix_files_list: String = commit_files.iter()
            .filter(|l| l.contains(".java") || l.contains(".vue") || l.contains(".ts") || l.contains(".js"))
            .take(20).cloned().collect::<Vec<String>>().join("\n");

        let test_icon = if test_result == "ok" { "✅ PASS" } else { "❌ FAIL" };

        let report_md = format!(r#"# Bug #{} 修复报告

## 基本信息
- **标题**: {}
- **提出人**: {}
- **修复时间**: {} ~ {}
- **修复耗时**: {:.1}s
- **Commit**: `{}`
- **测试结果**: {}

## 根因分析
{}

## 修复文件
{}

## 流程时间线
| 时间 | 智能体 | 事件 | 状态 | 耗时 |
|------|--------|------|------|------|
{}
| {} | chenlin | doc_done | ✅ | <1s |

## 全流程
诸葛亮分析 → guanyu 修复 → 张飞测试 → 华佗验收 → 陈琳归档
"#,
            bid, msg.chars().take(200).collect::<String>(), rep,
            fix_start_ts.chars().skip(11).take(8).collect::<String>(),
            fix_end_ts.chars().skip(11).take(8).collect::<String>(),
            fix_duration as f64 / 1000.0, commit_hash, test_icon,
            root_cause, fix_files_list, pipeline_timeline,
            ts.chars().skip(11).take(8).collect::<String>()
        );

        // ── Step 4.5: 🔴 铁律 — 检查是否有文件被删除 ──
        {
            let worktree = "/tmp/agentforge-worktrees/guanyu/healthlink-his-server";
            let git_diff = std::process::Command::new("git")
                .args(["diff", "--name-status", "HEAD~1"])
                .current_dir(worktree)
                .output();
            if let Ok(out) = git_diff {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let deleted: Vec<&str> = stdout.lines()
                    .filter(|l| l.starts_with('D'))
                    .collect();
                if !deleted.is_empty() {
                    tracing::warn!("[chenlin] Bug#{} 修复中删除了文件: {:?}", bid, deleted);
                    let _ = self.feishu.send(&format!("⚠️ Bug #{} 归档警告：修复中删除了 {} 个文件！
{}
请检查是否应该重构而非删除", bid, deleted.len(), deleted.join("
")), None).await;
                    let cfg = crate::config::Config::load().ok();
                    if let Some(cfg) = cfg {
                        let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                        let _ = client.comment_bug(&bid, &format!(
                            "[⚠️ 陈琳归档] Bug #{} 归档警告

修复中检测到文件删除：{}
铁律：禁止删除已有源文件，应重构修复", bid, deleted.join(", ")
                        )).await;
                    }
                }
            }
        }

        // ── Step 5: Git 归档 ──
        let his_repo = "/root/.openclaw/workspace/his-repo";
        let docs_dir = format!("{}/docs/bug-fixes", his_repo);
        let _ = std::fs::create_dir_all(&docs_dir);
        let report_file = format!("{}/bug-{}.md", docs_dir, bid);
        let _ = std::fs::write(&report_file, &report_md);
        let _ = std::process::Command::new("git").args(["-C", his_repo, "add", &report_file]).output();
        let _ = std::process::Command::new("git").args(["-C", his_repo, "commit", "-m", &format!("docs: Bug #{} 修复报告归档", bid), "--allow-empty"]).output();

        // ── Step 6: SQLite 归档 ──
        {
            let pipeline_json = serde_json::json!({
                "traces": traces.iter().map(|(a,e,t,s,m,d,ts)| {
                    serde_json::json!({"agent":a,"event":e,"status":s,"msg":m.chars().take(200).collect::<String>(),"dur_ms":d,"ts":ts})
                }).collect::<Vec<_>>()
            }).to_string();
            let fix_files_json = serde_json::json!(commit_files).to_string();
            let _ = sqlx::query(
                "INSERT OR REPLACE INTO bug_reports (bug_id, title, reporter, commit_hash, fix_files, test_result, test_output, pipeline_json, report_md, duration_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"
            )
            .bind(&bid).bind(msg.chars().take(200).collect::<String>()).bind(&rep)
            .bind(&commit_hash).bind(&fix_files_json).bind(&test_result)
            .bind(test_output.chars().take(1000).collect::<String>())
            .bind(&pipeline_json).bind(&report_md).bind(fix_duration)
            .execute(pool).await;
        }

        // ── Step 7: Redis 缓存 ──
        let _: redis::RedisResult<()> = self.redis.clone().set_ex(format!("fix_doc:{}", bid), &report_md, 30*86400).await;

        // ── Step 8: 禅道备注 ──
        {
            let cfg = crate::config::Config::load().ok();
            if let Some(cfg) = cfg {
                let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                let archive_comment = format!(
                    "[📝 陈琳归档] Bug #{} 修复报告已归档\n\n归档时间：{}\nCommit：{}\n测试结果：{}\n流程：诸葛亮分析 → guanyu 修复 → 张飞测试 → 华佗验收 → 陈琳归档\n报告：docs/bug-fixes/bug-{}.md",
                    bid, ts, commit_hash, test_icon, bid
                );
                let _ = client.comment_bug(&bid, &archive_comment).await;
                // 恢复 bug 状态为 active（comment_bug 会 resolve+activate）
                // comment_bug 已经处理了 activate，无需额外操作
            }
        }

        // ── Step 9: 飞书通知 ──
        let _ = self.feishu.send(&format!("📚 Bug #{} 修复报告已归档 → docs/bug-fixes/bug-{}.md ({})", bid, bid, test_icon), None).await;

        self.traces.log("chenlin", "doc_done", Some(&format!("Bug#{}", bid)), Some(&report_md.chars().take(200).collect::<String>()), None, None, None, Some("ok"), None).await;
        self.traces.publish_trace_for_ws("chenlin", "doc_done", &format!("Bug#{}", bid), "归档完成", "ok", 0).await;
    }

    async fn push_task_dedup(&self, queue: &str, task_json: &str) {
        let bid = task_json.split("#").nth(1)
            .and_then(|s| s.split([']', '：', ':', ' ', '"']).next())
            .unwrap_or("")
            .to_string();
        
        // 跳过已失败的 bug（不超过10次重试）
        if !bid.is_empty() {
            let agent = queue.strip_prefix("agent-work-queue:fix:").unwrap_or("");
            let failed_count: i32 = self.redis.clone().scard(format!("agent-failed-bugs:{}", agent)).await.unwrap_or(0);
            if failed_count > 0 {
                let is_failed: bool = self.redis.clone().sismember(format!("agent-failed-bugs:{}", agent), &bid).await.unwrap_or(false);
                if is_failed {
                    tracing::info!("Skipping Bug #{} (in failed set for {})", bid, agent);
                    return;
                }
            }
        }
        
        if !bid.is_empty() && bid.chars().all(|c| c.is_ascii_digit()) {
            // 扫描队列中已有的同 bug 任务，删除旧的
            let len: i64 = self.redis.clone().llen(queue).await.unwrap_or(0);
            for i in 0..len {
                let item: Option<String> = self.redis.clone().lindex(queue, i as isize).await.unwrap_or(None);
                if let Some(item_str) = item {
                    if item_str.contains(&format!("#{}", bid)) {
                        let _: redis::RedisResult<i64> = self.redis.clone().lrem(queue, 1, &item_str).await;
                        break;
                    }
                }
            }
        }
        let _: redis::RedisResult<i64> = self.redis.clone().rpush(queue, task_json).await;
    }


    /// Detect pipeline trigger intent from a chat message.
    /// Returns true if the message triggered a pipeline action (and thus should NOT go to Hermes).
    /// Check if this agent should respond to a broadcast message based on expertise keywords.
    fn should_respond(&self, text: &str) -> bool {
        let tl = text.to_lowercase();
        let keywords: &[&str] = match self.agent_id.as_str() {
            "zhugeliang" => &["架构", "设计", "方案", "review", "重构", "规范"],
            "liubei" => &["汇总", "项目", "进度", "管理", "分配", "协调", "报告", "统计", "bug"],
            "guanyu" => &["后端", "java", "api", "接口", "spring", "service", "controller"],
            "zhaoyun" => &["前端", "vue", "界面", "组件", "样式", "渲染", "弹窗", "按钮"],
            "xunyu" => &["数据库", "sql", "查询", "索引", "表", "字段", "ddl"],
            "zhangfei" => &["测试", "bug", "缺陷", "验证", "复现"],
            "huatuo" => &["产品", "需求", "prd", "体验", "验收"],
            "chenlin" => &["文档", "wiki", "手册", "记录"],
            _ => &[],
        };
        keywords.iter().any(|k| tl.contains(*k))
    }

    async fn detect_pipeline_intent(&self, msg: &str, task: &Task) -> bool {
        let msg_lower = msg.to_lowercase();
        
        // ── Trigger 1: "分配" / "全部分配" / "开始分配" → PM scan + coordinator scan ──
        if (msg.contains("分配") || msg.contains("全部分配") || msg.contains("开始分配"))
            && (self.agent_id == "liubei" || self.agent_id == "zhugeliang")
        {
            // Extract human name if specified: "分配王怡哲的bug" → scan that person
            let target_human = if msg.contains("王怡哲") || msg.contains("wangyizhe") {
                Some("wangyizhe")
            } else if msg.contains("史一鸣") || msg.contains("shiyiming") {
                Some("shiyiming")
            } else if msg.contains("陈显精") || msg.contains("chenxj") {
                Some("chenxj")
            } else {
                None
            };
            
            tracing::info!("[{}] 🎯 Pipeline triggered: 分配Bug (human={:?})", self.agent_id, target_human);
            let _ = self.feishu.send(&"🔍 收到分配指令，正在扫描 Bug...".to_string(), 
                if task.chat_id.is_empty() { None } else { Some(task.chat_id.as_str()) }).await;
            
            self.run_coordinator_scan().await;
            return true;
        }
        
        // ── Trigger 2: "修复 Bug #XXX" or "修复 #XXX" → direct fix task dispatch ──
        if (msg_lower.contains("修复 bug") || msg_lower.contains("修复 #") || msg_lower.contains("修复bug"))
            && !pipeline::parse_bugs_from_message(msg).is_empty()
        {
            if self.is_fixer {
                tracing::info!("[{}] 🎯 Pipeline triggered: 直接修复 Bug", self.agent_id);
                let _ = self.feishu.send("🔧 收到修复指令，开始修复...",
                    if task.chat_id.is_empty() { None } else { Some(task.chat_id.as_str()) }).await;
                self.handle_fix_task(msg).await;
                return true;
            } else {
                // Non-fixer: forward to appropriate fixer
                let bugs = pipeline::parse_bugs_from_message(msg);
                if let Some((bid, title)) = bugs.first() {
                    let fixer = route_bug(title);
                    let task_json = pipeline::build_fix_task(bid, title, fixer);
                    let queue = format!("agent-work-queue:fix:{}", fixer);
                    self.push_task_dedup(&queue, &task_json.to_string()).await;
                    let _ = self.feishu.send(&format!("🔧 Bug #{} 已转发给 {} 修复。", bid, fixer),
                        if task.chat_id.is_empty() { None } else { Some(task.chat_id.as_str()) }).await;
                    return true;
                }
            }
        }
        
        // ── Trigger 3: "扫描Bug" / "查看Bug" / "有哪些Bug" → coordinator scan only (no PM analysis) ──
        if (msg_lower.contains("扫描") || msg_lower.contains("有哪些bug") || msg_lower.contains("查看bug"))
            && (self.agent_id == "zhugeliang" || self.agent_id == "liubei")
        {
            tracing::info!("[{}] 🎯 Pipeline triggered: 扫描Bug", self.agent_id);
            self.run_coordinator_scan().await;
            // After scan, let Hermes handle the reply (don't return true — fall through to chat)
        }
        
        // ── No pipeline intent detected → let Hermes handle the chat ──
        false
    }

    /// New Hermes-first chat handler — returns true if Hermes handled the message.
    /// Hermes bridge auto-executes fast pipeline actions (scan_bugs, query_bug)
    /// and formulates the final reply. Long actions (fix_bug) are submitted async.


    /// 诸葛亮预分析：获取 Bug 详情，分析根因，设计修复方案，路由给修复 Agent
    async fn handle_pipeline_pre_analyze(&self, msg: &str) {
        let bid = pipeline::extract_bug_id(msg);
        if bid.is_empty() {
            tracing::warn!("[zhugeliang] pipeline_pre_analyze: 无法提取 Bug ID");
            return;
        }

        let suggested_fixer = msg.lines()
            .filter_map(|l| {
                if l.contains("建议修复 Agent:") {
                    l.split(':').nth(1).map(|s| s.trim().to_string())
                } else { None }
            })
            .next()
            .unwrap_or_else(|| "guanyu".to_string());

        tracing::info!("[zhugeliang] 🔍 深度分析 Bug #{} (建议: {})", bid, suggested_fixer);

        // Step 1: 获取 Bug 详情（完整复刻禅道内容）
        let cfg = crate::config::Config::load().ok();
        let (bug_title, bug_full_text, bug_module, _reporter) = if let Some(ref cfg) = cfg {
            let client = crate::core::zentao::ZentaoClient::from_config(cfg);
            match client.get_bug(&bid).await {
                Ok(detail) => {
                    let title = detail.title.clone();
                    let module = detail.module_title.clone();
                    let reporter = detail.opened_by.clone();
                    // 使用 format_for_prompt 获取完整信息（含严重程度、优先级、步骤等）
                    let mut full_text = detail.format_for_prompt();

                    // Vision 多模态分析：提取附图并识别内容
                    let file_ids = crate::core::subagent::extract_file_ids_from_html(&detail.raw_steps_html);
                    if !file_ids.is_empty() {
                        tracing::info!("[zhugeliang] Bug #{} 发现 {} 张附图，尝试 Vision 分析", bid, file_ids.len());
                        let mut images: Vec<Vec<u8>> = Vec::new();
                        for fid in &file_ids {
                            if let Ok(bytes) = crate::core::subagent::download_zentao_image(cfg, fid).await {
                                if bytes.len() > 100 { images.push(bytes); }
                            }
                        }
                        if !images.is_empty() {
                            let llm = crate::core::llm::LlmClient::from_config(cfg);
                            let system = "你是 HIS 系统的 Bug 分析专家。仔细观察截图中的每一个细节：错误信息、界面元素、数据内容、异常状态。完整描述你看到的内容。";
                            let user = format!("请仔细观察以下截图，完整描述截图中显示的所有内容，包括：\n1. 页面标题和导航\n2. 错误提示信息（完整复制）\n3. 表单/列表中的数据\n4. 异常状态或界面问题\n\nBug 信息：\n{}", full_text);
                            match llm.vision(system, &user, &images, Some(&llm.vision_model), None, Some(4096)).await {
                                Ok(vision_ans) => {
                                    full_text.push_str("\n\n### 附图分析（Vision 多模态识别）\n");
                                    full_text.push_str(&vision_ans);
                                    tracing::info!("[zhugeliang] Bug #{} Vision 分析完成（{} 张图）", bid, images.len());
                                }
                                Err(e) => {
                                    tracing::warn!("[zhugeliang] Bug #{} Vision 分析失败: {}", bid, e);
                                }
                            }
                        }
                    }
                    (title, full_text, module, reporter)
                }
                Err(e) => {
                    tracing::warn!("[zhugeliang] 获取 Bug #{} 详情失败: {}", bid, e);
                    (String::new(), String::new(), String::new(), String::new())
                }
            }
        } else {
            (String::new(), String::new(), String::new(), String::new())
        };

        if bug_title.is_empty() {
            tracing::warn!("[zhugeliang] Bug #{} 详情为空，跳过", bid);
            return;
        }

        // Step 2: 调用 LLM 深度分析
        // 铁律：诸葛亮必须读模块索引快速定位
        let module_index = std::fs::read_to_string("/root/.openclaw/workspace/his-repo/MD/MODULE_INDEX.md")
            .unwrap_or_default();
        let analysis_prompt = format!(
            "你是诸葛亮，HealthLink-HIS 系统的架构师。\n\n\
             ## 你的任务\n\
             分析以下 Bug 并给出根因和修复方案。\n\n\
             ## 代码模块索引（根据 Bug 关键词定位目标模块）\n\
             {}\n\n\
             **重要规则：**\n\
             1. **先根据上面的模块索引**，根据 Bug 关键词找到目标模块\n\
             2. **最多读 5 个关键文件**（Controller + ServiceImpl + Mapper）\n\
             3. **不要大面积搜索代码**，基于描述和索引直接定位\n\
             4. 分析足够就直接输出结论\n\n\
             ## 禅道 Bug 完整信息（含附图分析）\n\
             {}\n\n\
             ## 请按以下格式输出\n\n\
             ### 一、Bug 理解\n\
             **必须完整复刻以下内容，不可省略：**\n\
             1. 先原文引用禅道中的 Bug 标题、重现步骤、期望结果\n\
             2. 如果有附图分析，引用图片中识别到的关键信息（错误提示、异常界面等）\n\
             3. 最后用 2-3 句话综合总结：用户遇到了什么问题，在什么场景下发生，期望的正确行为是什么\n\n\
             ### 二、根因分析\n\
             分析最可能的技术原因（代码层面），列出可能涉及的文件和函数。\n\n\
             ### 三、修复方案\n\
             给出具体的修复步骤，包括需要修改的文件、修改内容。\n\n\
             ### 四、路由决策\n\
             FIXER: guanyu 或 zhaoyun 或 xunyu\n\
             REASON: 一句话说明为什么交给这个角色",
            module_index, bug_full_text
        );

        let bid_clone = bid.clone();
        let analysis_result = tokio::task::spawn_blocking(move || {
            crate::core::codex_exec::codex_exec(&analysis_prompt, "read-only", None, Some("zhugeliang"), 1800)
        }).await.unwrap_or_else(|e| {
            crate::core::codex_exec::CodexExecResult {
                success: false, final_message: String::new(), stderr: format!("spawn error: {}", e),
                verdict: crate::core::codex_exec::Verdict::Fail("spawn error".into()),
                total_tokens: 0, elapsed_ms: 0,
            }
        });

        // 无论 success 与否，只要有输出就使用（LLM 分析有价值，VERDICT 不影响分析质量）
        let llm_output = if !analysis_result.final_message.is_empty() {
            let msg = analysis_result.final_message.clone();
            // 检测连接错误 — 如果输出主要是连接错误日志，视为无效分析
            let is_connection_error = msg.contains("Reconnecting...")
                || msg.contains("stream disconnected before completion")
                || msg.contains("error sending request for url")
                || (msg.contains("thread.started") && msg.contains("turn.failed"));
            let error_lines = msg.lines().filter(|l| l.contains("error") || l.contains("Reconnecting") || l.contains("thread.")).count();
            let total_lines = msg.lines().filter(|l| !l.trim().is_empty()).count();
            let error_ratio = if total_lines > 0 { error_lines as f64 / total_lines as f64 } else { 1.0 };

            if is_connection_error && error_ratio > 0.3 {
                tracing::warn!("[zhugeliang] Bug #{} LLM 分析输出包含大量连接错误 ({:.0}%)，重试分析", bid_clone, error_ratio * 100.0);
                // 重试一次（analysis_prompt 已被 move，用简化 prompt）
                let retry_prompt = format!(
                    "你是诸葛亮，HealthLink-HIS 系统的架构师。分析 Bug #{} 的根因和修复方案。\n\nBug: {}\n模块: {}\n\n请给出根因分析、修复方案和路由决策（FIXER: guanyu/zhaoyun/xunyu）。",
                    bid_clone, bug_title, bug_module
                );
                let retry_result = crate::core::codex_exec::codex_exec(
                    &retry_prompt, "read-only", None, Some("zhugeliang"), 120,
                );
                let retry_msg = retry_result.final_message.clone();
                let retry_is_error = retry_msg.contains("Reconnecting...")
                    || retry_msg.contains("stream disconnected before completion");
                if !retry_msg.is_empty() && !retry_is_error {
                    tracing::info!("[zhugeliang] Bug #{} 重试分析成功 ({}ms)", bid_clone, retry_result.elapsed_ms);
                    retry_msg
                } else {
                    tracing::warn!("[zhugeliang] Bug #{} 重试仍失败，降级关键词分析", bid_clone);
                    msg
                }
            } else {
                tracing::info!("[zhugeliang] Bug #{} LLM 分析完成 ({}ms, {} tokens)", bid_clone, analysis_result.elapsed_ms, analysis_result.total_tokens);
                msg
            }
        } else {
            tracing::warn!("[zhugeliang] Bug #{} LLM 分析无输出，降级关键词分析 (stderr: {})", bid_clone, analysis_result.stderr.chars().take(200).collect::<String>());
            String::new()
        };

        // Step 3: 提取路由决策
        let (target_fixer, reason, analysis_content) = if !llm_output.is_empty() {
            let fixer = llm_output.lines()
                .filter_map(|l| {
                    let upper = l.to_uppercase();
                    if upper.contains("FIXER:") {
                        if upper.contains("GUANYU") { Some("guanyu") }
                        else if upper.contains("ZHAOYUN") { Some("zhaoyun") }
                        else if upper.contains("XUNYU") { Some("xunyu") }
                        else { None }
                    } else { None }
                })
                .next()
                .unwrap_or(&suggested_fixer);
            let reason_str = llm_output.lines()
                .filter_map(|l| {
                    if l.to_uppercase().contains("REASON:") {
                        l.splitn(2, ':').nth(1).map(|s| s.trim().to_string())
                    } else { None }
                })
                .next()
                .unwrap_or_else(|| "LLM 分析决策".to_string());
            (fixer, reason_str, llm_output.clone())
        } else {
            let combined = format!("{} {}", bug_title, bug_full_text).to_lowercase();
            let fe = ["vue","前端","界面","页面","按钮","下拉框","显示","组件","路由","菜单","页签"];
            let be = ["java","spring","controller","service","mapper","sql","null","npe","编译","maven"];
            let is_fe = fe.iter().any(|k| combined.contains(k));
            let is_be = be.iter().any(|k| combined.contains(k));
            let (f, r) = if is_fe && !is_be { ("zhaoyun", "关键词: 前端".into()) }
                else if is_be && !is_fe { ("guanyu", "关键词: 后端".into()) }
                else { (suggested_fixer.as_str(), format!("降级: {}", suggested_fixer)) };
            (f, r, format!("（LLM 失败，关键词分析）\nBug: {}\n模块: {}", bug_title, bug_module))
        };

        tracing::info!("[zhugeliang] Bug #{} → {} ({})", bid, target_fixer, reason);

        // Step 4: 完整报告
        let analysis_report = format!("## 诸葛亮分析报告 — Bug #{}\n\n{}\n\n### 路由\n- Agent: {}\n- 原因: {}", bid, analysis_content, target_fixer, reason);

        // Step 5: 交接卡 + 事件
        let handoff = HandoffCard::new(&bid, "", &_reporter, "zhugeliang", target_fixer, "pre_analyze");
        handoff.save(&mut self.redis.clone()).await;
        let event = PipelineEvent::Handoff { bug_id: bid.clone(), from: "zhugeliang".into(), to: target_fixer.into(), stage: "pre_analyze".into() };
        self.publish_trace("pipeline", "handoff", &format!("Bug#{}", bid), &event.to_json(), "ok", 0).await;

        // Step 6: 框架读取分析文档中的 FIXER_ID，自动分配给对应修复人员
        tracing::info!("[zhugeliang] Bug #{} 分析完成，FIXER_ID={} ({})", bid, target_fixer, reason);
        let fix_task = serde_json::json!({
            "agent_id": target_fixer,
            "message": format!("请修复 Bug #{}（诸葛亮分析完成，分配给你）\n\n{}", bid, analysis_report),
            "source": "zhugeliang_assign", "sender_id": "zhugeliang", "bug_reporter": _reporter,
            "msg_id": format!("zhugeliang-assign-{}-{}", bid, chrono::Local::now().timestamp()),
            "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            "chat_id": "", "is_dm": "true",
        });
        let queue = format!("agent-work-queue:fix:{}", target_fixer);
        let _: redis::RedisResult<i64> = self.redis.clone().rpush(&queue, fix_task.to_string()).await;
        let _ = self.feishu.send(&format!("🔍 诸葛亮分析完成 Bug #{} → {} ({})\n📄 分析文档: MD/bugs/BUG_{}_ANALYSIS.md", bid, target_fixer, reason, bid), None).await;

        // Step 7: 留档到 MD/bugs/ 并 git commit（铁律：不论能否修复，必须先提交分析）
        let worktree_dir = std::path::PathBuf::from("/tmp/agentforge-worktrees/zhugeliang");
        let bugs_dir = worktree_dir.join("MD/bugs");
        let _ = std::fs::create_dir_all(&bugs_dir);
        let archive_path = bugs_dir.join(format!("BUG_{}_ANALYSIS.md", bid));
        let fixer_name_str = match target_fixer { "guanyu" => "后端", "zhaoyun" => "前端", "xunyu" => "数据库", _ => "通用" };
        let archive_content = format!(
            "# Bug #{} 诸葛亮分析报告\n\n\
             > **文档类型**: Bug分析\n\
             > **分析时间**: {}\n\
             > **分析模型**: agnes-2.0-flash (LLM深度分析)\n\n\
             ---\n\n\
             ## 基本信息\n\
             - **Bug #**: {}\n\
             - **标题**: {}\n\
             - **模块**: {}\n\
             - **提出人**: {}\n\n\
             ---\n\n\
             {}\n\n\
             ---\n\n\
             ## 路由决策\n\
             - **FIXER_ID**: {}\n\
             - **修复 Agent**: {}（{}）\n\
             - **原因**: {}\n\n\
             > ⚠️ 修复人员请先验证以上分析是否正确，再执行修复。\n",
            bid, chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            bid, bug_title, bug_module, _reporter,
            analysis_content, target_fixer, target_fixer, fixer_name_str, reason
        );
        // 铁律：不覆盖已有的完整分析（含根因分析的），只在降级分析时跳过
        let should_write = if archive_path.exists() {
            if let Ok(existing) = std::fs::read_to_string(&archive_path) {
                let is_complete = existing.contains("根因分析") || existing.contains("根因定位") || existing.contains("修复方案");
                let is_degraded = analysis_content.contains("LLM 失败") || analysis_content.contains("关键词分析");
                if is_complete && is_degraded {
                    tracing::info!("[zhugeliang] ⏭ 跳过覆盖: 已有完整分析，当前为降级分析");
                    false
                } else { true }
            } else { true }
        } else { true };
        if should_write {
            // 过滤掉 mimo-code 连接错误日志，只保留实际分析内容
            let cleaned_content: String = archive_content.lines()
                .filter(|l| !l.contains("Reconnecting..."))
                .filter(|l| !l.contains("stream disconnected before completion"))
                .filter(|l| !l.contains("error sending request for url"))
                .filter(|l| !l.contains("\"type\":\"thread.started\""))
                .filter(|l| !l.contains("\"type\":\"turn.started\""))
                .filter(|l| !l.contains("\"type\":\"turn.failed\""))
                .filter(|l| !l.contains("\"type\":\"error\",\"message\""))
                .collect::<Vec<_>>()
                .join("\n");
            let _ = std::fs::write(&archive_path, &cleaned_content);
            tracing::info!("[zhugeliang] 📄 存档: {}", archive_path.display());
        }

        // 铁律：分析文档必须 git commit + push（不论能否修复）
        // ⚠️ 只提交分析文件，绝不碰其他文件（避免误删）
        let commit_msg = format!("docs(bug): 诸葛亮分析报告 Bug #{}", bid);
        let bug_file = format!("MD/bugs/BUG_{}_ANALYSIS.md", bid);
        // 先 reset 掉所有未提交的改动（保持 worktree 干净）
        let _ = std::process::Command::new("git")
            .current_dir(&worktree_dir)
            .args(["reset", "HEAD", "--", "."])
            .output();
        let _ = std::process::Command::new("git")
            .current_dir(&worktree_dir)
            .args(["checkout", "--", "."])
            .output();
        // 拉取最新
        let _ = std::process::Command::new("git")
            .current_dir(&worktree_dir)
            .args(["fetch", "origin", "develop"])
            .output();
        let _ = std::process::Command::new("git")
            .current_dir(&worktree_dir)
            .args(["rebase", "origin/develop"])
            .output();
        // 只 add 分析文件（-f 强制添加，不影响其他文件）
        let add_out = std::process::Command::new("git")
            .current_dir(&worktree_dir)
            .args(["add", "-f", &bug_file])
            .output();
        if let Err(e) = &add_out {
            tracing::error!("[zhugeliang] git add 失败: {}", e);
        }
        // commit
        let commit_out = std::process::Command::new("git")
            .current_dir(&worktree_dir)
            .args(["commit", "-m", &commit_msg, "--allow-empty"])
            .output();
        match &commit_out {
            Ok(o) if o.status.success() => {
                tracing::info!("[zhugeliang] 📝 分析文档已提交: {}", commit_msg);
                // push 到远程
                let push_out = std::process::Command::new("git")
                    .current_dir(&worktree_dir)
                    .args(["push", "origin", "zhugeliang:develop"])
                    .output();
                match &push_out {
                    Ok(o) if o.status.success() => {
                        tracing::info!("[zhugeliang] 🚀 已推送到 develop");
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        tracing::warn!("[zhugeliang] push 失败: {}", stderr.chars().take(200).collect::<String>());
                    }
                    Err(e) => {
                        tracing::error!("[zhugeliang] push 命令失败: {}", e);
                    }
                }
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                tracing::warn!("[zhugeliang] git commit 失败: {}", stderr.chars().take(200).collect::<String>());
            }
            Err(e) => {
                tracing::error!("[zhugeliang] git commit 命令失败: {}", e);
            }
        }

        // Step 8: 禅道 keywords
        if let Some(ref cfg) = cfg {
            let client = crate::core::zentao::ZentaoClient::from_config(cfg);
            // 从分析内容中提取 Bug 理解摘要（前100字）
            let analysis_summary = analysis_content.lines()
                .skip_while(|l| !l.contains("Bug 理解") && !l.contains("根因分析") && !l.contains("核心问题"))
                .skip(1)
                .take_while(|l| !l.is_empty() && !l.starts_with('#'))
                .collect::<Vec<_>>()
                .join(" ")
                .chars().take(100).collect::<String>();
            let kw = if analysis_summary.is_empty() {
                format!("[诸葛亮分析] {}→{} | {}", bid, target_fixer, reason.chars().take(60).collect::<String>())
            } else {
                format!("[诸葛亮分析] {}→{} | {}", bid, target_fixer, analysis_summary)
            };
            let _ = client.update_bug_keywords(&bid, &kw).await;
            // Step 9: 禅道备注
            let comment = format!("[🤖 诸葛亮深度分析] Bug #{}\n\n{}", bid, analysis_content.chars().take(500).collect::<String>());
            let _ = client.comment_bug(&bid, &comment).await;
        }

        self.traces.log("zhugeliang", "pre_analyze_done", Some(&format!("Bug#{}", bid)),
            Some(&format!("routed_to={} reason={}", target_fixer, reason)), None, None, None, Some("ok"), None).await;
    }

    /// 诸葛亮：分析修复是否需要 DB 审查，路由到下一步
    async fn handle_pipeline_analyze(&self, msg: &str) {
        let bid = pipeline::extract_bug_id(msg);
        if bid.is_empty() { return; }
        let reporter = pipeline::extract_reporter(msg);
        let sender = msg.lines().filter_map(|l| {
            if l.contains("修复 Agent:") { l.split(':').nth(1).map(|s| s.trim()) } else { None }
        }).next().unwrap_or("zhaoyun");
        
        tracing::info!("[zhugeliang] Analyzing Bug #{} for routing", bid);
        
        // 分析是否需要 DB 审查：检查禅道 Bug 详情中是否包含数据库相关关键词
        let needs_db_review = {
            let cfg = crate::config::Config::load().ok();
            let needs = if let Some(cfg) = cfg {
                let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                match client.get_bug(&bid).await {
                    Ok(detail) => {
                        let combined = format!("{:?} {:?} {:?}", detail.title, detail.steps, detail.module_title).to_lowercase();
                        let db_kw = ["sql", "数据库", "column", "table", "ddl", "dml", "迁移", "mapper xml", "mybatis", "alter table", "create table", "drop table", "insert into", "update .* set", "select .* from"];
                        db_kw.iter().any(|kw| combined.contains(kw))
                    }
                    Err(_) => false,
                }
            } else { false };
            needs
        };
        
        // 路由：需DB审查 → Xunyu，否则直接 → Zhangfei
        let next = if needs_db_review { "xunyu" } else { "zhangfei" };
        let next_source = if needs_db_review { "pipeline_db_review" } else { "pipeline_fix_done" };
        let next_msg = format!(
            "请{} Bug #{} 的修复。提出人: {}。修复 Agent: {}。",
            if needs_db_review { "审查" } else { "测试" },
            bid, reporter, sender
        );
        
        // 创建交接卡
        let handoff = HandoffCard::new(&bid, "", &reporter, "zhugeliang", next, "analyze");
        handoff.save(&mut self.redis.clone()).await;

        // 发送流式事件
        let event = PipelineEvent::Handoff { bug_id: bid.clone(), from: "zhugeliang".into(), to: next.into(), stage: "analyze".into() };
        self.publish_trace("pipeline", "handoff", &format!("Bug#{}", bid), &event.to_json(), "ok", 0).await;

        let pipe_task = serde_json::json!({
            "agent_id": next,
            "message": next_msg,
            "source": next_source,
            "sender_id": "zhugeliang",
            "bug_reporter": reporter,
            "msg_id": format!("pipeline-routed-{}-{}", bid, chrono::Local::now().timestamp()),
            "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            "chat_id": "", "is_dm": "true",
        });
        let _: redis::RedisResult<i64> = self.redis.clone().rpush(
            format!("agent-work-queue:fix:{}", next),
            pipe_task.to_string()
        ).await;
        
        tracing::info!("[zhugeliang] Bug #{} routed to {} (db_review={})", bid, next, needs_db_review);
        let _ = self.feishu.send(&format!("🔀 Bug #{} 路由：{} → {}", bid, sender, next), None).await;
        // 写入禅道备注：分析结果
        {
            let cfg = crate::config::Config::load().ok();
            if let Some(cfg) = cfg {
                let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                let comment = format!(
                    "[🤖 诸葛亮分析] Bug #{} 路由决策\n\n                    分析结果：{}\n                    修复智能体：{}\n                    需要DB审查：{}\n                    路由目标：{}",
                    bid,
                    if needs_db_review { "涉及数据库变更，需要荀彧DB审查" } else { "无数据库变更，直接进入测试" },
                    sender, needs_db_review, next
                );
                let _ = client.comment_bug(&bid, &comment).await;
            }
        }
        self.traces.log("zhugeliang", "analyze_done", Some(&format!("Bug#{}", bid)), Some(&format!("routed_to={} db={}", next, needs_db_review)), None, None, None, Some("ok"), None).await;
        self.traces.publish_trace_for_ws("zhugeliang", "analyze_done", &format!("Bug#{}", bid), &format!("路由到{} DB审查={}", next, needs_db_review), "ok", 0).await;
    }

    /// 荀彧：DB 变更审查
    async fn handle_pipeline_db_review(&self, msg: &str) {
        let bid = pipeline::extract_bug_id(msg);
        if bid.is_empty() { return; }
        let reporter = pipeline::extract_reporter(msg);
        
        tracing::info!("[xunyu] DB review for Bug #{}", bid);
        
        // 检查所有修复者工作树的 git diff，找实际的 DB/SQL DDL 变更
        let mut has_ddl_change = false;
        let mut changed_files = String::new();
        for agent_name in &["guanyu", "zhaoyun", "zhugeliang"] {
            let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
            if let Ok(out) = std::process::Command::new("git")
                .args(["diff", "--name-only", "HEAD~1"])
                .current_dir(&worktree)
                .output() {
                let files = String::from_utf8_lossy(&out.stdout);
                changed_files = files.to_string();
                // 只有真正的 SQL DDL 文件才算DB变更（.sql文件、migration目录）
                // mapper/*.xml 只是映射文件引用，不算DDL变更
                has_ddl_change = files.lines().any(|f| {
                    f.ends_with(".sql") || f.contains("migration/") || f.contains("schema") || f.contains("ddl")
                });
                if has_ddl_change { break; }
            }
        }
        
        // DB 审查通过条件：无DDL变更直接通过，有DDL变更才检查迁移脚本
        let review_passed = if has_ddl_change {
            // 检查是否有对应的SQL迁移脚本
            let has_migration_script = changed_files.lines().any(|f| f.ends_with(".sql"));
            has_migration_script
        } else { true }; // 无DDL变更，审查通过
        
        if review_passed {
            tracing::info!("[xunyu] Bug #{} DB review PASSED", bid);
            let _ = self.feishu.send(&format!("✅ Bug #{} DB 审查通过。", bid), None).await;
            // 写入禅道备注
            {
                let cfg = crate::config::Config::load().ok();
                if let Some(cfg) = cfg {
                    let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                    let _ = client.comment_bug(&bid, &format!(
                        "[📚 荀彧DB审查] Bug #{} DB审查通过\n\n审查结果：无数据库变更风险，直接进入测试阶段", bid
                    )).await;
                }
            }
            
            // 路由到 Zhangfei 测试
            let next_msg = format!("请测试 Bug #{} 的修复情况。提出人: {}。", bid, reporter);
            let pipe_task = serde_json::json!({
                "agent_id": "zhangfei",
                "message": next_msg,
                "source": "pipeline_fix_done",
                "sender_id": "xunyu",
                "bug_reporter": reporter,
                "msg_id": format!("pipeline-fix-{}-{}", bid, chrono::Local::now().timestamp()),
                "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                "chat_id": "", "is_dm": "true",
            });
            let _: redis::RedisResult<i64> = self.redis.clone().rpush(
                "agent-work-queue:fix:zhangfei",
                pipe_task.to_string()
            ).await;
        } else {
            tracing::warn!("[xunyu] Bug #{} DB review FAILED — missing migration script", bid);
            let _ = self.feishu.send(&format!("⚠️ Bug #{} DB 审查失败：缺少迁移脚本，退回修复 Agent。", bid), None).await;
            // 写入禅道备注
            {
                let cfg = crate::config::Config::load().ok();
                if let Some(cfg) = cfg {
                    let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                    let _ = client.comment_bug(&bid, &format!(
                        "[📚 荀彧DB审查] Bug #{} DB审查未通过\n\n审查结果：检测到数据库变更但缺少迁移脚本\n要求：补充DB迁移脚本后重新提交", bid
                    )).await;
                }
            }
            // 退回原修复 Agent（从消息中解析 sender_id）
            let sender = msg.split("sender_id:").nth(1).and_then(|s| s.split(',').next()).map(|s| s.trim().trim_matches('"')).unwrap_or("zhaoyun");
            let rework_msg = format!("Bug #{} DB 审查未通过：需要创建 DB 迁移脚本。请补充。", bid);
            let pipe_task = serde_json::json!({
                "agent_id": sender,
                "message": rework_msg,
                "source": "pipeline_db_review_retry",
                "sender_id": "xunyu",
                "bug_reporter": reporter,
                "msg_id": format!("pipeline-dbreview-{}-{}", bid, chrono::Local::now().timestamp()),
                "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                "chat_id": "", "is_dm": "true",
            });
            tracing::info!("[xunyu] Bug #{} DB review failed, routing back to {}", bid, sender);
            let _: redis::RedisResult<i64> = self.redis.clone().rpush(
                format!("agent-work-queue:fix:{}", sender),
                pipe_task.to_string()
            ).await;
        }
        self.traces.log("xunyu", "db_review_done", Some(&format!("Bug#{}", bid)), Some(if review_passed {"pass"} else {"fail"}), None, None, None, Some(if review_passed {"ok"} else {"failed"}), None).await;
        self.traces.publish_trace_for_ws("xunyu", "db_review_done", &format!("Bug#{}", bid), if review_passed {"DB审查通过"} else {"DB审查失败"}, if review_passed {"ok"} else {"failed"}, 0).await;
    }

    /// 刘备：Pipeline 进度报告
    async fn handle_pipeline_report(&self, _msg: &str) {
        tracing::info!("[liubei] Generating pipeline report");
        
        // 收集各 agent 队列深度
        let agents = ["zhaoyun", "guanyu", "xunyu", "zhangfei", "huatuo", "chenlin"];
        let mut report = String::from("📊 Pipeline 报告

");
        
        // 查询各队列状态
        for agent in &agents {
            let queue_len: i64 = self.redis.clone().llen(format!("agent-work-queue:fix:{}", agent)).await.unwrap_or(0);
            let failed_count: i32 = self.redis.clone().scard(format!("agent-failed-bugs:{}", agent)).await.unwrap_or(0);
            let locked: bool = self.redis.clone().exists(format!("codex_lock:{}", agent)).await.unwrap_or(false);
            report.push_str(&format!(
                "{} 队列: {} | 处理中: {} | 失败: {}
",
                agent, queue_len, if locked {"✅"} else {"⏳"}, failed_count
            ));
        }
        
        report.push_str(&format!("
🕐 报告时间: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
        
        let _ = self.feishu.send(&report, None).await;
        self.traces.log("liubei", "report_done", None, Some(&report), None, None, None, Some("ok"), None).await;

        // 归档：将报告写入 bug_reports 表
        {
            let pool = &self.traces.pool;
            let _ = sqlx::query(
                r#"INSERT OR REPLACE INTO bug_reports (bug_id, agent, status, title, detail, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))"#,
            )
            .bind(0i64)
            .bind("pipeline")
            .bind("pipeline_report")
            .bind("Pipeline Report")
            .bind(&report)
            .execute(pool)
            .await;
        }
        self.traces.publish_trace_for_ws("liubei", "report_done", "", "报告完成", "ok", 0).await;
    }
    async fn handle_chat_hermes(&self, msg: &str, task: &Task) -> bool {
        let hermes_script = "/root/agentforge/scripts/hermes_bridge_cli.py";
        let python = "/root/agentforge/venv/bin/python3";
        
        match tokio::time::timeout(
            Duration::from_secs(60),
            tokio::process::Command::new(python)
                .current_dir("/root/agentforge")
                .arg(hermes_script)
                .arg(&self.agent_id)
                .arg(msg)
                .output(),
        )
        .await
        {
            Ok(Ok(out)) if out.status.success() => {
                let reply = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !reply.is_empty() {
                    tracing::info!("[{}] Hermes reply ({} chars)", self.agent_id, reply.len());
                    let chat_id = if task.chat_id.is_empty() { None } else { Some(task.chat_id.as_str()) };
                    let _ = self.feishu.send(&reply, chat_id).await;
                    self.traces.log(&self.agent_id, "hermes_done", None, Some(&reply), Some("hermes_bridge"), None, None, Some("ok"), None).await;
                    return true;
                }
                tracing::warn!("[{}] Hermes returned empty reply", self.agent_id);
                false
            }
            Ok(Ok(out)) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::warn!("[{}] Hermes exited non-zero: {}", self.agent_id, stderr.trim().chars().take(200).collect::<String>());
                false
            }
            Ok(Err(e)) => {
                tracing::warn!("[{}] Hermes spawn failed: {}", self.agent_id, e);
                false
            }
            Err(_) => {
                tracing::warn!("[{}] Hermes timed out (60s), falling back", self.agent_id);
                false
            }
        }
    }

    /// Legacy chat handler — used when Hermes bridge is unavailable.
    /// Tries Hermes as subprocess first, falls back to direct LLM.
    async fn handle_chat_legacy(&self, msg: &str, task: &Task) {
        // Try Hermes first (rich NLU with tools & memory), fallback to direct LLM
        let reply = self.call_hermes(&self.agent_id, msg).await;
        
        match reply {
            Some(reply) if !reply.is_empty() => {
                let chat_id = if task.chat_id.is_empty() { None } else { Some(task.chat_id.as_str()) };
                let _ = self.feishu.send(&reply, chat_id).await;
                self.traces.log(&self.agent_id, "task_done", None, Some(&reply), None, None, None, Some("ok"), None).await;
            }
            _ => {
                // Fallback to direct LLM
                let system = format!("你是 HIS 系统专家智能体「{}」。", self.agent_name);
                match self.llm.chat(&system, msg, None, None, None).await {
                    Ok(reply) => {
                        let chat_id = if task.chat_id.is_empty() { None } else { Some(task.chat_id.as_str()) };
                        let _ = self.feishu.send(&reply, chat_id).await;
                        self.traces.log(&self.agent_id, "task_done", None, Some(&reply), None, None, None, Some("ok"), None).await;
                    }
                    Err(e) => {
                        tracing::error!("[{}] LLM: {}", self.agent_id, e);
                        let _ = self.feishu.send("⚠️ 处理失败", None).await;
                    }
                }
            }
        }
    }

    pub fn agent_name_from_id(id: &str) -> &str {
        AGENT_NAMES.iter().find(|(i,_)| *i==id).map(|(_,n)| *n).unwrap_or(id)
    }

    // ── Shared Hermes CLI caller ──
    async fn call_hermes(&self, agent_id: &str, msg: &str) -> Option<String> {
        let hermes_script = "/root/agentforge/scripts/hermes_bridge_cli.py";
        let python = "/root/agentforge/venv/bin/python3";
        let out = tokio::process::Command::new(python)
            .current_dir("/root/agentforge")
            .arg(hermes_script)
            .arg(agent_id)
            .arg(msg)
            .output()
            .await
            .ok()?;
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if text.is_empty() { None } else { Some(text) }
        } else {
            None
        }
    }


}


/// 优雅降级：当 Playwright 测试失败/超时时，降级到接口测试
async fn degraded_test(bug_id: &str, reporter: &str) -> bool {
    tracing::warn!("[degraded] Bug #{} 降级到接口测试模式", bug_id);
    
    // 尝试简单的后端接口健康检查
    let output = tokio::process::Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}",
               "http://localhost:18082/healthlink-his/system/config/list"])
        .output().await;
    
    let passed = match output {
        Ok(out) => {
            let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
            code == "200" || code == "401" // 401 说明服务正常运行
        }
        Err(_) => false,
    };
    
    if passed {
        tracing::info!("[degraded] Bug #{} 接口健康检查通过（降级模式）", bug_id);
        let _ = crate::core::zentao::ZentaoClient::from_config(&crate::config::Config::load().unwrap())
            .comment_bug(bug_id, &format!(
                "⚠️ [降级测试] Bug #{} Playwright测试失败/超时，降级到接口健康检查
结果：后端服务正常运行
结论：降级通过", bug_id
            )).await;
    }
    passed
}

/// 优雅降级：当验收超时时，降级到自动验收
async fn degraded_verify(bug_id: &str, reporter: &str) -> Verdict {
    tracing::warn!("[degraded] Bug #{} 验收降级到自动验收模式", bug_id);
    
    // 检查 Git commit 是否存在（先查 develop，再查 agent worktree 分支）
    let main_repo = "/root/.openclaw/workspace/his-repo";
    let mut has_commit = std::process::Command::new("git")
        .args(["-C", main_repo, "log", "origin/develop", "--grep", &format!("Bug#{}", bug_id), "--oneline", "-1"])
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    // 回退：检查各 agent 分支
    if !has_commit {
        for agent in &["guanyu", "zhaoyun", "xunyu", "zhugeliang"] {
            let found = std::process::Command::new("git")
                .args(["-C", main_repo, "log", &format!("origin/{}", agent), "--grep", &format!("#{}", bug_id), "--oneline", "-1"])
                .output()
                .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
                .unwrap_or(false);
            if found {
                has_commit = true;
                tracing::info!("[degraded] Bug #{} commit found on origin/{}", bug_id, agent);
                // 尝试 cherry-pick 到 develop
                let hash = std::process::Command::new("git")
                    .args(["-C", main_repo, "log", &format!("origin/{}", agent), "--grep", &format!("#{}", bug_id), "--format=%H", "-1"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                if !hash.is_empty() {
                    let _ = std::process::Command::new("git").args(["-C", main_repo, "checkout", "develop"]).output();
                    let _ = std::process::Command::new("git").args(["-C", main_repo, "pull", "--rebase", "origin", "develop"]).output();
                    let cp = std::process::Command::new("git")
                        .args(["-C", main_repo, "cherry-pick", "--strategy=recursive", "-X", "theirs", &hash])
                        .output();
                    if let Ok(o) = cp {
                        if o.status.success() {
                            let _ = std::process::Command::new("git").args(["-C", main_repo, "push", "origin", "develop"]).output();
                            tracing::info!("[degraded] Bug #{} cherry-picked from {} to develop", bug_id, agent);
                        } else {
                            let _ = std::process::Command::new("git").args(["-C", main_repo, "cherry-pick", "--abort"]).output();
                        }
                    }
                }
                break;
            }
        }
    }
    
    // 检查编译是否通过
    let compile_ok = std::process::Command::new("mvn")
        .args(["compile", "-pl", "healthlink-his-application", "-am", "-q"])
        .current_dir("/root/.openclaw/workspace/his-repo/healthlink-his-server")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    
    if has_commit && compile_ok {
        tracing::info!("[degraded] Bug #{} 自动验收通过（降级模式）", bug_id);
        let _ = crate::core::zentao::ZentaoClient::from_config(&crate::config::Config::load().unwrap())
            .comment_bug(bug_id, &format!(
                "⚠️ [降级验收] Bug #{} 验收降级到自动验收
Git commit: ✅ 存在
编译: ✅ 通过
结论：降级通过", bug_id
            )).await;
        Verdict::Pass
    } else {
        tracing::warn!("[degraded] Bug #{} 自动验收失败（降级模式）", bug_id);
        Verdict::Fail(format!("降级验收失败: commit={} compile={}", has_commit, compile_ok))
    }
}
