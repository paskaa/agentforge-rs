---
name: durable-execution
description: Checkpoint management and state persistence for long-running agents. Use when executing tasks that span multiple steps, need failure recovery, or require idempotent operations. Covers checkpoint strategy, state management layers, event sourcing, and recovery patterns.
---

# 持久化执行 — 检查点与状态管理

> 可靠是规模化的前提。每个长时任务都必须有可恢复的检查点。

## 🎯 何时使用

- 任务预计超过 5 个步骤
- 任务涉及文件修改（需要回滚能力）
- 任务依赖外部服务/API
- 任务需要在中断后恢复

## 📦 三层状态管理

| 层级 | 内容 | 本项目的实现 |
|---|---|---|
| **系统层** | 工作流 ID、超时、重试配置 | update_plan 记录任务 ID 和进度 |
| **执行层** | 当前活动、执行进度、等待事件 | checklist_write 分步骤记录 |
| **业务层** | 已完成工作、中间产物 | git diff + 编译结果作为验证 |

## 🔄 检查点策略

### 触发时机
```
时间触发：每完成 1 个关键步骤
事件触发：编译通过 / 失败后
状态变化：每次代码修改后（git diff 可见）
```

### 检查点内容
```yaml
checkpoint:
  step_id: "string"
  status: "pending | in_progress | completed | failed"
  inputs: { }           # 该步骤的输入参数
  outputs: { }          # 该步骤的产出
  error_message: ""     # 失败原因
  timestamp: "ISO8601"  # 时间戳
```

### 恢复流程
```
失败检测
  → 定位最新检查点（update_plan / checklist）
  → 分析失败原因（编译错误 / 测试失败 / 逻辑错误）
  → git restore 撤销本次修改
  → 从失败点修复（不从头开始）
  → 继续执行
  → 恢复验证（确认状态一致性）
```

## ♻️ 幂等性模式

### 模式 1：唯一标识
每个操作生成唯一 ID，已执行则跳过：
```
generate_code(task_id="abc123")
if executed(task_id="abc123"):
    return cached_result  # 已执行，跳过
```

### 模式 2：状态检查
执行前检查目标是否已达成：
```
if file_exists("src/utils/helper.js"):
    return  # 已生成，跳过
```

### 模式 3：补偿操作
不可逆操作提供回滚机制：
```
try:
    modify_file(path)
except:
    restore_from_git(path)  # 补偿操作
```

## 📝 事件溯源（简化版）

每次操作产生不可变事件记录：

```
事件流（按时间顺序）:
  ① 人类提交任务 → ② Agent 制定计划
  → ③ 修改文件 → ④ 编译检查
  → ⑤ 数据流验证 → ⑥ 人类审查
  → ⑦ 提交代码
```

每次事件可通过 `git log` + `update_plan` 追溯。

## ⚡ 本项目的检查点命令速查

```bash
# 查看当前进度
grep -n "^[0-9]\+\." <(cat AGENTS.md | grep -A 100 "工作流程")

# 回滚最近的修改
git checkout -- <file>

# 查看修改历史
git log --oneline -10

# 查看未提交的变更
git diff --stat HEAD
```

## ⚠️ 常见陷阱

| 陷阱 | 表现 | 解决 |
|---|---|---|
| 不设检查点 | 失败后全部重来 | 步骤间自动 save_checkpoint |
| 幂等性缺失 | 重复执行导致错误 | 唯一 ID / 状态检查 |
| 状态不一致 | 检查点与实际不符 | git diff 验证后再恢复 |
| 检查点过大 | 保存和恢复慢 | 只保存增量状态 |
