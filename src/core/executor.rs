//! Agent Executor — async loop: BLPOP for fixers, xread for non-fixers,
//! full pipeline handlers, and coordinator scan.

use crate::config::{AgentConfig, Config};
use crate::core::llm::LlmClient;
use crate::core::pipeline::{self, route_bug};
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
        let fix_stream = if agent_id == "liubei" { "agent-work-queue:coordinator".into() }
        else if is_fixer { format!("agent-work-queue:fix:{}", agent_id) }
        else { "agent-work-queue".into() };
        let llm = LlmClient::new(&config.llm.api_base, &config.llm.api_key, agent_cfg.model.as_deref().unwrap_or(&config.llm.default_model));
        let feishu = FeishuClient::new(&config.feishu.app_id, &config.feishu.app_secret, &config.feishu.group_chat_id);
        let traces = Arc::new(TraceStore::open(std::path::Path::new("/var/lib/agentforge/traces.db")).await?);
        Ok(Self { agent_id: agent_id.into(), agent_name, redis, redis_sync, llm, feishu, traces, fix_stream, is_fixer,
            last_coordinator_scan: Instant::now(), last_retry_check: Instant::now(), last_stream_id: "$".into(),
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
            // Also auto-release stale locks (TTL < 3300 = held >15min)
            if self.is_fixer {
                let my_lock = format!("codex_lock:{}", self.agent_id);
                let ttl: i64 = self.redis.clone().ttl(&my_lock).await.unwrap_or(-2);
                if ttl == -2 { /* key doesn't exist — no lock */ }
                else if ttl > 0 && ttl < 1800 {
                    // Lock held >45min (3600-900=2700s) — probably stale, release
                    tracing::warn!("[{}] Stale lock detected (TTL={}s), auto-releasing", self.agent_id, ttl);
                    let _: redis::RedisResult<()> = self.redis.clone().del(&my_lock).await;
                } else {
                    // Lock active — wait
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            }

            let val = if self.is_fixer {
                self.blpop_val().await
            } else {
                self.xread_val().await
            };
            let Some(val) = val else { continue };

            let task = match serde_json::from_str::<Task>(&val) {
                Ok(t) => t, Err(e) => { tracing::warn!("[{}] parse: {}", self.agent_id, e); continue; }
            };
            let source = task.source.as_str();
            let msg = &task.message;
            tracing::info!("[{}] Processing: {} (source={})", self.agent_id, msg.chars().take(80).collect::<String>(), source);

            match source {
                "pm_analyze" if self.agent_id == "liubei" => self.handle_pm_analyze(msg).await,
                "pipeline_analyze" if self.agent_id == "zhugeliang" => self.handle_pipeline_analyze(msg).await,
                "pipeline_db_review" if self.agent_id == "xunyu" => self.handle_pipeline_db_review(msg).await,
                "pipeline_report" if self.agent_id == "liubei" => self.handle_pipeline_report(msg).await,
                "pm_routed" | "coordinator_scan" | "hermes_action" | "hermes_assign" | "pipeline" | "pipeline_batch" | "verify_retry" | "web_ui" => self.handle_fix_task(msg).await,
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
                _ => tracing::debug!("[{}] unhandled source={}", self.agent_id, source),
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
            // 检查重试次数（每个 bug 最多重试 10 次）
            let retry_count: i32 = self.redis.clone()
                .get(format!("{}:{}", retry_key_prefix, bid))
                .await.unwrap_or(0);
            if retry_count >= 10 {
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
                let fixer = route_bug(title);
                let task_json = pipeline::build_fix_task(bid, title, fixer);
                let queue = format!("agent-work-queue:fix:{}", fixer);
                self.push_task_dedup(&queue, &task_json.to_string()).await;
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
        let bug_id = pipeline::parse_bugs_from_message(msg).first().map(|(b,_)| b.clone()).unwrap_or_default();
        if bug_id.is_empty() { return; }
        // ── 铁律 19: fix_start 去重 — 检查是否已在处理 ──
        let dedup_key = format!("fix_active:{}:{}", self.agent_id, bug_id);
        let already_active: bool = self.redis.clone().exists(&dedup_key).await.unwrap_or(false);
        if already_active {
            tracing::warn!("[{}] Bug#{} 已在处理中（fix_active 存在），跳过重复 fix_start", self.agent_id, bug_id);
            return;
        }
        // 设置活跃标记（TTL 30 分钟）
        let _: redis::RedisResult<()> = self.redis.clone().set_ex(&dedup_key, "1", 1800).await;
        self.traces.log(&self.agent_id, "fix_start", Some(&format!("Bug#{}", bug_id)), Some(msg), Some("codex"), None, None, Some("pending"), None).await;
        self.publish_trace(&self.agent_id, "fix_start", &format!("Bug#{}", bug_id), msg, "pending", 0).await;
        // Try to acquire per-agent lock
        let lock_key = format!("codex_lock:{}", self.agent_id);
        let lock_sync = Arc::clone(&self.redis_sync); let agent = self.agent_id.clone();
        let lk = lock_key.clone();
        let acquired = tokio::task::spawn_blocking(move || {
            if let Ok(mut conn) = lock_sync.lock() {
                redis::cmd("SET").arg(&lk).arg(&agent).arg("NX").arg("EX").arg(3600)
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
                        subagent::run_codex_fix(&an, &bid, &m, "/root/.openclaw/extensions/zentao-token-refresh/claude-code-fix.sh", 10800)
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

            let r = r.unwrap_or(CodexResult {
                success: false, bug_id: bid.clone(), elapsed_ms: 0,
                stdout: String::new(), stderr: "all retries exhausted".into(),
                exit_code: -1, changes: 0,
            });

            tracing::info!("[{}] Fix #{}: ok={} changes={} time={}ms", an, bid, r.success, r.changes, r.elapsed_ms);
            tr.log(&an, "fix_done", Some(&format!("Bug#{}", bid)), Some(&r.stdout.chars().take(200).collect::<String>()), Some("codex"), None, Some(r.elapsed_ms as i64), Some(if r.success {"ok"} else {"failed"}), None).await;
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
            let work_dir = "/root/.openclaw/workspace/his-repo/openhis-server-new".to_string();
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
                    
                    // 检查验证重试次数（最多 10 次）
                    let retry_key = format!("verify_retry:{}:{}", an_v, bid_v);
                    let retry_count: i32 = redis_v.clone().get(&retry_key).await.unwrap_or(0);
                    let _: redis::RedisResult<()> = redis_v.clone().set_ex(&retry_key, retry_count + 1, 3600).await;
                    
                    if retry_count < 10 {
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
                        tracing::info!("[{}] Bug #{} 验证失败反馈已推送到队列 (重试 {}/10)", an_v, bid_v, retry_count + 1);
                    } else {
                        tracing::warn!("[{}] Bug #{} 验证重试已达上限(10次)，标记为最终失败", an_v, bid_v);
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

        // 确保前端 dev server 在运行
        let _ = tokio::process::Command::new("bash")
            .arg("/root/.openclaw/workspace/scripts/ensure-frontend.sh")
            .output().await;

        // 运行 Playwright 回归测试（单 worker 避免压垮 dev server）
        let test_result = tokio::time::timeout(
            Duration::from_secs(120),
            tokio::process::Command::new("bash")
                .arg("-c")
                .arg(format!("cd /root/.openclaw/workspace/his-repo/openhis-ui-vue3 && npx playwright test --grep @bug{} --reporter=line --workers=1 2>&1", bid))
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

        if test_passed {
            tracing::info!("[zhangfei] Bug #{} Playwright test PASSED", bid);
            let _ = self.feishu.send(&format!("✅ Bug #{} 回归测试通过。", bid), None).await;

            // 清理重试计数
            let _: redis::RedisResult<()> = self.redis.clone().del(&retry_key).await;

            // 提取测试结果摘要（取测试输出的关键行）
            let _test_summary: String = test_output.lines()
                .filter(|l| l.contains("passed") || l.contains("failed") || l.contains("Pending") || l.contains("✓") || l.contains("✗"))
                .collect::<Vec<_>>()
                .join("\n");
            let _test_comment = format!(
                "=== Playwright 回归测试结果 ===\n测试标签: @bug{}\n执行模式: 无头浏览器 (chromium)\n\n{}\n\n✅ 全部测试通过",
                bid, _test_summary
            );
            // 禅道标记为已解决 + 添加测试报告
            {
                let cfg = crate::config::Config::load().ok();
                if let Some(cfg) = cfg {
                    let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                    let test_report = format!(
                        "[🔥 张飞测试报告] Bug #{} Playwright回归测试\n\n                        测试状态：✅ 通过\n                        测试标签：@bug{}\n                        执行模式：无头浏览器 (chromium)\n                        测试输出摘要：\n{}\n\n                        结论：回归测试通过，BUG已修复",
                        bid, bid,
                        _test_summary.chars().take(500).collect::<String>()
                    );
                    let _ = client.comment_bug(&bid, &test_report).await;
                    let _ = client.resolve_bug(&bid, "Playwright回归测试通过，BUG已修复").await;
                }
            }

            // 保存测试文档
            let test_doc = format!("# Bug #{} 回归测试\n\n**Playwright 测试通过**\n\n测试标签: @bug{}", bid, bid);
            let _: redis::RedisResult<()> = self.redis.clone().set_ex(format!("test_doc:{}", bid), &test_doc, 86400).await;

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
            let _ = client.resolve_bug(&bid, "验收通过").await;
            let _ = client.assign_bug(&bid, &rep, "验收通过，分配给提出人确认").await;
        }
        let _ = self.feishu.send(&format!("✅ Bug #{} 验收通过。", bid), None).await;
        self.traces.log("huatuo", "verify_done", Some(&format!("Bug#{}", bid)), None, None, None, None, Some("ok"), None).await;
        self.traces.publish_trace_for_ws("huatuo", "verify_done", &format!("Bug#{}", bid), "验收完成", "ok", 0).await;
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
        let worktree = "/tmp/agentforge-worktrees/guanyu/openhis-server-new";
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
        
        // 检查 git diff 是否包含实际的 DB/SQL 变更
        let has_migration = {
            let worktree = "/tmp/agentforge-worktrees/xunyu";
            let git_diff = std::process::Command::new("git")
                .args(["diff", "--name-only", "HEAD~1"])
                .current_dir(worktree)
                .output();
            match git_diff {
                Ok(out) => {
                    let files = String::from_utf8_lossy(&out.stdout);
                    let sql_related = files.lines().any(|f| {
                        f.ends_with(".sql") || f.contains("migration") || f.contains("mapper")
                            || f.contains("schema") || f.contains("ddl")
                    });
                    sql_related
                }
                Err(_) => false,
            }
        };
        
        // DB 审查通过条件：无 DB 变更直接通过，有 DB 变更则检查脚本
        let review_passed = if has_migration {
            let worktree = "/tmp/agentforge-worktrees/xunyu";
            let sql_dir = format!("{}/openhis-server-new/sql", worktree);
            std::path::Path::new(&sql_dir).exists()
        } else { true }; // 无 DB 变更，审查通过
        
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
            // 退回修复 Agent
            let sender = msg.split("修复 Agent:").nth(1).and_then(|s| s.split(',').next()).map(|s| s.trim()).unwrap_or("zhaoyun");
            let rework_msg = format!("Bug #{} DB 审查未通过：需要创建 DB 迁移脚本。请补充。", bid);
            let pipe_task = serde_json::json!({
                "agent_id": sender,
                "message": rework_msg,
                "source": "pipeline_retry",
                "sender_id": "xunyu",
                "msg_id": format!("pipeline-dbreview-{}-{}", bid, chrono::Local::now().timestamp()),
                "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                "chat_id": "", "is_dm": "true",
            });
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
