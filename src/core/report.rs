//! L4 Report — generates Markdown analytics reports for human review.

use super::analytics::{AnalyticsReport, AgentMetrics, FailurePattern, PipelineMetrics};

/// Generate a full Markdown report from analytics data.
pub fn generate_markdown(report: &AnalyticsReport) -> String {
    let mut md = String::new();

    md.push_str(&format!("# 📊 AgentForge-RS 分析报告\n\n"));
    md.push_str(&format!("> 生成时间: {}\n\n", report.generated_at));

    // Pipeline overview
    md.push_str("## Pipeline 概览\n\n");
    md.push_str(&pipeline_table(&report.pipeline));

    // Agent performance
    md.push_str("\n## 智能体绩效\n\n");
    md.push_str(&agent_table(&report.agent_metrics));

    // Failure patterns
    if !report.failure_patterns.is_empty() {
        md.push_str("\n## 失败模式分析\n\n");
        md.push_str(&failure_table(&report.failure_patterns));
    }

    // Slow bugs
    if !report.top_slow_bugs.is_empty() {
        md.push_str("\n## 耗时最长的 Bug\n\n");
        md.push_str("| Bug | Agent | 耗时 | 状态 |\n|---|---|---|---|\n");
        for bug in &report.top_slow_bugs {
            let status = if bug.success { "✅" } else { "❌" };
            md.push_str(&format!(
                "| #{} | {} | {:.0}s | {} |\n",
                bug.bug_id, bug.agent, bug.duration_s, status
            ));
        }
    }

    // Recommendations
    md.push_str("\n## 优化建议\n\n");
    for rec in &report.recommendations {
        md.push_str(&format!("- {}\n", rec));
    }

    md
}

fn pipeline_table(p: &PipelineMetrics) -> String {
    let mut t = String::from("| 指标 | 值 |\n|---|---|\n");
    t.push_str(&format!("| 扫描总数 | {} |\n", p.total_scanned));
    t.push_str(&format!("| 已完成 | {} |\n", p.total_completed));
    t.push_str(&format!("| 成功 | {} |\n", p.total_success));
    t.push_str(&format!("| 失败 | {} |\n", p.total_failed));
    t.push_str(&format!("| 超时 | {} |\n", p.total_timeout));
    if p.total_completed > 0 {
        let rate = p.total_success as f64 / p.total_completed as f64 * 100.0;
        t.push_str(&format!("| **总成功率** | **{:.1}%** |\n", rate));
    }
    t.push_str(&format!("| 平均修复耗时 | {:.0}s |\n", p.avg_fix_time_ms / 1000.0));
    t
}

fn agent_table(metrics: &[AgentMetrics]) -> String {
    let mut t = String::from("| Agent | 总修复 | 成功 | 失败 | 成功率 | 平均耗时 |\n");
    t.push_str("|---|---|---|---|---|---|\n");
    for m in metrics {
        let rate_icon = if m.success_rate >= 80.0 { "🟢" }
            else if m.success_rate >= 50.0 { "🟡" }
            else { "🔴" };
        t.push_str(&format!(
            "| {} {} | {} | {} | {} | {:.0}% | {:.0}s |\n",
            rate_icon, m.agent_id, m.total_fixes, m.success_count,
            m.fail_count, m.success_rate, m.avg_duration_s
        ));
    }
    t
}

fn failure_table(patterns: &[FailurePattern]) -> String {
    let mut t = String::from("| 错误类别 | 次数 | 涉及 Agent | 示例 Bug |\n");
    t.push_str("|---|---|---|---|\n");
    for p in patterns.iter().take(10) {
        let agents = p.agents.join(", ");
        let bugs = p.example_bugs.iter()
            .map(|b| format!("#{}", b))
            .collect::<Vec<_>>()
            .join(", ");
        t.push_str(&format!("| {} | {} | {} | {} |\n",
            p.error_category.chars().take(60).collect::<String>(),
            p.count, agents, bugs));
    }
    t
}

/// Save report to file.
pub fn save_report(report: &AnalyticsReport, path: &str) -> std::io::Result<()> {
    let md = generate_markdown(report);
    std::fs::write(path, md)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::analytics::SlowBug;

    #[test]
    fn test_generate_markdown() {
        let report = AnalyticsReport {
            generated_at: "2026-05-31 10:00:00 UTC".into(),
            agent_metrics: vec![AgentMetrics {
                agent_id: "guanyu".into(),
                total_fixes: 10,
                success_count: 7,
                fail_count: 3,
                success_rate: 70.0,
                avg_duration_ms: 300000.0,
                avg_duration_s: 300.0,
            }],
            failure_patterns: vec![],
            pipeline: PipelineMetrics {
                total_scanned: 10,
                total_queued: 10,
                total_completed: 10,
                total_success: 7,
                total_failed: 3,
                total_timeout: 0,
                avg_queue_wait_ms: 0.0,
                avg_fix_time_ms: 300000.0,
            },
            top_slow_bugs: vec![SlowBug {
                bug_id: "630".into(),
                agent: "guanyu".into(),
                duration_s: 584.0,
                success: true,
            }],
            recommendations: vec!["测试建议".into()],
        };

        let md = generate_markdown(&report);
        assert!(md.contains("Pipeline 概览"));
        assert!(md.contains("guanyu"));
        assert!(md.contains("70.0%"));
        assert!(md.contains("测试建议"));
    }
}
