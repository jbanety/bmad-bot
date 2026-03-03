//! Daemon state file tracking for the `status` command.
//!
//! The daemon writes a `bmad-bot.state.json` file while running so that
//! `bmad-bot status` can report the daemon's state from a separate process.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use bmad_bot::config::discovery::BmadDiscovery;

/// Default state file name, written in the current working directory.
pub const STATE_FILE_NAME: &str = "bmad-bot.state.json";

/// Persistent daemon state written to disk for cross-process communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonState {
    /// Process ID of the running daemon.
    pub pid: u32,
    /// ISO 8601 timestamp when daemon started.
    pub started_at: String,
    /// ISO 8601 timestamp of last activity (poll cycle, story processing).
    pub last_activity: String,
    /// Current status: `"running"` or `"stopped"`.
    pub status: String,
    /// Path to the log file.
    pub log_file: PathBuf,
    /// BMAD discovery results from startup.
    pub bmad_discovery: Option<BmadDiscovery>,
    /// Number of stories processed during this daemon session (AC #1: "stories processed count").
    pub stories_processed: usize,
}

impl DaemonState {
    /// Create a new state for a freshly started daemon.
    pub fn new_running(log_file: PathBuf, bmad_discovery: BmadDiscovery) -> Self {
        let now = chrono::Local::now().to_rfc3339();
        Self {
            pid: std::process::id(),
            started_at: now.clone(),
            last_activity: now,
            status: "running".to_string(),
            log_file,
            bmad_discovery: Some(bmad_discovery),
            stories_processed: 0,
        }
    }

    /// Increment the stories_processed counter by one.
    #[allow(dead_code)] // Used by Story 2.1 watcher
    pub fn record_story_processed(&mut self) {
        self.stories_processed += 1;
    }

    /// Update the last_activity timestamp to now.
    pub fn touch(&mut self) {
        self.last_activity = chrono::Local::now().to_rfc3339();
    }

    /// Mark state as stopped.
    pub fn mark_stopped(&mut self) {
        self.status = "stopped".to_string();
        self.last_activity = chrono::Local::now().to_rfc3339();
    }

    /// Write state to the state file (atomic: write to tmp then rename).
    pub fn write(&self, path: &Path) -> Result<(), super::CliError> {
        let json = serde_json::to_string_pretty(self).map_err(|e| super::CliError::State {
            reason: format!("Failed to serialize daemon state: {e}"),
        })?;
        // Write to a temporary file then rename for atomicity
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Read state from the state file. Returns `None` if file doesn't exist.
    pub fn read(path: &Path) -> Result<Option<Self>, super::CliError> {
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        let state: Self = serde_json::from_str(&content).map_err(|e| super::CliError::State {
            reason: format!("Failed to parse daemon state: {e}"),
        })?;
        Ok(Some(state))
    }

    /// Check if a given PID is still alive (Unix: macOS + Linux).
    ///
    /// Uses POSIX `kill -0` via `std::process::Command` — no `libc` or `unsafe` needed.
    /// Returns `false` for PID 0 or if the process doesn't exist.
    pub fn is_process_alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Remove the state file.
    pub fn cleanup(path: &Path) -> Result<(), super::CliError> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_test_state() -> DaemonState {
        DaemonState {
            pid: std::process::id(),
            started_at: "2026-02-07T10:00:00+01:00".to_string(),
            last_activity: "2026-02-07T10:05:00+01:00".to_string(),
            status: "running".to_string(),
            log_file: PathBuf::from("bmad-bot.log"),
            bmad_discovery: None,
            stories_processed: 0,
        }
    }

    #[test]
    fn test_state_write_and_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("test.state.json");
        let state = make_test_state();

        state.write(&state_path).unwrap();
        let loaded = DaemonState::read(&state_path).unwrap().unwrap();

        assert_eq!(loaded.pid, state.pid);
        assert_eq!(loaded.started_at, state.started_at);
        assert_eq!(loaded.status, "running");
    }

    #[test]
    fn test_state_read_returns_none_for_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("nonexistent.json");
        let result = DaemonState::read(&state_path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_state_cleanup_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("test.state.json");
        let state = make_test_state();
        state.write(&state_path).unwrap();
        assert!(state_path.exists());

        DaemonState::cleanup(&state_path).unwrap();
        assert!(!state_path.exists());
    }

    #[test]
    fn test_state_cleanup_noop_for_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("nonexistent.json");
        // Should not error
        DaemonState::cleanup(&state_path).unwrap();
    }

    #[test]
    fn test_is_process_alive_with_current_pid() {
        assert!(DaemonState::is_process_alive(std::process::id()));
    }

    #[test]
    fn test_is_process_alive_with_zero_pid() {
        assert!(!DaemonState::is_process_alive(0));
    }

    #[test]
    fn test_touch_updates_last_activity() {
        let mut state = make_test_state();
        let before = state.last_activity.clone();
        std::thread::sleep(std::time::Duration::from_millis(10));
        state.touch();
        assert_ne!(state.last_activity, before);
    }

    #[test]
    fn test_mark_stopped() {
        let mut state = make_test_state();
        assert_eq!(state.status, "running");
        state.mark_stopped();
        assert_eq!(state.status, "stopped");
    }

    #[test]
    fn test_record_story_processed_increments() {
        let mut state = make_test_state();
        assert_eq!(state.stories_processed, 0);
        state.record_story_processed();
        assert_eq!(state.stories_processed, 1);
        state.record_story_processed();
        assert_eq!(state.stories_processed, 2);
    }

    #[test]
    fn test_stories_processed_survives_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("test.state.json");
        let mut state = make_test_state();
        state.record_story_processed();
        state.record_story_processed();
        state.record_story_processed();
        state.write(&state_path).unwrap();

        let loaded = DaemonState::read(&state_path).unwrap().unwrap();
        assert_eq!(loaded.stories_processed, 3);
    }

    #[test]
    fn test_new_running_sets_fields_correctly() {
        let state = DaemonState::new_running(
            PathBuf::from("test.log"),
            BmadDiscovery {
                bmad_version: Some("1.0.0".to_string()),
                installed_modules: vec!["bmm".to_string()],
                config_path: None,
                project_root: PathBuf::from("."),
                bmad_detected: true,
            },
        );
        assert_eq!(state.pid, std::process::id());
        assert_eq!(state.status, "running");
        assert_eq!(state.log_file, PathBuf::from("test.log"));
        assert_eq!(state.stories_processed, 0);
        assert!(state.bmad_discovery.is_some());
    }

    #[test]
    fn test_state_write_is_valid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("test.state.json");
        let state = make_test_state();
        state.write(&state_path).unwrap();

        let content = std::fs::read_to_string(&state_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["status"], "running");
        assert_eq!(parsed["stories_processed"], 0);
    }

    #[test]
    fn test_mark_stopped_updates_last_activity() {
        let mut state = make_test_state();
        let before = state.last_activity.clone();
        std::thread::sleep(std::time::Duration::from_millis(10));
        state.mark_stopped();
        assert_ne!(state.last_activity, before);
        assert_eq!(state.status, "stopped");
    }
}
