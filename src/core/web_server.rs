//! Web server — dashboard SPA + REST API + WebSocket real-time push.

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct AppState {
    pub pool: Option<SqlitePool>,
    pub scores_path: String,
    pub tx: broadcast::Sender<String>,
    pub zentao_cache: Arc<tokio::sync::RwLock<Option<(String, std::time::Instant)>>>,
}

// ── REST types ──

#[derive(Serialize, Default)]
struct HealthResp { ok: bool, version: String, agents: usize }

#[derive(Serialize, Default)]
struct ZentaoStats {
    unclosed: i64,
    unresolved: i64,
    active: i64,
    total: i64,
    fixed_today: i64,
    last_sync: String,
    #[serde(default)]
    bugs: Vec<ZentaoBug>,
    #[serde(default)]
    today_fixed: Vec<ZentaoBug>,
}

#[derive(Serialize, Default, Clone)]
struct ZentaoBug {
    id: i64,
    title: String,
    status: String,
    assigned_to: String,
    severity: String,
    url: String,
    #[serde(default)]
    resolved_date: String,
}

#[derive(Serialize, Default)]
struct DashResp {
    stats: Stats,
    agents: Vec<AgentSt>,
    recent: Vec<FixRow>,
    queue: Vec<QueueItem>,
    dispatcher: DispatcherSt,
}

#[derive(Serialize, Default)]
struct Stats { total: i64, fixed_today: i64, running: i64, rate: String }

#[derive(Serialize, Default)]
struct AgentSt {
    id: String, name: String, role: String, icon: String,
    status: String, rate: String, avg_s: String,
    current_bug: String,
}

#[derive(Serialize, Default)]
struct FixRow { bug: String, agent: String, ok: bool, dur: String, ts: String }

#[derive(Serialize, Default)]
struct QueueItem { bug_id: String, agent: String, source: String, queued_at: String }

#[derive(Serialize, Default)]
struct DispatcherSt { mode: String, active_tasks: i64, redis_queues: i64 }

#[derive(Serialize)]
struct WsEvent { event: String, data: serde_json::Value }

// ── Helpers ──

const AGENT_META: &[(&str, &str, &str, &str)] = &[
    ("guanyu", "关羽", "后端开发", "⚔️"),
    ("zhaoyun", "赵云", "前端开发", "🐉"),
    ("xunyu", "荀彧", "DBA", "📚"),
    ("zhangfei", "张飞", "测试", "🔥"),
    ("huatuo", "华佗", "产品", "💊"),
    ("chenlin", "陈琳", "文档", "📝"),
    ("liubei", "刘备", "项目管理", "👑"),
    ("zhugeliang", "诸葛亮", "架构", "🪶"),
];

fn running_count() -> usize {
    std::fs::read_dir("/tmp/agentforge-worktrees")
        .map(|d| d.filter(|e| e.as_ref().map(|x| x.path().is_dir()).unwrap_or(false)).count())
        .unwrap_or(0)
}

fn locked(id: &str) -> bool {
    std::process::Command::new("redis-cli")
        .args(["-p", "16379", "exists", &format!("codex_lock:{}", id)])
        .output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1").unwrap_or(false)
}

fn current_bug_for(id: &str) -> String {
    // Try Redis key first (set by executor when processing)
    let output = std::process::Command::new("redis-cli")
        .args(["-p", "16379", "get", &format!("current_bug:{}", id)])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if !output.is_empty() && output != "(nil)" {
        return output;
    }
    // Fallback to traces DB
    let db_path = "/var/lib/agentforge/traces.db";
    let output = std::process::Command::new("sqlite3")
        .args([db_path, &format!(
            "SELECT t.task_id FROM traces t WHERE t.agent_id='{}' AND t.event='fix_start' AND t.task_id IS NOT NULL AND t.task_id != '?' AND t.ts > datetime('now', '-2 hours') AND NOT EXISTS (SELECT 1 FROM traces t2 WHERE t2.agent_id=t.agent_id AND t2.task_id=t.task_id AND t2.event='fix_done' AND t2.ts > t.ts) ORDER BY t.ts DESC LIMIT 1", id
        )])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    output
}

fn redis_queue_len(queue: &str) -> i64 {
    std::process::Command::new("redis-cli")
        .args(["-p", "16379", "llen", queue])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0))
        .unwrap_or(0)
}

fn redis_queue_items(queue: &str, limit: usize) -> Vec<QueueItem> {
    let out = std::process::Command::new("redis-cli")
        .args(["-p", "16379", "lrange", queue, "0", &(limit as i64 - 1).to_string()])
        .output();
    match out {
        Ok(o) => {
            let stdout_str = String::from_utf8_lossy(&o.stdout).to_string();
            let lines: Vec<&str> = stdout_str.lines().collect();
            let mut items = Vec::new();
            let mut i = 0;
            while i < lines.len() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(lines[i]) {
                    // Support both {"bug_id":"Bug#630",...} and {"message":"请修复 Bug #630:...",...}
                    let bug_id = v.get("bug_id").and_then(|b| b.as_str())
                        .map(|b| b.trim_start_matches("Bug#").to_string())
                        .or_else(|| v.get("message").and_then(|m| m.as_str())
                            .and_then(|m| m.strip_prefix("请修复 Bug #"))
                            .and_then(|m| {
                                // Split on Chinese colon or regular colon
                                if let Some(pos) = m.find('：') { Some(&m[..pos]) }
                                else if let Some(pos) = m.find(':') { Some(&m[..pos]) }
                                else { Some(m) }
                            })
                            .map(|m| m.trim().to_string()))
                        .unwrap_or("?".to_string());
                    let agent = v.get("agent_id").and_then(|a| a.as_str()).unwrap_or("?").to_string();
                    let source = v.get("source").and_then(|s| s.as_str()).unwrap_or("pipeline").to_string();
                    let queued_at = v.get("queued_at").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    items.push(QueueItem { bug_id, agent, source, queued_at });
                }
                i += 1;
            }
            items
        }
        Err(_) => vec![],
    }
}

// ── Handlers ──

async fn health() -> impl IntoResponse {
    Json(HealthResp { ok: true, version: env!("CARGO_PKG_VERSION").into(), agents: running_count() })
}

async fn dashboard(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let mut r = DashResp::default();
    r.stats.running = running_count() as i64;

    // Queue status
    let queues = ["agent-work-queue:fix:guanyu", "agent-work-queue:fix:zhaoyun",
                   "agent-work-queue:fix:xunyu", "agent-work-queue:fix:zhangfei",
                   "agent-work-queue:fix:huatuo", "agent-work-queue:fix:chenlin",
                   "agent-work-queue:fix:liubei", "agent-work-queue:fix:zhugeliang"];
    let mut total_queue = 0i64;
    for q in &queues {
        let len = redis_queue_len(q);
        total_queue += len;
        if len > 0 {
            let agent = q.split(':').last().unwrap_or("?");
            for item in redis_queue_items(q, 5) {
                r.queue.push(item);
            }
        }
    }
    r.dispatcher = DispatcherSt {
        mode: "Redis Stream + Agent IPC".into(),
        active_tasks: running_count() as i64,
        redis_queues: total_queue,
    };

    // Agent status
    for (id, name, role, icon) in AGENT_META {
        let is_locked = locked(id);
        let bug = if is_locked { current_bug_for(id) } else { String::new() };
        let (rate_str, avg_str) = if let Some(ref pool) = s.pool {
            let fc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM traces WHERE agent_id = ?1 AND event IN ('fix_done','test_done','verify_done','doc_done','analyze_done','pm_routed')")
                .bind(id).fetch_one(pool).await.unwrap_or(0);
            let sc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM traces WHERE agent_id = ?1 AND event IN ('fix_done','test_done','verify_done','doc_done','analyze_done') AND status = 'ok'")
                .bind(id).fetch_one(pool).await.unwrap_or(0);
            let avg: f64 = sqlx::query_scalar("SELECT COALESCE(AVG(duration_ms),0) FROM traces WHERE agent_id = ?1 AND event IN ('fix_done','test_done','verify_done','doc_done','analyze_done')")
                .bind(id).fetch_one(pool).await.unwrap_or(0.0);
            let rate = if fc > 0 { sc as f64 / fc as f64 * 100.0 } else { 0.0 };
            (format!("{:.1}%", rate), format!("{:.0}s", avg / 1000.0))
        } else { ("N/A".into(), "N/A".into()) };

        r.agents.push(AgentSt {
            id: id.to_string(), name: name.to_string(), role: role.to_string(), icon: icon.to_string(),
            status: if is_locked { "working".into() } else { "idle".into() },
            rate: rate_str, avg_s: avg_str, current_bug: bug,
        });
    }

    // Stats — use zentao cache for real bug counts
    {
        let cache = s.zentao_cache.read().await;
        if let Some((ref json, _ts)) = *cache {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
                r.stats.total = v.get("total").and_then(|x| x.as_i64()).unwrap_or(0);
                r.stats.fixed_today = v.get("fixed_today").and_then(|x| x.as_i64()).unwrap_or(0);
                let unc = v.get("unclosed").and_then(|x| x.as_i64()).unwrap_or(0);
                let unres = v.get("unresolved").and_then(|x| x.as_i64()).unwrap_or(0);
                r.stats.rate = if unc > 0 { format!("{:.1}%", (unc - unres) as f64 / unc as f64 * 100.0) } else { "N/A".into() };
            }
        }
    }
    // Fallback: traces-based stats if zentao cache empty
    if r.stats.total == 0 {
        if let Some(ref pool) = s.pool {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let today_pattern = format!("{}%", today);
            if let Ok(v) = sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT task_id) FROM traces WHERE event IN ('fix_start','test_start','verify_start','task_start','pm_routed') AND ts LIKE ?1")
                .bind(&today_pattern).fetch_one(pool).await { r.stats.total = v; }
            let ok: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT task_id) FROM traces WHERE event IN ('fix_done','test_done','verify_done','doc_done','analyze_done') AND status = 'ok' AND ts LIKE ?1")
                .bind(&today_pattern).fetch_one(pool).await.unwrap_or(0);
            let tot: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT task_id) FROM traces WHERE event IN ('fix_done','test_done','verify_done','doc_done','analyze_done') AND ts LIKE ?1")
                .bind(&today_pattern).fetch_one(pool).await.unwrap_or(0);
            r.stats.fixed_today = ok;
            r.stats.rate = if tot > 0 { format!("{:.1}%", ok as f64 / tot as f64 * 100.0) } else { "N/A".into() };
        }
    }

        if let Some(ref pool) = s.pool {
            if let Ok(rows) = sqlx::query_as::<_, (String,String,String,i64,String)>(
                "SELECT COALESCE(task_id,'?'), agent_id, COALESCE(status,'?'), COALESCE(duration_ms,0), COALESCE(ts,'') FROM traces WHERE event IN ('fix_done','test_done','verify_done','doc_done','analyze_done','fix_start','test_start','verify_start','pm_routed') ORDER BY ts DESC LIMIT 50"
            ).fetch_all(pool).await {
                for (bid,aid,st,dur,ts) in rows {
                    r.recent.push(FixRow { bug: bid.replace("Bug#",""), agent: aid, ok: st=="ok", dur: format!("{:.0}s",dur as f64/1000.0), ts });
                }
            }
        }

    Json(r)
}

async fn analytics_api(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(ref pool) = s.pool {
        let report = super::analytics::Analytics::new(pool.clone()).generate_report().await;
        Json(serde_json::to_value(&report).unwrap_or(serde_json::json!({})))
    } else {
        Json(serde_json::json!({"error":"no db"}))
    }
}

async fn scores_api(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let data = std::fs::read_to_string(&s.scores_path).unwrap_or_else(|_| "{}".into());
    let raw: serde_json::Value = serde_json::from_str(&data).unwrap_or(serde_json::json!({}));
    let result = normalize_scores_value(&raw);
    Json(serde_json::json!({"scores": result}))
}

fn normalize_scores_value(raw: &serde_json::Value) -> Vec<serde_json::Value> {
    tracing::debug!("[scores_api] normalize_scores_value called, keys: {:?}", raw.as_object().map(|o| o.keys().collect::<Vec<_>>()));
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return vec![],
    };
    let pairs = [
        ("关羽", "guanyu"), ("赵云", "zhaoyun"),
        ("荙录", "xunyu"), ("张飞", "zhangfei"),
        ("华佮", "huatuo"), ("陈琳", "chenlin"),
        ("刘备", "liubei"), ("诸葛亮", "zhugeliang"),
    ];
    let mut merged: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
    for (k, v) in obj {
        let nk = pairs.iter().find(|(cn, _)| *cn == k.as_str()).map(|(_, py)| *py).unwrap_or(k.as_str());
        let new_score = v.get("overall_score").and_then(|s| s.as_f64()).unwrap_or(0.0);
        let old_score = merged.get(nk).and_then(|e| e.get("overall_score")).and_then(|s| s.as_f64()).unwrap_or(0.0);
        if new_score >= old_score {
            merged.insert(nk.to_string(), v.clone());
        }
    }
    merged.into_values().collect()
}

// ── Agent traces API ──

#[derive(Serialize)]
struct TraceRow { ts: String, event: String, task_id: String, message: String, status: String, duration_ms: i64 }


async fn agent_traces_realtime(
    State(s): State<Arc<AppState>>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(ref pool) = s.pool {
        let rows: Vec<(String,String,String,String,String,i64)> = sqlx::query_as(
            "SELECT COALESCE(ts,''), event, COALESCE(task_id,''), COALESCE(message,''), COALESCE(status,''), COALESCE(duration_ms,0) FROM traces WHERE agent_id = ?1 ORDER BY ts DESC LIMIT 50"
        ).bind(&agent_id).fetch_all(pool).await.unwrap_or_default();
        let traces: Vec<serde_json::Value> = rows.iter().map(|(ts,ev,task,msg,st,dur)| {
            serde_json::json!({"ts":ts,"event":ev,"task_id":task,"message":msg,"status":st,"duration_ms":dur})
        }).collect();
        Json(serde_json::json!({"traces": traces}))
    } else {
        Json(serde_json::json!({"traces": []}))
    }
}

async fn agent_queue_api(
    State(s): State<Arc<AppState>>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let queue = format!("agent-work-queue:fix:{}", agent_id);
    let len = redis_queue_len(&queue);
    let mut items = if len > 0 { redis_queue_items(&queue, 20) } else { vec![] };

    // Check if agent is currently processing a bug
    let current_bug = current_bug_for(&agent_id);
    let is_locked = !current_bug.is_empty();
    if !current_bug.is_empty() {
        // Check if this bug is already in the queue items
        let already_queued = items.iter().any(|i| i.bug_id == current_bug.replace("Bug#",""));
        if !already_queued {
            items.insert(0, QueueItem {
                bug_id: current_bug.replace("Bug#",""),
                agent: agent_id.clone(),
                source: "processing".into(),
                queued_at: "正在处理".into(),
            });
        }
    }

    Json(serde_json::json!({"agent": agent_id, "queue_len": items.len() as i64, "items": items, "processing": is_locked, "current_bug": current_bug}))
}

async fn agent_traces(
    State(s): State<Arc<AppState>>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(ref pool) = s.pool {
        if let Ok(rows) = sqlx::query_as::<_, (String,String,String,String,String,i64)>(
            "SELECT COALESCE(ts,''), event, COALESCE(task_id,''), COALESCE(message,''), COALESCE(status,''), COALESCE(duration_ms,0) FROM traces WHERE agent_id = ?1 ORDER BY ts DESC LIMIT 50"
        ).bind(&agent_id).fetch_all(pool).await {
            let traces: Vec<TraceRow> = rows.into_iter().map(|(ts,event,task_id,message,status,dur)|
                TraceRow { ts, event, task_id, message: message.chars().take(200).collect(), status, duration_ms: dur }
            ).collect();
            return Json(serde_json::json!({"agent_id": agent_id, "traces": traces}));
        }
    }
    Json(serde_json::json!({"agent_id": agent_id, "traces": []}))
}

// ── Zentao Stats API ──

async fn constraints_api() -> impl IntoResponse {
    let path = "/var/lib/agentforge/agent_scores.json.constraints";
    match std::fs::read_to_string(path) {
        Ok(data) => {
            let v: serde_json::Value = serde_json::from_str(&data).unwrap_or(serde_json::json!({}));
            Json(v)
        }
        Err(_) => Json(serde_json::json!({})),
    }
}

async fn zentao_stats_api(
    State(s): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let force_refresh = params.get("refresh").map(|v| v == "true").unwrap_or(false);
    // Check cache (60s TTL) — skip if refresh=true
    if !force_refresh {
        let cache = s.zentao_cache.read().await;
        if let Some((ref json, ts)) = *cache {
            if ts.elapsed() < std::time::Duration::from_secs(60) {
                return Json(serde_json::from_str(json).unwrap_or(serde_json::json!({})));
            }
        }
    }

    // Fetch from Zentao API
    let stats = fetch_zentao_stats(&s.pool).await;
    let json_str = serde_json::to_string(&stats).unwrap_or_default();

    // Update cache
    {
        let mut cache = s.zentao_cache.write().await;
        *cache = Some((json_str.clone(), std::time::Instant::now()));
    }

    Json(serde_json::from_str(&json_str).unwrap_or(serde_json::json!({})))
}

async fn fetch_zentao_stats(_pool: &Option<SqlitePool>) -> ZentaoStats {
    let base_url = "https://zentao.gentronhealth.com";
    let token_file = "/root/.config/zentao/.env";
    let token = std::fs::read_to_string(token_file)
        .map(|s| {
            for line in s.lines() {
                if let Some(val) = line.strip_prefix("ZENTAO_TOKEN=") {
                    return val.trim().to_string();
                }
            }
            String::new()
        })
        .unwrap_or_default();

    if token.is_empty() {
        return ZentaoStats { last_sync: "no token".into(), ..Default::default() };
    }

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    // Fetch all bugs (paginated)
    let mut all_bugs: Vec<serde_json::Value> = Vec::new();
    let mut page = 1;
    loop {
        let url = format!("{}/api.php/v1/products/4/bugs?page={}&limit=100", base_url, page);
        let resp = client.get(&url).header("Token", &token).send().await;
        match resp {
            Ok(r) if r.status().is_success() => {
                if let Ok(body) = r.json::<serde_json::Value>().await {
                    let bugs = body.get("bugs").and_then(|b| b.as_array()).cloned().unwrap_or_default();
                    let total = body.get("total").and_then(|t| t.as_i64()).unwrap_or(0);
                    all_bugs.extend(bugs);
                    if all_bugs.len() as i64 >= total { break; }
                    page += 1;
                } else { break; }
            }
            _ => break,
        }
    }

    let total = all_bugs.len() as i64;
    let mut unclosed = 0i64;
    let mut unresolved = 0i64;
    let mut active = 0i64;
    let today_str = chrono::Local::now().format("%Y-%m-%d").to_string();

    for bug in &all_bugs {
        let status = bug.get("status").and_then(|s| s.as_str()).unwrap_or("");
        if status != "closed" { unclosed += 1; }
        if status == "active" { active += 1; unresolved += 1; }
    }

    let mut today_fixed: Vec<ZentaoBug> = Vec::new();
    let bugs: Vec<ZentaoBug> = all_bugs.iter().map(|b| {
        let assignee = b.get("assignedTo").and_then(|a| {
            if a.is_object() { a.get("name").or(a.get("account")).and_then(|v| v.as_str()).map(String::from) }
            else { a.as_str().map(String::from) }
        }).unwrap_or_default();
        let bug_id = b.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let resolved_date = b.get("resolvedDate").and_then(|v| v.as_str()).unwrap_or("").to_string();
        ZentaoBug {
            id: bug_id,
            title: b.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            status: b.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            assigned_to: assignee,
            severity: {
                    let s = b.get("severity");
                    match s.and_then(|v| v.as_i64()) {
                        Some(1) => "致命".into(),
                        Some(2) => "严重".into(),
                        Some(3) => "重要".into(),
                        Some(4) => "一般".into(),
                        Some(5) => "轻微".into(),
                        Some(n) => format!("{}", n),
                        None => s.and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    }
                },
            url: format!("https://zentao.gentronhealth.com/index.php?m=bug&f=view&bugID={}", bug_id),
            resolved_date: resolved_date.clone(),
        }
    }).collect();

    // Count today's resolved bugs
    for b in &bugs {
        if !b.resolved_date.is_empty() && b.resolved_date.starts_with(&today_str) && b.status == "resolved" {
            today_fixed.push((*b).clone());
        }
    }

    let fixed_today = today_fixed.len() as i64;

    ZentaoStats {
        unclosed, unresolved, active, total, fixed_today,
        last_sync: chrono::Local::now().format("%H:%M:%S").to_string(),
        bugs,
        today_fixed,
    }
}

// ── Queues API ──

#[derive(Serialize)]
struct QueueApiItem { agent: String, queue_len: i64, items: Vec<QueueItem> }

async fn queues_api() -> impl IntoResponse {
    let agent_ids = ["guanyu", "zhaoyun", "xunyu", "zhangfei", "huatuo", "chenlin", "liubei", "zhugeliang"];
    let mut queues: Vec<QueueApiItem> = Vec::new();
    for id in &agent_ids {
        let queue = format!("agent-work-queue:fix:{}", id);
        let len = redis_queue_len(&queue);
        let mut items = if len > 0 { redis_queue_items(&queue, 10) } else { vec![] };

        // Include current processing bug from Redis
        let current_bug = current_bug_for(id);
        if !current_bug.is_empty() {
            let already_queued = items.iter().any(|i| i.bug_id == current_bug.replace("Bug#",""));
            if !already_queued {
                items.insert(0, QueueItem {
                    bug_id: current_bug.replace("Bug#",""),
                    agent: id.to_string(),
                    source: "processing".into(),
                    queued_at: "正在处理".into(),
                });
            }
        }

        queues.push(QueueApiItem { agent: id.to_string(), queue_len: items.len() as i64, items });
    }
    Json(queues)
}

// ── WebSocket ──

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(s): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, s))
}

async fn handle_ws(socket: axum::extract::ws::WebSocket, s: Arc<AppState>) {
    let mut rx = s.tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Send initial state
    let init = serde_json::json!({
        "event": "init",
        "data": { "agents": running_count(), "version": env!("CARGO_PKG_VERSION") }
    });
    let _ = sender.send(axum::extract::ws::Message::Text(init.to_string())).await;

    // Spawn task to forward broadcast events
    tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(axum::extract::ws::Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Read client messages (keep connection alive)
    while let Some(Ok(_)) = receiver.next().await {}
}

// ── Redis subscriber — forward trace events to WebSocket ──

async fn redis_trace_subscriber(tx: broadcast::Sender<String>) {
    let client = match redis::Client::open("redis://127.0.0.1:16379") {
        Ok(c) => c,
        Err(e) => { tracing::warn!("[ws] Redis connect failed: {}", e); return; }
    };
    let mut pubsub = match client.get_async_connection().await {
        Ok(c) => c.into_pubsub(),
        Err(e) => { tracing::warn!("[ws] Redis pubsub failed: {}", e); return; }
    };
    if let Err(e) = pubsub.subscribe("agentforge:traces").await {
        tracing::warn!("[ws] Subscribe failed: {}", e);
        return;
    }
    tracing::info!("[ws] Subscribed to agentforge:traces channel");
    loop {
        match futures_util::StreamExt::next(&mut pubsub.on_message()).await {
            Some(msg) => {
                if let Ok(payload) = msg.get_payload::<String>() {
                    let _ = tx.send(payload);
                }
            }
            None => {
                tracing::warn!("[ws] Redis stream ended, reconnecting...");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                break;
            }
        }
    }
}

// ── Background ticker — push status every 10s ──

async fn status_ticker(tx: broadcast::Sender<String>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
    loop {
        interval.tick().await;
        let agents = running_count();
        let mut queue_total = 0i64;
        for q in &["agent-work-queue:fix:guanyu", "agent-work-queue:fix:zhaoyun",
                     "agent-work-queue:fix:xunyu", "agent-work-queue:fix:zhangfei"] {
            queue_total += redis_queue_len(q);
        }
        let event = serde_json::json!({
            "event": "tick",
            "data": { "agents": agents, "queue": queue_total, "ts": chrono::Local::now().format("%H:%M:%S").to_string() }
        });
        let _ = tx.send(event.to_string());
    }
}

// ── Entrypoint ──

pub async fn start_web_server(pool: Option<SqlitePool>, port: u16) -> anyhow::Result<()> {
    let (tx, _) = broadcast::channel::<String>(64);

    let state = Arc::new(AppState {
        pool,
        scores_path: "/var/lib/agentforge/agent_scores.json".into(),
        tx: tx.clone(),
        zentao_cache: Arc::new(tokio::sync::RwLock::new(None)),
    });


async fn l5_history_api() -> impl IntoResponse {
    let path = "/var/lib/agentforge/l5_optimization_log.json";
    let data = std::fs::read_to_string(path).unwrap_or_else(|_| "[]".into());
    let entries: Vec<serde_json::Value> = serde_json::from_str(&data).unwrap_or_default();
    Json(serde_json::json!({"history": entries}))
}

/// POST /api/execute — 自主 Harness Loop 入口
async fn execute_api(
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let command = payload.get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim().to_string();

    if command.is_empty() {
        return Json(serde_json::json!({"ok": false, "error": "command 不能为空"}));
    }

    // 提取 Bug ID: "Bug #704" / "#704" / "bug704"
    let bug_id: Option<String> = {
        let lower = command.to_lowercase();
        let chars: Vec<char> = command.chars().collect();
        let mut found = None;
        // Find "bug" keyword then digits
        if let Some(pos) = lower.find("bug") {
            let after = &command[pos+3..];
            let after = after.trim_start_matches(|c: char| c == '#' || c == ' ' || c == '：' || c == ':');
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 2 { found = Some(digits); }
        }
        if found.is_none() {
            // Find standalone "#NNN"
            for (i, ch) in chars.iter().enumerate() {
                if *ch == '#' {
                    let digits: String = chars[i+1..].iter().take_while(|c| c.is_ascii_digit()).collect();
                    if digits.len() >= 2 { found = Some(digits); break; }
                }
            }
        }
        found
    };

    let agent_ids = ["guanyu", "zhaoyun", "xunyu", "zhangfei", "huatuo", "chenlin", "liubei", "zhugeliang"];

    let (target_agent, message, source) = if let Some(ref bid) = bug_id {
        let routed = route_by_keywords(&command);
        let best = find_least_queued(routed, &agent_ids);
        (best, format!("请修复 Bug #{}：{}", bid, command), "web_execute".to_string())
    } else {
        ("liubei".to_string(), command.clone(), "web_execute".to_string())
    };

    let task_id = format!("exec-{}-{}", chrono::Local::now().timestamp_millis(), &target_agent[..std::cmp::min(2, target_agent.len())]);
    let queue = format!("agent-work-queue:fix:{}", target_agent);

    let task = serde_json::json!({
        "agent_id": target_agent,
        "message": message,
        "source": source,
        "sender_id": "web_admin",
        "chat_id": "",
        "is_dm": "true",
        "msg_id": task_id,
        "timestamp": chrono::Local::now().to_rfc3339(),
    });

    let out = std::process::Command::new("redis-cli")
        .args(["-p", "16379", "rpush", &queue, &task.to_string()])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("error: {}", e));

    if out.parse::<i64>().unwrap_or(0) > 0 {
        Json(serde_json::json!({
            "ok": true,
            "task_id": task_id,
            "agent": target_agent,
            "queue": queue,
            "message": message,
        }))
    } else {
        Json(serde_json::json!({"ok": false, "error": format!("redis error: {}", out)}))
    }
}

fn route_by_keywords(cmd: &str) -> &'static str {
    let lower = cmd.to_lowercase();
    if lower.contains("前端") || lower.contains("vue") || lower.contains("ui") || lower.contains("页面") || lower.contains("弹窗") || lower.contains("下拉") || lower.contains("列表") {
        "zhaoyun"
    } else if lower.contains("数据库") || lower.contains("sql") || lower.contains("db") || lower.contains("mapper") {
        "xunyu"
    } else if lower.contains("测试") || lower.contains("test") || lower.contains("playwright") {
        "zhangfei"
    } else if lower.contains("后端") || lower.contains("接口") || lower.contains("api") || lower.contains("service") || lower.contains("controller") {
        "guanyu"
    } else {
        "guanyu"
    }
}

fn find_least_queued(preferred: &str, all: &[&str]) -> String {
    let mut best = preferred.to_string();
    let mut min_len = i64::MAX;
    for id in all {
        let queue = format!("agent-work-queue:fix:{}", id);
        let len = redis_queue_len(&queue);
        if len < min_len {
            min_len = len;
            best = id.to_string();
        }
    }
    best
}

async fn enqueue_bug_api(
    State(s): State<Arc<AppState>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let bug_id = payload.get("bug_id").and_then(|v| v.as_i64()).unwrap_or(0);
    if bug_id == 0 {
        return Json(serde_json::json!({"ok": false, "error": "missing bug_id"}));
    }

    // Find an available agent (least queued)
    let agent_ids = ["guanyu", "zhaoyun", "xunyu", "zhangfei", "huatuo", "chenlin", "liubei", "zhugeliang"];
    let mut best_agent = "guanyu";
    let mut min_queue = i64::MAX;
    for id in &agent_ids {
        let queue = format!("agent-work-queue:fix:{}", id);
        let len = redis_queue_len(&queue);
        if len < min_queue {
            min_queue = len;
            best_agent = id;
        }
    }

    let queue = format!("agent-work-queue:fix:{}", best_agent);
    let task = serde_json::json!({
        "agent_id": best_agent,
        "message": format!("请修复 Bug #{}: web_ui 手动入列", bug_id),
        "source": "web_ui",
        "sender_id": "web_admin",
        "msg_id": format!("web_{}", chrono::Local::now().timestamp()),
        "timestamp": chrono::Local::now().to_rfc3339(),
    });

    // Push to Redis
    let out = std::process::Command::new("redis-cli")
        .args(["-p", "16379", "rpush", &queue, &task.to_string()])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("error: {}", e));

    if out.parse::<i64>().unwrap_or(0) > 0 {
        Json(serde_json::json!({"ok": true, "agent": best_agent, "queue": queue}))
    } else {
        Json(serde_json::json!({"ok": false, "error": format!("redis error: {}", out)}))
    }
}


async fn batch_enqueue_api(
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let bug_ids: Vec<i64> = payload.get("bug_ids")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    if bug_ids.is_empty() {
        return Json(serde_json::json!({"ok": false, "error": "no bug_ids", "enqueued": 0}));
    }

    let agent_ids = ["guanyu", "zhaoyun", "xunyu", "zhangfei", "huatuo", "chenlin", "liubei", "zhugeliang"];
    let mut enqueued = 0i64;
    let mut errors = Vec::new();

    for bug_id in &bug_ids {
        // Round-robin to least queued agent
        let mut best_agent = "guanyu";
        let mut min_queue = i64::MAX;
        for id in &agent_ids {
            let queue = format!("agent-work-queue:fix:{}", id);
            let len = redis_queue_len(&queue);
            if len < min_queue {
                min_queue = len;
                best_agent = id;
            }
        }

        let queue = format!("agent-work-queue:fix:{}", best_agent);
        let task = serde_json::json!({
            "agent_id": best_agent,
            "message": format!("请修复 Bug #{}: batch enqueue", bug_id),
            "source": "web_ui",
            "sender_id": "web_admin",
            "msg_id": format!("web_batch_{}_{}", bug_id, chrono::Local::now().timestamp()),
            "timestamp": chrono::Local::now().to_rfc3339(),
        });

        let out = std::process::Command::new("redis-cli")
            .args(["-p", "16379", "rpush", &queue, &task.to_string()])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| format!("error: {}", e));

        if out.parse::<i64>().unwrap_or(0) > 0 {
            enqueued += 1;
        } else {
            errors.push(format!("Bug#{}: {}", bug_id, out));
        }
    }

    Json(serde_json::json!({
        "ok": errors.is_empty(),
        "enqueued": enqueued,
        "total": bug_ids.len(),
        "errors": errors,
    }))
}



async fn bug_verification_api(
    axum::extract::Path(bug_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let db_path = "/var/lib/agentforge/traces.db";
    // Get the latest verification report for this bug
    let output = std::process::Command::new("sqlite3")
        .args([db_path, "-json", &format!(
            "SELECT ts, agent_id, event, COALESCE(message,'') as message, COALESCE(status,'') as status, COALESCE(duration_ms,0) as duration_ms, COALESCE(detail,'null') as detail FROM traces WHERE task_id LIKE '%{}%' AND event IN ('verification','verify_start','verify_done','verify_read_testdoc','verify_diff','test_generated','baseline_test','regression_test','test_done') ORDER BY ts ASC", bug_id
        )])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let traces: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap_or_default();
    
    // Find the full verification report (stored as detail in 'verification' event)
    let full_report = traces.iter()
        .find(|t| t.get("event").and_then(|e| e.as_str()) == Some("verification"))
        .and_then(|t| t.get("detail"))
        .and_then(|d| d.as_str())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    
    Json(serde_json::json!({
        "bug_id": bug_id,
        "traces": traces,
        "full_report": full_report,
        "count": traces.len()
    }))
}


async fn bug_traces_api(
    axum::extract::Path(bug_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let db_path = "/var/lib/agentforge/traces.db";
    let output = std::process::Command::new("sqlite3")
        .args([db_path, "-json", &format!(
            "SELECT ts, agent_id, event, COALESCE(task_id,'') as task_id, COALESCE(message,'') as message, COALESCE(status,'') as status, COALESCE(duration_ms,0) as duration_ms FROM traces WHERE task_id LIKE '%{}%' ORDER BY ts ASC", bug_id
        )])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let traces: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap_or_default();
    Json(serde_json::json!({"bug_id": bug_id, "traces": traces, "count": traces.len()}))
}

async fn bug_report_api(
    axum::extract::Path(bug_id): axum::extract::Path<String>,
    State(s): State<Arc<AppState>>,
) -> impl IntoResponse {
    if let Some(ref pool) = s.pool {
        if let Ok(row) = sqlx::query_as::<_, (i64,String,String,String,String,String,String,i64,String)>(
            "SELECT bug_id, COALESCE(title,''), COALESCE(reporter,''), COALESCE(commit_hash,''), COALESCE(test_result,''), COALESCE(report_md,''), COALESCE(fix_files,'[]'), COALESCE(duration_ms,0), COALESCE(created_at,'') FROM bug_reports WHERE bug_id = ?1"
        ).bind(&bug_id).fetch_optional(pool).await {
            if let Some((bid,title,rep,hash,test,md,files,dur,created)) = row {
                return Json(serde_json::json!({
                    "bug_id": bid, "title": title, "reporter": rep,
                    "commit_hash": hash, "test_result": test,
                    "report_md": md, "fix_files": serde_json::from_str::<serde_json::Value>(&files).unwrap_or(serde_json::json!([])),
                    "duration_ms": dur, "created_at": created
                }));
            }
        }
    }
    Json(serde_json::json!({"error": "not found"}))
}

async fn bug_reports_api(
    State(s): State<Arc<AppState>>,
) -> impl IntoResponse {
    if let Some(ref pool) = s.pool {
        match sqlx::query_as::<_, (i64,String,String,String,String,i64,String)>(
            "SELECT bug_id, COALESCE(title,''), COALESCE(reporter,''), COALESCE(commit_hash,''), COALESCE(test_result,''), COALESCE(duration_ms,0), COALESCE(created_at,'') FROM bug_reports ORDER BY created_at DESC LIMIT 100"
        ).fetch_all(pool).await {
            Ok(rows) => {
            let reports: Vec<serde_json::Value> = rows.iter().map(|(bid,title,rep,hash,test,dur,created)| {
                serde_json::json!({
                    "bug_id": bid, "title": title, "reporter": rep,
                    "commit_hash": hash, "test_result": test,
                    "duration_ms": dur, "created_at": created
                })
            }).collect();
            return Json(serde_json::json!({"reports": reports, "count": reports.len()}));
            }
            Err(e) => {
                tracing::error!("[bug_reports_api] query failed: {}", e);
            }
        }
    }
    Json(serde_json::json!({"reports": [], "count": 0}))
}

async fn deploy_status_api() -> impl IntoResponse {
    // 1. Get his-backend.service start time
    let backend_start = tokio::process::Command::new("systemctl")
        .args(["show", "his-backend.service", "--property=ActiveEnterTimestamp"])
        .output()
        .await
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            s.strip_prefix("ActiveEnterTimestamp=")
                .unwrap_or(&s)
                .trim()
                .to_string()
        })
        .unwrap_or_else(|e| format!("error: {}", e));

    // 2. Get latest develop branch commit time
    let develop_commit_time = tokio::process::Command::new("git")
        .args(["log", "origin/develop", "--format=%ai", "-1"])
        .current_dir("/root/.openclaw/workspace/his-repo")
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("error: {}", e));

    // 3. Get recent 5 commits (time + message)
    let recent_commits_output = tokio::process::Command::new("git")
        .args(["log", "origin/develop", "--format=%ai %s", "-5"])
        .current_dir("/root/.openclaw/workspace/his-repo")
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("error: {}", e));

    let recent_commits: Vec<&str> = recent_commits_output.lines().collect();

    // 4. Determine if deployed: parse both timestamps and compare
    let deployed = parse_timestamp(&backend_start)
        .and_then(|backend_ts| {
            parse_timestamp(&develop_commit_time).map(|commit_ts| backend_ts >= commit_ts)
        })
        .unwrap_or(false);

    Json(serde_json::json!({
        "backend_start": backend_start,
        "develop_commit_time": develop_commit_time,
        "recent_commits": recent_commits,
        "deployed": deployed
    }))
}

/// Parse a datetime string like "Tue 2025-06-02 14:30:00 CST" or "2025-06-02 14:30:00 +0800"
/// into a unix timestamp for comparison.
fn parse_timestamp(s: &str) -> Option<i64> {
    use chrono::NaiveDateTime;
    let s = s.trim();
    // Try parsing "YYYY-MM-DD HH:MM:SS" (possibly with timezone suffix)
    // Strip common prefixes like "Mon ", "Tue ", etc.
    let stripped = if s.len() > 4 && s.as_bytes()[3] == b' ' {
        &s[4..]
    } else {
        s
    };
    // Try "YYYY-MM-DD HH:MM:SS" format
    if let Ok(dt) = NaiveDateTime::parse_from_str(stripped, "%Y-%m-%d %H:%M:%S") {
        return Some(dt.and_utc().timestamp());
    }
    // Try "YYYY-MM-DDTHH:MM:SS" format (ISO)
    if let Ok(dt) = NaiveDateTime::parse_from_str(stripped, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc().timestamp());
    }
    // Try with fractional seconds "YYYY-MM-DD HH:MM:SS.ffffff"
    if let Ok(dt) = NaiveDateTime::parse_from_str(stripped, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(dt.and_utc().timestamp());
    }
    None
}

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "/root/agentforge-rs/static".into());

    // Start background ticker
    tokio::spawn(status_ticker(tx.clone()));
    tokio::spawn(redis_trace_subscriber(tx));

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/dashboard", get(dashboard))
        .route("/api/analytics", get(analytics_api))
        .route("/api/scores", get(scores_api))
        .route("/api/agent/:id/traces", get(agent_traces))
        .route("/api/agent/:id/traces/rt", get(agent_traces_realtime))
        .route("/api/agent/:id/queue", get(agent_queue_api))
                .route("/api/bugs/:id/traces", get(bug_traces_api))
        .route("/api/bugs/:id/report", get(bug_report_api))
        .route("/api/bugs/reports", get(bug_reports_api))
        .route("/api/bugs/:id/verification", get(bug_verification_api))
        .route("/api/queues", get(queues_api))
        .route("/api/zentao/stats", get(zentao_stats_api))
        .route("/api/constraints", get(constraints_api))
        .route("/api/l5/history", get(l5_history_api))
        .route("/api/deploy-status", get(deploy_status_api))
        .route("/api/execute", axum::routing::post(execute_api))
        .route("/api/bugs/enqueue", axum::routing::post(enqueue_bug_api))
        .route("/api/bugs/batch-enqueue", axum::routing::post(batch_enqueue_api))
        .route("/ws", get(ws_handler))
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("🌐 Dashboard: http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
