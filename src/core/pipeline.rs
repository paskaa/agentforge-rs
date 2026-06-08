//! Pipeline handlers — the full fix → test → verify workflow.
//!
//! Each handler receives context and a task, processes it, and may emit follow-up tasks.


use redis::AsyncCommands;


// ═══════════════════════════════════════════════════════════════
// VERDICT 协议 — 强制二元输出，流水线自动化决策的基础
// ═══════════════════════════════════════════════════════════════

/// VERDICT 二元输出 — 将主观判断转化为机器可处理的信号
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Verdict {
    Pass,
    Fail(String), // 失败原因
    Unknown,      // 未识别到 VERDICT
}

impl Verdict {
    pub fn is_pass(&self) -> bool { matches!(self, Verdict::Pass) }
    pub fn is_fail(&self) -> bool { matches!(self, Verdict::Fail(_)) }

    pub fn to_comment(&self, agent_name: &str, bug_id: &str) -> String {
        match self {
            Verdict::Pass => format!("[🤖 {}] Bug #{} VERDICT: PASS", agent_name, bug_id),
            Verdict::Fail(reason) => format!("[🤖 {}] Bug #{} VERDICT: FAIL
原因: {}", agent_name, bug_id, reason),
            Verdict::Unknown => format!("[🤖 {}] Bug #{} VERDICT: UNKNOWN", agent_name, bug_id),
        }
    }
}

/// 从输出中解析 VERDICT
pub fn parse_verdict(output: &str) -> Verdict {
    // 优先匹配 "VERDICT: PASS" 或 "VERDICT: FAIL"
    for line in output.lines() {
        let line = line.trim();
        if line.contains("VERDICT:") || line.contains("VERDICT：") {
            if line.contains("PASS") || line.contains("通过") {
                return Verdict::Pass;
            }
            if line.contains("FAIL") || line.contains("失败") {
                // 提取失败原因：VERDICT: FAIL [原因] 或 VERDICT：失败 [原因]
                let reason = line
                    .split(&['：', ':'][..])
                    .nth(1)
                    .map(|s| {
                        s.trim()
                            .trim_start_matches("FAIL")
                            .trim_start_matches("失败")
                            .trim_start_matches(&['[', '（', '('][..])
                            .trim_end_matches(&[']', '）', ')'][..])
                            .trim()
                            .to_string()
                    })
                    .unwrap_or_else(|| "未提供原因".to_string());
                return Verdict::Fail(reason);
            }
        }
    }
    Verdict::Unknown
}

/// 轮次预算配置
pub struct RoundBudget {
    pub max_fix_rounds: u32,
    pub max_test_rounds: u32,
    pub max_verify_rounds: u32,
    pub max_total_rounds: u32,
}

impl Default for RoundBudget {
    fn default() -> Self {
        Self {
            max_fix_rounds: 3,
            max_test_rounds: 3,
            max_verify_rounds: 2,
            max_total_rounds: 8,
        }
    }
}

/// 检查轮次预算
pub async fn check_round_budget(
    bug_id: &str,
    agent: &str,
    redis: &mut redis::aio::MultiplexedConnection,
    budget: &RoundBudget,
) -> Result<bool, String> {
    let key = format!("round_budget:{}:{}", bug_id, agent);
    let count: i32 = redis.clone().get(&key).await.unwrap_or(0);
    let max = match agent {
        "guanyu" | "zhaoyun" | "xunyu" => budget.max_fix_rounds,
        "zhangfei" => budget.max_test_rounds,
        "huatuo" => budget.max_verify_rounds,
        _ => budget.max_total_rounds,
    };
    Ok(count >= max as i32)
}

/// 增加轮次计数
pub async fn increment_round(
    bug_id: &str,
    agent: &str,
    redis: &mut redis::aio::MultiplexedConnection,
) {
    let key = format!("round_budget:{}:{}", bug_id, agent);
    let _: redis::RedisResult<i32> = redis.clone().incr(&key, 1).await;
    // 设置 TTL 为 7 天
    let _: redis::RedisResult<()> = redis.clone().expire(&key, 604800).await;
}

/// 文件快照 Diff
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileDiff {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

impl FileDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.added.is_empty() { parts.push(format!("新增{}个", self.added.len())); }
        if !self.modified.is_empty() { parts.push(format!("修改{}个", self.modified.len())); }
        if !self.deleted.is_empty() { parts.push(format!("删除{}个", self.deleted.len())); }
        if parts.is_empty() { "无变更".to_string() } else { parts.join(", ") }
    }

    pub fn detail(&self) -> String {
        let mut lines = Vec::new();
        for f in &self.added { lines.push(format!("  + {}", f)); }
        for f in &self.modified { lines.push(format!("  ~ {}", f)); }
        for f in &self.deleted { lines.push(format!("  - {}", f)); }
        lines.join("\n")
    }
}

/// 捕获项目目录快照（文件路径 → size + mtime）
fn capture_snapshot(project_dir: &str) -> std::collections::HashMap<String, (u64, String)> {
    let mut snapshot = std::collections::HashMap::new();
    let output = std::process::Command::new("find")
        .args([project_dir, "-type", "f", "-name", "*.java", "-o", "-name", "*.vue", "-o", "-name", "*.ts", "-o", "-name", "*.js", "-o", "-name", "*.sql", "-o", "-name", "*.xml"])
        .output();
    if let Ok(out) = output {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let path = line.trim().to_string();
            if let Ok(meta) = std::fs::metadata(&path) {
                let size = meta.len();
                let mtime = format!("{:?}", meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH));
                snapshot.insert(path, (size, mtime));
            }
        }
    }
    snapshot
}

/// 计算两个快照的差异
pub fn compute_diff(before: &std::collections::HashMap<String, (u64, String)>,
                     after: &std::collections::HashMap<String, (u64, String)>) -> FileDiff {
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    for (path, _) in after {
        if !before.contains_key(path) {
            added.push(path.clone());
        } else if before[path] != after[path] {
            modified.push(path.clone());
        }
    }
    for (path, _) in before {
        if !after.contains_key(path) {
            deleted.push(path.clone());
        }
    }

    // 只保留 src/ 下的文件，过滤掉 target/ node_modules/ 等
    let filter = |p: &mut Vec<String>| {
        p.retain(|f| f.contains("/src/") || f.contains("/src\\"));
        p.sort();
    };
    filter(&mut added);
    filter(&mut modified);
    filter(&mut deleted);

    FileDiff { added, modified, deleted }
}

/// 在项目目录前后拍摄快照并计算差异
pub fn snapshot_and_diff(project_dir: &str, before: &std::collections::HashMap<String, (u64, String)>) -> FileDiff {
    let after = capture_snapshot(project_dir);
    compute_diff(before, &after)
}

/// 拍摄快照（供外部调用）
pub fn take_snapshot(project_dir: &str) -> std::collections::HashMap<String, (u64, String)> {
    capture_snapshot(project_dir)
}

/// Known human accounts — their bugs get fixed but status/assignment unchanged.
pub const HUMAN_ACCOUNTS: &[&str] = &[
    "chenxj", "sjjh", "admin", "doctor1", "ssshs1",
    "yangkexiang", "yangkeixang",
];

/// Keyword-based routing for fixer agents.
const ROUTE_KW: &[(&str, &[&str])] = &[
    ("xunyu", &["数据库", "sql", "慢查询", "索引", "字段缺失", "ddl", "schema"]),
    ("guanyu", &["后端", "java", "api", "接口", "spring", "service", "controller",
                  "mapper", "保存失败", "事务", "缓存", "校验", "签发", "退回", "撤回", "错误提示",
                  "审计", "完诊", "操作失败", "div_log", "SQL", "执行科室", "库存", "发药", "计费"]),
    ("zhaoyun", &["前端", "vue", "界面", "显示", "弹窗", "按钮", "回显", "刷新",
                  "不规范", "缺失", "操作项", "命名", "加载", "过滤", "提示语",
                  "样式", "组件", "渲染", "模板", "提交申请", "检查申请", "报卡", "手术安排"]),
];

/// Route a bug title to the best fixer agent.
pub fn route_bug(title: &str) -> &str {
    let t = title.to_lowercase();
    for (agent, kws) in ROUTE_KW {
        for kw in *kws {
            if t.contains(kw) {
                return agent;
            }
        }
    }
    "zhaoyun"
}

/// Determine if a reporter account is human.
pub fn is_human(account: &str) -> bool {
    HUMAN_ACCOUNTS.iter().any(|h| h.eq_ignore_ascii_case(account))
}

/// Result from Claude Code fix attempt.
#[derive(Debug, Clone)]
pub struct FixResult {
    pub success: bool,
    pub bug_id: String,
    pub summary: String,
    pub elapsed_s: f64,
    pub stdout: String,
    pub changes: u32,
}

/// Parse bug IDs from a PM analysis message.
pub fn parse_bugs_from_message(msg: &str) -> Vec<(String, String)> {
    let mut bugs = Vec::new();
    for line in msg.lines() {
        if let Some(pos) = line.find('#') {
            let after = &line[pos+1..];
            let end = after.find(|c: char| c == '：' || c == ':' || c.is_whitespace())
                .unwrap_or(after.len());
            let bid = after[..end].trim().to_string();
            if bid.chars().all(|c| c.is_ascii_digit()) && bid.len() >= 2 {
                let title = after[end..].trim().trim_start_matches(&['：', ':', ' '][..]).to_string();
                bugs.push((bid, title));
            }
        }
    }
    bugs
}

/// Extract bug ID from a message like "Bug #462（测试标题）回归测试完成...".
pub fn extract_bug_id(msg: &str) -> String {
    msg.split('#').nth(1)
        .and_then(|s| s.split(|c: char| c == '：' || c == ':' || c == '（' || c == '(' || c.is_whitespace()).next())
        .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Extract reporter from a message like "...已指派回提出人 陈显精(chenxj)。".
pub fn extract_reporter(msg: &str) -> String {
    // Try "提出人: XXX" format
    if let Some(pos) = msg.find("提出人") {
        let after = &msg[pos + "提出人".len()..];
        let after = after.trim_start_matches(&[':', '：', ' '][..]);
        // Take until period or newline
        let end = after.find(['。', '\n'])
            .unwrap_or(after.len());
        let name = after[..end].trim().to_string();
        if !name.is_empty() { return name; }
    }
    // Try "已指派回 XXX" format (zhangfei reply)
    if let Some(pos) = msg.find("已指派回") {
        let after = &msg[pos + "已指派回".len()..];
        let after = after.trim_start_matches(&[' ', '：', ':'][..]);
        let end = after.find(['。', '\n']).unwrap_or(after.len());
        let name = after[..end].trim().to_string();
        if !name.is_empty() { return name; }
    }
    // Try "chenxj" format
    if let Some(_pos) = msg.find("(chenxj)") {
        return "chenxj".to_string();
    }
    if let Some(_pos) = msg.find("(yangkexiang)") {
        return "yangkexiang".to_string();
    }
    // Default to "chenxj" when extraction fails (prevents empty pipeline loops)
    "chenxj".to_string()
}

/// Build a fix task message for Redis queue.
pub fn build_fix_task(bid: &str, title: &str, fixer: &str) -> serde_json::Value {
    serde_json::json!({
        "agent_id": fixer,
        "message": format!("请修复 Bug #{}：{}", bid, title),
        "source": "pm_routed",
        "sender_id": "liubei",
        "chat_id": "",
        "is_dm": "true",
        "msg_id": format!("pm-routed-{}-{}", bid, chrono::Local::now().timestamp()),
        "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    })
}


/// Check if a bug should be skipped before enqueuing.
/// Returns (should_skip, reason).
pub async fn should_skip_bug(
    bug_id: &str,
    fixer: &str,
    redis_conn: &mut redis::aio::MultiplexedConnection,
    zentao_client: &crate::core::zentao::ZentaoClient,
) -> (bool, String) {
    // ── Check 1: Zentao status (resolved/closed = skip) ──
    match zentao_client.get_bug(bug_id).await {
        Ok(bug) => {
            if bug.status == "resolved" || bug.status == "closed" {
                return (true, format!("禅道状态已={}", bug.status));
            }
        }
        Err(e) => {
            tracing::warn!("[dedup] Failed to check Zentao status for Bug#{}: {}", bug_id, e);
            // Don't skip on API error — might be a transient failure
        }
    }

    // ── Check 2: develop branch already has fix commit ──
    let output = std::process::Command::new("git")
        .args(["log", "origin/develop", "--grep", &format!("Bug#{}", bug_id), "--oneline", "-1"])
        .current_dir("/root/.openclaw/workspace/his-repo")
        .output();
    if let Ok(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout).to_string();
        if !stdout.trim().is_empty() {
            return (true, format!("develop 已有 commit: {}", stdout.trim()));
        }
    }

    // ── Check 3: Redis lock (agent already processing) ──
    let lock_key = format!("codex_lock:{}", fixer);
    let lock_exists: bool = redis_conn.clone().exists(&lock_key).await.unwrap_or(false);
    if lock_exists {
        return (true, format!("agent {} 正在处理中", fixer));
    }

    // ── Check 4: Already in queue (dedup within queue) ──
    let queue = format!("agent-work-queue:fix:{}", fixer);
    let existing: Vec<String> = redis_conn.clone().lrange(&queue, 0, -1).await.unwrap_or_default();
    let bug_marker = format!("Bug #{}", bug_id);
    if existing.iter().any(|s| s.contains(&bug_marker)) {
        return (true, "已在队列中".to_string());
    }

    (false, String::new())
}




// ═══════════════════════════════════════════════════════════════
// Phase 3: 上下文交接优化 — 结构化交接卡
// ═══════════════════════════════════════════════════════════════

/// 结构化交接卡 — Agent 之间传递完整上下文
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HandoffCard {
    pub bug_id: String,
    pub bug_title: String,
    pub reporter: String,
    pub from_agent: String,
    pub to_agent: String,
    pub stage: String,              // "fix" | "db_review" | "test" | "verify" | "archive"
    pub file_diff: Option<FileDiff>,
    pub verification_summary: Option<String>,
    pub previous_rounds: u32,
    pub context_summary: String,
    pub timestamp: String,
}

impl HandoffCard {
    pub fn new(bug_id: &str, bug_title: &str, reporter: &str, from: &str, to: &str, stage: &str) -> Self {
        Self {
            bug_id: bug_id.to_string(),
            bug_title: bug_title.to_string(),
            reporter: reporter.to_string(),
            from_agent: from.to_string(),
            to_agent: to.to_string(),
            stage: stage.to_string(),
            file_diff: None,
            verification_summary: None,
            previous_rounds: 0,
            context_summary: String::new(),
            timestamp: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }

    pub fn to_message(&self) -> String {
        let mut msg = format!(
            "📋 交接卡: Bug #{} ({})
来源: {} → {}
阶段: {}
提出人: {}",
            self.bug_id, self.bug_title, self.from_agent, self.to_agent, self.stage, self.reporter
        );
        if let Some(diff) = &self.file_diff {
            if !diff.is_empty() {
                msg.push_str(&format!("
文件变更: {}", diff.summary()));
                msg.push_str(&format!("
{}", diff.detail()));
            }
        }
        if let Some(summary) = &self.verification_summary {
            msg.push_str(&format!("
验证结果: {}", summary));
        }
        if self.previous_rounds > 0 {
            msg.push_str(&format!("
已执行轮次: {}", self.previous_rounds));
        }
        if !self.context_summary.is_empty() {
            msg.push_str(&format!("
上下文: {}", self.context_summary));
        }
        msg
    }

    /// 从 Redis 获取交接卡
    pub async fn load(bug_id: &str, redis: &mut redis::aio::MultiplexedConnection) -> Option<Self> {
        let key = format!("handoff:{}", bug_id);
        let json: String = redis.clone().get(&key).await.ok()?;
        serde_json::from_str(&json).ok()
    }

    /// 保存交接卡到 Redis (TTL 24小时)
    pub async fn save(&self, redis: &mut redis::aio::MultiplexedConnection) {
        let key = format!("handoff:{}", self.bug_id);
        if let Ok(json) = serde_json::to_string(self) {
            let _: redis::RedisResult<()> = redis.clone().set_ex(&key, &json, 86400).await;
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Phase 3: 实时可观测性增强 — 流式事件
// ═══════════════════════════════════════════════════════════════

/// 流式事件类型 — 用于 WebSocket 实时推送
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum PipelineEvent {
    #[serde(rename = "agent_start")]
    AgentStart { agent_id: String, bug_id: String, stage: String },
    
    #[serde(rename = "agent_end")]
    AgentEnd { agent_id: String, bug_id: String, verdict: String, duration_ms: u64 },
    
    #[serde(rename = "file_changed")]
    FileChanged { bug_id: String, path: String, change_type: String },
    
    #[serde(rename = "budget_update")]
    BudgetUpdate { bug_id: String, agent: String, current: u32, max: u32 },
    
    #[serde(rename = "degradation")]
    Degradation { bug_id: String, level: String, reason: String },
    
    #[serde(rename = "handoff")]
    Handoff { bug_id: String, from: String, to: String, stage: String },
    
    #[serde(rename = "pipeline_complete")]
    PipelineComplete { bug_id: String, total_duration_ms: u64, stages: Vec<String> },
}

impl PipelineEvent {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn timestamp(&self) -> String {
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_human_positive() {
        assert!(is_human("chenxj"));
        assert!(is_human("yangkexiang"));
        assert!(is_human("sjjh"));
    }

    #[test]
    fn test_is_human_negative() {
        assert!(!is_human("zhaoyun"));
        assert!(!is_human("guanyu"));
        assert!(!is_human(""));
    }

    #[test]
    fn test_parse_bugs() {
        let msg = "请分析并分派以下 3 个 Bug：\n  #462：目录管理-诊疗目录 编辑弹窗中所需标本下拉框数据加载失败\n  #456：门诊医生站：诊疗类医嘱保存后类型变更为检查\n  #471：手术管理-门诊手术安排 手术申请查询结果中混入住院检验申请单数据";
        let bugs = parse_bugs_from_message(msg);
        assert_eq!(bugs.len(), 3);
        assert_eq!(bugs[0].0, "462");
        assert!(bugs[0].1.contains("标本"));
        assert_eq!(bugs[1].0, "456");
        assert_eq!(bugs[2].0, "471");
    }

    #[test]
    fn test_parse_bugs_empty() {
        assert!(parse_bugs_from_message("你好世界").is_empty());
    }

    #[test]
    fn test_route_bug_kw() {
        assert_eq!(route_bug("前端vue组件渲染异常"), "zhaoyun");
        assert_eq!(route_bug("后端java接口报500"), "guanyu");
        assert_eq!(route_bug("数据库sql查询优化"), "xunyu");
    }

    #[test]
    fn test_build_fix_task() {
        let task = build_fix_task("462", "测试标题", "zhaoyun");
        assert_eq!(task["agent_id"], "zhaoyun");
        assert_eq!(task["source"], "pm_routed");
        assert!(task["message"].as_str().unwrap().contains("462"));
    }

    #[test]
    fn test_keyword_in_list() {
        // All keywords should have at least one entry
        assert!(!ROUTE_KW.is_empty());
        for (agent, kws) in ROUTE_KW {
            assert!(!kws.is_empty(), "Agent {} has no keywords", agent);
        }
    }

    #[test]
    fn test_route_xunyu_exact_match() {
        // These should match xunyu specifically
        assert_eq!(route_bug("数据库字段缺失导致ddl执行失败"), "xunyu");
        assert_eq!(route_bug("sql慢查询索引优化"), "xunyu");
    }

    #[test]
    fn test_route_guanyu_exact_match() {
        assert_eq!(route_bug("后端spring service事务处理异常"), "guanyu");
        assert_eq!(route_bug("mapper接口校验签发逻辑"), "guanyu");
    }

    #[test]
    fn test_route_zhaoyun_exact_match() {
        assert_eq!(route_bug("前端vue弹窗按钮渲染错误"), "zhaoyun");
        assert_eq!(route_bug("界面组件样式提示语不规范"), "zhaoyun");
    }
}
