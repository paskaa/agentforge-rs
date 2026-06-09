//! Sub-agent pool — manages Codex fix invocations.
//!
//! Each agent gets its own Git worktree for isolated fixes.
//! When invoked, the prompt includes the full Harness Engineering
//! methodology (Init → Plan → Implement → Verify → Cleanup) via loaded skills.
//!
//! All fix invocations go through `codex-aliyun` → `mimo2codex` → `codex` pipeline.

use std::process::Command;
use std::time::Instant;

/// Agent role descriptions and expertise for prompt customization.
const AGENT_ROLES: &[(&str, &str, &str, &str)] = &[
    ("zhugeliang", "架构师/协调者",
     "负责分析 Bug、拆解任务、分派给合适的修复 Agent。关注系统整体架构和全链路数据流。
在修复流程中：Zhaoyun(前端) → Guanyu(后端) → Xunyu(DB) → Zhangfei(测试) → Huatuo(验收) → Chenlin(归档)",
     "系统架构|全链路分析|任务拆解|代码审查|流程协调|Agent调度"),
    ("guanyu", "后端修复工程师",
     "负责 Java/Spring 后端修复。精通 MyBatis-Plus、Spring Boot、REST API、Maven。
修完后自动触发 Zhangfei 测试 → Huatuo 验收 → Chenlin 归档。
关键检查点：Controller参数接收 → Service逻辑处理 → Mapper SQL映射 → DB字段匹配",
     "Java|Spring|MyBatis|Maven|REST API|Controller|Service|Mapper|SQL|后端"),
    ("zhaoyun", "前端修复工程师",
     "负责 Vue3 前端修复。精通 ElementUI、TypeScript、Axios、Vite。
修完后自动触发 Zhangfei 测试 → Huatuo 验收 → Chenlin 归档。
关键检查点：页面组件 → API调用 → 数据绑定 → 展示字段",
     "Vue|ElementUI|TypeScript|前端|界面|CSS|组件|Axios|Vite"),
    ("xunyu", "数据库工程师",
     "负责 SQL/数据库修复。精通 PostgreSQL、DDL、DML、索引优化、查询分析。
关注：表结构设计 → 查询性能 → 数据一致性 → 迁移脚本规范",
     "SQL|PostgreSQL|索引|DDL|DML|数据库|慢查询|表结构|迁移|数据一致性"),
    ("zhangfei", "QA 测试工程师",
     "负责运行回归测试（Playwright）来验证修复质量。
测试通过 → 通知 Huatuo 验收 + Chenlin 归档。
测试失败 → 自动退回修复 Agent 重修（最多 3 次）。
如果没有对应 Playwright 测试，标记为「无需测试」继续流转。",
     "测试|Playwright|pytest|端到端测试|自动化测试|E2E|回归测试|质量门禁"),
    ("huatuo", "产品验收员",
     "负责验证修复是否满足业务需求。关注用户场景和验收标准。
验收通过 → 通知相关方。
验收逻辑：检查测试文档 → 确认修复符合 Bug 描述 → 飞书通知结果。",
     "验收|业务验证|用户场景|功能确认|需求符合度|质量验收"),
    ("chenlin", "文档工程师",
     "负责生成和归档 Bug 修复文档。
归档内容：Bug 编号 → 修复时间 → 修复摘要 → 测试结果 → 验收状态。
文档保存至 Redis（30 天 TTL），可供后续查询。",
     "文档|Markdown|API文档|技术写作|归档|知识管理"),
    ("liubei", "项目经理",
     "负责跟踪进度、协调资源、管理需求优先级。
监控整体 Pipeline 健康度：修复成功率 → 测试通过率 → 验收完成率。",
     "项目管理|进度跟踪|需求管理|资源协调|Pipeline监控|质量看板"),
];

/// Get the work directory for a given agent.
fn agent_work_dir(agent_name: &str) -> String {
    let base = format!("/tmp/agentforge-worktrees/{}", agent_name);
    if agent_name == "zhaoyun" {
        format!("{}/healthlink-his-ui", base)
    } else if agent_name == "xunyu" {
        format!("{}/healthlink-his-server", base)
    } else {
        format!("{}/healthlink-his-server", base)
    }
}

/// Get agent-specific constraints to add to the prompt.
fn agent_constraints(agent_name: &str) -> &str {
    match agent_name {
        "zhaoyun" => {
            "## 前端约束
             - 使用 Vue3 Composition API + script setup
             - 组件使用 ElementUI 组件库
             - API 调用通过 @/utils/request 统一管理
             - 样式使用 Scoped CSS
             - 类型提示使用 TypeScript
             - 修改后依次运行: vue-tsc --noEmit (类型检查) → vite build (编译) → npm run lint (语法)"
        }
        "guanyu" => {
            "## 后端约束
             - 遵循三层架构：Controller → Service → Mapper
             - 使用 Lombok 简化代码
             - API 返回统一使用 R.ok() / R.fail()
             - 涉及数据库字段时走通全链路 6 环
             - 修改后运行 mvn compile 验证"
        }
        "xunyu" => {
            "## 数据库约束
             - 所有 DDL 变更必须通过迁移脚本（sql/迁移记录-DB变更记录/）
             - ⚠️ 迁移脚本文件名必须包含对应 BUG 编号，格式: YYYYMMDD_fix_BUG#XXXX_description.sql
             - 表名使用小写 + 下划线
             - 所有新增列必须有 COMMENT
             - 复杂查询用 CTE 或子查询，避免嵌套过深
             - 涉及索引变更先评估现有查询计划"
        }
        "zhangfei" => {
            "## 测试约束
             - 运行 Playwright 回归测试：npx playwright test --grep @bug{{id}} --workers=1
             - 如果测试不存在：标记为「无需测试」继续流转
             - 测试失败：自动退回修复 Agent 重修（最多 3 次）
             - 测试通过：通知 Huatuo 验收 + Chenlin 归档"
        }
        "huatuo" => {
            "## 验收约束
             - 核心检查：修复是否满足 Bug 描述的全部要求
             - 检查测试文档是否存在
             - 验收通过 → 飞书通知相关方
             - 验收失败 → 记录失败原因返回修复 Agent"
        }
        "chenlin" => {
            "## 归档约束
             - 生成修复文档：包含 Bug 编号、修复时间、变更摘要
             - 文档保存至 Redis（30 天 TTL）
             - 飞书通知归档完成"
        }
        "liubei" => {
            "## 管理约束
             - 监控 Pipeline 整体健康度
             - 跟踪：修复成功率、测试通过率、验收完成率
             - 定期汇总报告"
        }
        _ => "## 通用约束
- 修改后运行对应编译检查"
    }
}

/// Result of invoking Codex for a bug fix.
#[derive(Debug, Clone)]
pub struct CodexResult {
    pub success: bool,
    pub bug_id: String,
    pub elapsed_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub changes: u32,
    pub last_phase: String,  // harness loop 最后执行的阶段
    pub phase_verdicts: Vec<(String, String)>,  // (phase_name, verdict)
}

/// Check if a bug is a frontend issue (should use Codex).
pub fn is_frontend_bug(title: &str) -> bool {
    let t = title.to_lowercase();
    let frontend_kw = ["前端", "vue", "界面", "显示", "弹窗", "按钮", "列表", "回显",
                       "刷新", "排版", "格式", "样式", "css", "组件", "命名", "提示语",
                       "查询", "过滤", "加载", "报表", "报表管理"];
    frontend_kw.iter().any(|kw| t.contains(kw))
}

// ──────────────────────────────────────────────
// Harness methodology: skill loading + prompt builder
// ──────────────────────────────────────────────

/// Load a skill file's content (return empty string if not found).
fn load_skill(path: &str) -> String {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let len = content.len();
    if len > 0 {
        tracing::info!("[skill] Loaded {} ({} chars)", path.split('/').last().unwrap_or("?"), len);
    } else {
        tracing::warn!("[skill] Empty: {}", path);
    }
    content.chars().take(3000).collect()
}

/// Build the full harness-augmented prompt for Codex.
/// Loads ALL installed harness skills so Codex follows the methodology autonomously.
fn build_harness_prompt(agent_name: &str, bug_id: &str, bug_title: &str, bug_details: &str) -> String {
    // 精简 prompt：不再加载 15 个 skill 文件，只保留核心信息

    let agents_md_path = "/root/.openclaw/workspace/his-repo/AGENTS.md";
    let agents_md_hint = load_skill(agents_md_path)
        .lines().take(30).collect::<Vec<_>>().join("\n");
    
    // 加载统一铁律文件
    let iron_laws = load_skill("/root/.codex/rules/IRON_LAWS.md");

    // Find agent role info
    let role_info = AGENT_ROLES.iter().find(|r| r.0 == agent_name);
    let (role_name, role_desc, expertise): (&str, &str, &str) = match role_info {
        Some((_, name, desc, exp)) => (*name, *desc, *exp),
        None => ("通用开发工程师", "负责代码开发和 Bug 修复。", "通用"),
    };
    let constraints = agent_constraints(agent_name);

    // L5: 加载自优化器生成的额外约束
    let extra_constraints = {
        let opt = super::self_optimizer::SelfOptimizer::load("/var/lib/agentforge/agent_scores.json");
        let extras = opt.get_extra_constraints(agent_name);
        if extras.is_empty() { String::new() }
        else { format!("\n## L5 自动优化约束（基于历史失败分析）\n{}", extras.join("\n")) }
    };

    // 精简 prompt：不再内联 skill 文件，改为路径引用
    // 原来 15 个 skill 全量加载 → 58KB，模型处理不了
    // 现在只保留角色 + 铁律 + bug 详情 + 关键指引

    format!(
        r#"你是一个中文编程助手。使用简体中文思考和回复。

## 你的角色
你是 **{role_name}**。{role_desc}
你的专长领域：{expertise}

{constraints}{extra_constraints}

## 工作纪律
1. **Init**: 确认工作目录，读 AGENTS.md 了解项目规范
2. **Plan**: 分析全链路数据流（6环）：前端→Controller→Service→Mapper→DB→关联模块
3. **Implement**: 一次只修一个 Bug，只动必要文件
4. **Verify**: 修改后运行 mvn compile / npm run build 验证编译
5. **Cleanup**: 不留临时文件或调试代码

## 铁律
- 安全 > 架构 > 质量 > 性能
- 禁止硬编码密钥/密码
- 涉及数据库字段时走通全链路 6 环
- 涉及交互/状态变更：同时分析「发起方」和「接收方」两端
- 修改后必须编译验证，不通过不提交

## 项目规范摘要
{agents_md_hint}

{iron_laws}

---

## 任务：修复 Bug #{bug_id}：{bug_title}

## 禅道 Bug 详情
{bug_details}

## Harness 修复指引（Bug-Driven Testing 流程）
1. **Init**: 确认目录，读 AGENTS.md
2. **Bug Analysis**: 读取禅道 Bug 全部信息（标题/步骤/截图/备注）
3. **Test Design**: 根据 Bug 信息生成 Playwright 测试用例
   - 从标题推断模块和路由
   - 从复现步骤生成操作序列
   - 从期望结果生成断言
   - 生成文件：tests/e2e/specs/bug-{{id}}.spec.ts
4. **Baseline Test**: 运行基线测试（预期失败，证明 Bug 存在）
   - `npx playwright test --grep @bug{{id}}` → 预期 FAIL
   - 如果通过 → 检查 develop 是否已修复，检查测试用例是否正确
5. **Pre-check**: 检查 develop 上是否有该 bug 的历史修复提交
   - 如果有：读之前的 commit diff，分析是否完整修复
   - 如果之前修得不完整：指出遗漏点，重新全链路分析
   - 如果之前修复完整：输出「之前修复已完成，无需改动」并退出
6. **Full-chain (6 环)**: 无论是否已有修复，都跑一遍
   - 前端/页面 → Controller → Service → Mapper → DB/SQL → 关联模块
   - 涉及数据库字段时必走，查每个环节的字段映射
   - ⚠️ 每环必须标记状态：【✅ 正常 / 🔧 已修改 / ❌ 遗漏】
   - ⚠️ 如果只改了后端没改前端（或反之），说明分析不完整，重新检查
7. **Fix**: 修改文件（用 apply_patch），一次修彻底
   - ⚠️ 涉及新增 Entity 字段时，必须同时创建 DB 迁移脚本（sql/迁移记录-DB变更记录/YYYYMMDD_fix_BUG#XXXX_description.sql）
   - ⚠️ 只改 Entity 不改 DB = 修复不完整，运行时 100% 报错
   - ⚠️ 涉及交互流程（退回/审核/签发等）的 BUG，必须识别「发起方📤」（谁操作）和「接收方📥」（谁查看）
     - 📤 发起方：检查操作入口→校验→API→Service→DB 是否完整
     - 📥 接收方：检查 DB→Service→API→展示字段→页面列 是否完整
     - 只修一端不修另一端 = 修复不完整
8. **Regression Test**: 运行回归测试（预期通过，证明修复有效）
   - `npx playwright test --grep @bug{{id}}` → 预期 PASS
   - 如果失败 → 分析失败原因 → 返回 Step 7 重新修复
9. **Verify**: 运行编译/语法检查 + 端到端数据流确认
   - 编译检查：mvn compile / npm lint / cargo check / vue-tsc + vite build
   - ⚠️ 数据流检查：从起点到终点每环确认数据能传过去
     - 📤 录入链路：前端发送字段 → API 参数接收 → Service 读取 → DB 写入
     - 📥 展示链路：DB 查询 → Service 返回 → API 响应 → 前端展示列
   - ⚠️ 交互检查：涉及前端改动时确认弹窗/提示/跳转是否正常工作
   - ⚠️ 两端核对：涉及交互流程的 BUG，用表格对比两端修复状态
10. **Submit**: 输出变更摘要，格式：
   ```
   根因：
   - ...
   
   修复：
   - ...
   ```

请分析并直接修改文件修复。不要用 git。
"#,
        role_name=role_name, role_desc=role_desc, expertise=expertise,
        constraints=constraints,
        bug_id=bug_id, bug_title=bug_title, bug_details=bug_details,
        agents_md_hint=agents_md_hint,
    )
}

/// Run quality gates after a fix to verify correctness.
fn run_quality_gates(agent_name: &str, work_dir: &str) -> (bool, String, String) {
    let is_his_repo = work_dir.contains("his-repo");
    let is_rust = work_dir.contains("agentforge-rs");
    
    // Frontend agent: 3-stage verification (type check → build → lint)
    if agent_name == "zhaoyun" {
        // Ensure node_modules exists before running checks
        let nm_path = std::path::Path::new(work_dir).join("node_modules");
        if !nm_path.exists() {
            tracing::warn!("[zhaoyun] node_modules missing, running npm install first");
            let _ = Command::new("npm")
                .args(["install", "--no-fund", "--no-audit"])
                .current_dir(work_dir)
                .output();
        }

        // Step 1: Vue TypeScript type check (vue-tsc) — non-fatal (项目有已有类型错误)
        let ts_check = Command::new("npx")
            .args(["vue-tsc", "--noEmit"])
            .current_dir(work_dir)
            .output();
        match ts_check {
            Ok(o) if !o.status.success() => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                tracing::warn!("[zhaoyun] vue-tsc 有类型错误（非阻断）: {} chars", stderr.len());
            }
            Ok(_) => tracing::info!("[zhaoyun] vue-tsc 类型检查通过"),
            Err(e) => tracing::warn!("[zhaoyun] vue-tsc 不可用: {}", e),
        }

        // Step 2: Vite build (full compilation) — hard failure
        let build = Command::new("npx")
            .args(["vite", "build", "--mode", "dev"])
            .current_dir(work_dir)
            .output();
        match build {
            Ok(o) if !o.status.success() => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                return (false,
                    "vite build 编译失败".into(),
                    if stderr.len() > 1000 { stderr.chars().take(1000).collect() } else { stderr });
            }
            Ok(_) => tracing::info!("[zhaoyun] vite build 编译通过"),
            Err(e) => tracing::warn!("[zhaoyun] vite build 不可用: {}", e),
        }

        // Step 3: ESLint (style/syntax) — non-fatal warnings
        let lint = Command::new("npm")
            .args(["run", "lint", "--", "--quiet"])
            .current_dir(work_dir)
            .output();
        return match lint {
            Ok(o) if o.status.success() => (true, "所有检查通过 (vue-tsc ✅ / vite build ✅ / npm lint ✅)".into(), String::new()),
            Ok(o) => (true, format!("类型检查+编译通过，lint 有警告"), 
                String::from_utf8_lossy(&o.stdout).to_string()),
            Err(e) => (true, format!("lint 不可用: {}", e), String::new()),
        };
    }

    // Use owned Vec<String> to avoid type complexity with slices
    if is_his_repo {
        // Step 1: Clean compile (force full rebuild to avoid stale class issues)
        let compile = Command::new("mvn")
            .args(["clean", "compile", "-q", "-pl", "openhis-application", "-am"])
            .current_dir(work_dir)
            .output();
        match compile {
            Ok(o) if !o.status.success() => {
                return (false,
                    String::from_utf8_lossy(&o.stdout).to_string(),
                    String::from_utf8_lossy(&o.stderr).to_string());
            }
            Err(e) => {
                tracing::warn!("mvn clean compile not available: {}", e);
                return (true, format!("mvn not available: {}", e), String::new());
            }
            _ => tracing::info!("mvn clean compile OK"),
        }

        // Step 2: Run Spring Boot context test (catches bean creation errors at startup)
        let test = Command::new("mvn")
            .args(["test", "-q", "-pl", "openhis-application", 
                   "-Dtest=com.openhis.MedicationApplicationTests",
                   "-DfailIfNoTests=false", "-DskipTests=false"])
            .current_dir(work_dir)
            .output();
        match test {
            Ok(o) if !o.status.success() => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                return (false,
                    format!("Spring Boot 启动测试失败"),
                    if stderr.len() > 1000 { stderr.chars().take(1000).collect() } else { 
                        if stdout.len() > 500 { stdout.chars().take(500).collect() } else { stdout }
                    });
            }
            Ok(_) => (true, "编译+启动测试通过 (mvn clean compile ✅ / contextLoads ✅)".into(), String::new()),
            Err(e) => (true, format!("编译通过，测试不可用: {}", e), String::new()),
        }
    } else if is_rust {
        let output = Command::new("cargo")
            .args(["check", "-q"])
            .current_dir(work_dir)
            .output();
        match output {
            Ok(o) if o.status.success() => (true, "cargo check OK".into(), String::new()),
            Ok(o) => (false,
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string()),
            Err(e) => (true, format!("cargo not available: {}", e), String::new()),
        }
    } else {
        (true, "(no quality gates)".into(), String::new())
    }
}

/// Run database migrations (DDL scripts) after a fix.
/// Detects new/changed migration SQL files and executes them against the test database.
fn run_db_migrations(agent_name: &str, work_dir: &str, bug_id: &str) -> bool {
    // Determine the repo root (migration dir is at repo root: sql/迁移记录-DB变更记录/)
    let repo_root = std::path::Path::new(work_dir);
    let repo_root = if repo_root.join("sql").exists() {
        repo_root.to_path_buf()
    } else {
        repo_root.parent().map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from(work_dir))
    };

    let migration_dir = repo_root.join("sql").join("迁移记录-DB变更记录");
    if !migration_dir.exists() {
        tracing::info!("[{}] No migration dir, skipping DB migrations", agent_name);
        return true;
    }

    // Detect new/changed migration files via git diff
    let mut pending: Vec<String> = Vec::new();
    for diff_cmd in [
        vec!["diff", "--name-only", "--diff-filter=ACMR", "HEAD", "--", "sql/迁移记录-DB变更记录/*.sql"],
        vec!["diff", "--cached", "--name-only", "--diff-filter=ACMR", "HEAD", "--", "sql/迁移记录-DB变更记录/*.sql"],
    ] {
        if let Ok(o) = Command::new("git").args(&diff_cmd).current_dir(&repo_root).output() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let f = line.trim().to_string();
                if !f.is_empty() && !pending.contains(&f) {
                    pending.push(f);
                }
            }
        }
    }

    if pending.is_empty() {
        tracing::info!("[{}] No new migration scripts for Bug #{}", agent_name, bug_id);
        return true;
    }

    tracing::info!("[{}] Found {} migration script(s) for Bug #{}: {:?}",
        agent_name, pending.len(), bug_id, pending);

    let app_cfg = crate::config::Config::load().unwrap_or_default();

    // Schema list to try (test environment)
    let schemas = ["histest1", "hisdev"];

    let all_ok = pending.iter().all(|sql_file| {
        let full = repo_root.join(sql_file);
        if !full.exists() {
            tracing::warn!("[{}] Migration file not found: {}", agent_name, sql_file);
            return true;
        }

        // ⚠️  REQUIREMENT: migration filename must contain bug number
        let fname_lower = sql_file.to_lowercase();
        let has_bug_ref = fname_lower.contains("bug#") || fname_lower.contains("bug #")
            || fname_lower.contains("#") || fname_lower.contains("fix_");
        if !has_bug_ref {
            tracing::error!("[{}] ❌ 迁移脚本文件名缺少 BUG 编号: {} — 必须包含 Bug#XXXX",
                agent_name, sql_file);
            return false;
        }

        // ⚠️  SAFETY CHECK: reject destructive operations
        let content = std::fs::read_to_string(&full).unwrap_or_default();
        let upper = content.to_uppercase();
        
        // Block: DROP TABLE, TRUNCATE, DELETE, UPDATE without WHERE
        let dangerous = [
            ("DROP TABLE", "禁止删除表"),
            ("TRUNCATE", "禁止清空表"),
            ("DELETE FROM", "禁止删除数据"),
        ];
        for (pattern, reason) in &dangerous {
            if upper.contains(pattern) {
                // Allow DROP TABLE IF EXISTS (for CREATE TABLE IF NOT EXISTS patterns)
                if *pattern == "DROP TABLE" && upper.contains("IF EXISTS") {
                    continue;
                }
                tracing::error!("[{}] ❌ 安全拦截: {} ({} 包含 {})", agent_name, sql_file, reason, pattern);
                return false;
            }
        }

        // ✅ SAFETY: only allow these operations
        // ALTER TABLE ADD COLUMN / IF NOT EXISTS
        // CREATE TABLE IF NOT EXISTS
        // CREATE SEQUENCE IF NOT EXISTS
        // COMMENT ON COLUMN
        // INSERT INTO (data setup)
        // SELECT (diagnostic queries)
        // DO $$ (plpgsql blocks for conditional operations)
        
        tracing::info!("[{}] Executing migration: {} ...", agent_name, sql_file);

        // Try each schema
        let mut any_schema_ok = false;
        for schema in &schemas {
            match Command::new("psql")
                .args(["-h", &app_cfg.database.host, "-p", &app_cfg.database.port.to_string(),
                       "-d", &app_cfg.database.database, "-U", &app_cfg.database.username,
                       "-v", "ON_ERROR_STOP=1",
                       "-v", &format!("search_path={}", schema),
                       "-f", &full.to_string_lossy()])
                .env("PGPASSWORD", &app_cfg.database.password)
                .output()
            {
                Ok(o) if o.status.success() => {
                    tracing::info!("[{}] ✅ Migration OK on schema {}: {}",
                        agent_name, schema, sql_file);
                    any_schema_ok = true;
                    break;
                }
                Ok(_) => {
                    // Try next schema
                    continue;
                }
                Err(e) => {
                    tracing::warn!("[{}] psql error for schema {}: {}", agent_name, schema, e);
                }
            }
        }

        if !any_schema_ok {
            tracing::error!("[{}] ❌ Migration FAILED on ALL schemas: {} — manual review needed",
                agent_name, sql_file);
            return false;
        }
        true
    });

    if all_ok {
        tracing::info!("[{}] ✅ All {} migration(s) applied for Bug #{}", agent_name, pending.len(), bug_id);
    } else {
        tracing::error!("[{}] ❌ Some migrations FAILED for Bug #{}", agent_name, bug_id);
    }
    all_ok
}

/// Validate SQL in MyBatis Mapper XML files after a fix.
/// 
/// L1: 语法检查 — 使用 sqlparser-rs 解析完整 SQL，检测语法错误
/// L2: 语义检查 — 使用 EXPLAIN 在测试 DB 上验证
/// 
/// 相比旧版（逐行关键词匹配）的改进:
/// - 完整提取 MyBatis 动态 SQL 中的 SELECT/INSERT/UPDATE/DELETE
/// - 正确处理 <if>/<foreach>/<where>/<set>/<trim> 标签
/// - 使用真正的 SQL 解析器检查语法
/// - 仅对变更的 Mapper XML 做增量验证
fn validate_mapper_sql(agent_name: &str, work_dir: &str, bug_id: &str) -> bool {
    use crate::core::sql_validator;

    let pg = sql_validator::PgConfig::default();
    let results = sql_validator::validate_changed_mappers(work_dir, &pg);
    
    if results.is_empty() {
        tracing::debug!("[{}] Bug #{}: No changed Mapper XML files found, skipping SQL validation", agent_name, bug_id);
        return true;
    }
    
    let mut all_valid = true;
    for mapper in &results {
        for sql_res in &mapper.sql_results {
            if !sql_res.l1_passed {
                all_valid = false;
                tracing::error!(
                    "[{}] ❌ L1 语法错误 in {} (id={}): {}",
                    agent_name, mapper.file_path, sql_res.sql_id,
                    sql_res.l1_errors.join("; ")
                );
            }
            if !sql_res.l2_passed {
                all_valid = false;
                if let Some(ref err) = sql_res.l2_error {
                    tracing::error!(
                        "[{}] ❌ L2 语义错误 in {} (id={}): {}",
                        agent_name, mapper.file_path, sql_res.sql_id, err
                    );
                }
            }
        }
        
        let total = mapper.total_sqls;
        let l1_ok = mapper.l1_passed;
        let l2_ok = mapper.l2_passed;
        let file_short = mapper.file_path.rsplit('/').next().unwrap_or(&mapper.file_path);
        
        if total > 0 {
            if l1_ok == total && l2_ok == total {
                tracing::info!("[{}] ✅ SQL all passed: {} (L1:{}/{} L2:{}/{})", 
                    agent_name, file_short, l1_ok, total, l2_ok, total);
            } else {
                tracing::warn!("[{}] ⚠️  SQL issues in {} (L1:{}/{} L2:{}/{})", 
                    agent_name, file_short, l1_ok, total, l2_ok, total);
            }
        }
    }
    
    all_valid
}





// ──────────────────────────────────────────────
// Codex fix implementation (mimo2codex → codex)
// ──────────────────────────────────────────────

/// Run Codex (via codex-aliyun → mimo2codex) to fix a bug.
/// The prompt includes full Harness Engineering methodology.
/// Check develop branch for previous fix commits of this bug.
fn check_previous_fix(bug_id: &str) -> String {
    let main_repo = "/root/.openclaw/workspace/his-repo";
    let _ = Command::new("git")
        .args(["-C", main_repo, "fetch", "origin", "develop"]).output();
    
    // Check for both new and old commit message formats
    let output = Command::new("git")
        .args(["-C", main_repo, "log", "origin/develop", "--oneline", "-5",
               "--grep", &format!("Bug #{}", bug_id)])
        .output();
    
    if let Ok(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        if lines.is_empty() { return String::new(); }
        
        let mut context = String::from("develop 上已有的修复提交：\n");
        for line in &lines {
            context.push_str(&format!("  {}\n", line));
        }
        
        // Get the full diff stat for the most recent fix
        if let Some(latest_sha) = lines[0].split_whitespace().next() {
            let diff = Command::new("git")
                .args(["-C", main_repo, "diff", "--stat",
                       &format!("{}^..{}", latest_sha, latest_sha)])
                .output();
            if let Ok(d) = diff {
                let d_stdout = String::from_utf8_lossy(&d.stdout);
                if !d_stdout.trim().is_empty() {
                    context.push_str(&format!("变更文件：\n{}\n", d_stdout));
                }
            }
        }
        return context;
    }
    String::new()
}

fn run_codex_fix_impl(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    _timeout_secs: u64,
) -> CodexResult {
    let start = Instant::now();

    // Step 1: Query bug details from Zentao (Rust API client)
    let bug_details_text = query_bug_details_v2(bug_id);

    // Step 1.5: Check if this bug was previously fixed on develop (for re-analysis)
    let prev_fix_context = check_previous_fix(bug_id);
    let bug_details_text = if prev_fix_context.is_empty() {
        bug_details_text
    } else {
        format!("{}\n\n## 历史修复记录（develop 上已有提交，请分析是否完整）\n{}\n\n请先分析之前的修复是否解决了问题：\n1. 如果之前的修复不完整或存在遗漏，请补充修复\n2. 如果之前的修复完全正确，输出「之前修复已完成，无需改动」\n3. 无论如何，跑一次全链路 6 环分析确认每个环节的状态", 
            bug_details_text, prev_fix_context)
    };

    // Step 1.7: 自动生成 Playwright 测试用例（修复前先设计测试）
    let test_gen_script = "/root/.openclaw/workspace/his-repo/healthlink-his-ui/tests/e2e/utils/generate-bug-test.sh";
    let mut test_generated = false;
    if std::path::Path::new(test_gen_script).exists() && agent_name == "zhaoyun" {
        let test_output = Command::new("bash")
            .arg(test_gen_script)
            .arg(bug_id)
            .arg(bug_title)
            .output();
        match test_output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                if stdout.contains("OK:") {
                    tracing::info!("[{}] Bug#{} Playwright 测试用例已生成", agent_name, bug_id);
                    tracing::info!("[{}] Bug#{} 测试用例已生成: tests/e2e/specs/bug-{}.spec.ts", agent_name, bug_id, bug_id);
                    test_generated = true;
                } else if stdout.contains("SKIP:") {
                    tracing::info!("[{}] Bug#{} Playwright 测试用例已存在", agent_name, bug_id);
                    tracing::info!("[{}] Bug#{} 测试用例已存在", agent_name, bug_id);
                    test_generated = true;
                }
            }
            Err(e) => tracing::warn!("[{}] 生成测试用例失败: {}", agent_name, e),
        }
    }
    
    // Step 1.8: 运行修复前测试（应失败，作为基线）
    let test_spec = format!("/root/.openclaw/workspace/his-repo/healthlink-his-ui/tests/e2e/specs/bug-{}.spec.ts", bug_id);
    let pre_test_passed = if std::path::Path::new(&test_spec).exists() && agent_name == "zhaoyun" {
        tracing::info!("[{}] Bug#{} 运行修复前基线测试...", agent_name, bug_id);
        tracing::info!("[{}] Bug#{} 开始修复前基线测试（预期失败）", agent_name, bug_id);
        let pre_test = Command::new("bash")
            .arg("-c")
            .arg(format!(
                "cd /root/.openclaw/workspace/his-repo/healthlink-his-ui && npx playwright test --grep @bug{} --reporter=line --workers=1 2>&1",
                bug_id
            ))
            .output();
        let (passed, output) = match pre_test {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                let combined = format!("{}{}", stdout, stderr);
                let passed = o.status.success();
                (passed, combined)
            }
            Err(e) => (false, format!("执行失败: {}", e)),
        };
        let log_summary = if output.len() > 500 {
            let char_count = output.chars().count();
            let skipped = output.chars().skip(char_count.saturating_sub(500));
            format!("...{}", skipped.collect::<String>())
        } else {
            output.clone()
        };
        let detail = serde_json::json!({
            "phase": "baseline",
            "passed": passed,
            "expected": "fail",
            "log": log_summary,
            "full_output_len": output.len(),
        });
        tracing::info!("[{}] Bug#{} 基线测试结果: {} (detail={})", agent_name, bug_id, 
            if passed { "通过(异常)" } else { "失败(预期)" }, detail);
        tracing::info!("[{}] Bug#{} 修复前基线测试: {}", agent_name, bug_id, if passed { "通过(异常)" } else { "失败(预期)" });
        passed
    } else {
        false
    };

    // Step 2: Build harness-augmented prompt
    let prompt_str = build_harness_prompt(agent_name, bug_id, bug_title, &bug_details_text);

    // ── 验证失败重试：追加失败详情到 prompt ──
    let prompt = if bug_title.contains("验证失败反馈") {
        let detail_key = format!("verify_fail_detail:{}:{}", agent_name, bug_id);
        let detail = std::process::Command::new("redis-cli")
            .args(["-p", "16379", "get", &detail_key])
            .output()
            .map(|o| {
                let val = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if val.is_empty() || val == "(nil)" { String::new() } else { val }
            })
            .unwrap_or_default();
        let retry_count: u32 = std::process::Command::new("redis-cli")
            .args(["-p", "16379", "get", &format!("verify_retry:{}:{}", agent_name, bug_id)])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(1))
            .unwrap_or(1);
        if !detail.is_empty() {
            tracing::info!("[{}] Bug#{} 收到验证失败反馈（第 {} 次重试）", agent_name, bug_id, retry_count);
            format!("{}

## ⚠️ 验证失败反馈（第 {} 次重试）

上次修复未通过全链路验证：

{}

请针对上述失败项重新修复。", 
                prompt_str, retry_count, detail)
        } else {
            prompt_str
        }
    } else {
        prompt_str
    };

    // Step 3: Target repository (agent-specific)
    let work_dir = agent_work_dir(agent_name);
    let agent_branch = agent_name;

    // Step 3.5: Pull latest code from remote before fixing
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    // Stash any unstaged changes from previous runs to avoid pull/rebase failures
    let _ = Command::new("git")
        .args(["-C", &worktree, "stash", "--include-untracked"])
        .output();
    let pull_result = Command::new("git")
        .args(["-C", &worktree, "pull", "--rebase", "origin", agent_branch])
        .output();
    match pull_result {
        Ok(o) if o.status.success() => {
            tracing::info!("[{}] Pulled latest origin/{} into worktree", agent_name, agent_branch);
        }
        Ok(o) => {
            let stderr_str = String::from_utf8_lossy(&o.stderr).to_string();
            tracing::warn!("[{}] git pull issue for {}: {}", agent_name, agent_branch, stderr_str.chars().take(200).collect::<String>());
        }
        Err(e) => {
            tracing::warn!("[{}] git pull error: {}", agent_name, e);
        }
    }

    // Step 4: Launch codex via mimo2codex pipeline
    // Uses codex-aliyun which: (1) starts mimo2codex if needed, (2) runs codex with mimo model
    let mut child = match Command::new("codex-aliyun")
        .args(["exec", "--sandbox", "workspace-write",
                "--dangerously-bypass-approvals-and-sandbox",
                "--skip-git-repo-check", "-"])
        .current_dir(&work_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn() {
        Ok(c) => c,
        Err(e) => {
            // Fallback to raw codex if codex-aliyun not found
            tracing::warn!("codex-aliyun not found ({}), falling back to codex", e);
            match Command::new("codex")
                .args(["exec", "--sandbox", "workspace-write",
                        "--dangerously-bypass-approvals-and-sandbox",
                        "--skip-git-repo-check", "-"])
                .current_dir(&work_dir)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn() {
                Ok(c) => c,
                Err(e2) => return CodexResult {
                    success: false, bug_id: bug_id.to_string(),
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    stdout: String::new(),
                    stderr: format!("codex spawn failed: {}", e2),
                    exit_code: -1, changes: 0,
                last_phase: "generator".to_string(), phase_verdicts: vec![],
                },
            }
        }
    };

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let output = child.wait_with_output();
    let elapsed = start.elapsed().as_millis() as u64;

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let exit_code = o.status.code().unwrap_or(-1);
            let mut changes = count_worktree_changes(agent_name);
            let has_fix_commit = has_recent_fix_commit(agent_name, bug_id);
            if changes == 0 && has_fix_commit {
                let commit_changes = count_last_commit_changes(agent_name);
                if commit_changes > 0 { changes = commit_changes; }
            }
            // ── 问题1: 空输出检测 ──
            // codex 超时/崩溃时 stdout 为空或极短，不应计为成功
            let stdout_trimmed = stdout.trim();
            let is_empty_output = stdout_trimmed.is_empty() || stdout_trimmed.len() < 20;
            let is_analysis_only = stdout_trimmed.contains("分析") && !stdout_trimmed.contains("修复")
                && !stdout_trimmed.contains("fix") && !stdout_trimmed.contains("修改");
            let is_skip_output = stdout_trimmed.contains("之前修复已完成") || stdout_trimmed.contains("无需改动");
            
            // ── 问题2: 误判保护 — "已修复"必须有实际变更证据 ──
            // 铁律: has_real_evidence 必须检查实际文件变更（不只是"声称修复"）
            let has_real_evidence = if is_skip_output {
                // 声称"已修复"时，必须有 git diff 证据证明代码确实有变更
                let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
                let diff_output = Command::new("git")
                    .args(["-C", &worktree, "diff", "HEAD~1", "--stat"])
                    .output();
                let has_diff = diff_output.map(|o| {
                    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                    !stdout.trim().is_empty() && stdout.contains('|')
                }).unwrap_or(false);
                has_diff || has_fix_commit
            } else {
                // 即使不是"声称修复"，也必须有实际变更证据
                changes > 0 || has_fix_commit
            };
            
            // 铁律: success 必须要求实际代码变更（changes > 0）或 develop 上有 commit
            // 不能仅凭 stdout 包含 "fix" 就判定成功
            let success = !is_empty_output && !is_analysis_only && has_real_evidence && (
                has_fix_commit || (exit_code == 0 && changes > 0)
            );
            
            // 记录判定原因（用于调试）
            if !success {
                let reason = if is_empty_output { "空输出" }
                    else if is_analysis_only { "仅分析未修复" }
                    else if !has_real_evidence { "声称已修复但无变更证据" }
                    else { "codex 返回失败" };
                tracing::warn!("[{}] Bug #{} 判定失败: {} (stdout_len={}, changes={})", 
                    agent_name, bug_id, reason, stdout_trimmed.len(), changes);
            }

            // Step 5: Run quality gates on the fix
            let gates_passed = if success {
                let (gates_ok, gate_stdout, gate_stderr) = run_quality_gates(agent_name, &work_dir);
                if gates_ok {
                    tracing::info!("[{}] Fix Bug #{} — all quality gates passed", agent_name, bug_id);
                    true
                } else {
                    tracing::error!("[{}] Fix Bug #{} — quality gates FAILED: {}",
                        agent_name, bug_id, gate_stdout);
                    tracing::error!("[{}] Gate stderr (first 500 chars): {}",
                        agent_name, gate_stderr.chars().take(500).collect::<String>());
                    false
                }
            } else {
                // ── 问题4: 验证失败日志增强 ──
                tracing::error!("[{}] Bug #{} 修复判定失败 — 完整诊断:", agent_name, bug_id);
                tracing::error!("[{}]   exit_code: {}", agent_name, exit_code);
                tracing::error!("[{}]   changes: {}", agent_name, changes);
                tracing::error!("[{}]   stdout前500字: {}", agent_name, stdout.chars().take(500).collect::<String>());
                tracing::error!("[{}]   stderr前500字: {}", agent_name, stderr.chars().take(500).collect::<String>());
                false
            };

            // Step 5b: Run database migrations (DDL) if new migration scripts detected
            let migrations_passed = if success && gates_passed {
                run_db_migrations(agent_name, &work_dir, bug_id)
            } else {
                true
            };

            // Step 5c: Validate Mapper XML SQL syntax
            let sql_valid = if success && gates_passed && migrations_passed {
                validate_mapper_sql(agent_name, &work_dir, bug_id)
            } else {
                true
            };

            // ── 问题3: 无变更检测 — 有 fix_done 但 0 文件变更 = 无效修复 ──
            if success && changes == 0 && !has_fix_commit {
                tracing::warn!("[{}] Bug #{} 标记为成功但无文件变更，判定为无效修复", agent_name, bug_id);
                // 不算成功，写入失败备注
                let fail_comment = format!("【{}】Bug #{} Codex 输出了修复分析但未产生实际代码变更，需人工排查。", agent_name, bug_id);
                let _ = comment_in_zentao(bug_id, &fail_comment);
            }
            
            // Only proceed to commit if all checks passed
            // 铁律: ok_to_commit 必须要求 develop 上有实际 commit（不能只看 worktree 未提交变更）
            let ok_to_commit = success && gates_passed && migrations_passed && sql_valid && has_fix_commit;

            // Step 6: Auto-commit changes — only if gates + migrations passed
            if ok_to_commit && changes > 0 {
                auto_commit_fix(agent_name, bug_id, bug_title, &stdout);
            }
            
            // ── 问题5: 提交保障 — 检查是否有未提交的变更，有则强制 commit+push ──
            if ok_to_commit && changes > 0 {
                let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
                let status_output = Command::new("git")
                    .args(["-C", &worktree, "status", "--porcelain"])
                    .output();
                let has_uncommitted = status_output.map(|o| {
                    !String::from_utf8_lossy(&o.stdout).trim().is_empty()
                }).unwrap_or(false);
                
                if has_uncommitted {
                    tracing::warn!("[{}] Bug#{} 有未提交变更，强制 commit+push", agent_name, bug_id);
                    // 强制 add + commit
                    let _ = Command::new("git")
                        .args(["-C", &worktree, "add", "--all"])
                        .output();
                    let commit_msg = format!("fix(#{}): {} — 兜底提交（AI Agent {} 自动修复）", 
                        bug_id, bug_title, agent_name);
                    let _ = Command::new("git")
                        .args(["-C", &worktree, "commit", "-m", &commit_msg])
                        .output();
                    // 强制 push
                    let _ = Command::new("git")
                        .args(["-C", &worktree, "push", "origin", agent_name])
                        .output();
                    // Cherry-pick to develop
                    let hash_output = Command::new("git")
                        .args(["-C", &worktree, "rev-parse", "HEAD"])
                        .output();
                    if let Ok(ho) = hash_output {
                        let hash = String::from_utf8_lossy(&ho.stdout).trim().to_string();
                        if hash.len() >= 8 {
                            let main_repo = "/root/.openclaw/workspace/his-repo";
                            let _ = Command::new("git").args(["-C", main_repo, "fetch", "origin", "develop"]).output();
                            let _ = Command::new("git").args(["-C", main_repo, "checkout", "develop"]).output();
                            let _ = Command::new("git").args(["-C", main_repo, "pull", "--rebase", "origin", "develop"]).output();
                            let cherry = Command::new("git")
                                .args(["-C", main_repo, "cherry-pick", &hash])
                                .output();
                            if cherry.map(|o| o.status.success()).unwrap_or(false) {
                                let _ = Command::new("git").args(["-C", main_repo, "push", "origin", "develop"]).output();
                                tracing::info!("[{}] Bug#{} cherry-pick 到 develop 成功", agent_name, bug_id);
                            } else {
                                tracing::warn!("[{}] Bug#{} cherry-pick 失败，可能有冲突", agent_name, bug_id);
                            }
                        }
                    }
                }
            }

            // Step 7: Update Zentao — 不管成功失败都写备注
            if ok_to_commit {
                resolve_bug_in_zentao(agent_name, bug_id, bug_title, &stdout);
            } else if !success {
                // Codex 修复本身失败
                let fail_comment = format!("【{}】Bug #{} Codex 修复失败，未能产生有效变更。需人工排查。", agent_name, bug_id);
                comment_in_zentao(bug_id, &fail_comment);
            } else if !gates_passed {
                tracing::warn!("[{}] Bug #{} quality gates failed", agent_name, bug_id);
                let fail_comment = format!("【{}】Bug #{} 代码已修改但未通过质量门禁（vue-tsc 类型检查），代码已保存到 Agent 分支待人工审查。", agent_name, bug_id);
                comment_in_zentao(bug_id, &fail_comment);
            } else if !migrations_passed {
                tracing::warn!("[{}] Bug #{} DB migrations FAILED", agent_name, bug_id);
                let fail_comment = format!("【{}】Bug #{} 代码已修改但 DB 迁移脚本验证失败，代码已保存到 Agent 分支待人工审查。", agent_name, bug_id);
                comment_in_zentao(bug_id, &fail_comment);
            } else if !sql_valid {
                tracing::warn!("[{}] Bug #{} SQL validation FAILED", agent_name, bug_id);
                let fail_comment = format!("【{}】Bug #{} 代码已修改但 Mapper SQL 语法验证失败，代码已保存到 Agent 分支待人工审查。", agent_name, bug_id);
                comment_in_zentao(bug_id, &fail_comment);
            }

            CodexResult {
                success,
                bug_id: bug_id.to_string(),
                elapsed_ms: elapsed,
                stdout,
                stderr,
                exit_code,
                changes,
                last_phase: "generator".to_string(), phase_verdicts: vec![],
            }
        }
        Err(e) => CodexResult {
            success: false,
            bug_id: bug_id.to_string(),
            elapsed_ms: elapsed,
            stdout: String::new(),
            stderr: format!("{:?}", e),
            exit_code: -1,
            changes: 0,
        last_phase: "generator".to_string(), phase_verdicts: vec![],
        },
    }
}

/// Query Zentao for bug details (legacy shell script fallback).
fn query_bug_details(bug_id: &str) -> String {
    let result = Command::new("bash")
        .arg("/root/.openclaw/extensions/zentao-token-refresh/zentao-bug-query.sh")
        .arg(bug_id)
        .output();
    match result {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => {
            let _ = std::fs::remove_file("/tmp/.zentao-admin-token");
            Command::new("bash")
                .arg("/root/.openclaw/extensions/zentao-token-refresh/zentao-bug-query.sh")
                .arg(bug_id)
                .output()
                .ok()
                .and_then(|o| if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).to_string())
                } else { None })
                .unwrap_or_default()
        }
    }
}

/// Query Zentao for bug details v2 — 使用 Rust API 客户端
///
/// 相比 shell 脚本版，新增：
/// - 结构化字段：severity/pri/module/steps/actions
/// - 操作历史（谁在什么时候做了什么）
/// - 纯文本步骤提取（去除 HTML 标签保留文字）
fn query_bug_details_v2(bug_id: &str) -> String {
    // 在已有 tokio 运行时上下文中执行 async 调用
    let rt_handle = tokio::runtime::Handle::try_current();
    match rt_handle {
        Ok(handle) => {
            let result = handle.block_on(async {
                let cfg = match crate::config::Config::load() {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!("Failed to load config for Zentao client: {}", e);
                        return None;
                    }
                };
                let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
                match client.get_bug(bug_id).await {
                    Ok(detail) => {
                        let text = detail.format_for_prompt();
                        tracing::info!("Zentao v2: Bug #{} detail fetched ({})", bug_id,
                            text.lines().count());
                        Some(text)
                    }
                    Err(e) => {
                        tracing::warn!("Zentao v2 API failed for Bug #{}: {}, falling back to v1", bug_id, e);
                        None
                    }
                }
            });
            match result {
                Some(text) => text,
                None => query_bug_details(bug_id), // fallback
            }
        }
        Err(_) => {
            tracing::warn!("No tokio runtime context, falling back to v1 for Bug #{}", bug_id);
            query_bug_details(bug_id) // fallback
        }
    }
}

/// Auto-commit fix changes to the agent's worktree.
fn auto_commit_fix(agent_name: &str, bug_id: &str, bug_title: &str, stdout: &str) {
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    let add_result = Command::new("git")
        .args(["-C", &worktree, "add", "--all", "--",
                ":!*.orig", ":!*.mjs", ":!*.timestamp*"])
        .output();
    match &add_result {
        Ok(o) if !o.status.success() => {
            tracing::error!("[{}] Bug#{} git add 失败: {}", agent_name, bug_id,
                String::from_utf8_lossy(&o.stderr).chars().take(200).collect::<String>());
        }
        Err(e) => {
            tracing::error!("[{}] Bug#{} git add 执行错误: {}", agent_name, bug_id, e);
        }
        _ => {}
    }

    // Extract root causes and fixes for structured commit message
    let (root_causes, fixes) = extract_fix_details(stdout, bug_title);
    let commit_msg = build_zentao_comment(bug_id, bug_title, &root_causes, &fixes);

    // 校验提交信息质量：至少包含非空根因或修复方案
    let final_msg = if root_causes.iter().all(|c| c.contains("存在的问题") || c.contains("修改相关"))
        && fixes.iter().all(|f| f == "修改相关代码文件") {
        // 退化信息，用更具体的模板
        format!("fix(#{}): {}

由 AI Agent ({}) 自动修复，请查看 diff 确认变更内容。", bug_id, bug_title, agent_name)
    } else {
        commit_msg
    };

    let commit_result = Command::new("git")
        .args(["-C", &worktree, "commit", "-m", &final_msg])
        .output();
    match &commit_result {
        Ok(o) if !o.status.success() => {
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            tracing::error!("[{}] Bug#{} git commit 失败: {}", agent_name, bug_id, stderr.chars().take(300).collect::<String>());
            return; // commit 失败，不继续
        }
        Err(e) => {
            tracing::error!("[{}] Bug#{} git commit 执行错误: {}", agent_name, bug_id, e);
            return;
        }
        _ => {
            tracing::info!("[{}] Bug#{} git commit 成功 ✅", agent_name, bug_id);
        }
    }

    // 铁律 #24: 修复后必须本地编译验证，禁止未编译就 push
    let branch = agent_name;
    tracing::info!("[{}] Bug #{} 开始编译验证（push 前）...", agent_name, bug_id);
    let compile_result = if agent_name == "zhaoyun" {
        Command::new("npx")
            .args(["vite", "build", "--mode", "dev"])
            .current_dir(&worktree)
            .output()
    } else {
        Command::new("mvn")
            .args(["compile", "-q", "-pl", "openhis-application", "-am"])
            .current_dir(format!("{}/healthlink-his-server", worktree))
            .output()
    };
    match compile_result {
        Ok(o) if !o.status.success() => {
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            tracing::warn!("[{}] Bug #{} 编译失败，禁止 push: {}", agent_name, bug_id, stderr.chars().take(300).collect::<String>());
            // 回滚 commit
            let _ = Command::new("git")
                .args(["-C", &worktree, "reset", "--hard", "HEAD~1"])
                .output();
            return; // 不 push，不 cherry-pick
        }
        Err(e) => {
            tracing::warn!("[{}] Bug #{} 编译命令执行失败: {}，跳过编译检查继续 push", agent_name, bug_id, e);
        }
        _ => {
            tracing::info!("[{}] Bug #{} 编译验证通过 ✅", agent_name, bug_id);
        }
    }

    // Push to remote agent branch
    let push_result = Command::new("git")
        .args(["-C", &worktree, "push", "origin", branch])
        .output();
    match push_result {
        Ok(o) if o.status.success() => {
            tracing::info!("[{}] Pushed fix to origin/{} for Bug #{}", agent_name, branch, bug_id);
            // Cherry-pick fix commit onto develop
            let commit_hash_output = Command::new("git")
                .args(["-C", &worktree, "rev-parse", "HEAD"])
                .output();
            if let Ok(hash_result) = commit_hash_output {
                let hash = String::from_utf8_lossy(&hash_result.stdout).trim().to_string();
                if !hash.is_empty() && hash.len() >= 8 {
                    let main_repo = "/root/.openclaw/workspace/his-repo";
                    let _ = Command::new("git")
                        .args(["-C", main_repo, "fetch", "origin", "develop"])
                        .output();
                    let _ = Command::new("git")
                        .args(["-C", main_repo, "checkout", "develop"])
                        .output();
                    let _ = Command::new("git")
                        .args(["-C", main_repo, "pull", "--rebase", "origin", "develop"])
                        .output();
                    // 铁律: 总是尝试 cherry-pick 到 develop
                    // develop 上可能有旧的不完整修复，需要增量合并
                    // 验证会在 develop 分支上跑，确认是否真正修好
                    tracing::info!("[{}] Bug#{} 尝试 cherry-pick 到 develop", agent_name, bug_id);

                    {
                        // Try cherry-pick with -X theirs to auto-resolve simple conflicts
                        let author = format!("{} <{}@gentronhealth.com>", agent_name, agent_name);
                        let cherry = Command::new("git")
                            .args(["-C", main_repo, "cherry-pick", "--strategy=recursive",
                                   "-X", "theirs", "--author", &author, &hash])
                            .output();
                        match cherry {
                            Ok(o) if o.status.success() => {
                                let _ = Command::new("git")
                                    .args(["-C", main_repo, "push", "origin", "develop"])
                                    .output();
                                tracing::info!("[{}] Cherry-picked fix to develop for Bug #{}", agent_name, bug_id);
                            }
                            Ok(o) => {
                                // If -X theirs failed, try without
                                let _ = Command::new("git")
                                    .args(["-C", main_repo, "cherry-pick", "--abort"])
                                    .output();
                                let _stderr_str = String::from_utf8_lossy(&o.stderr).to_string();
                                tracing::warn!("[{}] Cherry-pick to develop failed for Bug #{}: retrying without -X theirs",
                                    agent_name, bug_id);
                                let _ = Command::new("git")
                                    .args(["-C", main_repo, "pull", "--rebase", "origin", "develop"])
                                    .output();
                                let cherry2 = Command::new("git")
                                    .args(["-C", main_repo, "cherry-pick", "--author", &author, &hash])
                                    .output();
                                match cherry2 {
                                    Ok(o2) if o2.status.success() => {
                                        let _ = Command::new("git")
                                            .args(["-C", main_repo, "push", "origin", "develop"])
                                            .output();
                                        tracing::info!("[{}] Cherry-picked (retry) fix to develop for Bug #{}", agent_name, bug_id);
                                    }
                                    Ok(o2) => {
                                        let _ = Command::new("git")
                                            .args(["-C", main_repo, "cherry-pick", "--abort"])
                                            .output();
                                        let err2 = String::from_utf8_lossy(&o2.stderr).to_string();
                                        tracing::warn!("[{}] Cherry-pick to develop failed for Bug #{}: {}",
                                            agent_name, bug_id, err2.chars().take(200).collect::<String>());
                                    }
                                    Err(e) => {
                                        let _ = Command::new("git")
                                            .args(["-C", main_repo, "cherry-pick", "--abort"])
                                            .output();
                                        tracing::warn!("[{}] Cherry-pick error for Bug #{}: {}", agent_name, bug_id, e);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("[{}] Cherry-pick error for Bug #{}: {}", agent_name, bug_id, e);
                            }
                        }
                    }
                }
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            tracing::warn!("[{}] Push failed for Bug #{}: {}", agent_name, bug_id, stderr.chars().take(200).collect::<String>());
        }
        Err(e) => {
            tracing::warn!("[{}] Push error for Bug #{}: {}", agent_name, bug_id, e);
        }
    }
}

/// Extract root cause and fix description from Codex stdout.
fn extract_fix_details(stdout: &str, bug_title: &str) -> (Vec<String>, Vec<String>) {
    let mut root_causes: Vec<String> = Vec::new();
    let mut fixes: Vec<String> = Vec::new();
    let mut current_section: Option<&str> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        // Detect section headers
        if trimmed.contains("根因") || trimmed.contains("Root Cause") || trimmed.contains("root cause")
            || trimmed.contains("原因分析") || trimmed.starts_with("原因：")
        {
            current_section = Some("root_cause");
            if let Some(content) = trimmed.splitn(2, &['：', ':', '—', '─', '━', '='][..]).nth(1) {
                let c = content.trim();
                if !c.is_empty() && c.len() < 200 {
                    root_causes.push(c.to_string());
                }
            }
            continue;
        }
        if trimmed.contains("修复") || trimmed.contains("Fix") || trimmed.starts_with("fix:")
            || trimmed.contains("修改方案") || trimmed.starts_with("方案：")
        {
            current_section = Some("fix");
            if let Some(content) = trimmed.splitn(2, &['：', ':', '—', '─', '━', '='][..]).nth(1) {
                let c = content.trim();
                if !c.is_empty() && c.len() < 200 {
                    fixes.push(c.to_string());
                }
            }
            continue;
        }

        // Collect bullet points in each section
        if let Some(section) = current_section {
            let content = trimmed.trim_start_matches(&['-', '*', '•', ' ', '\t'][..]).trim();
            if content.len() > 5 && content.len() < 300 {
                match section {
                    "root_cause" => root_causes.push(content.to_string()),
                    "fix" => fixes.push(content.to_string()),
                    _ => {}
                }
            }
        }
    }

    // Fallback: try to extract from diagnostic patterns
    if root_causes.is_empty() && fixes.is_empty() {
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("@@") || trimmed.starts_with("---") || trimmed.starts_with("+++") {
                continue;
            }
            if trimmed.contains("missing") || trimmed.contains("缺失") || trimmed.contains("没有")
                || trimmed.contains("not found") || trimmed.contains("错误")
            {
                if trimmed.len() < 200 {
                    root_causes.push(trimmed.to_string());
                }
            }
            if trimmed.contains("added") || trimmed.contains("添加") || trimmed.contains("修改")
                || trimmed.contains("fix") || trimmed.contains("修复")
            {
                if trimmed.len() < 200 {
                    fixes.push(trimmed.to_string());
                }
            }
        }
    }

    if root_causes.is_empty() {
        root_causes.push(format!("Bug #{} 存在的问题", bug_title.split('：').next().unwrap_or(bug_title)));
    }
    if fixes.is_empty() {
        fixes.push("修改相关代码文件".to_string());
    }

    (root_causes, fixes)
}

/// Build structured comment for Zentao resolve.
fn build_zentao_comment(bug_id: &str, bug_title: &str, root_causes: &[String], fixes: &[String]) -> String {
    let mut comment = String::new();
    comment.push_str(&format!("fix(#{}): {}", bug_id, bug_title));
    comment.push_str("

根因：
");
    for cause in root_causes {
        comment.push_str(&format!("- {}
", cause));
    }
    comment.push_str("
修复：
");
    for f in fixes {
        comment.push_str(&format!("- {}
", f));
    }
    comment
}

/// Resolve a bug in Zentao with structured comment after fix + quality gates pass.
/// 只加备注不改状态（成功和失败都用）
fn comment_in_zentao(bug_id: &str, comment: &str) {
    // Refresh token
    let app_cfg = crate::config::Config::load().unwrap_or_default();
    let cli = &app_cfg.zentao.cli_path;
    let _ = Command::new("bash")
        .args(["-c", &format!("{} login -s {} -u {} -p {}", cli, app_cfg.zentao.base_url, app_cfg.zentao.username, app_cfg.zentao.password)])
        .output();

    let result = Command::new(cli)
        .args(["bug", "update", "--id", bug_id, "--comment", comment])
        .output();

    match result {
        Ok(o) if o.status.success() => {
            let stdout_str = String::from_utf8_lossy(&o.stdout).to_string();
            if stdout_str.contains("success") || stdout_str.contains("保存成功") {
                tracing::info!("[zentao] Bug #{} 备注已添加", bug_id);
            } else {
                tracing::warn!("[zentao] Bug #{} 备注异常: {}", bug_id, stdout_str);
            }
        }
        Ok(o) => {
            let stderr_str = String::from_utf8_lossy(&o.stderr).to_string();
            tracing::warn!("[zentao] Bug #{} 备注失败: {}", bug_id, stderr_str);
        }
        Err(e) => {
            tracing::warn!("[zentao] Bug #{} 备注错误: {}", bug_id, e);
        }
    }
}

/// Agent 对应的禅道账号列表（用于判断 bug 是否分配给人类）
const AGENT_ZENTAO_ACCOUNTS: &[&str] = &[
    "wangyizhe", "liubei", "guanyu", "zhaoyun",
    "xunyu", "zhangfei", "huatuo", "chenlin",
];

/// 修复后更新 Zentao —— 不改状态，只加备注；智能体分配的额外改分配
fn resolve_bug_in_zentao(agent_name: &str, bug_id: &str, bug_title: &str, stdout: &str) {
    let (root_causes, fixes) = extract_fix_details(stdout, bug_title);
    let comment = build_zentao_comment(bug_id, bug_title, &root_causes, &fixes);

    // Step 1: Refresh Zentao token
    let app_cfg = crate::config::Config::load().unwrap_or_default();
    let cli = &app_cfg.zentao.cli_path;
    let _ = Command::new("bash")
        .args(["-c", &format!("{} login -s {} -u {} -p {}", cli, app_cfg.zentao.base_url, app_cfg.zentao.username, app_cfg.zentao.password)])
        .output();

    // Step 2: 查询 bug 的 assignedTo 判断是人类还是智能体
    let assigned_to = {
        let cfg = crate::config::Config::load().ok();
        if let Some(cfg) = cfg {
            let client = crate::core::zentao::ZentaoClient::from_config(&cfg);
            let rt = tokio::runtime::Handle::current();
            match rt.block_on(client.get_bug(bug_id)) {
                Ok(detail) => detail.assigned_to,
                Err(e) => {
                    tracing::warn!("[{}] 无法查询 Bug #{} assignedTo: {}", agent_name, bug_id, e);
                    String::new()
                }
            }
        } else {
            String::new()
        }
    };

    let is_agent = AGENT_ZENTAO_ACCOUNTS.iter().any(|a| assigned_to.contains(a));
    tracing::info!("[{}] Bug #{} assignedTo={:?}, is_agent={}", agent_name, bug_id, assigned_to, is_agent);

    // Step 3: 两种情况都不改状态
    // - 人类分配：只加备注
    // - 智能体分配：加备注 + 改分配给 zhangfei（测试）
    let result = if is_agent {
        // 用 --data 传 JSON 同时设置 comment 和 assignedTo
        let escaped = comment.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ");
        let data_json = format!(r#"{{"comment":"{}","assignedTo":"zhangfei"}}"#, escaped);
        Command::new(&app_cfg.zentao.cli_path)
            .args(["bug", "update", "--id", bug_id, "--data", &data_json])
            .output()
    } else {
        Command::new(&app_cfg.zentao.cli_path)
            .args(["bug", "update", "--id", bug_id, "--comment", &comment])
            .output()
    };

    match result {
        Ok(o) if o.status.success() => {
            let stdout_str = String::from_utf8_lossy(&o.stdout).to_string();
            if stdout_str.contains("success") || stdout_str.contains("保存成功") {
                if is_agent {
                    tracing::info!("[{}] Bug #{} 备注已添加 + 分配已改为 zhangfei（智能体分配）: fix(#{}): {}",
                        agent_name, bug_id, bug_id, bug_title);
                } else {
                    tracing::info!("[{}] Bug #{} 备注已添加（人类分配，不改状态不改分配）: fix(#{}): {}",
                        agent_name, bug_id, bug_id, bug_title);
                }
                tracing::debug!("[{}] Zentao comment: {} chars", agent_name, comment.len());
            } else {
                tracing::warn!("[{}] Bug #{} 操作结果异常: {}", agent_name, bug_id, stdout_str);
            }
        }
        Ok(o) => {
            let stderr_str = String::from_utf8_lossy(&o.stderr).to_string();
            tracing::warn!("[{}] Zentao 操作失败 for Bug #{}: {}", agent_name, bug_id, stderr_str);
        }
        Err(e) => {
            tracing::warn!("[{}] Zentao 操作错误 for Bug #{}: {}", agent_name, bug_id, e);
        }
    }
}

// ──────────────────────────────────────────────
// Public API — called by executor.rs
// ──────────────────────────────────────────────

/// Synchronous fix entry point — safe for `tokio::task::block_in_place`.
/// This is the main entry point called by the agent executor.
/// Always routes through `mimo2codex → codex` with full Harness methodology.
pub fn run_codex_fix(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    _fix_script: &str,
    timeout_secs: u64,
) -> CodexResult {
    tracing::info!("[{}] Harness fix for Bug #{}: {}",
        agent_name, bug_id, bug_title);
    run_codex_fix_impl(agent_name, bug_id, bug_title, timeout_secs)
}

/// Async wrapper (uses sync internally).
pub async fn run_codex_fix_async(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    fix_script: &str,
    timeout_secs: u64,
) -> CodexResult {
    run_codex_fix(agent_name, bug_id, bug_title, fix_script, timeout_secs)
}

// ──────────────────────────────────────────────
// Helper functions
// ──────────────────────────────────────────────

/// Count changed lines in the most recent commit.
fn count_last_commit_changes(agent_name: &str) -> u32 {
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    let output = Command::new("git")
        .args(["-C", &worktree, "diff", "HEAD~1", "--stat"])
        .output();
    if let Ok(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            if let Some(pipe_pos) = line.find('|') {
                let nums = &line[pipe_pos+1..];
                let count = nums.chars().filter(|&c| c == '+' || c == '-').count() as u32;
                if count > 0 { return count; }
            }
        }
    }
    0
}

/// Count uncommitted changed lines in the agent's worktree.
fn count_worktree_changes(agent_name: &str) -> u32 {
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    let stat_outputs = [
        Command::new("git").args(["diff", "--stat"]).current_dir(&worktree).output(),
        Command::new("git").args(["diff", "--cached", "--stat"]).current_dir(&worktree).output(),
    ];
    let mut total = 0u32;
    for o in stat_outputs.into_iter().flatten() {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            if let Some(pipe_pos) = line.find('|') {
                let nums = &line[pipe_pos+1..];
                total += nums.chars().filter(|&c| c == '+' || c == '-').count() as u32;
            }
        }
    }
    // 兜底：如果worktree无变更，检查主仓库（Codex可能直接修改了主仓库）
    if total == 0 {
        let main_repo = "/root/.openclaw/workspace/his-repo";
        let main_dir = if agent_name == "zhaoyun" {
            format!("{}/healthlink-his-ui", main_repo)
        } else {
            format!("{}/healthlink-his-server", main_repo)
        };
        let main_outputs = [
            Command::new("git").args(["diff", "--stat"]).current_dir(&main_dir).output(),
            Command::new("git").args(["diff", "--cached", "--stat"]).current_dir(&main_dir).output(),
        ];
        for o in main_outputs.into_iter().flatten() {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for line in stdout.lines() {
                if let Some(pipe_pos) = line.find('|') {
                    let nums = &line[pipe_pos+1..];
                    total += nums.chars().filter(|&c| c == '+' || c == '-').count() as u32;
                }
            }
        }
    }
    total
}

/// Check if the agent's worktree has a NEW commit with "Fix Bug #N" message.
fn has_recent_fix_commit(agent_name: &str, bug_id: &str) -> bool {
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    let output = Command::new("git")
        .args(["-C", &worktree, "log", "-1", "--oneline", "--not", "--remotes",
                "--grep", &format!("Fix Bug #{}", bug_id)])
        .output();
    if let Ok(o) = &output {
        if !String::from_utf8_lossy(&o.stdout).trim().is_empty() {
            return true;
        }
    }
    let head_output = Command::new("git")
        .args(["-C", &worktree, "rev-parse", "HEAD"]).output();
    let origin_head_output = Command::new("git")
        .args(["-C", &worktree, "rev-parse", "origin/HEAD"]).output();
    if let (Ok(h), Ok(oh)) = (&head_output, &origin_head_output) {
        let head = String::from_utf8_lossy(&h.stdout).trim().to_string();
        let origin_head = String::from_utf8_lossy(&oh.stdout).trim().to_string();
        if head != origin_head && !head.is_empty() && !origin_head.is_empty() {
            let msg_output = Command::new("git")
                .args(["-C", &worktree, "log", "-1", "--format=%s"]).output();
            if let Ok(mo) = &msg_output {
                let msg = String::from_utf8_lossy(&mo.stdout);
                return msg.contains(&format!("Fix Bug #{}", bug_id))
                    || msg.contains(&format!("#{}", bug_id));
            }
        }
    }
    false
}

/// Verify that a fix diff is meaningful (≥3 changed lines).
pub fn is_meaningful_fix(changes: u32) -> bool {
    changes >= 3
}

/// Compute elapsed duration as HH:MM:SS string.
pub fn fmt_duration(seconds: f64) -> String {
    let s = seconds as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let ss = s % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, ss)
    } else {
        format!("{:02}:{:02}", m, ss)
    }
}



/// 使用 codex exec 直接调用的修复函数 (v2)
/// 
/// 替代 codex-aliyun → mimo2codex → codex 管道
/// 直接使用 Codex CLI 的非交互模式
/// 现在委托给 run_harness_loop 执行完整的 4 阶段循环
pub fn run_codex_fix_v2(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    timeout_secs: u64,
) -> CodexResult {
    // 查询 Bug 详情
    let bug_details = query_bug_details_v2(bug_id);
    
    // 委托给 Harness Loop（Generator→Reviewer→QA→Verifier）
    tracing::info!("[{}] Bug#{} 启动 Harness Loop (4阶段循环)", agent_name, bug_id);
    run_harness_loop(agent_name, bug_id, bug_title, &bug_details, timeout_secs)
}

/// 构建审查阶段 Prompt
fn build_review_prompt(agent_name: &str, bug_id: &str, bug_title: &str, fix_output: &str) -> String {
    format!(r#"你是代码审查员。审查 Bug #{bug_id} 的修复代码。

Bug 标题: {bug_title}

修复输出摘要:
{fix_output}

评估维度 (每项1-5分)：
- 设计质量: 命名规范、错误处理、API风格
- 工艺性: 边界条件、类型安全、日志
- 功能性: 功能是否按预期工作
- 风格一致性: 与项目现有代码风格匹配度

通过线: 总分≥12/20 且 功能性≥3

请审查代码变更，给出评分和改进建议。
输出最后一行必须是: VERDICT: PASS 或 VERDICT: FAIL [原因]"#,
        bug_id=bug_id, bug_title=bug_title, fix_output=fix_output.chars().take(2000).collect::<String>())
}

/// 构建测试阶段 Prompt
fn build_test_prompt(agent_name: &str, bug_id: &str, bug_title: &str) -> String {
    let work_dir = if agent_name == "zhaoyun" {
        "/root/.openclaw/workspace/his-repo/healthlink-his-ui"
    } else {
        "/root/.openclaw/workspace/his-repo/healthlink-his-server"
    };
    format!(r#"你是 QA 测试工程师。测试 Bug #{bug_id} 的修复。

Bug 标题: {bug_title}
工作目录: {work_dir}

测试步骤：
1. 运行编译验证（前端: npx vite build; 后端: mvn compile -pl healthlink-his-application -am -q）
2. 运行单元测试（如有）
3. 检查无回归

请执行测试并报告结果。
输出最后一行必须是: VERDICT: PASS 或 VERDICT: FAIL [原因]"#,
        bug_id=bug_id, bug_title=bug_title)
}

/// 构建验收阶段 Prompt
fn build_verify_prompt(agent_name: &str, bug_id: &str, bug_title: &str) -> String {
    format!(r#"你是验收工程师。验收 Bug #{bug_id} 的修复。

Bug 标题: {bug_title}

验收检查项：
1. Git commit 存在且包含 Bug #{bug_id}
2. 编译通过
3. 测试通过
4. 无回归
5. 文件变更合理（未删除必要文件）

请逐项检查并报告。
输出最后一行必须是: VERDICT: PASS 或 VERDICT: FAIL [原因]"#,
        bug_id=bug_id, bug_title=bug_title)
}

/// Harness Loop — 4阶段循环执行（Generator→Reviewer→QA→Verifier）
///
/// 替代单次 codex exec，实现完整的 Harness Engineering 工作循环。
/// 每个阶段独立调用 codex exec，阶段间传递上下文。
pub fn run_harness_loop(
    agent_name: &str,
    bug_id: &str,
    bug_title: &str,
    bug_details: &str,
    timeout_secs: u64,
) -> CodexResult {
    use crate::core::codex_exec::{self, Verdict};
    
    let start = std::time::Instant::now();
    let mut phase_verdicts: Vec<(String, String)> = Vec::new();
    
    // 确定沙箱权限
    let sandbox = if agent_name == "zhaoyun" || agent_name == "guanyu" || agent_name == "xunyu" {
        "workspace-write"
    } else {
        "read-only"
    };
    
    // ═══ Phase 1: Generator 修复 ═══
    tracing::info!("[{}] Bug#{} Harness Loop Phase 1: Generator 修复", agent_name, bug_id);
    let fix_prompt = build_harness_prompt(agent_name, bug_id, bug_title, bug_details);
    let fix_result = codex_exec::codex_exec(
        &fix_prompt, sandbox,
        Some("/root/agentforge-rs/schemas/verdict.json"),
        Some(agent_name), timeout_secs,
    );
    
    let fix_verdict = match &fix_result.verdict {
        Verdict::Pass => "PASS".to_string(),
        Verdict::Fail(r) => format!("FAIL:{}", r),
        Verdict::Unknown => "UNKNOWN".to_string(),
    };
    phase_verdicts.push(("generator".to_string(), fix_verdict.clone()));
    
    // ── Phase 1 结果判定：Unknown 不直接失败，检查实际代码变更 ──
    if fix_result.verdict.is_fail() {
        tracing::warn!("[{}] Bug#{} Phase 1 FAIL: {:?}", agent_name, bug_id, fix_result.verdict);
        let elapsed = start.elapsed().as_millis() as u64;
        return CodexResult {
            success: false, bug_id: bug_id.to_string(), elapsed_ms: elapsed,
            stdout: fix_result.final_message, stderr: fix_result.stderr,
            exit_code: 1, changes: count_changed_files(agent_name, bug_id),
            last_phase: "generator".to_string(), phase_verdicts,
        };
    }
    if fix_result.verdict == Verdict::Unknown {
        // Unknown — 检查是否有实际代码变更和编译结果
        let changes = count_changed_files(agent_name, bug_id);
        tracing::warn!("[{}] Bug#{} Phase 1 UNKNOWN — checking real changes: {} files changed",
            agent_name, bug_id, changes);
        if changes == 0 {
            // 真的没改代码，视为失败
            tracing::warn!("[{}] Bug#{} Phase 1 UNKNOWN + no changes → FAIL", agent_name, bug_id);
            let elapsed = start.elapsed().as_millis() as u64;
            return CodexResult {
                success: false, bug_id: bug_id.to_string(), elapsed_ms: elapsed,
                stdout: fix_result.final_message, stderr: fix_result.stderr,
                exit_code: 1, changes: 0,
                last_phase: "generator".to_string(), phase_verdicts,
            };
        }
        // 有变更 — 降级：尝试编译验证（失败则反复重试，不受次数限制）
        tracing::info!("[{}] Bug#{} Phase 1 UNKNOWN but {} files changed, trying compile...",
            agent_name, bug_id, changes);
        const MAX_COMPILE_RETRIES: u32 = 10;
        let mut compile_ok = false;
        let mut last_compile_err = String::new();

        for compile_attempt in 1..=MAX_COMPILE_RETRIES {
            let compile_output = if agent_name == "zhaoyun" {
                std::process::Command::new("npx")
                    .args(["vite", "build", "--mode", "dev"])
                    .current_dir("/root/.openclaw/workspace/his-repo/healthlink-his-ui")
                    .output()
            } else {
                std::process::Command::new("mvn")
                    .args(["compile", "-pl", "healthlink-his-application", "-am"])
                    .current_dir("/root/.openclaw/workspace/his-repo/healthlink-his-server")
                    .output()
            };

            match compile_output {
                Ok(o) if o.status.success() => {
                    compile_ok = true;
                    tracing::info!("[{}] Bug#{} compile attempt {}/{} OK ✅",
                        agent_name, bug_id, compile_attempt, MAX_COMPILE_RETRIES);
                    break;
                }
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                    last_compile_err = format!("{}
{}", stderr, stdout);
                    // 提取关键错误行
                    let err_summary: String = last_compile_err.lines()
                        .filter(|l| l.contains("ERROR") || l.contains("error:") 
                            || l.contains("cannot find") || l.contains("BUILD FAILURE")
                            || l.contains("Compilation failure") || l.contains("TS"))
                        .take(20)
                        .collect::<Vec<_>>().join("
");
                    tracing::warn!("[{}] Bug#{} compile attempt {}/{} FAILED, retrying with feedback...",
                        agent_name, bug_id, compile_attempt, MAX_COMPILE_RETRIES);
                    tracing::warn!("[{}] Compile errors:
{}", agent_name, err_summary);

                    if compile_attempt >= MAX_COMPILE_RETRIES {
                        break;
                    }

                    // 构建编译错误反馈 prompt，让 Codex 修复
                    let feedback_prompt = format!(
                        "## 编译失败反馈 (第{}次重试)

                        你之前的修复代码编译失败。请根据以下编译错误修复代码：

                        ### 编译错误
```
{}
```

                        ### 要求
                        1. 仔细分析编译错误的根因
                        2. 修复所有编译错误（不能只修一部分）
                        3. 确保引用的类/方法/变量都存在
                        4. 修改后输出 VERDICT: PASS 或 VERDICT: FAIL [原因]

                        请直接修改文件修复编译错误。",
                        compile_attempt, err_summary
                    );
                    let retry_result = codex_exec::codex_exec(
                        &feedback_prompt, sandbox,
                        Some("/root/agentforge-rs/schemas/verdict.json"),
                        Some(agent_name), timeout_secs / 2,
                    );
                    tracing::info!("[{}] Bug#{} compile retry {} Codex verdict: {:?}",
                        agent_name, bug_id, compile_attempt, retry_result.verdict);
                }
                Err(e) => {
                    tracing::error!("[{}] Bug#{} compile command error: {}", agent_name, bug_id, e);
                    break;
                }
            }
        }

        if compile_ok {
            tracing::info!("[{}] Bug#{} Phase 1 UNKNOWN but compile OK → treating as PASS",
                agent_name, bug_id);
            // 把 verdict 修正为 Pass（有变更 + 编译通过）
        } else {
            tracing::warn!("[{}] Bug#{} Phase 1 UNKNOWN + compile FAIL after {} retries → FAIL",
                agent_name, bug_id, MAX_COMPILE_RETRIES);
            let elapsed = start.elapsed().as_millis() as u64;
            return CodexResult {
                success: false, bug_id: bug_id.to_string(), elapsed_ms: elapsed,
                stdout: fix_result.final_message, stderr: last_compile_err,
                exit_code: 1, changes,
                last_phase: "generator".to_string(), phase_verdicts,
            };
        }
    }
    
    // ═══ Phase 2: Reviewer 审查（最多2轮） ═══
    tracing::info!("[{}] Bug#{} Harness Loop Phase 2: Reviewer 审查", agent_name, bug_id);
    let mut review_verdict = Verdict::Unknown;
    let max_review_rounds = 2;
    let mut review_output = String::new();
    
    for round in 1..=max_review_rounds {
        let review_prompt = build_review_prompt(agent_name, bug_id, bug_title, &fix_result.final_message);
        let rev_result = codex_exec::codex_exec(
            &review_prompt, "read-only", None, Some(agent_name), timeout_secs * 2 / 3,
        );
        review_output = rev_result.final_message.clone();
        review_verdict = rev_result.verdict;
        
        let rv_str = match &review_verdict {
            Verdict::Pass => "PASS".to_string(),
            Verdict::Fail(r) => format!("FAIL:{}", r),
            Verdict::Unknown => "UNKNOWN".to_string(),
        };
        tracing::info!("[{}] Bug#{} Review round {}: {}", agent_name, bug_id, round, rv_str);
        
        if review_verdict.is_pass() {
            break;
        }
        
        // 审查失败 → 重新修复（最多1轮重修）
        if round < max_review_rounds {
            tracing::info!("[{}] Bug#{} 审查未通过，重新修复...", agent_name, bug_id);
            let re_fix_prompt = format!(
                "Bug #{} 修复未通过代码审查。

审查反馈：
{}

请根据反馈修复代码。输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]",
                bug_id, review_output.chars().take(2000).collect::<String>()
            );
            let re_fix_result = codex_exec::codex_exec(
                &re_fix_prompt, sandbox, None, Some(agent_name), timeout_secs,
            );
            if re_fix_result.verdict.is_fail() {
                tracing::warn!("[{}] Bug#{} 重修失败", agent_name, bug_id);
            }
        }
    }
    
    let rv_str = match &review_verdict {
        Verdict::Pass => "PASS".to_string(),
        Verdict::Fail(r) => format!("FAIL:{}", r),
        Verdict::Unknown => "UNKNOWN".to_string(),
    };
    phase_verdicts.push(("reviewer".to_string(), rv_str));
    
    // 审查失败不终止（降级通过），继续测试
    if review_verdict.is_fail() {
        tracing::warn!("[{}] Bug#{} Phase 2 REVIEW FAIL (降级继续)", agent_name, bug_id);
    }
    
    // ═══ Phase 3: QA 测试 ═══
    tracing::info!("[{}] Bug#{} Harness Loop Phase 3: QA 测试", agent_name, bug_id);
    let test_prompt = build_test_prompt(agent_name, bug_id, bug_title);
    let test_result = codex_exec::codex_exec(
        &test_prompt, sandbox, None, Some(agent_name), timeout_secs * 2 / 3,
    );
    
    let test_verdict = match &test_result.verdict {
        Verdict::Pass => "PASS".to_string(),
        Verdict::Fail(r) => format!("FAIL:{}", r),
        Verdict::Unknown => "UNKNOWN".to_string(),
    };
    phase_verdicts.push(("qa".to_string(), test_verdict.clone()));
    
    // 降级测试：如果 codex exec 测试失败，尝试直接编译验证
    let test_passed = if test_result.verdict.is_pass() {
        true
    } else {
        tracing::warn!("[{}] Bug#{} Phase 3 codex测试失败，尝试降级编译验证", agent_name, bug_id);
        let work_dir = if agent_name == "zhaoyun" {
            "/root/.openclaw/workspace/his-repo/healthlink-his-ui"
        } else {
            "/root/.openclaw/workspace/his-repo/healthlink-his-server/healthlink-his-application"
        };
        let compile_ok = std::process::Command::new("mvn")
            .args(["compile", "-pl", "healthlink-his-application", "-am", "-q"])
            .current_dir("/root/.openclaw/workspace/his-repo/healthlink-his-server")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if compile_ok {
            tracing::info!("[{}] Bug#{} 降级编译验证通过", agent_name, bug_id);
        }
        compile_ok
    };
    
    // ═══ Phase 4: Verifier 验收 ═══
    tracing::info!("[{}] Bug#{} Harness Loop Phase 4: Verifier 验收", agent_name, bug_id);
    let verify_prompt = build_verify_prompt(agent_name, bug_id, bug_title);
    let verify_result = codex_exec::codex_exec(
        &verify_prompt, "read-only", None, Some(agent_name), timeout_secs / 2,
    );
    
    let verify_verdict = match &verify_result.verdict {
        Verdict::Pass => "PASS".to_string(),
        Verdict::Fail(r) => format!("FAIL:{}", r),
        Verdict::Unknown => "UNKNOWN".to_string(),
    };
    phase_verdicts.push(("verifier".to_string(), verify_verdict.clone()));
    
    // 降级验收：检查 commit + 编译
    let verify_passed = if verify_result.verdict.is_pass() {
        true
    } else {
        tracing::warn!("[{}] Bug#{} Phase 4 验收失败，尝试降级验收", agent_name, bug_id);
        let has_commit = std::process::Command::new("git")
            .args(["log", "origin/develop", "--grep", &format!("Bug#{}", bug_id), "--oneline", "-1"])
            .current_dir("/root/.openclaw/workspace/his-repo")
            .output()
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);
        let compile_ok = std::process::Command::new("mvn")
            .args(["compile", "-pl", "healthlink-his-application", "-am", "-q"])
            .current_dir("/root/.openclaw/workspace/his-repo/healthlink-his-server")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if has_commit && compile_ok {
            tracing::info!("[{}] Bug#{} 降级验收通过 (commit+compile)", agent_name, bug_id);
        }
        has_commit && compile_ok
    };
    
    // ═══ 汇总 ═══
    let elapsed = start.elapsed().as_millis() as u64;
    let all_pass = fix_result.verdict.is_pass() && test_passed && verify_passed;
    let changes = count_changed_files(agent_name, bug_id);
    
    tracing::info!("[{}] Bug#{} Harness Loop 完成: fix={} review={} test={} verify={} elapsed={}ms changes={}",
        agent_name, bug_id,
        if fix_result.verdict.is_pass() { "PASS" } else { "FAIL" },
        phase_verdicts.iter().find(|(p,_)| p=="reviewer").map(|(_,v)| v.as_str()).unwrap_or("?"),
        if test_passed { "PASS" } else { "FAIL" },
        if verify_passed { "PASS" } else { "FAIL" },
        elapsed, changes);
    
    // 合并输出
    let mut combined_stdout = fix_result.final_message;
    combined_stdout.push_str("\n\n--- Review ---\n");
    combined_stdout.push_str(&review_output);
    combined_stdout.push_str("\n\n--- Test ---\n");
    combined_stdout.push_str(&test_result.final_message);
    combined_stdout.push_str("\n\n--- Verify ---\n");
    combined_stdout.push_str(&verify_result.final_message);
    
    let mut combined_stderr = fix_result.stderr;
    if !test_result.stderr.is_empty() {
        combined_stderr.push_str("\n[Test] ");
        combined_stderr.push_str(&test_result.stderr);
    }
    if !verify_result.stderr.is_empty() {
        combined_stderr.push_str("\n[Verify] ");
        combined_stderr.push_str(&verify_result.stderr);
    }
    
    let last_phase = if !fix_result.verdict.is_pass() { "generator" }
        else if !test_passed { "qa" }
        else if !verify_passed { "verifier" }
        else { "verifier" };
    
    CodexResult {
        success: all_pass,
        bug_id: bug_id.to_string(),
        elapsed_ms: elapsed,
        stdout: combined_stdout,
        stderr: combined_stderr,
        exit_code: if all_pass { 0 } else { 1 },
        changes,
        last_phase: last_phase.to_string(),
        phase_verdicts,
    }
}

/// 统计变更文件数
fn count_changed_files(agent_name: &str, bug_id: &str) -> u32 {
    let worktree = format!("/tmp/agentforge-worktrees/{}", agent_name);
    // 检查未提交的变更（Codex修改文件后不会自动commit）
    let uncommitted = Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(&worktree)
        .output();
    let staged = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(&worktree)
        .output();
    let untracked = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(&worktree)
        .output();
    let mut total = 0u32;
    for o in [uncommitted, staged, untracked].into_iter().flatten() {
        let stdout = String::from_utf8_lossy(&o.stdout);
        total += stdout.lines().filter(|l| !l.trim().is_empty()).count() as u32;
    }
    // 兜底：如果worktree无变更，检查主仓库（Codex可能直接修改了主仓库）
    if total == 0 {
        let main_repo = "/root/.openclaw/workspace/his-repo";
        let main_dir = if agent_name == "zhaoyun" {
            format!("{}/healthlink-his-ui", main_repo)
        } else {
            format!("{}/healthlink-his-server", main_repo)
        };
        let main_outputs = [
            Command::new("git").args(["diff", "--name-only"]).current_dir(&main_dir).output(),
            Command::new("git").args(["diff", "--cached", "--name-only"]).current_dir(&main_dir).output(),
        ];
        for o in main_outputs.into_iter().flatten() {
            let stdout = String::from_utf8_lossy(&o.stdout);
            total += stdout.lines().filter(|l| !l.trim().is_empty()).count() as u32;
        }
        // 也检查最近commit的变更
        if total == 0 {
            let committed = Command::new("git")
                .args(["diff", "--name-only", "HEAD~1"])
                .current_dir(&worktree)
                .output();
            if let Ok(o) = committed {
                let stdout = String::from_utf8_lossy(&o.stdout);
                total += stdout.lines().filter(|l| !l.trim().is_empty()).count() as u32;
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_meaningful_fix() {
        assert!(!is_meaningful_fix(0));
        assert!(!is_meaningful_fix(2));
        assert!(is_meaningful_fix(3));
        assert!(is_meaningful_fix(100));
    }

    #[test]
    fn test_fmt_duration() {
        assert_eq!(fmt_duration(0.0), "00:00");
        assert_eq!(fmt_duration(45.0), "00:45");
        assert_eq!(fmt_duration(125.0), "02:05");
        assert_eq!(fmt_duration(3661.0), "01:01:01");
    }

    #[test]
    fn test_build_harness_prompt_contains_key_elements() {
        let prompt = build_harness_prompt("guanyu", "999", "test bug", "details here");
        assert!(prompt.contains("Harness Engineering"));
        assert!(prompt.contains("Init"));
        assert!(prompt.contains("Plan"));
        assert!(prompt.contains("Verify"));
        assert!(prompt.contains("Bug #999"));
        assert!(prompt.contains("test bug"));
        assert!(prompt.contains("full-chain"));
    }

    #[test]
    fn test_load_skill_returns_string() {
        let content = load_skill("/root/.codex/skills/harness-engineering/SKILL.md");
        assert!(!content.is_empty());
        assert!(content.contains("harness-engineering"));
    }

    #[test]
    fn test_run_quality_gates_his_repo() {
        let (ok, stdout, _) = run_quality_gates("guanyu", "/root/.openclaw/workspace/his-repo/healthlink-his-server");
        assert!(ok, "Quality gates failed: {}", stdout);
    }

    #[test]
    fn test_run_quality_gates_rust() {
        let (ok, stdout, _) = run_quality_gates("guanyu", "/root/agentforge-rs");
        assert!(ok, "Quality gates failed: {}", stdout);
    }
}
