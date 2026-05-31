# Harness Engineering Pipeline — 进度报告

## 运行时间
2026-05-31 — 持续运行

## 当前状态
- ✅ 框架完全运行正常
- ✅ 8 个 Fixer Agent 并行修复中
- ✅ Web Dashboard 运行在 18081 端口
- ✅ WebSocket 实时日志推送
- ✅ L4 量化分析 + L5 AI 自优化
- ✅ systemd Restart=always — 崩溃自动恢复

## 最新变更 (2026-05-31)
- ✅ Web Dashboard: Vue3 + Element Plus 全面升级
- ✅ 面板页面: Dashboard, BugList, AgentDetail, Agents, Analytics, Queues
- ✅ WebSocket 实时日志 + Redis pub/sub
- ✅ 批量入列 API + 全部加入队列按钮
- ✅ Bug 严重程度字段 + 禅道链接跳转
- ✅ 流水线进度 13 节点可视化 (PipelineProgress.vue)
- ✅ L5 优化记录 + Git Diff 追踪
- ✅ 失败模式按真实错误类别分类 (SQL/空指针/编译/限流等)
- ✅ 智能体评分保留 2 位小数
- ✅ current_bug_for SQLite 回退修复 (排除已完成 bug)
- ✅ 时区统一为 Asia/Shanghai
- ✅ 8 个智能体独立配置文件 (agents/*.yaml)
- ✅ 8 个 Harness Engineering 技能文件 (skills/)
- ✅ Codex CLI 配置模板 (codex-config/)
- ✅ systemd 服务模板 (deploy/)
- ✅ 多语言 README (EN/CN/JP)

## 智能体配置
| 代号 | 名称 | 角色 | 配置 |
|---|---|---|---|
| guanyu | 关羽 | 后端开发 | agents/guanyu.yaml |
| zhaoyun | 赵云 | 前端开发 | agents/zhaoyun.yaml |
| xunyu | 荀彧 | DBA | agents/xunyu.yaml |
| zhangfei | 张飞 | 测试 | agents/zhangfei.yaml |
| huatuo | 华佗 | 产品经理 | agents/huatuo.yaml |
| chenlin | 陈琳 | 文档专员 | agents/chenlin.yaml |
| liubei | 刘备 | 项目经理 | agents/liubei.yaml |
| zhugeliang | 诸葛亮 | 架构师 | agents/zhugeliang.yaml |

## 技能文件 (skills/)
- harness-engineering — 主方法论
- walkinglabs-harness — 5 子系统模型
- durable-execution — 持久化执行
- closed-loop-testing — 闭环测试
- constraint-design — 约束设计
- review-audit — 审查审计
- karpathy-guidelines — Karpathy 编码准则
- full-chain-fix — 全链路修复

## 服务状态
- agentforge-web.service → :18081
- agentforge-pipeline.service → 批量修复
- agentforge-rust@{agent}.service × 8 → 智能体执行器
- agentforge-optimize.timer → 每 30 分钟 L5 自优化

## 备注
- 失败 bug 会被自动重试
- 锁超过 45 分钟自动清除
- 全部 8 个 Agent 运行正常

## 🔴 2026-06-01 根因分析：Bug 反复修不好的原因

### 数据摘要
| 指标 | 数值 |
|------|------|
| 总 fix_start | 7,830 |
| fix_done (ok) | 793 (10.1%) |
| fix_done (failed) | 2,120 (27.1%) |
| fix_start 无 fix_done（丢失） | 4,917 (62.8%) |
| verification 总运行 | 4 次 |
| verification 全部失败 | 4 次 |
| test_done 通过 | 108/153 (70.6%) |

### 浪费最严重的 Bug
| Bug | Agent | fix_start 次数 | 成功次数 | 成功率 |
|-----|-------|---------------|---------|--------|
| #568 | zhaoyun | 1,627 | 5 | 0.3% |
| #571 | zhaoyun | 1,499 | 2 | 0.1% |
| #579 | zhaoyun | 1,284 | 3 | 0.2% |
| #547 | guanyu | 416 | 9 | 2.2% |
| #537 | guanyu | 388 | 232 | 59.8% |

### 四大根因
1. **Pipeline 无去重** — 不检查禅道状态、develop 分支、Redis 锁，同一 Bug 被反复入列
2. **Executor 任务丢失** — 62.8% 的 fix_start 没有 fix_done，进程崩溃后任务丢失
3. **验证系统形同虚设** — 7830 次修复只触发 4 次验证，且全部失败
4. **禅道状态未同步** — 已修复的 Bug 禅道状态还是 active，流水线不断重新扫描

### 已写入铁律
- 铁律 18: Pipeline 入列前三重检查（禅道状态 + develop 分支 + Redis 锁）
- 铁律 19: 同一 Bug 禁止重复入列
- 铁律 20: 验证不通过禁止进 Pipeline
- 铁律 21: 测试失败禁止关禅道
- 铁律 22: 修复结果必须有全链路证据
- 铁律 23: Executor 崩溃恢复
