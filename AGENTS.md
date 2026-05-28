# AgentForge-RS — Harness Engineering 开发指南

> **模型决定上限，Harness 决定底线。**
> Multi-agent bug fixing framework — Rust rewrite. 8 agents, Zentao integration, Feishu notifications.

---

## 📋 项目信息

### 构建和运行

```bash
cd /root/agentforge-rs

# 编译
cargo build

# 运行
cargo run -- check                                    # 配置检查
cargo run -- scan-bugs                                # 扫描所有 Bug
cargo run -- query-bug --bug-id 597                   # 查询单个 Bug
cargo run -- fix-bug --bug-id 597 --fixer zhaoyun     # 提交修复任务

# 启动 Agent Executor（8 个 agent）
cargo run -- executor --agent zhaoyun
cargo run -- ws --agent zhugeliang                    # WebSocket 监听

# 测试
cargo test
cargo clippy
```

### 关键路径

```
src/                    Rust 源码
├── main.rs             CLI 入口（12 个子命令）
├── lib.rs              模块声明
├── config/mod.rs       YAML/ENV 配置加载
├── core/
│   ├── executor.rs     Agent 主循环（640 行）
│   ├── coordinator.rs   Bug 扫描和路由
│   ├── pipeline.rs      修复管线逻辑
│   ├── subagent.rs     Claude Code / Codex 调用
│   ├── llm.rs          LLM 客户端（Bailian/DeepSeek）
│   ├── trace.rs        SQLite 追踪存储
│   ├── dead_letter.rs  死信队列（Redis）
│   ├── fix_trajectory.rs 修复轨迹记录
│   ├── quota_monitor.rs 限流检测
│   └── tool_executor.rs 脚本执行器
├── network/
│   ├── feishu.rs       飞书 API 客户端
│   └── ws_listener.rs  WebSocket 监听
└── tools/mod.rs        调度器（日报/健康检查）
config/agentforge.yaml   主配置文件
```

---

## ⚙️ 5 子系统模型

### 1. 指令子系统

| 文件 | 用途 |
|---|---|
| `AGENTS.md`（本文件） | 项目规则、约束、工作流程 |
| `.harness/PROGRESS.md` | 会话进度 |
| `.harness/feature_list.json` | 功能状态 |
| `.harness/STANDARD_OPERATING_PROCEDURE.md` | 标准作业流程 |

### 2. 工具子系统

| 工具 | 用途 |
|---|---|
| `cargo build / check / test` | 编译和测试 |
| `cargo run -- <subcommand>` | 运行各个子命令 |
| `cargo clippy` | 代码质量检查 |
| `git` | 版本控制 + 回滚 |

### 3. 环境子系统

| 组件 | 路径 |
|---|---|
| Rust | `Cargo.toml` 锁定 edition 2021 |
| Redis | `config/agentforge.yaml` → redis://127.0.0.1:16379 |
| Zentao | `config/agentforge.yaml` → zentao.gentronhealth.com |
| Feishu | `config/agentforge.yaml` → app_id + app_secret |

### 4. 状态子系统

| 机制 | 用途 |
|---|---|
| `TraceStore` (SQLite) | Agent 活动追踪 |
| `fix_trajectory` (JSON) | 修复轨迹记录 |
| `dead_letter` (Redis) | 失败任务持久化 |
| `.harness/PROGRESS.md` | 跨会话进度 |
| `git log` | 变更历史 |

### 5. 反馈子系统

| 层级 | 命令 | 时间 |
|---|---|---|
| L1 编译 | `cargo check` | <10 秒 |
| L1 测试 | `cargo test` | <5 秒 |
| L2 代码质量 | `cargo clippy` | <15 秒 |
| L3 人工审查 | diff review | 5-10 分钟 |

---

## 🔄 标准工作循环

```bash
# Init
pwd && cargo check && cat .harness/PROGRESS.md

# Plan — 全链路分析完成 → update_plan

# Implement — 约束内修改，一次一个功能

# Verify
cargo check && cargo test && cargo clippy

# Cleanup
# 更新 .harness/PROGRESS.md → git commit
```

---

## 🔗 铁律

1. **所有配置走 `Config` struct**，禁止裸写环境变量
2. **用户数据永远不做 shell 字符串拼接**，用 `Command::new("bash").arg(script).args(args)`
3. **ACK 用 Redis stream ID（`_redis_id`）**，不是飞书消息 ID
4. **改 executor 后必跑 `cargo test`**
5. **`src/core/subagent.rs.bak` 不允许提交** — 用 git 做版本控制

---

## 📐 代码风格规范

| 项目 | 规范 |
|---|---|
| 版本 | Rust edition 2021 |
| 格式化 | `cargo fmt` |
| Lint | `cargo clippy` — 无 warning |
| 错误处理 | `anyhow::Result` + `thiserror` |
| 异步 | `tokio` full features |
| 命名 | snake_case 函数/变量，PascalCase 类型 |
| 文档 | 公共 API 必须有 `///` doc comment |

---

## 📊 Agent 架构

| Agent | 角色 | 职责 |
|---|---|---|
| zhugeliang | 架构师 | 扫描 Bug、协调分配 |
| liubei | 项目经理 | 汇总、管理 |
| guanyu | 后端开发 | Java/Spring API Bug |
| zhaoyun | 前端开发 | Vue/前端 Bug |
| xunyu | DBA | 数据库/SQL |
| zhangfei | 测试 | 测试验证 |
| huatuo | 产品经理 | 需求分析 |
| chenlin | 文档专员 | 文档维护 |

---

## 📈 成熟度追踪

| 等级 | 特征 | 本项目 |
|---|---|---|
| L1 初始 | 无规范 | ✅ 已超越 |
| L2 管理 | 基础约束 + 反馈 | ✅ **当前** |
| L3 定义 | 标准化流程 | 🔄 本次目标 |
| L4 量化 | 数据驱动优化 | ⏳ |
| L5 优化 | AI 自主优化 | ⏳ |

---

> **总纲：** 一次一个功能，编译通过再提交
