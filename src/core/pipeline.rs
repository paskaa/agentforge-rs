//! Pipeline handlers — the full fix → test → verify workflow.
//!
//! Each handler receives context and a task, processes it, and may emit follow-up tasks.


use redis::AsyncCommands;
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
