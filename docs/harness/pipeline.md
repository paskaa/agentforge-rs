# 管线流程详解

## 完整管线

```
fix_done (guanyu/zhaoyun)
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
  └── 失败 → 回退给修复者重修（最多10次）
```

## 禅道操作规则

| 阶段 | 智能体 | 禅道操作 |
|---|---|---|
| 分析路由 | 诸葛亮 | 添加备注（分析结果、路由决策） |
| DB审查 | 荀彧 | 添加备注（审查结果、风险评估） |
| 测试 | 张飞 | 添加测试报告 + resolve |
| 验收 | 华佗 | 添加备注 + resolve + assign 给提出人 |
| 归档 | 陈琳 | 添加备注（全流程完成记录） |

## 去重机制

- `pipeline_sent:{bug_id}` — 24h TTL，防止重复触发管线
- `pipeline_retry:{bug_id}` — 重试计数器
- `codex_lock:{agent}` — 1h TTL，Agent 互斥锁
- `fix_active:{agent}:{bug_id}` — 30min TTL，防止重复 fix_start

## BDT 方法论

1. 获取 Bug — 从禅道获取完整详情
2. 设计测试用例 — 根据 Bug 描述生成 Playwright 测试脚本
3. 基线测试 — 修复前运行测试确认 Bug 存在
4. 修复代码 — 全链路验证修复
5. 回归测试 — 运行 Playwright 测试确认修复有效
6. 全链路验证 — 前端 → API → DB → 关联模块
