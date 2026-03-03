// Tests for Story 7.9: CLI Lifecycle Integration Tests
//
// Validates DaemonState lifecycle (create → mutate → persist → re-read → cleanup)
// and BotConfig roundtrip serialization from integration test context.

use bmad_bot::cli::state::{DaemonState, STATE_FILE_NAME};
use bmad_bot::config::discovery::BmadDiscovery;
use bmad_bot::config::BotConfig;
use std::path::PathBuf;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Construct a minimal DaemonState with deterministic fields for testing.
fn make_test_state() -> DaemonState {
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

/// Construct a minimal BmadDiscovery for use with `new_running()`.
fn make_test_discovery() -> BmadDiscovery {
    BmadDiscovery {
        bmad_version: Some("6.0.0-test".to_string()),
        installed_modules: vec!["bmm".to_string()],
        config_path: None,
        project_root: PathBuf::from("."),
        bmad_detected: true,
    }
}

// ---------------------------------------------------------------------------
// Task 2: DaemonState read/write roundtrip tests (AC #1, #2)
// ---------------------------------------------------------------------------

#[test]
fn test_daemon_state_read_nonexistent_returns_none() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join(STATE_FILE_NAME);

    let result = DaemonState::read(&state_path).expect("read should not error");
    assert!(result.is_none(), "read on non-existent file should return None");
}

#[test]
fn test_daemon_state_manual_construct_write_read_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join(STATE_FILE_NAME);

    let state = make_test_state();
    state.write(&state_path).expect("write");

    let loaded = DaemonState::read(&state_path)
        .expect("read should not error")
        .expect("state should exist");

    assert_eq!(loaded.pid, state.pid);
    assert_eq!(loaded.started_at, state.started_at);
    assert_eq!(loaded.last_activity, state.last_activity);
    assert_eq!(loaded.status, "running");
    assert_eq!(loaded.log_file, state.log_file);
    assert_eq!(loaded.stories_processed, 0);
    assert!(loaded.bmad_discovery.is_none());
}

#[test]
fn test_daemon_state_new_running_write_read_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join(STATE_FILE_NAME);

    let state = DaemonState::new_running(PathBuf::from("daemon.log"), make_test_discovery());
    state.write(&state_path).expect("write");

    let loaded = DaemonState::read(&state_path)
        .expect("read should not error")
        .expect("state should exist");

    assert_eq!(loaded.pid, std::process::id());
    assert_eq!(loaded.status, "running");
    assert_eq!(loaded.stories_processed, 0);
    assert_eq!(loaded.log_file, PathBuf::from("daemon.log"));
    assert!(loaded.bmad_discovery.is_some());
    // AC #2 explicitly requires verifying started_at is preserved through roundtrip
    assert!(
        !loaded.started_at.is_empty(),
        "started_at should be a non-empty ISO 8601 timestamp"
    );
    assert_eq!(
        loaded.started_at, state.started_at,
        "started_at must survive write/read roundtrip unchanged"
    );

    let disc = loaded.bmad_discovery.unwrap();
    assert_eq!(disc.bmad_version, Some("6.0.0-test".to_string()));
    assert!(disc.bmad_detected);
}

// ---------------------------------------------------------------------------
// Task 3: DaemonState mutation + persistence tests (AC #3, #4)
// ---------------------------------------------------------------------------

#[test]
fn test_daemon_state_touch_record_story_write_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join(STATE_FILE_NAME);

    let mut state = DaemonState::new_running(PathBuf::from("daemon.log"), make_test_discovery());
    let original_activity = state.last_activity.clone();

    // Sleep to ensure timestamp differs
    std::thread::sleep(Duration::from_millis(10));

    state.touch();
    state.record_story_processed();
    state.record_story_processed();
    state.write(&state_path).expect("write");

    let loaded = DaemonState::read(&state_path)
        .expect("read should not error")
        .expect("state should exist");

    assert_eq!(loaded.stories_processed, 2);
    assert_ne!(
        loaded.last_activity, original_activity,
        "last_activity should have been updated by touch()"
    );
    assert_eq!(loaded.status, "running");
}

#[test]
fn test_daemon_state_mark_stopped_write_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join(STATE_FILE_NAME);

    let mut state = DaemonState::new_running(PathBuf::from("daemon.log"), make_test_discovery());
    state.write(&state_path).expect("write first");

    // Re-read to confirm running
    let loaded = DaemonState::read(&state_path)
        .expect("read")
        .expect("state");
    assert_eq!(loaded.status, "running");
    let before_activity = loaded.last_activity.clone();

    // Sleep to ensure timestamp differs
    std::thread::sleep(Duration::from_millis(10));

    // Mutate and re-persist
    let mut state = loaded;
    state.mark_stopped();
    state.write(&state_path).expect("write stopped");

    let final_state = DaemonState::read(&state_path)
        .expect("read")
        .expect("state");
    assert_eq!(final_state.status, "stopped");
    assert_ne!(
        final_state.last_activity, before_activity,
        "last_activity should have been updated by mark_stopped()"
    );
}

#[test]
fn test_daemon_state_touch_only_updates_last_activity() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join(STATE_FILE_NAME);

    let state = DaemonState::new_running(PathBuf::from("daemon.log"), make_test_discovery());
    let original_status = state.status.clone();
    let original_stories = state.stories_processed;
    let original_activity = state.last_activity.clone();

    std::thread::sleep(Duration::from_millis(10));
    let mut state = state;
    state.touch();
    state.write(&state_path).expect("write");

    let loaded = DaemonState::read(&state_path)
        .expect("read")
        .expect("state");

    assert_eq!(loaded.status, original_status, "touch should not change status");
    assert_eq!(
        loaded.stories_processed, original_stories,
        "touch should not change stories_processed"
    );
    assert_ne!(
        loaded.last_activity, original_activity,
        "touch should update last_activity"
    );
}

// ---------------------------------------------------------------------------
// Task 4: DaemonState cleanup tests (AC #5)
// ---------------------------------------------------------------------------

#[test]
fn test_daemon_state_cleanup_removes_file_and_read_returns_none() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join(STATE_FILE_NAME);

    let state = make_test_state();
    state.write(&state_path).expect("write");
    assert!(state_path.exists(), "state file should exist after write");

    DaemonState::cleanup(&state_path).expect("cleanup");
    assert!(!state_path.exists(), "state file should be removed after cleanup");

    let result = DaemonState::read(&state_path).expect("read should not error");
    assert!(result.is_none(), "read after cleanup should return None");
}

#[test]
fn test_daemon_state_cleanup_idempotent_on_missing_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join("nonexistent.json");

    // Should not error even though file doesn't exist
    DaemonState::cleanup(&state_path).expect("cleanup on missing file should not error");
}

// ---------------------------------------------------------------------------
// Task 5: BotConfig load roundtrip test (AC #6)
// ---------------------------------------------------------------------------

#[test]
fn test_botconfig_roundtrip_serialize_load_validate() {
    let config = BotConfig::_test_minimal("pretty", "info");

    let yaml = serde_yml::to_string(&config).expect("serialize to yaml");

    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("bmad-bot.yaml");
    std::fs::write(&config_path, &yaml).expect("write config yaml");

    let loaded = BotConfig::load(&config_path).expect("load config");
    loaded.validate().expect("validate config");

    assert_eq!(loaded.polling_interval_secs, config.polling_interval_secs);
    assert_eq!(loaded.git_provider.provider, config.git_provider.provider);
    assert_eq!(loaded.llm.dev.provider, config.llm.dev.provider);
    assert_eq!(loaded.log_format, "pretty");
    assert_eq!(loaded.log_level, "info");
}

#[test]
fn test_botconfig_load_malformed_yaml_returns_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("bmad-bot.yaml");
    std::fs::write(&config_path, "{{{{not valid yaml: [[[").expect("write bad yaml");

    let result = BotConfig::load(&config_path);
    assert!(result.is_err(), "loading malformed YAML should return an error");
}

// ---------------------------------------------------------------------------
// Task 6: Cross-concern integration tests (AC: ALL)
// ---------------------------------------------------------------------------

#[test]
fn test_daemon_state_full_lifecycle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join(STATE_FILE_NAME);

    // Create
    let mut state = DaemonState::new_running(PathBuf::from("lifecycle.log"), make_test_discovery());
    let started_at = state.started_at.clone();
    state.write(&state_path).expect("write initial");

    // Touch
    std::thread::sleep(Duration::from_millis(10));
    state.touch();
    let after_touch = state.last_activity.clone();

    // Record 3 stories (with small sleeps to ensure timestamp ordering)
    std::thread::sleep(Duration::from_millis(10));
    state.record_story_processed();
    state.record_story_processed();
    state.record_story_processed();

    // Mark stopped
    std::thread::sleep(Duration::from_millis(10));
    state.mark_stopped();
    state.write(&state_path).expect("write final");

    // Re-read and verify
    let final_state = DaemonState::read(&state_path)
        .expect("read")
        .expect("state");

    assert_eq!(final_state.status, "stopped");
    assert_eq!(final_state.stories_processed, 3);
    assert_eq!(final_state.started_at, started_at, "started_at should never change");

    // Timestamps should be monotonically ordered: started_at <= after_touch <= last_activity
    assert!(
        started_at <= after_touch,
        "after_touch ({after_touch}) should be >= started_at ({started_at})"
    );
    assert!(
        after_touch <= final_state.last_activity,
        "final last_activity ({}) should be >= after_touch ({after_touch})",
        final_state.last_activity
    );
}

#[test]
fn test_daemon_state_file_is_valid_json_with_expected_keys() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state_path = tmp.path().join(STATE_FILE_NAME);

    let state = make_test_state();
    state.write(&state_path).expect("write");

    let content = std::fs::read_to_string(&state_path).expect("read raw file");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse as JSON");

    let obj = parsed.as_object().expect("should be a JSON object");
    let expected_keys = [
        "pid",
        "started_at",
        "last_activity",
        "status",
        "log_file",
        "bmad_discovery",
        "stories_processed",
    ];
    for key in &expected_keys {
        assert!(obj.contains_key(*key), "JSON should contain key '{key}'");
    }

    assert_eq!(parsed["status"], "running");
    assert_eq!(parsed["stories_processed"], 0);
    assert_eq!(parsed["pid"], std::process::id());
}
