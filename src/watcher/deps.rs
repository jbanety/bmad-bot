//! Dependency resolution and pre-gate filtering for the watcher.
//!
//! This module implements the daemon's pre-gate: a cheap, deterministic
//! dependency check that prevents token burn on stories that cannot proceed.
//!
//! **Dependency rule:** Within an epic, stories are sequential — story N.M
//! depends on story N.(M-1) being `done`. Cross-epic dependencies are NOT
//! enforced here (handled by the BMAD agent as a second layer).
//!
//! **Architecture Decision 2:** The daemon is a pure reader. All dependency
//! logic is computed in-memory per poll cycle. Nothing is written to disk.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::path::Path;

use super::{StoryInfo, WatcherError};

/// Resolves story dependencies and computes execution order.
///
/// The dependency graph is a directed acyclic graph (DAG) where edges
/// represent "depends on" relationships. Topological sort determines
/// the safe execution order, using sprint-status document position as
/// a tiebreaker for deterministic output.
///
/// **Architecture:** This is the daemon's pre-gate — a cheap, deterministic
/// dependency check that prevents token burn on stories that can't proceed.
#[derive(Debug)]
pub struct DependencyGraph {
    /// story_key → list of story_keys this story depends on
    adjacency: HashMap<String, Vec<String>>,
    /// story_key → current status from sprint-status.yaml (all entries, not just eligible)
    all_statuses: HashMap<String, String>,
    /// story_key → position index in sprint-status.yaml (for deterministic ordering).
    /// When multiple stories have equal precedence in topological sort,
    /// they are ordered by their document position (sprint order).
    doc_order: HashMap<String, usize>,
}

impl DependencyGraph {
    /// Build a dependency graph from stories and their full status context.
    ///
    /// # Arguments
    /// * `stories` — Stories to include in the graph (typically eligible stories)
    /// * `all_statuses` — Complete (key, status) pairs from sprint-status.yaml
    ///   (needed to check if dependencies are `done` and to derive document order)
    pub fn new(stories: &[StoryInfo], all_statuses: &[(String, String)]) -> Self {
        let adjacency: HashMap<String, Vec<String>> = stories
            .iter()
            .map(|s| (s.story_key.clone(), s.dependencies.clone()))
            .collect();

        // Derive document order from position in all_statuses slice
        // (preserves sprint-status.yaml ordering)
        let doc_order: HashMap<String, usize> = all_statuses
            .iter()
            .enumerate()
            .map(|(i, (key, _))| (key.clone(), i))
            .collect();

        let all_statuses: HashMap<String, String> = all_statuses.iter().cloned().collect();

        Self {
            adjacency,
            all_statuses,
            doc_order,
        }
    }

    /// Topological sort via Kahn's algorithm with sprint-order tiebreaker.
    ///
    /// Returns story keys in dependency-safe order (prerequisites first).
    /// When multiple stories have in_degree 0 simultaneously, they are
    /// ordered by their position in sprint-status.yaml (document order).
    /// This guarantees deterministic output matching "sprint order."
    ///
    /// Returns `WatcherError::CyclicDependency` if a cycle is detected.
    pub fn topological_sort(&self) -> Result<Vec<String>, WatcherError> {
        // Build in-degree map (only for nodes in the graph)
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();

        // Initialize all nodes
        for key in self.adjacency.keys() {
            in_degree.entry(key.as_str()).or_insert(0);
            graph.entry(key.as_str()).or_default();
        }

        // Build edges: if A depends on B, then B → A (B must come before A)
        // Only count edges where the dependency is also in the graph
        for (key, deps) in &self.adjacency {
            for dep in deps {
                if self.adjacency.contains_key(dep) {
                    graph.entry(dep.as_str()).or_default().push(key.as_str());
                    *in_degree.entry(key.as_str()).or_insert(0) += 1;
                }
            }
        }

        // Kahn's algorithm with sprint-order tiebreaker:
        // Use a min-heap keyed by document position so that when multiple
        // nodes have in_degree 0, the one appearing first in sprint-status
        // is dequeued first.
        let mut heap: BinaryHeap<Reverse<(usize, String)>> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(key, _)| {
                let pos = self.doc_order.get(*key).copied().unwrap_or(usize::MAX);
                Reverse((pos, key.to_string()))
            })
            .collect();

        let mut sorted: Vec<String> = Vec::new();

        while let Some(Reverse((_, node))) = heap.pop() {
            sorted.push(node.clone());
            if let Some(dependents) = graph.get(node.as_str()) {
                for &dependent in dependents {
                    if let Some(deg) = in_degree.get_mut(dependent) {
                        *deg -= 1;
                        if *deg == 0 {
                            let pos = self.doc_order.get(dependent).copied().unwrap_or(usize::MAX);
                            heap.push(Reverse((pos, dependent.to_string())));
                        }
                    }
                }
            }
        }

        // If sorted doesn't include all nodes, there's a cycle
        if sorted.len() != self.adjacency.len() {
            let in_cycle: Vec<String> = self
                .adjacency
                .keys()
                .filter(|k| !sorted.contains(k))
                .cloned()
                .collect();
            return Err(WatcherError::CyclicDependency { cycle: in_cycle });
        }

        Ok(sorted)
    }

    /// Check if all dependencies of a story are satisfied (status == "done").
    ///
    /// A dependency is satisfied when its status in sprint-status.yaml is `done`.
    /// Dependencies not found in all_statuses are treated as unmet.
    pub fn deps_satisfied(&self, story_key: &str) -> (bool, Option<(String, String)>) {
        if let Some(deps) = self.adjacency.get(story_key) {
            for dep in deps {
                let status = self
                    .all_statuses
                    .get(dep)
                    .map(|s| s.as_str())
                    .unwrap_or("unknown");
                if status != "done" {
                    return (false, Some((dep.clone(), status.to_string())));
                }
            }
        }
        (true, None)
    }
}

/// Derive intra-epic sequential dependencies for a set of stories.
///
/// **Rule:** Within an epic, story N.M depends on story N.(M-1).
/// The first story in each epic (story_num == 1) has no dependency.
/// Cross-epic dependencies are NOT enforced at the pre-gate level.
///
/// # Arguments
/// * `stories` — Mutable slice of stories; their `dependencies` field will be populated
/// * `all_statuses` — Complete (key, status) pairs from sprint-status.yaml
///   (needed to find the predecessor story key)
pub fn derive_dependencies(stories: &mut [StoryInfo], all_statuses: &[(String, String)]) {
    // Build a lookup: (epic_num, story_num) → story_key for ALL entries in sprint-status.
    // Reuses StoryInfo::from_key_and_status() to parse keys (DRY — single source of
    // truth for what constitutes a valid story key vs epic/retrospective entry).
    let dummy_dir = Path::new("");
    let key_lookup: HashMap<(u32, u32), String> = all_statuses
        .iter()
        .filter_map(|(key, status)| {
            let info = StoryInfo::from_key_and_status(key, status, dummy_dir)?;
            Some(((info.epic_num, info.story_num), key.clone()))
        })
        .collect();

    for story in stories.iter_mut() {
        // First story in epic → no dependency
        if story.story_num <= 1 {
            continue;
        }

        // Look up predecessor: same epic, story_num - 1
        let predecessor_key = (story.epic_num, story.story_num - 1);
        if let Some(dep_key) = key_lookup.get(&predecessor_key)
            && !story.dependencies.contains(dep_key)
        {
            story.dependencies.push(dep_key.clone());
        }
    }
}

/// Pre-gate filter: resolve dependencies and return only eligible stories in order.
///
/// This is the main entry point for the dependency pre-gate.
/// It derives dependencies, builds the graph, checks for cycles,
/// and filters out stories with unmet dependencies.
///
/// # Arguments
/// * `stories` — Eligible stories from the watcher (status == `ready-for-dev`)
/// * `all_statuses` — Complete (key, status) pairs from sprint-status.yaml
///
/// # Returns
/// Filtered stories in topological order, or `WatcherError::CyclicDependency`.
pub fn filter_eligible(
    mut stories: Vec<StoryInfo>,
    all_statuses: &[(String, String)],
) -> Result<Vec<StoryInfo>, WatcherError> {
    if stories.is_empty() {
        return Ok(stories);
    }

    // Step 1: Derive dependencies from sprint-status ordering
    derive_dependencies(&mut stories, all_statuses);

    // Step 2: Build dependency graph
    let graph = DependencyGraph::new(&stories, all_statuses);

    // Step 3: Topological sort (detects cycles)
    let sorted_keys = graph.topological_sort()?;

    // Step 4: Filter — only include stories with all deps satisfied
    let story_map: HashMap<String, StoryInfo> = stories
        .into_iter()
        .map(|s| (s.story_key.clone(), s))
        .collect();

    let mut eligible: Vec<StoryInfo> = Vec::new();
    for key in &sorted_keys {
        let (satisfied, unmet) = graph.deps_satisfied(key);
        if satisfied {
            if let Some(story) = story_map.get(key) {
                eligible.push(story.clone());
            }
        } else if let Some((dep_key, dep_status)) = unmet {
            tracing::info!(
                story_key = %key,
                unmet_dep = %dep_key,
                dep_status = %dep_status,
                "Story skipped — dependency not met"
            );
        }
    }

    Ok(eligible)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::StoryInfo;
    use std::path::Path;

    /// Helper: create a StoryInfo with given key, status, and optional deps
    fn make_story(key: &str, status: &str) -> StoryInfo {
        StoryInfo::from_key_and_status(key, status, Path::new("/tmp/artifacts"))
            .unwrap_or_else(|| panic!("Should parse story key: {key}"))
    }

    // --- derive_dependencies tests ---

    #[test]
    fn test_derive_deps_sequential_intra_epic() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "done".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
            ("1-3-init".to_string(), "ready-for-dev".to_string()),
        ];
        let mut stories = vec![
            make_story("1-2-cli", "ready-for-dev"),
            make_story("1-3-init", "ready-for-dev"),
        ];

        derive_dependencies(&mut stories, &all_statuses);

        assert_eq!(stories[0].dependencies, vec!["1-1-scaffolding"]);
        assert_eq!(stories[1].dependencies, vec!["1-2-cli"]);
    }

    #[test]
    fn test_derive_deps_first_story_no_deps() {
        let all_statuses = vec![("1-1-scaffolding".to_string(), "ready-for-dev".to_string())];
        let mut stories = vec![make_story("1-1-scaffolding", "ready-for-dev")];

        derive_dependencies(&mut stories, &all_statuses);

        assert!(
            stories[0].dependencies.is_empty(),
            "First story should have no deps"
        );
    }

    #[test]
    fn test_derive_deps_multiple_epics_independent() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "done".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
            ("2-1-polling".to_string(), "ready-for-dev".to_string()),
            ("2-2-deps".to_string(), "ready-for-dev".to_string()),
        ];
        let mut stories = vec![
            make_story("1-2-cli", "ready-for-dev"),
            make_story("2-1-polling", "ready-for-dev"),
            make_story("2-2-deps", "ready-for-dev"),
        ];

        derive_dependencies(&mut stories, &all_statuses);

        // 1-2 depends on 1-1 (same epic)
        assert_eq!(stories[0].dependencies, vec!["1-1-scaffolding"]);
        // 2-1 has no deps (first in epic 2)
        assert!(stories[1].dependencies.is_empty());
        // 2-2 depends on 2-1 (same epic)
        assert_eq!(stories[2].dependencies, vec!["2-1-polling"]);
    }

    // --- DependencyGraph::topological_sort tests ---

    #[test]
    fn test_topo_sort_linear_chain() {
        let all_statuses = vec![
            ("1-1-a".to_string(), "done".to_string()),
            ("1-2-b".to_string(), "ready-for-dev".to_string()),
            ("1-3-c".to_string(), "ready-for-dev".to_string()),
        ];
        let mut stories = vec![
            make_story("1-2-b", "ready-for-dev"),
            make_story("1-3-c", "ready-for-dev"),
        ];
        derive_dependencies(&mut stories, &all_statuses);
        let graph = DependencyGraph::new(&stories, &all_statuses);

        let sorted = graph.topological_sort().unwrap();
        // 1-2-b must come before 1-3-c (1-3 depends on 1-2)
        let pos_b = sorted.iter().position(|k| k == "1-2-b").unwrap();
        let pos_c = sorted.iter().position(|k| k == "1-3-c").unwrap();
        assert!(pos_b < pos_c, "1-2-b must come before 1-3-c");
    }

    #[test]
    fn test_topo_sort_detects_cycle() {
        // Manually create a cycle (not possible with derive_dependencies, but test the graph directly)
        let mut story_a = make_story("1-1-a", "ready-for-dev");
        let mut story_b = make_story("1-2-b", "ready-for-dev");
        story_a.dependencies = vec!["1-2-b".to_string()];
        story_b.dependencies = vec!["1-1-a".to_string()];

        let all_statuses = vec![
            ("1-1-a".to_string(), "ready-for-dev".to_string()),
            ("1-2-b".to_string(), "ready-for-dev".to_string()),
        ];
        let graph = DependencyGraph::new(&[story_a, story_b], &all_statuses);

        let result = graph.topological_sort();
        assert!(result.is_err());
        match result.unwrap_err() {
            WatcherError::CyclicDependency { cycle } => {
                assert!(cycle.contains(&"1-1-a".to_string()));
                assert!(cycle.contains(&"1-2-b".to_string()));
            }
            other => panic!("Expected CyclicDependency, got: {other:?}"),
        }
    }

    #[test]
    fn test_topo_sort_independent_stories_in_document_order() {
        let stories = vec![
            make_story("1-1-a", "ready-for-dev"),
            make_story("2-1-b", "ready-for-dev"),
            make_story("3-1-c", "ready-for-dev"),
        ];
        // Document order: 1-1-a at position 0, 2-1-b at position 1, 3-1-c at position 2
        let all_statuses = vec![
            ("1-1-a".to_string(), "ready-for-dev".to_string()),
            ("2-1-b".to_string(), "ready-for-dev".to_string()),
            ("3-1-c".to_string(), "ready-for-dev".to_string()),
        ];
        let graph = DependencyGraph::new(&stories, &all_statuses);

        let sorted = graph.topological_sort().unwrap();
        assert_eq!(
            sorted.len(),
            3,
            "All independent stories should be in output"
        );
        // Sprint-order tiebreaker: must match document order
        assert_eq!(sorted[0], "1-1-a");
        assert_eq!(sorted[1], "2-1-b");
        assert_eq!(sorted[2], "3-1-c");
    }

    #[test]
    fn test_topo_sort_determinism_reverse_document_order() {
        // Stories appear in reverse document order in input — output should still be document order
        let stories = vec![
            make_story("3-1-c", "ready-for-dev"),
            make_story("2-1-b", "ready-for-dev"),
            make_story("1-1-a", "ready-for-dev"),
        ];
        let all_statuses = vec![
            ("1-1-a".to_string(), "ready-for-dev".to_string()),
            ("2-1-b".to_string(), "ready-for-dev".to_string()),
            ("3-1-c".to_string(), "ready-for-dev".to_string()),
        ];
        let graph = DependencyGraph::new(&stories, &all_statuses);

        let sorted = graph.topological_sort().unwrap();
        // Regardless of input story order, output follows document order
        assert_eq!(sorted[0], "1-1-a");
        assert_eq!(sorted[1], "2-1-b");
        assert_eq!(sorted[2], "3-1-c");
    }

    // --- filter_eligible tests ---

    #[test]
    fn test_filter_eligible_returns_stories_with_deps_done() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "done".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![make_story("1-2-cli", "ready-for-dev")];

        let result = filter_eligible(stories, &all_statuses).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].story_key, "1-2-cli");
    }

    #[test]
    fn test_filter_eligible_skips_story_with_dep_ready_for_dev() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "ready-for-dev".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![
            make_story("1-1-scaffolding", "ready-for-dev"),
            make_story("1-2-cli", "ready-for-dev"),
        ];

        let result = filter_eligible(stories, &all_statuses).unwrap();
        // Only 1-1 should be eligible (no deps), 1-2 skipped (dep 1-1 not done)
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].story_key, "1-1-scaffolding");
    }

    #[test]
    fn test_filter_eligible_skips_story_with_dep_in_progress() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "in-progress".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![make_story("1-2-cli", "ready-for-dev")];

        let result = filter_eligible(stories, &all_statuses).unwrap();
        assert!(
            result.is_empty(),
            "1-2 should be skipped because 1-1 is in-progress, not done"
        );
    }

    #[test]
    fn test_filter_eligible_first_story_always_eligible() {
        let all_statuses = vec![("1-1-scaffolding".to_string(), "ready-for-dev".to_string())];
        let stories = vec![make_story("1-1-scaffolding", "ready-for-dev")];

        let result = filter_eligible(stories, &all_statuses).unwrap();
        assert_eq!(
            result.len(),
            1,
            "First story in epic has no deps, always eligible"
        );
    }

    #[test]
    fn test_filter_eligible_empty_when_all_deps_unmet() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "in-progress".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
            ("1-3-init".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![
            make_story("1-2-cli", "ready-for-dev"),
            make_story("1-3-init", "ready-for-dev"),
        ];

        let result = filter_eligible(stories, &all_statuses).unwrap();
        assert!(result.is_empty(), "All stories have unmet deps");
    }

    #[test]
    fn test_filter_eligible_preserves_topological_order() {
        let all_statuses = vec![
            ("1-1-a".to_string(), "done".to_string()),
            ("1-2-b".to_string(), "done".to_string()),
            ("1-3-c".to_string(), "ready-for-dev".to_string()),
            ("2-1-x".to_string(), "ready-for-dev".to_string()),
        ];
        // Pass stories in reverse order — output should be topological, not input order
        let stories = vec![
            make_story("1-3-c", "ready-for-dev"),
            make_story("2-1-x", "ready-for-dev"),
        ];

        let result = filter_eligible(stories, &all_statuses).unwrap();
        assert_eq!(result.len(), 2);
        // Both should be eligible: 1-3 (deps 1-2 done) and 2-1 (no deps)
        let keys: Vec<&str> = result.iter().map(|s| s.story_key.as_str()).collect();
        assert!(keys.contains(&"1-3-c"));
        assert!(keys.contains(&"2-1-x"));
    }

    #[test]
    fn test_filter_eligible_empty_input_returns_empty() {
        let result = filter_eligible(vec![], &[]).unwrap();
        assert!(result.is_empty());
    }

    // --- Integration test with Watcher::poll (requires watcher test infra) ---

    #[test]
    fn test_watcher_poll_with_deps_filtering() {
        use std::fs;
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path();
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: ready-for-dev
  1-3-init: ready-for-dev
"#;
        fs::write(artifacts_dir.join("sprint-status.yaml"), content).unwrap();

        let config =
            std::sync::Arc::new(crate::watcher::tests::make_test_bot_config(artifacts_dir));
        let watcher = crate::watcher::Watcher::new(config);
        let result = watcher.poll();
        assert!(result.is_ok());
        let stories = result.unwrap();

        // 1-1 is done → not eligible (not ready-for-dev)
        // 1-2 is ready-for-dev AND dep 1-1 is done → eligible
        // 1-3 is ready-for-dev BUT dep 1-2 is NOT done → skipped
        assert_eq!(stories.len(), 1);
        assert_eq!(stories[0].story_key, "1-2-cli");
    }
}
