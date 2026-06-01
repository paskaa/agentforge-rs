# AgentForge-RS — Harness Engineering 总纲

> **模型决定上限，Harness 决定底线。**

## 快速命令

```bash
cargo check                        # 编译检查（<10s）
cargo test                         # 单元测试（<5s）
cargo clippy                       # 代码质量（<15s）
cargo build --release              # 发布构建
bash .harness/init.sh              # 初始化开发环境
agentforge executor --agent <name> # 启动智能体
agentforge web --port 18081        # 启动面板
agentforge pipeline --max-bugs 5   # 流水线修 Bug
```

## 项目结构

```
agentforge-rs/
├── src/core/          # Rust 后端（executor, zentao, pipeline, verification）
├── web/               # Vue 3 + Element Plus 前端
├── agents/            # 8 个智能体 YAML 配置
├── skills/            # 8 个 Harness Engineering 技能
├── codex-config/      # Codex CLI 配置模板
├── deploy/            # systemd 服务 + setup.sh
├── .harness/          # 进度、清单、初始化脚本
├── docs/harness/      # 详细文档（按需加载）
└── AGENTS.md          # 本文件 — 全局铁律
```

## 🔴 铁律（不遵守就会出事）

### 铁律 1：先分解再行动
任何非平凡任务 → 先 `update_plan`，再执行。

### 铁律 2：并行优先
独立操作必须批量调用，不要串行等待。

### 铁律 3：验证后信
每次工具调用后确认结果，不信记忆。

### 铁律 24：编译验证铁律
修改代码后必须 `cargo check` 通过才能 push。编译失败 → 禁止 push。

### 铁律 25：接口签名铁律
修改实现类时必须确认方法签名与 interface 一致。

### 铁律 26：BDT 流程（Bug-Driven Testing）
1. 获取 Bug → 2. 设计测试用例 → 3. 基线测试 → 4. 修复代码 → 5. 回归测试 → 6. 全链路验证

### 铁律 27：cherry-pick 验证
cherry-pick 后必须在 develop 上验证。

### 铁律 28：登录凭证铁律
登录凭证和路由必须从禅道获取，不硬编码。

## 🔴 测试铁律

### 铁律 A：修复后必须本地编译
修改代码后 → `cargo check` → 通过才能提交。

### 铁律 B：全链路 6 环验证
前端/页面 → Controller → Service → Mapper → DB/SQL → 关联模块。
**禁止** 只改前端就提交。

### 铁律 C：测试必须完整严肃
编译验证 + 单元测试 + Playwright 回归 + 全链路手动验证 + 关联模块回归。

### 铁律 D：已有 commit 的 bug 也必须验证
`develop 上已有 commit` ≠ `bug 已修好`。必须重新跑测试。

### 铁律 E：修复备注必须包含完整证据
根因分析 + 修复方案 + 验证结果 + 影响范围 + 6 环验证表。

## 🔴 管线铁律

### 管线流程
```
guanyu/zhaoyun(修复) → zhugeliang(分析路由) → zhangfei(Playwright测试) → huatuo(验收) + chenlin(归档)
```
每个阶段自动写入禅道备注。

### 禅道操作
- 人类提的 bug：只加备注，不改状态和分配
- 智能体提的 bug：加备注 + resolve + assign 给提出人
- Zentao API：`/api.php/v1/bugs/{id}/comment`、`/resolve`、`/assign`

## 🔴 不能碰的东西

- `config/agentforge.yaml` — 运行时配置
- `.env*` 文件
- `deploy/systemd/*.service` — 除非明确要求
- `/tmp/agentforge-worktrees/` — 智能体工作树

## ⚙️ 环境

| 组件 | 地址 |
|---|---|
| Redis | `127.0.0.1:16379` |
| 禅道 | `zentao.gentronhealth.com` |
| HIS 前端 | `:81` (vite dev) |
| HIS 后端 | `:18082` (spring boot) |
| AgentForge 面板 | `:18081` |
| 时区 | `Asia/Shanghai` |

## 详细文档

- 管线流程详解 → `docs/harness/pipeline.md`
- 智能体配置 → `docs/harness/agents.md`
- L4/L5 分析 → `docs/harness/analytics.md`
- 5 子系统模型 → `docs/harness/five-subsystems.md`
- 版本历史 → `docs/harness/changelog.md`
