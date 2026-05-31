---
name: constraint-design
description: Design patterns for writing effective Agent constraints. Use when defining rules, policies, or guardrails for AI code generation. Covers constraint types (architectural/code quality/security/business), DSL patterns, conflict resolution, and progressive trust scaling.
---

# 约束设计 — 让 Agent 在边界内发挥

> 好的约束不是束缚，是护栏。它让 Agent 在安全区域内自由发挥。

## 📐 四类约束

### 1. 架构约束
定义系统结构和组件关系：
```yaml
architecture:
  required_interface: "PaymentProcessor"  # 必须实现
  file_location: "src/payment/"           # 文件位置
  naming_convention: "{name}_processor.py"
  forbidden_patterns:
    - "god_class"     # 禁止上帝类
    - "circular_dep"  # 禁止循环依赖
```

### 2. 代码质量约束
定义代码质量和风格标准：
```yaml
code_quality:
  max_complexity: 10          # 圈复杂度上限
  max_line_length: 120        # Java / 100（Vue）
  type_hints: "required"      # 必须类型提示
  docstrings: "public_only"   # 公共方法需要文档
  test_coverage: 90           # 覆盖率目标
```

### 3. 安全约束
防止 Agent 引入安全隐患：
```yaml
security:
  no_hardcoded_secrets: true
  use_vault_for_credentials: true
  input_sanitization: "strict"
  forbidden_functions:
    - "eval()"
    - "exec()"
```

### 4. 业务规则
领域特定的约束：
```yaml
business:
  data_flow: "full_chain"     # 必须走通全链路
  soft_delete: true           # 软删除机制
  audit_log: true             # 操作审计
```

## 🎯 约束设计原则

### 原则 1：可验证
每条约束必须能被自动化检查：
```
✅ "代码覆盖率 > 90%" — 可用 pytest-cov 验证
❌ "代码质量要高" — 无法验证
```

### 原则 2：无歧义
约束必须精确，不留解读空间：
```
✅ "每函数不超过 50 行"
❌ "函数不要太长"
```

### 原则 3：优先级排序
约束冲突时按优先级裁决：
```
安全(1) > 架构(2) > 业务(3) > 质量(4) > 性能(5)
```

### 原则 4：渐进增强
从最少的约束开始，按需增加：
```
L1: 编译通过
L2: + 类型提示 + 命名规范
L3: + 测试覆盖 + 复杂度限制
L4: + 安全扫描 + 架构合规
```

## 🔧 约束 DSL 模式

### 声明式（推荐）
```yaml
constraint:
  type: "must" | "must_not" | "should" | "may"
  scope: "file" | "class" | "method" | "project"
  rule: "具体规则"
  verification: "如何验证"
```

### 命令式（脚本）
```python
# validate_constraints.py
def check_naming(file_path):
    if not file_path.endswith("_processor.py"):
        raise Violation("命名不规范")
```

## ⚠️ 常见陷阱

| 陷阱 | 表现 | 解决 |
|---|---|---|
| 过度约束 | Agent 无法完成任务 | 从最小约束集开始 |
| 约束冲突 | 约束 A 要求 X，约束 B 禁止 X | 建立优先级链 |
| 无法验证 | 约束定义模糊，无法自动化检查 | 每条约束都必须可验证 |
| 静态不变 | 项目演进后约束过时 | 定期复审约束集 |
