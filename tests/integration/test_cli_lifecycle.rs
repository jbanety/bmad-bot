//! Integration tests for CLI daemon state lifecycle (Story 7.9).
//!
//! Tests verify the full DaemonState lifecycle: create → mutate → persist →
//! re-read → cleanup, plus BotConfig roundtrip loading.

use bmad_bot::cli::state::{DaemonState, STATE_FILE_NAME};
use bmad_bot::config::discovery::BmadDiscovery;
use bmad_bot::config::BotConfig;
use std::path::PathBuf;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal `BmadDiscovery` for tests.
fn test_discovery() -> BmadDiscovery {
    BmadDiscovery {
        bmad_version: Some("6.0.0-test".to_string()),
        installed_modules: vec!["bmm".to_string()],
        config_path: None,
        project_root: PathBuf::from("."),
        bmad_detected: true,
    }
}

/// Build a `DaemonState` with explicit field values (no `new_running()` dependency).
fn make_manual_state() -> DaemonState {
    DaemonState {
        pid: std::process::id(),
        started_at: "2026-02-08T10:00:00+01:00".to_string(),
        last_activity: "2026-02-08T10:00:00+01:00".to_string(),
        status: "running".to_string(),
        log_file: PathBuf::from("test.log"),
        bmad_discovery: None,
        stories_processed: 0,
    }
}

// ---------------------------------------------------------------------------
// Task 2 — DaemonState read/write roundtrip tests (AC #1, #2)
// ---------------------------------------------------------------------------

/// AC #1: read() on non-existent file returns Ok(None).
#[test]
fn test_daemon_state_read_nonexistent_returns_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STATE_FILE_NAME);

    let result = DaemonState::read(&path).expect("read should not error");
    assert!(result.is_none(), "expected None for missing state file");
}

/// AC #2: manual construct → write → read → verify all fields match.
#[test]
fn test_daemon_state_manual_write_read_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STATE_FILE_NAME);

    let state = DaemonState {
        pid: 12345,
        started_at: "2026-02-08T10:00:00+01:00".to_string(),
        last_activity: "2026-02-08T10:05:00+01:00".to_string(),
        status: "running".to_string(),
        log_file: PathBuf::from("/tmp/test.log"),
        bmad_discovery: Some(test_discovery()),
        stories_processed: 42,
    };

    state.write(&path).expect("write");
    let loaded = DaemonState::read(&path)
        .expect("read")
        .expect("should be Some");

    assert_eq!(loaded.pid, 12345);
    assert_eq!(loaded.started_at, "2026-02-08T10:00:00+01:00");
    assert_eq!(loaded.last_activity, "2026-02-08T10:05:00+01:00");
    assert_eq!(loaded.status, "running");
    assert_eq!(loaded.log_file, PathBuf::from("/tmp/test.log"));
    assert_eq!(loaded.stories_processed, 42);
    let disc = loaded.bmad_discovery.expect("discovery present");
    assert_eq!(disc.bmad_version, Some("6.0.0-test".to_string()));
    assert!(disc.bmad_detected);
}

/// AC #2: new_running() → write → read → verify pid, status, stories_processed.
#[test]
fn test_daemon_state_new_running_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STATE_FILE_NAME);

    let state = DaemonState::new_running(PathBuf::from("daemon.log"), test_discovery());
    state.write(&path).expect("write");

    let loaded = DaemonState::read(&path)
        .expect("read")
        .expect("should be Some");

    assert_eq!(loaded.pid, std::process::id());
    assert_eq!(loaded.status, "running");
    assert_eq!(loaded.stories_processed, 0);
    assert_eq!(loaded.log_file, PathBuf::from("daemon.log"));
    // started_at should be a valid ISO 8601 timestamp
    assert!(loaded.started_at.starts_with("20"), "timestamp should start with year");
    assert!(loaded.started_at.contains('T'), "timestamp should contain T separator");
}

// ---------------------------------------------------------------------------
// Task 3 — DaemonState mutation + persistence tests (AC #3, #4)
// ---------------------------------------------------------------------------

/// AC #3: new_running → touch → record_story_processed ×2 → write → read →
/// assert stories_processed == 2 and last_activity differs from started_at.
#[test]
fn test_daemon_state_touch_and_record_stories_persist() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STATE_FILE_NAME);

    let mut state = DaemonState::new_running(PathBuf::from("test.log"), test_discovery());
    let original_started = state.started_at.clone();

    // Sleep to ensure timestamp differs
    std::thread::sleep(Duration::from_millis(10));
    state.touch();
    state.record_story_processed();
    state.record_story_processed();

    state.write(&path).expect("write");
    let loaded = DaemonState::read(&path)
        .expect("read")
        .expect("should be Some");

    assert_eq!(loaded.stories_processed, 2);
    assert_ne!(
        loaded.last_activity, original_started,
        "last_activity should differ from started_at after touch()"
    );
    assert_eq!(loaded.status, "running");
}

/// AC #4: new_running → write → re-read → mark_stopped → write → re-read →
/// assert status is "stopped" and last_activity is updated.
#[test]
fn test_daemon_state_mark_stopped_persists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STATE_FILE_NAME);

    let mut state = DaemonState::new_running(PathBuf::from("test.log"), test_discovery());
    state.write(&path).expect("write initial");

    let loaded_before = DaemonState::read(&path)
        .expect("read")
        .expect("some");
    assert_eq!(loaded_before.status, "running");
    let activity_before = loaded_before.last_activity.clone();

    // Sleep to ensure timestamp differs
    std::thread::sleep(Duration::from_millis(10));
    state.mark_stopped();
    state.write(&path).expect("write stopped");

    let loaded_after = DaemonState::read(&path)
        .expect("read")
        .expect("some");
    assert_eq!(loaded_after.status, "stopped");
    assert_ne!(
        loaded_after.last_activity, activity_before,
        "last_activity should be updated after mark_stopped()"
    );
}

/// Verify touch() updates last_activity but not status or stories_processed.
#[test]
fn test_daemon_state_touch_only_updates_last_activity() {
    let mut state = make_manual_state();
    let original_status = state.status.clone();
    let original_stories = state.stories_processed;
    let original_activity = state.last_activity.clone();

    std::thread::sleep(Duration::from_millis(10));
    state.touch();

    assert_ne!(state.last_activity, original_activity, "last_activity should change");
    assert_eq!(state.status, original_status, "status should not change");
    assert_eq!(
        state.stories_processed, original_stories,
        "stories_processed should not change"
    );
}

// ---------------------------------------------------------------------------
// Task 4 — DaemonState cleanup tests (AC #5)
// ---------------------------------------------------------------------------

/// AC #5: write state → verify file exists → cleanup → verify removed → read returns None.
#[test]
fn test_daemon_state_cleanup_removes_file_and_read_returns_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STATE_FILE_NAME);

    let state = make_manual_state();
    state.write(&path).expect("write");
    assert!(path.exists(), "state file should exist after write");

    DaemonState::cleanup(&path).expect("cleanup");
    assert!(!path.exists(), "state file should be removed after cleanup");

    let result = DaemonState::read(&path).expect("read after cleanup");
    assert!(result.is_none(), "read after cleanup should return None");
}

/// cleanup on non-existent file does not error (idempotent).
#[test]
fn test_daemon_state_cleanup_nonexistent_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("does-not-exist.state.json");

    // Should not error
    DaemonState::cleanup(&path).expect("cleanup on missing file should succeed");
}

// ---------------------------------------------------------------------------
// Task 5 — BotConfig load roundtrip test (AC #6)
// ---------------------------------------------------------------------------

/// AC #6: construct BotConfig → serialize to YAML → write → load → validate → assert fields match.
#[test]
fn test_bot_config_yaml_roundtrip() {
    let config = BotConfig::_test_minimal("pretty", "info");

    let yaml = serde_yml::to_string(&config).expect("serialize to YAML");

    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("bmad-bot.yaml");
    std::fs::write(&config_path, &yaml).expect("write YAML");

    let loaded = BotConfig::load(&config_path).expect("load");
    loaded.validate().expect("validate");

    assert_eq!(loaded.polling_interval_secs, config.polling_interval_secs);
    assert_eq!(loaded.git_provider.provider, config.git_provider.provider);
    assert_eq!(loaded.git_provider.repo_owner, config.git_provider.repo_owner);
    assert_eq!(loaded.llm.dev.provider, config.llm.dev.provider);
    assert_eq!(loaded.llm.dev.model, config.llm.dev.model);
    assert_eq!(loaded.log_format, config.log_format);
    assert_eq!(loaded.log_level, config.log_level);
    assert_eq!(loaded.log_file, config.log_file);
}

/// Load from malformed YAML → verify ConfigError is returned (not a panic).
#[test]
fn test_bot_config_malformed_yaml_returns_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("bad.yaml");
    std::fs::write(&config_path, "not: [valid: yaml: {{{{").expect("write");

    let result = BotConfig::load(&config_path);
    assert!(result.is_err(), "malformed YAML should return Err");
}

// ---------------------------------------------------------------------------
// Task 6 — Cross-concern integration tests (AC: ALL)
// ---------------------------------------------------------------------------

/// Full lifecycle: create → write → touch → record 3 stories → mark_stopped →
/// write → read → verify final state is coherent.
#[test]
fn test_daemon_state_full_lifecycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STATE_FILE_NAME);

    let mut state = DaemonState::new_running(PathBuf::from("lifecycle.log"), test_discovery());
    let original_started = state.started_at.clone();

    state.write(&path).expect("initial write");

    // Mutate: touch + record stories
    std::thread::sleep(Duration::from_millis(10));
    state.touch();
    state.record_story_processed();
    state.record_story_processed();
    state.record_story_processed();

    // Mark stopped
    std::thread::sleep(Duration::from_millis(10));
    state.mark_stopped();
    state.write(&path).expect("final write");

    // Read back and verify coherence
    let loaded = DaemonState::read(&path)
        .expect("read")
        .expect("should be Some");

    assert_eq!(loaded.status, "stopped");
    assert_eq!(loaded.stories_processed, 3);
    assert_eq!(loaded.pid, std::process::id());
    assert_eq!(loaded.log_file, PathBuf::from("lifecycle.log"));

    // Timestamps should be monotonically ordered: started_at <= last_activity
    // (started_at is fixed, last_activity was updated by mark_stopped)
    assert_eq!(loaded.started_at, original_started, "started_at should not change");
    assert_ne!(
        loaded.last_activity, original_started,
        "last_activity should differ from started_at"
    );
    // Verify timestamp format
    assert!(loaded.started_at.contains('T'));
    assert!(loaded.last_activity.contains('T'));
}

/// State file is valid JSON: write → read raw → parse as serde_json::Value → verify keys.
#[test]
fn test_daemon_state_file_is_valid_json_with_expected_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STATE_FILE_NAME);

    let state = DaemonState::new_running(PathBuf::from("json-test.log"), test_discovery());
    state.write(&path).expect("write");

    let raw = std::fs::read_to_string(&path).expect("read raw file");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("parse JSON");

    // Verify all expected keys exist
    assert!(parsed.get("pid").is_some(), "missing 'pid' key");
    assert!(parsed.get("started_at").is_some(), "missing 'started_at' key");
    assert!(parsed.get("last_activity").is_some(), "missing 'last_activity' key");
    assert!(parsed.get("status").is_some(), "missing 'status' key");
    assert!(parsed.get("log_file").is_some(), "missing 'log_file' key");
    assert!(
        parsed.get("stories_processed").is_some(),
        "missing 'stories_processed' key"
    );
    assert!(
        parsed.get("bmad_discovery").is_some(),
        "missing 'bmad_discovery' key"
    );

    // Verify values
    assert_eq!(parsed["status"], "running");
    assert_eq!(parsed["stories_processed"], 0);
}
