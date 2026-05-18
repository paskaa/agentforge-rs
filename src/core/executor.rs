//! Agent Executor — async loop: BLPOP for fixers, xread for non-fixers,
//! full pipeline handlers, and coordinator scan.

use crate::config::{AgentConfig, Config};
use crate::core::llm::LlmClient;
use crate::core::pipeline::{self, route_bug};
use crate::core::subagent;
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
const FIXERS: &[&str] = &["zhugeliang","liubei","guanyu","zhaoyun","xunyu","zhangfei","huatuo","chenlin"];
const ALL_AGENTS: &[&str] = &["zhugeliang","liubei","guanyu","zhaoyun","xunyu","zhangfei","huatuo","chenlin"];

pub struct AgentExecutor {
    pub agent_id: String, pub agent_name: String,
    pub redis: redis::aio::MultiplexedConnection,
    pub redis_sync: Arc<Mutex<redis::Connection>>,
    pub llm: LlmClient, pub feishu: FeishuClient,
    pub traces: Arc<TraceStore>,
    fix_stream: String, is_fixer: bool,
    last_coordinator_scan: Instant,
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
        let fix_stream = if is_fixer { format!("agent-work-queue:fix:{}", agent_id) } else { "agent-work-queue".into() };
        let llm = LlmClient::new(&config.llm.api_base, &config.llm.api_key, agent_cfg.model.as_deref().unwrap_or(&config.llm.default_model));
        let feishu = FeishuClient::new(&config.feishu.app_id, &config.feishu.app_secret, &config.feishu.group_chat_id);
        let traces = Arc::new(TraceStore::open(std::path::Path::new("/var/lib/agentforge/traces.db")).await?);
        Ok(Self { agent_id: agent_id.into(), agent_name, redis, redis_sync, llm, feishu, traces, fix_stream, is_fixer,
            last_coordinator_scan: Instant::now(), last_stream_id: "$".into(),
            zentao_dir: "/root/.openclaw/extensions/zentao-token-refresh".into() })
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        tracing::info!("[{}] Started as {} (stream={}, fixer={})", self.agent_id, self.agent_name, self.fix_stream, self.is_fixer);
        // Non-fixer: read from stream without consumer group
        // (simpler than XREADGROUP — just xread with new messages)
        loop {
            if (self.agent_id == "zhugeliang" || self.agent_id == "liubei")
                && self.last_coordinator_scan.elapsed() > Duration::from_secs(300)
            { self.last_coordinator_scan = Instant::now(); self.run_coordinator_scan().await; }


            // For fixers: check per-agent lock BEFORE consuming — avoid task loss
            // Also auto-release stale locks (TTL < 3300 = held >15min)
            if self.is_fixer {
                let my_lock = format!("claude_code_lock:{}", self.agent_id);
                let ttl: i64 = self.redis.clone().ttl(&my_lock).await.unwrap_or(-2);
                if ttl == -2 { /* key doesn't exist — no lock */ }
                else if ttl > 0 && ttl < 3300 {
                    // Lock held >15min (3600-3300=300s) — probably stale, release
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
                "pm_routed" | "coordinator_scan" | "hermes_action" | "hermes_assign" => self.handle_fix_task(msg).await,
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

    async fn blpop_val(&self) -> Option<String> {
        let stream = self.fix_stream.clone();
        let sync = Arc::clone(&self.redis_sync);
        tokio::task::spawn_blocking(move || {
            let mut conn = match sync.lock() { Ok(c) => c, Err(p) => p.into_inner() };
            let (_, val): (String, String) = redis::cmd("BLPOP").arg(&stream).arg(10_i64).query(&mut *conn).ok()?;
            Some(val)
        }).await.unwrap_or(None)
    }

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
                let _: redis::RedisResult<i64> = self.redis.clone().rpush(&queue, task_json.to_string()).await;
            }
        }
        if self.agent_id == "liubei" { self.handle_pm_analyze("请分析和分派所有活跃 Bug").await; }
    }

    async fn handle_pm_analyze(&self, msg: &str) {
        let bugs = pipeline::parse_bugs_from_message(msg);
        if bugs.is_empty() { let _ = self.feishu.send("✅ 暂无需要分派的 Bug。", None).await; return; }
        for (bid, title) in &bugs {
            let fixer = route_bug(title);
            let task_json = pipeline::build_fix_task(bid, title, fixer);
            let queue = format!("agent-work-queue:fix:{}", fixer);
            let _: redis::RedisResult<i64> = self.redis.clone().rpush(&queue, task_json.to_string()).await;
        }
        let reply = format!("✅ 已分析 {} 个 Bug，已分派给对应智能体。", bugs.len());
        let _ = self.feishu.send(&reply, None).await;
        self.traces.log(&self.agent_id, "pm_routed", None, Some(&reply), None, None, None, Some("ok")).await;
    }

    async fn handle_fix_task(&self, msg: &str) {
        let bug_id = pipeline::parse_bugs_from_message(msg).first().map(|(b,_)| b.clone()).unwrap_or_default();
        if bug_id.is_empty() { return; }
        self.traces.log(&self.agent_id, "fix_start", Some(&format!("Bug#{}", bug_id)), Some(msg), Some("claude_code"), None, None, Some("pending")).await;
        // Try to acquire per-agent lock
        let lock_key = format!("claude_code_lock:{}", self.agent_id);
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
        
        let (an, bid, m, tr) = (self.agent_id.clone(), bug_id.clone(), msg.to_string(), Arc::clone(&self.traces));
        let mut redis_clone = self.redis.clone();
        tokio::spawn(async move {
            tracing::info!("[{}] Claude Code spawn started for Bug #{}", an, bid);
            let r = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tokio::task::block_in_place(|| {
                    subagent::run_claude_fix_sync(&an, &bid, &m, "/root/.openclaw/extensions/zentao-token-refresh/claude-code-fix.sh", 10800)
                })
            })) {
                Ok(r) => r,
                Err(panic) => {
                    let msg = if let Some(s) = panic.downcast_ref::<String>() { s.clone() } else { "panic".into() };
                    tracing::error!("[{}] Claude Code panic for #{}: {}", an, bid, msg);
                    let _: redis::RedisResult<()> = redis_clone.del(&format!("claude_code_lock:{}", an)).await;
                    return;
                }
            };
            tracing::info!("[{}] Fix #{}: ok={} changes={} time={}ms", an, bid, r.success, r.changes, r.elapsed_ms);
            tr.log(&an, "fix_done", Some(&format!("Bug#{}", bid)), Some(&r.stdout.chars().take(200).collect::<String>()), Some("claude_code"), None, Some(r.elapsed_ms as i64), Some(if r.success {"ok"} else {"failed"})).await;
            let _: redis::RedisResult<()> = redis_clone.del(&format!("claude_code_lock:{}", an)).await;
            // Kick off pipeline: zhangfei test
            if r.success {
                // Dedup: only trigger pipeline once per bug (Redis key with 24h TTL)
                let pipeline_key = format!("pipeline_sent:{}", bid);
                let already_sent: bool = redis_clone.exists(&pipeline_key).await.unwrap_or(false);
                if !already_sent {
                    let _: redis::RedisResult<()> = redis_clone.set_ex(&pipeline_key, "1", 86400).await;
                    let reporter = pipeline::extract_reporter(&m);
                    let pipe_task = serde_json::json!({
                        "agent_id": "zhangfei",
                        "message": format!("请测试 Bug #{} 的修复情况。提出人: {}。", bid, reporter),
                        "source": "pipeline_fix_done",
                        "sender_id": an,
                        "bug_reporter": reporter,
                        "msg_id": format!("pipeline-fix-{}-{}", bid, chrono::Utc::now().timestamp()),
                        "timestamp": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                        "chat_id": "", "is_dm": "true",
                    });
                    let _: redis::RedisResult<i64> = redis_clone.rpush("agent-work-queue:fix:zhangfei", pipe_task.to_string()).await;
                }
            }
        });
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    async fn handle_pipeline_test(&self, msg: &str) {
        let bid = pipeline::extract_bug_id(msg); let rep = pipeline::extract_reporter(msg);
        tracing::info!("[zhangfei] Testing Bug #{}", bid);
        let test_doc = format!("# 测试文档 — Bug #{}\n\n{}\n\n## 验收标准\n- [ ] 功能正常", bid, msg);
        let _: redis::RedisResult<()> = self.redis.clone().set(format!("test_doc:{}", bid), &test_doc).await;
        if pipeline::is_human(&rep) { let _ = self.feishu.send(&format!("Bug #{} 测试完成（人类 {}，跳过指派）。", bid, rep), None).await; return; }
        let _ = tokio::process::Command::new("bash").arg("-c").arg(format!("{}/zentao-write-bug.sh assign {} {} '验证确认'", self.zentao_dir, bid, rep)).output().await;
        let _ = self.feishu.send(&format!("Bug #{} 测试完成，已指派回 {}。", bid, rep), None).await;
        self.traces.log("zhangfei", "test_done", Some(&format!("Bug#{}", bid)), None, None, None, None, Some("ok")).await;
    }

    async fn handle_pipeline_verify(&self, msg: &str) {
        let bid = pipeline::extract_bug_id(msg); let rep = pipeline::extract_reporter(msg);
        let test_doc: Option<String> = self.redis.clone().get(format!("test_doc:{}", bid)).await.ok();
        if test_doc.is_none() { let _ = self.feishu.send(&format!("⚠️ Bug #{} 验收失败：无测试文档", bid), None).await; return; }
        if pipeline::is_human(&rep) { let _ = self.feishu.send(&format!("Bug #{} 验收通过（人类 {}）。", bid, rep), None).await; return; }
        let _ = tokio::process::Command::new("bash").arg("-c").arg(format!("{}/zentao-write-bug.sh resolve {} '验收通过'", self.zentao_dir, bid)).output().await;
        let _ = tokio::process::Command::new("bash").arg("-c").arg(format!("{}/zentao-write-bug.sh assign {} {} '验收通过'", self.zentao_dir, bid, rep)).output().await;
        let _ = self.feishu.send(&format!("✅ Bug #{} 验收通过。", bid), None).await;
        self.traces.log("huatuo", "verify_done", Some(&format!("Bug#{}", bid)), None, None, None, None, Some("ok")).await;
    }

    async fn handle_chenlin_doc(&self, msg: &str) {
        let bid = pipeline::extract_bug_id(msg);
        let doc = format!("# Bug #{} 修复文档\n\n**时间**: {}\n\n{}\n\n✅ 测试通过 ✅ 验收通过", bid, chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), msg.chars().take(300).collect::<String>());
        let _: redis::RedisResult<()> = self.redis.clone().set_ex(format!("fix_doc:{}", bid), &doc, 30*86400).await;
        let _ = self.feishu.send(&format!("📚 Bug #{} 文档已归档。", bid), None).await;
        self.traces.log("chenlin", "doc_done", Some(&format!("Bug#{}", bid)), None, None, None, None, Some("ok")).await;
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
            let _ = self.feishu.send(&format!("🔍 收到分配指令，正在扫描 Bug..."), 
                if task.chat_id.is_empty() { None } else { Some(task.chat_id.as_str()) }).await;
            
            self.run_coordinator_scan().await;
            return true;
        }
        
        // ── Trigger 2: "修复 Bug #XXX" or "修复 #XXX" → direct fix task dispatch ──
        if (msg_lower.contains("修复 bug") || msg_lower.contains("修复 #") || msg_lower.contains("修复bug"))
            && pipeline::parse_bugs_from_message(msg).first().is_some()
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
                    let _: redis::RedisResult<i64> = self.redis.clone().rpush(&queue, task_json.to_string()).await;
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
                    self.traces.log(&self.agent_id, "hermes_done", None, Some(&reply), Some("hermes_bridge"), None, None, Some("ok")).await;
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
                self.traces.log(&self.agent_id, "task_done", None, Some(&reply), None, None, None, Some("ok")).await;
            }
            _ => {
                // Fallback to direct LLM
                let system = format!("你是 HIS 系统专家智能体「{}」。", self.agent_name);
                match self.llm.chat(&system, msg, None, None, None).await {
                    Ok(reply) => {
                        let chat_id = if task.chat_id.is_empty() { None } else { Some(task.chat_id.as_str()) };
                        let _ = self.feishu.send(&reply, chat_id).await;
                        self.traces.log(&self.agent_id, "task_done", None, Some(&reply), None, None, None, Some("ok")).await;
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