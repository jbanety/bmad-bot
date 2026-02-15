use bmad_bot::cli::state::{DaemonState, STATE_FILE_NAME};
use bmad_bot::config::discovery::BmadDiscovery;
use bmad_bot::config::BotConfig;
use std::path::PathBuf;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_test_discovery() -> BmadDiscovery {
    BmadDiscovery {
        bmad_version: Some("6.0.0-test".to_string()),
        installed_modules: vec!["bmm".to_string()],
        config_path: None,
        project_root: PathBuf::from("."),
        bmad_detected: true,
    }
}

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

// ===========================================================================
// Task 2: DaemonState read/write roundtrip tests (AC #1, #2)
// ===========================================================================

/// AC #1: read() on non-existent file returns Ok(None)
#[test]
fn test_daemon_state_read_nonexistent_returns_none() {
    let dir = TempDir::new().expect("tempdir");
    let state_path = dir.path().join(STATE_FILE_NAME);
    let result = DaemonState::read(&state_path).expect("read should not error");
    assert!(result.is_none(), "Expected None for non-existent state file");
}

/// AC #2: construct DaemonState manually → write() → read() → verify all fields match
#[test]
fn test_daemon_state_manual_write_read_roundtrip() {
    let dir = TempDir::new().expect("tempdir");
    let state_path = dir.path().join(STATE_FILE_NAME);

    let state = make_manual_state();
    state.write(&state_path).expect("write");

    let loaded = DaemonState::read(&state_path)
        .expect("read")
        .expect("should be Some");

    assert_eq!(loaded.pid, state.pid);
    assert_eq!(loaded.started_at, state.started_at);
    assert_eq!(loaded.last_activity, state.last_activity);
    assert_eq!(loaded.status, state.status);
    assert_eq!(loaded.log_file, state.log_file);
    assert!(loaded.bmad_discovery.is_none());
    assert_eq!(loaded.stories_processed, state.stories_processed);
}

/// AC #2: new_running() → write() → read() → verify pid, status, stories_processed
#[test]
fn test_daemon_state_new_running_roundtrip() {
    let dir = TempDir::new().expect("tempdir");
    let state_path = dir.path().join(STATE_FILE_NAME);

    let state = DaemonState::new_running(PathBuf::from("daemon.log"), make_test_discovery());
    state.write(&state_path).expect("write");

    let loaded = DaemonState::read(&state_path)
        .expect("read")
        .expect("should be Some");

    assert_eq!(loaded.pid, std::process::id());
    assert_eq!(loaded.status, "running");
    assert_eq!(loaded.stories_processed, 0);
    assert_eq!(loaded.log_file, PathBuf::from("daemon.log"));
    assert!(loaded.bmad_discovery.is_some());
}

// ===========================================================================
// Task 3: DaemonState mutation + persistence tests (AC #3, #4)
// ===========================================================================

/// AC #3: new_running → touch → record_story_processed ×2 → write → read → assert
#[test]
fn test_daemon_state_touch_and_record_stories_persist() {
    let dir = TempDir::new().expect("tempdir");
    let state_path = dir.path().join(STATE_FILE_NAME);

    let mut state = DaemonState::new_running(PathBuf::from("test.log"), make_test_discovery());
    let original_activity = state.last_activity.clone();

    std::thread::sleep(std::time::Duration::from_millis(10));
    state.touch();
    state.record_story_processed();
    state.record_story_processed();
    state.write(&state_path).expect("write");

    let loaded = DaemonState::read(&state_path)
        .expect("read")
        .expect("should be Some");

    assert_eq!(loaded.stories_processed, 2);
    assert_ne!(
        loaded.last_activity, original_activity,
        "last_activity should have been updated by touch()"
    );
}

/// AC #4: new_running → write → re-read → mark_stopped → write → re-read → assert stopped
#[test]
fn test_daemon_state_mark_stopped_persists() {
    let dir = TempDir::new().expect("tempdir");
    let state_path = dir.path().join(STATE_FILE_NAME);

    let state = DaemonState::new_running(PathBuf::from("test.log"), make_test_discovery());
    state.write(&state_path).expect("write");

    let mut loaded = DaemonState::read(&state_path)
        .expect("read")
        .expect("should be Some");
    assert_eq!(loaded.status, "running");

    let before_activity = loaded.last_activity.clone();
    std::thread::sleep(std::time::Duration::from_millis(10));
    loaded.mark_stopped();
    loaded.write(&state_path).expect("write stopped");

    let reloaded = DaemonState::read(&state_path)
        .expect("read")
        .expect("should be Some");
    assert_eq!(reloaded.status, "stopped");
    assert_ne!(
        reloaded.last_activity, before_activity,
        "last_activity should be updated after mark_stopped()"
    );
}

/// touch() updates last_activity but not status or stories_processed
#[test]
fn test_daemon_state_touch_only_updates_last_activity() {
    let state = DaemonState::new_running(PathBuf::from("test.log"), make_test_discovery());
    let original_status = state.status.clone();
    let original_stories = state.stories_processed;
    let original_activity = state.last_activity.clone();

    // Clone then mutate
    let mut state = state;

    std::thread::sleep(std::time::Duration::from_millis(10));
    state.touch();

    assert_ne!(state.last_activity, original_activity);
    assert_eq!(state.status, original_status);
    assert_eq!(state.stories_processed, original_stories);
}

// ===========================================================================
// Task 4: DaemonState cleanup tests (AC #5)
// ===========================================================================

/// AC #5: write → verify exists → cleanup → verify removed → read returns Ok(None)
#[test]
fn test_daemon_state_cleanup_lifecycle() {
    let dir = TempDir::new().expect("tempdir");
    let state_path = dir.path().join(STATE_FILE_NAME);

    let state = make_manual_state();
    state.write(&state_path).expect("write");
    assert!(state_path.exists(), "state file should exist after write");

    DaemonState::cleanup(&state_path).expect("cleanup");
    assert!(
        !state_path.exists(),
        "state file should be removed after cleanup"
    );

    let result = DaemonState::read(&state_path).expect("read after cleanup");
    assert!(result.is_none(), "read after cleanup should return None");
}

/// cleanup() on non-existent file does not error (idempotent)
#[test]
fn test_daemon_state_cleanup_idempotent() {
    let dir = TempDir::new().expect("tempdir");
    let state_path = dir.path().join("nonexistent.state.json");
    DaemonState::cleanup(&state_path).expect("cleanup on nonexistent should not error");
}

// ===========================================================================
// Task 5: BotConfig load roundtrip test (AC #6)
// ===========================================================================

/// AC #6: construct BotConfig → serialize to YAML → write → load() → validate() → assert fields
#[test]
fn test_bot_config_roundtrip() {
    let config = BotConfig::_test_minimal("pretty", "info");

    let yaml = serde_yml::to_string(&config).expect("serialize to YAML");

    let dir = TempDir::new().expect("tempdir");
    let config_path = dir.path().join("bmad-bot.yaml");
    std::fs::write(&config_path, &yaml).expect("write YAML");

    let loaded = BotConfig::load(&config_path).expect("load config");
    loaded.validate().expect("validate config");

    assert_eq!(loaded.polling_interval_secs, config.polling_interval_secs);
    assert_eq!(
        loaded.git_provider.provider,
        config.git_provider.provider
    );
    assert_eq!(loaded.llm.dev.provider, config.llm.dev.provider);
}

/// Load from malformed YAML → verify ConfigError returned (not panic)
#[test]
fn test_bot_config_load_malformed_yaml() {
    let dir = TempDir::new().expect("tempdir");
    let config_path = dir.path().join("bmad-bot.yaml");
    std::fs::write(&config_path, "{{{{not: valid:: yaml").expect("write bad YAML");

    let result = BotConfig::load(&config_path);
    assert!(
        result.is_err(),
        "Loading malformed YAML should return an error"
    );
}

// ===========================================================================
// Task 6: Cross-concern integration tests (AC: ALL)
// ===========================================================================

/// Full lifecycle: create → write → touch → record 3 stories → mark_stopped → write → read → verify
#[test]
fn test_daemon_state_full_lifecycle() {
    let dir = TempDir::new().expect("tempdir");
    let state_path = dir.path().join(STATE_FILE_NAME);

    let mut state = DaemonState::new_running(PathBuf::from("daemon.log"), make_test_discovery());
    let started_at = state.started_at.clone();
    state.write(&state_path).expect("initial write");

    std::thread::sleep(std::time::Duration::from_millis(10));
    state.touch();
    let after_touch = state.last_activity.clone();

    state.record_story_processed();
    state.record_story_processed();
    state.record_story_processed();

    std::thread::sleep(std::time::Duration::from_millis(10));
    state.mark_stopped();
    state.write(&state_path).expect("final write");

    let loaded = DaemonState::read(&state_path)
        .expect("read")
        .expect("should be Some");

    // Final state is coherent
    assert_eq!(loaded.status, "stopped");
    assert_eq!(loaded.stories_processed, 3);
    assert_eq!(loaded.pid, std::process::id());

    // Timestamps are monotonically ordered: started_at <= after_touch <= last_activity(stopped)
    assert!(
        started_at <= after_touch,
        "started_at should be <= after_touch"
    );
    assert!(
        after_touch <= loaded.last_activity,
        "after_touch should be <= last_activity (stopped)"
    );
}

/// State file is valid JSON: write → read raw → parse as serde_json::Value → verify keys
#[test]
fn test_daemon_state_file_is_valid_json() {
    let dir = TempDir::new().expect("tempdir");
    let state_path = dir.path().join(STATE_FILE_NAME);

    let state = make_manual_state();
    state.write(&state_path).expect("write");

    let raw = std::fs::read_to_string(&state_path).expect("read raw file");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("parse JSON");

    assert!(parsed.get("pid").is_some(), "JSON should have 'pid' key");
    assert!(
        parsed.get("started_at").is_some(),
        "JSON should have 'started_at' key"
    );
    assert!(
        parsed.get("last_activity").is_some(),
        "JSON should have 'last_activity' key"
    );
    assert!(
        parsed.get("status").is_some(),
        "JSON should have 'status' key"
    );
    assert!(
        parsed.get("log_file").is_some(),
        "JSON should have 'log_file' key"
    );
    assert!(
        parsed.get("stories_processed").is_some(),
        "JSON should have 'stories_processed' key"
    );
}
