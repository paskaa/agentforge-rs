//! Web server — dashboard SPA + REST API + WebSocket real-time push.

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
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
}

// ── REST types ──

#[derive(Serialize, Default)]
struct HealthResp { ok: bool, version: String, agents: usize }

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
    std::process::Command::new("redis-cli")
        .args(["-p", "16379", "get", &format!("codex_lock:{}", id)])
        .output()
        .map(|o| {
            let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if v.is_empty() || v == "(nil)" { String::new() } else { v }
        })
        .unwrap_or_default()
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
                    let bug_id = v.get("message").and_then(|m| m.as_str())
                        .and_then(|m| m.strip_prefix("请修复 Bug #"))
                        .and_then(|m| m.split('：').next())
                        .unwrap_or("?").to_string();
                    let agent = v.get("agent_id").and_then(|a| a.as_str()).unwrap_or("?").to_string();
                    let source = v.get("source").and_then(|s| s.as_str()).unwrap_or("pipeline").to_string();
                    items.push(QueueItem { bug_id, agent, source, queued_at: String::new() });
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
                   "agent-work-queue:fix:xunyu", "agent-work-queue:fix:zhangfei"];
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
            let fc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM traces WHERE agent_id = ?1 AND event = 'fix_done'")
                .bind(id).fetch_one(pool).await.unwrap_or(0);
            let sc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM traces WHERE agent_id = ?1 AND event = 'fix_done' AND status = 'ok'")
                .bind(id).fetch_one(pool).await.unwrap_or(0);
            let avg: f64 = sqlx::query_scalar("SELECT COALESCE(AVG(duration_ms),0) FROM traces WHERE agent_id = ?1 AND event = 'fix_done'")
                .bind(id).fetch_one(pool).await.unwrap_or(0.0);
            let rate = if fc > 0 { sc as f64 / fc as f64 * 100.0 } else { 0.0 };
            (format!("{:.0}%", rate), format!("{:.0}s", avg / 1000.0))
        } else { ("N/A".into(), "N/A".into()) };

        r.agents.push(AgentSt {
            id: id.to_string(), name: name.to_string(), role: role.to_string(), icon: icon.to_string(),
            status: if is_locked { "working".into() } else { "idle".into() },
            rate: rate_str, avg_s: avg_str, current_bug: bug,
        });
    }

    // Stats
    if let Some(ref pool) = s.pool {
        if let Ok(v) = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM traces WHERE event LIKE 'fix%'")
            .fetch_one(pool).await { r.stats.total = v; }
        let ok: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM traces WHERE event = 'fix_done' AND status = 'ok'")
            .fetch_one(pool).await.unwrap_or(0);
        let tot: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM traces WHERE event = 'fix_done'")
            .fetch_one(pool).await.unwrap_or(0);
        r.stats.fixed_today = ok;
        r.stats.rate = if tot > 0 { format!("{:.0}%", ok as f64 / tot as f64 * 100.0) } else { "N/A".into() };

        if let Ok(rows) = sqlx::query_as::<_, (String,String,String,f64,String)>(
            "SELECT COALESCE(task_id,'?'), agent_id, COALESCE(status,'?'), COALESCE(duration_ms,0), COALESCE(ts,'') FROM traces WHERE event = 'fix_done' ORDER BY ts DESC LIMIT 20"
        ).fetch_all(pool).await {
            for (bid,aid,st,dur,ts) in rows {
                r.recent.push(FixRow { bug: bid.replace("Bug#",""), agent: aid, ok: st=="ok", dur: format!("{:.0}s",dur/1000.0), ts });
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
    let opt = super::self_optimizer::SelfOptimizer::load(&s.scores_path);
    Json(serde_json::json!({"scores": opt.scores.values().collect::<Vec<_>>()}))
}

// ── Agent traces API ──

#[derive(Serialize)]
struct TraceRow { ts: String, event: String, task_id: String, message: String, status: String, duration_ms: i64 }

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

// ── Queues API ──

#[derive(Serialize)]
struct QueueApiItem { agent: String, queue_len: i64, items: Vec<QueueItem> }

async fn queues_api() -> impl IntoResponse {
    let agent_ids = ["guanyu", "zhaoyun", "xunyu", "zhangfei", "huatuo", "chenlin", "liubei", "zhugeliang"];
    let mut queues: Vec<QueueApiItem> = Vec::new();
    for id in &agent_ids {
        let queue = format!("agent-work-queue:fix:{}", id);
        let len = redis_queue_len(&queue);
        let items = if len > 0 { redis_queue_items(&queue, 10) } else { vec![] };
        queues.push(QueueApiItem { agent: id.to_string(), queue_len: len, items });
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
            "data": { "agents": agents, "queue": queue_total, "ts": chrono::Utc::now().format("%H:%M:%S").to_string() }
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
    });

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "/root/agentforge-rs/static".into());

    // Start background ticker
    tokio::spawn(status_ticker(tx));

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/dashboard", get(dashboard))
        .route("/api/analytics", get(analytics_api))
        .route("/api/scores", get(scores_api))
        .route("/api/agent/:id/traces", get(agent_traces))
        .route("/api/queues", get(queues_api))
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
