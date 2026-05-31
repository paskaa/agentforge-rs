---
name: closed-loop-testing
description: Quality gates, test automation, and feedback loops for AI-generated code. Use when generating tests, running validation pipelines, setting up quality gates, or implementing auto-fix loops for test failures. Covers L1-L5 quality gates, mutation testing, coverage strategies, and failure analysis.
---

# 闭环测试 — 质量门禁与反馈循环

> 没有门禁的 Agent 是不受控的。每层门禁捕获特定类型的问题。

## 🚪 五层质量门禁

| 门禁 | 时间 | 范围 | 失败处理 |
|---|---|---|---|
| **L1 编译检查** | <30 秒 | 语法、类型、导入 | Agent 自行修复 |
| **L2 静态分析** | <2 分钟 | 代码风格、复杂度、安全 | Agent 修复 |
| **L3 单元测试** | <5 分钟 | 功能正确性、边界条件 | 自动修复或上报 |
| **L4 集成测试** | <15 分钟 | 模块间交互、数据流 | 上报人工 |
| **L5 生产验证** | 持续 | 监控、告警、性能 | 自动回滚 |

## 🔄 反馈循环机制

```
测试失败
  → 分析失败原因（编译/逻辑/边界/依赖）
  → 提取可行动的反馈（文件:行号:错误类型:修复方向）
  → Agent 修复
  → 重测
  → 若持续失败（3次）→ 上报人类
```

### 反馈格式规范
```
文件路径:行号 错误类型 错误描述 | 修复建议
示例:
src/payment/applepay_processor.py:42 TypeError amount must be int | 添加类型转换 str → int
```

## 🧪 测试生成策略

### 策略优先级
1. **边界值分析** — 测试空值、越界、特殊值
2. **等价类划分** — 覆盖每个分支路径
3. **错误猜测** — 基于经验预测常见错误
4. **组合测试** — 参数组合爆炸场景

### 覆盖率目标
```yaml
unit_test_coverage: 90%      # 行覆盖率
mutation_score: 80%          # 变异测试通过率
branch_coverage: 85%         # 分支覆盖率
```

## 📊 失败原因分析（基于项目数据）

| 类型 | 占比 | 典型表现 | 捕获门禁 |
|---|---|---|---|
| 架构错误 | 35% | 接口不匹配、依赖错乱 | L1 编译 |
| 业务逻辑 | 25% | 条件判断错误、数据流断裂 | L3 单元测试 |
| 创造性偏差 | 20% | 过度设计、不必要的抽象 | L3 + L5 |
| Debug 残留 | 15% | print、临时变量未清理 | L2 静态分析 |
| 其他 | 5% | 环境、工具问题 | L5 |

## ⚙️ 本项目测试命令

```bash
# L1 编译检查（Java）
cd /root/.openclaw/workspace/his-repo/openhis-server-new
mvn compile -pl openhis-application -am

# L1 语法检查（Python）
python3 -c "import py_compile; py_compile.compile('strategy.py', doraise=True)"

# L2 全链路验证
# 检查 AGENTS.md 中的铁律清单：录入 → 保存 → 查询 → 修改 → 删除 → 关联
```

## ⚠️ 常见陷阱

| 陷阱 | 表现 | 解决 |
|---|---|---|
| 测试滞后 | Agent 写完代码再补测试 | 测试先行或并行生成 |
| 覆盖幻觉 | 覆盖率数字达标但逻辑没测 | 引入变异测试 |
| 反馈过载 | 同时报太多错误 | 按优先级逐层暴露 |
| Mock 泛滥 | 过度 Mock 导致测试失真 | 优先写集成测试 |
