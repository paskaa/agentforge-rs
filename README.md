<p align="center">
  <img src="https://img.shields.io/badge/Rust-2021-blue?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.6.0-green" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License">
  <img src="https://img.shields.io/badge/agents-8-orange" alt="Agents">
  <img src="https://img.shields.io/badge/model-mimo--code-purple" alt="Model">
  <img src="https://img.shields.io/badge/maturity-L5-brightgreen" alt="Maturity L5">
  <img src="https://img.shields.io/badge/ui-Element%20Plus-blue" alt="Element Plus">
  <img src="https://img.shields.io/badge/realtime-WebSocket-green" alt="WebSocket">
</p>

<h1 align="center">AgentForge-RS</h1>

<p align="center">
  <strong>Multi-Agent Bug Fixing Framework — Rust Rewrite</strong><br>
  <em>模型决定上限，Harness 决定底线。</em>
</p>

<p align="center">
  <a href="#-english">English</a> •
  <a href="#-简体中文">简体中文</a> •
  <a href="#-日本語">日本語</a>
</p>

---

# 🇬🇧 English

## Overview

AgentForge-RS is a **multi-agent automated bug fixing framework** built in Rust. It orchestrates 8 AI agents through a full pipeline — scan, diagnose, fix, test, verify, and document bugs from Zentao — with automatic git commits, Playwright testing, Zentao comment integration, quality gates, and Feishu notifications. Features a **real-time Web Dashboard** built with Vue 3 + Element Plus.

**Harness Engineering Maturity: L5 (Fully Optimized)**

## ✨ Features

- **8 Specialized Agents** — Backend, Frontend, DBA, Tester, Product, Docs, PM, Architect
- **Full Pipeline** — Scan → Queue → Fix → Analyze → Test → Verify → Archive → Zentao
- **Playwright Testing** — Every bug gets automated regression tests via BDT methodology
- **Zentao Integration** — Read bugs, structured comments at every pipeline stage, resolve + assign
- **Real-Time Dashboard** — Vue 3 + Element Plus, WebSocket real-time logs, clickable stats
- **Quality Gates** — Compilation verification, SQL validation, interface signature checks
- **Git Worktree Isolation** — Each agent has its own worktree, zero conflict
- **Smart Retry** — 3 verify attempts + consecutive same-error detection (stops if same error repeats)
- **Full-Chain Fix** — Frontend → Controller → Service → Mapper → DB → Related modules
- **L4 Analytics** — Data-driven: success rates, failure patterns, agent scoring
- **L5 Self-Optimizer** — AI auto-tuning: constraints, smart routing, retry strategy with git diff tracking
- **Harness Loop 4-Phase Cycle** — Generator→(Skip Review)→QA→Lightweight Verify, with compile retry ≤10min + loop ≤60min total timeout
- **Async Full-Chain Verification** — Post-fix: compile → unit test → Playwright → DB → API (5 checks, short-circuit on failure)
- **Batch Enqueue** — Select multiple bugs and enqueue to agents from the dashboard

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────┐
│           Web Dashboard (Vue3 + Element Plus)             │
│  http://localhost:18081                                   │
│  WebSocket: /ws  →  Real-time agent logs                 │
│  REST API: /api/dashboard, /api/queues, /api/bugs        │
│              /api/analytics, /api/agent/:id/traces        │
└──────────────┬──────────────────────────┬────────────────┘
               │                          │
┌──────────────▼──────────────────────────▼────────────────┐
│                    8 Agent Executors                       │
│  consume from Redis queues  →  spawn Codex/mimo-v2.5     │
│  → quality gates  →  git commit+push                     │
│  → Zentao comment (analyze/test/verify/archive)          │
│  → Feishu notification  →  Redis pub/sub                 │
└──────────────────────────────────────────────────────────┘
               │
    ┌──────────▼──────────────────────────────────┐
    │            Redis (Queue + Pub/Sub)           │
    │  agent-work-queue:fix:<agent_id>            │
    │  pipeline_sent:<bug_id>  (dedup 24h)        │
    │  pipeline_retry:<bug_id>  (retry counter)   │
    │  codex_lock:<agent_id>  (1h TTL)            │
    └─────────────────────────────────────────────┘

### Harness Loop 内部架构

```
subagent::run_codex_fix_v2()
  │
  └─→ subagent::run_harness_loop()          [总超时 60min]
        │
        ├─ Phase 1: codex_exec(harness_prompt)     → Verdict
        │   └─ 编译失败 → 无限重试（总超时 10min）
        │   └─ UNKNOWN + 有变更 → 降级编译验证
        │   └─ 完成后立即 commit（不等后续阶段）
        │
        ├─ Phase 2: ⏭ 跳过（99% 返回 UNKNOWN，节省 LLM 调用）
        │
        ├─ Phase 3: codex_exec(test_prompt)         → Verdict
        │   └─ 失败 → 降级: 直接 mvn compile / vite build
        │
        └─ Phase 4: 轻量级脚本验证（不调用 LLM）
            └─ 检查: fix_commit 存在 + 文件变更 > 0
            └─ 92ms 完成（旧 LLM 验证需 3-5 分钟）
        │
        └─→ CodexResult { last_phase, phase_verdicts }
```
```

## 🔄 Pipeline Flow

### Harness Loop（4 阶段循环）

每个 Bug 修复自动执行 4 阶段 Harness Loop（v0.6.0 优化版）：

```
┌──────────────────────────────────────────────────────────────┐
│            Harness Loop (per Bug, 总超时 60min)                │
│                                                                │
│  Phase 1: Generator 修复                                       │
│    └─ mimo-code exec (workspace-write)                        │
│    └─ 编译失败 → 无限重试（总超时 10min）                      │
│    └─ UNKNOWN + 有变更 → 降级编译验证                          │
│    └─ 完成后立即 commit（不等后续阶段）                        │
│    └─ 失败 → 终止循环                                         │
│                                                                │
│  Phase 2: ⏭ Reviewer 跳过                                     │
│    └─ 99% 返回 UNKNOWN，节省 LLM 调用                         │
│    └─ 节省 2-3 分钟/次                                        │
│                                                                │
│  Phase 3: QA 测试                                             │
│    └─ mimo-code exec (编译+测试)                               │
│    └─ 失败 → 降级: 直接 mvn compile / vite build              │
│                                                                │
│  Phase 4: 轻量级脚本验证                                       │
│    └─ 检查: fix_commit 存在 + 文件变更 > 0                     │
│    └─ 92ms 完成（旧 LLM 验证需 3-5 分钟）                     │
│    └─ 不调用 LLM，零 token 成本                                │
│                                                                │
│  结果: CodexResult { last_phase, phase_verdicts }             │
└──────────────────────────────────────────────────────────────┘

                    ↓ Harness Loop 完成后 ↓

┌──────────────────────────────────────────────────────────────┐
│         异步全链路验证 (executor.rs, 5 项检查)                  │
│                                                                │
│  1. 编译验证 (mvn compile / vite build)     ← 失败则短路       │
│  2. 单元测试 (mvn test / vitest)            ← 失败则短路       │
│  3. Playwright 回归 (@bug{id})              ← 无测试则跳过     │
│  4. 数据库验证 (PostgreSQL schema check)    ← 无关键词则跳过   │
│  5. 接口验证 (HTTP health check)                               │
│                                                                │
│  全部通过 → PASS                                               │
│  任一失败 → verify_retry（最多 3 次 + 连续相同错误检测）        │
└──────────────────────────────────────────────────────────────┘
```

### 全局流水线

```
fix_done (Harness Loop 完成)
  │
  ▼
诸葛亮 (分析路由)
  │── 无DB变更 ──→ 张飞 (Playwright测试)
  │                     │
  │── 有DB变更 ─→ 荀彧 (DB审查)
  │                     │
  │                     ▼
  │               张飞 (Playwright回归)
  │                     │
  │               华佗 (产品验收)
  │                     │
  │               陈琳 (文档归档)
  │                     │
  │               刘备 (协调/关闭禅道)
  │
  ▼
赵云/关羽 (二次修复 ← 验证失败反馈)
```

**每个阶段都自动写入禅道备注：**
- 诸葛亮：分析结果、路由决策、DB审查判断
- 荀彧：DB审查结果、风险评估
- 张飞：Playwright测试报告、测试输出摘要 → resolve
- 华佗：验收结果 → resolve + assign给提出人
- 陈琳：归档信息、全流程完成记录

## 🖥️ Dashboard Pages

| Page | Path | Description |
|---|---|---|
| **Dashboard** | `/` | Stats cards, agent status, recent fixes, pipeline flow |
| **Bug List** | `/bugs` | All bugs with refresh, batch enqueue, severity filter |
| **Bug Detail** | `/bugs/:id` | Bug info + Zentao link + test verification timeline |
| **Agent Detail** | `/agent/:id` | WebSocket real-time logs, queue status, success rate |
| **Agent System** | `/agents` | 8 agents + L5 scores + pipeline assignment |
| **Analytics** | `/analytics` | L4 metrics + L5 optimization records with git diff |
| **Queues** | `/queues` | Pipeline progress visualization (13 nodes) |

## 🤖 Agent System

| Code | Name | Role | Expertise |
|---|---|---|---|
| `guanyu` | 关羽 | Backend | Java, Spring, API, Service |
| `zhaoyun` | 赵云 | Frontend | Vue, UI, Components |
| `xunyu` | 荀彧 | DBA | Database, SQL, Migration |
| `zhangfei` | 张飞 | Tester | Playwright, QA, Regression |
| `huatuo` | 华佗 | Product | Requirements, Acceptance |
| `chenlin` | 陈琳 | Documentation | Docs, Wiki, Archival |
| `liubei` | 刘备 | PM | Coordination, Dispatch |
| `zhugeliang` | 诸葛亮 | Architect | Analysis, Routing, Design |

## 🧪 BDT (Bug-Driven Testing)

Every bug fix follows the 6-step BDT methodology:

1. **获取 Bug** — Fetch full details from Zentao (title, steps, attachments, comments)
2. **设计测试用例** — Generate Playwright spec from bug description
3. **基线测试** — Run test before fix to confirm bug exists
4. **修复代码** — Fix the bug with full-chain verification
5. **回归测试** — Run Playwright test to confirm fix works
6. **全链路验证** — Frontend → API → DB → Related modules

## 📁 Project Structure

```
agentforge-rs/
├── src/
│   ├── core/
│   │   ├── executor.rs        # Agent executor + pipeline handlers
│   │   ├── web_server.rs      # REST API + WebSocket
│   │   ├── zentao.rs          # Zentao API client (Rust native)
│   │   ├── pipeline.rs        # Pipeline routing + dedup
│   │   ├── subagent.rs        # Codex/mimo spawn + fix logic
│   │   ├── verification.rs    # Playwright test runner
│   │   └── trace.rs           # Trace store (SQLite)
│   ├── config.rs              # Configuration
│   └── main.rs                # CLI entry points
├── web/                       # Vue 3 + Element Plus frontend
│   ├── src/views/             # Dashboard, Agents, Analytics, Bugs, Queues
│   └── src/components/        # PipelineProgress, BugTable, VerificationFlow
├── agents/                    # 8 agent YAML configs
│   ├── guanyu.yaml, zhaoyun.yaml, xunyu.yaml, zhangfei.yaml
│   ├── huatuo.yaml, chenlin.yaml, liubei.yaml, zhugeliang.yaml
├── skills/                    # 8 Harness Engineering skill files
│   ├── harness-engineering/
│   ├── full-chain-fix/
│   ├── closed-loop-testing/
│   ├── constraint-design/
│   ├── durable-execution/
│   ├── review-audit/
│   ├── karpathy-guidelines/
│   └── walkinglabs-harness/
├── codex-config/              # Codex CLI config templates
├── deploy/                    # systemd service templates + setup.sh
├── .harness/                  # Harness Engineering metadata
├── tests/                     # E2E Playwright tests
├── AGENTS.md                  # Harness Engineering iron rules (304 lines)
└── README.md
```

## ⚡ Quick Start

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
sudo bash deploy/setup.sh          # One-click deploy
```

### Configuration

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# Edit with your Zentao, Redis, LLM credentials
```

### Run

```bash
# Build
cargo build --release
cp target/release/agentforge /usr/local/bin/

# Start all services
agentforge web --port 18081                 # Dashboard
agentforge executor --agent guanyu          # Single agent
agentforge executor --agent zhaoyun         # Single agent
# ... or use systemd templates in deploy/

# Pipeline
agentforge scan-bugs                        # Scan all active bugs
agentforge pipeline --max-bugs 10           # Fix via pipeline

# Analytics
agentforge analytics                        # L4: metrics JSON
agentforge report                           # L4: Markdown report
agentforge optimize                         # L5: self-optimization
agentforge scores                           # L5: agent scores
```

### Services (systemd)

```bash
agentforge-web.service              # Dashboard on port 18081
agentforge-rust@guanyu.service      # 关羽 executor
agentforge-rust@zhaoyun.service     # 赵云 executor
agentforge-rust@xunyu.service       # 荀彧 executor
agentforge-rust@zhangfei.service    # 张飞 executor
agentforge-rust@huatuo.service      # 华佗 executor
agentforge-rust@chenlin.service     # 陈琳 executor
agentforge-rust@liubei.service      # 刘备 executor
agentforge-rust@zhugeliang.service  # 诸葛亮 executor
```

## 🔧 Iron Rules (铁律)

> **18 comprehensive iron rules** — see [agentforge-harness-skill](https://github.com/paskaa/agentforge-harness-skill) for the full set.

| # | Rule | Description |
|---|---|---|
| 1 | Bug状态管理 | Skip resolved/closed bugs |
| 2 | 修复流程 | One bug at a time |
| 3 | 全链路6环 | Frontend→Controller→Service→Mapper→DB→Relations |
| 4 | 状态值一致性 | Check 6 places before changing status |
| 5 | 影响面分析 | Search all references with `rg` |
| 6 | 逆向流程验证 | Test cancel/refund/stop flows |
| 7 | 全链路验证 | DB→Backend→Frontend→Stats |
| 8 | 池/统计表同步 | Status changes must sync stats |
| 9 | 统计变更验证 | Verify stats in database |
| 10 | 禁止删除源文件 | Fix errors, don't delete files |
| 18 | 禁止硬编码默认值 | Use user-selected values |
| 25 | 接口签名 | Interface compatibility check |
| 26 | BDT 流程 | 6-step bug-driven testing |
| 27 | cherry-pick 验证 | Verify on develop after cherry-pick |
| 28 | 登录凭证 | Credentials from Zentao only |

## 📊 Maturity Model

| Level | Feature | Status |
|---|---|---|
| L1 Initial | No constraints | ✅ Exceeded |
| L2 Managed | Basic constraints + feedback | ✅ Done |
| L3 Defined | Standardized flow + auto-commit + Zentao | ✅ Done |
| L4 Quantified | Data-driven optimization | ✅ Done |
| L5 Optimized | AI self-optimization | ✅ Done |

## 🚀 v0.6.0 Optimization Summary

### Before vs After

| Metric | v0.5.0 | v0.6.0 |
|---|---|---|
| Phase 4 验证耗时 | 3-5 min (LLM) | **92 ms** (script) |
| Phase 2 Review 耗时 | 2-3 min (99% UNKNOWN) | **0** (skipped) |
| verify 重试上限 | 10 次 | **3 次** + same-error detection |
| fix 重试上限 | 10 次 | **5 次** |
| 编译重试 | 无限制循环 | **10 min** total timeout |
| Harness Loop 总超时 | 无上限 | **60 min** hard limit |
| 总 LLM 调用/bug | 6-8 次 | **2-3 次** |
| 验证假阳性 | 频繁（verify=PASS 但实际失败） | **消除**（脚本真实检查） |

### Architecture Changes

```
v0.5.0:  Phase1 → Phase2(LLM review ×2) → Phase3(QA) → Phase4(LLM verify)
         ↓ 全部完成后再异步验证
         ↓ verify_retry × 10 (无限制重试)

v0.6.0:  Phase1 → [Skip] → Phase3(QA) → Phase4(script 92ms)
         ↓ 完成后立即 commit + 异步全链路验证
         ↓ verify_retry × 3 (相同错误 2 次即停止)
         ↓ 总超时 60min / 编译超时 10min
```

## 📜 License

MIT

---

# 🇨🇳 简体中文

## 概述

AgentForge-RS 是一个用 Rust 构建的 **多智能体自动修复 Bug 框架**。编排 8 个 AI 智能体完成全流程 — 扫描、诊断、修复、测试、验收、归档 — 包含自动 Git 提交、Playwright 测试、禅道备注集成、质量门禁和飞书通知。

**Harness Engineering 成熟度：L5（完全优化）**

## ✨ 核心特性

- **8 个专业智能体** — 后端、前端、DBA、测试、产品、文档、项目管理、架构
- **完整管线** — 扫描 → 入队 → 修复 → 分析路由 → 测试 → 验收 → 归档 → 禅道
- **Playwright 测试** — 每个 Bug 自动生成回归测试（BDT 方法论）
- **禅道集成** — 读取 Bug、每个管线阶段写入结构化备注、解决+分配
- **实时面板** — Vue 3 + Element Plus、WebSocket 实时日志、可点击统计
- **质量门禁** — 编译验证、SQL 校验、接口签名检查
- **Git Worktree 隔离** — 每个智能体独立工作树，零冲突
- **智能重试** — 5 次修复 + 3 次验证，连续相同错误自动停止
- **全链路修复** — 前端 → Controller → Service → Mapper → DB → 关联模块
- **L4 量化分析** — 数据驱动：成功率、失败模式、智能体评分
- **L5 自优化** — AI 自主调优：约束调整、智能路由、重试策略（Git Diff 追踪）

## 🔄 管线流程

```
修复完成 (关羽/赵云)
  │
  ▼
诸葛亮 (分析路由)
  │── 无DB变更 ──→ 张飞 (Playwright测试)
  │                     │
  │── 有DB变更 ─→ 荀彧 (DB审查)
  │                     │
  │                     ▼
  │               张飞 (Playwright测试)
  │                     │
  │               ┌─────┴─────┐
  │               ▼           ▼
  │           华佗(验收)   陈琳(归档)
  │               │
  │               ▼
  │          禅道 resolve + assign
  │
  └── 失败 → 回退给修复者重修（最多 5 次 fix + 3 次 verify，连续相同错误自动停止）
```

**每个阶段自动写入禅道备注：**

| 阶段 | 智能体 | 禅道操作 |
|---|---|---|
| 分析路由 | 诸葛亮 | 添加备注（分析结果、路由决策） |
| DB审查 | 荀彧 | 添加备注（审查结果、风险评估） |
| 测试 | 张飞 | 添加测试报告 + resolve |
| 验收 | 华佗 | 添加备注 + resolve + assign 给提出人 |
| 归档 | 陈琳 | 添加备注（全流程完成记录） |

## 🖥️ 面板页面

| 页面 | 路径 | 说明 |
|---|---|---|
| **仪表盘** | `/` | 统计卡片、智能体状态、最近修复、管线流程 |
| **Bug 明细** | `/bugs` | 全部 Bug、刷新、批量入列、严重程度筛选 |
| **Bug 详情** | `/bugs/:id` | Bug 信息 + 禅道链接 + 测试验证时间线 |
| **智能体详情** | `/agent/:id` | WebSocket 实时日志、队列状态、成功率 |
| **智能体系统** | `/agents` | 8 智能体 + L5 评分 + 管线分配 |
| **L4/L5 分析** | `/analytics` | L4 指标 + L5 优化记录（含 Git Diff 对比） |
| **队列** | `/queues` | 管线进度可视化（13 节点） |

## 🤖 智能体系统

| 代号 | 名称 | 角色 | 专长 |
|---|---|---|---|
| `guanyu` | 关羽 | 后端开发 | Java, Spring, API, Service |
| `zhaoyun` | 赵云 | 前端开发 | Vue, UI, 组件 |
| `xunyu` | 荀彧 | DBA | 数据库, SQL, 迁移 |
| `zhangfei` | 张飞 | 测试 | Playwright, QA, 回归 |
| `huatuo` | 华佗 | 产品 | 需求, 验收 |
| `chenlin` | 陈琳 | 文档 | 文档, Wiki, 归档 |
| `liubei` | 刘备 | 项目管理 | 协调, 分派 |
| `zhugeliang` | 诸葛亮 | 架构 | 分析, 路由, 设计 |

## 🧪 BDT (Bug-Driven Testing)

每个 Bug 修复遵循 6 步 BDT 方法论：

1. **获取 Bug** — 从禅道获取完整详情（标题、步骤、附件、备注）
2. **设计测试用例** — 根据 Bug 描述生成 Playwright 测试脚本
3. **基线测试** — 修复前运行测试确认 Bug 存在
4. **修复代码** — 全链路验证修复
5. **回归测试** — 运行 Playwright 测试确认修复有效
6. **全链路验证** — 前端 → API → DB → 关联模块

## 📁 项目结构

```
agentforge-rs/
├── src/
│   ├── core/
│   │   ├── executor.rs        # 智能体执行器 + 管线处理器
│   │   ├── web_server.rs      # REST API + WebSocket
│   │   ├── zentao.rs          # 禅道 API 客户端（Rust 原生）
│   │   ├── pipeline.rs        # 管线路由 + 去重
│   │   ├── subagent.rs        # Harness Loop 4阶段 + 智能体修复逻辑
│   │   ├── verification.rs    # 5项全链路验证（编译/单测/Playwright/DB/API）
│   ├── analytics.rs       # L4 数据分析 + L5 评分
│   ├── self_optimizer.rs  # L5 AI 自优化引擎
│   ├── report.rs          # 分析报告生成（含连接错误检测）
│   ├── quota_monitor.rs   # 配额监控
│   ├── dead_letter.rs     # 死信队列管理
│   ├── fix_trajectory.rs  # 修复轨迹追踪
│   │   └── trace.rs           # 追踪存储（SQLite）
│   ├── config.rs              # 配置
│   └── main.rs                # CLI 入口
├── web/                       # Vue 3 + Element Plus 前端
│   ├── src/views/             # 仪表盘、智能体、分析、Bug、队列
│   └── src/components/        # PipelineProgress, BugTable, VerificationFlow
├── agents/                    # 8 智能体 YAML 配置
├── skills/                    # 8 Harness Engineering 技能文件
├── codex-config/              # Codex CLI 配置模板
├── deploy/                    # systemd 服务模板 + setup.sh
├── .harness/                  # Harness Engineering 元数据
├── tests/                     # E2E Playwright 测试
├── AGENTS.md                  # Harness Engineering 铁律（304 行）
└── README.md
```

## ⚡ 快速开始

> **v0.6.0**: 验证系统重构 — 消除假阳性、智能重试、超时保护
> 
> **核心改进:**
> - Phase 4 LLM 验证器 → 轻量级脚本检查（92ms vs 3-5min，零 token 成本）
> - Phase 2 Review 跳过（99% 返回 UNKNOWN，节省 2-3min/次）
> - verify_retry 10→3 + 连续相同错误检测（相同错误 2 次即停止）
> - fix_retry 10→5（减少无效重试浪费）
> - 编译重试增加 10 分钟总超时（防止死循环）
> - Harness Loop 增加 60 分钟总超时保护
> - 异步全链路验证：编译→单测→Playwright→DB→API（5 项检查，短路机制）
> 
> **v0.5.0 核心改进:**
> - `parse_verdict` 启发式容错 — verdict 解析率 0% → ~80%
> - `fix_done` 状态修正 — 有实际文件变更时覆盖为 ok
> - `fix_active` 锁优化 — TTL 24h→30min，防止 Bug 永久阻塞
> - Dashboard 数据归一化 — 中英文 agent_id 合并统计
> - L5 分数直接计算 — 分数准确反映实际能力

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
sudo bash deploy/setup.sh          # 一键部署
```

### 配置

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# 填写你的禅道、Redis、LLM 凭证
```

### 运行

```bash
cargo build --release
cp target/release/agentforge /usr/local/bin/

# 启动服务
agentforge web --port 18081                 # 面板
agentforge executor --agent guanyu          # 单个智能体

# 管线
agentforge scan-bugs                        # 扫描所有活跃 Bug
agentforge pipeline --max-bugs 10           # 管线修复

# 分析
agentforge analytics                        # L4 指标
agentforge optimize                         # L5 自优化
agentforge scores                           # L5 评分
```

### 系统服务

```bash
agentforge-web.service              # 面板 :18081
agentforge-rust@guanyu.service      # 关羽
agentforge-rust@zhaoyun.service     # 赵云
agentforge-rust@xunyu.service       # 荀彧
agentforge-rust@zhangfei.service    # 张飞
agentforge-rust@huatuo.service      # 华佗
agentforge-rust@chenlin.service     # 陈琳
agentforge-rust@liubei.service      # 刘备
agentforge-rust@zhugeliang.service  # 诸葛亮
```

## 🔧 铁律

| # | 规则 | 说明 |
|---|---|---|
| 1 | 先分解再行动 | 非平凡任务先出计划 |
| 2 | 并行优先 | 独立操作必须批量调用 |
| 3 | 验证后信 | 每次工具调用后确认结果 |
| 4 | 上下文 40% 阈值 | 接近时建议 compact |
| 5 | AGENTS.md 是地图 | 详细规则按需加载 |
| 24 | 编译验证 | push 前必须编译通过 |
| 25 | 接口签名 | 接口兼容性检查 |
| 26 | BDT 流程 | 6 步 Bug 驱动测试 |
| 27 | cherry-pick 验证 | develop 分支验证 |
| 28 | 登录凭证 | 凭证从禅道获取 |

## 📊 成熟度模型

| 级别 | 特征 | 状态 |
|---|---|---|
| L1 初期 | 无约束 | ✅ 已超越 |
| L2 管理 | 基本约束 + 反馈 | ✅ 已完成 |
| L3 定义 | 标准化流程 + 自动提交 + 禅道 | ✅ 已完成 |
| L4 量化 | 数据驱动优化 | ✅ 已完成 |
| L5 优化 | AI 自主优化 + Harness Loop 4阶段循环 | ✅ 已完成 |

## 🚀 v0.6.0 优化总结

### 前后对比

| 指标 | v0.5.0 | v0.6.0 |
|---|---|---|
| Phase 4 验证耗时 | 3-5 分钟（LLM） | **92 毫秒**（脚本） |
| Phase 2 Review 耗时 | 2-3 分钟（99% UNKNOWN） | **0**（跳过） |
| verify 重试上限 | 10 次 | **3 次** + 相同错误检测 |
| fix 重试上限 | 10 次 | **5 次** |
| 编译重试 | 无限制循环 | **10 分钟**总超时 |
| Harness Loop 总超时 | 无上限 | **60 分钟**硬上限 |
| 总 LLM 调用/Bug | 6-8 次 | **2-3 次** |
| 验证假阳性 | 频繁 | **消除**（脚本真实检查） |

### 架构变更

```
v0.5.0:  Phase1 → Phase2(LLM review ×2) → Phase3(QA) → Phase4(LLM verify)
         ↓ 全部完成后再异步验证
         ↓ verify_retry × 10（无限制重试）

v0.6.0:  Phase1 → [跳过] → Phase3(QA) → Phase4(脚本 92ms)
         ↓ 完成后立即 commit + 异步全链路验证
         ↓ verify_retry × 3（相同错误 2 次即停止）
         ↓ 总超时 60min / 编译超时 10min
```

## 📜 许可证

MIT

---

# 🇯🇵 日本語

## 概要

AgentForge-RSはRustで構築された**マルチエージェント自動バグ修正フレームワーク**です。8つのAIエージェントを連携させ、Zentaoからバグをスキャン、診断、修正、テストし、自動コミット、Playwrightテスト、Zentaoコメント統合、品質ゲート、Feishu通知に対応しています。

**Harness Engineering 成熟度：L5（完全最適化）**

## ✨ 特徴

- **8つの専門エージェント** — バックエンド、フロントエンド、DBA、テスト、プロダクト、ドキュメント、PM、アーキテクト
- **完全パイプライン** — スキャン → キュー → 修正 → 分析 → テスト → 検収 → アーカイブ → Zentao
- **Playwrightテスト** — 全バグに自動リグレステスト（BDT方法論）
- **Zentao連携** — バグ取得、全パイプライン段階で構造化コメント、解決+アサイン
- **リアルタイムダッシュボード** — Vue 3 + Element Plus、WebSocketリアルタイムログ
- **品質ゲート** — コンパイル検証、SQL検証、インターフェース署名チェック
- **Git Worktree分離** — 競合なし
- **L4分析** — データドリブン最適化
- **L5自己最適化** — AI自律最適化、Git Diff追跡

## 🤖 エージェントシステム

| コード | 名前 | 役割 | 専門分野 |
|---|---|---|---|
| `guanyu` | 関羽 | バックエンド | Java, Spring, API |
| `zhaoyun` | 趙雲 | フロントエンド | Vue, UI |
| `xunyu` | 荀彧 | DBA | データベース, SQL |
| `zhangfei` | 張飛 | テスター | Playwright, QA |
| `huatuo` | 華佗 | プロダクト | 要件, 検収 |
| `chenlin` | 陳琳 | ドキュメント | ドキュメント, Wiki |
| `liubei` | 劉備 | PM | 協調, 分配 |
| `zhugeliang` | 諸葛亮 | アーキテクト | 分析, ルーティング |

## 📜 ライセンス

MIT
