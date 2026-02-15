//! Integration tests for Watcher → Dependency Resolution → Story Selection.
//!
//! Story 7.3 — verifies the full watcher poll → deps → eligible story
//! selection chain using real filesystem fixtures (no mocks).

use std::sync::Arc;

use bmad_bot::watcher::deps::{
    build_full_dependency_map, filter_eligible, find_cascade_blocks, DependencyGraph,
};
use bmad_bot::watcher::{SprintStatusFile, StoryInfo, Watcher, WatcherError};

use crate::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};

// ===========================================================================
// Task 2 — Watcher poll with dependency filtering (AC #1)
// ===========================================================================

#[test]
fn test_watcher_poll_returns_eligible_with_deps_satisfied() {
    // Arrange: 5 stories across 2 epics
    //   1-1 done, 1-2 ready-for-dev (dep on 1-1 → satisfied), 1-3 ready-for-dev (dep on 1-2 → NOT satisfied)
    //   2-1 ready-for-dev (first in epic → no dep), 2-2 backlog
    let tmp = tempfile::tempdir().unwrap();
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
    write_sprint_status(tmp.path(), &entries);

    // Act
    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    // Assert: eligible = [1-2-cli-framework, 2-1-polling]
    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 2, "Expected exactly 2 eligible stories, got {keys:?}");
    assert!(keys.contains(&"1-2-cli-framework"), "1-2 should be eligible (dep 1-1 is done)");
    assert!(keys.contains(&"2-1-polling"), "2-1 should be eligible (no deps)");
    assert!(
        !keys.contains(&"1-3-init-command"),
        "1-3 should NOT be eligible (dep 1-2 not done)"
    );
}

#[test]
fn test_watcher_poll_dependency_valid_ordering() {
    // Arrange: same setup — verify ordering is dependency-valid
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    write_sprint_status(tmp.path(), &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().unwrap();
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    // 1-2 must appear before any story that depends on it (1-3 is excluded,
    // but 1-2 and 2-1 are independent — both valid in either order).
    // The topological sort with document-order tiebreaker places 1-2 first.
    assert_eq!(keys[0], "1-2-cli-framework", "1-2 should come first (document order tiebreaker)");
    assert_eq!(keys[1], "2-1-polling", "2-1 should come second");
}

// ===========================================================================
// Task 3 — Cascade blocking tests (AC #2)
// ===========================================================================

#[test]
fn test_watcher_cascade_blocks_transitive_dependents() {
    // Arrange: 1-1 blocked, 1-2 ready-for-dev (dep on 1-1), 1-3 ready-for-dev (dep on 1-2)
    //          2-1 ready-for-dev (independent epic)
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), &entries);

    // Act
    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().unwrap();

    // Assert: only 2-1 is eligible (1-2 and 1-3 cascade-blocked by 1-1)
    assert_eq!(eligible.len(), 1, "Only 2-1 should be eligible");
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_cascade_blocks_with_needs_clarification() {
    // Arrange: 1-1 needs-clarification triggers same cascade as blocked
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "needs-clarification"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().unwrap();

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_no_cascade_on_in_progress_dep() {
    // Arrange: 1-1 in-progress (NOT a blocking status), 1-2 ready-for-dev
    // 1-2 should be skipped (dep not done) but NOT cascade-blocked
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "in-progress"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().unwrap();

    // 1-2 is skipped (dep not done) but NOT cascade-blocked
    // 2-1 is eligible
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0], "2-1-polling");

    // Verify via find_cascade_blocks directly that 1-2 is NOT cascade-blocked
    let all_statuses_map: std::collections::HashMap<String, String> = entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let all_statuses_slice: Vec<(String, String)> =
        entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let full_dep_map = build_full_dependency_map(&all_statuses_slice);

    let story_1_2 = StoryInfo::from_key_and_status(
        "1-2-cli-framework",
        "ready-for-dev",
        tmp.path(),
    )
    .unwrap();
    let cascades = find_cascade_blocks(&[story_1_2], &all_statuses_map, &full_dep_map);
    assert!(cascades.is_empty(), "in-progress should NOT trigger cascade blocking");
}

#[test]
fn test_watcher_no_cascade_on_review_dep() {
    // Arrange: 1-1 review (NOT a blocking status), 1-2 ready-for-dev
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "review"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().unwrap();

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0], "2-1-polling");

    // Verify no cascade blocking for review status
    let all_statuses_map: std::collections::HashMap<String, String> = entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let all_statuses_slice: Vec<(String, String)> =
        entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let full_dep_map = build_full_dependency_map(&all_statuses_slice);

    let story_1_2 = StoryInfo::from_key_and_status(
        "1-2-cli-framework",
        "ready-for-dev",
        tmp.path(),
    )
    .unwrap();
    let cascades = find_cascade_blocks(&[story_1_2], &all_statuses_map, &full_dep_map);
    assert!(cascades.is_empty(), "review should NOT trigger cascade blocking");
}

#[test]
fn test_watcher_cascade_independent_epic_unaffected() {
    // Arrange: epic-1 has blocked root, epic-2 is independent
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(tmp.path(), &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().unwrap();

    // 2-1 is unaffected by epic-1's blocking
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

// ===========================================================================
// Task 4 — All-done scenario (AC #3)
// ===========================================================================

#[test]
fn test_watcher_poll_all_done_returns_no_eligible() {
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "done"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "done"),
        ("epic-2", "done"),
        ("2-1-polling", "done"),
    ];
    write_sprint_status(tmp.path(), &entries);

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err(), "Should return error when all stories are done");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::NoEligibleStories),
        "Expected NoEligibleStories, got: {err}"
    );
}

// ===========================================================================
// Task 5 — Cyclic dependency (AC #4)
// ===========================================================================

#[test]
fn test_watcher_cyclic_dependency_detected() {
    // Arrange: manually create stories with circular deps
    let mut story_a = make_test_story("1-1-foo", "foo", vec!["1-2-bar".to_string()]);
    let mut story_b = make_test_story("1-2-bar", "bar", vec!["1-1-foo".to_string()]);
    // Override status to ready-for-dev (make_test_story already does this)
    story_a.status = "ready-for-dev".to_string();
    story_b.status = "ready-for-dev".to_string();

    let all_statuses: Vec<(String, String)> = vec![
        ("epic-1".to_string(), "in-progress".to_string()),
        ("1-1-foo".to_string(), "ready-for-dev".to_string()),
        ("1-2-bar".to_string(), "ready-for-dev".to_string()),
    ];

    // Use DependencyGraph + topological_sort directly
    let graph = DependencyGraph::new(&[story_a, story_b], &all_statuses);
    let result = graph.topological_sort();

    assert!(result.is_err(), "Should detect cyclic dependency");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::CyclicDependency { ref cycle } if cycle.contains(&"1-1-foo".to_string()) && cycle.contains(&"1-2-bar".to_string())),
        "Expected CyclicDependency containing both story keys, got: {err}"
    );
}

#[test]
fn test_watcher_cyclic_dependency_via_filter_eligible() {
    // Verify the cycle also propagates through the full filter_eligible path
    let mut story_a = make_test_story("1-1-foo", "foo", vec!["1-2-bar".to_string()]);
    let mut story_b = make_test_story("1-2-bar", "bar", vec!["1-1-foo".to_string()]);
    story_a.status = "ready-for-dev".to_string();
    story_b.status = "ready-for-dev".to_string();

    let all_statuses: Vec<(String, String)> = vec![
        ("epic-1".to_string(), "in-progress".to_string()),
        ("1-1-foo".to_string(), "ready-for-dev".to_string()),
        ("1-2-bar".to_string(), "ready-for-dev".to_string()),
    ];

    // Note: filter_eligible calls derive_dependencies which will add 1-1 as dep of 1-2.
    // Since 1-2 already depends on 1-1, and 1-1 depends on 1-2 (manual), this creates a cycle.
    let result = filter_eligible(vec![story_a, story_b], &all_statuses);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        WatcherError::CyclicDependency { .. }
    ));
}

// ===========================================================================
// Task 6 — Missing file (AC #5)
// ===========================================================================

#[test]
fn test_watcher_poll_missing_sprint_status_file() {
    // Arrange: temp dir with no sprint-status.yaml
    let tmp = tempfile::tempdir().unwrap();

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err(), "Should return error for missing file");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusNotFound { ref path } if path.contains("sprint-status.yaml")),
        "Expected SprintStatusNotFound with path containing sprint-status.yaml, got: {err}"
    );
}

// ===========================================================================
// Task 7 — SprintStatusFile integration tests (supplementary)
// ===========================================================================

#[test]
fn test_sprint_status_load_valid_yaml_story_count_and_order() {
    let tmp = tempfile::tempdir().unwrap();
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
    let path = write_sprint_status(tmp.path(), &entries);

    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
    let stories = ssf.stories();

    // stories() should filter out epics and retrospectives
    assert_eq!(stories.len(), 5, "Should have 5 stories (no epics/retros)");
    // Verify order preservation
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[1].story_key, "1-2-cli-framework");
    assert_eq!(stories[2].story_key, "1-3-init-command");
    assert_eq!(stories[3].story_key, "2-1-polling");
    assert_eq!(stories[4].story_key, "2-2-deps-resolution");
}

#[test]
fn test_sprint_status_stories_filters_out_epic_and_retrospective() {
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("epic-1-retrospective", "optional"),
    ];
    let path = write_sprint_status(tmp.path(), &entries);

    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
    let stories = ssf.stories();

    assert_eq!(stories.len(), 1);
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
}

#[test]
fn test_sprint_status_eligible_stories_returns_only_ready_for_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let entries: Vec<(&str, &str)> = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "in-progress"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
        ("2-2-deps-resolution", "backlog"),
    ];
    let path = write_sprint_status(tmp.path(), &entries);

    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
    let eligible = ssf.eligible_stories();

    assert_eq!(eligible.len(), 2);
    assert_eq!(eligible[0].story_key, "1-2-cli-framework");
    assert_eq!(eligible[1].story_key, "2-1-polling");
}

#[test]
fn test_sprint_status_malformed_yaml_returns_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sprint-status.yaml");
    std::fs::write(&path, "this is: [not: valid: yaml: {{{{").unwrap();

    let result = SprintStatusFile::load(&path, tmp.path());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusParse(_)),
        "Expected SprintStatusParse, got: {err}"
    );
}
