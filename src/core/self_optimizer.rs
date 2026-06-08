//! L5 Self-Optimizer — AI-driven optimization based on analytics data.
//!
//! Analyzes failure patterns → adjusts prompts/constraints/routing → feedback loop.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

    /// Load scores from JSON file (归一化 agent_id).
    pub fn load(path: &str) -> Self {
        let data = std::fs::read_to_string(path).unwrap_or_default();
        let raw: HashMap<String, AgentScore> =
            serde_json::from_str(&data).unwrap_or_default();
        // 归一化并合并
        let mut scores: HashMap<String, AgentScore> = HashMap::new();
        for (id, score) in raw {
            let normalized = normalize_agent_id(&id);
            tracing::debug!("[SelfOptimizer] load: {} -> {}", id, normalized);
            let mut score = score;
            score.agent_id = normalized.clone();
            if let Some(existing) = scores.get_mut(&normalized) {
                if score.overall_score > existing.overall_score {
                    *existing = score;
                }
            } else {
                scores.insert(normalized, score);
            }
        }
        // Load extra constraints from separate file
        let extra_path = format!("{}.constraints", path);
        let extra_data = std::fs::read_to_string(&extra_path).unwrap_or_default();
        let extra_constraints: HashMap<String, Vec<String>> =
            serde_json::from_str(&extra_data).unwrap_or_default();
        Self { scores, extra_constraints }
    }

    /// Save scores to JSON file (强制归一化 key).
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut normalized: HashMap<String, AgentScore> = HashMap::new();
        for (k, v) in &self.scores {
            let nk = normalize_agent_id(k);
            let mut v = v.clone();
            v.agent_id = nk.clone();
            if let Some(existing) = normalized.get(&nk) {
                if v.overall_score > existing.overall_score {
                    normalized.insert(nk, v);
                }
            } else {
                normalized.insert(nk, v);
            }
        }
        let json = serde_json::to_string_pretty(&normalized)?;
        std::fs::write(path, json)?;
        // Also persist extra constraints
        let extra_path = format!("{}.constraints", path);
        let extra_json = serde_json::to_string_pretty(&self.extra_constraints)?;
        std::fs::write(extra_path, extra_json)
    }

    /// Update agent scores based on recent fix results.
    pub fn update_scores(
        &mut self,
        agent_id: &str,
        bug_type: &str,
        success: bool,
        duration_s: f64,
    ) {
        let agent_id = normalize_agent_id(agent_id);
        let score = self.scores.entry(agent_id.clone()).or_insert_with(|| AgentScore {
            agent_id: agent_id.clone(),
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
        for fp in failure_patterns.iter().filter(|p| p.error_category != "其他错误").take(5) {
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

    /// Apply optimization actions — populate extra_constraints from generated actions.
    /// Deduplicates and caps at MAX_EXTRA_CONSTRAINTS per agent.
    const MAX_EXTRA_CONSTRAINTS: usize = 10;

    pub fn apply_actions(&mut self, actions: &[OptimizationAction]) {
        for action in actions {
            if action.confidence < 0.6 {
                continue;
            }
            // Skip noise: git HEAD, attempt=N, empty category patterns
            let change = &action.change;
            if change.contains("HEAD is now at") || change.contains("attempt=")
                || change.chars().filter(|c| *c != '\n' && *c != '-' && *c == ' ').count() < 3 {
                continue;
            }
            let constraints = self.extra_constraints
                .entry(action.target_agent.clone())
                .or_insert_with(Vec::new);
            // Deduplicate: skip if similar constraint already exists
            let is_dup = constraints.iter().any(|existing| {
                let a: Vec<&str> = existing.split_whitespace().collect();
                let b: Vec<&str> = change.split_whitespace().collect();
                let common = a.iter().filter(|w| b.contains(w)).count();
                common > a.len() * 6 / 10
            });
            if !is_dup && constraints.len() < Self::MAX_EXTRA_CONSTRAINTS {
                constraints.push(change.clone());
            }
        }
    }

    /// Suggest prompt enhancement based on failure patterns.
    fn suggest_prompt_boost(&self, agent_id: &str, patterns: &[super::analytics::FailurePattern]) -> String {
        // Get patterns relevant to this agent
        let relevant: Vec<(&str, i64)> = patterns.iter()
            .filter(|p| p.agents.contains(&agent_id.to_string()) && p.count >= 1 && p.error_category != "其他错误")
            .map(|p| (p.error_category.as_str(), p.count))
            .collect();

        if relevant.is_empty() {
            return format!("建议 {} 修复后运行编译/语法检查验证", agent_id);
        }

        let hints: Vec<String> = relevant.iter().take(3).map(|(cat, count)| {
            match *cat {
                "编译失败" => format!("编译失败 {} 次 → 修改后必须运行 mvn compile / cargo check", count),
                "SQL/数据库错误" => format!("SQL 错误 {} 次 → 修改 Mapper 后用 EXPLAIN 验证", count),
                "空指针/空值异常" => format!("空指针 {} 次 → 增加 null 检查和 Optional 处理", count),
                "类型不匹配" => format!("类型错误 {} 次 → 注意泛型和类型转换", count),
                "Git 合并冲突" => format!("合并冲突 {} 次 → 修复前先 git pull --rebase", count),
                "API/接口错误" => format!("API 错误 {} 次 → 检查请求参数和响应格式", count),
                "前端组件错误" => format!("前端错误 {} 次 → 检查 Vue 响应式和生命周期", count),
                "权限/鉴权错误" => format!("权限错误 {} 次 → 检查鉴权注解和拦截器配置", count),
                "文件/路径错误" => format!("文件错误 {} 次 → 确认文件路径和权限", count),
                "请求超时" => format!("超时 {} 次 → 优化查询性能或增加超时时间", count),
                "修复逻辑错误" => format!("修复逻辑错误 {} 次 → 修复前先分析全链路数据流", count),
                _ => format!("「{}」失败 {} 次 → 增加针对性检查", cat, count),
            }
        }).collect();

        format!(
            "基于 {} 种失败模式分析：
- {}",
            relevant.len(), hints.join("
- ")
        )
    }

    /// Suggest constraint for a specific error pattern.
    fn suggest_constraint_for_error(&self, error_category: &str) -> String {
        match error_category {
            "编译失败" => "修改代码后必须运行 mvn compile / cargo check 确认编译通过，禁止提交编译不通过的代码".into(),
            "SQL/数据库错误" => "修改 Mapper/SQL 后必须用 EXPLAIN 验证查询计划，涉及字段变更必须走全链路 6 环".into(),
            "空指针/空值异常" => "增加 null 检查，使用 Optional/?.unwrap_or_default() 防御性编程".into(),
            "类型不匹配" => "注意泛型和类型转换，修改后运行类型检查（vue-tsc / mvn compile）".into(),
            "Git 合并冲突" => "修复前先 git pull --rebase 同步最新代码，解决冲突后再提交".into(),
            "API/接口错误" => "检查请求参数校验和响应格式，确保前后端字段名一致".into(),
            "前端组件错误" => "检查 Vue 响应式数据绑定和生命周期钩子，确保组件更新触发正确".into(),
            "权限/鉴权错误" => "检查 @PreAuthorize 注解和 SecurityConfig 配置，确保接口鉴权正确".into(),
            "文件/路径错误" => "确认文件路径存在且有读写权限，使用相对路径避免硬编码".into(),
            "请求超时" => "优化查询性能（加索引/分页），或调整超时配置".into(),
            "修复逻辑错误" => "修复前分析全链路数据流（录入→保存→查询→修改→删除→关联），确保逻辑完整".into(),
            "其他错误" => "修复后运行完整测试套件验证".into(),
            _ => format!("针对「{}」错误增加专项检查", error_category.chars().take(20).collect::<String>()),
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



// ═══════════════════════════════════════════════════════════════
// 结构化评分框架 — 4维评估体系 (Anthropic Harness Engineering)
// ═══════════════════════════════════════════════════════════════

/// 4维结构化评分
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StructuredScore {
    /// 设计质量：模块间命名规范、错误处理模式、API设计风格是否全局统一
    pub design_quality: u8,    // 1-5
    /// 工艺性：边界条件覆盖、类型安全、性能热点处理、日志充分
    pub craft: u8,             // 1-5
    /// 功能性：功能是否按预期工作、测试是否通过、用户路径是否畅通
    pub functionality: u8,     // 1-5
    /// 风格一致性：与项目现有代码风格的匹配度
    pub style_consistency: u8, // 1-5
    /// 附加说明
    pub notes: String,
}

impl StructuredScore {
    pub fn total(&self) -> u32 {
        self.design_quality as u32 + self.craft as u32 +
        self.functionality as u32 + self.style_consistency as u32
    }

    pub fn max_total() -> u32 { 20 }

    pub fn pass_threshold() -> u32 { 12 } // 60% 通过线

    pub fn is_pass(&self) -> bool {
        self.total() >= Self::pass_threshold()
            && self.functionality >= 3 // 功能性最低要求
    }

    pub fn to_verdict(&self) -> String {
        if self.is_pass() {
            format!("VERDICT: PASS (总分{}/{}，设计{} 工艺{} 功能{} 风格{})",
                self.total(), Self::max_total(),
                self.design_quality, self.craft, self.functionality, self.style_consistency)
        } else {
            format!("VERDICT: FAIL (总分{}/{}，设计{} 工艺{} 功能{} 风格{})
原因: {}",
                self.total(), Self::max_total(),
                self.design_quality, self.craft, self.functionality, self.style_consistency,
                self.notes)
        }
    }
}

/// 从 VERDICT 输出中解析结构化评分
pub fn parse_structured_score(output: &str) -> Option<StructuredScore> {
    // 尝试从输出中提取评分维度
    let extract = |label: &str| -> Option<u8> {
        output.lines().find_map(|line| {
            if line.contains(label) {
                line.chars().find(|c| c.is_ascii_digit())
                    .and_then(|c| c.to_digit(10))
                    .map(|n| n as u8)
            } else { None }
        })
    };

    let design = extract("设计质量").unwrap_or(3);
    let craft = extract("工艺性").unwrap_or(3);
    let func = extract("功能性").unwrap_or(3);
    let style = extract("风格一致性").unwrap_or(3);

    // 只有找到明确评分才返回
    if output.contains("设计质量") || output.contains("craft") {
        Some(StructuredScore {
            design_quality: design,
            craft,
            functionality: func,
            style_consistency: style,
            notes: output.lines().last().unwrap_or("").to_string(),
        })
    } else {
        None
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
