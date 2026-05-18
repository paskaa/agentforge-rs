//! Feishu WebSocket Listener — receives group messages, routes to Redis.
//!
//! Replaces the Python `ws_listener`. Uses reqwest for Feishu API,
//! tokio-tungstenite for WebSocket, and Redis for message routing.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const AGENT_MAP: &[(&str, &str)] = &[
    ("诸葛亮", "zhugeliang"), ("刘备", "liubei"),
    ("关羽", "guanyu"), ("赵云", "zhaoyun"),
    ("荀彧", "xunyu"), ("张飞", "zhangfei"),
    ("华佗", "huatuo"), ("陈琳", "chenlin"),
];

pub struct WsListener {
    pub agent_id: String,
    pub app_id: String,
    pub app_secret: String,
    pub redis_url: String,
    last_msg_time: Arc<Mutex<Instant>>,
}

impl WsListener {
    pub fn new(agent_id: &str, app_id: &str, app_secret: &str, redis_url: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            redis_url: redis_url.to_string(),
            last_msg_time: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Route a mention name to an agent ID.
    pub fn route_mention(text: &str) -> Option<&str> {
        for (name, id) in AGENT_MAP {
            if text.contains(name) {
                return Some(*id);
            }
        }
        None
    }

    /// Check if a message is a broadcast.
    pub fn is_broadcast(text: &str) -> bool {
        text.contains("@所有人") || text.contains("@_user_1")
    }

    /// Start the WebSocket listener loop.
    pub async fn run(&self) -> anyhow::Result<()> {
        tracing::info!("[WS:{}] Starting Feishu WebSocket listener", self.agent_id);
        
        // Get tenant access token
        let client = reqwest::Client::new();
        let token_resp: serde_json::Value = client
            .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send().await?
            .json().await?;
        
        let token = token_resp["tenant_access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Failed to get tenant access token"))?;
        tracing::info!("[WS:{}] Token obtained", self.agent_id);

        // TODO: WebSocket connection with tungstenite
        // The lark-oapi SDK handles WS lifecycle; for now, Redis-based
        // message passing handles all agent communication.
        // Python WS listener remains as the Feishu transport bridge.
        
        tracing::warn!("[WS:{}] Feishu WebSocket not yet implemented in Rust — using Python bridge. Messages route via Redis list queues.", self.agent_id);
        
        // Keep the process alive
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_mention() {
        assert_eq!(WsListener::route_mention("@赵云 你好"), Some("zhaoyun"));
        assert_eq!(WsListener::route_mention("请诸葛亮 review"), Some("zhugeliang"));
        assert_eq!(WsListener::route_mention("hello world"), None);
    }

    #[test]
    fn test_is_broadcast() {
        assert!(WsListener::is_broadcast("@所有人 注意"));
        assert!(WsListener::is_broadcast("@_user_1 测试"));
        assert!(!WsListener::is_broadcast("@赵云 hi"));
    }
}
