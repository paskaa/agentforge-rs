//! Web server — serves the dashboard SPA + REST API endpoints.

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct AppState {
    pub pool: Option<SqlitePool>,
    pub scores_path: String,
}

#[derive(Serialize, Default)]
struct HealthResp { ok: bool, version: String, agents: usize }

#[derive(Serialize, Default)]
struct DashResp {
    stats: Stats,
    agents: Vec<AgentSt>,
    recent: Vec<FixRow>,
}

#[derive(Serialize, Default)]
struct Stats { total: i64, fixed_today: i64, running: i64, rate: String }

#[derive(Serialize, Default)]
struct AgentSt { id: String, status: String, rate: String, avg_s: String }

#[derive(Serialize, Default)]
struct FixRow { bug: String, agent: String, ok: bool, dur: String, ts: String }

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

async fn health() -> impl IntoResponse {
    Json(HealthResp { ok: true, version: env!("CARGO_PKG_VERSION").into(), agents: running_count() })
}

async fn dashboard(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let mut r = DashResp::default();
    r.stats.running = running_count() as i64;

    if let Some(ref pool) = s.pool {
        // Each query individually wrapped — no panic
        let q1 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM traces WHERE event LIKE 'fix%'")
            .fetch_one(pool).await;
        if let Ok(v) = q1 { r.stats.total = v; }

        let q2 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM traces WHERE event = 'fix_done' AND status = 'ok'")
            .fetch_one(pool).await;
        let ok_count = q2.unwrap_or(0);

        let q3 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM traces WHERE event = 'fix_done'")
            .fetch_one(pool).await;
        let total_done = q3.unwrap_or(0);
        r.stats.fixed_today = ok_count;
        r.stats.rate = if total_done > 0 {
            format!("{:.0}%", ok_count as f64 / total_done as f64 * 100.0)
        } else { "N/A".into() };

        // Agents
        let ids = ["guanyu","zhaoyun","xunyu","zhangfei","huatuo","chenlin","liubei","zhugeliang"];
        for id in &ids {
            let fc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM traces WHERE agent_id = ?1 AND event = 'fix_done'")
                .bind(id).fetch_one(pool).await.unwrap_or(0);
            let sc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM traces WHERE agent_id = ?1 AND event = 'fix_done' AND status = 'ok'")
                .bind(id).fetch_one(pool).await.unwrap_or(0);
            let avg: f64 = sqlx::query_scalar("SELECT COALESCE(AVG(duration_ms),0) FROM traces WHERE agent_id = ?1 AND event = 'fix_done'")
                .bind(id).fetch_one(pool).await.unwrap_or(0.0);
            let rate = if fc > 0 { sc as f64 / fc as f64 * 100.0 } else { 0.0 };
            r.agents.push(AgentSt {
                id: id.to_string(),
                status: if locked(id) { "working".into() } else { "idle".into() },
                rate: format!("{:.0}%", rate),
                avg_s: format!("{:.0}s", avg / 1000.0),
            });
        }

        // Recent fixes
        let rows: Result<Vec<(String,String,String,f64,String)>,_> = sqlx::query_as(
            "SELECT COALESCE(task_id,'?'), agent_id, COALESCE(status,'?'), COALESCE(duration_ms,0), COALESCE(ts,'') FROM traces WHERE event = 'fix_done' ORDER BY ts DESC LIMIT 20"
        ).fetch_all(pool).await;
        if let Ok(rows) = rows {
            for (bid,aid,st,dur,ts) in rows {
                r.recent.push(FixRow {
                    bug: bid.replace("Bug#",""), agent: aid, ok: st == "ok",
                    dur: format!("{:.0}s", dur / 1000.0), ts,
                });
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

pub async fn start_web_server(pool: Option<SqlitePool>, port: u16) -> anyhow::Result<()> {
    let state = Arc::new(AppState { pool, scores_path: "/var/lib/agentforge/agent_scores.json".into() });
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "/root/agentforge-rs/static".into());

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/dashboard", get(dashboard))
        .route("/api/analytics", get(analytics_api))
        .route("/api/scores", get(scores_api))
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("🌐 Dashboard: http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
