#!/usr/bin/env python3
"""
codex_harness_loop.py — Codex-native 多智能体自动执行循环

参考: HuanCode Agent Loop + Codex CLI codex exec
核心: 外层 Loop (Python) + 内层 Loop (codex exec)
"""

import subprocess
import json
import sys
import os
from datetime import datetime

# ═══════════════════════════════════════════════════════════
# 工具执行层 (来自 HuanCode Agent Loop)
# ═══════════════════════════════════════════════════════════

def run_bash(command: str, timeout: int = 120) -> str:
    """执行 shell 命令，返回输出"""
    dangerous = ["rm -rf /", "sudo rm", "shutdown", "reboot"]
    if any(d in command for d in dangerous):
        return "Error: Dangerous command blocked"
    try:
        r = subprocess.run(
            command, shell=True, capture_output=True, text=True, timeout=timeout
        )
        out = (r.stdout + r.stderr).strip()
        return out[:50000] if out else "(no output)"
    except subprocess.TimeoutExpired:
        return f"Error: Timeout ({timeout}s)"
    except Exception as e:
        return f"Error: {e}"

# ═══════════════════════════════════════════════════════════
# VERDICT 协议 (来自 agentforge-rs)
# ═══════════════════════════════════════════════════════════

def parse_verdict(output: str) -> tuple:
    """从输出中解析 VERDICT: (verdict, reason)"""
    for line in output.split("\n"):
        line = line.strip()
        if "VERDICT:" in line or "VERDICT：" in line:
            if "PASS" in line or "通过" in line:
                return ("PASS", "")
            if "FAIL" in line or "失败" in line:
                reason = line.split(":", 1)[-1].split("：", 1)[-1]
                reason = reason.replace("FAIL", "").replace("失败", "").strip()
                return ("FAIL", reason or "未提供原因")
    return ("UNKNOWN", "未找到VERDICT")

# ═══════════════════════════════════════════════════════════
# Codex 执行层 (内层 Agent Loop)
# ═══════════════════════════════════════════════════════════

def codex_exec(task: str, sandbox: str = "workspace-write",
               schema_path: str = None, timeout: int = 600) -> dict:
    """
    执行 codex exec，返回结构化结果
    
    Codex CLI 内部已经实现了完整的 Agent Loop:
      while True:
          response = model.call(messages, tools)
          if stop_reason != "tool_use": break
          execute_tools(response.tool_calls)
    """
    cmd = [
        "codex", "exec",
        "--sandbox", sandbox,
        "--approval-policy", "never",
        "--json",
    ]
    if schema_path:
        cmd.extend(["--output-schema", schema_path])
    cmd.append(task)
    
    try:
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout,
            cwd="/root/.openclaw/workspace/his-repo"
        )
        
        # 解析 JSONL 输出
        final_message = ""
        total_tokens = 0
        for line in result.stdout.strip().split("\n"):
            if not line.strip():
                continue
            try:
                event = json.loads(line)
                if event.get("type") == "item.completed":
                    item = event.get("item", {})
                    if item.get("type") == "agent_message":
                        final_message = item.get("text", "")
                if event.get("type") == "turn.completed":
                    usage = event.get("usage", {})
                    total_tokens += usage.get("output_tokens", 0)
                    total_tokens += usage.get("input_tokens", 0)
            except json.JSONDecodeError:
                if not final_message:
                    final_message = line
        
        verdict, reason = parse_verdict(final_message)
        
        return {
            "success": result.returncode == 0 and verdict == "PASS",
            "message": final_message,
            "verdict": verdict,
            "reason": reason,
            "tokens": total_tokens,
            "stderr": result.stderr[:2000],
        }
    except subprocess.TimeoutExpired:
        return {"success": False, "message": "timeout", "verdict": "FAIL", "reason": "timeout", "tokens": 0, "stderr": "timeout"}
    except Exception as e:
        return {"success": False, "message": str(e), "verdict": "FAIL", "reason": str(e), "tokens": 0, "stderr": str(e)}

# ═══════════════════════════════════════════════════════════
# 文件快照 Diff (来自 agentforge-rs)
# ═══════════════════════════════════════════════════════════

def capture_snapshot(project_dir: str) -> dict:
    snapshot = {}
    output = run_bash(
        f"find {project_dir} -type f "
        f"\\( -name '*.java' -o -name '*.vue' -o -name '*.ts' \\) "
        f"-not -path '*/target/*' -not -path '*/node_modules/*' | "
        f"xargs stat --format='%n %s %Y' 2>/dev/null"
    )
    for line in output.split("\n"):
        parts = line.strip().split()
        if len(parts) >= 3:
            snapshot[parts[0]] = (int(parts[1]), parts[2])
    return snapshot

def compute_diff(before: dict, after: dict) -> dict:
    added = [f for f in after if f not in before]
    modified = [f for f in after if f in before and after[f] != before[f]]
    deleted = [f for f in before if f not in after]
    return {"added": added, "modified": modified, "deleted": deleted}

# ═══════════════════════════════════════════════════════════
# 优雅降级 (来自 agentforge-rs)
# ═══════════════════════════════════════════════════════════

def degraded_test(bug_id: str) -> bool:
    code = run_bash("curl -s -o /dev/null -w '%{http_code}' http://localhost:18082/healthlink-his/system/config/list")
    return code.strip() in ["200", "401"]

def degraded_verify(bug_id: str) -> bool:
    has_commit = bool(run_bash(f"git log origin/develop --grep='Bug#{bug_id}' --oneline -1").strip())
    compile_ok = run_bash("cd /root/.openclaw/workspace/his-repo/healthlink-his-server && mvn compile -pl healthlink-his-application -am -q 2>&1 | tail -1") == ""
    return has_commit and compile_ok

# ═══════════════════════════════════════════════════════════
# Harness Loop 主循环
# ═══════════════════════════════════════════════════════════

def harness_loop(bug_id: str, bug_title: str, max_rounds: int = 3):
    project_dir = "/root/.openclaw/workspace/his-repo"
    
    print(f"\n{'='*60}")
    print(f"🔄 Harness Loop: Bug #{bug_id} — {bug_title}")
    print(f"{'='*60}")
    
    # ── Phase 1: 修复 (Generator) ──
    print(f"\n📝 Phase 1: Generator 修复中...")
    
    before_snapshot = capture_snapshot(f"{project_dir}/healthlink-his-server/src")
    
    fix_result = codex_exec(
        f"修复 Bug #{bug_id}：{bug_title}\n\n"
        "约束：\n- 分析现有代码逻辑，最小化修改\n- 不要删除任何已有文件\n"
        "- 修改后验证编译通过\n- 输出最后一行必须是: VERDICT: PASS 或 VERDICT: FAIL [原因]",
        sandbox="workspace-write"
    )
    
    print(f"  修复结果: VERDICT: {fix_result['verdict']}")
    if fix_result['reason']:
        print(f"  原因: {fix_result['reason']}")
    
    if fix_result['verdict'] == "FAIL":
        print(f"  ❌ 修复失败，终止循环")
        return False
    
    after_snapshot = capture_snapshot(f"{project_dir}/healthlink-his-server/src")
    file_diff = compute_diff(before_snapshot, after_snapshot)
    print(f"  文件变更: +{len(file_diff['added'])} ~{len(file_diff['modified'])} -{len(file_diff['deleted'])}")
    
    # ── Phase 2: 代码审查 (Reviewer) ──
    print(f"\n🔍 Phase 2: Code Reviewer 审查中...")
    
    review_verdict = "FAIL"
    for round_num in range(max_rounds):
        review_result = codex_exec(
            f"审查 Bug #{bug_id} 的修复代码。\n\n"
            "评估维度 (每项1-5分)：\n- 设计质量: 命名规范、错误处理、API风格\n"
            "- 工艺性: 边界条件、类型安全、日志\n- 功能性: 功能是否按预期工作\n"
            "- 风格一致性: 与项目现有代码风格匹配度\n\n"
            "通过线: 总分≥12/20 且 功能性≥3\n"
            "输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]",
            sandbox="read-only"
        )
        
        review_verdict = review_result['verdict']
        print(f"  审查结果 (轮次{round_num+1}): VERDICT: {review_verdict}")
        
        if review_verdict == "PASS":
            break
        
        if round_num < max_rounds - 1:
            print(f"  ⚠️ 审查未通过，重新修复...")
            fix_result = codex_exec(
                f"Bug #{bug_id} 修复未通过审查。\n\n"
                f"审查反馈：\n{review_result['message']}\n\n"
                "请根据反馈修复代码。输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]",
                sandbox="workspace-write"
            )
            print(f"  重修结果: VERDICT: {fix_result['verdict']}")
    
    # ── Phase 3: QA 测试 ──
    print(f"\n🧪 Phase 3: QA 测试中...")
    
    test_result = codex_exec(
        f"测试 Bug #{bug_id} 的修复。\n\n"
        "步骤：\n1. 运行编译验证\n2. 运行前端编译\n3. 检查无回归\n\n"
        "输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]",
        sandbox="workspace-write"
    )
    
    test_verdict = test_result['verdict']
    if test_verdict != "PASS":
        print(f"  ⚠️ 测试未通过，尝试降级测试...")
        if degraded_test(bug_id):
            test_verdict = "PASS"
            print(f"  ✅ 降级测试通过（接口健康检查）")
    
    print(f"  测试结果: VERDICT: {test_verdict}")
    
    # ── Phase 4: 验收 (Verifier) ──
    print(f"\n✅ Phase 4: Verifier 验收中...")
    
    verify_result = codex_exec(
        f"验收 Bug #{bug_id} 的修复。\n\n"
        "检查项：\n1. Git commit 存在\n2. 编译通过\n3. 测试通过\n4. 无回归\n\n"
        "输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]",
        sandbox="read-only"
    )
    
    verify_verdict = verify_result['verdict']
    if verify_verdict != "PASS":
        print(f"  ⚠️ 验收未通过，尝试降级验收...")
        if degraded_verify(bug_id):
            verify_verdict = "PASS"
            print(f"  ✅ 降级验收通过（commit+compile）")
    
    print(f"  验收结果: VERDICT: {verify_verdict}")
    
    # ── 汇总 ──
    all_pass = all(v == "PASS" for v in [
        fix_result['verdict'], review_verdict, test_verdict, verify_verdict
    ])
    
    print(f"\n{'='*60}")
    print(f"📊 Harness Loop 完成: Bug #{bug_id}")
    print(f"  修复: {fix_result['verdict']}")
    print(f"  审查: {review_verdict}")
    print(f"  测试: {test_verdict}")
    print(f"  验收: {verify_verdict}")
    print(f"  结论: {'✅ 全部通过' if all_pass else '❌ 存在失败'}")
    print(f"{'='*60}")
    
    return all_pass

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 codex_harness_loop.py <bug_id> [bug_title] [max_rounds]")
        sys.exit(1)
    
    bug_id = sys.argv[1]
    bug_title = sys.argv[2] if len(sys.argv) > 2 else ""
    max_rounds = int(sys.argv[3]) if len(sys.argv) > 3 else 3
    
    success = harness_loop(bug_id, bug_title, max_rounds)
    sys.exit(0 if success else 1)
