//! L5 Self-Optimizer — AI-driven optimization based on analytics data.
//!
//! Analyzes failure patterns → adjusts prompts/constraints/routing → feedback loop.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Optimization action — what to change and why.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationAction {
    pub action_type: String,   // "adjust_constraint" | "reroute" | "retry_strategy" | "prompt_boost"
    pub target_agent: String,
    pub reason: String,
    pub change: String,
    pub confidence: f64,       // 0.0 - 1.0
}

/// Agent score — used for smart routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentScore {
    pub agent_id: String,
    pub success_rate: f64,
    pub avg_duration_s: f64,
    pub bug_type_scores: HashMap<String, f64>,  // bug_type → score
    pub overall_score: f64,
}

/// Self-optimizer — analyzes metrics and generates optimization actions.
pub struct SelfOptimizer {
    /// Historical agent scores (persisted to JSON).
    pub scores: HashMap<String, AgentScore>,
    /// Constraint additions per agent (accumulated from optimizations).
    extra_constraints: HashMap<String, Vec<String>>,
}

impl SelfOptimizer {
    pub fn new() -> Self {
        Self {
            scores: HashMap::new(),
            extra_constraints: HashMap::new(),
        }
    }

    /// Load scores from JSON file.
    pub fn load(path: &str) -> Self {
        let data = std::fs::read_to_string(path).unwrap_or_default();
        let scores: HashMap<String, AgentScore> =
            serde_json::from_str(&data).unwrap_or_default();
        Self { scores, extra_constraints: HashMap::new() }
    }

    /// Save scores to JSON file.
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.scores)?;
        std::fs::write(path, json)
    }

    /// Update agent scores based on recent fix results.
    pub fn update_scores(
        &mut self,
        agent_id: &str,
        bug_type: &str,
        success: bool,
        duration_s: f64,
    ) {
        let score = self.scores.entry(agent_id.to_string()).or_insert_with(|| AgentScore {
            agent_id: agent_id.to_string(),
            success_rate: 0.0,
            avg_duration_s: 0.0,
            bug_type_scores: HashMap::new(),
            overall_score: 0.0,
        });

        // Update bug type score (exponential moving average)
        let type_score = score.bug_type_scores.entry(bug_type.to_string()).or_insert(50.0);
        let reward = if success { 10.0 } else { -15.0 };
        *type_score = (*type_score * 0.7 + (50.0 + reward) * 0.3).clamp(0.0, 100.0);

        // Update overall score
        let alpha = 0.3;
        let new_rate = if success { 100.0 } else { 0.0 };
        score.success_rate = score.success_rate * (1.0 - alpha) + new_rate * alpha;
        score.avg_duration_s = score.avg_duration_s * (1.0 - alpha) + duration_s * alpha;

        // Overall = weighted combination
        score.overall_score = score.success_rate * 0.6
            + (100.0 - score.avg_duration_s.min(100.0)) * 0.2
            + score.bug_type_scores.values().copied().sum::<f64>()
                / score.bug_type_scores.len().max(1) as f64 * 0.2;
    }

    /// Pick the best agent for a bug type based on historical scores.
    pub fn best_agent_for(&self, bug_type: &str) -> Option<String> {
        let mut best: Option<(String, f64)> = None;
        for (agent_id, score) in &self.scores {
            let type_score = score.bug_type_scores.get(bug_type)
                .copied()
                .unwrap_or(50.0);
            let combined = score.overall_score * 0.5 + type_score * 0.5;
            if combined > best.as_ref().map_or(0.0, |(_, s)| *s) {
                best = Some((agent_id.clone(), combined));
            }
        }
        best.map(|(id, _)| id)
    }

    /// Analyze failure patterns and generate optimization actions.
    pub fn analyze_and_optimize(
        &self,
        agent_metrics: &[super::analytics::AgentMetrics],
        failure_patterns: &[super::analytics::FailurePattern],
    ) -> Vec<OptimizationAction> {
        let mut actions = Vec::new();

        // 1. Agents with low success rate → add constraints
        for am in agent_metrics {
            if am.total_fixes >= 3 && am.success_rate < 50.0 {
                let prompt_boost = self.suggest_prompt_boost(&am.agent_id, failure_patterns);
                actions.push(OptimizationAction {
                    action_type: "prompt_boost".into(),
                    target_agent: am.agent_id.clone(),
                    reason: format!(
                        "成功率 {:.0}%（{}次成功/{}次总计），低于 50% 阈值",
                        am.success_rate, am.success_count, am.total_fixes
                    ),
                    change: prompt_boost,
                    confidence: 0.8,
                });
            }
        }

        // 2. Common failure patterns → targeted constraints
        for fp in failure_patterns.iter().take(5) {
            if fp.count >= 3 {
                for agent in &fp.agents {
                    let constraint = self.suggest_constraint_for_error(&fp.error_category);
                    actions.push(OptimizationAction {
                        action_type: "adjust_constraint".into(),
                        target_agent: agent.clone(),
                        reason: format!(
                            "「{}」失败 {} 次",
                            fp.error_category.chars().take(50).collect::<String>(),
                            fp.count
                        ),
                        change: constraint,
                        confidence: 0.7,
                    });
                }
            }
        }

        // 3. Route optimization — suggest reassignment
        let mut agent_rates: HashMap<String, f64> = HashMap::new();
        for am in agent_metrics {
            agent_rates.insert(am.agent_id.clone(), am.success_rate);
        }
        if let Some((worst_id, worst_rate)) = agent_rates.iter()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        {
            if *worst_rate < 40.0 && agent_metrics.len() > 1 {
                let best_id = agent_rates.iter()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(id, _)| id.clone())
                    .unwrap_or_default();
                actions.push(OptimizationAction {
                    action_type: "reroute".into(),
                    target_agent: worst_id.clone(),
                    reason: format!(
                        "{} 成功率 {:.0}% 最低，建议将部分 bug 路由给 {}",
                        worst_id, worst_rate, best_id
                    ),
                    change: format!("减少 {} 分配量，增加 {} 分配", worst_id, best_id),
                    confidence: 0.6,
                });
            }
        }

        actions
    }

    /// Suggest prompt enhancement based on failure patterns.
    fn suggest_prompt_boost(&self, agent_id: &str, patterns: &[super::analytics::FailurePattern]) -> String {
        let relevant: Vec<&str> = patterns.iter()
            .filter(|p| p.agents.contains(&agent_id.to_string()))
            .map(|p| p.error_category.as_str())
            .collect();

        if relevant.is_empty() {
            return format!("建议 {} 增加通用约束：先读 AGENTS.md 再修复", agent_id);
        }

        let hints: Vec<String> = relevant.iter().map(|cat| {
            if cat.contains("编译") || cat.contains("compile") {
                "修改后必须运行编译检查".into()
            } else if cat.contains("SQL") || cat.contains("sql") {
                "SQL 修改后必须用 EXPLAIN 验证".into()
            } else if cat.contains("类型") || cat.contains("type") {
                "注意 TypeScript/Java 类型匹配".into()
            } else if cat.contains("导入") || cat.contains("import") {
                "检查 import 路径和包依赖".into()
            } else {
                format!("注意「{}」相关问题", cat.chars().take(20).collect::<String>())
            }
        }).collect();

        format!(
            "基于 {} 的失败模式分析，建议 {} 增加约束：\n- {}",
            relevant.len(), agent_id, hints.join("\n- ")
        )
    }

    /// Suggest constraint for a specific error pattern.
    fn suggest_constraint_for_error(&self, error_category: &str) -> String {
        if error_category.contains("编译") || error_category.contains("compile") {
            "修改后必须运行 cargo check / mvn compile / npm run build 确认编译通过".into()
        } else if error_category.contains("SQL") || error_category.contains("mapper") {
            "修改 Mapper XML 后必须用 EXPLAIN 验证查询计划".into()
        } else if error_category.contains("字段") || error_category.contains("column") {
            "涉及数据库字段变更时，必须走通全链路 6 环".into()
        } else if error_category.contains("权限") || error_category.contains("auth") {
            "注意权限检查和鉴权逻辑".into()
        } else {
            format!("针对「{}」错误增加专项检查", error_category.chars().take(30).collect::<String>())
        }
    }

    /// Get accumulated extra constraints for an agent.
    pub fn get_extra_constraints(&self, agent_id: &str) -> Vec<String> {
        self.extra_constraints.get(agent_id).cloned().unwrap_or_default()
    }
}

impl Default for SelfOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_scores() {
        let mut opt = SelfOptimizer::new();
        opt.update_scores("guanyu", "backend", true, 300.0);
        opt.update_scores("guanyu", "backend", false, 600.0);

        let score = opt.scores.get("guanyu").unwrap();
        assert!(score.success_rate > 0.0);
        assert!(score.bug_type_scores.contains_key("backend"));
    }

    #[test]
    fn test_best_agent_for() {
        let mut opt = SelfOptimizer::new();
        opt.update_scores("guanyu", "backend", true, 200.0);
        opt.update_scores("guanyu", "backend", true, 250.0);
        opt.update_scores("zhaoyun", "frontend", true, 150.0);
        opt.update_scores("zhaoyun", "frontend", true, 180.0);

        assert_eq!(opt.best_agent_for("backend"), Some("guanyu".into()));
        assert_eq!(opt.best_agent_for("frontend"), Some("zhaoyun".into()));
    }

    #[test]
    fn test_analyze_empty() {
        let opt = SelfOptimizer::new();
        let actions = opt.analyze_and_optimize(&[], &[]);
        assert!(actions.is_empty());
    }
}
