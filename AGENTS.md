# AgentForge-RS — Harness Engineering 操作系统

> **模型决定上限，Harness 决定底线。**
> Multi-agent bug fixing framework — Rust rewrite. 8 agents, Zentao integration, Feishu notifications.
> 本文件是本项目的 Harness Engineering 总纲，整合 OpenAI Harness Engineering + WalkingLabs 5 子系统模型。

---

## 📋 快速导航

| 我要做什么 | 怎么做 |
|---|---|
| 启动开发 | `bash .harness/init.sh` |
| 扫描所有 Bug | `cargo run -- scan-bugs` |
| 流水线修 Bug | `cargo run -- pipeline --max-bugs 5` |
| 查询 Bug | `cargo run -- query-bug --bug-id NNN` |
| 编译检查 | `cargo check` |
| 运行测试 | `cargo test` |
| 代码质量 | `cargo clippy` |
| 启动 Agent | `agentforge executor --agent <name>` |
| L4 分析 | `agentforge analytics` / `agentforge report` |
| L5 优化 | `agentforge optimize` / `agentforge scores` |
| 查看进度 | `cat .harness/PROGRESS.md` |
| 更新清单 | `vim .harness/feature_list.json` |

**工作区项目一览：**
- his-repo (OpenHIS): `/root/.openclaw/workspace/his-repo/`
- agentforge-rs: `/root/agentforge-rs/` ← **当前项目**
- hermes-agent: `/root/hermes-agent/`
- 所有 agent 工作树: `/tmp/agentforge-worktrees/<agent_name>/`

**项目目录结构：**
- `agents/` — 8 个智能体 YAML 配置（角色/约束/门禁/工作目录）
- `skills/` — 8 个 Harness Engineering 技能文件
- `codex-config/` — Codex CLI 配置模板
- `deploy/` — systemd 服务模板 + 一键部署脚本
- `config/` — 运行时配置（git 忽略）

---

## ⚙️ WalkingLabs 5 子系统模型

### 1. 指令子系统 (Instruction)

| 文件 | 用途 |
|---|---|
| `AGENTS.md`（本文件） | 项目铁律、约束、标准工作流 |
| `.harness/PROGRESS.md` | 会话进度 + 已验证状态（跨 session） |
| `.harness/feature_list.json` | 功能状态唯一事实来源 |
| `.harness/init.sh` | 统一启动入口 |
| `.harness/clean-state-checklist.md` | 结束时的清洁检查 |
| `.harness/evaluator-rubric.md` | 评审评分表 |

### 2. 工具子系统 (Tools)

| 层级 | 工具 | 用途 |
|---|---|---|
| L0 开发 | `cargo build/check/test/clippy` | 编译、测试、质量 |
| L1 Agent | `cargo run -- executor --agent <name>` | 启动 Agent 主循环 |
| L2 Pipeline | `cargo run -- pipeline` | 流水线批量修 Bug |
| L3 集成 | `zentao-write-bug.sh` | 禅道操作（解决/关闭/备注） |
| L4 辅助 | `zentao-bug-query.sh` | 查询 Bug 详情 |
| L5 入口 | `codex-aliyun` → `mimo2codex` → `codex` | 修复管道 |

### 3. 环境子系统 (Environment)

| 组件 | 配置来源 |
|---|---|
| Redis | `config/agentforge.yaml` → `redis://127.0.0.1:16379` |
| Zentao | `config/agentforge.yaml` → `zentao.gentronhealth.com` |
| 飞书 | `config/agentforge.yaml` → app_id + app_secret |
| LLM | `config/agentforge.yaml` → api_base + api_key |
| Git | his-repo: `http://guanyu:GentronHIS2025@192.168.110.253:3000/wangyizhe/his.git` |

### 4. 状态子系统 (State)

| 机制 | 用途 | 持久化 |
|---|---|---|
| `TraceStore` (SQLite) | Agent 活动追踪 | `/var/lib/agentforge/traces.db` |
| `fix_trajectory` | 修复轨迹记录 | Redis Hash |
| `dead_letter` | 失败任务持久化 | Redis List |
| `pipeline:result:{bug_id}` | Pipeline 修复结果 | Redis (24h TTL) |
| `claude_code_lock:{agent}` | Agent 互斥锁 | Redis (1h TTL) |
| `.harness/PROGRESS.md` | 跨会话进度 | 文件系统 |
| `feature_list.json` | 功能状态 | 文件系统 |
| `git log` | 变更历史 | Git |

### 5. 反馈子系统 (Feedback)

| 层级 | 速度 | 命令 | 失败处理 |
|---|---|---|---|
| L1 编译检查 | <10 秒 | `cargo check` | 立即阻断，Agent 自行修复 |
| L1 单元测试 | <5 秒 | `cargo test` | 失败回退，重试 |
| L2 代码质量 | <15 秒 | `cargo clippy` | 警告可忽略，错误阻断 |
| L3 质量门禁 | <30 秒 | `run_quality_gates()` | 编译验证通过才提交 |
| L4 人工审查 | 5-10 分钟 | diff review | 驳回 / 指导 / 批准 |

---

## 🔄 标准工作循环 (Init → Plan → Implement → Verify → Cleanup)

```
收到任务
  │
  ├─→ 1️⃣ Init（初始化）
  │   ├── pwd 确认目录
  │   ├── bash .harness/init.sh
  │   ├── cat .harness/PROGRESS.md
  │   ├── cat .harness/feature_list.json
  │   └── git log --oneline -5
  │
  ├─→ 2️⃣ Plan（全链路分析）
  │   ├── 全链路 6 环：录入 → 保存 → 查询 → 修改 → 删除 → 关联
  │   ├── rg/grep 搜索相关代码
  │   ├── git blame 追溯历史
  │   └── update_plan 确定步骤
  │
  ├─→ 3️⃣ Implement（约束内修改）
  │   ├── 一次只修一个 Bug，只动必要文件
  │   ├── 遵守 AGENT_ROLES 特定约束
  │   ├── 数据库字段变更走通 6 环
  │   └── 不改实体结构（优先用 jsonb）
  │
  ├─→ 4️⃣ Verify（验证）
  │   ├── cargo check / mvn compile / npm lint
  │   ├── cargo test / pytest
  │   ├── cargo clippy
  │   └── 有可运行证据才标记完成
  │
  └─→ 5️⃣ Cleanup（提交 + 解决）
      ├── auto_commit_fix → git push origin <branch>
      ├── resolve_bug_in_zentao（结构化备注）
      ├── 更新 .harness/PROGRESS.md
      ├── 更新 .harness/feature_list.json
      ├── 运行 clean-state-checklist.md
      └── 留下干净重启路径
```

---

## 🔗 全链路修复原则（6 环）

涉及**数据库字段**的 Bug，必须走通全部 6 环：

```
前端/页面 ─→ Controller ─→ Service ─→ Mapper/DAO ─→ DB/SQL ─→ 关联模块
   ①录入      ②验证      ③业务      ④持久化      ⑤存储     ⑥联动
```

**每个环的检查清单：**

| 环 | 检查项 | 常见遗漏 |
|---|---|---|
| ① 前端 | 表单字段、校验规则、回显、类型 | 弹窗/抽屉表单字段缺失 |
| ② Controller | DTO 接收、参数验证、API 路径 | `@RequestBody` 缺字段 |
| ③ Service | 业务逻辑、事务、状态机 | 状态流转忘记更新 |
| ④ Mapper | XML 映射、参数绑定、ResultMap | `#{xxx}` 缺参数、WHERE 条件不全 |
| ⑤ DB | 列定义、约束、索引 | 新字段没有 COMMENT |
| ⑥ 关联 | 联动查询、级联更新、依赖模块 | 汇总/统计查询未同步 |

**铁律：** 不走过 6 环，不允许提交代码。

---

## 🧠 Agent 架构

| Agent | 角色 | 专长 | 工作目录 | 质量门禁 |
|---|---|---|---|---|
| zhugeliang | 架构师/协调者 | 系统架构、全链路分析 | backend | `mvn compile` |
| liubei | 项目经理 | 进度跟踪、需求管理 | backend | 跳过 |
| guanyu | 后端工程师 | Java/Spring/MyBatis/API | backend | `mvn compile` |
| zhaoyun | 前端工程师 | Vue3/ElementUI/TS/组件 | frontend | `npm lint` |
| xunyu | DBA | SQL/PostgreSQL/DDL/索引 | `sql/` | `mvn compile` |
| zhangfei | QA 测试 | Playwright/E2E/边界用例 | backend | `mvn compile` |
| huatuo | 产品验收 | 业务验证、用户场景 | backend | 跳过 |
| chenlin | 文档 | Markdown/API 文档/README | backend | 跳过 |

### 多 Agent 的意义

所有 8 个 Agent 虽然接同一后端模型，但差异化体现在：
1. **Prompt 定制** — 每个 Agent 有独立角色描述、专长领域和约束
2. **工作目录隔离** — 前端 Agent 在 `openhis-ui-vue3`，DBA 在 `sql/`
3. **质量门禁差异化** — 前端跑 `npm lint`，后端跑 `mvn compile`
4. **路由策略** — 根据 Bug 标题关键词自动路由到最匹配 Agent
5. **协作管线** — Fix → Test → Verify → Archive，多 Agent 接力

---

## 📐 约束系统四层模型

| 层级 | 内容 | 落地方式 |
|---|---|---|
| **L1 架构约束** | 接口合约、包结构、命名规范 | `agent_constraints()` 函数 + AGENT_ROLES |
| **L2 代码质量** | 圈复杂度、类型提示、错误处理 | 编译门禁 + `cargo clippy` |
| **L3 安全约束** | 敏感信息、权限检查、输入验证 | 禁止硬编码 + Vault 凭证 |
| **L4 业务规则** | 领域逻辑、数据一致性、事务 | 全链路 6 环验证 |

**优先级：** 安全 > 架构 > 质量 > 业务

---

## 🔧 Harness Engineering 四大支柱

### 支柱 1: 持久化执行 (Durable Execution)

```
Agent 执行步骤 → Checkpoint 保存 → 失败 → 从 Checkpoint 恢复 → 继续
```

**本项目的 Checkpoint 机制：**
- **Agent 锁**: `claude_code_lock:{agent}` — Redis EX 3600 秒，防止并发
- **Pipeline 结果**: `pipeline:result:{bug_id}` — Redis 24h TTL
- **Trace 日志**: `TraceStore` (SQLite) — 每次 fix_start/fix_done 记入
- **重试计数**: `pipeline_retry:{bug_id}` — 最多 3 次重试

### 支柱 2: 闭环测试 (Closed-Loop Testing)

```
修复 → 编译检查 → 单元测试 → 质量门禁 → 提交
                                         ↓ 失败 → 退回重试
```

**本项目测试层级：**
- L1: `cargo check` / `mvn compile` / `npm lint`
- L2: `cargo test` / pytest / Playwright
- L3: `run_quality_gates()` — 强制验证
- L4: diff review + zentao resolve

### 支柱 3: 架构约束 (Architectural Constraints)

见上方「约束系统四层模型」。每个 Agent 通过 `agent_constraints()` 函数获取特定约束。

### 支柱 4: 运行时策略 (Runtime Policies)

```
资源限制 → 安全检查 → 审批流 → 审计日志
```

**本项目策略：**
- `claude_code_lock` — 每 Agent 同时只修一个 Bug
- `pipeline_retry` — 同 Bug 最多重试 3 次
- `pipeline_sent` — 24h 内不重复触发下游
- `dead_letter` — 失败任务持久化防丢

---

## 📊 Pipeline 管线工作流

```
cargo run -- pipeline --max-bugs 5
  │
  ├─→ 扫描所有 Agent 的活跃 Bug
  │
  ├─→ 对每个 Bug（串行）:
  │   ├── 路由到最匹配的 Fixer
  │   ├── 推送到 Redis 队列 `agent-work-queue:fix:{fixer}`
  │   └── 轮询 `pipeline:result:{bug_id}`（最长等 30 分钟）
  │
  ├─→ Fixer 消费队列:
  │   ├── build_harness_prompt() — 加载 8 个技能
  │   ├── codex-aliyun → mimo2codex → codex
  │   ├── run_quality_gates()
  │   ├── auto_commit_fix() — commit + push
  │   └── resolve_bug_in_zentao() — 结构化备注
  │
  ├─→ zhangfei 测试:
  │   ├── npx playwright test --grep @bug{id}
  │   └── 通过 → huatuo + chenlin
  │   └── 失败 → 退回 fixer 重修（最多 3 次）
  │
  ├─→ huatuo 验收:
  │   └── zentao-write-bug.sh resolve + assign
  │
  └─→ chenlin 归档
```

---

## 🔗 铁律

1. **先分解再行动** — 任何非平凡任务先出 `update_plan`
2. **全链路必走 6 环** — 涉及数据库字段不跳过任何一环
3. **一次只修一个 Bug** — 不扩大范围
4. **验证通过才提交** — 质量门禁不跳过
5. **提交必推远程** — commit + push 不能只做一半
6. **注释必结构化** — `fix(#N): title\n\n根因：\n- ...\n\n修复：\n- ...`
7. **所有配置走 Config** — 禁止裸写环境变量
8. **用户数据不做 shell 拼接** — 用 `Command::new("bash").arg(script).args(args)`
9. **ACK 用 Redis stream ID（`_redis_id`）** — 不是飞书消息 ID
10. **AGENTS.md 是地图** — 详细规则放 .harness/ 按需加载
11. **交互型 BUG 必查两端** — 涉及退回/审核/签发等操作流程的 BUG：
    - 📤 发起方检查：入口→校验→API→Service→DB
    - 📥 接收方检查：DB→Service→API→展示字段→页面列
    - 两端各跑一次 6 环，用表格对比修复状态
12. **Entity 字段变更必配迁移脚本** — 新增 Entity 字段必须同时创建 
13. **修前必查禅道状态** — Pipeline 扫描时，`status == resolved || status == closed` 的 bug **禁止入列**。已关闭的 bug 不需要修复。
14. **修前必查 develop** — 入列前检查 `git log origin/develop --grep="Bug#N"` 是否已有修复 commit。已有则跳过，不重复入列。
15. **修后必关禅道** — 修复成功后必须调用 `resolve_bug_in_zentao()` 关闭 bug 并写入结构化备注。不关禅道 = 浪费后续所有 agent 的处理时间。
16. **测试环境必须就绪** — zhangfei 运行 Playwright 前必须确认：(1) node_modules 已安装 (2) dev server 已启动 (3) 目标 URL 可达。环境不就绪则跳过测试，不记录 test_done。
17. **修复失败必须退出** — 同一个 bug 被标记为"之前已修复，无需改动"后，必须退出并关闭禅道，不允许反复重试同一个已修复的 bug。

---

## ⚠️ 修复质量铁律（零容忍）

### 🔴 铁律 A：修前必须完整获取 Bug 全部信息

**任何 bug 修复前，必须获取并阅读禅道上的全部信息，不允许遗漏：**

1. **Bug 标题和描述** — 理解问题本质，不能只看标题
2. **复现步骤** — 逐条阅读，理解操作路径
3. **期望结果 vs 实际结果** — 明确修复目标
4. **截图/附件** — 必须下载并查看所有图片附件，图片中可能包含关键的错误信息、页面截图、数据库状态等文字无法描述的内容
5. **备注/评论历史** — 必须阅读所有后续备注，可能包含补充信息、复现条件、环境说明
6. **严重程度和优先级** — 决定修复策略
7. **指派人和报告人** — 了解上下文
8. **关联模块** — 检查是否有关联的 bug 或需求

**获取方式：**
- 优先使用 Zentao API v2 `GET /api.php/v1/bugs/{id}` 获取完整字段
- 附件/图片通过 `GET /api.php/v1/files/{fileID}` 下载
- 备注通过 `GET /api.php/v1/bugs/{id}/comments` 获取

**禁止：** 只看标题就开始写代码。

### 🔴 铁律 B：修复必须走全链路 6 环验证

**每个涉及数据的 bug，修复后必须验证全部 6 个环节：**

```
① 前端/页面 → ② Controller → ③ Service → ④ Mapper → ⑤ DB/SQL → ⑥ 关联模块
```

| 环节 | 验证方式 | 不通过则 |
|------|---------|---------|
| ① 前端页面 | Playwright 自动化测试 / 手动检查 | 阻断 |
| ② Controller | curl/API 调用验证请求参数和响应 | 阻断 |
| ③ Service | 单元测试 / 编译验证 | 阻断 |
| ④ Mapper | SQL 执行验证 / EXPLAIN 分析 | 阻断 |
| ⑤ DB 数据 | 直接查询数据库确认数据正确 | 阻断 |
| ⑥ 关联模块 | 检查上下游模块是否受影响 | 阻断 |

**禁止：** 只改了前端就提交，不验证后端和数据库。

### 🔴 铁律 C：测试必须完整严肃

**测试不是走过场，必须严肃对待：**

1. **编译验证** — `mvn compile`（后端）/ `vue-tsc --noEmit`（前端）必须 0 error 通过
2. **单元测试** — `mvn test` / `npm run test:run` 必须全部通过
3. **Playwright 回归测试** — 必须运行相关的 `@bug{id}` 标签测试
4. **全链路手动验证** — 按照禅道的复现步骤，在 dev 环境实际操作一遍
5. **关联模块回归** — 检查修改是否影响了其他功能

**测试环境就绪检查清单：**
- [ ] `node_modules/` 已安装
- [ ] dev server 已启动且可访问
- [ ] 数据库连接正常
- [ ] 测试账号可登录

**禁止：** 测试环境没准备好就记录测试结果。

### 🔴 铁律 D：已有 commit 的 bug 也必须验证

**"develop 上已有该 bug 的 commit" 不等于"bug 已修好"：**

1. **检查 commit 内容** — `git show <commit>` 看实际改了什么
2. **验证修复是否完整** — commit 可能只修了一部分，或者修错了地方
3. **重新运行测试** — 之前的测试可能因为环境问题没跑通
4. **检查禅道备注** — 看是否有遗漏的复现步骤或补充信息
5. **验证数据一致性** — 前端改了，数据库是否也需要改

**只有通过完整 6 环验证 + Playwright 测试的 bug，才能标记为"已修复"。**

### 🔴 铁律 E：修复备注必须包含完整证据

**commit message 和禅道备注必须包含：**

1. **根因分析** — 具体到哪个文件、哪个函数、哪一行
2. **修复方案** — 改了什么、为什么这么改
3. **验证结果** — 编译通过截图/日志、测试通过截图/日志
4. **影响范围** — 是否影响其他模块
5. **全链路 6 环验证表** — 每个环节的验证结果

**禁止：** 空洞的 commit message 如"修复了bug"、"修改相关代码"。

---

## 📊 L4 量化分析

### 数据来源
- **TraceStore** (SQLite): `/var/lib/agentforge/traces.db`
  - agent_id, event, task_id, duration_ms, status
- **FixTrajectory** (JSON): `/var/lib/agentforge/trajectories/`
  - bug_id, agent, success, elapsed_s, fix_summary
- **DeadLetter** (Redis): `agentforge:dead_letter`
  - 失败任务持久化

### 分析指标
| 指标 | 来源 | 用途 |
|---|---|---|
| Agent 成功率 | TraceStore `fix_done` 事件 | 评估智能体表现 |
| 平均修复耗时 | TraceStore `duration_ms` | 识别慢修复 |
| 失败模式分布 | TraceStore `status=failed` | 针对性优化 |
| Pipeline 吞吐量 | TraceStore 全量事件 | 监控整体健康度 |

### CLI 命令
```bash
agentforge analytics          # JSON 指标输出
agentforge report             # Markdown 分析报告
```

---

## 🧠 L5 AI 自主优化

### 优化机制
| 机制 | 触发条件 | 动作 |
|---|---|---|
| 约束增强 | Agent 成功率 < 50%（≥3次修复） | 自动补充专项约束 |
| 智能路由 | 按 bug 类型匹配历史最优 Agent | `best_agent_for(bug_type)` |
| 重试策略 | 失败后换提示词/换 Agent 重试 | 最多 3 次 |
| 路由调整 | 某 Agent 成功率最低（< 40%） | 减少分配，转移给最优 Agent |

### 评分系统
- **评分维度**: 成功率(60%) + 速度(20%) + 类型匹配(20%)
- **持久化**: `/var/lib/agentforge/agent_scores.json`
- **更新时机**: 每次 fix_done 事件后

### CLI 命令
```bash
agentforge optimize           # 分析 + 优化建议
agentforge scores             # 智能体评分排名
```

---

## 📈 成熟度追踪

| 等级 | 特征 | 本项目 |
|---|---|---|
| L1 初始 | 无规范 | ✅ 已超越 |
| L2 管理 | 基础约束 + 反馈 | ✅ 已完成 |
| L3 定义 | 标准化流程 + 自动提交 + 禅道闭环 | ✅ 已完成 |
| L4 量化 | 数据驱动优化 | ✅ **已达成** |
| L5 优化 | AI 自主优化 | ✅ **已达成** |

---

## 📝 版本记录

| 日期 | 版本 | 变更 |
|---|---|---|
| 2026-05-28 | v1.0 | 初始版本 — 5 子系统模型 |
| 2026-05-28 | v2.0 | 完整 Harness Engineering 方法论 + 全链路 6 环 + Pipeline 管线 |
| 2026-05-31 | v3.0 | L4 量化分析 + L5 AI 自主优化 + 脱敏 + 多语言 README |
| 2026-05-31 | v3.1 | 8 智能体独立配置文件 + 部署脚本 + Skills + Codex 配置 |

> **总纲：** 一次一个功能，全链路 6 环，编译通过再提交，提交必修远程，修完必写备注。
