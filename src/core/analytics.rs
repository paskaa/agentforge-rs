//! L4 Analytics — data-driven optimization via TraceStore + FixTrajectory queries.
//!
//! Provides metrics: success rates, avg fix time, failure patterns, agent scoring.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

/// 归一化 agent_id: 中文名 → pinyin ID
fn normalize_agent_id(id: &str) -> String {
    match id {
        "关羽" => "guanyu".into(),
        "赵云" => "zhaoyun".into(),
        "荀彧" => "xunyu".into(),
        "张飞" => "zhangfei".into(),
        "华佗" => "huatuo".into(),
        "陈琳" => "chenlin".into(),
        "刘备" => "liubei".into(),
        "诸葛亮" => "zhugeliang".into(),
        other => other.to_string(),
    }
}

/// Agent performance metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub agent_id: String,
    pub total_fixes: i64,
    pub success_count: i64,
    pub fail_count: i64,
    pub success_rate: f64,
    pub avg_duration_ms: f64,
    pub avg_duration_s: f64,
}

/// Failure pattern — groups failures by error category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePattern {
    pub error_category: String,
    pub count: i64,
    pub agents: Vec<String>,
    pub example_bugs: Vec<String>,
}

/// Pipeline throughput metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMetrics {
    pub total_scanned: i64,
    pub total_queued: i64,
    pub total_completed: i64,
    pub total_success: i64,
    pub total_failed: i64,
    pub total_timeout: i64,
    pub avg_queue_wait_ms: f64,
    pub avg_fix_time_ms: f64,
}

/// Full analytics report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsReport {
    pub generated_at: String,
    pub agent_metrics: Vec<AgentMetrics>,
    pub failure_patterns: Vec<FailurePattern>,
    pub pipeline: PipelineMetrics,
    pub top_slow_bugs: Vec<SlowBug>,
    pub recommendations: Vec<String>,
}

/// Slow bug — took unusually long to fix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowBug {
    pub bug_id: String,
    pub agent: String,
    pub duration_s: f64,
    pub success: bool,
}

pub struct Analytics {
    pool: SqlitePool,
}

impl Analytics {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get per-agent performance metrics from traces.
    pub async fn agent_metrics(&self) -> Vec<AgentMetrics> {
        let rows: Vec<(String, i64, i64, i64, f64)> = sqlx::query_as(
            r#"
            SELECT
                agent_id,
                COUNT(*) as total,
                SUM(CASE WHEN status = 'ok' THEN 1 ELSE 0 END) as success,
                SUM(CASE WHEN status = 'failed' OR status = 'error' THEN 1 ELSE 0 END) as fail,
                COALESCE(AVG(duration_ms), 0.0) as avg_dur
            FROM traces
            WHERE event = 'fix_done'
            GROUP BY agent_id
            ORDER BY total DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        // 归一化 agent_id 并合并同一智能体的数据
        let mut merged: std::collections::HashMap<String, (i64, i64, i64, f64)> = std::collections::HashMap::new();
        for (agent_id, total, success, fail, avg_dur) in rows {
            let normalized = normalize_agent_id(&agent_id);
            let entry = merged.entry(normalized).or_insert((0, 0, 0, 0.0));
            entry.0 += total;
            entry.1 += success;
            entry.2 += fail;
            let old_total = entry.0 - total;
            if entry.0 > 0 {
                entry.3 = (entry.3 * old_total as f64 + avg_dur * total as f64) / entry.0 as f64;
            }
        }

        merged.into_iter()
            .map(|(agent_id, (total, success, fail, avg_dur))| {
                let success_rate = if total > 0 {
                    success as f64 / total as f64 * 100.0
                } else {
                    0.0
                };
                AgentMetrics {
                    agent_id,
                    total_fixes: total,
                    success_count: success,
                    fail_count: fail,
                    success_rate,
                    avg_duration_ms: avg_dur,
                    avg_duration_s: avg_dur / 1000.0,
                }
            })
            .collect::<Vec<_>>()
    }

    /// Get failure patterns grouped by real error category (not raw message text).
    pub async fn failure_patterns(&self) -> Vec<FailurePattern> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT
                COALESCE(agent_id, '') as agent_id,
                COALESCE(message, '') as message,
                COALESCE(task_id, '') as task_id
            FROM traces
            WHERE (event = 'fix_done' AND (status = 'failed' OR status = 'error'))
               OR (event = 'fix_attempt' AND (status = 'failed' OR status = 'error'))
            ORDER BY ts DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let mut categories: std::collections::HashMap<String, (i64, Vec<String>, Vec<String>)> =
            std::collections::HashMap::new();

        for (agent_id, message, task_id) in &rows {
            let agent = normalize_agent_id(agent_id);
            let msg = message.trim();

            // Skip noise: empty, attempt=N, HEAD is now at, Creating isolate
            if msg.is_empty() || msg.starts_with("attempt=") || msg.starts_with("HEAD is")
                || msg.contains("Creating isolate") || msg.contains("Claude Code") {
                continue;
            }

            let category = if msg.contains("编译") || msg.contains("compile") || msg.contains("Cargo") || msg.contains("cargo check") {
                "编译失败"
            } else if msg.contains("合并冲突") || msg.contains("merge conflict") || msg.contains("Merge remote") {
                "Git 合并冲突"
            } else if msg.contains("超时") || msg.contains("timeout") || msg.contains("timed out") {
                "请求超时"
            } else if msg.contains("rate limit") || msg.contains("429") || msg.contains("API error") {
                "API 限流"
            } else if msg.contains("max retries") || msg.contains("最大重试") {
                "超过最大重试次数"
            } else if msg.contains("lock") || msg.contains("锁定") {
                "锁冲突"
            } else if msg.contains("null") || msg.contains("None") || msg.contains("空指针") {
                "空指针/空值异常"
            } else if msg.contains("SQL") || msg.contains("sql") || msg.contains("数据库") || msg.contains("查询") {
                "SQL/数据库错误"
            } else if msg.contains("类型") || msg.contains("type") || msg.contains("mismatch") {
                "类型不匹配"
            } else if msg.contains("权限") || msg.contains("auth") || msg.contains("403") || msg.contains("401") {
                "权限/鉴权错误"
            } else if msg.contains("前端") || msg.contains("vue") || msg.contains("Vue") || msg.contains("组件") {
                "前端组件错误"
            } else if msg.contains("API") || msg.contains("接口") || msg.contains("request") {
                "API/接口错误"
            } else if msg.contains("文件") || msg.contains("file") || msg.contains("路径") {
                "文件/路径错误"
            } else {
                "其他错误"
            };

            let entry = categories.entry(category.to_string()).or_insert((0, Vec::new(), Vec::new()));
            entry.0 += 1;
            if !entry.1.contains(&agent) {
                entry.1.push(agent);
            }
            let bug_id = task_id.trim_start_matches("Bug#").to_string();
            if !bug_id.is_empty() && bug_id != "?" && !entry.2.contains(&bug_id) && entry.2.len() < 5 {
                entry.2.push(bug_id);
            }
        }

        let mut result: Vec<FailurePattern> = categories.into_iter()
            .map(|(error_category, (count, agents, example_bugs))| {
                FailurePattern { error_category, count, agents, example_bugs }
            })
            .collect();
        result.sort_by(|a, b| b.count.cmp(&a.count));
        result
    }

    /// Get pipeline throughput metrics.
    pub async fn pipeline_metrics(&self) -> PipelineMetrics {
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM traces WHERE event = 'fix_start'"
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let success: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM traces WHERE event = 'fix_done' AND status = 'ok'"
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let failed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM traces WHERE event = 'fix_done' AND (status = 'failed' OR status = 'error')"
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let avg_dur: f64 = sqlx::query_scalar(
            "SELECT COALESCE(AVG(duration_ms), 0.0) FROM traces WHERE event = 'fix_done'"
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0.0);

        PipelineMetrics {
            total_scanned: total,
            total_queued: total,
            total_completed: success + failed,
            total_success: success,
            total_failed: failed,
            total_timeout: 0,
            avg_queue_wait_ms: 0.0,
            avg_fix_time_ms: avg_dur,
        }
    }

    /// Get slowest bugs (took longest to fix).
    pub async fn slow_bugs(&self, limit: i64) -> Vec<SlowBug> {
        let rows: Vec<(String, String, f64, String)> = sqlx::query_as(
            r#"
            SELECT
                COALESCE(task_id, '?') as bug_id,
                agent_id,
                COALESCE(duration_ms, 0) as dur,
                COALESCE(status, '?') as st
            FROM traces
            WHERE event = 'fix_done'
            ORDER BY dur DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .map(|(bug_id, agent, dur, status)| SlowBug {
                bug_id,
                agent,
                duration_s: dur / 1000.0,
                success: status == "ok",
            })
            .collect()
    }

    /// Generate recommendations based on metrics.
    pub fn generate_recommendations(
        agent_metrics: &[AgentMetrics],
        failure_patterns: &[FailurePattern],
        pipeline: &PipelineMetrics,
    ) -> Vec<String> {
        let mut recs = Vec::new();

        // Low success rate agents
        for am in agent_metrics {
            if am.total_fixes >= 3 && am.success_rate < 50.0 {
                recs.push(format!(
                    "⚠️ {} 成功率仅 {:.0}%（{}次成功/{}次总计）— 建议检查约束或增加重试",
                    am.agent_id, am.success_rate, am.success_count, am.total_fixes
                ));
            }
        }

        // High failure patterns
        for fp in failure_patterns.iter().take(3) {
            if fp.count >= 3 {
                recs.push(format!(
                    "🔴 「{}」失败 {} 次（涉及 {:?}）— 建议针对性优化约束",
                    fp.error_category, fp.count, fp.agents
                ));
            }
        }

        // Pipeline overall
        if pipeline.total_completed > 0 {
            let overall_rate = pipeline.total_success as f64 / pipeline.total_completed as f64 * 100.0;
            if overall_rate < 60.0 {
                recs.push(format!(
                    "📊 Pipeline 总成功率 {:.0}% — 建议降低 max_bugs 或增加重试上限",
                    overall_rate
                ));
            }
        }

        // Slow fixes
        for am in agent_metrics {
            if am.avg_duration_s > 600.0 && am.total_fixes >= 2 {
                recs.push(format!(
                    "🐢 {} 平均耗时 {:.0}s — 建议检查 prompt 复杂度或门禁耗时",
                    am.agent_id, am.avg_duration_s
                ));
            }
        }

        if recs.is_empty() {
            recs.push("✅ 各项指标正常，无需调整".into());
        }

        recs
    }

    /// Generate full analytics report.
    pub async fn generate_report(&self) -> AnalyticsReport {
        let agent_metrics = self.agent_metrics().await;
        let failure_patterns = self.failure_patterns().await;
        let pipeline = self.pipeline_metrics().await;
        let top_slow_bugs = self.slow_bugs(10).await;
        let recommendations = Self::generate_recommendations(
            &agent_metrics, &failure_patterns, &pipeline,
        );

        AnalyticsReport {
            generated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            agent_metrics,
            failure_patterns,
            pipeline,
            top_slow_bugs,
            recommendations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recommendations_empty_metrics() {
        let recs = Analytics::generate_recommendations(&[], &[], &PipelineMetrics {
            total_scanned: 0, total_queued: 0, total_completed: 0,
            total_success: 0, total_failed: 0, total_timeout: 0,
            avg_queue_wait_ms: 0.0, avg_fix_time_ms: 0.0,
        });
        assert_eq!(recs.len(), 1);
        assert!(recs[0].contains("正常"));
    }

    #[test]
    fn test_recommendations_low_success() {
        let metrics = vec![AgentMetrics {
            agent_id: "guanyu".into(),
            total_fixes: 10,
            success_count: 2,
            fail_count: 8,
            success_rate: 20.0,
            avg_duration_ms: 300000.0,
            avg_duration_s: 300.0,
        }];
        let recs = Analytics::generate_recommendations(&metrics, &[], &PipelineMetrics {
            total_scanned: 10, total_queued: 10, total_completed: 10,
            total_success: 2, total_failed: 8, total_timeout: 0,
            avg_queue_wait_ms: 0.0, avg_fix_time_ms: 300000.0,
        });
        assert!(recs.iter().any(|r| r.contains("guanyu")));
        assert!(recs.iter().any(|r| r.contains("成功率")));
    }
}
