//! Integration tests: Watcher → Dependency Resolution → Story Selection.
//!
//! Story 7.3 — exercises the full pipeline: `Watcher::poll()` parses
//! sprint-status.yaml, filters eligible stories, resolves dependencies,
//! detects cascade blocks, and returns stories in topological order.

use std::path::Path;
use std::sync::Arc;

use bmad_bot::watcher::deps::{filter_eligible, DependencyGraph};
use bmad_bot::watcher::{SprintStatusFile, Watcher, WatcherError};

use crate::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create the `_bmad-output/implementation-artifacts` subdirectory under `root`
/// and return its path.  `make_test_config(root)` sets
/// `bmad_paths.implementation_artifacts` to this location.
fn impl_artifacts_dir(root: &Path) -> std::path::PathBuf {
    let dir = root.join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&dir).expect("create impl artifacts dir");
    dir
}

// ===========================================================================
// Task 2 — Watcher poll with dependency filtering (AC #1)
// ===========================================================================

#[test]
fn test_watcher_poll_returns_eligible_with_deps_satisfied() {
    // Arrange: 5 stories across 2 epics
    //   1-1 done, 1-2 ready-for-dev (dep 1-1 done → eligible),
    //   1-3 ready-for-dev (dep 1-2 NOT done → skipped),
    //   2-1 ready-for-dev (first in epic → no deps → eligible),
    //   2-2 backlog (not eligible)
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "done"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("1-3-init-command", "ready-for-dev"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
            ("2-2-deps-resolution", "backlog"),
        ],
    );

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));

    // Act
    let result = watcher.poll();

    // Assert
    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 2, "expected exactly 2 eligible stories, got {keys:?}");
    assert!(keys.contains(&"1-2-cli-framework"), "1-2 should be eligible");
    assert!(keys.contains(&"2-1-polling"), "2-1 should be eligible");
    assert!(!keys.contains(&"1-3-init-command"), "1-3 should be skipped (dep 1-2 not done)");
    assert!(!keys.contains(&"2-2-deps-resolution"), "2-2 should be skipped (backlog)");
}

#[test]
fn test_watcher_poll_dependency_valid_ordering() {
    // 1-2 depends on 1-1 (done) — eligible; 2-1 no deps — eligible
    // topological order: 1-2 must come before any hypothetical dependent
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "done"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("1-3-init-command", "ready-for-dev"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
        ],
    );

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // Verify ordering: eligible stories must be in topological (dependency-valid) order.
    // Both 1-2 and 2-1 are independent of each other → either order is valid,
    // but document order should be the tiebreaker: 1-2 before 2-1.
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-2-cli-framework", "2-1-polling"]);
}

// ===========================================================================
// Task 3 — Cascade blocking tests (AC #2)
// ===========================================================================

#[test]
fn test_watcher_cascade_blocks_transitive_dependents() {
    // 1-1 blocked → 1-2 cascade-blocked → 1-3 cascade-blocked
    // 2-1 independent → eligible
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "blocked"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("1-3-init-command", "ready-for-dev"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
        ],
    );

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_cascade_blocks_needs_clarification() {
    // needs-clarification is a BLOCKING_STATUS, same cascade as blocked
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "needs-clarification"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("1-3-init-command", "ready-for-dev"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
        ],
    );

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_no_cascade_on_in_progress() {
    // in-progress is NOT a BLOCKING_STATUS → 1-2 should NOT be cascade-blocked,
    // but it IS skipped because dep 1-1 is not done yet.
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "in-progress"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
        ],
    );

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // 1-2 is skipped (dep not done), but NOT cascade-blocked
    // 2-1 is eligible (no deps)
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["2-1-polling"]);
    // Key distinction: this is NOT cascade blocking — 1-2 is simply dep-unmet.
    // With `blocked` status, 1-2 would be cascade-blocked (different codepath logged at warn).
}

#[test]
fn test_watcher_no_cascade_on_review() {
    // review is NOT a BLOCKING_STATUS
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "review"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
        ],
    );

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // 1-2 skipped (dep not done), 2-1 eligible
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["2-1-polling"]);
}

// ===========================================================================
// Task 4 — All-done scenario (AC #3)
// ===========================================================================

#[test]
fn test_watcher_poll_all_done_returns_no_eligible() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "done"),
            ("1-1-scaffolding", "done"),
            ("1-2-cli-framework", "done"),
            ("epic-2", "done"),
            ("2-1-polling", "done"),
        ],
    );

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::NoEligibleStories),
        "Expected NoEligibleStories, got: {err:?}"
    );
}

// ===========================================================================
// Task 5 — Cyclic dependency (AC #4)
// ===========================================================================

#[test]
fn test_watcher_cyclic_dependency_detected() {
    // Create stories with manually injected circular deps.
    // derive_dependencies() can't produce cycles, so we build StoryInfo manually.
    let mut story_a = make_test_story("1-1-foo", "Foo", vec!["1-2-bar".to_string()]);
    let mut story_b = make_test_story("1-2-bar", "Bar", vec!["1-1-foo".to_string()]);
    // Override status to ready-for-dev (make_test_story default)
    story_a.status = "ready-for-dev".to_string();
    story_b.status = "ready-for-dev".to_string();

    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-foo".to_string(), "ready-for-dev".to_string()),
        ("1-2-bar".to_string(), "ready-for-dev".to_string()),
    ];

    let graph = DependencyGraph::new(&[story_a, story_b], &all_statuses);
    let result = graph.topological_sort();

    assert!(result.is_err(), "Expected CyclicDependency error");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::CyclicDependency { ref cycle } if cycle.contains(&"1-1-foo".to_string())),
        "Expected cycle containing 1-1-foo, got: {err:?}"
    );
    assert!(
        matches!(err, WatcherError::CyclicDependency { ref cycle } if cycle.contains(&"1-2-bar".to_string())),
        "Expected cycle containing 1-2-bar, got: {err:?}"
    );
}

#[test]
fn test_watcher_cyclic_dependency_via_filter_eligible() {
    // Ensure filter_eligible also propagates the CyclicDependency error
    let story_a = make_test_story("1-1-foo", "Foo", vec!["1-2-bar".to_string()]);
    let story_b = make_test_story("1-2-bar", "Bar", vec!["1-1-foo".to_string()]);

    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-foo".to_string(), "ready-for-dev".to_string()),
        ("1-2-bar".to_string(), "ready-for-dev".to_string()),
    ];

    let result = filter_eligible(vec![story_a, story_b], &all_statuses);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::CyclicDependency { .. }),
        "Expected CyclicDependency, got: {err:?}"
    );
}

// ===========================================================================
// Task 6 — Missing file (AC #5)
// ===========================================================================

#[test]
fn test_watcher_poll_missing_file_returns_error() {
    // No sprint-status.yaml written — temp dir is empty
    let tmp = tempfile::tempdir().unwrap();
    let _artifacts = impl_artifacts_dir(tmp.path());

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusNotFound { ref path } if path.contains("sprint-status.yaml")),
        "Expected SprintStatusNotFound with path containing 'sprint-status.yaml', got: {err:?}"
    );
}

// ===========================================================================
// Task 7 — SprintStatusFile integration tests (supplementary)
// ===========================================================================

#[test]
fn test_sprint_status_load_valid_yaml_correct_story_count() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "done"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("1-3-init-command", "ready-for-dev"),
            ("epic-1-retrospective", "optional"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
            ("2-2-deps-resolution", "backlog"),
        ],
    );

    let path = artifacts.join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, &artifacts).expect("load should succeed");

    // stories() skips epics and retrospectives
    let stories = ssf.stories();
    assert_eq!(stories.len(), 5, "Expected 5 stories, got: {}", stories.len());

    // Verify order is preserved from YAML
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "1-1-scaffolding",
            "1-2-cli-framework",
            "1-3-init-command",
            "2-1-polling",
            "2-2-deps-resolution",
        ]
    );
}

#[test]
fn test_sprint_status_stories_filters_out_epics_and_retrospectives() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "done"),
            ("epic-1-retrospective", "optional"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
            ("epic-2-retrospective", "optional"),
        ],
    );

    let path = artifacts.join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, &artifacts).expect("load should succeed");
    let stories = ssf.stories();

    // Only actual stories, no epics or retrospectives
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-1-scaffolding", "2-1-polling"]);
}

#[test]
fn test_sprint_status_eligible_stories_returns_only_ready_for_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    write_sprint_status(
        &artifacts,
        vec![
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "done"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("1-3-init-command", "in-progress"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
            ("2-2-deps-resolution", "backlog"),
        ],
    );

    let path = artifacts.join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, &artifacts).expect("load should succeed");
    let eligible = ssf.eligible_stories();

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-2-cli-framework", "2-1-polling"]);
}

#[test]
fn test_sprint_status_malformed_yaml_returns_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = impl_artifacts_dir(tmp.path());

    let path = artifacts.join("sprint-status.yaml");
    std::fs::write(&path, "{{invalid yaml: [}").expect("write malformed yaml");

    let result = SprintStatusFile::load(&path, &artifacts);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusParse(_)),
        "Expected SprintStatusParse, got: {err:?}"
    );
}
