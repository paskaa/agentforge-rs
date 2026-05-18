//! SQLite trace store — records agent activity for analytics and debugging.

use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};
use std::path::Path;

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct Trace {
    pub ts: String,
    pub agent_id: String,
    pub event: String,
    pub task_id: Option<String>,
    pub message: Option<String>,
    pub tool: Option<String>,
    pub model: Option<String>,
    pub duration_ms: Option<i64>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AgentSummary {
    pub agent_id: String,
    pub event: String,
    pub cnt: i64,
}

pub struct TraceStore {
    pool: SqlitePool,
}

impl TraceStore {
    /// Open (or create) the trace database at `path`.
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/var/lib/agentforge")))?;

        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS traces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts TEXT NOT NULL DEFAULT (datetime('now')),
                agent_id TEXT NOT NULL,
                event TEXT NOT NULL,
                task_id TEXT,
                message TEXT,
                tool TEXT,
                model TEXT,
                duration_ms INTEGER,
                status TEXT
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_traces_agent_ts ON traces(agent_id, ts)",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    /// Log a trace event.
    pub async fn log(
        &self,
        agent_id: &str,
        event: &str,
        task_id: Option<&str>,
        message: Option<&str>,
        tool: Option<&str>,
        model: Option<&str>,
        duration_ms: Option<i64>,
        status: Option<&str>,
    ) {
        let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
        let _ = sqlx::query(
            "INSERT INTO traces (ts, agent_id, event, task_id, message, tool, model, duration_ms, status) VALUES (?,?,?,?,?,?,?,?,?)",
        )
        .bind(&ts)
        .bind(agent_id)
        .bind(event)
        .bind(task_id)
        .bind(message)
        .bind(tool)
        .bind(model)
        .bind(duration_ms)
        .bind(status)
        .execute(&self.pool)
        .await;
    }

    /// Query recent traces.
    pub async fn query(&self, limit: i64) -> Vec<Trace> {
        sqlx::query_as::<_, Trace>(
            "SELECT ts, agent_id, event, task_id, message, tool, model, duration_ms, status FROM traces ORDER BY ts DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }

    /// Agent summary (events per agent).
    pub async fn agent_summary(&self) -> Vec<AgentSummary> {
        sqlx::query_as::<_, AgentSummary>(
            "SELECT agent_id, event, COUNT(*) as cnt FROM traces GROUP BY agent_id, event ORDER BY agent_id, cnt DESC",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }

    /// Recent errors.
    pub async fn recent_errors(&self, limit: i64) -> Vec<Trace> {
        sqlx::query_as::<_, Trace>(
            "SELECT ts, agent_id, event, task_id, message, tool, model, duration_ms, status FROM traces WHERE status = 'error' OR status = 'failed' ORDER BY ts DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }
}
