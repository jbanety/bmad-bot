//! Integration tests: Watcher → Dependency Resolution → Story Selection.
//!
//! Exercises the full `Watcher::poll()` pipeline with real `SprintStatusFile`,
//! `DependencyGraph`, and `deps` module functions — no mocks.
//! Filesystem isolation via `tempfile::tempdir()`.

use std::collections::HashMap;
use std::sync::Arc;

use bmad_bot::watcher::deps::{
    build_full_dependency_map, derive_dependencies, find_cascade_blocks, DependencyGraph,
};
use bmad_bot::watcher::{SprintStatusFile, Watcher, WatcherError};

use super::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};

// ===========================================================================
// Task 2 — Watcher poll with dependency filtering (AC #1)
// ===========================================================================

#[test]
fn test_watcher_poll_returns_eligible_with_deps_satisfied() {
    // Arrange: 5 stories across 2 epics
    // 1-1 done, 1-2 ready-for-dev (dep on 1-1 → satisfied), 1-3 ready-for-dev (dep on 1-2 → NOT satisfied)
    // 2-1 ready-for-dev (no deps → eligible), 2-2 backlog (not eligible)
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    write_sprint_status(tmp.path(), entries);

    // Act
    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    // Assert: eligible = [1-2, 2-1] (1-3 dep not met, 2-2 not ready, 1-1 done)
    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&"1-2-cli-framework"), "1-2 should be eligible");
    assert!(keys.contains(&"2-1-polling"), "2-1 should be eligible");
    assert!(
        !keys.contains(&"1-3-init-command"),
        "1-3 should NOT be eligible"
    );
}

#[test]
fn test_watcher_poll_dependency_valid_ordering() {
    // 1-2 depends on 1-1 (done). 2-1 has no deps. Both eligible.
    // Verify topological order: 1-2 appears before any story that depends on it.
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    // 1-3 is filtered out (its dep 1-2 is not done).
    // Remaining eligible: 1-2 and 2-1 — independent across epics.
    // Sprint-order tiebreaker (insertion order in YAML) means 1-2 before 2-1.
    assert_eq!(keys, vec!["1-2-cli-framework", "2-1-polling"]);
}

// ===========================================================================
// Task 3 — Cascade blocking tests (AC #2)
// ===========================================================================

#[test]
fn test_watcher_cascade_blocks_transitive_dependents() {
    // 1-1 blocked → 1-2 (depends on 1-1) cascade-blocked → 1-3 (depends on 1-2) cascade-blocked
    // 2-1 ready-for-dev, no deps → eligible
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // Only 2-1 should be eligible (1-2 and 1-3 cascade-blocked by 1-1)
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_cascade_blocks_needs_clarification() {
    // needs-clarification is also a BLOCKING_STATUS — same cascade behavior as blocked
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "needs-clarification"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_in_progress_does_not_cascade_block() {
    // in-progress is a transient status — NOT a cascade trigger.
    // 1-1 in-progress, 1-2 ready-for-dev → 1-2 is skipped (dep not done) but NOT cascade-blocked.
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "in-progress"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // 1-2 is skipped (dep not satisfied) but NOT cascade-blocked.
    // 2-1 has no deps → eligible.
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["2-1-polling"]);

    // CRITICAL: Directly verify find_cascade_blocks() returns empty.
    // This distinguishes "dep not satisfied" (skipped) from "cascade-blocked".
    // poll() returns the same output for both; only direct inspection proves the difference.
    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
    let mut eligible_for_cascade = ssf.eligible_stories();
    let all_entries = ssf.entries();
    let all_statuses_map: HashMap<String, String> = all_entries.iter().cloned().collect();
    derive_dependencies(&mut eligible_for_cascade, all_entries);
    let full_dep_map = build_full_dependency_map(all_entries);
    let cascade_blocks = find_cascade_blocks(&eligible_for_cascade, &all_statuses_map, &full_dep_map);
    assert!(
        cascade_blocks.is_empty(),
        "in-progress must NOT trigger cascade blocking, found {} cascade block(s)",
        cascade_blocks.len()
    );
}

#[test]
fn test_watcher_review_does_not_cascade_block() {
    // review is a transient status — NOT a cascade trigger.
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "review"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // 1-2 skipped (dep not done, review != done), NOT cascade-blocked.
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["2-1-polling"]);

    // CRITICAL: Directly verify find_cascade_blocks() returns empty.
    // Proves "review" is a transient status — not a cascade trigger.
    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
    let mut eligible_for_cascade = ssf.eligible_stories();
    let all_entries = ssf.entries();
    let all_statuses_map: HashMap<String, String> = all_entries.iter().cloned().collect();
    derive_dependencies(&mut eligible_for_cascade, all_entries);
    let full_dep_map = build_full_dependency_map(all_entries);
    let cascade_blocks = find_cascade_blocks(&eligible_for_cascade, &all_statuses_map, &full_dep_map);
    assert!(
        cascade_blocks.is_empty(),
        "review must NOT trigger cascade blocking, found {} cascade block(s)",
        cascade_blocks.len()
    );
}

// ===========================================================================
// Task 4 — All-done scenario (AC #3)
// ===========================================================================

#[test]
fn test_watcher_poll_all_done_returns_no_eligible() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "done"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "done"),
        ("epic-2", "done"),
        ("2-1-polling", "done"),
    ];
    write_sprint_status(tmp.path(), entries);

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
    // A depends on B, B depends on A → cycle.
    // make_test_story defaults to status="ready-for-dev" (no override needed).
    let story_a = make_test_story("1-1-alpha", "alpha", vec!["1-2-beta".to_string()]);
    let story_b = make_test_story("1-2-beta", "beta", vec!["1-1-alpha".to_string()]);

    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-alpha".to_string(), "ready-for-dev".to_string()),
        ("1-2-beta".to_string(), "ready-for-dev".to_string()),
    ];

    let graph = DependencyGraph::new(&[story_a, story_b], &all_statuses);
    let result = graph.topological_sort();

    assert!(result.is_err());
    let err = result.unwrap_err();
    match err {
        WatcherError::CyclicDependency { ref cycle } => {
            assert!(
                cycle.contains(&"1-1-alpha".to_string()),
                "Cycle should contain 1-1-alpha, got: {cycle:?}"
            );
            assert!(
                cycle.contains(&"1-2-beta".to_string()),
                "Cycle should contain 1-2-beta, got: {cycle:?}"
            );
        }
        _ => panic!("Expected CyclicDependency, got: {err:?}"),
    }
}

// ===========================================================================
// Task 6 — Missing file (AC #5)
// ===========================================================================

#[test]
fn test_watcher_poll_missing_file_returns_error() {
    // Create Watcher pointing to a temp dir with no sprint-status.yaml.
    let tmp = tempfile::tempdir().unwrap();
    // Do NOT write sprint-status.yaml

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusNotFound { ref path } if path.contains("sprint-status.yaml")),
        "Expected SprintStatusNotFound containing 'sprint-status.yaml', got: {err:?}"
    );
}

// ===========================================================================
// Task 7 — SprintStatusFile integration tests (supplementary)
// ===========================================================================

#[test]
fn test_sprint_status_load_valid_yaml_correct_story_count() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), entries);

    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).expect("load should succeed");

    // stories() filters out epics and retrospectives
    let stories = ssf.stories();
    assert_eq!(stories.len(), 3, "Should have 3 stories (excluding epics/retros)");

    // Verify order preservation
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[1].story_key, "1-2-cli-framework");
    assert_eq!(stories[2].story_key, "2-1-polling");
}

#[test]
fn test_sprint_status_stories_filters_out_epics_and_retros() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("epic-2-retrospective", "optional"),
    ];
    write_sprint_status(tmp.path(), entries);

    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();

    let stories = ssf.stories();
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-1-scaffolding", "2-1-polling"]);
    // No epic-* or *-retrospective entries
    assert!(keys.iter().all(|k| !k.starts_with("epic-")));
    assert!(keys.iter().all(|k| !k.contains("retrospective")));
}

#[test]
fn test_sprint_status_eligible_stories_returns_only_ready_for_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "in-progress"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    write_sprint_status(tmp.path(), entries);

    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();

    let eligible = ssf.eligible_stories();
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-2-cli-framework", "2-1-polling"]);
}

#[test]
fn test_sprint_status_malformed_yaml_returns_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sprint-status.yaml");
    std::fs::write(&path, "{{{{invalid yaml: [[[").unwrap();

    let result = SprintStatusFile::load(&path, tmp.path());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusParse(_)),
        "Expected SprintStatusParse, got: {err:?}"
    );
}

#[test]
fn test_sprint_status_entries_returns_all_raw_entries() {
    // entries() returns ALL entries including epics and retrospectives (no filtering)
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), entries);

    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();

    let raw = ssf.entries();
    // All 6 entries preserved — epics and retrospectives are NOT filtered out
    assert_eq!(raw.len(), 6);
    // Verify specific keys present (order-preserving)
    let raw_keys: Vec<&str> = raw.iter().map(|(k, _)| k.as_str()).collect();
    assert!(raw_keys.contains(&"epic-1"));
    assert!(raw_keys.contains(&"epic-1-retrospective"));
    assert!(raw_keys.contains(&"1-1-scaffolding"));
    assert!(raw_keys.contains(&"2-1-polling"));
}

#[test]
fn test_sprint_status_entry_count_matches_raw_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    write_sprint_status(tmp.path(), entries);

    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();

    // entry_count() == entries().len() — consistent view of raw entries
    assert_eq!(ssf.entry_count(), 5);
    assert_eq!(ssf.entry_count(), ssf.entries().len());
}
