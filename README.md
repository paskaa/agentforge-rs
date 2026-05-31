<p align="center">
  <img src="https://img.shields.io/badge/Rust-2021-blue?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.2.0-green" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License">
  <img src="https://img.shields.io/badge/agents-8-orange" alt="Agents">
  <img src="https://img.shields.io/badge/model-mimo--v2.5-purple" alt="Model">
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

AgentForge-RS is a **multi-agent automated bug fixing framework** built in Rust. It orchestrates 8 AI agents to scan, diagnose, fix, test, and document bugs from Zentao, with automatic git commits, quality gates, and Feishu notifications. Features a **real-time Web Dashboard** built with Vue 3 + Element Plus.

**Harness Engineering Maturity: L5 (Fully Optimized)**

## ✨ Features

- **8 Specialized Agents** — Backend, Frontend, DBA, Tester, PM, Docs, Coordination, Architect
- **Automated Pipeline** — Scan → Queue → Fix → Quality gates → Commit + push → Zentao comments
- **Real-Time Dashboard** — Vue 3 + Element Plus, WebSocket real-time logs, clickable stats
- **Quality Gates** — Compilation, SQL validation, code review before commit
- **Zentao Integration** — Read bugs, structured comments, assignment management, bug detail links
- **Feishu Notifications** — Real-time alerts
- **Git Worktree Isolation** — Each agent has its own worktree
- **Dead Letter Queue** — Failed tasks preserved for retry
- **Full-Chain Fix** — Frontend → Controller → Service → Mapper → DB
- **L4 Analytics** — Data-driven: success rates, failure patterns, agent scoring
- **L5 Self-Optimizer** — AI auto-tuning: constraints, smart routing, retry strategy with git diff tracking
- **Batch Enqueue** — Select multiple bugs and enqueue to agents from the dashboard
- **Pipeline Progress** — 13-node visualization (分配→分析→尝试→LLM→生成→重试→完成→测试→验证→Diff→验收→归档→解决)

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────┐
│               Web Dashboard (Vue3 + Element Plus)        │
│  http://localhost:18081                                   │
│  WebSocket: /ws  →  Real-time agent logs                 │
│  REST API: /api/dashboard, /api/queues, /api/bugs        │
│              /api/analytics, /api/agent/:id/traces        │
└──────────────┬──────────────────────────┬────────────────┘
               │                          │
┌──────────────▼──────────────────────────▼────────────────┐
│                    Pipeline (CLI)                         │
│  scan Zentao → queue to Redis → poll for results         │
└──────────────┬──────────────────────────┬────────────────┘
               │                          │
    ┌──────────▼──────────┐    ┌──────────▼──────────┐
    │   Agent Executor     │    │   Agent Executor     │
    │   (guanyu/zhaoyun)   │    │   (xunyu/huatuo)     │
    │  consume from Redis  │    │  consume from Redis  │
    │  → build prompt      │    │  → build prompt      │
    │  → spawn Codex       │    │  → spawn Codex       │
    │  → quality gates     │    │  → quality gates     │
    │  → git commit+push   │    │  → git commit+push   │
    │  → zentao comment    │    │  → zentao comment    │
    │  → Redis pub/sub     │    │  → Redis pub/sub     │
    └──────────┬───────────┘    └──────────┬───────────┘
               │                          │
    ┌──────────▼──────────────────────────▼──────────┐
    │         L4 Analytics + L5 Self-Optimizer         │
    │  TraceStore → Metrics → Auto-tune → Scores      │
    │  failure_patterns → constraint_adjustments      │
    │  L5 optimization_log.json + git diff tracking   │
    └─────────────────────────────────────────────────┘
```

## ⚡ Quick Start

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
sudo bash deploy/setup.sh          # one-click deploy
```

### Configure

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# edit with your credentials (Redis, Zentao, LLM, Feishu)
```

### Run

```bash
agentforge scan-bugs                    # scan all active bugs
agentforge pipeline --max-bugs 10       # run fix pipeline
agentforge executor --agent zhaoyun     # start single agent
agentforge web --port 18081             # start dashboard
agentforge analytics                    # L4: metrics JSON
agentforge report                       # L4: Markdown report
agentforge optimize                     # L5: self-optimize
agentforge scores                       # L5: agent scores
```

## 🖥️ CLI Commands

| Command | Description |
|---|---|
| `scan-bugs` | Scan all active bugs from Zentao |
| `pipeline --max-bugs N` | Run bug-fixing pipeline |
| `executor --agent <name>` | Start agent executor |
| `web --port <port>` | Start dashboard server |
| `query-bug --bug-id N` | Query bug details |
| `analytics` | L4: Metrics JSON from TraceStore |
| `report` | L4: Markdown analysis report |
| `optimize` | L5: Self-optimize with git diff tracking |
| `scores` | L5: Agent performance scores |

## 🖥️ Web Dashboard

The dashboard runs on port **18081** with these pages:

| Page | Path | Description |
|---|---|---|
| **Dashboard** | `/` | Stats cards (clickable), recent fixes, pipeline status |
| **Bug List** | `/bugs` | All bugs with refresh, batch enqueue, severity labels |
| **Agent Detail** | `/agent/:id` | WebSocket real-time logs, queue, success rate |
| **Agents** | `/agents` | All 8 agents with L5 scores (2 decimal places) |
| **Analytics** | `/analytics` | L4 metrics + L5 optimization records with git history |
| **Queues** | `/queues` | Pipeline progress visualization (13 nodes) |

### API Endpoints

| Endpoint | Method | Description |
|---|---|---|
| `/api/dashboard` | GET | Dashboard stats + recent fixes |
| `/api/queues` | GET | All agent queues with current bug |
| `/api/bugs` | GET | All bugs from Zentao |
| `/api/bugs/fixed_today` | GET | Today's fixed bugs |
| `/api/bugs/:id/traces` | GET | Bug trace timeline |
| `/api/bugs/enqueue` | POST | Enqueue single bug |
| `/api/bugs/batch-enqueue` | POST | Batch enqueue multiple bugs |
| `/api/analytics` | GET | Full L4 analytics report |
| `/api/agent/:id/traces` | GET | Agent trace history |
| `/api/agent/:id/traces/rt` | GET | Agent real-time traces |
| `/api/agent/:id/queue` | GET | Agent queue items |
| `/api/l5/optimizations` | GET | L5 optimization log |
| `/ws` | WebSocket | Real-time trace broadcast |

## 🤖 Agent System

| Code | Name | Role | Expertise |
|---|---|---|---|
| `guanyu` | 关羽 | Backend Dev | Java, Spring, API |
| `zhaoyun` | 赵云 | Frontend Dev | Vue, UI |
| `xunyu` | 荀彧 | DBA | Database, SQL |
| `zhangfei` | 张飞 | Tester | Testing, QA |
| `huatuo` | 华佗 | Product Manager | Requirements |
| `chenlin` | 陈琳 | Documentation | Docs, Wiki |
| `liubei` | 刘备 | Project Manager | Coordination |
| `zhugeliang` | 诸葛亮 | Architect | Architecture |

Agent configs: `agents/*.yaml` | Skills: `skills/` | Deploy: `deploy/`

## 📁 Project Structure

```
agentforge-rs/
├── src/
│   └── core/
│       ├── analytics.rs          # L4 analytics engine
│       ├── coordinator.rs        # Bug routing + Zentao integration
│       ├── executor.rs           # Agent executor main loop
│       ├── fix_trajectory.rs     # Fix trajectory tracking
│       ├── pipeline.rs           # Batch bug-fixing pipeline
│       ├── self_optimizer.rs     # L5 self-optimization engine
│       ├── subagent.rs           # Codex invocation + prompt builder
│       ├── trace.rs              # SQLite trace store
│       ├── web_server.rs         # REST API + WebSocket server
│       └── zentao.rs             # Zentao API client
├── web/                          # Vue 3 + Element Plus frontend
│   └── src/
│       ├── views/
│       │   ├── Dashboard.vue     # Main dashboard
│       │   ├── BugList.vue       # Bug list with batch enqueue
│       │   ├── AgentDetail.vue   # Agent detail + WebSocket logs
│       │   ├── Agents.vue        # Agent system overview
│       │   ├── Analytics.vue     # L4/L5 analytics
│       │   └── Queues.vue        # Pipeline progress
│       └── components/
│           ├── BugTable.vue      # Bug table with enqueue button
│           ├── FixTable.vue      # Fix history table
│           └── PipelineProgress.vue # 13-node pipeline visualization
├── agents/                       # 8 agent YAML configurations
├── skills/                       # 8 Harness Engineering skill files
├── codex-config/                 # Codex CLI config templates
├── deploy/                       # systemd service templates + setup
├── .harness/                     # Harness Engineering metadata
├── config/
│   └── agentforge.yaml           # Runtime configuration
└── AGENTS.md                     # Harness Engineering master doc
```

## 📊 Maturity Model

| Level | Characteristics | Status |
|---|---|---|
| L1 Initial | No standards | ✅ Surpassed |
| L2 Managed | Basic constraints + feedback | ✅ Complete |
| L3 Defined | Standardized workflow + auto-commit + Zentao | ✅ Complete |
| L4 Quantitative | Data-driven optimization | ✅ Complete |
| L5 Optimizing | AI self-optimization | ✅ Complete |

## 📜 License

MIT

---

# 🇨🇳 简体中文

## 概览

AgentForge-RS 是一个基于 Rust 构建的**多智能体自动 Bug 修复框架**。8 个 AI 智能体协同工作，从禅道扫描、诊断、修复、测试、归档 Bug，支持自动 Git 提交、质量门禁、飞书通知。配备**实时 Web 控制面板**（Vue 3 + Element Plus）。

**Harness Engineering 成熟度：L5（完全优化）**

## ✨ 特性

- **8 个专业智能体** — 后端、前端、DBA、测试、PM、文档、协调、架构师
- **自动流水线** — 扫描 → 入列 → 修复 → 质量门禁 → 提交推送 → 禅道备注
- **实时控制面板** — Vue 3 + Element Plus，WebSocket 实时日志，可点击统计卡片
- **质量门禁** — 编译验证、SQL 校验、代码审查
- **禅道集成** — 读取 Bug、结构化备注、分配管理、Bug 链接跳转
- **飞书通知** — 实时告警
- **Git Worktree 隔离** — 每个智能体独立工作树
- **死信队列** — 失败任务持久化
- **全链路修复** — 前端 → Controller → Service → Mapper → DB
- **L4 量化分析** — 数据驱动：成功率、失败模式、智能体评分
- **L5 AI 自主优化** — 智能调优：约束增强、智能路由、重试策略，含 Git Diff 追踪
- **批量入列** — 在面板中选择多个 Bug 一键入列
- **流水线进度** — 13 节点可视化（分配→分析→尝试→LLM→生成→重试→完成→测试→验证→Diff→验收→归档→解决）

## 🏗️ 架构

```
┌─────────────────────────────────────────────────────────┐
│               Web 控制面板 (Vue3 + Element Plus)         │
│  http://localhost:18081                                  │
│  WebSocket: /ws  →  智能体实时日志                       │
│  REST API: /api/dashboard, /api/queues, /api/bugs       │
│              /api/analytics, /api/agent/:id/traces       │
└──────────────┬──────────────────────────┬───────────────┘
               │                          │
┌──────────────▼──────────────────────────▼───────────────┐
│                    流水线 (Pipeline)                      │
│  扫描禅道 → Redis 队列 → 等待结果                        │
└──────────────┬──────────────────────────┬───────────────┘
               │                          │
    ┌──────────▼──────────┐    ┌──────────▼──────────┐
    │   智能体执行器       │    │   智能体执行器       │
    │   (guanyu/zhaoyun)   │    │   (xunyu/huatuo)    │
    │  消费 Redis 队列     │    │  消费 Redis 队列     │
    │  → 构建 Prompt       │    │  → 构建 Prompt       │
    │  → 调用 Codex        │    │  → 调用 Codex        │
    │  → 质量门禁          │    │  → 质量门禁          │
    │  → Git 提交推送      │    │  → Git 提交推送      │
    │  → 禅道备注          │    │  → 禅道备注          │
    │  → Redis pub/sub     │    │  → Redis pub/sub     │
    └──────────┬───────────┘    └──────────┬───────────┘
               │                          │
    ┌──────────▼──────────────────────────▼──────────┐
    │         L4 量化分析 + L5 AI 自主优化              │
    │  TraceStore → 指标 → 自动调优 → 评分             │
    │  失败模式分析 → 约束调整                         │
    │  L5 优化日志 + Git Diff 追踪                     │
    └─────────────────────────────────────────────────┘
```

## ⚡ 快速开始

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
sudo bash deploy/setup.sh          # 一键部署
```

### 配置

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# 编辑配置（Redis、禅道、LLM、飞书）
```

### 运行

```bash
agentforge scan-bugs                    # 扫描所有活跃 Bug
agentforge pipeline --max-bugs 10       # 运行修复流水线
agentforge executor --agent zhaoyun     # 启动单个智能体
agentforge web --port 18081             # 启动控制面板
agentforge analytics                    # L4：指标 JSON
agentforge report                       # L4：Markdown 报告
agentforge optimize                     # L5：自优化
agentforge scores                       # L5：智能体评分
```

## 🖥️ 控制面板

面板运行在端口 **18081**，包含以下页面：

| 页面 | 路径 | 说明 |
|---|---|---|
| **仪表盘** | `/` | 统计卡片（可点击）、最近修复、流水线状态 |
| **Bug 列表** | `/bugs` | 所有 Bug，支持刷新、批量入列、严重程度标签 |
| **智能体详情** | `/agent/:id` | WebSocket 实时日志、队列、成功率 |
| **智能体系统** | `/agents` | 8 个智能体 + L5 评分（保留 2 位小数） |
| **L4/L5 分析** | `/analytics` | L4 指标 + L5 优化记录（含 Git 历史） |
| **队列** | `/queues` | 流水线进度可视化（13 节点） |

### API 端点

| 端点 | 方法 | 说明 |
|---|---|---|
| `/api/dashboard` | GET | 仪表盘统计 + 最近修复 |
| `/api/queues` | GET | 所有智能体队列 + 当前处理 |
| `/api/bugs` | GET | 禅道所有 Bug |
| `/api/bugs/fixed_today` | GET | 今日修复 Bug |
| `/api/bugs/:id/traces` | GET | Bug 修复时间线 |
| `/api/bugs/enqueue` | POST | 单个 Bug 入列 |
| `/api/bugs/batch-enqueue` | POST | 批量 Bug 入列 |
| `/api/analytics` | GET | 完整 L4 分析报告 |
| `/api/agent/:id/traces` | GET | 智能体修复历史 |
| `/api/agent/:id/traces/rt` | GET | 智能体实时追踪 |
| `/api/agent/:id/queue` | GET | 智能体队列 |
| `/api/l5/optimizations` | GET | L5 优化记录 |
| `/ws` | WebSocket | 实时追踪推送 |

## 🤖 智能体系统

| 代号 | 名称 | 角色 | 专长 |
|---|---|---|---|
| `guanyu` | 关羽 | 后端开发 | Java, Spring, API |
| `zhaoyun` | 赵云 | 前端开发 | Vue, 界面 |
| `xunyu` | 荀彧 | DBA | 数据库, SQL |
| `zhangfei` | 张飞 | 测试 | 测试, QA |
| `huatuo` | 华佗 | 产品经理 | 需求分析 |
| `chenlin` | 陈琳 | 文档专员 | 文档, Wiki |
| `liubei` | 刘备 | 项目经理 | 协调管理 |
| `zhugeliang` | 诸葛亮 | 架构师 | 架构设计 |

智能体配置：`agents/*.yaml` | 技能：`skills/` | 部署：`deploy/`

## 📁 项目结构

```
agentforge-rs/
├── src/
│   └── core/
│       ├── analytics.rs          # L4 量化分析引擎
│       ├── coordinator.rs        # Bug 路由 + 禅道集成
│       ├── executor.rs           # 智能体执行器主循环
│       ├── fix_trajectory.rs     # 修复轨迹记录
│       ├── pipeline.rs           # 批量修复流水线
│       ├── self_optimizer.rs     # L5 自优化引擎
│       ├── subagent.rs           # Codex 调用 + Prompt 构建
│       ├── trace.rs              # SQLite 追踪存储
│       ├── web_server.rs         # REST API + WebSocket 服务
│       └── zentao.rs             # 禅道 API 客户端
├── web/                          # Vue 3 + Element Plus 前端
│   └── src/
│       ├── views/
│       │   ├── Dashboard.vue     # 主仪表盘
│       │   ├── BugList.vue       # Bug 列表 + 批量入列
│       │   ├── AgentDetail.vue   # 智能体详情 + WebSocket 日志
│       │   ├── Agents.vue        # 智能体系统总览
│       │   ├── Analytics.vue     # L4/L5 分析
│       │   └── Queues.vue        # 流水线进度
│       └── components/
│           ├── BugTable.vue      # Bug 表格 + 入列按钮
│           ├── FixTable.vue      # 修复历史表格
│           └── PipelineProgress.vue # 13 节点流水线可视化
├── agents/                       # 8 个智能体 YAML 配置
├── skills/                       # 8 个 Harness Engineering 技能文件
├── codex-config/                 # Codex CLI 配置模板
├── deploy/                       # systemd 服务模板 + 部署脚本
├── .harness/                     # Harness Engineering 元数据
├── config/
│   └── agentforge.yaml           # 运行时配置
└── AGENTS.md                     # Harness Engineering 总纲
```

## 📊 成熟度模型

| 等级 | 特征 | 状态 |
|---|---|---|
| L1 初始 | 无规范 | ✅ 已超越 |
| L2 管理 | 基础约束 + 反馈 | ✅ 已完成 |
| L3 定义 | 标准化流程 + 自动提交 + 禅道闭环 | ✅ 已完成 |
| L4 量化 | 数据驱动优化 | ✅ 已完成 |
| L5 优化 | AI 自主优化 | ✅ 已完成 |

## 📜 许可证

MIT

---

# 🇯🇵 日本語

## 概要

AgentForge-RSはRustで構築された**マルチエージェント自動バグ修正フレームワーク**です。8つのAIエージェントを連携させ、Zentaoからバグをスキャン、診断、修正、テストし、自動コミット、品質ゲート、Feishu通知に対応しています。**リアルタイムWebダッシュボード**（Vue 3 + Element Plus）を装備。

**Harness Engineering 成熟度：L5（完全最適化）**

## ✨ 特徴

- **8つの専門エージェント** — バックエンド、フロントエンド、DBA、テスト、PM、ドキュメント、協調、アーキテクト
- **自動パイプライン** — スキャン → キュー → 修正 → 品質ゲート → コミット+プッシュ → Zentaoコメント
- **リアルタイムダッシュボード** — Vue 3 + Element Plus、WebSocketリアルタイムログ
- **品質ゲート** — コンパイル、SQL検証、コードレビュー
- **Zentao連携** — バグ読み取り、構造化コメント、アサイン管理
- **Git Worktree分離** — 競合なし
- **フルチェーン修正** — フロントエンド → Controller → Service → Mapper → DB
- **L4 分析** — データドリブン最適化：成功率、失敗パターン、エージェントスコア
- **L5 自己最適化** — AI自律最適化：制約調整、スマートルーティング、Git Diff追跡
- **バッチエンキュー** — ダッシュボードから複数バグを一括キュー投入
- **パイプライン進捗** — 13ノード可視化

## ⚡ クイックスタート

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
sudo bash deploy/setup.sh          # ワンクリックデプロイ
```

### 設定

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# 認証情報を入力
```

### 実行

```bash
agentforge scan-bugs                    # 全アクティブバグスキャン
agentforge pipeline --max-bugs 10       # パイプライン修正
agentforge executor --agent zhaoyun     # 単一エージェント起動
agentforge web --port 18081             # ダッシュボード起動
agentforge analytics                    # L4：指標JSON
agentforge report                       # L4：Markdownレポート
agentforge optimize                     # L5：自己最適化
agentforge scores                       # L5：エージェントスコア
```

## 🖥️ ダッシュボード

| ページ | パス | 説明 |
|---|---|---|
| **ダッシュボード** | `/` | 統計カード、最近の修正、パイプライン状態 |
| **バグ一覧** | `/bugs` | 全バグ、リフレッシュ、バッチエンキュー |
| **エージェント詳細** | `/agent/:id` | WebSocketリアルタイムログ、キュー |
| **エージェントシステム** | `/agents` | 8エージェント + L5スコア |
| **L4/L5分析** | `/analytics` | L4指標 + L5最適化記録 |
| **キュー** | `/queues` | パイプライン進捗可視化 |

## 🤖 エージェントシステム

| コード | 名前 | 役割 | 専門分野 |
|---|---|---|---|
| `guanyu` | 関羽 | バックエンド開発 | Java, Spring, API |
| `zhaoyun` | 趙雲 | フロントエンド開発 | Vue, UI |
| `xunyu` | 荀彧 | DBA | データベース, SQL |
| `zhangfei` | 張飛 | テスター | テスト, QA |
| `huatuo` | 華佗 | プロダクトマネージャー | 要件定義 |
| `chenlin` | 陳琳 | ドキュメント担当 | ドキュメント, Wiki |
| `liubei` | 劉備 | プロジェクトマネージャー | 協調管理 |
| `zhugeliang` | 諸葛亮 | アーキテクト | アーキテクチャ設計 |

エージェント設定：`agents/*.yaml` | スキル：`skills/` | デプロイ：`deploy/`

## 📁 プロジェクト構成

```
agentforge-rs/
├── src/core/          # Rust バックエンド (L4/L5 エンジン含む)
├── web/               # Vue 3 + Element Plus フロントエンド
├── agents/            # 8 エージェント YAML 設定
├── skills/            # 8 Harness Engineering スキルファイル
├── codex-config/      # Codex CLI 設定テンプレート
├── deploy/            # systemd サービステンプレート
├── .harness/          # Harness Engineering メタデータ
└── AGENTS.md          # Harness Engineering 総綱
```

## 📊 成熟度モデル

| レベル | 特徴 | 状態 |
|---|---|---|
| L1 初期 | 規範なし | ✅ 超越済み |
| L2 管理 | 基本制約 + フィードバック | ✅ 完了 |
| L3 定義 | 標準化フロー + 自動コミット + Zentao | ✅ 完了 |
| L4 量化 | データドリブン最適化 | ✅ 完了 |
| L5 最適化 | AI自律最適化 | ✅ 完了 |

## 📜 ライセンス

MIT
