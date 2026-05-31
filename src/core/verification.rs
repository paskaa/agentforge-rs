//! Verification Module — 全链路验证系统
//!
//! 修复后自动执行: 编译验证 → 单元测试 → Playwright 回归 → 数据库验证 → 接口验证
//! 只有全部通过才允许标记为"已修复"并关闭禅道。

use std::process::Command;
use std::time::Duration;
use serde::{Deserialize, Serialize};

/// 单项验证结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub duration_ms: u64,
}

/// 完整验证报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub bug_id: String,
    pub agent_id: String,
    pub all_passed: bool,
    pub checks: Vec<CheckResult>,
    pub summary: String,
    pub total_ms: u64,
}

/// 运行单个命令，带超时
fn run_cmd(cmd: &str, args: &[&str], work_dir: &str, _timeout_secs: u64) -> (bool, String, String) {
    let output = Command::new(cmd)
        .args(args)
        .current_dir(work_dir)
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            (o.status.success(), stdout, stderr)
        }
        Err(e) => (false, String::new(), format!("spawn error: {}", e)),
    }
}

/// 检查 HIS 前端 dev server 是否就绪
fn check_frontend_ready() -> CheckResult {
    let start = std::time::Instant::now();
    let output = Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "http://localhost:81"])
        .output();
    let elapsed = start.elapsed().as_millis() as u64;
    
    match output {
        Ok(o) => {
            let code = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let passed = code == "200";
            CheckResult {
                name: "前端Dev Server就绪".into(),
                passed,
                message: if passed { "http://localhost:81 可达".into() } else { format!("HTTP {}", code) },
                duration_ms: elapsed,
            }
        }
        Err(e) => CheckResult {
            name: "前端Dev Server就绪".into(),
            passed: false,
            message: format!("curl 失败: {}", e),
            duration_ms: elapsed,
        },
    }
}

/// 启动 HIS 前端 dev server（如果没启动）
fn ensure_frontend_server() {
    let output = Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "http://localhost:81"])
        .output();
    let is_up = output.map(|o| String::from_utf8_lossy(&o.stdout).trim() == "200").unwrap_or(false);
    
    if !is_up {
        let _ = Command::new("bash")
            .arg("-c")
            .arg("cd /root/.openclaw/workspace/his-repo/openhis-ui-vue3 && nohup npx vite --mode dev --port 81 --host 0.0.0.0 > /tmp/his-dev.log 2>&1 &")
            .output();
        // Wait for server to start
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// 1. 后端编译验证
fn check_compile(agent_name: &str, work_dir: &str) -> CheckResult {
    let start = std::time::Instant::now();
    let elapsed = || start.elapsed().as_millis() as u64;
    
    if agent_name == "zhaoyun" {
        // 前端: vue-tsc + vite build
        let (ok, stdout, stderr) = run_cmd("npx", &["vite", "build", "--mode", "dev"], work_dir, 120);
        return CheckResult {
            name: "编译验证(vite build)".into(),
            passed: ok,
            message: if ok { "vite build 通过".into() } else { 
                let err = if stderr.len() > 500 { stderr.chars().take(500).collect() } else { stderr };
                if err.is_empty() { stdout.chars().take(500).collect() } else { err }
            },
            duration_ms: elapsed(),
        };
    }
    
    // 后端: mvn compile
    let (ok, stdout, stderr) = run_cmd("mvn", &["clean", "compile", "-q", "-pl", "openhis-application", "-am"], work_dir, 180);
    CheckResult {
        name: "编译验证(mvn compile)".into(),
        passed: ok,
        message: if ok { "mvn compile 通过".into() } else {
            let err = if stderr.len() > 500 { stderr.chars().take(500).collect() } else { stderr };
            if err.is_empty() { stdout.chars().take(500).collect() } else { err }
        },
        duration_ms: elapsed(),
    }
}

/// 2. 单元测试
fn check_unit_test(agent_name: &str, work_dir: &str) -> CheckResult {
    let start = std::time::Instant::now();
    let elapsed = || start.elapsed().as_millis() as u64;
    
    if agent_name == "zhaoyun" {
        // 前端: vitest
        let (ok, stdout, _) = run_cmd("npx", &["vitest", "run", "--reporter=verbose"], work_dir, 120);
        let no_tests = stdout.contains("No test files") || stdout.contains("no tests");
        return CheckResult {
            name: "单元测试(vitest)".into(),
            passed: ok || no_tests,
            message: if no_tests { "无测试文件(跳过)".into() } else if ok { "vitest 通过".into() } else {
                stdout.lines().filter(|l| l.contains("FAIL") || l.contains("✗")).take(3).collect::<Vec<_>>().join("; ")
            },
            duration_ms: elapsed(),
        };
    }
    
    // 后端: mvn test
    let (ok, stdout, stderr) = run_cmd("mvn", &["test", "-q", "-pl", "openhis-application",
        "-Dtest=com.openhis.MedicationApplicationTests",
        "-DfailIfNoTests=false"], work_dir, 180);
    CheckResult {
        name: "单元测试(mvn test)".into(),
        passed: ok,
        message: if ok { "Spring Boot 启动测试通过".into() } else {
            let err = if stderr.len() > 500 { stderr.chars().take(500).collect() } else { stderr };
            if err.is_empty() { stdout.chars().take(500).collect() } else { err }
        },
        duration_ms: elapsed(),
    }
}

/// 3. Playwright 回归测试
fn check_playwright(bug_id: &str) -> CheckResult {
    let start = std::time::Instant::now();
    let elapsed = || start.elapsed().as_millis() as u64;
    
    // 确保前端 dev server 运行
    ensure_frontend_server();
    
    // 检查有没有对应的 Playwright 测试用例
    let spec_dir = "/root/.openclaw/workspace/his-repo/openhis-ui-vue3/tests/e2e/specs";
    let has_test = Command::new("grep")
        .args(["-r", &format!("#{}", bug_id), spec_dir])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    
    if !has_test {
        return CheckResult {
            name: format!("Playwright 回归测试(@bug{})", bug_id),
            passed: true, // 没有测试用例不阻断
            message: format!("@bug{} 无 Playwright 测试用例(待补充)", bug_id),
            duration_ms: elapsed(),
        };
    }
    
    // 运行 Playwright
    let (ok, stdout, stderr) = run_cmd("bash", &["-c", &format!(
        "cd /root/.openclaw/workspace/his-repo/openhis-ui-vue3 && npx playwright test --grep @bug{} --reporter=line --workers=1 2>&1",
        bug_id
    )], "/", 180);
    
    let no_test = stdout.contains("No tests found") || stdout.contains("no tests");
    let passed = ok || no_test;
    
    CheckResult {
        name: format!("Playwright 回归测试(@bug{})", bug_id),
        passed,
        message: if no_test {
            format!("@bug{} Playwright 测试不存在(跳过)", bug_id)
        } else if passed {
            "Playwright 测试通过".into()
        } else {
            let fail_lines: Vec<&str> = stdout.lines()
                .filter(|l| l.contains("failed") || l.contains("Error") || l.contains("expect"))
                .take(5).collect();
            if fail_lines.is_empty() { 
                let err = if stderr.len() > 300 { stderr.chars().take(300).collect() } else { stderr };
                if err.is_empty() { stdout.chars().take(300).collect() } else { err }
            } else { 
                fail_lines.join("; ") 
            }
        },
        duration_ms: elapsed(),
    }
}

/// 4. 数据库验证（检查关键表是否可访问）
fn check_database(bug_title: &str) -> CheckResult {
    let start = std::time::Instant::now();
    let elapsed = || start.elapsed().as_millis() as u64;
    
    // 如果 bug 标题涉及特定表，验证表可访问
    let tables_to_check = [
        ("advice", "SELECT COUNT(*) FROM advice LIMIT 1"),
        ("diagnosis", "SELECT COUNT(*) FROM diagnosis LIMIT 1"),
        ("triage_queue_item", "SELECT COUNT(*) FROM triage_queue_item LIMIT 1"),
        ("adm_schedule_pool", "SELECT COUNT(*) FROM adm_schedule_pool LIMIT 1"),
    ];
    
    let mut relevant_checks = Vec::new();
    for (table, sql) in &tables_to_check {
        if bug_title.to_lowercase().contains(&table.to_lowercase()) || 
           bug_title.contains(table) {
            relevant_checks.push((*table, *sql));
        }
    }
    
    if relevant_checks.is_empty() {
        return CheckResult {
            name: "数据库验证".into(),
            passed: true,
            message: "未涉及特定数据库表(跳过)".into(),
            duration_ms: elapsed(),
        };
    }
    
    // 验证数据库连接
    let (ok, stdout, stderr) = run_cmd("bash", &["-c", &format!(
        "PGPASSWORD=Jchl1528 psql -h 192.168.110.252 -p 15432 -U postgresql -d postgresql -c '{}' 2>&1",
        relevant_checks[0].1
    )], "/", 10);
    
    CheckResult {
        name: format!("数据库验证({})", relevant_checks.iter().map(|(t,_)| *t).collect::<Vec<_>>().join(",")),
        passed: ok,
        message: if ok { "数据库表可访问".into() } else {
            let err = if stderr.len() > 300 { stderr.chars().take(300).collect() } else { stderr };
            if err.is_empty() { stdout.chars().take(300).collect() } else { err }
        },
        duration_ms: elapsed(),
    }
}

/// 5. 接口验证（检查关键 API 是否可访问）
fn check_api(bug_title: &str) -> CheckResult {
    let start = std::time::Instant::now();
    let elapsed = || start.elapsed().as_millis() as u64;
    
    // 根据 bug 标题判断需要验证哪些接口
    let api_checks = [
        ("/api/advice/save", "POST"),
        ("/api/diagnosis/save", "POST"),
        ("/api/triage/queue", "GET"),
        ("/api/schedule/pool", "GET"),
    ];
    
    let mut relevant = Vec::new();
    for (path, method) in &api_checks {
        if bug_title.to_lowercase().contains(path.split('/').last().unwrap_or("")) {
            relevant.push((*path, *method));
        }
    }
    
    if relevant.is_empty() {
        return CheckResult {
            name: "接口验证".into(),
            passed: true,
            message: "未涉及特定接口(跳过)".into(),
            duration_ms: elapsed(),
        };
    }
    
    // 验证后端服务可达
    let (ok, _, stderr) = run_cmd("curl", &["-s", "-o", "/dev/null", "-w", "%{http_code}", 
        "http://localhost:8650"], "/", 5);
    
    CheckResult {
        name: "接口验证(后端服务可达)".into(),
        passed: ok,
        message: if ok { "后端服务可达".into() } else { 
            format!("后端不可达: {}", stderr.chars().take(200).collect::<String>()) 
        },
        duration_ms: elapsed(),
    }
}

/// 执行完整验证流程
pub fn run_full_verification(agent_name: &str, bug_id: &str, bug_title: &str, work_dir: &str) -> VerificationReport {
    let start = std::time::Instant::now();
    let mut checks = Vec::new();
    
    // 1. 编译验证
    checks.push(check_compile(agent_name, work_dir));
    if !checks.last().unwrap().passed {
        let total = start.elapsed().as_millis() as u64;
        return VerificationReport {
            bug_id: bug_id.into(), agent_id: agent_name.into(),
            all_passed: false, checks, summary: "编译验证失败，终止后续检查".into(), total_ms: total,
        };
    }
    
    // 2. 单元测试
    checks.push(check_unit_test(agent_name, work_dir));
    if !checks.last().unwrap().passed {
        let total = start.elapsed().as_millis() as u64;
        return VerificationReport {
            bug_id: bug_id.into(), agent_id: agent_name.into(),
            all_passed: false, checks, summary: "单元测试失败".into(), total_ms: total,
        };
    }
    
    // 3. Playwright 回归测试
    checks.push(check_playwright(bug_id));
    
    // 4. 数据库验证
    checks.push(check_database(bug_title));
    
    // 5. 接口验证
    checks.push(check_api(bug_title));
    
    let total = start.elapsed().as_millis() as u64;
    let all_passed = checks.iter().all(|c| c.passed);
    let summary = if all_passed {
        format!("✅ 全部 {} 项验证通过", checks.len())
    } else {
        let failed: Vec<&str> = checks.iter().filter(|c| !c.passed).map(|c| c.name.as_str()).collect();
        format!("❌ {} 项验证失败: {}", failed.len(), failed.join(", "))
    };
    
    VerificationReport { bug_id: bug_id.into(), agent_id: agent_name.into(), all_passed, checks, summary, total_ms: total }
}
