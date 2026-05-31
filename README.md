<p align="center">
  <img src="https://img.shields.io/badge/Rust-2021-blue?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/version-0.1.0-green" alt="Version">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License">
  <img src="https://img.shields.io/badge/agents-8-orange" alt="Agents">
  <img src="https://img.shields.io/badge/model-mimo--v2.5-purple" alt="Model">
</p>

<h1 align="center">AgentForge-RS</h1>

<p align="center">
  <strong>Multi-Agent Bug Fixing Framework — Rust Rewrite</strong><br>
  <em>模型决定上限，Harness 决定底线。</em>
</p>

<p align="center">
  <a href="#-features">Features</a> •
  <a href="#-architecture">Architecture</a> •
  <a href="#-quick-start">Quick Start</a> •
  <a href="#-configuration">Configuration</a> •
  <a href="#-cli-commands">CLI</a> •
  <a href="#-agent-system">Agents</a> •
  <a href="#-contributing">Contributing</a>
</p>

---

## 🌐 Language / 语言 / 言語

| [English](#-english) | [简体中文](#-简体中文) | [日本語](#-日本語) |
|:---:|:---:|:---:|

---

# 🇬🇧 English

## Overview

AgentForge-RS is a **multi-agent automated bug fixing framework** built in Rust. It orchestrates 8 AI agents to scan, diagnose, fix, test, and document bugs from Zentao (project management), with automatic git commits, quality gates, and Feishu notifications.

## ✨ Features

- **8 Specialized Agents** — Each with defined roles (backend, frontend, DBA, testing, etc.)
- **Automated Pipeline** — Scans Zentao bugs → queues to Redis → agents consume → Codex fixes → quality gates → commit + push → Zentao comments
- **Quality Gates** — Compilation checks, SQL validation, code review before commit
- **Zentao Integration** — Read bugs, add structured comments, manage assignments
- **Feishu Notifications** — Real-time alerts on fix progress
- **Git Worktree Isolation** — Each agent works in its own worktree, no conflicts
- **Dead Letter Queue** — Failed tasks preserved for retry
- **Full-Chain Fix** — Traces data flow across frontend → controller → service → mapper → DB

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────┐
│                    Pipeline (CLI)                     │
│  scan Zentao → queue to Redis → poll for results     │
└──────────────┬──────────────────────────┬────────────┘
               │                          │
    ┌──────────▼──────────┐    ┌──────────▼──────────┐
    │   Agent Executor     │    │   Agent Executor     │
    │   (guanyu/zhaoyun)   │    │   (xunyu/huatuo)     │
    │                      │    │                      │
    │  consume from Redis  │    │  consume from Redis  │
    │  → build prompt      │    │  → build prompt      │
    │  → spawn Codex       │    │  → spawn Codex       │
    │  → quality gates     │    │  → quality gates     │
    │  → git commit+push   │    │  → git commit+push   │
    │  → zentao comment    │    │  → zentao comment    │
    └──────────────────────┘    └──────────────────────┘
               │                          │
    ┌──────────▼──────────────────────────▼──────────┐
    │              Redis (Streams + Queues)            │
    │  agent-work-queue:fix:<agent>                   │
    │  codex_lock:<agent>  pipeline:result:<bug_id>   │
    └─────────────────────────────────────────────────┘
```

## ⚡ Quick Start

### Prerequisites

- Rust 1.75+
- Redis 6+
- Node.js 18+ (for Zentao CLI)
- Git

### Build

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
cargo build --release
```

### Configure

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# Edit config/agentforge.yaml with your credentials
```

### Run

```bash
# Scan all active bugs
cargo run -- scan-bugs

# Pipeline: fix up to 5 bugs sequentially
cargo run -- pipeline --max-bugs 5

# Start a single agent
cargo run -- executor --agent zhaoyun

# Query a specific bug
cargo run -- query-bug --bug-id 630
```

## ⚙️ Configuration

All configuration lives in `config/agentforge.yaml` (git-ignored). See `config/agentforge.yaml.example` for the template.

| Section | Description |
|---|---|
| `redis` | Redis connection (host, port, db, auth) |
| `llm` | LLM API endpoint and model settings |
| `feishu` | Feishu (Lark) bot credentials |
| `zentao` | Zentao instance URL, CLI path, credentials |
| `database` | PostgreSQL connection for SQL validation |
| `agents` | Agent definitions (name, role, expertise) |

## 🖥️ CLI Commands

| Command | Description |
|---|---|
| `scan-bugs` | Scan all active bugs from Zentao |
| `pipeline --max-bugs N` | Run the bug-fixing pipeline (N bugs max) |
| `executor --agent <name>` | Start an agent executor |
| `query-bug --bug-id N` | Query bug details from Zentao |

## 🤖 Agent System

| Agent | Name | Role | Expertise |
|---|---|---|---|
| `guanyu` | 关羽 (Guan Yu) | Backend Dev | Java, Spring, API |
| `zhaoyun` | 赵云 (Zhao Yun) | Frontend Dev | Vue, UI |
| `xunyu` | 荀彧 (Xun Yu) | DBA | Database, SQL |
| `zhangfei` | 张飞 (Zhang Fei) | Tester | Testing, QA |
| `huatuo` | 华佗 (Hua Tuo) | Product Manager | Requirements |
| `chenlin` | 陈琳 (Chen Lin) | Documentation | Docs, Wiki |
| `liubei` | 刘备 (Liu Bei) | Project Manager | Coordination |
| `zhugeliang` | 诸葛亮 (Zhuge Liang) | Architect | Architecture, Design |

## 🔧 Tech Stack

| Component | Technology |
|---|---|
| Language | Rust 2021 |
| Async Runtime | Tokio |
| Message Queue | Redis Streams |
| HTTP Client | Reqwest |
| SQL Parser | sqlparser-rs |
| XML Parser | quick-xml |
| Serialization | Serde (JSON/YAML) |
| CLI Framework | Clap |
| Logging | Tracing + Tracing-subscriber |

## 📁 Project Structure

```
agentforge-rs/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library root
│   ├── config/              # Configuration layer
│   ├── core/                # Core logic
│   │   ├── coordinator.rs   # Pipeline orchestration
│   │   ├── executor.rs      # Agent executor loop
│   │   ├── subagent.rs      # Codex integration + fix logic
│   │   ├── llm.rs           # LLM API client
│   │   ├── zentao.rs        # Zentao API client
│   │   ├── sql_validator.rs # SQL validation (MyBatis)
│   │   ├── dead_letter.rs   # Failed task persistence
│   │   ├── fix_trajectory.rs# Fix trajectory tracking
│   │   └── trace.rs         # Activity tracing (SQLite)
│   ├── network/             # External integrations
│   │   ├── feishu.rs        # Feishu (Lark) bot
│   │   └── ws_listener.rs   # WebSocket listener
│   └── tools/               # Tool definitions
├── config/                  # Configuration files (git-ignored)
├── .harness/                # Harness Engineering state
├── AGENTS.md                # Agent methodology docs
└── Cargo.toml
```

## 📜 License

MIT

---

# 🇨🇳 简体中文

## 概述

AgentForge-RS 是一个用 Rust 编写的**多智能体自动化 Bug 修复框架**。它协调 8 个 AI 智能体，从禅道（项目管理系统）扫描、诊断、修复、测试并记录 Bug，支持自动 Git 提交、质量门禁和飞书通知。

## ✨ 特性

- **8 个专业智能体** — 各司其职（后端、前端、DBA、测试等）
- **自动化流水线** — 扫描禅道 Bug → Redis 队列 → 智能体消费 → Codex 修复 → 质量门禁 → 提交推送 → 禅道备注
- **质量门禁** — 编译检查、SQL 校验、代码审查后再提交
- **禅道集成** — 读取 Bug、添加结构化备注、管理分配
- **飞书通知** — 修复进度实时推送
- **Git Worktree 隔离** — 每个智能体独立工作树，互不冲突
- **死信队列** — 失败任务持久化，支持重试
- **全链路修复** — 追踪前端 → Controller → Service → Mapper → DB 数据流

## 🏗️ 架构

```
┌─────────────────────────────────────────────────────┐
│                    流水线 (CLI)                       │
│  扫描禅道 → 入队 Redis → 轮询结果                      │
└──────────────┬──────────────────────────┬────────────┘
               │                          │
    ┌──────────▼──────────┐    ┌──────────▼──────────┐
    │   智能体执行器        │    │   智能体执行器        │
    │   (guanyu/zhaoyun)   │    │   (xunyu/huatuo)     │
    │                      │    │                      │
    │  从 Redis 消费任务    │    │  从 Redis 消费任务    │
    │  → 构建提示词         │    │  → 构建提示词         │
    │  → 调用 Codex        │    │  → 调用 Codex        │
    │  → 质量门禁           │    │  → 质量门禁           │
    │  → Git 提交+推送      │    │  → Git 提交+推送      │
    │  → 禅道备注           │    │  → 禅道备注           │
    └──────────────────────┘    └──────────────────────┘
               │                          │
    ┌──────────▼──────────────────────────▼──────────┐
    │           Redis (Streams + Queues)              │
    └─────────────────────────────────────────────────┘
```

## ⚡ 快速开始

### 环境要求

- Rust 1.75+
- Redis 6+
- Node.js 18+（禅道 CLI 依赖）
- Git

### 构建

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
cargo build --release
```

### 配置

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# 编辑 config/agentforge.yaml 填入你的凭据
```

### 运行

```bash
# 扫描所有活跃 Bug
cargo run -- scan-bugs

# 流水线：按顺序修复最多 5 个 Bug
cargo run -- pipeline --max-bugs 5

# 启动单个智能体
cargo run -- executor --agent zhaoyun

# 查询指定 Bug
cargo run -- query-bug --bug-id 630
```

## ⚙️ 配置说明

所有配置在 `config/agentforge.yaml`（已 git 忽略）。模板见 `config/agentforge.yaml.example`。

| 配置段 | 说明 |
|---|---|
| `redis` | Redis 连接（主机、端口、数据库、认证） |
| `llm` | LLM API 端点和模型设置 |
| `feishu` | 飞书机器人凭据 |
| `zentao` | 禅道实例 URL、CLI 路径、凭据 |
| `database` | PostgreSQL 连接（SQL 校验用） |
| `agents` | 智能体定义（名称、角色、专长） |

## 🖥️ CLI 命令

| 命令 | 说明 |
|---|---|
| `scan-bugs` | 从禅道扫描所有活跃 Bug |
| `pipeline --max-bugs N` | 运行 Bug 修复流水线（最多 N 个） |
| `executor --agent <name>` | 启动智能体执行器 |
| `query-bug --bug-id N` | 查询禅道 Bug 详情 |

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

## 📜 许可证

MIT

---

# 🇯🇵 日本語

## 概要

AgentForge-RSはRustで構築された**マルチエージェント自動バグ修正フレームワーク**です。8つのAIエージェントを連携させ、Zentao（プロジェクト管理システム）からバグをスキャン、診断、修正、テストし、構造化されたコメントを自動で追加します。Gitコミット、品質ゲート、Feishu通知に対応しています。

## ✨ 特徴

- **8つの専門エージェント** — 各エージェントが専門分野を持つ（バックエンド、フロントエンド、DBA、テストなど）
- **自動パイプライン** — Zentaoバグスキャン → Redisキュー → エージェント消費 → Codex修正 → 品質ゲート → コミット+プッシュ → Zentaoコメント
- **品質ゲート** — コンパイルチェック、SQL検証、コードレビュー後にコミット
- **Zentao連携** — バグ読み取り、構造化コメント追加、アサイン管理
- **Feishu通知** — 修正進捗のリアルタイム通知
- **Git Worktree分離** — 各エージェントが独立したWorktreeで作業、競合なし
- **デッドレターキュー** — 失敗したタスクの永続化とリトライ対応
- **フルチェーン修正** — フロントエンド → Controller → Service → Mapper → DBのデータフロー追跡

## 🏗️ アーキテクチャ

```
┌─────────────────────────────────────────────────────┐
│                 パイプライン (CLI)                     │
│  Zentaoスキャン → Redisキュー登録 → 結果ポーリング       │
└──────────────┬──────────────────────────┬────────────┘
               │                          │
    ┌──────────▼──────────┐    ┌──────────▼──────────┐
    │   エージェント実行器   │    │   エージェント実行器   │
    │   (guanyu/zhaoyun)   │    │   (xunyu/huatuo)     │
    │                      │    │                      │
    │  Redisからタスク消費  │    │  Redisからタスク消費  │
    │  → プロンプト構築     │    │  → プロンプト構築     │
    │  → Codex呼び出し     │    │  → Codex呼び出し     │
    │  → 品質ゲート        │    │  → 品質ゲート        │
    │  → Gitコミット+プッシュ│    │  → Gitコミット+プッシュ│
    │  → Zentaoコメント    │    │  → Zentaoコメント    │
    └──────────────────────┘    └──────────────────────┘
```

## ⚡ クイックスタート

### 前提条件

- Rust 1.75+
- Redis 6+
- Node.js 18+（Zentao CLI用）
- Git

### ビルド

```bash
git clone https://github.com/paskaa/agentforge-rs.git
cd agentforge-rs
cargo build --release
```

### 設定

```bash
cp config/agentforge.yaml.example config/agentforge.yaml
# config/agentforge.yaml を編集して認証情報を入力
```

### 実行

```bash
# 全てのアクティブなバグをスキャン
cargo run -- scan-bugs

# パイプライン：最大5つのバグを順次修正
cargo run -- pipeline --max-bugs 5

# 単一エージェントを起動
cargo run -- executor --agent zhaoyun

# 特定バグを照会
cargo run -- query-bug --bug-id 630
```

## ⚙️ 設定項目

全ての設定は `config/agentforge.yaml`（git除外）に格納。テンプレートは `config/agentforge.yaml.example` を参照。

| セクション | 説明 |
|---|---|
| `redis` | Redis接続（ホスト、ポート、DB、認証） |
| `llm` | LLM APIエンドポイントとモデル設定 |
| `feishu` | Feishu（Lark）ボット認証情報 |
| `zentao` | ZentaoインスタンスURL、CLIパス、認証情報 |
| `database` | PostgreSQL接続（SQL検証用） |
| `agents` | エージェント定義（名前、役割、専門分野） |

## 🖥️ CLI コマンド

| コマンド | 説明 |
|---|---|
| `scan-bugs` | Zentaoから全アクティブバグをスキャン |
| `pipeline --max-bugs N` | バグ修正パイプライン実行（最大N件） |
| `executor --agent <name>` | エージェント実行器を起動 |
| `query-bug --bug-id N` | Zentaoバグ詳細を照会 |

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
| `zhugeliang` | �諸葛亮 | アーキテクト | アーキテクチャ設計 |

## 📜 ライセンス

MIT
