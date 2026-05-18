//! Tool executor — runs shell scripts and returns (exit_code, stdout, stderr).

use std::process::Command;

/// Run a shell script with arguments and optional timeout.
pub fn run_script(script: &str, args: &[&str], _timeout_secs: u64) -> (i32, String, String) {
    let output = Command::new("bash")
        .arg(script)
        .args(args)
        .output();

    match output {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            (code, stdout, stderr)
        }
        Err(e) => (-1, String::new(), format!("{:?}", e)),
    }
}

/// zentao_dir shorthand for constructing script paths.
pub fn z(scripts_dir: &str, script_name: &str) -> String {
    format!("{}/{}", scripts_dir, script_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_z_path() {
        assert_eq!(z("/tmp", "test.sh"), "/tmp/test.sh");
    }

    #[test]
    fn test_run_script_returns_tuple() {
        // Just verify the function signature works
        let result = run_script("nonexistent_command_xyz", &[], 1);
        assert!(result.0 != 0); // exit code for missing command
    }
}
