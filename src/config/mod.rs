//! Configuration layer — reads from agentforge.yaml, .env, and environment.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub redis: RedisConfig,
    pub llm: LlmConfig,
    pub feishu: FeishuConfig,
    pub zentao: ZentaoConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    pub agents: HashMap<String, AgentConfig>,
    pub scheduler: SchedulerConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig {
    #[serde(default = "default_redis_host")]
    pub host: String,
    #[serde(default = "default_redis_port")]
    pub port: u16,
    #[serde(default)]
    pub db: i64,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    pub api_base: String,
    pub api_key: String,
    pub default_model: String,
    pub coding_model: String,
    #[serde(default = "default_vision_model")]
    pub vision_model: String,

}

#[derive(Debug, Deserialize, Clone)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    pub group_chat_id: String,
    pub credentials_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ZentaoConfig {
    pub base_url: String,
    pub scripts_dir: PathBuf,
    pub token_file: PathBuf,
    #[serde(default = "default_zentao_cli")]
    pub cli_path: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub role: String,
    pub expertise: Vec<String>,
    pub model: Option<String>,
    pub feishu_app_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SchedulerConfig {
    pub enabled: bool,
    pub tasks_file: PathBuf,
}


#[derive(Debug, Deserialize, Clone, Default)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_host")]
    pub host: String,
    #[serde(default = "default_db_port")]
    pub port: u16,
    #[serde(default = "default_db_name")]
    pub database: String,
    #[serde(default = "default_db_user")]
    pub username: String,
    #[serde(default = "default_db_password")]
    pub password: String,
}

fn default_db_host() -> String { "127.0.0.1".into() }
fn default_db_port() -> u16 { 5432 }
fn default_db_name() -> String { "postgresql".into() }
fn default_db_user() -> String { "postgresql".into() }
fn default_db_password() -> String { String::new() }
// Defaults
fn default_redis_host() -> String {
    "127.0.0.1".into()
}
fn default_redis_port() -> u16 {
    16379
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            host: default_redis_host(),
            port: default_redis_port(),
            db: 0,
            username: String::new(),
            password: String::new(),
        }
    }
}
fn default_vision_model() -> String {
    "agnes-2.0-flash".into()
}

fn default_zentao_cli() -> String {
    "/usr/local/bin/zentao".into()
}

impl Default for ZentaoConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            scripts_dir: PathBuf::from("."),
            token_file: PathBuf::from("."),
            cli_path: default_zentao_cli(),
            username: String::new(),
            password: String::new(),
        }
    }
}


impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_base: String::new(),
            api_key: String::new(),
            default_model: String::new(),
            coding_model: String::new(),
            vision_model: default_vision_model(),
        }
    }
}

impl Default for FeishuConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            app_secret: String::new(),
            group_chat_id: String::new(),
            credentials_file: None,
        }
    }
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tasks_file: PathBuf::from("./config/scheduler_tasks.json"),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            redis: RedisConfig::default(),
            llm: LlmConfig::default(),
            feishu: FeishuConfig::default(),
            zentao: ZentaoConfig::default(),
            agents: HashMap::new(),
            database: DatabaseConfig::default(),
            scheduler: SchedulerConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from agentforge.yaml (or CONFIG_PATH env).
    pub fn load() -> anyhow::Result<Self> {
        let config_path = std::env::var("CONFIG_PATH")
            .unwrap_or_else(|_| "config/agentforge.yaml".into());

        let settings = config::Config::builder()
            .add_source(config::File::with_name(&config_path).required(false))
            .add_source(config::Environment::with_prefix("AGENTFORGE").separator("__"))
            .build()?;

        let cfg: Config = settings.try_deserialize()?;
        Ok(cfg)
    }

    /// Redis connection URL for the `redis` crate.
    pub fn redis_url(&self) -> String {
        if self.redis.password.is_empty() {
            format!("redis://{}:{}/{}", self.redis.host, self.redis.port, self.redis.db)
        } else {
            format!(
                "redis://{}:{}@{}:{}/{}",
                self.redis.username, self.redis.password,
                self.redis.host, self.redis.port, self.redis.db,
            )
        }
    }

    pub fn stream_name(&self) -> &str {
        "agent-work-queue"
    }

    pub fn fix_queue(&self, agent_id: &str) -> String {
        format!("agent-work-queue:fix:{}", agent_id)
    }
}
