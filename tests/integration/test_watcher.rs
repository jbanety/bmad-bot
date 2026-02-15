//! Integration tests for the Watcher → Dependency Resolution → Story Selection chain.
//!
//! Story 7.3 — Watcher → Dependency Resolution → Story Selection Integration Tests
//!
//! These tests exercise the real `Watcher`, `SprintStatusFile`, `DependencyGraph`,
//! and `deps` module functions together — no mocks. The only external dependency
//! is the filesystem, isolated via `tempfile::tempdir()`.

use std::path::Path;
use std::sync::Arc;

use bmad_bot::watcher::deps::DependencyGraph;
use bmad_bot::watcher::{SprintStatusFile, Watcher, WatcherError};

use crate::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create the `implementation-artifacts` subdirectory inside a temp dir
/// and return its path. `make_test_config(dir)` sets
/// `implementation_artifacts` to `{dir}/implementation-artifacts`, so the
/// Watcher will look for `sprint-status.yaml` inside this subdirectory.
fn setup_impl_dir(base: &Path) -> std::path::PathBuf {
    let impl_dir = base.join("implementation-artifacts");
    std::fs::create_dir_all(&impl_dir).expect("create implementation-artifacts dir");
    impl_dir
}

// ===========================================================================
// Task 2: Watcher poll with dependency filtering (AC #1)
// ===========================================================================

#[test]
fn test_watcher_poll_returns_eligible_with_deps_satisfied() {
    // Arrange: 5 stories across 2 epics
    // 1-1: done, 1-2: ready-for-dev (dep on 1-1 done → eligible),
    // 1-3: ready-for-dev (dep on 1-2 not done → skipped),
    // 2-1: ready-for-dev (first in epic, no dep → eligible),
    // 2-2: backlog (not ready → excluded)
    let tmp = tempfile::tempdir().unwrap();
    let impl_dir = setup_impl_dir(tmp.path());

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    write_sprint_status(&impl_dir, &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));

    // Act
    let result = watcher.poll();

    // Assert: eligible stories are [1-2-cli-framework, 2-1-polling]
    let stories = result.expect("poll should succeed");
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 2, "Expected 2 eligible stories, got: {keys:?}");
    assert!(
        keys.contains(&"1-2-cli-framework"),
        "1-2-cli-framework should be eligible"
    );
    assert!(
        keys.contains(&"2-1-polling"),
        "2-1-polling should be eligible"
    );
}

#[test]
fn test_watcher_poll_dependency_valid_ordering() {
    // Arrange: 1-1 done, 1-2 and 2-1 are both eligible and independent.
    // Topological sort with doc-order tiebreaker should yield 1-2 before 2-1.
    let tmp = tempfile::tempdir().unwrap();
    let impl_dir = setup_impl_dir(tmp.path());

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&impl_dir, &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));

    let stories = watcher.poll().expect("poll should succeed");
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();

    // 1-2 appears before 2-1 in document order, so topo sort should preserve that
    let pos_12 = keys.iter().position(|k| *k == "1-2-cli-framework");
    let pos_21 = keys.iter().position(|k| *k == "2-1-polling");
    assert!(
        pos_12.is_some() && pos_21.is_some(),
        "Both stories should be present"
    );
    assert!(
        pos_12.unwrap() < pos_21.unwrap(),
        "1-2 should come before 2-1 (dependency-valid order)"
    );
}

// ===========================================================================
// Task 3: Cascade blocking tests (AC #2)
// ===========================================================================

#[test]
fn test_watcher_cascade_blocks_transitive_dependents() {
    // Arrange: 1-1 blocked, 1-2 ready-for-dev (depends on 1-1),
    //          1-3 ready-for-dev (depends on 1-2 → transitively blocked by 1-1)
    //          2-1 ready-for-dev (independent epic, unaffected)
    let tmp = tempfile::tempdir().unwrap();
    let impl_dir = setup_impl_dir(tmp.path());

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&impl_dir, &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));

    // Act
    let result = watcher.poll();

    // Assert: only 2-1 is eligible (1-2 and 1-3 cascade-blocked by 1-1)
    let eligible = result.expect("poll should succeed");
    assert_eq!(eligible.len(), 1, "Only 2-1 should be eligible");
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_cascade_blocks_needs_clarification() {
    // needs-clarification is also a blocking status, same cascade behavior
    let tmp = tempfile::tempdir().unwrap();
    let impl_dir = setup_impl_dir(tmp.path());

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "needs-clarification"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&impl_dir, &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));

    let eligible = watcher.poll().expect("poll should succeed");
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_no_cascade_on_in_progress_status() {
    // in-progress is NOT a blocking status — story is skipped (dep not met)
    // but NOT cascade-blocked
    let tmp = tempfile::tempdir().unwrap();
    let impl_dir = setup_impl_dir(tmp.path());

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "in-progress"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&impl_dir, &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));

    let eligible = watcher.poll().expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    // 1-2 is skipped (dep 1-1 not done) but NOT cascade-blocked
    // 2-1 is eligible (no deps)
    assert!(
        keys.contains(&"2-1-polling"),
        "2-1-polling should be eligible"
    );
    // 1-2 should NOT be eligible (dep not satisfied), but it's NOT cascade-blocked
    assert!(
        !keys.contains(&"1-2-cli-framework"),
        "1-2 should be skipped (dep not done)"
    );
}

#[test]
fn test_watcher_no_cascade_on_review_status() {
    // review is NOT a blocking status — same as in-progress behavior
    let tmp = tempfile::tempdir().unwrap();
    let impl_dir = setup_impl_dir(tmp.path());

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "review"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&impl_dir, &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));

    let eligible = watcher.poll().expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    // 1-2 is skipped (dep not satisfied: review != done) but NOT cascade-blocked
    assert!(keys.contains(&"2-1-polling"));
    assert!(!keys.contains(&"1-2-cli-framework"));
}

// ===========================================================================
// Task 4: All-done scenario (AC #3)
// ===========================================================================

#[test]
fn test_watcher_poll_all_done_returns_no_eligible() {
    let tmp = tempfile::tempdir().unwrap();
    let impl_dir = setup_impl_dir(tmp.path());

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "done"),
        ("1-3-init-command", "done"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "done"),
    ];
    write_sprint_status(&impl_dir, &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));

    let result = watcher.poll();

    assert!(result.is_err(), "Should return error when all done");
    assert!(
        matches!(result.unwrap_err(), WatcherError::NoEligibleStories),
        "Expected NoEligibleStories error"
    );
}

// ===========================================================================
// Task 5: Cyclic dependency test (AC #4)
// ===========================================================================

#[test]
fn test_watcher_cyclic_dependency_detected() {
    // Create stories with manually injected circular deps.
    // derive_dependencies() only creates linear deps, so we bypass it.
    let mut story_a = make_test_story("1-1-foo", "Foo", vec!["1-2-bar".to_string()]);
    let mut story_b = make_test_story("1-2-bar", "Bar", vec!["1-1-foo".to_string()]);
    // Override status to ready-for-dev
    story_a.status = "ready-for-dev".to_string();
    story_b.status = "ready-for-dev".to_string();

    let all_statuses: Vec<(String, String)> = vec![
        ("epic-1".to_string(), "in-progress".to_string()),
        ("1-1-foo".to_string(), "ready-for-dev".to_string()),
        ("1-2-bar".to_string(), "ready-for-dev".to_string()),
    ];

    let graph = DependencyGraph::new(&[story_a, story_b], &all_statuses);
    let result = graph.topological_sort();

    assert!(result.is_err(), "Should detect cyclic dependency");
    match result.unwrap_err() {
        WatcherError::CyclicDependency { cycle } => {
            assert!(
                cycle.contains(&"1-1-foo".to_string()),
                "Cycle should contain 1-1-foo, got: {cycle:?}"
            );
            assert!(
                cycle.contains(&"1-2-bar".to_string()),
                "Cycle should contain 1-2-bar, got: {cycle:?}"
            );
        }
        other => panic!("Expected CyclicDependency, got: {other}"),
    }
}

// ===========================================================================
// Task 6: Missing file test (AC #5)
// ===========================================================================

#[test]
fn test_watcher_poll_missing_sprint_status_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    // Create the implementation-artifacts dir but do NOT write sprint-status.yaml
    let _impl_dir = setup_impl_dir(tmp.path());

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));

    let result = watcher.poll();

    assert!(result.is_err(), "Should return error for missing file");
    match result.unwrap_err() {
        WatcherError::SprintStatusNotFound { path } => {
            assert!(
                path.contains("sprint-status.yaml"),
                "Error path should contain 'sprint-status.yaml', got: {path}"
            );
        }
        other => panic!("Expected SprintStatusNotFound, got: {other}"),
    }
}

// ===========================================================================
// Task 7: SprintStatusFile integration tests (supplementary)
// ===========================================================================

#[test]
fn test_sprint_status_load_valid_yaml_correct_count_and_order() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(dir, &entries);

    let path = dir.join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, dir).expect("load should succeed");

    // entry_count includes all entries (epics, stories, retrospectives)
    assert_eq!(ssf.entry_count(), 6, "Should have 6 total entries");

    // Verify order is preserved
    let loaded_entries = ssf.entries();
    assert_eq!(loaded_entries[0].0, "epic-1");
    assert_eq!(loaded_entries[1].0, "1-1-scaffolding");
    assert_eq!(loaded_entries[2].0, "1-2-cli-framework");
    assert_eq!(loaded_entries[3].0, "epic-1-retrospective");
    assert_eq!(loaded_entries[4].0, "epic-2");
    assert_eq!(loaded_entries[5].0, "2-1-polling");
}

#[test]
fn test_sprint_status_stories_filters_out_epics_and_retros() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(dir, &entries);

    let path = dir.join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, dir).expect("load should succeed");

    let stories = ssf.stories();
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();

    assert_eq!(keys.len(), 3, "Should have 3 story entries (no epics/retros)");
    assert!(keys.contains(&"1-1-scaffolding"));
    assert!(keys.contains(&"1-2-cli-framework"));
    assert!(keys.contains(&"2-1-polling"));
    // Verify epics and retrospectives are excluded
    assert!(!keys.iter().any(|k| k.starts_with("epic-")));
}

#[test]
fn test_sprint_status_eligible_stories_returns_only_ready_for_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "in-progress"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps", "backlog"),
    ];
    write_sprint_status(dir, &entries);

    let path = dir.join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, dir).expect("load should succeed");

    let eligible = ssf.eligible_stories();
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    assert_eq!(keys.len(), 2, "Should have 2 eligible stories");
    assert!(keys.contains(&"1-2-cli-framework"));
    assert!(keys.contains(&"2-1-polling"));
}

#[test]
fn test_sprint_status_malformed_yaml_returns_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, "{{invalid yaml: [unterminated").expect("write malformed yaml");

    let result = SprintStatusFile::load(&path, dir);

    assert!(result.is_err(), "Should fail on malformed YAML");
    assert!(
        matches!(result.unwrap_err(), WatcherError::SprintStatusParse(_)),
        "Expected SprintStatusParse error"
    );
}
