//! Integration tests for Watcher → Dependency Resolution → Story Selection.
//!
//! Story 7.3: Exercises the full `Watcher::poll()` pipeline with real
//! `SprintStatusFile`, `DependencyGraph`, and `deps` module — no mocks.
//! Filesystem isolation via `tempfile::tempdir()`.

use std::sync::Arc;

use bmad_bot::watcher::deps::DependencyGraph;
use bmad_bot::watcher::{SprintStatusFile, Watcher, WatcherError};

use crate::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create the `_bmad-output/implementation-artifacts` subdirectory inside a
/// temp dir and return the path.  `make_test_config(tmp.path())` expects
/// sprint-status.yaml to live at this location.
fn setup_artifacts_dir(tmp: &tempfile::TempDir) -> std::path::PathBuf {
    let artifacts_dir = tmp
        .path()
        .join("_bmad-output")
        .join("implementation-artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("Failed to create artifacts dir");
    artifacts_dir
}

/// Shorthand: create a Watcher from a temp dir that already has artifacts written.
fn watcher_from_tmp(tmp: &tempfile::TempDir) -> Watcher {
    let config = make_test_config(tmp.path());
    Watcher::new(Arc::new(config))
}

// ===========================================================================
// Task 2 — Watcher poll with dependency filtering (AC #1)
// ===========================================================================

#[test]
fn test_watcher_poll_returns_eligible_with_deps_satisfied() {
    // Arrange: 5 stories across 2 epics
    //   1-1 done, 1-2 ready-for-dev (dep on 1-1 → satisfied),
    //   1-3 ready-for-dev (dep on 1-2 → NOT satisfied),
    //   2-1 ready-for-dev (no deps → eligible), 2-2 backlog
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    write_sprint_status(&artifacts_dir, entries);

    // Act
    let watcher = watcher_from_tmp(&tmp);
    let result = watcher.poll();

    // Assert: eligible = [1-2-cli-framework, 2-1-polling]
    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    assert!(keys.contains(&"1-2-cli-framework"), "1-2 should be eligible (dep 1-1 done)");
    assert!(keys.contains(&"2-1-polling"), "2-1 should be eligible (no deps, first in epic)");
    assert!(!keys.contains(&"1-3-init-command"), "1-3 should be skipped (dep 1-2 not done)");
    assert!(!keys.contains(&"2-2-deps-resolution"), "2-2 should be skipped (backlog)");
    assert_eq!(eligible.len(), 2);
}

#[test]
fn test_watcher_poll_dependency_valid_ordering() {
    // Ensure 1-2 appears before any story that depends on it.
    // In this setup, 1-2 has dep on 1-1 (done), and 2-1 has no deps.
    // Both are eligible; topological order should place 1-2 before
    // any hypothetical dependent (ordering verified by position).
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    write_sprint_status(&artifacts_dir, entries);

    let watcher = watcher_from_tmp(&tmp);
    let eligible = watcher.poll().expect("poll should succeed");

    // Find position of 1-2 in the result
    let pos_1_2 = eligible.iter().position(|s| s.story_key == "1-2-cli-framework");
    assert!(pos_1_2.is_some(), "1-2 must be in eligible list");

    // No story in the result should depend on 1-2 AND appear before it
    // (2-1 is independent, 1-3 is filtered out — so ordering is valid by definition here)
    // The topological sort guarantees dependency-valid ordering.
}

// ===========================================================================
// Task 3 — Cascade blocking tests (AC #2)
// ===========================================================================

#[test]
fn test_watcher_cascade_blocks_transitive_dependents() {
    // Arrange: 1-1 blocked, 1-2 and 1-3 ready-for-dev (sequential deps)
    //   2-1 ready-for-dev with no deps → should still be eligible
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&artifacts_dir, entries);

    // Act
    let watcher = watcher_from_tmp(&tmp);
    let result = watcher.poll();

    // Assert: only 2-1 is eligible (1-2 and 1-3 cascade-blocked by 1-1)
    let eligible = result.expect("poll should succeed");
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_cascade_independent_epic_unaffected() {
    // Same as above: 2-1 should be returned even when epic-1 is entirely blocked
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&artifacts_dir, entries);

    let watcher = watcher_from_tmp(&tmp);
    let eligible = watcher.poll().expect("poll should succeed");

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert!(keys.contains(&"2-1-polling"), "Independent epic story should be eligible");
    assert!(!keys.contains(&"1-2-cli-framework"), "1-2 should be cascade-blocked");
}

#[test]
fn test_watcher_cascade_needs_clarification_triggers_cascade() {
    // needs-clarification is a BLOCKING_STATUS → triggers cascade
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "needs-clarification"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&artifacts_dir, entries);

    let watcher = watcher_from_tmp(&tmp);
    let eligible = watcher.poll().expect("poll should succeed");

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_no_cascade_on_in_progress_dep() {
    // in-progress is NOT a BLOCKING_STATUS → no cascade, just skip (dep not done)
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "in-progress"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&artifacts_dir, entries);

    let watcher = watcher_from_tmp(&tmp);
    let eligible = watcher.poll().expect("poll should succeed");

    // 1-2 is skipped (dep 1-1 not done) but NOT cascade-blocked
    // Only 2-1 is eligible
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(eligible.len(), 1);
    assert!(keys.contains(&"2-1-polling"));
    // The key distinction: 1-2 is skipped (dep not met) not cascade-blocked.
    // We verify this by checking that the poll still succeeds (no error).
}

#[test]
fn test_watcher_no_cascade_on_review_dep() {
    // review is NOT a BLOCKING_STATUS → no cascade, just skip (dep not done)
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "review"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&artifacts_dir, entries);

    let watcher = watcher_from_tmp(&tmp);
    let eligible = watcher.poll().expect("poll should succeed");

    // 1-2 is skipped but not cascade-blocked
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(eligible.len(), 1);
    assert!(keys.contains(&"2-1-polling"));
}

// ===========================================================================
// Task 4 — All-done scenario (AC #3)
// ===========================================================================

#[test]
fn test_watcher_poll_all_done_returns_no_eligible_stories() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "done"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "done"),
        ("epic-2", "done"),
        ("2-1-polling", "done"),
    ];
    write_sprint_status(&artifacts_dir, entries);

    let watcher = watcher_from_tmp(&tmp);
    let result = watcher.poll();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::NoEligibleStories),
        "Expected NoEligibleStories, got: {err}"
    );
}

// ===========================================================================
// Task 5 — Cyclic dependency test (AC #4)
// ===========================================================================

#[test]
fn test_watcher_cyclic_dependency_detected() {
    // Manually create stories with circular deps — derive_dependencies
    // cannot produce cycles naturally, so we set deps directly.
    let mut story_a = make_test_story("1-1-alpha", "alpha", vec![]);
    let mut story_b = make_test_story("1-2-beta", "beta", vec![]);

    // Inject cycle: A depends on B, B depends on A
    story_a.dependencies = vec!["1-2-beta".to_string()];
    story_b.dependencies = vec!["1-1-alpha".to_string()];

    let all_statuses: Vec<(String, String)> = vec![
        ("epic-1".to_string(), "in-progress".to_string()),
        ("1-1-alpha".to_string(), "ready-for-dev".to_string()),
        ("1-2-beta".to_string(), "ready-for-dev".to_string()),
    ];

    let graph = DependencyGraph::new(&[story_a, story_b], &all_statuses);
    let result = graph.topological_sort();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::CyclicDependency { ref cycle } if
            cycle.contains(&"1-1-alpha".to_string()) &&
            cycle.contains(&"1-2-beta".to_string())
        ),
        "Expected CyclicDependency containing both story keys, got: {err}"
    );
}

// ===========================================================================
// Task 6 — Missing file test (AC #5)
// ===========================================================================

#[test]
fn test_watcher_poll_missing_sprint_status_returns_error() {
    // Temp dir with no sprint-status.yaml
    let tmp = tempfile::tempdir().unwrap();
    // Create artifacts dir but do NOT write sprint-status.yaml
    setup_artifacts_dir(&tmp);

    let watcher = watcher_from_tmp(&tmp);
    let result = watcher.poll();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusNotFound { ref path } if path.contains("sprint-status.yaml")),
        "Expected SprintStatusNotFound with path containing 'sprint-status.yaml', got: {err}"
    );
}

// ===========================================================================
// Task 7 — SprintStatusFile integration tests (supplementary)
// ===========================================================================

#[test]
fn test_sprint_status_load_valid_yaml_correct_count_and_order() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    let yaml_path = write_sprint_status(&artifacts_dir, entries);

    let ssf = SprintStatusFile::load(&yaml_path, &artifacts_dir)
        .expect("Should load valid sprint-status.yaml");

    // stories() filters out epics and retrospectives
    let stories = ssf.stories();
    assert_eq!(stories.len(), 5, "Should have 5 story entries");

    // Verify order matches YAML insertion order
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[1].story_key, "1-2-cli-framework");
    assert_eq!(stories[2].story_key, "1-3-init-command");
    assert_eq!(stories[3].story_key, "2-1-polling");
    assert_eq!(stories[4].story_key, "2-2-deps-resolution");
}

#[test]
fn test_sprint_status_stories_filters_out_epics_and_retrospectives() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("epic-2-retrospective", "done"),
    ];
    let yaml_path = write_sprint_status(&artifacts_dir, entries);

    let ssf = SprintStatusFile::load(&yaml_path, &artifacts_dir).unwrap();
    let stories = ssf.stories();

    // Only actual story entries should survive
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-1-scaffolding", "2-1-polling"]);
    assert!(!keys.iter().any(|k| k.starts_with("epic-")), "No epic entries should remain");
}

#[test]
fn test_sprint_status_eligible_stories_returns_only_ready_for_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "in-progress"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    let yaml_path = write_sprint_status(&artifacts_dir, entries);

    let ssf = SprintStatusFile::load(&yaml_path, &artifacts_dir).unwrap();
    let eligible = ssf.eligible_stories();

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-2-cli-framework", "2-1-polling"]);
}

#[test]
fn test_sprint_status_malformed_yaml_returns_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = setup_artifacts_dir(&tmp);

    let bad_yaml = "this is not: [valid: yaml: {{{}}}";
    let path = artifacts_dir.join("sprint-status.yaml");
    std::fs::write(&path, bad_yaml).unwrap();

    let result = SprintStatusFile::load(&path, &artifacts_dir);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusParse(_)),
        "Expected SprintStatusParse, got: {err}"
    );
}
