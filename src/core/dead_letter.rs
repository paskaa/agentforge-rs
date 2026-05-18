//! Dead letter queue — persists failed tasks for later replay.

use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const DLQ_KEY: &str = "agentforge:dead_letter";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetter {
    pub task_json: String,
    pub error: String,
    pub agent_id: String,
    pub timestamp: u64,
}

/// Record a failed task to the dead letter queue.
pub async fn record_failure(
    redis: &mut redis::aio::MultiplexedConnection,
    agent_id: &str,
    task_json: &str,
    error: &str,
) -> redis::RedisResult<()> {
    let entry = serde_json::json!({
        "task_json": task_json,
        "error": error,
        "agent_id": agent_id,
        "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
    });

    redis.rpush::<_, _, ()>(DLQ_KEY, entry.to_string()).await?;
    Ok(())
}

/// List all dead letter entries.
pub async fn list_dlq(
    redis: &mut redis::aio::MultiplexedConnection,
) -> redis::RedisResult<Vec<DeadLetter>> {
    let items: Vec<String> = redis.lrange(DLQ_KEY, 0, -1).await?;
    Ok(items
        .iter()
        .filter_map(|s| serde_json::from_str::<DeadLetter>(s).ok())
        .collect())
}

/// Purge all dead letter entries.
pub async fn purge_dlq(redis: &mut redis::aio::MultiplexedConnection) -> redis::RedisResult<()> {
    redis.del(DLQ_KEY).await
}
