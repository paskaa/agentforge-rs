//! AgentForge — Multi-agent bug fixing framework (Rust rewrite)
//!
//! Architecture:
//! - config:     YAML/ENV configuration
//! - core:       Executor, Pipeline, Coordinator, SubAgent, LLM client
//! - network:    Feishu WebSocket listener + API client
//! - tools:      Scheduler, Skill registry


use clap::Parser;

#[derive(Parser)]
#[command(name = "agentforge", about = "Multi-agent bug fixing framework")]
enum Cli {
    /// Run the executor for a single agent
    Executor {
        #[arg(long)]
        agent: String,
    },
    /// Run the WebSocket listener for Feishu
    Ws {
        #[arg(long)]
        agent: String,
    },
    /// Run the scheduler (daily reports, health checks)
    Scheduler,
    /// Check configuration
    Check,

    // ── Pipeline CLI (called by Hermes bridge) ──
    /// Scan all agent bugs from Zentao and print summary
    ScanBugs,
    /// Query a single bug detail from Zentao
    QueryBug {
        #[arg(long)]
        bug_id: String,
    },
    /// Submit a fix task to the Redis queue
    FixBug {
        #[arg(long)]
        bug_id: String,
        #[arg(long)]
        bug_title: Option<String>,
        #[arg(long, default_value = "zhugeliang")]
        fixer: String,
    },
    /// Assign a bug to a specific fixer agent
    AssignBug {
        #[arg(long)]
        bug_id: String,
        #[arg(long)]
        fixer: String,
    },
    /// List all agents and their expertise
    ListAgents,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,agentforge=debug".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli {
        Cli::Executor { agent } => {
            let cfg = agentforge::config::Config::load()?;
            let executor = agentforge::core::executor::AgentExecutor::new(&agent, cfg).await?;
            executor.run().await?;
        }
        Cli::Ws { agent } => {
            let cfg = agentforge::config::Config::load()?;
            let listener = agentforge::network::ws_listener::WsListener::new(
                &agent, &cfg.feishu.app_id, &cfg.feishu.app_secret, &cfg.redis_url(),
            );
            listener.run().await?;
        }
        Cli::Scheduler => {
            tracing::info!("Scheduler not yet wired — Python scheduler still active");
        }
        Cli::Check => {
            let cfg = agentforge::config::Config::load()?;
            tracing::info!("Config OK — agents: {}", cfg.agents.len());
        }

        // ── Pipeline CLI (called by Hermes bridge) ──
        Cli::ScanBugs => {
            agentforge::core::coordinator::scan_bugs_cli().await?;
        }
        Cli::QueryBug { bug_id } => {
            agentforge::core::coordinator::query_bug_cli(&bug_id).await?;
        }
        Cli::FixBug { bug_id, bug_title, fixer } => {
            agentforge::core::coordinator::submit_fix_cli(&bug_id, bug_title.as_deref().unwrap_or(""), &fixer).await?;
        }
        Cli::AssignBug { bug_id, fixer } => {
            agentforge::core::coordinator::assign_bug_cli(&bug_id, &fixer).await?;
        }
        Cli::ListAgents => {
            agentforge::core::coordinator::list_agents_cli();
        }
    }

    Ok(())
}
