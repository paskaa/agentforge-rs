# L4/L5 分析

## L4 量化分析

- TraceStore (SQLite): `/var/lib/agentforge/traces.db`
- Agent 成功率、平均修复耗时、失败模式分布、Pipeline 吞吐量
- CLI: `agentforge analytics` / `agentforge report`

## L5 AI 自主优化

| 机制 | 触发条件 | 动作 |
|---|---|---|
| 约束增强 | 成功率 < 50%（≥3次） | 自动补充专项约束 |
| 智能路由 | 按 bug 类型匹配历史最优 Agent | `best_agent_for(bug_type)` |
| 重试策略 | 失败后换提示词/换 Agent | 最多 10 次 |
| 路由调整 | 某 Agent 成功率最低 | 减少分配 |

- 评分维度: 成功率(60%) + 速度(20%) + 类型匹配(20%)
- 持久化: `/var/lib/agentforge/agent_scores.json`
- CLI: `agentforge optimize` / `agentforge scores`
