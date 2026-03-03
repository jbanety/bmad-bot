//! Integration tests for the Watcher → Dependency Resolution → Story Selection chain.
//!
//! Covers Story 7.3 — AC #1 through #5.
//! These tests exercise the real `Watcher`, `SprintStatusFile`, `DependencyGraph`,
//! and `deps` module functions together — no mocks. The only external dependency
//! is the filesystem, isolated via `tempfile::tempdir()`.

use std::collections::HashMap;
use std::sync::Arc;

use bmad_bot::watcher::deps::{build_full_dependency_map, find_cascade_blocks, DependencyGraph};
use bmad_bot::watcher::{SprintStatusFile, Watcher, WatcherError};

use crate::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};

// ---------------------------------------------------------------------------
// Helper: create the implementation artifacts directory and write sprint status
// ---------------------------------------------------------------------------

/// Sets up temp directory structure matching `make_test_config` expectations.
/// `make_test_config(dir)` sets `implementation_artifacts` to
/// `{dir}/_bmad-output/implementation-artifacts`, so sprint-status.yaml
/// must be written there.
fn setup_sprint_status(tmp: &std::path::Path, entries: &[(&str, &str)]) -> std::path::PathBuf {
    let impl_artifacts = tmp.join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&impl_artifacts).expect("create impl_artifacts dir");
    write_sprint_status(&impl_artifacts, entries);
    impl_artifacts
}

/// Build a `Watcher` from a temp directory with `make_test_config`.
fn make_watcher(tmp: &std::path::Path) -> Watcher {
    let config = make_test_config(tmp);
    Watcher::new(Arc::new(config))
}

// ===========================================================================
// Task 2 — AC #1: Watcher poll with dependency filtering
// ===========================================================================

#[test]
fn test_watcher_poll_returns_eligible_with_deps_satisfied() {
    // Arrange: 5 stories — 1-1 done, 1-2 ready (dep met), 1-3 ready (dep NOT met),
    //          2-1 ready (no deps, first in epic), 2-2 backlog
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    setup_sprint_status(tmp.path(), entries);

    // Act
    let watcher = make_watcher(tmp.path());
    let result = watcher.poll();

    // Assert: eligible = [1-2-cli-framework, 2-1-polling]
    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 2, "Expected exactly 2 eligible stories, got: {keys:?}");
    assert!(keys.contains(&"1-2-cli-framework"), "1-2 should be eligible (dep 1-1 is done)");
    assert!(keys.contains(&"2-1-polling"), "2-1 should be eligible (first in epic, no deps)");
    assert!(
        !keys.contains(&"1-3-init-command"),
        "1-3 should NOT be eligible (dep 1-2 not done)"
    );
    assert!(
        !keys.contains(&"2-2-deps-resolution"),
        "2-2 should NOT be eligible (backlog status)"
    );
}

#[test]
fn test_watcher_poll_dependency_valid_ordering() {
    // Arrange: 3 epics — verifies topological ordering with multiple independent stories.
    // Epic-1: 1-1 done → 1-2 ready (dep met) → 1-3 ready (dep NOT met, 1-2 not done)
    // Epic-2: 2-1 ready (no deps), 2-2 backlog
    // Epic-3: 3-1 done → 3-2 ready (dep met)
    // Expected eligible: [1-2, 2-1, 3-2] in topological order.
    // 1-2 MUST appear before any story that depends on it (3-2 has no relation, but
    // we explicitly verify document order is respected for independent stories).
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
        ("epic-3", "in-progress"),
        ("3-1-scaffolding", "done"),
        ("3-2-parser", "ready-for-dev"),
    ];
    setup_sprint_status(tmp.path(), entries);

    // Act
    let watcher = make_watcher(tmp.path());
    let eligible = watcher.poll().expect("poll should succeed");

    // Assert correct set of eligible stories
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 3, "Expected 3 eligible stories, got: {keys:?}");
    assert!(keys.contains(&"1-2-cli-framework"), "1-2 must be eligible");
    assert!(keys.contains(&"2-1-polling"), "2-1 must be eligible");
    assert!(keys.contains(&"3-2-parser"), "3-2 must be eligible");
    assert!(!keys.contains(&"1-3-init-command"), "1-3 must NOT be eligible (dep 1-2 not done)");

    // Assert topological ordering: document order is used as tiebreaker for independent
    // stories, so 1-2 (epic-1) < 2-1 (epic-2) < 3-2 (epic-3).
    let pos_1_2 = keys.iter().position(|k| *k == "1-2-cli-framework").unwrap();
    let pos_2_1 = keys.iter().position(|k| *k == "2-1-polling").unwrap();
    let pos_3_2 = keys.iter().position(|k| *k == "3-2-parser").unwrap();
    assert!(pos_1_2 < pos_2_1, "1-2 (epic-1) must precede 2-1 (epic-2) in document order");
    assert!(pos_2_1 < pos_3_2, "2-1 (epic-2) must precede 3-2 (epic-3) in document order");
}

// ===========================================================================
// Task 3 — AC #2: Cascade blocking tests
// ===========================================================================

#[test]
fn test_watcher_cascade_blocks_transitive_dependents() {
    // Arrange: 1-1 blocked → 1-2 and 1-3 should be cascade-blocked
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    setup_sprint_status(tmp.path(), entries);

    // Act
    let watcher = make_watcher(tmp.path());
    let result = watcher.poll();

    // Assert: only 2-1 is eligible (1-2 and 1-3 cascade-blocked by 1-1)
    let eligible = result.expect("poll should succeed");
    assert_eq!(eligible.len(), 1, "Only 2-1 should be eligible");
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_cascade_independent_epic_unaffected() {
    // Arrange: 1-1 blocked, but epic-2 stories should be unaffected
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    setup_sprint_status(tmp.path(), entries);

    let watcher = make_watcher(tmp.path());
    let eligible = watcher.poll().expect("poll should succeed");

    // Assert: 2-1 IS returned (independent epic unaffected by epic-1 blocking)
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert!(keys.contains(&"2-1-polling"), "2-1 should be eligible (independent epic)");
}

#[test]
fn test_watcher_cascade_needs_clarification_triggers_cascade() {
    // Arrange: needs-clarification should trigger cascade blocking same as blocked
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "needs-clarification"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    setup_sprint_status(tmp.path(), entries);

    let watcher = make_watcher(tmp.path());
    let eligible = watcher.poll().expect("poll should succeed");

    // Assert: only 2-1 eligible; 1-2 and 1-3 cascade-blocked by 1-1 (needs-clarification)
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_in_progress_does_not_trigger_cascade() {
    // Arrange: 1-1 in-progress — NOT a BLOCKING_STATUS, should NOT cascade
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "in-progress"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    setup_sprint_status(tmp.path(), entries);

    let watcher = make_watcher(tmp.path());
    let result = watcher.poll();

    // 1-2 is NOT cascade-blocked (in-progress is transient), but dep 1-1 is NOT done
    // so 1-2 is skipped (dep not met). Only 2-1 is eligible.
    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert!(keys.contains(&"2-1-polling"));
    // 1-2 is NOT cascade-blocked — just skipped because dep not done
    assert!(!keys.contains(&"1-2-cli-framework"));

    // Directly verify find_cascade_blocks() returns EMPTY for in-progress — proves
    // the exclusion is dep-not-met, NOT cascade blocking.
    let all_statuses: Vec<(String, String)> = entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let all_statuses_map: HashMap<String, String> = all_statuses.iter().cloned().collect();
    let full_dep_map = build_full_dependency_map(&all_statuses);
    let story_1_2 = make_test_story("1-2-cli-framework", "cli-framework", vec!["1-1-scaffolding".to_string()]);
    let cascade_blocks = find_cascade_blocks(&[story_1_2], &all_statuses_map, &full_dep_map);
    assert!(
        cascade_blocks.is_empty(),
        "in-progress must NOT trigger cascade blocking, got: {cascade_blocks:?}"
    );
}

#[test]
fn test_watcher_review_status_does_not_trigger_cascade() {
    // Arrange: 1-1 review — NOT a BLOCKING_STATUS
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "review"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    setup_sprint_status(tmp.path(), entries);

    let watcher = make_watcher(tmp.path());
    let eligible = watcher.poll().expect("poll should succeed");

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    // 1-2 skipped (dep 1-1 not done), but NOT cascade-blocked
    assert!(keys.contains(&"2-1-polling"));
    assert!(!keys.contains(&"1-2-cli-framework"));

    // Directly verify find_cascade_blocks() returns EMPTY for review — proves
    // the exclusion is dep-not-met, NOT cascade blocking.
    let all_statuses: Vec<(String, String)> = entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let all_statuses_map: HashMap<String, String> = all_statuses.iter().cloned().collect();
    let full_dep_map = build_full_dependency_map(&all_statuses);
    let story_1_2 = make_test_story("1-2-cli-framework", "cli-framework", vec!["1-1-scaffolding".to_string()]);
    let cascade_blocks = find_cascade_blocks(&[story_1_2], &all_statuses_map, &full_dep_map);
    assert!(
        cascade_blocks.is_empty(),
        "review must NOT trigger cascade blocking, got: {cascade_blocks:?}"
    );
}

// ===========================================================================
// Task 4 — AC #3: All-done scenario
// ===========================================================================

#[test]
fn test_watcher_poll_all_done_returns_no_eligible() {
    // Arrange: all stories done
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "done"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "done"),
        ("epic-2", "done"),
        ("2-1-polling", "done"),
    ];
    setup_sprint_status(tmp.path(), entries);

    let watcher = make_watcher(tmp.path());
    let result = watcher.poll();

    // Assert: NoEligibleStories error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::NoEligibleStories),
        "Expected NoEligibleStories, got: {err}"
    );
}

// ===========================================================================
// Task 5 — AC #4: Cyclic dependency detection
// ===========================================================================

#[test]
fn test_watcher_cyclic_dependency_detected() {
    // Arrange: manually create stories with circular deps.
    // Cannot occur naturally via derive_dependencies, so we create StoryInfo
    // manually and call DependencyGraph directly.
    // make_test_story already sets status = "ready-for-dev" by default.
    let story_a = make_test_story("1-1-foo", "foo", vec!["1-2-bar".to_string()]);
    let story_b = make_test_story("1-2-bar", "bar", vec!["1-1-foo".to_string()]);

    let stories = vec![story_a, story_b];
    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-foo".to_string(), "ready-for-dev".to_string()),
        ("1-2-bar".to_string(), "ready-for-dev".to_string()),
    ];

    // Act: topological_sort detects cycle
    let graph = DependencyGraph::new(&stories, &all_statuses);
    let result = graph.topological_sort();

    // Assert: CyclicDependency error with both keys
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::CyclicDependency { ref cycle } if cycle.contains(&"1-1-foo".to_string()) && cycle.contains(&"1-2-bar".to_string())),
        "Expected CyclicDependency containing both story keys, got: {err}"
    );
}

#[test]
fn test_watcher_cyclic_dependency_error_contains_keys() {
    // More granular assertion: extract the cycle keys
    let story_a = make_test_story("1-1-alpha", "alpha", vec!["1-2-beta".to_string()]);
    let story_b = make_test_story("1-2-beta", "beta", vec!["1-1-alpha".to_string()]);

    let stories = vec![story_a, story_b];
    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-alpha".to_string(), "ready-for-dev".to_string()),
        ("1-2-beta".to_string(), "ready-for-dev".to_string()),
    ];

    let graph = DependencyGraph::new(&stories, &all_statuses);
    let result = graph.topological_sort();

    match result {
        Err(WatcherError::CyclicDependency { cycle }) => {
            assert!(
                cycle.contains(&"1-1-alpha".to_string()),
                "Cycle should contain 1-1-alpha"
            );
            assert!(
                cycle.contains(&"1-2-beta".to_string()),
                "Cycle should contain 1-2-beta"
            );
        }
        other => panic!("Expected CyclicDependency, got: {other:?}"),
    }
}

// ===========================================================================
// Task 6 — AC #5: Missing file test
// ===========================================================================

#[test]
fn test_watcher_poll_missing_sprint_status_returns_error() {
    // Arrange: temp dir with NO sprint-status.yaml
    let tmp = tempfile::tempdir().unwrap();
    // Create the nested directory but don't write any file
    let impl_artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&impl_artifacts).expect("create dirs");

    let watcher = make_watcher(tmp.path());
    let result = watcher.poll();

    // Assert: SprintStatusNotFound
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusNotFound { ref path } if path.contains("sprint-status.yaml")),
        "Expected SprintStatusNotFound, got: {err}"
    );
}

#[test]
fn test_watcher_poll_missing_file_error_contains_path() {
    let tmp = tempfile::tempdir().unwrap();
    let impl_artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&impl_artifacts).expect("create dirs");

    let watcher = make_watcher(tmp.path());
    let err = watcher.poll().unwrap_err();

    // Verify the error message contains the expected path
    let err_msg = format!("{err}");
    // Verify both the filename AND the actual temp path are present in the error.
    assert!(
        err_msg.contains("sprint-status.yaml"),
        "Error message should contain 'sprint-status.yaml', got: {err_msg}"
    );
    let expected_path_fragment = impl_artifacts.to_string_lossy();
    assert!(
        err_msg.contains(expected_path_fragment.as_ref()),
        "Error message should contain the actual path '{expected_path_fragment}', got: {err_msg}"
    );
}

// ===========================================================================
// Task 7 — SprintStatusFile integration tests (supplementary)
// ===========================================================================

#[test]
fn test_sprint_status_load_valid_yaml_correct_story_count() {
    // Arrange: valid YAML with mixed entries
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    let impl_dir = setup_sprint_status(tmp.path(), entries);
    let yaml_path = impl_dir.join("sprint-status.yaml");

    // Act
    let sprint_status = SprintStatusFile::load(&yaml_path, &impl_dir).expect("load");

    // Assert: stories() filters out epics and retrospectives
    let stories = sprint_status.stories();
    assert_eq!(stories.len(), 4, "Should have 4 stories (excluding epics and retros)");

    // Verify order preservation (insertion order)
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-1-scaffolding", "1-2-cli-framework", "1-3-init-command", "2-1-polling"]);
}

#[test]
fn test_sprint_status_stories_filters_out_epics_and_retros() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-foo", "done"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "backlog"),
        ("2-1-bar", "ready-for-dev"),
        ("epic-2-retrospective", "optional"),
    ];
    let impl_dir = setup_sprint_status(tmp.path(), entries);
    let yaml_path = impl_dir.join("sprint-status.yaml");

    let sprint_status = SprintStatusFile::load(&yaml_path, &impl_dir).expect("load");
    let stories = sprint_status.stories();

    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-1-foo", "2-1-bar"]);
    // No epic or retrospective entries
    assert!(keys.iter().all(|k| !k.starts_with("epic-")));
    assert!(keys.iter().all(|k| !k.contains("retrospective")));
}

#[test]
fn test_sprint_status_eligible_stories_returns_only_ready_for_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = &[
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "in-progress"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps", "backlog"),
    ];
    let impl_dir = setup_sprint_status(tmp.path(), entries);
    let yaml_path = impl_dir.join("sprint-status.yaml");

    let sprint_status = SprintStatusFile::load(&yaml_path, &impl_dir).expect("load");
    let eligible = sprint_status.eligible_stories();

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-2-cli-framework", "2-1-polling"]);
}

#[test]
fn test_sprint_status_malformed_yaml_returns_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let impl_dir = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&impl_dir).expect("create dirs");
    let yaml_path = impl_dir.join("sprint-status.yaml");
    std::fs::write(&yaml_path, "{{{{invalid yaml: [[[").expect("write");

    let result = SprintStatusFile::load(&yaml_path, &impl_dir);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusParse(_)),
        "Expected SprintStatusParse, got: {err}"
    );
}
