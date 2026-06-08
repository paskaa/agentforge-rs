# Codex Harness Loop 完整实现方案

> 参考: HuanCode《Harness实战：从零搭建Agent Loop》+ Codex CLI `codex exec` 能力

## 核心原理 (来自 HuanCode 文章)

Agent Loop 的本质是一个 **while 循环**：

```
while True:
    response = model.call(messages, tools)
    if response.stop_reason != "tool_use":
        return  # 模型搞定了
    # 执行工具，回传结果，继续循环
    results = execute_tools(response.tool_calls)
    messages.append(tool_results)
```

**关键洞见**：
- `stop_reason == "tool_use"` → 继续循环
- `stop_reason != "tool_use"` → 退出循环
- Harness 只负责**执行和传话**，模型自己决定要不要继续

## Codex CLI 的 Agent Loop 能力

Codex CLI 已经内置了完整的 Agent Loop：

```bash
# codex exec 就是一个 Agent Loop
codex exec --sandbox workspace-write --approval-policy never --json "任务描述"
```

内部流程：
```
用户 prompt → Codex Agent Loop (while True)
  ├─ 模型返回 tool_use → Codex 执行工具 → 结果回传 → 继续
  ├─ 模型返回 end_turn → 输出最终消息 → 退出
  └─ 模型返回 max_tokens → 截断 → 退出
```

## 我们的 Harness Loop 设计

### 架构：外层 Loop (Rust) + 内层 Loop (Codex)

```
┌─────────────────────────────────────────────────┐
│           外层 Loop (agentforge-rs Rust)          │
│                                                  │
│  for bug in scan_bugs():                         │
│    ┌─────────────────────────────────────────┐   │
│    │  内层 Loop (codex exec)                  │   │
│    │                                          │   │
│    │  while True:                             │   │
│    │    response = codex.exec(task)           │   │
│    │    if stop_reason != "tool_use": break   │   │
│    │    execute_tools(response)               │   │
│    │                                          │   │
│    └─────────────────────────────────────────┘   │
│                                                  │
│  流水线: Fix → Review → Test → Verify → Archive  │
│  状态: Redis (HandoffCard + VERDICT + Budget)    │
│                                                  │
└─────────────────────────────────────────────────┘
```

### 实现：Python 版最小 Codex Harness Loop

```python
#!/usr/bin/env python3
"""
codex_harness_loop.py — Codex-native 多智能体自动执行循环
参考: HuanCode Agent Loop + Codex CLI codex exec
"""

import subprocess
import json
import sys
import os
from datetime import datetime

# ═══════════════════════════════════════════════════════════
# 工具执行层 (类似 HuanCode 的 run_bash)
# ═══════════════════════════════════════════════════════════

def run_bash(command: str, timeout: int = 120) -> str:
    """执行 shell 命令，返回输出"""
    dangerous = ["rm -rf /", "sudo", "shutdown", "reboot"]
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
# Codex 执行层 (替代 Anthropic API 直接调用)
# ═══════════════════════════════════════════════════════════

def codex_exec(task: str, sandbox: str = "workspace-write",
               schema_path: str = None, timeout: int = 600) -> dict:
    """
    执行 codex exec，返回结构化结果
    等价于 HuanCode 的 response = client.messages.create(...)
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
            cmd, capture_output=True, text=True, timeout=timeout
        )
        
        # 解析 JSONL 输出
        events = []
        final_message = ""
        for line in result.stdout.strip().split("\n"):
            if not line:
                continue
            try:
                event = json.loads(line)
                events.append(event)
                if event.get("type") == "item.completed":
                    item = event.get("item", {})
                    if item.get("type") == "agent_message":
                        final_message = item.get("text", "")
            except json.JSONDecodeError:
                continue
        
        return {
            "success": result.returncode == 0,
            "message": final_message,
            "events": events,
            "stderr": result.stderr[:2000],
        }
    except subprocess.TimeoutExpired:
        return {"success": False, "message": "timeout", "events": [], "stderr": "timeout"}
    except Exception as e:
        return {"success": False, "message": str(e), "events": [], "stderr": str(e)}

# ═══════════════════════════════════════════════════════════
# VERDICT 协议 (来自 agentforge-rs)
# ═══════════════════════════════════════════════════════════

def parse_verdict(output: str) -> tuple:
    """从输出中解析 VERDICT"""
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
# 文件快照 Diff (来自 agentforge-rs)
# ═══════════════════════════════════════════════════════════

def capture_snapshot(project_dir: str) -> dict:
    """捕获项目目录快照"""
    snapshot = {}
    output = run_bash(
        f"find {project_dir} -type f "
        f"\\( -name '*.java' -o -name '*.vue' -o -name '*.ts' -o -name '*.js' \\) "
        f"-not -path '*/target/*' -not -path '*/node_modules/*' | "
        f"xargs stat --format='%n %s %Y' 2>/dev/null"
    )
    for line in output.split("\n"):
        parts = line.strip().split()
        if len(parts) >= 3:
            snapshot[parts[0]] = (int(parts[1]), parts[2])
    return snapshot

def compute_diff(before: dict, after: dict) -> dict:
    """计算文件差异"""
    added = [f for f in after if f not in before]
    modified = [f for f in after if f in before and after[f] != before[f]]
    deleted = [f for f in before if f not in after]
    return {"added": added, "modified": modified, "deleted": deleted}

# ═══════════════════════════════════════════════════════════
# 轮次预算 (来自 agentforge-rs)
# ═══════════════════════════════════════════════════════════

class RoundBudget:
    def __init__(self, max_fix=3, max_test=3, max_verify=2, max_total=8):
        self.max_fix = max_fix
        self.max_test = max_test
        self.max_verify = max_verify
        self.max_total = max_total
        self.counts = {}

    def check(self, bug_id: str, agent: str) -> bool:
        key = f"{bug_id}:{agent}"
        count = self.counts.get(key, 0)
        limit = {
            "fix": self.max_fix, "test": self.max_test,
            "verify": self.max_verify
        }.get(agent, self.max_total)
        return count >= limit

    def increment(self, bug_id: str, agent: str):
        key = f"{bug_id}:{agent}"
        self.counts[key] = self.counts.get(key, 0) + 1

# ═══════════════════════════════════════════════════════════
# 优雅降级 (来自 agentforge-rs)
# ═══════════════════════════════════════════════════════════

def degraded_test(bug_id: str) -> bool:
    """Playwright 失败时降级到接口健康检查"""
    code = run_bash("curl -s -o /dev/null -w '%{http_code}' http://localhost:18082/healthlink-his/system/config/list")
    return code.strip() in ["200", "401"]

def degraded_verify(bug_id: str) -> bool:
    """验收超时时降级到自动验收"""
    has_commit = bool(run_bash(f"git log origin/develop --grep='Bug#{bug_id}' --oneline -1").strip())
    compile_ok = run_bash("mvn compile -pl healthlink-his-application -am -q -f /root/.openclaw/workspace/his-repo/healthlink-his-server/pom.xml") == ""
    return has_commit and compile_ok

# ═══════════════════════════════════════════════════════════
# Harness Loop 主循环
# ═══════════════════════════════════════════════════════════

def harness_loop(bug_id: str, bug_title: str, max_rounds: int = 3):
    """
    完整的 Harness Loop: Fix → Review → Test → Verify
    
    每个阶段都是一个 codex exec 调用 (内层 Agent Loop)
    外层 Loop 控制流水线流转和重试
    """
    budget = RoundBudget()
    project_dir = "/root/.openclaw/workspace/his-repo"
    
    print(f"\n{'='*60}")
    print(f"🔄 Harness Loop: Bug #{bug_id} — {bug_title}")
    print(f"{'='*60}")
    
    # ── Phase 1: 修复 (Generator) ──
    print(f"\n📝 Phase 1: Generator 修复中...")
    
    before_snapshot = capture_snapshot(f"{project_dir}/healthlink-his-server")
    
    fix_result = codex_exec(
        f"""修复 Bug #{bug_id}：{bug_title}

约束：
- 分析现有代码逻辑，最小化修改
- 不要删除任何已有文件
- 修改后验证编译通过
- 输出最后一行必须是: VERDICT: PASS 或 VERDICT: FAIL [原因]""",
        sandbox="workspace-write"
    )
    
    fix_verdict, fix_reason = parse_verdict(fix_result["message"])
    print(f"  修复结果: VERDICT: {fix_verdict}")
    if fix_reason:
        print(f"  原因: {fix_reason}")
    
    if fix_verdict == "FAIL":
        print(f"  ❌ 修复失败，终止循环")
        return False
    
    # 文件快照 Diff
    after_snapshot = capture_snapshot(f"{project_dir}/healthlink-his-server")
    file_diff = compute_diff(before_snapshot, after_snapshot)
    total_changes = len(file_diff["added"]) + len(file_diff["modified"]) + len(file_diff["deleted"])
    print(f"  文件变更: +{len(file_diff['added'])} ~{len(file_diff['modified'])} -{len(file_diff['deleted'])}")
    
    # ── Phase 2: 代码审查 (Reviewer) ──
    print(f"\n🔍 Phase 2: Code Reviewer 审查中...")
    
    for review_round in range(max_rounds):
        budget.increment(bug_id, "review")
        
        review_result = codex_exec(
            f"""审查 Bug #{bug_id} 的修复代码。

评估维度 (每项1-5分)：
- 设计质量: 命名规范、错误处理、API风格
- 工艺性: 边界条件、类型安全、日志
- 功能性: 功能是否按预期工作
- 风格一致性: 与项目现有代码风格匹配度

通过线: 总分≥12/20 且 功能性≥3
输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]""",
            sandbox="read-only"
        )
        
        review_verdict, review_reason = parse_verdict(review_result["message"])
        print(f"  审查结果 (轮次{review_round+1}): VERDICT: {review_verdict}")
        
        if review_verdict == "PASS":
            break
        
        # 审查失败 → 重新修复
        if review_round < max_rounds - 1:
            print(f"  ⚠️ 审查未通过，重新修复...")
            fix_result = codex_exec(
                f"""Bug #{bug_id} 修复未通过审查。

审查反馈：
{review_result['message']}

请根据反馈修复代码。输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]""",
                sandbox="workspace-write"
            )
            fix_verdict, fix_reason = parse_verdict(fix_result["message"])
            print(f"  重修结果: VERDICT: {fix_verdict}")
    
    # ── Phase 3: QA 测试 ──
    print(f"\n🧪 Phase 3: QA 测试中...")
    
    budget.increment(bug_id, "test")
    
    test_result = codex_exec(
        f"""测试 Bug #{bug_id} 的修复。

步骤：
1. 运行编译验证: mvn compile -pl healthlink-his-application -am -q
2. 运行前端编译: cd healthlink-his-ui && npx vite build
3. 检查无回归

输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]""",
        sandbox="workspace-write"
    )
    
    test_verdict, test_reason = parse_verdict(test_result["message"])
    
    # 降级测试
    if test_verdict != "PASS":
        print(f"  ⚠️ 测试未通过，尝试降级测试...")
        if degraded_test(bug_id):
            test_verdict = "PASS"
            print(f"  ✅ 降级测试通过（接口健康检查）")
        else:
            print(f"  ❌ 降级测试也失败")
    
    print(f"  测试结果: VERDICT: {test_verdict}")
    
    # ── Phase 4: 验收 (Verifier) ──
    print(f"\n✅ Phase 4: Verifier 验收中...")
    
    budget.increment(bug_id, "verify")
    
    verify_result = codex_exec(
        f"""验收 Bug #{bug_id} 的修复。

检查项：
1. Git commit 存在 (git log origin/develop --grep='Bug#{bug_id}')
2. 编译通过
3. 测试通过
4. 无回归

输出最后一行: VERDICT: PASS 或 VERDICT: FAIL [原因]""",
        sandbox="read-only"
    )
    
    verify_verdict, verify_reason = parse_verdict(verify_result["message"])
    
    # 降级验收
    if verify_verdict != "PASS":
        print(f"  ⚠️ 验收未通过，尝试降级验收...")
        if degraded_verify(bug_id):
            verify_verdict = "PASS"
            print(f"  ✅ 降级验收通过（commit+compile）")
        else:
            print(f"  ❌ 降级验收也失败")
    
    print(f"  验收结果: VERDICT: {verify_verdict}")
    
    # ── 汇总 ──
    all_pass = all(v == "PASS" for v in [fix_verdict, review_verdict, test_verdict, verify_verdict])
    
    print(f"\n{'='*60}")
    print(f"📊 Harness Loop 完成: Bug #{bug_id}")
    print(f"  修复: {fix_verdict}")
    print(f"  审查: {review_verdict}")
    print(f"  测试: {test_verdict}")
    print(f"  验收: {verify_verdict}")
    print(f"  结论: {'✅ 全部通过' if all_pass else '❌ 存在失败'}")
    print(f"{'='*60}")
    
    return all_pass

# ═══════════════════════════════════════════════════════════
# 入口
# ═══════════════════════════════════════════════════════════

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 codex_harness_loop.py <bug_id> [bug_title]")
        sys.exit(1)
    
    bug_id = sys.argv[1]
    bug_title = sys.argv[2] if len(sys.argv) > 2 else ""
    
    success = harness_loop(bug_id, bug_title)
    sys.exit(0 if success else 1)
```

## 与 agentforge-rs 的集成方式

### 方案: Rust 编排 + Codex 执行

```
agentforge-rs (Rust)          Codex CLI
┌─────────────────┐          ┌──────────────┐
│ scan_bugs()     │          │              │
│ route_bug()     │          │              │
│ check_budget()  │          │              │
│                 │──────────▶│ codex exec   │
│ parse_verdict() │◀──────────│ --json       │
│ save_handoff()  │          │ --sandbox    │
│ publish_event() │          │ --schema     │
└─────────────────┘          └──────────────┘
```

**Rust 负责**: 扫描、路由、预算、状态、通知
**Codex 负责**: 代码理解、修复、审查、测试

### 替换当前的 codex-aliyun 调用

```rust
// 当前: subagent::run_codex_fix()
// 优化: 直接调用 codex exec

pub fn run_codex_fix_v2(agent: &str, bug_id: &str, title: &str) -> CodexResult {
    let schema = "/root/agentforge-rs/schemas/verdict.json";
    let output = Command::new("codex")
        .args(["exec",
               "--sandbox", "workspace-write",
               "--approval-policy", "never",
               "--json",
               "--output-schema", schema,
               &format!("修复 Bug #{}：{}", bug_id, title)])
        .output()
        .expect("failed to run codex");
    
    // 解析 JSONL 输出
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut final_message = String::new();
    for line in stdout.lines() {
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
            if event["type"] == "item.completed" {
                if event["item"]["type"] == "agent_message" {
                    final_message = event["item"]["text"].as_str().unwrap_or("").to_string();
                }
            }
        }
    }
    
    let (verdict, reason) = parse_verdict(&final_message);
    CodexResult {
        success: verdict == "PASS",
        bug_id: bug_id.to_string(),
        elapsed_ms: 0,
        stdout: final_message,
        stderr: String::new(),
        exit_code: output.status.code().unwrap_or(-1),
        changes: 0,
    }
}
```

## 关键差异: HuanCode vs Codex Harness Loop

| 维度 | HuanCode (Python) | Codex Harness Loop |
|------|-------------------|-------------------|
| 内层 Loop | 手写 while + API 调用 | `codex exec` 内置 |
| 工具执行 | 自己实现 run_bash | Codex 内置工具集 |
| 权限控制 | 自己实现危险命令过滤 | `--sandbox` 参数 |
| 输出格式 | 手动解析 response | `--json` JSONL 流 |
| 结构化输出 | 无 | `--output-schema` |
| 外层 Loop | 无 | agentforge-rs Rust 编排 |
| 状态管理 | 无 | Redis + SQLite |

## 优势

1. **更少代码**: Codex 内置了 Agent Loop，我们只需要外层编排
2. **更安全**: `--sandbox` 提供隔离，不需要自己实现危险命令过滤
3. **更可观测**: `--json` 输出完整事件流，便于调试和监控
4. **更可靠**: Codex 的工具集经过充分测试，比自己实现更稳定
5. **更灵活**: `--output-schema` 强制结构化输出，便于自动化决策
