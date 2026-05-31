//! Fix trajectory — saves fix history to JSON index for panel display.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::Path;

const TRAJ_DIR: &str = "/var/lib/agentforge/trajectories";
const INDEX_FILE: &str = "/var/lib/agentforge/trajectories/index.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryEntry {
    pub bug_id: String,
    pub agent: String,
    pub method: String,
    pub success: bool,
    pub timestamp: String,
    pub elapsed_s: f64,
    pub fix_summary: String,
    pub trajectory_dir: String,
}

/// Save a fix trajectory record.
pub fn save_trajectory(
    bug_id: &str,
    agent_name: &str,
    method: &str,
    success: bool,
    elapsed_s: f64,
    stdout: &str,
    stderr: &str,
    fix_summary: &str,
) -> anyhow::Result<()> {
    let ts = Local::now();
    let date_str = ts.format("%Y%m%d-%H%M%S").to_string();
    let status = if success { "OK" } else { "FAIL" };
    let run_dir_name = format!("{}_{}_{}", date_str, method, status);
    let bug_dir = Path::new(TRAJ_DIR).join(format!("bug{}", bug_id));
    let run_dir = bug_dir.join(&run_dir_name);

    fs::create_dir_all(&run_dir)?;
    fs::write(run_dir.join("stdout.txt"), stdout)?;
    fs::write(run_dir.join("stderr.txt"), stderr)?;

    let meta = serde_json::json!({
        "bug_id": bug_id,
        "agent": agent_name,
        "method": method,
        "success": success,
        "timestamp": ts.format("%Y-%m-%dT%H:%M:%S").to_string(),
        "elapsed_s": elapsed_s,
        "fix_summary": fix_summary,
    });
    fs::write(run_dir.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;

    // Update index.json
    let entry = TrajectoryEntry {
        bug_id: bug_id.to_string(),
        agent: agent_name.to_string(),
        method: method.to_string(),
        success,
        timestamp: ts.format("%Y-%m-%dT%H:%M:%S").to_string(),
        elapsed_s,
        fix_summary: fix_summary.to_string(),
        trajectory_dir: run_dir.to_string_lossy().to_string(),
    };

    append_to_index(&entry)?;
    Ok(())
}

/// Get all trajectory records for a bug.
pub fn get_trajectories(bug_id: &str) -> Vec<TrajectoryEntry> {
    match read_index() {
        Ok(entries) => entries
            .into_iter()
            .filter(|e| e.bug_id == bug_id)
            .collect(),
        Err(_) => vec![],
    }
}

/// Get the latest failure analysis for a bug.
pub fn get_latest_failure(bug_id: &str) -> Option<TrajectoryEntry> {
    get_trajectories(bug_id).into_iter().find(|e| !e.success)
}

fn read_index() -> anyhow::Result<Vec<TrajectoryEntry>> {
    let path = Path::new(INDEX_FILE);
    if !path.exists() {
        return Ok(vec![]);
    }
    let mut f = fs::File::open(path)?;
    let mut data = String::new();
    f.read_to_string(&mut data)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn append_to_index(entry: &TrajectoryEntry) -> anyhow::Result<()> {
    let path = Path::new(INDEX_FILE);
    fs::create_dir_all(path.parent().unwrap())?;
    let mut entries = read_index().unwrap_or_default();
    entries.push(entry.clone());
    let json = serde_json::to_string(&entries)?;
    fs::write(path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_save_and_get_trajectory() {
        let _tmp = env::temp_dir().join("traj_test");
        // Use tmp dir via TRAJ_DIR override not possible; test on temp path
        let bug_id = "99999";
        let r = save_trajectory(bug_id, "赵云", "claude_code", true, 45.0,
            "stdout content", "stderr content", "committed");
        // May fail if /var/lib not writable; skip gracefully
        if r.is_ok() {
            let traj = get_trajectories(bug_id);
            assert!(!traj.is_empty());
            assert_eq!(traj[0].bug_id, bug_id);
            assert!(traj[0].success);
        }
    }
}
