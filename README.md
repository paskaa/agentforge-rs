<p align="center">
  <img src="https://img.shields.io/badge/Rust-2021-blue?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.1.0-green" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License">
  <img src="https://img.shields.io/badge/agents-8-orange" alt="Agents">
  <img src="https://img.shields.io/badge/model-mimo--v2.5-purple" alt="Model">
  <img src="https://img.shields.io/badge/maturity-L5-brightgreen" alt="Maturity L5">
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

AgentForge-RS is a **multi-agent automated bug fixing framework** built in Rust. It orchestrates 8 AI agents to scan, diagnose, fix, test, and document bugs from Zentao, with automatic git commits, quality gates, and Feishu notifications.

**Harness Engineering Maturity: L5 (Fully Optimized)**

## ✨ Features

- **8 Specialized Agents** — Backend, Frontend, DBA, Tester, PM, Docs, Coordination, Architect
- **Automated Pipeline** — Scan → Queue → Fix → Quality gates → Commit + push → Zentao comments
- **Quality Gates** — Compilation, SQL validation, code review before commit
- **Zentao Integration** — Read bugs, structured comments, assignment management
- **Feishu Notifications** — Real-time alerts
- **Git Worktree Isolation** — Each agent has its own worktree
- **Dead Letter Queue** — Failed tasks preserved for retry
- **Full-Chain Fix** — Frontend → Controller → Service → Mapper → DB
- **L4 Analytics** — Data-driven: success rates, failure patterns, agent scoring
- **L5 Self-Optimizer** — AI auto-tuning: constraints, smart routing, retry strategy

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────┐
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
    └──────────────────────┘    └──────────────────────┘
               │                          │
    ┌──────────▼──────────────────────────▼──────────┐
    │         L4 Analytics + L5 Self-Optimizer         │
    │  TraceStore → Metrics → Auto-tune → Scores      │
    └─────────────────────────────────────────────────┘
```

## ⚡ Quick Start

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
sudo bash deploy/setup.sh          # one-click deploy
# or
cargo build --release
cp target/release/agentforge /usr/local/bin/agentforge
```

### Configure

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# edit with your credentials
```

### Run

```bash
agentforge scan-bugs                    # scan all active bugs
agentforge pipeline --max-bugs 10       # fix pipeline
agentforge executor --agent zhaoyun     # start single agent
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
| `query-bug --bug-id N` | Query bug details |
| `analytics` | L4: Metrics JSON from TraceStore |
| `report` | L4: Markdown analytics report |
| `optimize` | L5: Self-optimizer analysis |
| `scores` | L5: Agent performance scores |

## 🤖 Agent System

| Agent | Name | Role | Expertise |
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

## 📊 Maturity

| Level | Feature | Status |
|---|---|---|
| L1 Initial | No standards | ✅ Exceeded |
| L2 Managed | Basic constraints + feedback | ✅ Done |
| L3 Defined | Standardized flow + auto-commit + Zentao | ✅ Done |
| L4 Quantitative | Data-driven optimization | ✅ Done |
| L5 Optimizing | AI self-optimization | ✅ Done |

## 📁 Project Structure

```
agentforge-rs/
├── src/                    # Rust source
│   ├── core/
│   │   ├── coordinator.rs  # Pipeline orchestration
│   │   ├── executor.rs     # Agent executor loop
│   │   ├── subagent.rs     # Codex integration + prompts
│   │   ├── analytics.rs    # L4: Metrics
│   │   ├── report.rs       # L4: Reports
│   │   ├── self_optimizer.rs # L5: Auto-tuning
│   │   ├── trace.rs        # SQLite trace store
│   │   └── zentao.rs       # Zentao API client
│   └── network/            # Feishu + WebSocket
├── agents/                 # 8 agent YAML configs
├── skills/                 # 8 Harness Engineering skills
├── codex-config/           # Codex CLI config templates
├── deploy/                 # systemd + setup scripts
├── config/                 # Runtime config (git-ignored)
├── .harness/               # Harness state files
└── AGENTS.md               # Methodology docs
```

## 🔧 Tech Stack

Rust 2021 | Tokio | Redis Streams | Reqwest | sqlparser-rs | quick-xml | Serde | Clap | Tracing | SQLite

## 📜 License

MIT

---

# 🇨🇳 简体中文

## 概述

AgentForge-RS 是用 Rust 编写的**多智能体自动化 Bug 修复框架**。协调 8 个 AI 智能体，从禅道扫描、诊断、修复、测试并记录 Bug，支持自动 Git 提交、质量门禁和飞书通知。

**Harness Engineering 成熟度：L5（完全优化）**

## ✨ 特性

- **8 个专业智能体** — 各司其职
- **自动化流水线** — 扫描 → 队列 → 修复 → 门禁 → 提交 → 禅道备注
- **质量门禁** — 编译检查、SQL 校验、代码审查
- **禅道集成** — 读取 Bug、结构化备注、分配管理
- **飞书通知** — 实时推送
- **Git Worktree 隔离** — 互不冲突
- **死信队列** — 失败任务持久化
- **全链路修复** — 前端 → Controller → Service → Mapper → DB
- **L4 量化分析** — 成功率、失败模式、智能体评分
- **L5 自主优化** — 约束调整、智能路由、重试策略

## ⚡ 快速开始

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
sudo bash deploy/setup.sh          # 一键部署
```

### 配置

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# 编辑填入凭据
```

### 运行

```bash
agentforge scan-bugs                    # 扫描所有活跃 Bug
agentforge pipeline --max-bugs 10       # 流水线修复
agentforge executor --agent zhaoyun     # 启动单个智能体
agentforge analytics                    # L4：指标 JSON
agentforge report                       # L4：Markdown 报告
agentforge optimize                     # L5：自优化分析
agentforge scores                       # L5：智能体评分
```

## 🖥️ CLI 命令

| 命令 | 说明 |
|---|---|
| `scan-bugs` | 从禅道扫描所有活跃 Bug |
| `pipeline --max-bugs N` | 运行 Bug 修复流水线 |
| `executor --agent <name>` | 启动智能体执行器 |
| `query-bug --bug-id N` | 查询 Bug 详情 |
| `analytics` | L4：从 TraceStore 生成指标 |
| `report` | L4：生成 Markdown 分析报告 |
| `optimize` | L5：自优化分析 + 建议 |
| `scores` | L5：智能体性能评分 |

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

## 📊 成熟度

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

AgentForge-RSはRustで構築された**マルチエージェント自動バグ修正フレームワーク**です。8つのAIエージェントを連携させ、Zentaoからバグをスキャン、診断、修正、テストし、自動コミット、品質ゲート、Feishu通知に対応しています。

**Harness Engineering 成熟度：L5（完全最適化）**

## ✨ 特徴

- **8つの専門エージェント** — バックエンド、フロントエンド、DBA、テスト、PM、ドキュメント、協調、アーキテクト
- **自動パイプライン** — スキャン → キュー → 修正 → 品質ゲート → コミット+プッシュ → Zentaoコメント
- **品質ゲート** — コンパイル、SQL検証、コードレビュー
- **Zentao連携** — バグ読み取り、構造化コメント、アサイン管理
- **Feishu通知** — リアルタイム通知
- **Git Worktree分離** — 競合なし
- **デッドレターキュー** — 失敗タスク永続化
- **フルチェーン修正** — フロントエンド → Controller → Service → Mapper → DB
- **L4 分析** — データドリブン最適化：成功率、失敗パターン、エージェントスコア
- **L5 自己最適化** — AI自律最適化：制約調整、スマートルーティング、リトライ戦略

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
agentforge analytics                    # L4：指標JSON
agentforge report                       # L4：Markdownレポート
agentforge optimize                     # L5：自己最適化
agentforge scores                       # L5：エージェントスコア
```

## 🖥️ CLI コマンド

| コマンド | 説明 |
|---|---|
| `scan-bugs` | Zentaoから全アクティブバグスキャン |
| `pipeline --max-bugs N` | バグ修正パイプライン |
| `executor --agent <name>` | エージェント実行器起動 |
| `query-bug --bug-id N` | バグ詳細照会 |
| `analytics` | L4：TraceStoreから指標生成 |
| `report` | L4：Markdown分析レポート |
| `optimize` | L5：自己最適化分析 |
| `scores` | L5：エージェントパフォーマンススコア |

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
