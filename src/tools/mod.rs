//! Scheduler — cron-style periodic tasks (daily reports, health checks).

use chrono::{Local, NaiveTime};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// Scheduled task definition.
#[derive(Debug, Clone)]
pub struct ScheduledTask {
    pub name: String,
    pub time: String, // "HH:MM"
    pub enabled: bool,
}

impl ScheduledTask {
    pub fn daily_report() -> Self {
        Self { name: "daily_report".into(), time: "09:00".into(), enabled: true }
    }
    pub fn health_check() -> Self {
        Self { name: "health_check".into(), time: "".into(), enabled: true }
    }
}

pub struct Scheduler {
    pub scripts_dir: PathBuf,
    tasks: Vec<ScheduledTask>,
}

impl Scheduler {
    pub fn new(scripts_dir: PathBuf, mut tasks: Vec<ScheduledTask>) -> Self {
        // Always include health check
        tasks.push(ScheduledTask::health_check());
        Self { scripts_dir, tasks }
    }

    /// Run the daily report — queries all bugs and sends summary.
    pub fn run_daily_report(&self) -> anyhow::Result<String> {
        let script = self.scripts_dir.join("zentao-all-bugs.sh");
        if !script.exists() {
            anyhow::bail!("Script not found: {:?}", script);
        }

        // Refresh token first
        self.refresh_token();

        let output = Command::new("bash")
            .arg(&script)
            .arg("20")
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let msg = format!(
            "每日 Bug 汇总 ({})\n\n{}",
            Local::now().format("%Y-%m-%d"),
            stdout.trim(),
        );

        Ok(msg)
    }

    /// Run health check on all 8 agent processes.
    pub fn run_health_check(&self) -> Vec<(String, bool)> {
        let agents = [
            "zhugeliang", "liubei", "guanyu", "zhaoyun",
            "xunyu", "zhangfei", "huatuo", "chenlin",
        ];
        let mut results = Vec::new();

        for agent in agents {
            let output = Command::new("systemctl")
                .args(["is-active", &format!("agentforge-executor@{}", agent)])
                .output();
            let ok = output.map(|o| o.status.success()).unwrap_or(false);
            results.push((agent.to_string(), ok));
        }
        results
    }

    fn refresh_token(&self) {
        let script = self.scripts_dir.join("zentao-token-refresh.sh");
        let _ = Command::new("bash")
            .arg(script)
            .arg("zhangfei")
            .output();
    }

    /// Get all tasks.
    pub fn tasks(&self) -> &[ScheduledTask] {
        &self.tasks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_tasks() {
        let sched = Scheduler::new(
            PathBuf::from("/tmp"),
            vec![ScheduledTask::daily_report()],
        );
        let tasks = sched.tasks();
        assert!(tasks.len() >= 2); // daily_report + health_check
        assert!(tasks.iter().any(|t| t.name == "daily_report"));
        assert!(tasks.iter().any(|t| t.name == "health_check"));
    }

    #[test]
    fn test_scheduled_task_defaults() {
        let t = ScheduledTask::daily_report();
        assert_eq!(t.time, "09:00");
        assert!(t.enabled);
    }
}
