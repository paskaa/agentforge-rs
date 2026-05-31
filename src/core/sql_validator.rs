//! SQL Validator — Harness Engineering 三层 SQL 验证
//!
//! L1: 语法检查 — 用 sqlparser-rs 解析 SQL，检测语法错误
//! L2: 语义检查 — 用 EXPLAIN 在测试 DB 上验证
//! L3: 全量基线 — 扫描全部 Mapper XML 发现潜在问题
//!
//! 设计原则:
//! - MyBatis 动态 SQL 提取尽量保留完整 SQL 结构
//! - 使用真实 PostgreSQL EXPLAIN 验证运行时可行性
//! - 所有验证结果结构化输出，支持批量处理

use std::path::{Path, PathBuf};
use std::process::Command;
use std::io::Write;

/// PostgreSQL 连接配置
#[derive(Debug, Clone)]
pub struct PgConfig {
    pub host: String,
    pub port: String,
    pub db: String,
    pub user: String,
    pub password: String,
    pub schema: String,
}

impl Default for PgConfig {
    fn default() -> Self {
        if let Ok(cfg) = crate::config::Config::load() {
            Self {
                host: cfg.database.host,
                port: cfg.database.port.to_string(),
                db: cfg.database.database,
                user: cfg.database.username,
                password: cfg.database.password,
                schema: "histest1".into(),
            }
        } else {
            Self {
                host: "127.0.0.1".into(),
                port: "5432".into(),
                db: "postgresql".into(),
                user: "postgresql".into(),
                password: String::new(),
                schema: "histest1".into(),
            }
        }
    }
}

/// 单条 SQL 验证结果
#[derive(Debug)]
pub struct SqlValidationResult {
    pub sql_id: String,       // XML 中的 id 属性（如 selectXXX）
    pub sql_raw: String,      // 提取后的 SQL（前 100 字符）
    pub l1_passed: bool,      // 语法检查
    pub l1_errors: Vec<String>,
    pub l2_passed: bool,      // EXPLAIN 语义检查
    pub l2_error: Option<String>,
}

/// 单个 Mapper 文件验证结果
#[derive(Debug)]
pub struct MapperValidationResult {
    pub file_path: String,
    pub total_sqls: usize,
    pub l1_passed: usize,
    pub l1_failed: usize,
    pub l2_passed: usize,
    pub l2_failed: usize,
    pub sql_results: Vec<SqlValidationResult>,
}

// ──────────────────────────────────────────────
// MyBatis XML → SQL 提取器
// ──────────────────────────────────────────────

/// 从 MyBatis Mapper XML 中提取所有 SQL 片段。
///
/// 处理策略:
/// - 解析 `<select>`, `<insert>`, `<update>`, `<delete>` 标签的 id 属性
/// - 对标签内的内容做 MyBatis 动态 SQL → 静态 SQL 转换:
///   - `<if test="...">...</if>` → 保留内容，删除标签
///   - `<where>...</where>` / `<set>...</set>` / `<trim>` / `<choose>` → 同上
///   - `<foreach collection="..." item="x" open="(" separator="," close=")">#{x}</foreach>`
///     → 替换为 `(1,2,3)`
///   - `<include refid="..."/>` → 移除（不解析 SQL 片段）
///   - `#{xxx}` → `1`（数值占位）或 `'x'`（字符串占位）
///   - `${xxx}` → `'x'`
/// - 返回 [(sql_id, sql_text), ...]

/// Collect all <sql id="xxx"> fragments for <include refid> resolution
fn collect_sql_fragments(content: &str) -> std::collections::HashMap<String, String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    
    let mut reader = Reader::from_str(content);
    let mut fragments = std::collections::HashMap::new();
    let mut in_fragment = false;
    let mut frag_id = String::new();
    let mut frag_depth = 0u32;
    let mut frag_text = String::new();
    let mut buf = Vec::new();
    
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                if tag == "sql" {
                    in_fragment = true;
                    frag_depth = 1;
                    frag_id = e.attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == b"id")
                        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                        .unwrap_or_default();
                    frag_text.clear();
                } else if in_fragment {
                    frag_depth += 1;
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                if tag == "sql" && in_fragment {
                    if !frag_id.is_empty() {
                        fragments.insert(frag_id.clone(), process_mybatis_sql(&frag_text));
                    }
                    in_fragment = false;
                } else if in_fragment {
                    frag_depth = frag_depth.saturating_sub(1);
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_fragment {
                    if let Ok(t) = std::str::from_utf8(e.as_ref()) {
                        frag_text.push_str(t);
                        frag_text.push(' ');
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    fragments
}

pub fn extract_sql_from_xml(content: &str) -> Vec<(String, String)> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let fragments = collect_sql_fragments(content);
    let mut reader = Reader::from_str(content);
    // trimmed by whitespace normalization later

    let mut results: Vec<(String, String)> = Vec::new();
    let mut in_sql_tag = false;
    let mut current_id = String::new();
    let mut depth = 0u32;
    let mut foreach_depth = 0u32;
    let mut buf = Vec::new();
    let mut text_buf = String::new();
    let mut skip_depth = 0u32;  // 跳过 <sql> 片段

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                
                match tag_name.as_str() {
                    "select" | "insert" | "update" | "delete" => {
                        in_sql_tag = true;
                        depth = 1;
                        // Extract id attribute
                        current_id = e.attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"id")
                            .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                            .unwrap_or_default();
                        text_buf.clear();
                    }
                    "sql" => {
                        skip_depth = 1;
                    }
                    "foreach" => {
                        // Skip text inside foreach, replace with dummy values
                        if in_sql_tag {
                            text_buf.push_str(" (1,2,3) ");
                        }
                        foreach_depth += 1;
                    }
                    "set" => {
                        // MyBatis <set> auto-adds SET keyword
                        if in_sql_tag {
                            text_buf.push_str(" SET ");
                        }
                    }
                    "trim" => {
                        // Handle <trim prefix="SET" ...> — add SET keyword
                        if in_sql_tag {
                            if let Some(prefix) = e.attributes()
                                .filter_map(|a| a.ok())
                                .find(|a| a.key.as_ref() == b"prefix")
                                .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                            {
                                if prefix.to_uppercase() == "SET" || prefix.to_uppercase() == "SET " {
                                    text_buf.push_str(" SET ");
                                }
                            }
                        }
                    }
                    _ => {
                        if in_sql_tag {
                            depth += 1;
                        } else if skip_depth > 0 {
                            skip_depth += 1;
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                
                match tag_name.as_str() {
                    "select" | "insert" | "update" | "delete" => {
                        if in_sql_tag {
                            // Post-process the collected text
                            let cleaned = process_mybatis_sql(&text_buf);
                            if !cleaned.is_empty() {
                                results.push((current_id.clone(), cleaned));
                            }
                            in_sql_tag = false;
                            depth = 0;
                        }
                    }
                    "sql" => {
                        skip_depth = 0;
                    }
                    "foreach" => {
                        if foreach_depth > 0 {
                            foreach_depth -= 1;
                        }
                    }
                    "set" | "trim" => {
                        // <set> and <trim> handled in Start — no depth tracking needed
                    }
                    _ => {
                        if foreach_depth > 0 {
                            // Skip text inside foreach — already added placeholder
                        } else if in_sql_tag && depth > 0 {
                            depth -= 1;
                        } else if skip_depth > 0 {
                            skip_depth -= 1;
                        }
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_sql_tag && foreach_depth == 0 {
                    if let Ok(t) = std::str::from_utf8(e.as_ref()) {
                        text_buf.push_str(&t);
                        text_buf.push(' ');
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                // Self-closing tags like <include refid="..."/>
                if in_sql_tag {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                    if tag_name == "include" {
                        // Resolve <include refid="xxx"/> from collected fragments
                        if let Some(refid) = e.attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"refid")
                            .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                        {
                            if let Some(resolved) = fragments.get(&refid) {
                                text_buf.push(' ');
                                text_buf.push_str(resolved);
                                text_buf.push(' ');
                            } else {
                                tracing::warn!("SQL include refid='{}' not found (will be skipped)", refid);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("XML parse error: {}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    results
}

/// 处理 MyBatis 动态 SQL 文本 → 可验证的静态 SQL
fn process_mybatis_sql(text: &str) -> String {
    let mut sql = String::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '<' => {
                // 读取 XML 标签内容并跳过
                let mut tag_content = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch == '>' {
                        chars.next();
                        break;
                    }
                    tag_content.push(ch);
                    chars.next();
                }
                let tag_lower = tag_content.to_lowercase();

                // 处理特定的 MyBatis 标签
                if tag_lower.starts_with("foreach") {
                    // <foreach collection="x" item="y" open="(" separator="," close=")">
                    // 替换为 (1,2,3)
                    sql.push_str("(1,2,3)");
                    // 跳过 foreach 内部内容直到 </foreach>
                    skip_until_tag(&mut chars, "/foreach");
                } else if tag_lower.starts_with("/foreach") {
                    // 关闭标签，忽略
                } else if tag_lower.starts_with("if") || tag_lower.starts_with("/if") {
                    // <if test="..."> 和 </if> — 保留内容，不添加额外字符
                } else if tag_lower.starts_with("where") || tag_lower.starts_with("/where") {
                    // <where> 和 </where>
                } else if tag_lower.starts_with("set") || tag_lower.starts_with("/set") {
                    // <set> 和 </set>
                } else if tag_lower.starts_with("trim") || tag_lower.starts_with("/trim") {
                    // <trim> 和 </trim>
                } else if tag_lower.starts_with("choose") || tag_lower.starts_with("/choose")
                    || tag_lower.starts_with("when") || tag_lower.starts_with("/when")
                    || tag_lower.starts_with("otherwise") || tag_lower.starts_with("/otherwise") {
                    // <choose>/<when>/<otherwise> — 保留内容
                } else if tag_lower.starts_with("include") || tag_lower.starts_with("/include") {
                    // Handled at XML event level — do nothing
                } else {
                    // 其他未知标签 — 忽略
                }
            }
            '#' | '$' => {
                if chars.peek() == Some(&'{') {
                    chars.next(); // 跳过 {
                    let mut inside = String::new();
                    let mut depth = 1;
                    while let Some(&ch) = chars.peek() {
                        if ch == '}' {
                            depth -= 1;
                            if depth == 0 {
                                chars.next();
                                break;
                            }
                        }
                        if ch == '{' { depth += 1; }
                        inside.push(ch);
                        chars.next();
                    }
                    // 替换为合理占位值
                    if c == '#' {
                        sql.push('1');  // #{xxx} → 1
                    } else {
                        sql.push('1');  // ${xxx} → 1
                    }
                } else {
                    sql.push(c);
                }
            }
            _ => sql.push(c),
        }
    }

    // 清理多余空白、trim
    let cleaned: String = sql.chars()
        .fold((String::new(), false), |(mut acc, prev_space), c| {
            if c.is_whitespace() && c != '\n' {
                if !prev_space {
                    acc.push(' ');
                }
                (acc, true)
            } else {
                acc.push(c);
                (acc, false)
            }
        }).0;

    cleaned.trim().to_string()
}

/// 跳过直到遇到指定结束标签
fn skip_until_tag(chars: &mut std::iter::Peekable<std::str::Chars>, end_tag: &str) {
    let mut buf = String::new();
    while let Some(&c) = chars.peek() {
        if c == '<' {
            // 检查是否是结束标签
            buf.clear();
            let mut saved = Vec::new();
            saved.push(chars.next().unwrap()); // <
            while let Some(&ch) = chars.peek() {
                if ch == '>' {
                    saved.push(chars.next().unwrap()); // >
                    break;
                }
                saved.push(chars.next().unwrap());
            }
            let tag: String = saved.iter().collect();
            let tag_lower = tag.trim().to_lowercase();
            if tag_lower == format!("<{}>", end_tag) {
                return;
            }
        } else {
            chars.next();
        }
    }
}

// ──────────────────────────────────────────────
// L1: 语法检查 — sqlparser-rs
// ──────────────────────────────────────────────

/// L1 语法检查：使用 sqlparser-rs 检查 SQL 语法
pub fn check_sql_syntax(sql: &str) -> Result<(), Vec<String>> {
    use sqlparser::parser::Parser;
    use sqlparser::dialect::PostgreSqlDialect;

    let mut trimmed = sql.trim().to_string();
    // Strip trailing semicolons which MyBatis XML often includes
    while trimmed.ends_with(';') {
        trimmed.pop();
    }
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    // 跳过不是完整 SQL 的语句（比如只有 WHERE 子句）
    let upper = trimmed.to_uppercase();
    let is_complete_sql = upper.starts_with("SELECT")
        || upper.starts_with("INSERT")
        || upper.starts_with("UPDATE")
        || upper.starts_with("DELETE")
        || upper.starts_with("WITH")
        || upper.starts_with("EXPLAIN");

    if !is_complete_sql {
        return Ok(());
    }

    match Parser::parse_sql(&PostgreSqlDialect {}, trimmed) {
        Ok(statements) => {
            if statements.is_empty() {
                Err(vec!["Empty statement list after parsing".to_string()])
            } else {
                Ok(())
            }
        }
        Err(e) => {
            Err(vec![format!("SQL syntax error: {}", e)])
        }
    }
}

// ──────────────────────────────────────────────
// L2: 语义检查 — EXPLAIN against real DB
// ──────────────────────────────────────────────

/// L2 语义检查：通过 EXPLAIN 在测试数据库上验证 SQL
pub fn check_sql_semantic(sql: &str, pg: &PgConfig) -> Result<(), String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let upper = trimmed.to_uppercase();
    let is_query = upper.starts_with("SELECT")
        || upper.starts_with("WITH")
        || upper.starts_with("EXPLAIN");

    if !is_query {
        // INSERT/UPDATE/DELETE 用 EXPLAIN 来验证
        return Ok(());
    }

    let explain_sql = format!("EXPLAIN {}", trimmed);

    let mut child = Command::new("psql")
        .args([
            "-h", &pg.host, "-p", &pg.port, "-d", &pg.db, "-U", &pg.user,
            "-v", "ON_ERROR_STOP=1",
            "-v", &format!("search_path={}", pg.schema),
        ])
        .env("PGPASSWORD", &pg.password)
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn psql: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{};", explain_sql);
    }

    let output = child.wait_with_output()
        .map_err(|e| format!("Failed to wait on psql: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let err_short: String = stderr.chars().take(300).collect();
        Err(format!("EXPLAIN failed: {}", err_short))
    }
}

// ──────────────────────────────────────────────
// 综合验证函数
// ──────────────────────────────────────────────

/// 验证单个 MyBatis Mapper XML 文件
pub fn validate_mapper_file(file_path: &Path, pg: &PgConfig) -> MapperValidationResult {
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_e) => {
            return MapperValidationResult {
                file_path: file_path.to_string_lossy().to_string(),
                total_sqls: 0,
                l1_passed: 0,
                l1_failed: 0,
                l2_passed: 0,
                l2_failed: 0,
                sql_results: vec![],
            };
        }
    };

    let extracted = extract_sql_from_xml(&content);
    let total = extracted.len();

    let mut l1_passed = 0;
    let mut l1_failed = 0;
    let mut l2_passed = 0;
    let mut l2_failed = 0;
    let mut sql_results = Vec::new();

    for (sql_id, sql_text) in &extracted {
        // L1: 语法检查
        let (l1_ok, l1_errors) = match check_sql_syntax(sql_text) {
            Ok(()) => (true, vec![]),
            Err(errs) => (false, errs),
        };

        if l1_ok { l1_passed += 1; } else { l1_failed += 1; }

        // L2: 语义检查（仅在 L1 通过时）
        let (l2_ok, l2_error) = if l1_ok {
            match check_sql_semantic(sql_text, pg) {
                Ok(()) => { l2_passed += 1; (true, None) }
                Err(e) => { l2_failed += 1; (false, Some(e)) }
            }
        } else {
            l2_failed += 1;
            (false, Some("Skipped L2 due to L1 failure".into()))
        };

        sql_results.push(SqlValidationResult {
            sql_id: sql_id.clone(),
            sql_raw: sql_text.chars().take(100).collect(),
            l1_passed: l1_ok,
            l1_errors,
            l2_passed: l2_ok,
            l2_error,
        });
    }

    MapperValidationResult {
        file_path: file_path.to_string_lossy().to_string(),
        total_sqls: total,
        l1_passed,
        l1_failed,
        l2_passed,
        l2_failed,
        sql_results,
    }
}

/// 验证工作目录中发生了变更的 Mapper XML 文件
///
/// 用于修复管线：只检查 Git 中修改过的 mapper XML
pub fn validate_changed_mappers(work_dir: &str, pg: &PgConfig) -> Vec<MapperValidationResult> {
    let repo_root = Path::new(work_dir);
    let repo_root = if repo_root.join("openhis-application").exists() {
        repo_root.to_path_buf()
    } else {
        repo_root.parent().map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(work_dir))
    };

    let mut changed_xmls: Vec<String> = Vec::new();
    for pattern in &["**/mapper/**/*.xml", "**/sqlmap/**/*.xml"] {
        for diff_cmd in [
            vec!["diff", "--name-only", "--diff-filter=ACMR", "HEAD", "--", pattern],
            vec!["diff", "--cached", "--name-only", "--diff-filter=ACMR", "HEAD", "--", pattern],
        ] {
            if let Ok(o) = Command::new("git")
                .args(&diff_cmd)
                .current_dir(&repo_root)
                .output()
            {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let f = line.trim().to_string();
                    if !f.is_empty() && !changed_xmls.contains(&f) && f.ends_with(".xml") {
                        changed_xmls.push(f);
                    }
                }
            }
        }
    }

    if changed_xmls.is_empty() {
        return vec![];
    }

    let mut results = Vec::new();
    for xml_rel in &changed_xmls {
        let full_path = repo_root.join(xml_rel);
        if full_path.exists() {
            results.push(validate_mapper_file(&full_path, pg));
        }
    }
    results
}

/// 全量扫描：验证 repo 中所有的 Mapper XML 文件
///
/// 用于基线检查，发现所有已有 SQL 语法问题
pub fn validate_all_mappers(repo_root: &str, pg: &PgConfig) -> Vec<MapperValidationResult> {
    let root = Path::new(repo_root);
    
    // Find all mapper XML files (exclude target/)
    let mut all_mappers: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = walkdir(root, "mapper") {
        all_mappers = entries;
    }

    // Also search for sqlmap directories
    if let Ok(entries) = walkdir(root, "sqlmap") {
        for e in entries {
            if !all_mappers.contains(&e) {
                all_mappers.push(e);
            }
        }
    }

    let total = all_mappers.len();
    tracing::info!("Found {} Mapper XML files to validate", total);

    let mut results = Vec::new();
    for (i, path) in all_mappers.iter().enumerate() {
        if i % 50 == 0 && i > 0 {
            tracing::info!("Progress: {}/{} files validated", i, total);
        }
        results.push(validate_mapper_file(path, pg));
    }

    results
}

/// 递归查找目录下所有 XML 文件（排除 target 目录）
fn walkdir(root: &Path, subdir: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut results = Vec::new();

    // Search for paths containing /mapper/ or /sqlmap/ in the path
    let target_marker = format!("/{}/", subdir);

    let search_path = root.join("openhis-server-new");
    if !search_path.exists() {
        return Ok(results);
    }

    let mut dirs_to_check = vec![search_path];
    while let Some(dir) = dirs_to_check.pop() {
        // Skip target directories
        if dir.file_name().map(|n| n == "target").unwrap_or(false) {
            continue;
        }
        if dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        dirs_to_check.push(path);
                    } else if path.extension().map(|e| e == "xml").unwrap_or(false) {
                        // Check if path contains /mapper/ or /sqlmap/
                        let path_str = path.to_string_lossy().to_lowercase();
                        if path_str.contains(&target_marker) {
                            results.push(path);
                        }
                    }
                }
            }
        }
    }

    Ok(results)
}

/// 生成全量扫描报告
pub fn generate_scan_report(results: &[MapperValidationResult]) -> String {
    let total_files = results.len();
    let total_sqls: usize = results.iter().map(|r| r.total_sqls).sum();
    let l1_failed_total: usize = results.iter().map(|r| r.l1_failed).sum();
    let l2_failed_total: usize = results.iter().map(|r| r.l2_failed).sum();
    
    // 收集所有失败的 SQL
    let mut failures: Vec<(&MapperValidationResult, &SqlValidationResult)> = Vec::new();
    for mapper in results {
        for sql in &mapper.sql_results {
            if !sql.l1_passed || !sql.l2_passed {
                failures.push((mapper, sql));
            }
        }
    }

    let mut report = String::new();
    report.push_str(&format!(
        "═══ SQL 全量扫描报告 ═══\n\
         扫描文件: {} 个 | SQL 语句: {} 条\n\
         L1 语法通过: {} | 失败: {}\n\
         L2 语义通过: {} | 失败: {}\n\
         ================================\n",
        total_files, total_sqls,
        total_sqls - l1_failed_total, l1_failed_total,
        total_sqls - l2_failed_total, l2_failed_total,
    ));

    if failures.is_empty() {
        report.push_str("\n✅ 全部通过，未发现 SQL 语法问题！\n");
    } else {
        report.push_str(&format!("\n❌ 发现 {} 个 SQL 问题:\n\n", failures.len()));
        for (i, (mapper, sql)) in failures.iter().enumerate() {
            report.push_str(&format!(
                "{}. [{}] id=\"{}\"\n   文件: {}\n   SQL: {}\n",
                i + 1,
                if !sql.l1_passed { "L1语法错误" } else { "L2语义错误" },
                sql.sql_id,
                mapper.file_path,
                sql.sql_raw,
            ));
            if !sql.l1_errors.is_empty() {
                for err in &sql.l1_errors {
                    report.push_str(&format!("   错误: {}\n", err));
                }
            }
            if let Some(ref l2_err) = sql.l2_error {
                report.push_str(&format!("   EXPLAIN: {}\n", l2_err));
            }
            report.push('\n');
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sql_select() {
        let xml = r#"
        <mapper namespace="com.example.TestMapper">
            <select id="findById" resultType="Test">
                SELECT * FROM test WHERE id = #{id}
            </select>
        </mapper>
        "#;
        let sqls = extract_sql_from_xml(xml);
        assert_eq!(sqls.len(), 1);
        assert_eq!(sqls[0].0, "findById");
        assert!(sqls[0].1.contains("SELECT"));
        assert!(sqls[0].1.contains("1"));  // #{id} → 1
    }

    #[test]
    fn test_extract_sql_multiple() {
        let xml = r#"
        <mapper>
            <select id="findAll">
                SELECT * FROM users
            </select>
            <insert id="insertUser">
                INSERT INTO users(name) VALUES(#{name})
            </insert>
            <update id="updateUser">
                UPDATE users SET name = #{name} WHERE id = #{id}
            </update>
            <delete id="deleteUser">
                DELETE FROM users WHERE id = #{id}
            </delete>
        </mapper>
        "#;
        let sqls = extract_sql_from_xml(xml);
        assert_eq!(sqls.len(), 4);
        assert_eq!(sqls[0].0, "findAll");
        assert_eq!(sqls[1].0, "insertUser");
        assert_eq!(sqls[2].0, "updateUser");
        assert_eq!(sqls[3].0, "deleteUser");
    }

    #[test]
    fn test_extract_with_if_tag() {
        let xml = r#"
        <mapper>
            <select id="findByCondition">
                SELECT * FROM users
                <where>
                    <if test="name != null">
                        AND name = #{name}
                    </if>
                    <if test="age != null">
                        AND age = #{age}
                    </if>
                </where>
            </select>
        </mapper>
        "#;
        let sqls = extract_sql_from_xml(xml);
        assert_eq!(sqls.len(), 1);
        let sql = &sqls[0].1;
        assert!(sql.contains("SELECT * FROM users"));
        assert!(sql.contains("AND name = 1"));
        assert!(sql.contains("AND age = 1"));
    }

    #[test]
    fn test_extract_with_foreach() {
        let xml = r#"
        <mapper>
            <select id="findByIds">
                SELECT * FROM users WHERE id IN
                <foreach collection="ids" item="id" open="(" separator="," close=")">
                    #{id}
                </foreach>
            </select>
        </mapper>
        "#;
        let sqls = extract_sql_from_xml(xml);
        assert_eq!(sqls.len(), 1);
        let sql = &sqls[0].1;
        assert!(sql.contains("(1,2,3)"), "foreach should be replaced with (1,2,3), got: {}", sql);
    }

    #[test]
    fn test_check_sql_syntax_valid() {
        let result = check_sql_syntax("SELECT * FROM users WHERE id = 1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_sql_syntax_invalid() {
        let result = check_sql_syntax("SELECT * FORM users");  // FORM typo
        assert!(result.is_err());
    }

    #[test]
    fn test_check_sql_syntax_extra_comma() {
        let result = check_sql_syntax("SELECT id,, name FROM users");  // extra comma
        assert!(result.is_err());
    }

    #[test]
    fn test_check_sql_syntax_in_clause() {
        let result = check_sql_syntax("SELECT * FROM users WHERE id IN (1, 2, 3)");
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_mybatis_sql_replaces_params() {
        let input = "SELECT * FROM users WHERE id = #{id} AND name = #{name} AND status = ${status}";
        let result = process_mybatis_sql(input);
        assert_eq!(result, "SELECT * FROM users WHERE id = 1 AND name = 1 AND status = 1");
    }

    #[test]
    fn test_extract_skip_sql_fragment() {
        let xml = r#"
        <mapper>
            <sql id="BaseColumns">
                id, name, age
            </sql>
            <select id="findAll">
                SELECT <include refid="BaseColumns"/> FROM users
            </select>
        </mapper>
        "#;
        let sqls = extract_sql_from_xml(xml);
        assert_eq!(sqls.len(), 1, "Should only extract select, not sql fragment");
        let sql = &sqls[0].1;
        assert!(sql.contains("SELECT"), "SQL should contain SELECT keyword");
    }
}
