//! Integration tests for the watcher → dependency resolution → story selection chain.
//!
//! Exercises `Watcher`, `SprintStatusFile`, `DependencyGraph`, and `deps` module
//! functions together with real filesystem I/O (isolated via `tempfile::tempdir()`).

use std::collections::HashMap;
use std::sync::Arc;

use bmad_bot::watcher::deps::{
    build_full_dependency_map, derive_dependencies, find_cascade_blocks, DependencyGraph,
};
use bmad_bot::watcher::{SprintStatusFile, Watcher, WatcherError};

use crate::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};

// ---------------------------------------------------------------------------
// Task 2: Watcher poll with dependency filtering (AC #1)
// ---------------------------------------------------------------------------

#[test]
fn test_watcher_poll_returns_eligible_with_deps_satisfied() {
    // Arrange: 5 stories across 2 epics
    //   1-1: done, 1-2: ready-for-dev (depends on 1-1 → satisfied),
    //   1-3: ready-for-dev (depends on 1-2 → NOT satisfied),
    //   2-1: ready-for-dev (no deps → eligible),
    //   2-2: backlog (not eligible)
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

    // Assert: only 1-2 and 2-1 are eligible
    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 2, "Expected 2 eligible stories, got: {keys:?}");
    assert!(
        keys.contains(&"1-2-cli-framework"),
        "Expected 1-2-cli-framework in eligible"
    );
    assert!(
        keys.contains(&"2-1-polling"),
        "Expected 2-1-polling in eligible"
    );
    assert!(
        !keys.contains(&"1-3-init-command"),
        "1-3 should NOT be eligible (dep 1-2 not done)"
    );
}

#[test]
fn test_watcher_poll_dependency_valid_ordering() {
    // Arrange: same scenario — verify sprint-order tiebreaker is respected.
    // Both 1-2 and 2-1 have in-degree 0 in the eligible graph (1-2's dep 1-1 is
    // not in the eligible set). Document order must be the tiebreaker:
    // 1-2 (pos 2 in YAML) must come before 2-1 (pos 5 in YAML).
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

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    assert_eq!(keys.len(), 2, "Expected exactly 2 eligible stories, got: {keys:?}");
    // Topological sort with doc-order tiebreaker: 1-2 (pos 2) before 2-1 (pos 5)
    assert_eq!(
        keys[0], "1-2-cli-framework",
        "1-2 should be first (earlier doc position)"
    );
    assert_eq!(
        keys[1], "2-1-polling",
        "2-1 should be second (later doc position)"
    );
}

// ---------------------------------------------------------------------------
// Task 3: Cascade blocking tests (AC #2)
// ---------------------------------------------------------------------------

#[test]
fn test_watcher_cascade_blocks_transitive_dependents() {
    // Arrange: 1-1 blocked, 1-2 and 1-3 ready-for-dev (sequential deps), 2-1 independent
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

    // Act
    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    // Assert: only 2-1 is eligible (1-2 and 1-3 cascade-blocked by 1-1)
    let eligible = result.expect("poll should succeed");
    assert_eq!(eligible.len(), 1, "Expected only 1 eligible story");
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_cascade_blocks_with_needs_clarification() {
    // Arrange: same as above but 1-1 is needs-clarification instead of blocked
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "needs-clarification"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    assert_eq!(eligible.len(), 1, "Expected only 1 eligible story");
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_no_cascade_on_in_progress_status() {
    // Negative test: 1-1 is in-progress (NOT a blocking status)
    // 1-2 should be skipped (dep not done) but NOT cascade-blocked
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "in-progress"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    // Convert to owned before write_sprint_status consumes entries
    let owned_entries: Vec<(String, String)> = entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let path = write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // 1-2 is skipped (dep not met) but not cascade-blocked — that's fine
    // 2-1 is eligible (independent)
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert!(
        keys.contains(&"2-1-polling"),
        "2-1 should be eligible (independent)"
    );
    assert!(
        !keys.contains(&"1-2-cli-framework"),
        "1-2 should be skipped (dep not done)"
    );

    // CRITICAL: directly verify in-progress does NOT trigger cascade blocking.
    // poll() cannot distinguish "dep not met" from "cascade-blocked" by output alone.
    let all_statuses_map: HashMap<String, String> = owned_entries.iter().cloned().collect();
    let full_dep_map = build_full_dependency_map(&owned_entries);
    let sprint_status = SprintStatusFile::load(&path, tmp.path()).unwrap();
    let mut eligible_stories = sprint_status.eligible_stories();
    derive_dependencies(&mut eligible_stories, &owned_entries);
    let cascade_blocks = find_cascade_blocks(&eligible_stories, &all_statuses_map, &full_dep_map);
    assert!(
        cascade_blocks.is_empty(),
        "in-progress must NOT trigger cascade blocks, got: {cascade_blocks:?}"
    );
}

#[test]
fn test_watcher_no_cascade_on_review_status() {
    // Negative test: 1-1 is in review (NOT a blocking status)
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "review"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    let owned_entries: Vec<(String, String)> = entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let path = write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert!(
        keys.contains(&"2-1-polling"),
        "2-1 should be eligible (independent)"
    );
    assert!(
        !keys.contains(&"1-2-cli-framework"),
        "1-2 should be skipped (dep not done, but NOT cascade-blocked)"
    );

    // CRITICAL: directly verify review does NOT trigger cascade blocking.
    let all_statuses_map: HashMap<String, String> = owned_entries.iter().cloned().collect();
    let full_dep_map = build_full_dependency_map(&owned_entries);
    let sprint_status = SprintStatusFile::load(&path, tmp.path()).unwrap();
    let mut eligible_stories = sprint_status.eligible_stories();
    derive_dependencies(&mut eligible_stories, &owned_entries);
    let cascade_blocks = find_cascade_blocks(&eligible_stories, &all_statuses_map, &full_dep_map);
    assert!(
        cascade_blocks.is_empty(),
        "review status must NOT trigger cascade blocks, got: {cascade_blocks:?}"
    );
}

// ---------------------------------------------------------------------------
// Task 4: All-done scenario (AC #3)
// ---------------------------------------------------------------------------

#[test]
fn test_watcher_poll_all_done_returns_no_eligible_stories() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "done"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "done"),
    ];
    write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err(), "Expected error when all stories are done");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::NoEligibleStories),
        "Expected NoEligibleStories, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Additional AC #3 coverage: NoEligibleStories via dep-filter (second code path)
// ---------------------------------------------------------------------------

#[test]
fn test_watcher_poll_no_eligible_after_dep_filter() {
    // Scenario: 1-2 is ready-for-dev but its dep 1-1 is in-progress (not done).
    // filter_eligible skips 1-2 → filtered list is empty → NoEligibleStories.
    // This exercises the SECOND NoEligibleStories branch in Watcher::poll()
    // (line 334: `if filtered.is_empty()`), distinct from the first branch
    // (line 319: `if eligible.is_empty()`) exercised by test_watcher_poll_all_done_returns_no_eligible_stories.
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "in-progress"), // dep NOT done
        ("1-2-cli-framework", "ready-for-dev"), // depends on 1-1, dep unmet
    ];
    write_sprint_status(tmp.path(), entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(
        result.is_err(),
        "Expected error when all ready-for-dev stories have unmet deps"
    );
    assert!(
        matches!(result.unwrap_err(), WatcherError::NoEligibleStories),
        "Expected NoEligibleStories when dep-filter empties the eligible list"
    );
}

// ---------------------------------------------------------------------------
// Task 5: Cyclic dependency test (AC #4)
// ---------------------------------------------------------------------------

#[test]
fn test_watcher_cyclic_dependency_detected() {
    // Create stories with manually injected circular deps
    // A depends on B, B depends on A
    let story_a = make_test_story(
        "1-1-alpha",
        "alpha",
        vec!["1-2-beta".to_string()],
    );
    let story_b = make_test_story(
        "1-2-beta",
        "beta",
        vec!["1-1-alpha".to_string()],
    );

    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-alpha".to_string(), "ready-for-dev".to_string()),
        ("1-2-beta".to_string(), "ready-for-dev".to_string()),
    ];

    let graph = DependencyGraph::new(&[story_a, story_b], &all_statuses);
    let result = graph.topological_sort();

    assert!(result.is_err(), "Expected cycle detection error");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::CyclicDependency { ref cycle } if !cycle.is_empty()),
        "Expected CyclicDependency with non-empty cycle, got: {err}"
    );

    // Verify the error contains BOTH story keys involved in the cycle
    if let WatcherError::CyclicDependency { cycle } = err {
        assert!(
            cycle.contains(&"1-1-alpha".to_string()) && cycle.contains(&"1-2-beta".to_string()),
            "Cycle should contain BOTH involved story keys, got: {cycle:?}"
        );
    }
}

#[test]
fn test_watcher_cyclic_dependency_three_node_cycle() {
    // A → B → C → A
    let story_a = make_test_story("1-1-alpha", "alpha", vec!["1-3-gamma".to_string()]);
    let story_b = make_test_story("1-2-beta", "beta", vec!["1-1-alpha".to_string()]);
    let story_c = make_test_story("1-3-gamma", "gamma", vec!["1-2-beta".to_string()]);

    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-alpha".to_string(), "ready-for-dev".to_string()),
        ("1-2-beta".to_string(), "ready-for-dev".to_string()),
        ("1-3-gamma".to_string(), "ready-for-dev".to_string()),
    ];

    let graph = DependencyGraph::new(&[story_a, story_b, story_c], &all_statuses);
    let result = graph.topological_sort();

    assert!(result.is_err(), "Expected cycle detection error");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::CyclicDependency { ref cycle } if cycle.len() >= 2),
        "Expected CyclicDependency with multiple stories, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Task 6: Missing file test (AC #5)
// ---------------------------------------------------------------------------

#[test]
fn test_watcher_poll_missing_sprint_status_returns_error() {
    // Create a temp dir with NO sprint-status.yaml
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err(), "Expected error for missing sprint-status.yaml");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusNotFound { ref path } if path.contains("sprint-status.yaml")),
        "Expected SprintStatusNotFound with path containing 'sprint-status.yaml', got: {err}"
    );
}

#[test]
fn test_watcher_poll_missing_file_error_contains_path() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let err = watcher.poll().unwrap_err();

    // Verify the error message contains the expected path
    let err_string = format!("{err}");
    assert!(
        err_string.contains("sprint-status.yaml"),
        "Error message should contain 'sprint-status.yaml', got: {err_string}"
    );
}

// ---------------------------------------------------------------------------
// Task 7: SprintStatusFile integration tests (supplementary)
// ---------------------------------------------------------------------------

#[test]
fn test_sprint_status_load_valid_yaml_correct_story_count() {
    let tmp = tempfile::tempdir().unwrap();
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
    let path = write_sprint_status(tmp.path(), entries);

    let ssf = SprintStatusFile::load(&path, tmp.path()).expect("load should succeed");

    // stories() filters out epics and retrospectives
    let stories = ssf.stories();
    assert_eq!(stories.len(), 5, "Expected 5 stories (epics/retro filtered out)");

    // entry_count() includes all entries
    assert_eq!(ssf.entry_count(), 8, "Expected 8 total entries including epics");
}

#[test]
fn test_sprint_status_stories_filters_epics_and_retrospectives() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("epic-2-retrospective", "optional"),
    ];
    let path = write_sprint_status(tmp.path(), entries);
    let ssf = SprintStatusFile::load(&path, tmp.path()).expect("load should succeed");

    let stories = ssf.stories();
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();

    // Only actual stories, no epics or retrospectives
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&"1-1-scaffolding"));
    assert!(keys.contains(&"2-1-polling"));
    assert!(!keys.iter().any(|k| k.starts_with("epic-")));
    assert!(!keys.iter().any(|k| k.contains("retrospective")));
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
    let path = write_sprint_status(tmp.path(), entries);
    let ssf = SprintStatusFile::load(&path, tmp.path()).expect("load should succeed");

    let eligible = ssf.eligible_stories();
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&"1-2-cli-framework"));
    assert!(keys.contains(&"2-1-polling"));
}

#[test]
fn test_sprint_status_preserves_insertion_order() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
    ];
    let path = write_sprint_status(tmp.path(), entries);
    let ssf = SprintStatusFile::load(&path, tmp.path()).expect("load should succeed");

    let all_entries = ssf.entries();
    let keys: Vec<&str> = all_entries.iter().map(|(k, _)| k.as_str()).collect();

    // Order should match YAML insertion order
    assert_eq!(keys[0], "epic-2");
    assert_eq!(keys[1], "2-1-polling");
    assert_eq!(keys[2], "epic-1");
    assert_eq!(keys[3], "1-1-scaffolding");
    assert_eq!(keys[4], "1-2-cli-framework");
}

#[test]
fn test_sprint_status_malformed_yaml_returns_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sprint-status.yaml");
    std::fs::write(&path, "{{{{invalid yaml!!!!").expect("write");

    let result = SprintStatusFile::load(&path, tmp.path());
    assert!(result.is_err(), "Expected parse error for malformed YAML");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusParse(_)),
        "Expected SprintStatusParse, got: {err}"
    );
}
