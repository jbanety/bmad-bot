//! Integration tests: Watcher → Dependency Resolution → Story Selection.
//!
//! Story 7.3 — exercises the full `Watcher::poll()` → `deps::filter_eligible()` chain
//! with real YAML files on disk. No mocks — only filesystem isolation via `tempfile`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};
use bmad_bot::watcher::deps::{
    build_full_dependency_map, derive_dependencies, filter_eligible, find_cascade_blocks,
    DependencyGraph,
};
use bmad_bot::watcher::{SprintStatusFile, Watcher, WatcherError};

// ===========================================================================
// Helpers
// ===========================================================================

/// Create the `_bmad-output/implementation-artifacts` subdirectory inside `root`
/// and return its path. Required because `make_test_config(root)` sets
/// `implementation_artifacts` to `root/_bmad-output/implementation-artifacts`.
fn artifacts_dir(root: &std::path::Path) -> std::path::PathBuf {
    let dir = root.join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&dir).expect("create artifacts dir");
    dir
}

// ===========================================================================
// Task 2: Watcher poll with dependency filtering (AC #1)
// ===========================================================================

#[test]
fn test_watcher_poll_returns_eligible_with_deps_satisfied() {
    // Arrange: 5 stories across 2 epics
    //   1-1 done, 1-2 ready-for-dev (dep 1-1 done → eligible),
    //   1-3 ready-for-dev (dep 1-2 NOT done → skipped),
    //   2-1 ready-for-dev (no dep → eligible), 2-2 backlog (not eligible)
    let tmp = tempfile::tempdir().expect("tempdir");
    let art = artifacts_dir(tmp.path());
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
    write_sprint_status(&art, &entries);
    let config = make_test_config(tmp.path());

    // Act
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    // Assert
    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 2, "expected 2 eligible stories, got {keys:?}");
    assert!(keys.contains(&"1-2-cli-framework"), "1-2 should be eligible");
    assert!(keys.contains(&"2-1-polling"), "2-1 should be eligible");
    assert!(
        !keys.contains(&"1-3-init-command"),
        "1-3 should NOT be eligible (dep 1-2 not done)"
    );
    assert!(
        !keys.contains(&"2-2-deps-resolution"),
        "2-2 should NOT be eligible (backlog)"
    );
}

#[test]
fn test_watcher_poll_dependency_valid_ordering() {
    // Arrange: same as above, verify topological order
    let tmp = tempfile::tempdir().expect("tempdir");
    let art = artifacts_dir(tmp.path());
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&art, &entries);
    let config = make_test_config(tmp.path());

    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    // 1-2 must appear before any story that depends on it.
    // In this case 1-3 is filtered out, so only ordering constraint is
    // document order: 1-2 before 2-1.
    assert_eq!(keys, vec!["1-2-cli-framework", "2-1-polling"]);
}

// ===========================================================================
// Task 3: Cascade blocking tests (AC #2)
// ===========================================================================

#[test]
fn test_watcher_cascade_blocks_transitive_dependents() {
    // Arrange: 1-1 blocked → 1-2 and 1-3 cascade-blocked, 2-1 independent
    let tmp = tempfile::tempdir().expect("tempdir");
    let art = artifacts_dir(tmp.path());
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&art, &entries);
    let config = make_test_config(tmp.path());

    // Act
    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // Assert: only 2-1 is eligible (1-2 and 1-3 cascade-blocked by 1-1)
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_cascade_blocks_needs_clarification() {
    // Arrange: 1-1 needs-clarification → same cascade behavior as blocked
    let tmp = tempfile::tempdir().expect("tempdir");
    let art = artifacts_dir(tmp.path());
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "needs-clarification"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&art, &entries);
    let config = make_test_config(tmp.path());

    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

#[test]
fn test_watcher_no_cascade_on_in_progress() {
    // Arrange: 1-1 is in-progress → 1-2 is skipped (dep not done) but NOT cascade-blocked
    let tmp = tempfile::tempdir().expect("tempdir");
    let art = artifacts_dir(tmp.path());
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "in-progress"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&art, &entries);
    let config = make_test_config(tmp.path());

    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // 1-2 skipped because dep 1-1 not done, but NOT cascade-blocked.
    // 2-1 eligible (no deps). Result: [2-1]
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["2-1-polling"]);
}

#[test]
fn test_watcher_no_cascade_on_review() {
    // Arrange: 1-1 is review → 1-2 is skipped (dep not done) but NOT cascade-blocked
    let tmp = tempfile::tempdir().expect("tempdir");
    let art = artifacts_dir(tmp.path());
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "review"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    write_sprint_status(&art, &entries);
    let config = make_test_config(tmp.path());

    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    // Same as in-progress: 1-2 skipped, 2-1 eligible
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["2-1-polling"]);
}

// ===========================================================================
// Task 4: All-done scenario (AC #3)
// ===========================================================================

#[test]
fn test_watcher_poll_all_done_returns_no_eligible() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let art = artifacts_dir(tmp.path());
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "done"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "done"),
    ];
    write_sprint_status(&art, &entries);
    let config = make_test_config(tmp.path());

    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        WatcherError::NoEligibleStories
    ));
}

// ===========================================================================
// Task 5: Cyclic dependency test (AC #4)
// ===========================================================================

#[test]
fn test_watcher_cyclic_dependency_detected() {
    // Arrange: manually create stories with circular deps
    // A depends on B, B depends on A
    let mut story_a = make_test_story("1-1-alpha", "Alpha", vec!["1-2-beta".to_string()]);
    let mut story_b = make_test_story("1-2-beta", "Beta", vec!["1-1-alpha".to_string()]);
    story_a.status = "ready-for-dev".to_string();
    story_b.status = "ready-for-dev".to_string();

    let all_statuses: Vec<(String, String)> = vec![
        ("epic-1".to_string(), "in-progress".to_string()),
        ("1-1-alpha".to_string(), "ready-for-dev".to_string()),
        ("1-2-beta".to_string(), "ready-for-dev".to_string()),
    ];

    // Act: build graph and attempt topological sort
    let stories = vec![story_a, story_b];
    let graph = DependencyGraph::new(&stories, &all_statuses);
    let result = graph.topological_sort();

    // Assert: CyclicDependency with both keys
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::CyclicDependency { ref cycle }
            if cycle.contains(&"1-1-alpha".to_string())
            && cycle.contains(&"1-2-beta".to_string())),
        "expected CyclicDependency containing both keys, got: {err:?}"
    );
}

#[test]
fn test_watcher_cyclic_dependency_three_way() {
    // A → B → C → A (3-way cycle)
    let story_a = make_test_story("1-1-aaa", "AAA", vec!["1-3-ccc".to_string()]);
    let story_b = make_test_story("1-2-bbb", "BBB", vec!["1-1-aaa".to_string()]);
    let story_c = make_test_story("1-3-ccc", "CCC", vec!["1-2-bbb".to_string()]);

    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-aaa".to_string(), "ready-for-dev".to_string()),
        ("1-2-bbb".to_string(), "ready-for-dev".to_string()),
        ("1-3-ccc".to_string(), "ready-for-dev".to_string()),
    ];

    let stories = vec![story_a, story_b, story_c];
    let graph = DependencyGraph::new(&stories, &all_statuses);
    let result = graph.topological_sort();

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        WatcherError::CyclicDependency { ref cycle } if cycle.len() == 3
    ));
}

// ===========================================================================
// Task 6: Missing file test (AC #5)
// ===========================================================================

#[test]
fn test_watcher_poll_missing_file_returns_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Create the artifacts dir but do NOT create sprint-status.yaml
    let _art = artifacts_dir(tmp.path());
    let config = make_test_config(tmp.path());

    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusNotFound { ref path }
            if path.contains("sprint-status.yaml")),
        "expected SprintStatusNotFound containing 'sprint-status.yaml', got: {err:?}"
    );
}

// ===========================================================================
// Task 7: SprintStatusFile integration tests (supplementary)
// ===========================================================================

#[test]
fn test_sprint_status_load_valid_yaml_correct_story_count() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-command", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-polling", "ready-for-dev"),
    ];
    let path = write_sprint_status(tmp.path(), &entries);

    let sprint = SprintStatusFile::load(&path, tmp.path()).expect("should load");

    // stories() filters out epics and retrospectives
    let stories = sprint.stories();
    assert_eq!(stories.len(), 4, "expected 4 stories, got {}", stories.len());

    // Order preserved from YAML
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[1].story_key, "1-2-cli-framework");
    assert_eq!(stories[2].story_key, "1-3-init-command");
    assert_eq!(stories[3].story_key, "2-1-polling");
}

#[test]
fn test_sprint_status_stories_filters_epics_and_retrospectives() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-a", "done"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "in-progress"),
        ("2-1-story-b", "ready-for-dev"),
        ("epic-2-retrospective", "done"),
    ];
    let path = write_sprint_status(tmp.path(), &entries);

    let sprint = SprintStatusFile::load(&path, tmp.path()).expect("should load");
    let stories = sprint.stories();

    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-story-a");
    assert_eq!(stories[1].story_key, "2-1-story-b");
}

#[test]
fn test_sprint_status_eligible_stories_returns_only_ready_for_dev() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-a", "done"),
        ("1-2-story-b", "ready-for-dev"),
        ("1-3-story-c", "in-progress"),
        ("1-4-story-d", "backlog"),
        ("1-5-story-e", "ready-for-dev"),
        ("1-6-story-f", "blocked"),
        ("1-7-story-g", "needs-clarification"),
        ("1-8-story-h", "review"),
    ];
    let path = write_sprint_status(tmp.path(), &entries);

    let sprint = SprintStatusFile::load(&path, tmp.path()).expect("should load");
    let eligible = sprint.eligible_stories();

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys, vec!["1-2-story-b", "1-5-story-e"]);
}

#[test]
fn test_sprint_status_malformed_yaml_returns_parse_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("sprint-status.yaml");
    std::fs::write(&path, "{{{{invalid yaml: [[[").expect("write");

    let result = SprintStatusFile::load(&path, tmp.path());

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        WatcherError::SprintStatusParse(_)
    ));
}

// ===========================================================================
// Additional edge-case tests
// ===========================================================================

#[test]
fn test_watcher_poll_single_eligible_story_no_deps() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let art = artifacts_dir(tmp.path());
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-only-story", "ready-for-dev"),
    ];
    write_sprint_status(&art, &entries);
    let config = make_test_config(tmp.path());

    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-1-only-story");
}

#[test]
fn test_watcher_poll_multiple_epics_independent() {
    // Stories from different epics have no cross-epic deps
    let tmp = tempfile::tempdir().expect("tempdir");
    let art = artifacts_dir(tmp.path());
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-alpha", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-beta", "ready-for-dev"),
        ("epic-3", "in-progress"),
        ("3-1-gamma", "ready-for-dev"),
    ];
    write_sprint_status(&art, &entries);
    let config = make_test_config(tmp.path());

    let watcher = Watcher::new(Arc::new(config));
    let eligible = watcher.poll().expect("poll should succeed");

    assert_eq!(eligible.len(), 3);
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert!(keys.contains(&"1-1-alpha"));
    assert!(keys.contains(&"2-1-beta"));
    assert!(keys.contains(&"3-1-gamma"));
}

#[test]
fn test_derive_dependencies_integration_with_sprint_status() {
    // Verify derive_dependencies uses sprint-status entries to resolve deps
    let tmp = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli", "ready-for-dev"),
        ("1-3-init", "ready-for-dev"),
    ];
    let path = write_sprint_status(tmp.path(), &entries);

    let sprint = SprintStatusFile::load(&path, tmp.path()).expect("load");
    let mut stories = sprint.eligible_stories();
    let all_entries = sprint.entries();

    derive_dependencies(&mut stories, all_entries);

    // 1-2 should depend on 1-1, 1-3 should depend on 1-2
    assert_eq!(stories[0].story_key, "1-2-cli");
    assert_eq!(stories[0].dependencies, vec!["1-1-scaffolding"]);

    assert_eq!(stories[1].story_key, "1-3-init");
    assert_eq!(stories[1].dependencies, vec!["1-2-cli"]);
}

#[test]
fn test_filter_eligible_integration_returns_topological_order() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-a", "done"),
        ("1-2-b", "done"),
        ("1-3-c", "ready-for-dev"),
        ("epic-2", "in-progress"),
        ("2-1-d", "ready-for-dev"),
    ];
    let path = write_sprint_status(tmp.path(), &entries);

    let sprint = SprintStatusFile::load(&path, tmp.path()).expect("load");
    let eligible = sprint.eligible_stories();
    let all_entries = sprint.entries();

    let (filtered, cascade_count) =
        filter_eligible(eligible, all_entries).expect("filter_eligible");

    assert_eq!(cascade_count, 0);
    assert_eq!(filtered.len(), 2);
    // 1-3 depends on 1-2 (done) → eligible
    // 2-1 no deps → eligible
    let keys: Vec<&str> = filtered.iter().map(|s| s.story_key.as_str()).collect();
    assert!(keys.contains(&"1-3-c"));
    assert!(keys.contains(&"2-1-d"));
}

#[test]
fn test_find_cascade_blocks_integration_with_full_dep_map() {
    // Build everything from scratch using real sprint-status structures
    let tmp = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "blocked"),
        ("1-2-cli", "ready-for-dev"),
        ("1-3-init", "ready-for-dev"),
    ];
    let path = write_sprint_status(tmp.path(), &entries);

    let sprint = SprintStatusFile::load(&path, tmp.path()).expect("load");
    let all_entries = sprint.entries();
    let mut stories = sprint.stories();
    derive_dependencies(&mut stories, all_entries);

    let all_statuses_map: HashMap<String, String> = all_entries.iter().cloned().collect();
    let full_dep_map = build_full_dependency_map(all_entries);
    let blocks = find_cascade_blocks(&stories, &all_statuses_map, &full_dep_map);

    // 1-2 and 1-3 should be cascade-blocked
    let blocked_keys: Vec<&str> = blocks.iter().map(|b| b.blocked_story.as_str()).collect();
    assert!(blocked_keys.contains(&"1-2-cli"), "1-2 should be cascade-blocked");
    assert!(
        blocked_keys.contains(&"1-3-init"),
        "1-3 should be cascade-blocked"
    );

    // Root cause should be 1-1 for both
    for block in &blocks {
        assert_eq!(block.root_cause_story, "1-1-scaffolding");
        assert_eq!(block.root_cause_status, "blocked");
    }
}
