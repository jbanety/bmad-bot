//! Integration tests: Watcher → Dependency Resolution → Story Selection.
//!
//! Story 7.3 — exercises the full pipeline: `Watcher::poll()` parses
//! sprint-status.yaml, filters eligible stories, resolves dependencies,
//! detects cascade blocks, and returns stories in topological order.

use std::sync::Arc;

use bmad_bot::watcher::deps::{filter_eligible, DependencyGraph, derive_dependencies};
use bmad_bot::watcher::{SprintStatusFile, Watcher, WatcherError};

use crate::helpers::fixtures::{impl_artifacts_dir, make_test_config, make_test_story, write_sprint_status};

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

    // Assert: exact set and deterministic document-order
    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["1-2-cli-framework", "2-1-polling"],
        "expected exactly [1-2, 2-1] in document order"
    );
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
    // in-progress is NOT a BLOCKING_STATUS → cascade_count MUST be 0.
    // Use filter_eligible() directly so we can assert the cascade count,
    // distinguishing "dep-unmet (correct)" from "incorrectly cascade-blocked".
    let all_statuses: Vec<(String, String)> = vec![
        ("epic-1".to_string(), "in-progress".to_string()),
        ("1-1-scaffolding".to_string(), "in-progress".to_string()),
        ("1-2-cli-framework".to_string(), "ready-for-dev".to_string()),
        ("epic-2".to_string(), "in-progress".to_string()),
        ("2-1-polling".to_string(), "ready-for-dev".to_string()),
    ];

    let mut eligible_input = vec![
        make_test_story("1-2-cli-framework", "cli-framework", vec![]),
        make_test_story("2-1-polling", "polling", vec![]),
    ];
    derive_dependencies(&mut eligible_input, &all_statuses);

    let (result, cascade_count) =
        filter_eligible(eligible_input, &all_statuses).expect("filter_eligible should succeed");

    // 1-2 is skipped because dep (1-1) is not done — NOT cascade-blocked
    // cascade_count MUST be 0, proving the in-progress codepath is NOT cascade
    assert_eq!(cascade_count, 0, "in-progress must not trigger cascade blocking");
    let keys: Vec<&str> = result.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["2-1-polling"], "only dep-free story should be eligible");
}

#[test]
fn test_watcher_no_cascade_on_review() {
    // review is NOT a BLOCKING_STATUS → cascade_count MUST be 0.
    // Use filter_eligible() directly to prove the mechanism, not just the output.
    let all_statuses: Vec<(String, String)> = vec![
        ("epic-1".to_string(), "in-progress".to_string()),
        ("1-1-scaffolding".to_string(), "review".to_string()),
        ("1-2-cli-framework".to_string(), "ready-for-dev".to_string()),
        ("epic-2".to_string(), "in-progress".to_string()),
        ("2-1-polling".to_string(), "ready-for-dev".to_string()),
    ];

    let mut eligible_input = vec![
        make_test_story("1-2-cli-framework", "cli-framework", vec![]),
        make_test_story("2-1-polling", "polling", vec![]),
    ];
    derive_dependencies(&mut eligible_input, &all_statuses);

    let (result, cascade_count) =
        filter_eligible(eligible_input, &all_statuses).expect("filter_eligible should succeed");

    // cascade_count MUST be 0 — review is transient, not a blocker
    assert_eq!(cascade_count, 0, "review must not trigger cascade blocking");
    let keys: Vec<&str> = result.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["2-1-polling"]);
}

#[test]
fn test_filter_eligible_cascade_count_positive() {
    // Positive case: blocked status → cascade_count reflects actual cascade blocks.
    // This is the counterpart to the two negative tests above.
    let all_statuses: Vec<(String, String)> = vec![
        ("epic-1".to_string(), "in-progress".to_string()),
        ("1-1-scaffolding".to_string(), "blocked".to_string()),
        ("1-2-cli-framework".to_string(), "ready-for-dev".to_string()),
        ("1-3-init-command".to_string(), "ready-for-dev".to_string()),
        ("epic-2".to_string(), "in-progress".to_string()),
        ("2-1-polling".to_string(), "ready-for-dev".to_string()),
    ];

    let mut eligible_input = vec![
        make_test_story("1-2-cli-framework", "cli-framework", vec![]),
        make_test_story("1-3-init-command", "init-command", vec![]),
        make_test_story("2-1-polling", "polling", vec![]),
    ];
    derive_dependencies(&mut eligible_input, &all_statuses);

    let (result, cascade_count) =
        filter_eligible(eligible_input, &all_statuses).expect("filter_eligible should succeed");

    // 1-2 directly cascade-blocked (dep 1-1 is blocked)
    // 1-3 transitively cascade-blocked (dep chain 1-3 → 1-2 → 1-1 blocked)
    assert_eq!(cascade_count, 2, "both 1-2 and 1-3 should be cascade-blocked");
    let keys: Vec<&str> = result.iter().map(|s| s.story_key.as_str()).collect();
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
    // make_test_story defaults status to "ready-for-dev" — no override needed
    let story_a = make_test_story("1-1-foo", "Foo", vec!["1-2-bar".to_string()]);
    let story_b = make_test_story("1-2-bar", "Bar", vec!["1-1-foo".to_string()]);

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
