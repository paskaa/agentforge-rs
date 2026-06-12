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
    /// Start web dashboard server
    Web {
        #[arg(long, default_value = "3100")]
        port: u16,
    },
    /// Upload screenshot evidence to Zentao bug
    UploadEvidence {
        #[arg(long)]
        bug_id: String,
        #[arg(long)]
        file: String,
        #[arg(long, default_value = "Playwright回归测试截图")]
        description: String,
    },
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

            // 直接从 analytics 数据计算 scores（不用 EMA，避免累积偏差）
            for am in &report.agent_metrics {
                let agent_id = am.agent_id.clone();
                let score = optimizer.scores.entry(agent_id.clone()).or_insert_with(|| {
                    agentforge::core::self_optimizer::AgentScore {
                        agent_id: agent_id.clone(),
                        success_rate: 0.0,
                        avg_duration_s: 0.0,
                        bug_type_scores: std::collections::HashMap::new(),
                        overall_score: 0.0,
                    }
                });
                // 直接覆盖 success_rate 和 avg_duration
                score.success_rate = am.success_rate;
                score.avg_duration_s = am.avg_duration_s;
                // 按 bug type 更新分数
                for bug_type in ["backend", "frontend", "database", "general"] {
                    let type_score = score.bug_type_scores.entry(bug_type.to_string()).or_insert(50.0);
                    if am.success_rate > 50.0 {
                        *type_score = (*type_score * 0.7 + 60.0 * 0.3).clamp(0.0, 100.0);
                    } else {
                        *type_score = (*type_score * 0.7 + 35.0 * 0.3).clamp(0.0, 100.0);
                    }
                }
                // overall = weighted combination
                score.overall_score = score.success_rate * 0.6
                    + (100.0 - score.avg_duration_s.min(100.0)) * 0.2
                    + score.bug_type_scores.values().copied().sum::<f64>()
                        / score.bug_type_scores.len().max(1) as f64 * 0.2;
            }

            // Generate optimization actions
            let actions = optimizer.analyze_and_optimize(&report.agent_metrics, &report.failure_patterns);
            // Apply actions to populate extra_constraints (persisted via save)
            optimizer.apply_actions(&actions);

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
            // Capture scores BEFORE update for comparison
            let scores_before = optimizer.scores.clone();
            optimizer.save(scores_path)?;

            // ── Capture git history from his-repo ──
            let his_repo = "/root/.openclaw/workspace/his-repo";
            let git_log: Vec<serde_json::Value> = std::process::Command::new("git")
                .args(["log", "--oneline", "-20", "--format=%H|%h|%s|%ai|%an"])
                .current_dir(his_repo)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).lines()
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.splitn(5, '|').collect();
                        if parts.len() >= 4 {
                            Some(serde_json::json!({
                                "hash": parts[0],
                                "short": parts[1],
                                "message": parts[2],
                                "date": parts[3],
                                "author": parts.get(4).unwrap_or(&""),
                            }))
                        } else { None }
                    }).collect())
                .unwrap_or_default();

            // Git diff stats (last 10 commits)
            let git_diff_stat: Vec<serde_json::Value> = std::process::Command::new("git")
                .args(["log", "--oneline", "-10", "--numstat", "--format="])
                .current_dir(his_repo)
                .output()
                .map(|o| {
                    let text = String::from_utf8_lossy(&o.stdout);
                    let mut commits = Vec::new();
                    let mut current_files = Vec::new();
                    let mut current_insertions = 0i64;
                    let mut current_deletions = 0i64;
                    for line in text.lines() {
                        if line.is_empty() && !current_files.is_empty() {
                            commits.push(serde_json::json!({
                                "files_changed": current_files.len(),
                                "insertions": current_insertions,
                                "deletions": current_deletions,
                                "files": current_files.iter().take(10).cloned().collect::<Vec<_>>(),
                            }));
                            current_files.clear();
                            current_insertions = 0;
                            current_deletions = 0;
                        } else if !line.is_empty() {
                            let parts: Vec<&str> = line.split("\t").collect();
                            if parts.len() >= 3 {
                                current_insertions += parts[0].parse::<i64>().unwrap_or(0);
                                current_deletions += parts[1].parse::<i64>().unwrap_or(0);
                                current_files.push(parts[2].to_string());
                            }
                        }
                    }
                    if !current_files.is_empty() {
                        commits.push(serde_json::json!({
                            "files_changed": current_files.len(),
                            "insertions": current_insertions,
                            "deletions": current_deletions,
                            "files": current_files,
                        }));
                    }
                    commits
                })
                .unwrap_or_default();

            // Agentforge-rs own recent commits
            let framework_commits: Vec<serde_json::Value> = std::process::Command::new("git")
                .args(["log", "--oneline", "-10", "--format=%h|%s|%ai"])
                .current_dir("/root/agentforge-rs")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).lines()
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.splitn(3, '|').collect();
                        if parts.len() >= 2 {
                            Some(serde_json::json!({
                                "short": parts[0],
                                "message": parts[1],
                                "date": parts.get(2).unwrap_or(&""),
                            }))
                        } else { None }
                    }).collect())
                .unwrap_or_default();

            // Score comparison: before vs after
            let score_changes: Vec<serde_json::Value> = optimizer.scores.iter().map(|(id, after)| {
                let before = scores_before.get(id);
                let before_rate = before.map(|b| b.success_rate).unwrap_or(0.0);
                let before_score = before.map(|b| b.overall_score).unwrap_or(0.0);
                serde_json::json!({
                    "agent": id,
                    "success_rate_before": before_rate,
                    "success_rate_after": after.success_rate,
                    "success_rate_delta": after.success_rate - before_rate,
                    "overall_score_before": before_score,
                    "overall_score_after": after.overall_score,
                    "overall_score_delta": after.overall_score - before_score,
                    "avg_duration_s": after.avg_duration_s,
                })
            }).collect::<Vec<_>>();

            // ── Write optimization log ──
            let log_path = "/var/lib/agentforge/l5_optimization_log.json";
            let mut log_entries: Vec<serde_json::Value> = if let Ok(data) = std::fs::read_to_string(log_path) {
                serde_json::from_str(&data).unwrap_or_default()
            } else { vec![] };

            let entry = serde_json::json!({
                "timestamp": chrono::Local::now().to_rfc3339(),
                "actions_count": actions.len(),
                "actions": actions.iter().map(|a| serde_json::json!({
                    "type": a.action_type,
                    "target": a.target_agent,
                    "reason": a.reason,
                    "change": a.change,
                    "confidence": a.confidence,
                })).collect::<Vec<_>>(),
                "scores_snapshot": report.agent_metrics.iter().map(|am| serde_json::json!({
                    "agent": am.agent_id,
                    "success_rate": am.success_rate,
                    "avg_duration_s": am.avg_duration_s,
                    "total_fixes": am.total_fixes,
                })).collect::<Vec<_>>(),
                "score_changes": score_changes,
                "git_commits": git_log,
                "git_diff_stats": git_diff_stat,
                "framework_commits": framework_commits,
            });
            log_entries.push(entry);
            // Keep last 50 entries (larger now with git data)
            if log_entries.len() > 50 {
                log_entries = log_entries[log_entries.len()-50..].to_vec();
            }
            let _ = std::fs::write(log_path, serde_json::to_string_pretty(&log_entries).unwrap_or_default());

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
        Cli::Web { port } => {
            let pool = sqlx::SqlitePool::connect("sqlite:///var/lib/agentforge/traces.db?mode=rwc").await.ok();
            agentforge::core::web_server::start_web_server(pool, port).await?;
        }
        Cli::UploadEvidence { bug_id, file, description } => {
            let cfg = agentforge::config::Config::load()?;
            let client = agentforge::core::zentao::ZentaoClient::from_config(&cfg);
            match client.upload_attachment(&bug_id, &file, &description).await {
                Ok(_) => println!("✅ 截图证据已上传到禅道 Bug #{}: {}", bug_id, file),
                Err(e) => eprintln!("❌ 上传失败: {}", e),
            }
        }
    }

    Ok(())
}
