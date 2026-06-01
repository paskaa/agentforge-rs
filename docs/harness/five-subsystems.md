# WalkingLabs 5 子系统模型

## 1. 指令子系统 (Instruction)

| 文件 | 用途 |
|---|---|
| `AGENTS.md` | 项目铁律、约束、标准工作流 |
| `.harness/PROGRESS.md` | 会话进度 + 已验证状态 |
| `.harness/feature_list.json` | 功能状态唯一事实来源 |
| `.harness/init.sh` | 统一启动入口 |
| `.harness/clean-state-checklist.md` | 结束时的清洁检查 |

## 2. 工具子系统 (Tools)

| 层级 | 工具 | 用途 |
|---|---|---|
| L0 开发 | `cargo build/check/test/clippy` | 编译、测试、质量 |
| L1 Agent | `agentforge executor --agent <name>` | 启动 Agent 主循环 |
| L2 Pipeline | `agentforge pipeline` | 流水线批量修 Bug |
| L3 集成 | Zentao REST API | 禅道操作 |
| L4 辅助 | `zentao-bug-query.sh` | 查询 Bug 详情 |

## 3. 环境子系统 (Environment)

| 组件 | 配置来源 |
|---|---|
| Redis | `config/agentforge.yaml` → `redis://127.0.0.1:16379` |
| Zentao | `config/agentforge.yaml` → `zentao.gentronhealth.com` |
| 飞书 | `config/agentforge.yaml` → app_id + app_secret |
| LLM | `config/agentforge.yaml` → api_base + api_key |
| Git | his-repo: `http://guanyu:GentronHIS2025@192.168.110.253:3000/wangyizhe/his.git` |

## 4. 状态子系统 (State)

| 机制 | 用途 | 持久化 |
|---|---|---|
| `TraceStore` (SQLite) | Agent 活动追踪 | `/var/lib/agentforge/traces.db` |
| `fix_trajectory` | 修复轨迹记录 | Redis Hash |
| `dead_letter` | 失败任务持久化 | Redis List |
| `pipeline:result:{bug_id}` | Pipeline 修复结果 | Redis (24h TTL) |
| `claude_code_lock:{agent}` | Agent 互斥锁 | Redis (1h TTL) |

## 5. 反馈子系统 (Feedback)

| 层级 | 速度 | 命令 | 失败处理 |
|---|---|---|---|
| L1 编译检查 | <10 秒 | `cargo check` | 立即阻断 |
| L1 单元测试 | <5 秒 | `cargo test` | 失败回退，重试 |
| L2 代码质量 | <15 秒 | `cargo clippy` | 警告可忽略，错误阻断 |
| L3 质量门禁 | <30 秒 | `run_quality_gates()` | 编译验证通过才提交 |
| L4 人工审查 | 5-10 分钟 | diff review | 驳回 / 指导 / 批准 |
