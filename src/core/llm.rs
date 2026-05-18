//! LLM client — async HTTP calls to Bailian/DeepSeek API with retry.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LlmClient {
    pub client: Client,
    pub api_base: String,
    pub api_key: String,
    pub default_model: String,
    pub coding_model: String,
    pub simple_model: String,
    pub analysis_model: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
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
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            api_base: api_base.to_string(),
            api_key: api_key.to_string(),
            default_model: default_model.to_string(),
            coding_model: "qwen-coder-plus".into(),
            simple_model: "qwen-turbo".into(),
            analysis_model: "qwen-plus".into(),
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
                ChatMessage {
                    role: "system".into(),
                    content: system.to_string(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: user.to_string(),
                },
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
}
