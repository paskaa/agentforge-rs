//! LLM client - async HTTP calls to Bailian/Xiaomi API with retry.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

#[derive(Debug, Clone)]
pub struct LlmClient {
    pub client: Client,
    pub api_base: String,
    pub api_key: String,
    pub default_model: String,
    pub coding_model: String,
    pub simple_model: String,
    pub analysis_model: String,
    pub vision_model: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatRequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

impl ChatMessage {
    fn into_request(self) -> ChatRequestMessage {
        ChatRequestMessage {
            role: self.role,
            content: vec![VisionContent::Text { text: self.content }],
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum VisionContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: VisionImageUrl },
}

#[derive(Debug, Serialize)]
struct VisionImageUrl {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatRequestMessage {
    role: String,
    content: Vec<VisionContent>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
}

impl LlmClient {
    pub fn new(api_base: &str, api_key: &str, default_model: &str) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            api_base: api_base.to_string(),
            api_key: api_key.to_string(),
            default_model: default_model.to_string(),
            coding_model: "qwen-coder-plus".into(),
            simple_model: "qwen-turbo".into(),
            analysis_model: "qwen-plus".into(),
            vision_model: "mimo-v2.5".into(),
        }
    }

    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            api_base: cfg.llm.api_base.clone(),
            api_key: cfg.llm.api_key.clone(),
            default_model: cfg.llm.default_model.clone(),
            coding_model: cfg.llm.coding_model.clone(),
            simple_model: "qwen-turbo".into(),
            analysis_model: "qwen-plus".into(),
            vision_model: cfg.llm.vision_model.clone(),
        }
    }

    /// Send a chat completion request with retry.
    pub async fn chat(
        &self,
        system: &str,
        user: &str,
        model: Option<&str>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> anyhow::Result<String> {
        let model = model.unwrap_or(&self.default_model);
        let url = format!("{}/chat/completions", self.api_base);

        let body = ChatRequest {
            model: model.to_string(),
            messages: vec![
                ChatMessage { role: "system".into(), content: system.to_string() }.into_request(),
                ChatMessage { role: "user".into(), content: user.to_string() }.into_request(),
            ],
            temperature,
            max_tokens,
        };

        // Up to 3 retries with exponential backoff
        let mut last_err = None;
        for attempt in 0..3 {
            match self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => {
                    if resp.status().is_success() {
                        match resp.json::<ChatResponse>().await {
                            Ok(data) => {
                                if let Some(choice) = data.choices.first() {
                                    return Ok(choice.message.content.clone());
                                }
                            }
                            Err(e) => last_err = Some(anyhow::anyhow!("JSON parse: {}", e)),
                        }
                    } else {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        if body.contains("throttling") || body.contains("quota exceeded") {
                            tracing::warn!("LLM quota exceeded (attempt {}), waiting...", attempt + 1);
                            tokio::time::sleep(Duration::from_secs(10 * (attempt + 1) as u64)).await;
                            continue;
                        }
                        last_err = Some(anyhow::anyhow!("HTTP {}: {}", status, &body[..body.len().min(200)]));
                    }
                }
                Err(e) => {
                    tracing::warn!("LLM call attempt {} failed: {}", attempt + 1, e);
                    last_err = Some(anyhow::anyhow!("{:?}", e));
                }
            }
            // Backoff
            tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("LLM call failed after 3 attempts")))
    }
    /// 发送支持图片的多模态请求（image_url 或 base64）。
    pub async fn vision(
        &self,
        system: &str,
        text: &str,
        images: &[Vec<u8>],
        model: Option<&str>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> anyhow::Result<String> {
        let model = model.unwrap_or(&self.vision_model);
        let url = format!("{}/chat/completions", self.api_base);
        // Vision 请求图片多、响应慢，使用 120 秒超时
        let vision_client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| self.client.clone());

        let mut user_parts = vec![VisionContent::Text { text: text.to_string() }];
        for image in images {
            let b64 = BASE64.encode(image);
            user_parts.push(VisionContent::ImageUrl {
                image_url: VisionImageUrl {
                    url: format!("data:image/png;base64,{}", b64),
                    detail: Some("high".into()),
                },
            });
        }

        let body = ChatRequest {
            model: model.to_string(),
            messages: vec![
                ChatRequestMessage {
                    role: "system".into(),
                    content: vec![VisionContent::Text { text: system.to_string() }],
                },
                ChatRequestMessage {
                    role: "user".into(),
                    content: user_parts,
                },
            ],
            temperature,
            max_tokens,
        };

        let mut last_err = None;
        for attempt in 0..3 {
            match vision_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => {
                    if resp.status().is_success() {
                        match resp.json::<ChatResponse>().await {
                            Ok(data) => {
                                if let Some(choice) = data.choices.first() {
                                    return Ok(choice.message.content.clone());
                                }
                            }
                            Err(e) => last_err = Some(anyhow::anyhow!("JSON parse: {}", e)),
                        }
                    } else {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        if body.contains("throttling") || body.contains("quota exceeded") {
                            tracing::warn!("LLM quota exceeded (attempt {}), waiting...", attempt + 1);
                            tokio::time::sleep(Duration::from_secs(10 * (attempt + 1) as u64)).await;
                            continue;
                        }
                        last_err = Some(anyhow::anyhow!("HTTP {}: {}", status, &body[..body.len().min(200)]));
                    }
                }
                Err(e) => {
                    tracing::warn!("LLM call attempt {} failed: {}", attempt + 1, e);
                    last_err = Some(anyhow::anyhow!("{:?}", e));
                }
            }
            tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("LLM call failed after 3 attempts")))
    }
}
