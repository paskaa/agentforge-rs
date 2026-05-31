---
name: review-audit
description: Review workflows, audit trails, and compliance checks for AI-generated code. Use when conducting code review, setting up approval workflows, maintaining audit logs, or ensuring compliance. Covers human-in-loop review, trust-scaled approval, compliance checks, and escalation paths.
---

# 审查与审计 — 人类在环的质量守门人

> AI 生成代码的速度再快，也需要人类在关键节点把关。

## 👁️ 三层审查体系

### L1：自审（Agent 自查）
提交前 Agent 对照约束逐条检查：
```yaml
self_review_checklist:
  - "所有修改是否能通过编译？"
  - "是否遵守了命名规范？"
  - "是否添加了类型提示？"
  - "测试覆盖是否达标？"
  - "有没有遗漏的 TODO / DEBUG？"
  - "变更范围是否超出任务边界？"
```

### L2：配对审查（Agent + 人类）
Agent 生成变更摘要，人类做终审：
```yaml
review_summary:
  files_changed: 3
  lines_added: 45
  lines_removed: 12
  coverage_delta: "+5%"
  risk_level: "low"
  key_decisions:
    - "选择了方案 B（性能优先于可读性）"
```

### L3：合规审查（审计追踪）
记录所有 AI 操作，满足合规要求：
```yaml
audit_record:
  agent_id: "codex-v4"
  task_id: "bug-597"
  timestamp: "2026-05-28T14:30:00Z"
  actions:
    - type: "file_modify"
      path: "AdviceManageAppMapper.xml"
      diff: "+7 lines, -2 lines"
  approvals:
    - reviewer: "human"
      decision: "approved"
      timestamp: "2026-05-28T14:35:00Z"
```

## 🔐 信任度比例审查

| 信任等级 | 自审 | 配对审查 | 合规审查 | 说明 |
|---|---|---|---|---|
| L1 怀疑 | 强制 | 逐行 | 强制 | 新 Agent / 新项目 |
| L2 试探 | 强制 | 抽样 30% | 强制 | 当前项目状态 |
| L3 信任 | 强制 | 抽样 10% | 按需 | Agent 可靠性已验证 |
| L4 委托 | 自动 | 仅异常 | 按需 | 高度信任环境 |

## 📋 审查工作流

```
Agent 完成工作
  → 执行自审（对照约束清单）
  → 生成变更摘要（files_changed / coverage / risk）
  → 提交 PR / 变更请求
  → 
  ├→ L1 通过 → 自动合并（低风险 + 高信任）
  ├→ L1 失败 → Agent 修复
  │
  ├→ L3 需要 → 生成审计记录
  │
  └→ 人类审查
      ├→ 批准 → 合并 / 部署
      ├→ 驳回 → 反馈具体问题 → Agent 修复 → 重审
      └→ 指导 → 提供修改方向 → Agent 调整
```

## 🚨 升级路径

```
问题类型          首次                     第2次                    第3次
─────────────────────────────────────────────────────────────────
编译失败           Agent 修复              Agent 修复              上报人类
测试失败           Agent 修复              上报人类                等待指导
逻辑错误           上报人类                等待指导                暂停任务
安全违规           立即暂停                安全团队介入             永久标记
```

## 📊 审查效率指标

| 指标 | 目标 | 说明 |
|---|---|---|
| 审查通过率 | > 80% | 一次提交即通过 |
| 审查时间 | < 30 分钟 | 人类每次审查耗时 |
| 缺陷逃逸率 | < 5% | 生产环境发现的问题 |
| 审计覆盖率 | 100% | 所有 AI 操作均记录 |

## ⚠️ 常见陷阱

| 陷阱 | 表现 | 解决 |
|---|---|---|
| 审查疲劳 | 人类草率批准 | 限制每日审查量，轮换审查人 |
| 过度信任 | 跳过审查直接合并 | 设置强制门禁 |
| 审计缺失 | 无法追溯问题来源 | 每次操作都记录审计事件 |
| 反馈模糊 | "这里不对" — 缺少具体位置 | 文件:行号:错误描述 |
