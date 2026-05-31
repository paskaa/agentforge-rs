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
    /// Download bug attachments and analyze via LLM vision
    AnalyzeBug {
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
    /// Run full pipeline: scan → fix → verify for all active bugs (one at a time)
    Pipeline {
        #[arg(long, default_value = "10")]
        max_bugs: usize,
        #[arg(long, default_value = "guanyu")]
        default_fixer: String,
    },
    /// List all agents and their expertise
    ListAgents,
    /// Validate all Mapper XML SQL syntax (baseline scan)
    ValidateAllSql {
        #[arg(long, default_value = "/root/.openclaw/workspace/his-repo")]
        repo: String,
    },
    /// L4: Run analytics — generate metrics from TraceStore
    Analytics,
    /// L4: Generate Markdown report
    Report {
        #[arg(long, default_value = "/var/lib/agentforge/report.md")]
        output: String,
    },
    /// L5: Run self-optimizer — analyze failures and generate optimizations
    Optimize,
    /// L5: Show agent scores for smart routing
    Scores,
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
        Cli::AnalyzeBug { bug_id } => {
            agentforge::core::coordinator::analyze_bug_cli(&bug_id).await?;
        }
        Cli::FixBug { bug_id, bug_title, fixer } => {
            agentforge::core::coordinator::submit_fix_cli(&bug_id, bug_title.as_deref().unwrap_or(""), &fixer).await?;
        }
        Cli::AssignBug { bug_id, fixer } => {
            agentforge::core::coordinator::assign_bug_cli(&bug_id, &fixer).await?;
        }
        Cli::Pipeline { max_bugs, default_fixer } => {
            agentforge::core::coordinator::pipeline_cli(max_bugs, &default_fixer).await?;
        }
        Cli::ListAgents => {
            agentforge::core::coordinator::list_agents_cli();
        }
        Cli::ValidateAllSql { repo } => {
            use agentforge::core::sql_validator;
            let pg = sql_validator::PgConfig::default();
            tracing::info!("开始全量 SQL 基线扫描: {}", repo);
            let results = sql_validator::validate_all_mappers(&repo, &pg);
            let report = sql_validator::generate_scan_report(&results);
            println!("{}", report);
        }

        // ── L4 Analytics ──
        Cli::Analytics => {
            let pool = sqlx::SqlitePool::connect("sqlite:///var/lib/agentforge/traces.db?mode=ro").await?;
            let analytics = agentforge::core::analytics::Analytics::new(pool);
            let report = analytics.generate_report().await;
            let json = serde_json::to_string_pretty(&report)?;
            println!("{}", json);
        }
        Cli::Report { output } => {
            let pool = sqlx::SqlitePool::connect("sqlite:///var/lib/agentforge/traces.db?mode=ro").await?;
            let analytics = agentforge::core::analytics::Analytics::new(pool);
            let report = analytics.generate_report().await;
            agentforge::core::report::save_report(&report, &output)?;
            println!("✅ 报告已生成: {}", output);
            // Also print summary
            let md = agentforge::core::report::generate_markdown(&report);
            println!("{}", md);
        }

        // ── L5 Self-Optimizer ──
        Cli::Optimize => {
            let pool = sqlx::SqlitePool::connect("sqlite:///var/lib/agentforge/traces.db?mode=ro").await?;
            let analytics = agentforge::core::analytics::Analytics::new(pool);
            let report = analytics.generate_report().await;

            let scores_path = "/var/lib/agentforge/agent_scores.json";
            let mut optimizer = agentforge::core::self_optimizer::SelfOptimizer::load(scores_path);

            // Update scores from report
            for am in &report.agent_metrics {
                for bug_type in ["backend", "frontend", "database", "general"] {
                    let success = am.success_rate > 50.0;
                    optimizer.update_scores(&am.agent_id, bug_type, success, am.avg_duration_s);
                }
            }

            // Generate optimization actions
            let actions = optimizer.analyze_and_optimize(&report.agent_metrics, &report.failure_patterns);

            println!("🔧 L5 自优化分析");
            println!("═══════════════════════════════");
            if actions.is_empty() {
                println!("✅ 无需优化调整");
            } else {
                for action in &actions {
                    println!("
📋 {} → {}", action.action_type, action.target_agent);
                    println!("   原因: {}", action.reason);
                    println!("   建议: {}", action.change);
                    println!("   置信度: {:.0}%", action.confidence * 100.0);
                }
            }

            // Save updated scores
            optimizer.save(scores_path)?;
            println!("
✅ 分数已更新: {}", scores_path);
        }
        Cli::Scores => {
            let scores_path = "/var/lib/agentforge/agent_scores.json";
            let optimizer = agentforge::core::self_optimizer::SelfOptimizer::load(scores_path);
            let mut scores: Vec<_> = optimizer.scores.values().collect();
            scores.sort_by(|a, b| b.overall_score.partial_cmp(&a.overall_score).unwrap_or(std::cmp::Ordering::Equal));

            println!("🏆 智能体评分排名");
            println!("═══════════════════════════════");
            println!("{:<12} {:>8} {:>10} {:>8}", "Agent", "总分", "成功率", "耗时");
            println!("───────────────────────────────");
            for s in scores {
                println!("{:<12} {:>8.1} {:>9.0}% {:>7.0}s",
                    s.agent_id, s.overall_score, s.success_rate, s.avg_duration_s);
            }
        }
    }

    Ok(())
}
