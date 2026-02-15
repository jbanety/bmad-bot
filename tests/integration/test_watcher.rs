//! Integration tests: Watcher → Dependency Resolution → Story Selection.
//!
//! Exercises the full Watcher::poll() → SprintStatusFile → DependencyGraph → filter_eligible chain
//! using real filesystem fixtures (no mocks). Covers AC #1–#5 from Story 7.3.

use std::sync::Arc;

use bmad_bot::watcher::deps::{filter_eligible, DependencyGraph};
use bmad_bot::watcher::{SprintStatusFile, Watcher, WatcherError};

use crate::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};

// ===========================================================================
// Task 2: Watcher poll with dependency filtering (AC #1)
// ===========================================================================

/// AC #1: Given 5 stories (1-1 done, 1-2 rfd, 1-3 rfd, 2-1 rfd, 2-2 backlog),
/// eligible stories are [1-2-*, 2-1-*] in dependency-valid order.
#[test]
fn test_watcher_poll_returns_eligible_with_deps_satisfied() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();

    write_sprint_status(
        &artifacts,
        &[
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

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    let eligible = result.expect("poll should succeed");
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    // 1-2 eligible (dep 1-1 done), 2-1 eligible (first in epic, no dep)
    // 1-3 skipped (dep 1-2 not done), 2-2 not ready-for-dev
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&"1-2-cli-framework"));
    assert!(keys.contains(&"2-1-polling"));
}

/// AC #1 ordering: 1-2 must appear before any story depending on it.
/// 2-1 is independent, so order between 1-2 and 2-1 is by document position.
#[test]
fn test_watcher_poll_dependency_valid_ordering() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();

    write_sprint_status(
        &artifacts,
        &[
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
    let eligible = watcher.poll().expect("poll should succeed");

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    // 1-2 should come before 2-1 based on document order (tiebreaker)
    let pos_1_2 = keys.iter().position(|k| *k == "1-2-cli-framework").unwrap();
    let pos_2_1 = keys.iter().position(|k| *k == "2-1-polling").unwrap();
    assert!(
        pos_1_2 < pos_2_1,
        "1-2 should appear before 2-1 in topological/document order"
    );
}

// ===========================================================================
// Task 3: Cascade blocking tests (AC #2)
// ===========================================================================

/// AC #2: 1-1 blocked → 1-2 and 1-3 cascade-blocked → only 2-1 eligible.
#[test]
fn test_watcher_cascade_blocks_transitive_dependents() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();

    write_sprint_status(
        &artifacts,
        &[
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

    // Only 2-1 should survive: 1-2 and 1-3 cascade-blocked by 1-1
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "2-1-polling");
}

/// AC #2: needs-clarification triggers same cascade as blocked.
#[test]
fn test_watcher_cascade_blocks_needs_clarification() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();

    write_sprint_status(
        &artifacts,
        &[
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

/// Task 3.5 negative: in-progress does NOT cascade-block, just skips.
#[test]
fn test_watcher_no_cascade_on_in_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();

    write_sprint_status(
        &artifacts,
        &[
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

    // 1-2 should NOT be cascade-blocked by in-progress — just skipped (dep not done)
    // Only 2-1 eligible since 1-2's dep is not done yet
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    assert_eq!(keys.len(), 1);
    assert!(keys.contains(&"2-1-polling"));
    // Importantly, 1-2 is NOT cascade-blocked — it's skipped because dep not done
    // We verify indirectly: if 1-2 were cascade-blocked AND it was the only eligible story,
    // the poll would still succeed with just 2-1
}

/// Task 3.5 negative: review status does NOT cascade-block.
#[test]
fn test_watcher_no_cascade_on_review() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();

    write_sprint_status(
        &artifacts,
        &[
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

    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
    // 1-2 dep not done (review != done), so skipped. 2-1 eligible.
    assert_eq!(keys.len(), 1);
    assert!(keys.contains(&"2-1-polling"));
}

/// Verify cascade blocking vs non-blocking directly using filter_eligible
/// to distinguish between cascade-blocked (count > 0) and simply skipped.
#[test]
fn test_filter_eligible_cascade_count_blocked_vs_in_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();

    // Scenario 1: blocked → cascade count > 0
    let entries_blocked: Vec<(String, String)> = vec![
        ("epic-1".into(), "in-progress".into()),
        ("1-1-scaffolding".into(), "blocked".into()),
        ("1-2-cli-framework".into(), "ready-for-dev".into()),
    ];

    let ssf = build_sprint_status_from_entries(&artifacts, &entries_blocked);
    let eligible_blocked = ssf.eligible_stories();
    let (_, cascade_count_blocked) =
        filter_eligible(eligible_blocked, ssf.entries()).expect("filter should succeed");
    assert!(
        cascade_count_blocked > 0,
        "blocked should trigger cascade blocking"
    );

    // Scenario 2: in-progress → cascade count == 0
    let entries_in_progress: Vec<(String, String)> = vec![
        ("epic-1".into(), "in-progress".into()),
        ("1-1-scaffolding".into(), "in-progress".into()),
        ("1-2-cli-framework".into(), "ready-for-dev".into()),
    ];

    let ssf2 = build_sprint_status_from_entries(&artifacts, &entries_in_progress);
    let eligible_ip = ssf2.eligible_stories();
    let (_, cascade_count_ip) =
        filter_eligible(eligible_ip, ssf2.entries()).expect("filter should succeed");
    assert_eq!(
        cascade_count_ip, 0,
        "in-progress should NOT trigger cascade blocking"
    );
}

/// Helper: write sprint-status and load SprintStatusFile
fn build_sprint_status_from_entries(
    artifacts_dir: &std::path::Path,
    entries: &[(String, String)],
) -> SprintStatusFile {
    let str_entries: Vec<(&str, &str)> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    write_sprint_status(artifacts_dir, &str_entries);
    let path = artifacts_dir.join("sprint-status.yaml");
    SprintStatusFile::load(&path, artifacts_dir).expect("load should succeed")
}

// ===========================================================================
// Task 4: All-done scenario (AC #3)
// ===========================================================================

/// AC #3: All stories done → NoEligibleStories error.
#[test]
fn test_watcher_poll_all_done_returns_no_eligible() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts = tmp.path().join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();

    write_sprint_status(
        &artifacts,
        &[
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "done"),
            ("1-2-cli-framework", "done"),
            ("epic-2", "in-progress"),
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
        "Expected NoEligibleStories, got: {err}"
    );
}

// ===========================================================================
// Task 5: Cyclic dependency (AC #4)
// ===========================================================================

/// AC #4: Circular dependencies → CyclicDependency error.
/// We manually inject circular deps since derive_dependencies() only produces linear chains.
#[test]
fn test_dependency_graph_detects_cycle() {
    // Create stories with manually injected circular dependencies
    let story_a = make_test_story("1-1-foo", "Foo", vec!["1-2-bar".to_string()]);
    let story_b = make_test_story("1-2-bar", "Bar", vec!["1-1-foo".to_string()]);

    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-foo".into(), "ready-for-dev".into()),
        ("1-2-bar".into(), "ready-for-dev".into()),
    ];

    let graph = DependencyGraph::new(&[story_a.clone(), story_b.clone()], &all_statuses);
    let result = graph.topological_sort();

    assert!(result.is_err(), "Should detect cycle");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::CyclicDependency { ref cycle } if !cycle.is_empty()),
        "Expected CyclicDependency with story keys, got: {err}"
    );
}

/// AC #4: Verify cycle error contains the offending keys.
#[test]
fn test_dependency_graph_cycle_contains_keys() {
    let story_a = make_test_story("1-1-alpha", "Alpha", vec!["1-2-beta".to_string()]);
    let story_b = make_test_story("1-2-beta", "Beta", vec!["1-1-alpha".to_string()]);

    let all_statuses: Vec<(String, String)> = vec![
        ("1-1-alpha".into(), "ready-for-dev".into()),
        ("1-2-beta".into(), "ready-for-dev".into()),
    ];

    let graph = DependencyGraph::new(&[story_a, story_b], &all_statuses);
    let err = graph.topological_sort().unwrap_err();

    match err {
        WatcherError::CyclicDependency { cycle } => {
            assert!(
                cycle.contains(&"1-1-alpha".to_string())
                    || cycle.contains(&"1-2-beta".to_string()),
                "Cycle should contain at least one of the involved keys: {cycle:?}"
            );
        }
        other => panic!("Expected CyclicDependency, got: {other}"),
    }
}

// ===========================================================================
// Task 6: Missing file (AC #5)
// ===========================================================================

/// AC #5: Missing sprint-status.yaml → SprintStatusNotFound error (not a panic).
#[test]
fn test_watcher_poll_missing_file_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    // Don't write any sprint-status.yaml — just a bare temp dir

    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let result = watcher.poll();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusNotFound { ref path } if path.contains("sprint-status.yaml")),
        "Expected SprintStatusNotFound with path containing 'sprint-status.yaml', got: {err}"
    );
}

/// AC #5: Verify error message includes the expected path.
#[test]
fn test_watcher_poll_missing_file_error_contains_path() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    let watcher = Watcher::new(Arc::new(config));
    let err = watcher.poll().unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("sprint-status.yaml"),
        "Error message should contain file path: {msg}"
    );
}

// ===========================================================================
// Task 7: SprintStatusFile integration tests (supplementary)
// ===========================================================================

/// Task 7.1: Load valid YAML → correct story count and order preservation.
#[test]
fn test_sprint_status_load_valid_yaml_preserves_order() {
    let tmp = tempfile::tempdir().unwrap();

    write_sprint_status(
        tmp.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "done"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("1-3-init-command", "ready-for-dev"),
            ("epic-1-retrospective", "optional"),
            ("epic-2", "in-progress"),
            ("2-1-polling", "ready-for-dev"),
        ],
    );

    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).expect("load should succeed");

    // All 7 entries loaded (including epics and retros)
    assert_eq!(ssf.entry_count(), 7);

    // stories() filters out epics and retrospectives
    let stories = ssf.stories();
    assert_eq!(stories.len(), 4); // 1-1, 1-2, 1-3, 2-1

    // Order preserved from YAML
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[1].story_key, "1-2-cli-framework");
    assert_eq!(stories[2].story_key, "1-3-init-command");
    assert_eq!(stories[3].story_key, "2-1-polling");
}

/// Task 7.2: stories() filters out epic and retrospective entries.
#[test]
fn test_sprint_status_stories_filters_epics_and_retros() {
    let tmp = tempfile::tempdir().unwrap();

    write_sprint_status(
        tmp.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "done"),
            ("epic-1-retrospective", "optional"),
            ("epic-2", "backlog"),
            ("2-1-polling", "ready-for-dev"),
            ("epic-2-retrospective", "optional"),
        ],
    );

    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();

    let stories = ssf.stories();
    let keys: Vec<&str> = stories.iter().map(|s| s.story_key.as_str()).collect();

    assert_eq!(keys, vec!["1-1-scaffolding", "2-1-polling"]);
    // No epic-*, no *-retrospective entries
    assert!(keys.iter().all(|k| !k.starts_with("epic-")));
    assert!(keys.iter().all(|k| !k.ends_with("-retrospective")));
}

/// Task 7.3: eligible_stories() returns only ready-for-dev stories.
#[test]
fn test_sprint_status_eligible_stories_only_ready_for_dev() {
    let tmp = tempfile::tempdir().unwrap();

    write_sprint_status(
        tmp.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-scaffolding", "done"),
            ("1-2-cli-framework", "ready-for-dev"),
            ("1-3-init-command", "in-progress"),
            ("1-4-status", "backlog"),
            ("1-5-git-remote", "review"),
            ("1-6-oauth", "ready-for-dev"),
        ],
    );

    let path = tmp.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();

    let eligible = ssf.eligible_stories();
    let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

    assert_eq!(keys, vec!["1-2-cli-framework", "1-6-oauth"]);
}

/// Task 7.4: Malformed YAML → SprintStatusParse error.
#[test]
fn test_sprint_status_load_malformed_yaml() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sprint-status.yaml");
    std::fs::write(&path, "{{{{invalid yaml: [[[").unwrap();

    let result = SprintStatusFile::load(&path, tmp.path());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, WatcherError::SprintStatusParse(_)),
        "Expected SprintStatusParse, got: {err}"
    );
}
