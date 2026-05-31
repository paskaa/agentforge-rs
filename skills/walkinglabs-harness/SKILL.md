---
name: walkinglabs-harness
description: "Practical Harness Engineering patterns from the walkinglabs course. Implements the 5-subsystem model (Instruction, Tools, Environment, State, Feedback) with concrete templates: feature_list.json, claude-progress.md, init.sh, session-handoff.md, clean-state-checklist.md. Use when setting up project harness infrastructure, session continuity, or implementing the init phase pattern."
---

# WalkingLabs Harness — 实战模式

> 来源：[Learn Harness Engineering](https://walkinglabs.github.io/learn-harness-engineering/zh/)

## 🎯 核心概念

### 能力鸿沟（Capability Gap）
模型在基准测试上的表现 vs 真实任务上的表现之间的落差。SWE-bench Verified 50-60% 不意味着真实任务也能达到这个成功率。

### Harness 诱导失败
模型本身能力足够，但因为执行环境有结构性缺陷而失败。Anthropic 对照实验：同一个 prompt + 同一个模型，裸跑 20 分钟 $9 失败，带 harness 跑 6 小时 $200 成功。

### 验证缺口
Agent 对自己输出的信心评估和实际正确性之间的偏差。"做完了" ≠ 真的做完了。

### 上下文焦虑
当 Agent 感觉上下文快满了，会匆忙结束当前工作，跳过验证，选简单方案而非最优方案。

## 🔧 5 子系统模型

```
┌─────────────────────────────────────────────┐
│  指令（Instruction）— AGENTS.md / CLAUDE.md  │
│  工具（Tools）— shell / 文件 / 测试          │
│  环境（Environment）— 依赖 / 服务 / 版本     │
│  状态（State）— PROGRESS.md / 功能清单       │
│  反馈（Feedback）— test / lint / build       │
└─────────────────────────────────────────────┘
```

**量化价值的方法：** 控制变量排除法 — 保持模型不变，逐个移除子系统，看哪个移除后性能下降最多。

## 📋 标准工作循环

```
开始会话
  ├→ 1. Init（初始化）
  │   ├── pwd 确认目录
  │   ├── 读取 claude-progress.md / feature_list.json
  │   ├── git log --oneline -5
  │   └── 运行 ./init.sh
  │
  ├→ 2. Select（选择功能）
  │   └── 只选一个未完成功能
  │
  ├→ 3. Implement（实现）
  │   └── 一次只做一个功能，不扩大范围
  │
  ├→ 4. Verify（验证）
  │   ├── 运行验证命令
  │   └── 有可运行证据才标记完成
  │
  └→ 5. Cleanup（清理）
      ├── 更新 claude-progress.md
      ├── 更新 feature_list.json
      ├── 运行 clean-state-checklist.md
      ├── git commit
      └── 留下干净重启路径
```

## 📁 必需模板文件

| 文件 | 用途 | 必需 |
|---|---|---|
| `feature_list.json` | 功能状态唯一事实来源 | ✅ |
| `PROGRESS.md` | 会话进度和已验证状态 | ✅ |
| `init.sh` | 统一启动与验证入口 | ✅ |
| `session-handoff.md` | 跨会话交接摘要 | 可选 |
| `clean-state-checklist.md` | 结束时清洁检查 | ✅ |
| `evaluator-rubric.md` | 评审评分表 | 可选 |
| `quality-document.md` | 质量快照 | 可选 |

## 📐 约束设计

### DoD（完成定义）
一个功能只有在以下条件都满足时才算完成：
1. 目标行为已经实现
2. 要求的验证真的跑过
3. 证据记录在 feature_list.json 或 PROGRESS.md
4. 仓库仍能按标准启动路径重新开始工作

### 规则
- 一次只做一个功能
- 不要因为"代码已经写了"就把功能标记为完成
- 不要悄悄改弱验证规则
- 优先依赖仓库里的持久化文件，而不是聊天记录

## 🔄 会话交接

每次会话结束前必须运行 `clean-state-checklist.md`：
```
□ 标准启动路径仍然可用
□ 标准验证路径仍然可运行
□ 当前进度已记录到进度日志
□ 功能状态真实反映 passing 和未验证的边界
□ 无半成品步骤处于未记录状态
□ 下一轮会话无需人工修复即可继续
```

## 📊 评审评分表

| 维度 | 问题 | 0-2分 |
|---|---|---|
| 正确性 | 行为是否符合目标功能？ | |
| 验证 | 检查是否真的跑过并留下证据？ | |
| 范围纪律 | 是否基本保持在选定功能范围内？ | |
| 可靠性 | 结果能否在重启后继续工作？ | |
| 可维护性 | 代码和文档是否清楚到可交接？ | |
| 交接准备度 | 新会话能否只靠仓库内工件继续推进？ | |

## 📚 12 讲大纲速查

| 讲 | 标题 | 核心观点 |
|---|---|---|
| L01 | 模型强 ≠ 执行可靠 | 能力鸿沟、harness 诱导失败 |
| L02 | Harness 到底是什么 | 5 子系统模型 |
| L03 | 仓库即记录系统 | Agent 看不到的就不存在 |
| L04 | 一个大文件不行 | 100 行 AGENTS.md，更多放 docs/ |
| L05 | 跨会话连续性 | 缺少持久化，30 分钟+ 任务失败率激增 |
| L06 | Init 需要独立阶段 | 启动检查标准化 |
| L07 | Agent 越界/半途而废 | 显式范围边界 + 完成定义 |
| L08 | 功能清单是原语 | feature_list.json 机器可读 |
| L09 | Agent 过早宣告胜利 | 自评 ≠ 验证，分开"干活"和"检查" |
| L10 | 端到端测试改变结果 | 集成测试发现单元测试漏掉的问题 |
| L11 | 可观测性属于 Harness | 日志、追踪、指标在 Harness 内 |
| L12 | 每次会话留下干净状态 | 技术债持续还，不攒到爆雷 |

## ⚠️ 常见陷阱

| 陷阱 | 表现 | 解决 |
|---|---|---|
| 无 Init 阶段 | Agent 直接开工，不知道环境状态 | 强制运行 init.sh |
| 无功能清单 | Agent 凭记忆工作 | 维护 feature_list.json |
| 无进度文件 | 跨会话从头探索 | 维护 PROGRESS.md |
| 不跑验证 | Agent 自己说了算 | 验证命令写在 AGENTS.md 里 |
| 不清理 | 技术债指数级累积 | 每次结束运行 clean-state-checklist |
| 同时做多个功能 | 上下文混乱，范围蔓延 | 一次做一个 |
