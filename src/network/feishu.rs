//! Feishu (Lark) API client — token management + message sending.

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct FeishuClient {
    client: Client,
    app_id: String,
    app_secret: String,
    group_chat_id: String,
    token: Arc<Mutex<CachedToken>>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    value: String,
    expires_at: Instant,
}

impl FeishuClient {
    pub fn new(app_id: &str, app_secret: &str, group_chat_id: &str) -> Self {
        Self {
            client: Client::new(),
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            group_chat_id: group_chat_id.to_string(),
            token: Arc::new(Mutex::new(CachedToken {
                value: String::new(),
                expires_at: Instant::now(),
            })),
        }
    }

    /// Get a valid tenant access token (cached).
    pub async fn get_token(&self) -> anyhow::Result<String> {
        let mut cached = self.token.lock().await;
        if Instant::now() < cached.expires_at && !cached.value.is_empty() {
            return Ok(cached.value.clone());
        }

        let resp: TenantTokenResp = self
            .client
            .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await
            .context("Feishu token request")?
            .json()
            .await
            .context("Feishu token JSON")?;

        cached.value = resp.tenant_access_token.clone();
        cached.expires_at = Instant::now() + Duration::from_secs(resp.expire.saturating_sub(300) as u64);
        Ok(resp.tenant_access_token)
    }

    /// Send an interactive card message to the group chat.
    pub async fn send(&self, content: &str, chat_id: Option<&str>) -> anyhow::Result<bool> {
        let token = self.get_token().await?;
        let target = chat_id.unwrap_or(&self.group_chat_id);

        let body = serde_json::json!({
            "receive_id": target,
            "msg_type": "interactive",
            "content": serde_json::to_string(&serde_json::json!({
                "config": {"wide_screen_mode": true},
                "elements": [
                    {"tag": "markdown", "content": content},
                    {"tag": "note", "elements": [{"tag": "plain_text", "content": "AgentForge 智能体"}]}
                ]
            })).unwrap_or_default(),
        });

        let url = format!(
            "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id"
        );
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
        let code = resp_body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);

        Ok(status.is_success() && code == 0)
    }
}

#[derive(Debug, Deserialize)]
struct TenantTokenResp {
    #[serde(rename = "tenant_access_token")]
    tenant_access_token: String,
    expire: i64,
}
