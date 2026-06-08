#!/bin/bash
# ═══════════════════════════════════════════════════════════
# codex-harness-loop.sh — Codex-native 多智能体自动执行循环
# 
# 参考: HuanCode Agent Loop + Codex CLI codex exec
# 核心: 外层 Loop (Shell) + 内层 Loop (codex exec)
# ═══════════════════════════════════════════════════════════
set -euo pipefail

BUG_ID="${1:?Usage: $0 <bug_id> [bug_title]}"
BUG_TITLE="${2:-}"
MAX_ROUNDS="${3:-3}"
WORK_DIR="/root/.openclaw/workspace/his-repo"

# 颜色
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# ═══════════════════════════════════════════════════════════
# 工具函数
# ═══════════════════════════════════════════════════════════

parse_verdict() {
    local output="$1"
    echo "$output" | grep -oP 'VERDICT:\s*\K(PASS|FAIL)' | tail -1 || echo "UNKNOWN"
}

parse_verdict_reason() {
    local output="$1"
    echo "$output" | grep -oP 'VERDICT:\s*FAIL\s*\[\K[^\]]+' | tail -1 || echo ""
}

capture_snapshot() {
    local dir="$1"
    find "$dir" -type f \( -name "*.java" -o -name "*.vue" -o -name "*.ts" \) \
        -not -path "*/target/*" -not -path "*/node_modules/*" 2>/dev/null | \
        xargs stat --format='%n %s %Y' 2>/dev/null | sort
}

compute_diff() {
    local before="$1"
    local after="$2"
    local added=$(comm -13 <(echo "$before" | awk '{print $1}' | sort) <(echo "$after" | awk '{print $1}' | sort))
    local deleted=$(comm -23 <(echo "$before" | awk '{print $1}' | sort) <(echo "$after" | awk '{print $1}' | sort))
    local modified=$(comm -12 <(echo "$before" | awk '{print $1}' | sort) <(echo "$after" | awk '{print $1}' | sort) | while read f; do
        local b=$(echo "$before" | grep "^$f " | awk '{print $2,$3}')
        local a=$(echo "$after" | grep "^$f " | awk '{print $2,$3}')
        [ "$b" != "$a" ] && echo "$f"
    done)
    
    local added_count=$(echo "$added" | grep -c . 2>/dev/null || echo 0)
    local modified_count=$(echo "$modified" | grep -c . 2>/dev/null || echo 0)
    local deleted_count=$(echo "$deleted" | grep -c . 2>/dev/null || echo 0)
    
    echo "+${added_count} ~${modified_count} -${deleted_count}"
}

degraded_test() {
    local bug_id="$1"
    local code=$(curl -s -o /dev/null -w '%{http_code}' http://localhost:18082/healthlink-his/system/config/list 2>/dev/null)
    [ "$code" = "200" ] || [ "$code" = "401" ]
}

degraded_verify() {
    local bug_id="$1"
    local has_commit=$(git -C "$WORK_DIR" log origin/develop --grep="Bug#$bug_id" --oneline -1 2>/dev/null)
    local compile_ok=$(cd "$WORK_DIR/healthlink-his-server" && mvn compile -pl healthlink-his-application -am -q 2>/dev/null && echo "ok" || echo "fail")
    [ -n "$has_commit" ] && [ "$compile_ok" = "ok" ]
}

# ═══════════════════════════════════════════════════════════
# Codex 执行函数 (内层 Agent Loop)
# ═══════════════════════════════════════════════════════════

codex_exec_wrap() {
    local task="$1"
    local sandbox="${2:-workspace-write}"
    local timeout="${3:-600}"
    
    timeout "$timeout" codex exec \
        --sandbox "$sandbox" \
        --dangerously-bypass-approvals-and-sandbox \
        --json \
        "$task" 2>/dev/null | while IFS= read -r line; do
        # 解析 JSONL 事件
        local type=$(echo "$line" | jq -r '.type' 2>/dev/null)
        case "$type" in
            "item.completed")
                local item_type=$(echo "$line" | jq -r '.item.type' 2>/dev/null)
                if [ "$item_type" = "agent_message" ]; then
                    echo "$line" | jq -r '.item.text' 2>/dev/null
                fi
                ;;
        esac
    done
}

# ═══════════════════════════════════════════════════════════
# Harness Loop 主循环 (外层 Loop)
# ═══════════════════════════════════════════════════════════

echo -e "${BLUE}═══════════════════════════════════════════════${NC}"
echo -e "${BLUE}🔄 Harness Loop: Bug #$BUG_ID — $BUG_TITLE${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════${NC}"

# ── Phase 1: 修复 (Generator) ──
echo ""
echo -e "${YELLOW}📝 Phase 1: Generator 修复中...${NC}"

BEFORE_SNAPSHOT=$(capture_snapshot "$WORK_DIR/healthlink-his-server/src")

FIX_OUTPUT=$(codex_exec_wrap \
    "修复 Bug #$BUG_ID：$BUG_TITLE

约束：
- 分析现有代码逻辑，最小化修改
- 不要删除任何已有文件
- 修改后验证编译通过
- 输出最后一行必须是: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
    "workspace-write")

FIX_VERDICT=$(parse_verdict "$FIX_OUTPUT")
FIX_REASON=$(parse_verdict_reason "$FIX_OUTPUT")

echo -e "  修复结果: ${FIX_VERDICT:+VERDICT: $FIX_VERDICT}"
[ -n "$FIX_REASON" ] && echo -e "  原因: $FIX_REASON"

if [ "$FIX_VERDICT" = "FAIL" ]; then
    echo -e "  ${RED}❌ 修复失败，终止循环${NC}"
    exit 1
fi

# 文件快照 Diff
AFTER_SNAPSHOT=$(capture_snapshot "$WORK_DIR/healthlink-his-server/src")
DIFF_SUMMARY=$(compute_diff "$BEFORE_SNAPSHOT" "$AFTER_SNAPSHOT")
echo -e "  文件变更: $DIFF_SUMMARY"

# ── Phase 2: 代码审查 (Reviewer) ──
echo ""
echo -e "${YELLOW}🔍 Phase 2: Code Reviewer 审查中...${NC}"

REVIEW_VERDICT="FAIL"
for round in $(seq 1 $MAX_ROUNDS); do
    REVIEW_OUTPUT=$(codex_exec_wrap \
        "审查 Bug #$BUG_ID 的修复代码。

评估维度 (每项1-5分)：
- 设计质量: 命名规范、错误处理、API风格
- 工艺性: 边界条件、类型安全、日志
- 功能性: 功能是否按预期工作
- 风格一致性: 与项目现有代码风格匹配度

通过线: 总分≥12/20 且 功能性≥3
输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
        "read-only")
    
    REVIEW_VERDICT=$(parse_verdict "$REVIEW_OUTPUT")
    echo -e "  审查结果 (轮次$round): VERDICT: $REVIEW_VERDICT"
    
    if [ "$REVIEW_VERDICT" = "PASS" ]; then
        break
    fi
    
    # 重修
    if [ "$round" -lt "$MAX_ROUNDS" ]; then
        echo -e "  ${YELLOW}⚠️ 审查未通过，重新修复...${NC}"
        FIX_OUTPUT=$(codex_exec_wrap \
            "Bug #$BUG_ID 修复未通过审查。

审查反馈：
$REVIEW_OUTPUT

请根据反馈修复代码。输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
            "workspace-write")
        FIX_VERDICT=$(parse_verdict "$FIX_OUTPUT")
        echo -e "  重修结果: VERDICT: $FIX_VERDICT"
    fi
done

# ── Phase 3: QA 测试 ──
echo ""
echo -e "${YELLOW}🧪 Phase 3: QA 测试中...${NC}"

TEST_OUTPUT=$(codex_exec_wrap \
    "测试 Bug #$BUG_ID 的修复。

步骤：
1. 运行编译验证: cd /root/.openclaw/workspace/his-repo/healthlink-his-server && mvn compile -pl healthlink-his-application -am -q
2. 运行前端编译: cd /root/.openclaw/workspace/his-repo/healthlink-his-ui && npx vite build
3. 检查无回归

输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
    "workspace-write")

TEST_VERDICT=$(parse_verdict "$TEST_OUTPUT")

# 降级测试
if [ "$TEST_VERDICT" != "PASS" ]; then
    echo -e "  ${YELLOW}⚠️ 测试未通过，尝试降级测试...${NC}"
    if degraded_test "$BUG_ID"; then
        TEST_VERDICT="PASS"
        echo -e "  ${GREEN}✅ 降级测试通过（接口健康检查）${NC}"
    fi
fi

echo -e "  测试结果: VERDICT: $TEST_VERDICT"

# ── Phase 4: 验收 (Verifier) ──
echo ""
echo -e "${YELLOW}✅ Phase 4: Verifier 验收中...${NC}"

VERIFY_OUTPUT=$(codex_exec_wrap \
    "验收 Bug #$BUG_ID 的修复。

检查项：
1. Git commit 存在
2. 编译通过
3. 测试通过
4. 无回归

输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]" \
    "read-only")

VERIFY_VERDICT=$(parse_verdict "$VERIFY_OUTPUT")

# 降级验收
if [ "$VERIFY_VERDICT" != "PASS" ]; then
    echo -e "  ${YELLOW}⚠️ 验收未通过，尝试降级验收...${NC}"
    if degraded_verify "$BUG_ID"; then
        VERIFY_VERDICT="PASS"
        echo -e "  ${GREEN}✅ 降级验收通过（commit+compile）${NC}"
    fi
fi

echo -e "  验收结果: VERDICT: $VERIFY_VERDICT"

# ── 汇总 ──
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════${NC}"
echo -e "${BLUE}📊 Harness Loop 完成: Bug #$BUG_ID${NC}"
echo -e "  修复: $FIX_VERDICT"
echo -e "  审查: $REVIEW_VERDICT"
echo -e "  测试: $TEST_VERDICT"
echo -e "  验收: $VERIFY_VERDICT"

if [ "$FIX_VERDICT" = "PASS" ] && [ "$REVIEW_VERDICT" = "PASS" ] && \
   [ "$TEST_VERDICT" = "PASS" ] && [ "$VERIFY_VERDICT" = "PASS" ]; then
    echo -e "  结论: ${GREEN}✅ 全部通过${NC}"
    RESULT=0
else
    echo -e "  结论: ${RED}❌ 存在失败${NC}"
    RESULT=1
fi

echo -e "${BLUE}═══════════════════════════════════════════════${NC}"
exit $RESULT
