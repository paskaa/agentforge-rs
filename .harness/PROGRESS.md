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

## 🔴 2026-06-01 Codex 修复日志分析 — 14 个 Bug 逐条审计

### 总体数据

| Bug | Agent | fix_start | fix_ok | fix_fail | verify_ok | verify_fail | develop commit | 真实状态 |
|-----|-------|-----------|--------|----------|-----------|-------------|----------------|---------|
| #466 | zhaoyun | 10 | 4 | 5 | 0 | 0 | ✅ 有 | 已修+已提交 |
| #467 | zhaoyun | 10 | 3 | 7 | 0 | 0 | ✅ 有 | 已修+已提交 |
| **#610** | **zhaoyun** | **2** | **2** | **0** | **0** | **0** | **❌ 无** | **未修复** |
| #611 | zhaoyun | 5 | 4 | 1 | 0 | 0 | ✅ 有 | 已修+已提交 |
| #613 | zhaoyun | 3 | 2 | 0 | 0 | 0 | ✅ 有 | 已修+已提交 |
| #614 | zhaoyun | 6 | 3 | 2 | 0 | 0 | ✅ 有 | 已修+已提交 |
| #615 | zhaoyun | 7 | 4 | 2 | 0 | 0 | ✅ 有 | 已修+已提交 |
| #616 | zhaoyun | 7 | 2 | 5 | 0 | 0 | ✅ 有 | 已修+已提交 |
| #625 | zhaoyun | 8 | 3 | 3 | 0 | 0 | ✅ 有 | 已修+已提交 |
| #626 | zhaoyun | 12 | 7 | 2 | 0 | 1 | ✅ 有 | 已修+已提交 |
| #627 | zhaoyun | 14 | 9 | 3 | 0 | 1 | ✅ 有 | 已修+已提交 |
| #628 | zhaoyun | 19 | 8 | 6 | 0 | 1 | ✅ 有 | 已修+已提交 |
| #629 | guanyu | 31 | 3 | 15 | 0 | 1 | ✅ 有 | 已修+已提交 |
| #630 | guanyu | 51 | 29 | 3 | 0 | 2 | ✅ 有 | 已修+已提交 |

### Codex 输出分类分析

**Bug#630 (guanyu, 51次fix_start)**:
- 21次空输出（codex 超时/崩溃，无任何输出）
- 4次"其他"（分析中但未产出修复）
- 3次"全链路分析"（分析完成但未改代码）
- 3次"之前修复已完成"（误判，实际仍在修）
- 1次"修复分析"（分析根因但未提交）
- **根因**: `DoctorStationEmrAppServiceImpl` 中 `getOne()` 多条记录异常，codex 反复分析但修复方案不稳定

**Bug#629 (guanyu, 31次fix_start)**:
- 7次"之前修复已完成"（误判 develop 已有修复）
- 5次空输出
- 3次"其他"
- 1次"全链路分析" + 1次"修复分析" + 1次"修复完成"
- **根因**: `RegAdviceSaveDto` 子类字段遮蔽父类 `categoryEnum`，Lombok @Data 导致 Jackson 反序列化失败。codex 多次判断"已修复"但实际只有 commit 没有验证

**Bug#466/#467 (zhaoyun, 各10次)**:
- 大量"其他"输出（分析过程但未产出最终修复）
- **根因**: 检验申请单界面字段缺失，codex 分析了但修复不完整

**Bug#610 (zhaoyun, 2次)**:
- 2次都标记为"修复完成"
- **但 develop 上无 commit** — 修复成功但未提交到远程
- **这是唯一真正未修复的 Bug**

### 五大问题模式

| # | 问题 | 频次 | 影响 |
|---|------|------|------|
| 1 | **空输出** — codex 超时/崩溃无输出 | 36次 | 任务丢失，executor 空跑 |
| 2 | **误判"已修复"** — 发现 develop 有 commit 就跳过 | 15次 | 部分修复被当作完整修复 |
| 3 | **分析不产出** — 长篇分析但不改代码 | 20次 | fix_done ok 但无实际变更 |
| 4 | **验证失败** — 编译/测试不通过 | 5次 | 修复被拦截，无法提交 |
| 5 | **提交丢失** — 修复成功但未 push 到 develop | 1次(#610) | 修复白做 |

### Codex Prompt 分析

发送给 codex 的 prompt 包含：
- 8 个 SKILL.md 文件（harness/walkinglabs/durable-execution/closed-loop-testing/constraint-design/review-audit/karpathy-guidelines/full-chain-fix）
- 完整的禅道 Bug 详情（标题/描述/步骤/截图/备注）
- AGENTS.md 项目规则摘要（前30行）
- L5 自优化器生成的额外约束

**问题**: prompt 过长（多个 skill 文件全量加载），codex 可能因上下文过长而丢失关键信息或超时。

### 建议改进

1. **精简 prompt**: 只加载与当前 bug 类型相关的 skill 文件，不全量加载 8 个
2. **空输出重试**: 空输出时自动重试，不计入 fix_done
3. **误判保护**: "之前修复已完成" 必须附带 `git diff` 证据，否则不允许跳过
4. **提交验证**: fix_done 后检查 worktree 是否有未提交的变更，有则强制 commit + push
5. **#610 专项修复**: 唯一未修复的 bug，需要人工介入或重新分配
