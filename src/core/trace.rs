//! SQLite trace store — records agent activity for analytics and debugging.

use chrono::Local;
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
    pub pool: SqlitePool,
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
                status TEXT,
                detail TEXT,
                created TEXT DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS bug_reports (
                bug_id INTEGER PRIMARY KEY,
                agent TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'pending',
                title TEXT NOT NULL DEFAULT '',
                detail TEXT NOT NULL DEFAULT '',
                commit_hash TEXT NOT NULL DEFAULT '',
                git_diff TEXT NOT NULL DEFAULT '',
                report_md TEXT NOT NULL DEFAULT '',
                zentao_commented INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')), reporter TEXT NOT NULL DEFAULT '', test_result TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
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

        // Migration: add detail column if missing
        let _ = sqlx::query("ALTER TABLE traces ADD COLUMN detail TEXT").execute(&pool).await;
        let _ = sqlx::query("ALTER TABLE traces ADD COLUMN created TEXT DEFAULT (datetime('now'))").execute(&pool).await;

        Ok(Self { pool })
    }

    /// Log a trace event.
        /// Save a bug report to the bug_reports table (chenlin archival)
    pub async fn save_report(
        &self,
        bug_id: i64,
        agent: &str,
        status: &str,
        title: &str,
        detail: &str,
        commit_hash: &str,
        git_diff: &str,
        report_md: &str,
    ) {
        let _ = sqlx::query(
            r#"INSERT OR REPLACE INTO bug_reports
               (bug_id, agent, status, title, detail, commit_hash, git_diff, report_md, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))"#,
        )
        .bind(bug_id)
        .bind(agent)
        .bind(status)
        .bind(title)
        .bind(detail)
        .bind(commit_hash)
        .bind(git_diff)
        .bind(report_md)
        .execute(&self.pool)
        .await;
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
        detail: Option<&str>,
    ) {
        let ts = Local::now().format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
        let _ = sqlx::query(
            "INSERT INTO traces (ts, agent_id, event, task_id, message, tool, model, duration_ms, status, detail) VALUES (?,?,?,?,?,?,?,?,?,?)",
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

    /// Publish a trace event to Redis pub/sub for WebSocket broadcasting.
    pub async fn publish_trace_for_ws(&self, agent_id: &str, event: &str, task_id: &str, message: &str, status: &str, duration_ms: i64) {
        let trace_event = serde_json::json!({
            "event": "trace",
            "data": {
                "ts": Local::now().format("%Y-%m-%dT%H:%M:%S%.6f").to_string(),
                "agent_id": agent_id,
                "event": event,
                "task_id": task_id,
                "message": message.chars().take(200).collect::<String>(),
                "status": status,
                "duration_ms": duration_ms,
            }
        });
        let client = redis::Client::open("redis://127.0.0.1:16379");
        if let Ok(c) = client {
            if let Ok(mut conn) = c.get_async_connection().await {
                let _: redis::RedisResult<()> = redis::cmd("PUBLISH")
                    .arg("agentforge:traces")
                    .arg(trace_event.to_string())
                    .query_async(&mut conn)
                    .await;
            }
        }
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
