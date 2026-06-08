# Codex Harness Loop 设计方案

> 基于 Codex CLI `codex exec` 的多智能体自动执行循环

## 架构概览

```
┌─────────────────────────────────────────────────────────┐
│                    Harness Orchestrator                  │
│                  (Rust / Shell 脚本)                     │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐          │
│  │ Planner  │───▶│ Generator│───▶│ Reviewer │───▶ ...   │
│  │ (codex   │    │ (codex   │    │ (codex   │          │
│  │  exec)   │    │  exec)   │    │  exec)   │          │
│  └──────────┘    └──────────┘    └──────────┘          │
│       │               │               │                 │
│       ▼               ▼               ▼                 │
│  ┌─────────────────────────────────────────────┐       │
│  │          Redis / SQLite 状态层               │       │
│  │  (HandoffCard + VERDICT + FileDiff)         │       │
│  └─────────────────────────────────────────────┘       │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

## 核心命令

### 1. 单步执行 (Generator)

```bash
codex exec \
  --sandbox workspace-write \
  --approval-policy never \
  --json \
  --output-schema verdict-schema.json \
  "修复 Bug #462：目录管理-诊疗目录 编辑弹窗中所需标本下拉框数据加载失败

   约束：
   - 使用 Vue3 Composition API + script setup
   - 修改后运行: vue-tsc --noEmit && vite build
   - 输出最后一行必须是: VERDICT: PASS 或 VERDICT: FAIL [原因]
   - 只修改 src/views/basicmanage/caseTemplates/ 下的文件" \
  2>/dev/null | tee /tmp/codex-fix-462.jsonl
```

### 2. 单步执行 (Reviewer — 只读)

```bash
codex exec \
  --sandbox read-only \
  --approval-policy never \
  --json \
  "审查 Bug #462 的修复代码。

   检查项：
   1. 设计质量 (1-5): 命名规范、错误处理、API风格
   2. 工艺性 (1-5): 边界条件、类型安全、日志
   3. 功能性 (1-5): 功能是否按预期工作
   4. 风格一致性 (1-5): 与项目现有代码风格匹配度

   输出格式：
   设计质量: X
   工艺性: X
   功能性: X
   风格一致性: X
   VERDICT: PASS 或 VERDICT: FAIL [原因]" \
  2>/dev/null | tee /tmp/codex-review-462.jsonl
```

### 3. QA 测试 (执行测试)

```bash
codex exec \
  --sandbox workspace-write \
  --approval-policy never \
  --json \
  "运行 Bug #462 的回归测试。

   步骤：
   1. 确保前端 dev server 运行: npm run dev -- --port 81
   2. 运行: npx playwright test --grep @bug462 --workers=1
   3. 如果没有 Playwright 测试，运行: npm run build 验证编译

   输出最后一行必须是: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
  2>/dev/null | tee /tmp/codex-test-462.jsonl
```

## 完整 Harness Loop 脚本

```bash
#!/bin/bash
# codex-harness-loop.sh — Codex-native 多智能体自动执行循环
set -euo pipefail

BUG_ID="${1:?Usage: $0 <bug_id> <bug_title>}"
BUG_TITLE="${2:-}"
MAX_ROUNDS=3
WORK_DIR="/root/.openclaw/workspace/his-repo"

# VERDICT Schema
cat > /tmp/verdict-schema.json << 'SCHEMA'
{
  "type": "object",
  "properties": {
    "verdict": { "type": "string", "enum": ["PASS", "FAIL"] },
    "reason": { "type": "string" },
    "files_changed": { "type": "array", "items": { "type": "string" } },
    "scores": {
      "type": "object",
      "properties": {
        "design_quality": { "type": "integer" },
        "craft": { "type": "integer" },
        "functionality": { "type": "integer" },
        "style_consistency": { "type": "integer" }
      }
    }
  },
  "required": ["verdict"]
}
SCHEMA

echo "═══════════════════════════════════════════════"
echo "🔄 Harness Loop: Bug #$BUG_ID"
echo "═══════════════════════════════════════════════"

# ── Phase 1: 修复 (Generator) ──
echo ""
echo "📝 Phase 1: Generator 修复中..."
FIX_OUTPUT=$(codex exec \
  --sandbox workspace-write \
  --approval-policy never \
  --json \
  "修复 Bug #$BUG_ID：$BUG_TITLE

   约束：
   - 分析现有代码逻辑，最小化修改
   - 修改后验证编译通过
   - 输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
  --output-last-message /tmp/fix-result.txt \
  2>/tmp/fix-stderr.jsonl | tee /tmp/fix-output.jsonl)

FIX_VERDICT=$(tail -1 /tmp/fix-result.txt 2>/dev/null || echo "UNKNOWN")
echo "  修复结果: $FIX_VERDICT"

if echo "$FIX_VERDICT" | grep -q "VERDICT: FAIL"; then
  echo "  ❌ 修复失败，终止循环"
  exit 1
fi

# ── Phase 2: 代码审查 (Reviewer) ──
echo ""
echo "🔍 Phase 2: Code Reviewer 审查中..."
REVIEW_OUTPUT=$(codex exec \
  --sandbox read-only \
  --approval-policy never \
  --json \
  "审查 Bug #$BUG_ID 的修复代码。

   评估维度 (每项1-5分)：
   - 设计质量: 命名规范、错误处理、API风格
   - 工艺性: 边界条件、类型安全、日志
   - 功能性: 功能是否按预期工作
   - 风格一致性: 与项目现有代码风格匹配度

   通过线: 总分≥12/20 且 功能性≥3
   输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
  --output-last-message /tmp/review-result.txt \
  2>/tmp/review-stderr.jsonl | tee /tmp/review-output.jsonl)

REVIEW_VERDICT=$(tail -1 /tmp/review-result.txt 2>/dev/null || echo "UNKNOWN")
echo "  审查结果: $REVIEW_VERDICT"

if echo "$REVIEW_VERDICT" | grep -q "VERDICT: FAIL"; then
  echo "  ⚠️ 审查未通过，重新修复..."
  for round in $(seq 2 $MAX_ROUNDS); do
    echo "  🔄 重试 #$round..."
    # 将审查反馈传给 Generator
    FEEDBACK=$(cat /tmp/review-result.txt)
    codex exec \
      --sandbox workspace-write \
      --approval-policy never \
      --json \
      "Bug #$BUG_ID 修复未通过审查。

       审查反馈：
       $FEEDBACK

       请根据反馈修复代码。输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
      --output-last-message /tmp/fix-result.txt \
      2>/dev/null
    FIX_VERDICT=$(tail -1 /tmp/fix-result.txt 2>/dev/null || echo "UNKNOWN")
    if echo "$FIX_VERDICT" | grep -q "VERDICT: PASS"; then
      echo "  ✅ 重试 #$round 通过"
      break
    fi
  done
fi

# ── Phase 3: QA 测试 ──
echo ""
echo "🧪 Phase 3: QA 测试中..."
TEST_OUTPUT=$(codex exec \
  --sandbox workspace-write \
  --approval-policy never \
  --json \
  "测试 Bug #$BUG_ID 的修复。

   步骤：
   1. 运行编译验证
   2. 如果有 Playwright 测试则运行
   3. 检查无回归

   输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
  --output-last-message /tmp/test-result.txt \
  2>/tmp/test-stderr.jsonl | tee /tmp/test-output.jsonl)

TEST_VERDICT=$(tail -1 /tmp/test-result.txt 2>/dev/null || echo "UNKNOWN")
echo "  测试结果: $TEST_VERDICT"

# ── Phase 4: 验收 (Verifier) ──
echo ""
echo "✅ Phase 4: Verifier 验收中..."
VERIFY_OUTPUT=$(codex exec \
  --sandbox read-only \
  --approval-policy never \
  --json \
  "验收 Bug #$BUG_ID 的修复。

   检查项：
   1. Git commit 存在
   2. 编译通过
   3. 测试通过
   4. 无回归

   输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
  --output-last-message /tmp/verify-result.txt \
  2>/tmp/verify-stderr.jsonl | tee /tmp/verify-output.jsonl)

VERIFY_VERDICT=$(tail -1 /tmp/verify-result.txt 2>/dev/null || echo "UNKNOWN")
echo "  验收结果: $VERIFY_VERDICT"

# ── 汇总 ──
echo ""
echo "═══════════════════════════════════════════════"
echo "📊 Harness Loop 完成: Bug #$BUG_ID"
echo "  修复: $FIX_VERDICT"
echo "  审查: $REVIEW_VERDICT"
echo "  测试: $TEST_VERDICT"
echo "  验收: $VERIFY_VERDICT"
echo "═══════════════════════════════════════════════"
```

## 关键设计决策

### 1. Context Reset (上下文重置)

每次 `codex exec` 都是全新进程，天然实现 Context Reset：
- 无上下文窗口侵蚀
- 每个阶段干净开始
- 通过 HandoffCard 传递结构化上下文

### 2. 工具权限隔离

| Agent | Sandbox | 权限 |
|-------|---------|------|
| Generator | `workspace-write` | Read + Write + Edit + Bash |
| Reviewer | `read-only` | Read Only |
| QA | `workspace-write` | Read + Bash (测试) |
| Verifier | `read-only` | Read + Bash (验证) |

### 3. VERDICT 协议

每个 Agent 输出最后一行必须是：
```
VERDICT: PASS [可选说明]
VERDICT: FAIL [具体原因]
```

通过 `--output-schema` 强制结构化输出。

### 4. 文件快照 Diff

```bash
# 修复前
find src/ -type f -name "*.java" -o -name "*.vue" | xargs stat > /tmp/before-snapshot.txt

# 修复后
find src/ -type f -name "*.java" -o -name "*.vue" | xargs stat > /tmp/after-snapshot.txt

# 计算差异
diff /tmp/before-snapshot.txt /tmp/after-snapshot.txt
```

### 5. 轮次预算

Redis 计数器 + 硬性上限：
```bash
ROUND_KEY="round:$BUG_ID:$(date +%Y%m%d)"
CURRENT=$(redis-cli incr "$ROUND_KEY")
redis-cli expire "$ROUND_KEY" 604800  # 7天TTL

if [ "$CURRENT" -gt "$MAX_ROUNDS" ]; then
  echo "🔴 超出轮次预算，升级到人工处理"
  exit 1
fi
```

## 与 agentforge-rs 的集成

### 方案 A: 替换 codex-aliyun

```bash
# 当前: codex-aliyun → mimo2codex → codex
# 优化: 直接用 codex exec

codex exec \
  --sandbox workspace-write \
  --approval-policy never \
  --json \
  --output-schema /root/agentforge-rs/schemas/verdict.json \
  "$(cat /root/agentforge-rs/agents/guanyu.md)

   修复 Bug #$BUG_ID：$BUG_TITLE" \
  2>/dev/null
```

### 方案 B: 混合模式

- Rust 编排器控制流程
- Codex 执行具体任务
- Redis 传递 HandoffCard
- SQLite 记录 Trace

## JSONL 事件解析

```bash
# 解析 Codex JSONL 输出
cat /tmp/fix-output.jsonl | while read -r line; do
  type=$(echo "$line" | jq -r '.type')
  case "$type" in
    "item.completed")
      item_type=$(echo "$line" | jq -r '.item.type')
      if [ "$item_type" = "agent_message" ]; then
        echo "Agent: $(echo "$line" | jq -r '.item.text')"
      fi
      ;;
    "turn.completed")
      tokens=$(echo "$line" | jq -r '.usage.output_tokens')
      echo "Tokens: $tokens"
      ;;
  esac
done
```
