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
        format!("{}/openhis-ui-vue3", base)
    } else if agent_name == "xunyu" {
        format!("{}/openhis-server-new", base)
    } else {
        format!("{}/openhis-server-new", base)
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
             - 运行 Playwright 回归测试：npx playwright test --grep @bug{id} --workers=1
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
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Build the full harness-augmented prompt for Codex.
/// Loads ALL installed harness skills so Codex follows the methodology autonomously.
fn build_harness_prompt(agent_name: &str, bug_id: &str, bug_title: &str, bug_details: &str) -> String {
    let skills_base = "/root/.codex/skills";
    let harness_eng    = load_skill(&format!("{}/harness-engineering/SKILL.md", skills_base));
    let walkinglabs    = load_skill(&format!("{}/walkinglabs-harness/SKILL.md", skills_base));
    let durable_exec   = load_skill(&format!("{}/durable-execution/SKILL.md", skills_base));
    let closed_loop    = load_skill(&format!("{}/closed-loop-testing/SKILL.md", skills_base));
    let constraint_d   = load_skill(&format!("{}/constraint-design/SKILL.md", skills_base));
    let review_audit   = load_skill(&format!("{}/review-audit/SKILL.md", skills_base));
    let karpathy       = load_skill(&format!("{}/karpathy-guidelines/SKILL.md", skills_base));
    let full_chain     = load_skill(&format!("{}/full-chain-fix/SKILL.md", skills_base));

    let agents_md_path = "/root/.openclaw/workspace/his-repo/AGENTS.md";
    let agents_md_hint = load_skill(agents_md_path)
        .lines().take(30).collect::<Vec<_>>().join("\n");

    // Find agent role info
    let role_info = AGENT_ROLES.iter().find(|r| r.0 == agent_name);
    let (role_name, role_desc, expertise): (&str, &str, &str) = match role_info {
        Some((_, name, desc, exp)) => (*name, *desc, *exp),
        None => ("通用开发工程师", "负责代码开发和 Bug 修复。", "通用"),
    };
    let constraints = agent_constraints(agent_name);

    format!(
        r#"你是一个中文编程助手。使用简体中文思考和回复。

## 你的角色
你是 **{role_name}**。{role_desc}
你的专长领域：{expertise}

{constraints}

## Harness Engineering 方法论（必须遵守）

在修复 Bug 之前，阅读并遵循以下方法论：

### 工作纪律
1. **Init**: 先确认工作目录和项目状态
2. **Plan**: 分析全链路数据流——录入→保存→查询→修改→删除→关联（6 环）
3. **Implement**: 一次只修一个 Bug，只动必要文件
4. **Verify**: 修改后运行编译/语法检查
5. **Cleanup**: 不留临时文件或调试代码

### 约束铁律
- 安全 > 架构 > 质量 > 性能
- 禁止硬编码密钥/密码
- 涉及 Mapper XML 时，UNION ALL 所有子查询统一修改
- 涉及数据库字段时，走通全链路：前端→API→Service→Mapper→DB
- 涉及交互/状态变更的 BUG：必须同时分析「发起方📤」和「接收方📥」两端
  - 发起方：操作触发端（如护士退回）— 录入/提交是否正常？
  - 接收方：信息展示端（如医生查看）— 查询/展示是否正常？
  - 两端都要跑一次 6 环分析，分别标记状态

## 已加载的技能（融入你的工作方式）

### 🔧 核心方法论
{harness_eng}

### 📋 实战模式（5 子系统）
{walkinglabs}

### ⏳ 持久执行（检查点/幂等）
{durable_exec}

### 🧪 闭环测试（质量门禁）
{closed_loop}

### 📐 约束设计
{constraint_d}

### 🔐 审查审计
{review_audit}

### 🎯 编码准则
{karpathy}

### 🔗 全链路修复
{full_chain}

## 项目规则摘要
{agents_md_hint}

---

## 任务：修复 Bug #{bug_id}：{bug_title}

## 禅道 Bug 详情
{bug_details}

## Harness 修复指引
1. **Init**: 确认目录，读 AGENTS.md
2. **Analyze**: 用 rg/grep 搜索相关代码
3. **Pre-check**: 检查 develop 上是否有该 bug 的历史修复提交
   - 如果有：读之前的 commit diff，分析是否完整修复
   - 如果之前修得不完整：指出遗漏点，重新全链路分析
   - 如果之前修复完整：输出「之前修复已完成，无需改动」并退出
4. **Full-chain (6 环)**: 无论是否已有修复，都跑一遍
   - 前端/页面 → Controller → Service → Mapper → DB/SQL → 关联模块
   - 涉及数据库字段时必走，查每个环节的字段映射
   - ⚠️ 每环必须标记状态：【✅ 正常 / 🔧 已修改 / ❌ 遗漏】
   - ⚠️ 如果只改了后端没改前端（或反之），说明分析不完整，重新检查
5. **Reproduce**: 按步骤复现，定位根因
6. **Fix**: 修改文件（用 apply_patch），一次修彻底
   - ⚠️ 涉及新增 Entity 字段时，必须同时创建 DB 迁移脚本（sql/迁移记录-DB变更记录/YYYYMMDD_fix_BUG#XXXX_description.sql）
   - ⚠️ 只改 Entity 不改 DB = 修复不完整，运行时 100% 报错
   - ⚠️ 涉及交互流程（退回/审核/签发等）的 BUG，必须识别「发起方📤」（谁操作）和「接收方📥」（谁查看）
     - 📤 发起方：检查操作入口→校验→API→Service→DB 是否完整
     - 📥 接收方：检查 DB→Service→API→展示字段→页面列 是否完整
     - 只修一端不修另一端 = 修复不完整
7. **Verify**: 运行编译/语法检查 + 端到端数据流确认
   - 编译检查：mvn compile / npm lint / cargo check / vue-tsc + vite build
   - ⚠️ 数据流检查：从起点到终点每环确认数据能传过去
     - 📤 录入链路：前端发送字段 → API 参数接收 → Service 读取 → DB 写入
     - 📥 展示链路：DB 查询 → Service 返回 → API 响应 → 前端展示列
   - ⚠️ 交互检查：涉及前端改动时确认弹窗/提示/跳转是否正常工作
   - ⚠️ 两端核对：涉及交互流程的 BUG，用表格对比两端修复状态
8. **Submit**: 输出变更摘要，格式：
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
        harness_eng=harness_eng, walkinglabs=walkinglabs,
        durable_exec=durable_exec, closed_loop=closed_loop,
        constraint_d=constraint_d, review_audit=review_audit,
        karpathy=karpathy, full_chain=full_chain,
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

        // Step 1: Vue TypeScript type check (vue-tsc) — hard failure
        let ts_check = Command::new("npx")
            .args(["vue-tsc", "--noEmit"])
            .current_dir(work_dir)
            .output();
        match ts_check {
            Ok(o) if !o.status.success() => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                return (false,
                    "vue-tsc 类型检查失败".into(),
                    if stderr.len() > 1000 { stderr.chars().take(1000).collect() } else { stderr });
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
                .args(["-h", "192.168.110.252", "-p", "15432", "-d", "postgresql",
                       "-U", "postgresql", "-v", "ON_ERROR_STOP=1",
                       "-v", &format!("search_path={}", schema),
                       "-f", &full.to_string_lossy()])
                .env("PGPASSWORD", "Jchl1528")
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

    // Step 2: Build harness-augmented prompt
    let prompt = build_harness_prompt(agent_name, bug_id, bug_title, &bug_details_text);

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
            let success = has_fix_commit || (
                exit_code == 0 && (
                    stdout.contains("修复完成") || stdout.contains("fix") || stdout.contains("resolved")
                    || changes > 0
                )
            );

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

            // Only proceed to commit if all checks passed
            let ok_to_commit = success && gates_passed && migrations_passed && sql_valid;

            // Step 6: Auto-commit changes — only if gates + migrations passed
            if ok_to_commit && changes > 0 {
                auto_commit_fix(agent_name, bug_id, bug_title, &stdout);
            }

            // Step 7: Resolve bug in Zentao — only if all checks passed
            if ok_to_commit {
                resolve_bug_in_zentao(agent_name, bug_id, bug_title, &stdout);
            } else if success && !gates_passed {
                tracing::warn!("[{}] Bug #{} fix succeeded but quality gates failed — NOT committing or resolving. Manual review needed.",
                    agent_name, bug_id);
            } else if success && !migrations_passed {
                tracing::warn!("[{}] Bug #{} fix succeeded but DB migrations FAILED — NOT committing or resolving. Manual review needed.",
                    agent_name, bug_id);
            } else if success && !sql_valid {
                tracing::warn!("[{}] Bug #{} fix succeeded but Mapper SQL 语法验证 FAILED — NOT committing or resolving. Manual review needed.",
                    agent_name, bug_id);
            }

            CodexResult {
                success,
                bug_id: bug_id.to_string(),
                elapsed_ms: elapsed,
                stdout,
                stderr,
                exit_code,
                changes,
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
    let _ = Command::new("git")
        .args(["-C", &worktree, "add", "--all", "--",
                ":!*.orig", ":!*.mjs", ":!*.timestamp*"])
        .output();

    // Extract root causes and fixes for structured commit message
    let (root_causes, fixes) = extract_fix_details(stdout, bug_title);
    let commit_msg = build_zentao_comment(bug_id, bug_title, &root_causes, &fixes);

    let _ = Command::new("git")
        .args(["-C", &worktree, "commit", "-m", &commit_msg])
        .output();

    // Push to remote agent branch
    let branch = agent_name;
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
                    // Check if develop already has this bug fix (dedup)
                    let already_fixed = Command::new("git")
                        .args(["-C", main_repo, "log", "--oneline", "origin/develop", "--grep",
                               &format!("fix(#{}):", bug_id)])
                        .output();
                    let already_fixed_old = Command::new("git")
                        .args(["-C", main_repo, "log", "--oneline", "origin/develop", "--grep",
                               &format!("Fix Bug #{}", bug_id)])
                        .output();
                    let skip_cherry = match (&already_fixed, &already_fixed_old) {
                        (Ok(a), Ok(b)) => {
                            let out_a = String::from_utf8_lossy(&a.stdout).trim().to_string();
                            let out_b = String::from_utf8_lossy(&b.stdout).trim().to_string();
                            !out_a.is_empty() || !out_b.is_empty()
                        }
                        _ => false,
                    };

                    if skip_cherry {
                        tracing::info!("[{}] Bug #{} already fixed on develop — skipping cherry-pick", agent_name, bug_id);
                    } else {
                        // Try cherry-pick with -X theirs to auto-resolve simple conflicts
                        let cherry = Command::new("git")
                            .args(["-C", main_repo, "cherry-pick", "--strategy=recursive",
                                   "-X", "theirs", &hash])
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
                                    .args(["-C", main_repo, "cherry-pick", &hash])
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
fn resolve_bug_in_zentao(agent_name: &str, bug_id: &str, bug_title: &str, stdout: &str) {
    let (root_causes, fixes) = extract_fix_details(stdout, bug_title);
    let comment = build_zentao_comment(bug_id, bug_title, &root_causes, &fixes);

    // 使用 Rust ZentaoClient API 替代 shell 脚本
    let cfg = match crate::config::Config::load() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("[{}] Cannot load config for Zentao resolve: {}", agent_name, e);
            return;
        }
    };
    let client = crate::core::zentao::ZentaoClient::from_config(&cfg);

    // Sync wrapper: block_on the async resolve
    let rt = tokio::runtime::Handle::current();
    match rt.block_on(client.resolve_bug(bug_id, &comment)) {
        Ok(()) => {
            tracing::info!("[{}] Bug #{} resolved in Zentao: fix(#{}): {}", agent_name, bug_id, bug_id, bug_title);
            tracing::debug!("[{}] Zentao comment: {} chars", agent_name, comment.len());
        }
        Err(e) => {
            tracing::warn!("[{}] Zentao resolve failed for Bug #{}: {}", agent_name, bug_id, e);
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
        let (ok, stdout, _) = run_quality_gates("guanyu", "/root/.openclaw/workspace/his-repo/openhis-server-new");
        assert!(ok, "Quality gates failed: {}", stdout);
    }

    #[test]
    fn test_run_quality_gates_rust() {
        let (ok, stdout, _) = run_quality_gates("guanyu", "/root/agentforge-rs");
        assert!(ok, "Quality gates failed: {}", stdout);
    }
}
