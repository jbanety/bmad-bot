# Story 2.2: Dependency Resolution & Execution Order

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer with interdependent stories,
I want the daemon to resolve dependencies and determine the correct execution order,
So that stories are processed in a sequence that respects their prerequisites.

## Acceptance Criteria

1. **Given** the watcher has detected multiple `ready-for-dev` stories **When** the dependency resolution module (`deps.rs`) processes them **Then** a directed acyclic graph of dependencies is computed in-memory **And** stories are returned in topological order (prerequisites first)

2. **Given** a story has dependencies that are not yet in `done` status **When** the pre-gate logic evaluates it **Then** the story is skipped for this cycle (not marked, not modified — pure read) **And** a tracing info message logs which story was skipped and which dependency is unmet

3. **Given** a story has all dependencies in `done` status **When** the pre-gate logic evaluates it **Then** the story is marked as eligible and included in the execution queue

4. **Given** a circular dependency exists in sprint-status.yaml **When** the dependency graph is computed **Then** a `WatcherError::CyclicDependency` error is returned with the cycle path **And** the error is logged and the affected stories are skipped

## Tasks / Subtasks

- [ ] Task 0: Verify prerequisites from Story 2.1 (AC: #1, #2, #3, #4)
  - [ ] 0.1 Verify `src/watcher/mod.rs` contains `WatcherError`, `StoryInfo`, `SprintStatusFile`, `Watcher` (from Story 2.1)
  - [ ] 0.2 Verify `StoryInfo` has `dependencies: Vec<String>` field (added in Story 2.1 validation)
  - [ ] 0.3 Verify `src/watcher/deps.rs` exists as a stub with `pub mod deps;` in `mod.rs`
  - [ ] 0.4 Run `cargo check` to confirm clean baseline

- [ ] Task 1: Add `CyclicDependency` variant to `WatcherError` in `src/watcher/mod.rs` (AC: #4)
  - [ ] 1.1 Add `CyclicDependency { cycle: Vec<String> }` variant to `WatcherError`
  - [ ] 1.2 Add `#[error("Cyclic dependency detected: {cycle:?}")]` error message
  - [ ] 1.3 Add `/// doc comment` explaining the variant

- [ ] Task 2: Implement `DependencyGraph` in `src/watcher/deps.rs` (AC: #1, #4)
  - [ ] 2.1 Create `pub struct DependencyGraph` with fields: `adjacency: HashMap<String, Vec<String>>` (story_key → dependencies), `all_statuses: HashMap<String, String>` (story_key → current status from sprint-status), `doc_order: HashMap<String, usize>` (story_key → position in sprint-status.yaml for deterministic ordering)
  - [ ] 2.2 Implement `DependencyGraph::new(stories: &[StoryInfo], all_statuses: &[(String, String)]) -> Self` — builds the graph from StoryInfo dependencies and full sprint status data; computes `doc_order` from the position of each story key in `all_statuses` slice
  - [ ] 2.3 Implement `DependencyGraph::topological_sort(&self) -> Result<Vec<String>, WatcherError>` — returns story keys in dependency order using Kahn's algorithm with **sprint-order tiebreaker** (when multiple nodes have in_degree 0, dequeue by `doc_order` ascending), returns `CyclicDependency` if cycle detected
  - [ ] 2.4 Derive `Debug`

- [ ] Task 3: Implement dependency derivation logic in `src/watcher/deps.rs` (AC: #1, #2, #3)
  - [ ] 3.1 Implement `pub fn derive_dependencies(stories: &mut [StoryInfo], all_statuses: &[(String, String)])` — populates the `dependencies` field on each StoryInfo based on intra-epic sequential ordering rule: story N.M depends on story N.(M-1). Reuse `StoryInfo::from_key_and_status()` for parsing keys (DRY — avoid duplicating parsing logic)
  - [ ] 3.2 Within an epic, story M depends on story M-1 being `done` (e.g., `2-2-*` depends on `2-1-*`)
  - [ ] 3.3 The first story in each epic (story_num == 1) has no intra-epic dependency
  - [ ] 3.4 Cross-epic dependencies are NOT enforced at pre-gate level — the BMAD agent handles those as a second layer (Architecture: two-layer dependency model)
  - [ ] 3.5 Populate `StoryInfo.dependencies` with the resolved dependency story keys

- [ ] Task 4: Implement pre-gate filtering in `src/watcher/deps.rs` (AC: #2, #3)
  - [ ] 4.1 Implement `pub fn filter_eligible(stories: Vec<StoryInfo>, all_statuses: &[(String, String)]) -> Result<Vec<StoryInfo>, WatcherError>` — the main entry point for the pre-gate
  - [ ] 4.2 Call `derive_dependencies()` to populate dependency fields
  - [ ] 4.3 Build `DependencyGraph` from the stories
  - [ ] 4.4 Run `topological_sort()` to detect cycles and get ordering
  - [ ] 4.5 For each story in topological order: check if ALL dependencies have status `done` in `all_statuses`
  - [ ] 4.6 If any dependency is NOT `done` → skip story, log at info level: `tracing::info!(story_key = %key, unmet_dep = %dep, dep_status = %status, "Story skipped — dependency not met")`
  - [ ] 4.7 If all dependencies are `done` → include in result vec
  - [ ] 4.8 Return filtered stories in topological order (prerequisites first)

- [ ] Task 5: Integrate pre-gate into `Watcher::poll()` in `src/watcher/mod.rs` (AC: #1, #2, #3, #4)
  - [ ] 5.1 After `SprintStatusFile::eligible_stories()` returns candidates, call `deps::filter_eligible()` with eligible stories and full entries from `SprintStatusFile`
  - [ ] 5.2 Expose `SprintStatusFile::entries()` as `pub fn entries(&self) -> &[(String, String)]` to provide all statuses to deps module
  - [ ] 5.3 On `Ok(filtered)` → return filtered stories (replaces raw eligible list)
  - [ ] 5.4 On `Err(WatcherError::CyclicDependency { .. })` → log error, return the error
  - [ ] 5.5 Log pre-gate summary: `tracing::info!(pre_gate_input = eligible.len(), pre_gate_output = filtered.len(), "Pre-gate dependency filter applied")`
  - [ ] 5.6 Make `make_test_bot_config` accessible from `deps.rs` tests: either mark it `pub(crate)` inside `#[cfg(test)] mod tests` in `watcher/mod.rs`, or duplicate the helper in `deps.rs` tests

- [ ] Task 6: Update `run_polling_loop()` in `src/cli/mod.rs` (AC: #4)
  - [ ] 6.1 Add match arm for `WatcherError::CyclicDependency { ref cycle }` → `tracing::error!(cycle = ?cycle, "Cyclic dependency detected — affected stories skipped")`
  - [ ] 6.2 Continue polling on next cycle (do not crash)

- [ ] Task 7: Write unit tests (AC: #1, #2, #3, #4)
  - [ ] 7.1 Test `derive_dependencies` correctly sets sequential intra-epic deps (2-2 depends on 2-1)
  - [ ] 7.2 Test `derive_dependencies` sets no deps for first story in epic (story_num == 1)
  - [ ] 7.3 Test `derive_dependencies` handles multiple epics independently
  - [ ] 7.4 Test `DependencyGraph::topological_sort` returns correct order for linear chain
  - [ ] 7.5 Test `DependencyGraph::topological_sort` returns `CyclicDependency` for circular deps
  - [ ] 7.6 Test `DependencyGraph::topological_sort` handles independent stories (no edges) and returns them in sprint-status document order
  - [ ] 7.7 Test `filter_eligible` returns only stories with all deps `done`
  - [ ] 7.8 Test `filter_eligible` skips story when dep is `ready-for-dev` (not done)
  - [ ] 7.9 Test `filter_eligible` skips story when dep is `in-progress`
  - [ ] 7.10 Test `filter_eligible` returns first story in epic even when nothing is done (no deps)
  - [ ] 7.11 Test `filter_eligible` returns empty vec when all stories have unmet deps
  - [ ] 7.12 Test `filter_eligible` preserves topological order with sprint-order tiebreaker in output
  - [ ] 7.14 Test `topological_sort` determinism: independent stories from different epics returned in document order
  - [ ] 7.13 Test full integration: `Watcher::poll()` with deps filtering active

- [ ] Task 8: Final quality checks
  - [ ] 8.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [ ] 8.2 Run `cargo clippy` and fix any warnings
  - [ ] 8.3 Run `cargo test` and verify all tests pass (including Story 2.1 tests)
  - [ ] 8.4 Verify all public items have `///` doc comments
  - [ ] 8.5 Manual integration test: create a sprint-status.yaml with stories where 1-1 is `done` and 1-2 is `ready-for-dev` → verify 1-2 is eligible
  - [ ] 8.6 Manual integration test: create sprint-status where 1-1 is `in-progress` and 1-2 is `ready-for-dev` → verify 1-2 is skipped

## Dev Notes

### Previous Story Intelligence

**Story 2.1** established:
- `StoryInfo` struct with fields: `story_id`, `story_key`, `epic_num`, `story_num`, `label`, `branch_name`, `specs_path`, `dependencies: Vec<String>`, `status`
- `StoryInfo::from_key_and_status()` — parses sprint-status keys, returns `None` for epics/retrospectives, initializes `dependencies` to `Vec::new()`
- `StoryInfo::is_eligible()` — returns `true` when status == `"ready-for-dev"`
- `SprintStatusFile` — loads and parses sprint-status.yaml, preserves document order via `serde_yml::Mapping`
- `SprintStatusFile::load(path, story_dir)` — TOCTOU-safe file read with error mapping (`NotFound` → `SprintStatusNotFound`)
- `SprintStatusFile::stories()` → all story entries as `Vec<StoryInfo>`
- `SprintStatusFile::eligible_stories()` → only `ready-for-dev` stories
- `SprintStatusFile::entry_count()` → total entries including epics/retros
- `WatcherError` enum: `SprintStatusNotFound`, `SprintStatusRead(std::io::Error)` (no `#[from]`), `SprintStatusParse(#[from] serde_yml::Error)`, `NoEligibleStories`
- `Watcher` struct with `config: Arc<BotConfig>`, `sprint_status_path: PathBuf`, `story_dir: PathBuf`
- `Watcher::poll()` → returns `Result<Vec<StoryInfo>, WatcherError>` (eligible stories)
- Integration in `run_polling_loop()`: match on `watcher.poll()` with arms for `Ok(stories)`, `NoEligibleStories` (info log), `SprintStatusNotFound` (warn), other errors (error)
- Test helper `make_test_bot_config(artifacts_dir)` creates minimal `BotConfig` for testing
- `tempfile` in `[dev-dependencies]` for temp dir tests
- `pub mod deps;` declared in `watcher/mod.rs`, `deps.rs` is a stub

**Stories 1.1–1.4** established:
- `BotConfig` with `bmad_paths: BmadPathsConfig` containing `implementation_artifacts` path
- `Arc<BotConfig>` sharing pattern across modules
- `run_polling_loop()` signature: `(config, watcher, daemon_state, state_path)`
- `CliError` variants for CLI-level error handling
- Per-module `thiserror` enum pattern (no `anyhow` in library modules)
- Tracing patterns: structured fields, never `println!`

### Dependency Model Design

**Two-layer dependency architecture** (from project-context and architecture):
1. **Layer 1 — Daemon pre-gate (THIS STORY):** Cheap deterministic dependency check on sprint-status data. No LLM involved. Pure graph resolution. Skips stories with unmet dependencies. Prevents token burn on impossible stories.
2. **Layer 2 — BMAD agent:** Full story selection and verification within its workflow. Both layers must agree before work proceeds.

**Dependency derivation rule — intra-epic sequential ordering:**
- Within an epic, story N.M depends on story N.(M-1) being `done`
- Example: `2-2-dependency-resolution` depends on `2-1-sprint-status-polling` being done
- The first story in each epic (story_num == 1) has NO intra-epic dependency
- Cross-epic dependencies are NOT enforced at the pre-gate — the BMAD agent handles them

**Why intra-epic only:**
- sprint-status.yaml contains no explicit dependency declarations
- Sequential ordering within epics is the natural development flow (SM creates stories in order)
- Cross-epic deps are complex and better handled by the BMAD agent with full context
- Architecture Decision 2: daemon is a pure reader — no modification of sprint data

**Dependency source — derived from sprint-status.yaml structure:**
```yaml
development_status:
  epic-1: in-progress
  1-1-scaffolding: done       # story_num=1 → no deps
  1-2-cli: ready-for-dev      # story_num=2 → depends on 1-1-*
  1-3-init: backlog            # story_num=3 → depends on 1-2-*
  epic-2: in-progress
  2-1-polling: ready-for-dev   # story_num=1 → no deps (first in epic 2)
  2-2-dependency: backlog      # story_num=2 → depends on 2-1-*
```

To find the dependency for story N.M: scan all entries for pattern `N-(M-1)-*` in sprint-status.yaml. The dependency is satisfied when that entry has status `done`.

### `DependencyGraph` Implementation — `src/watcher/deps.rs`

```rust
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::cmp::Reverse;
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

        let all_statuses: HashMap<String, String> = all_statuses
            .iter()
            .cloned()
            .collect();

        Self { adjacency, all_statuses, doc_order }
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
            .filter(|(_, &deg)| deg == 0)
            .map(|(&key, _)| {
                let pos = self.doc_order.get(key).copied().unwrap_or(usize::MAX);
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
            let in_cycle: Vec<String> = self.adjacency.keys()
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
                let status = self.all_statuses.get(dep).map(|s| s.as_str()).unwrap_or("unknown");
                if status != "done" {
                    return (false, Some((dep.clone(), status.to_string())));
                }
            }
        }
        (true, None)
    }
}
```

### `derive_dependencies` Implementation — `src/watcher/deps.rs`

```rust
use std::path::Path;

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
        if let Some(dep_key) = key_lookup.get(&predecessor_key) {
            if !story.dependencies.contains(dep_key) {
                story.dependencies.push(dep_key.clone());
            }
        }
    }
}
```

### `filter_eligible` Implementation — `src/watcher/deps.rs`

```rust
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
```

### Integration Changes to `Watcher::poll()` — `src/watcher/mod.rs`

The `poll()` method must be updated to pipe eligible stories through the pre-gate:

```rust
/// Updated poll() — now applies pre-gate dependency filtering.
pub fn poll(&self) -> Result<Vec<StoryInfo>, WatcherError> {
    tracing::debug!(
        path = %self.sprint_status_path.display(),
        "Polling sprint-status.yaml"
    );

    let sprint_status = SprintStatusFile::load(
        &self.sprint_status_path,
        &self.story_dir,
    )?;

    let all_stories = sprint_status.stories();
    let eligible = sprint_status.eligible_stories();

    tracing::info!(
        total_stories = all_stories.len(),
        eligible_count = eligible.len(),
        "Sprint status polled"
    );

    if eligible.is_empty() {
        return Err(WatcherError::NoEligibleStories);
    }

    // Pre-gate: dependency resolution and filtering (Story 2.2)
    let entries = sprint_status.entries();
    let filtered = deps::filter_eligible(eligible, entries)?;

    tracing::info!(
        pre_gate_input = all_stories.len(),
        pre_gate_output = filtered.len(),
        "Pre-gate dependency filter applied"
    );

    if filtered.is_empty() {
        return Err(WatcherError::NoEligibleStories);
    }

    for story in &filtered {
        tracing::info!(
            story_id = %story.story_id,
            story_key = %story.story_key,
            branch = %story.branch_name,
            deps = ?story.dependencies,
            "Eligible story detected (deps satisfied)"
        );
    }

    Ok(filtered)
}
```

### New `SprintStatusFile::entries()` Method

Add to `SprintStatusFile` impl in `src/watcher/mod.rs`:

```rust
/// Returns all entries as (key, status) pairs.
/// Used by the dependency resolution module to check status of
/// non-eligible stories (e.g., whether a dependency is `done`).
pub fn entries(&self) -> &[(String, String)] {
    &self.entries
}
```

### Update to `run_polling_loop()` — `src/cli/mod.rs`

Add a new match arm for `CyclicDependency`:

```rust
match watcher.poll() {
    Ok(stories) => {
        tracing::info!(
            eligible_count = stories.len(),
            "Found eligible stories — session launching not yet implemented (Epic 4)"
        );
        // TODO: Epic 4 — Launch dev session for first eligible story
    }
    Err(crate::watcher::WatcherError::NoEligibleStories) => {
        tracing::info!("No eligible stories in this cycle — waiting for next poll");
    }
    Err(crate::watcher::WatcherError::SprintStatusNotFound { ref path }) => {
        tracing::warn!(
            path = %path,
            "Sprint status file not found — has sprint-planning been run?"
        );
    }
    Err(crate::watcher::WatcherError::CyclicDependency { ref cycle }) => {
        tracing::error!(
            cycle = ?cycle,
            "Cyclic dependency detected in sprint-status — affected stories skipped, will retry next cycle"
        );
    }
    Err(e) => {
        tracing::error!(
            error = %e,
            "Failed to poll sprint status — will retry next cycle"
        );
    }
}
```

### Files Modified/Created in This Story

| File | Change |
|------|--------|
| `src/watcher/deps.rs` | **REPLACE STUB** — Full implementation: `DependencyGraph`, `derive_dependencies()`, `filter_eligible()`, unit tests |
| `src/watcher/mod.rs` | Add `CyclicDependency` variant to `WatcherError`. Add `SprintStatusFile::entries()`. Update `Watcher::poll()` to call `deps::filter_eligible()` |
| `src/cli/mod.rs` | Add `CyclicDependency` match arm in `run_polling_loop()` |

### Complete `src/watcher/deps.rs` Module-Level Doc Comment

```rust
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
```

### Anti-Patterns to Avoid

- ❌ **NO** writing to sprint-status.yaml — daemon is a PURE READER
- ❌ **NO** cross-epic dependency enforcement — BMAD agent handles those (two-layer model)
- ❌ **NO** `unwrap()` or `expect()` in production code — use `?` with `WatcherError`
- ❌ **NO** `anyhow::Result` in `deps.rs` — typed `WatcherError` only
- ❌ **NO** modifying `StoryInfo::from_key_and_status()` — dependencies are derived separately
- ❌ **NO** reading story .md files for dependency info — use sprint-status.yaml structure only
- ❌ **NO** implementing cascade blocking — that's Story 2.3
- ❌ **NO** implementing session launching — that's Epic 4
- ❌ **NO** modifying modules other than `watcher/mod.rs`, `watcher/deps.rs`, and `cli/mod.rs`
- ❌ **NO** complex dependency config — keep it simple: intra-epic sequential only

### Scope Boundaries

**IN SCOPE for this story:**
- `src/watcher/deps.rs` — `DependencyGraph`, `derive_dependencies`, `filter_eligible`
- `src/watcher/mod.rs` — `CyclicDependency` error variant, `entries()` accessor, updated `poll()`
- `src/cli/mod.rs` — `CyclicDependency` match arm in polling loop

**OUT OF SCOPE — do NOT implement:**
- Cascade blocking of dependent stories (Story 2.3)
- Cross-epic dependency resolution (BMAD agent handles this)
- Explicit dependency declarations in sprint-status.yaml (future enhancement)
- Session launching for eligible stories (Epic 4)
- Writing to sprint-status.yaml (Architecture Decision 2)
- Reading individual story .md files for dependency metadata

### Testing Requirements

All tests go inline at the bottom of `src/watcher/deps.rs` in `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::StoryInfo;
    use std::path::Path;

    /// Helper: create a StoryInfo with given key, status, and optional deps
    fn make_story(key: &str, status: &str) -> StoryInfo {
        StoryInfo::from_key_and_status(key, status, Path::new("/tmp/artifacts"))
            .expect(&format!("Should parse story key: {key}"))
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
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "ready-for-dev".to_string()),
        ];
        let mut stories = vec![make_story("1-1-scaffolding", "ready-for-dev")];

        derive_dependencies(&mut stories, &all_statuses);

        assert!(stories[0].dependencies.is_empty(), "First story should have no deps");
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
        assert_eq!(sorted.len(), 3, "All independent stories should be in output");
        // Sprint-order tiebreaker: must match document order
        assert_eq!(sorted[0], "1-1-a");
        assert_eq!(sorted[1], "2-1-b");
        assert_eq!(sorted[2], "3-1-c");
    }

    #[test]
    fn test_topo_sort_determinism_reverse_document_order() {
        // Stories appear in reverse document order in all_statuses
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
        assert!(result.is_empty(), "1-2 should be skipped because 1-1 is in-progress, not done");
    }

    #[test]
    fn test_filter_eligible_first_story_always_eligible() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![make_story("1-1-scaffolding", "ready-for-dev")];

        let result = filter_eligible(stories, &all_statuses).unwrap();
        assert_eq!(result.len(), 1, "First story in epic has no deps, always eligible");
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

        let config = std::sync::Arc::new(crate::watcher::tests::make_test_bot_config(artifacts_dir));
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
```

> **NOTE on `make_test_bot_config`:** The integration test references `crate::watcher::tests::make_test_bot_config` from Story 2.1. This function is in `#[cfg(test)] mod tests` of `watcher/mod.rs`. To make it accessible from `deps.rs` tests, change `mod tests` to `pub(crate) mod tests` in `watcher/mod.rs` and mark `make_test_bot_config` as `pub(crate)`. This is covered in Task 5.6.

### Project Structure Notes

After this story, the watcher module is feature-complete for the pre-gate:

```
src/watcher/
├── mod.rs      # WatcherError (+ CyclicDependency), StoryInfo, SprintStatusFile (+ entries()), Watcher (updated poll())
└── deps.rs     # DependencyGraph, derive_dependencies(), filter_eligible(), tests
```

The `watcher → session` interface is now dependency-aware:
- `Watcher::poll()` returns `Vec<StoryInfo>` where each story has populated `dependencies` and all deps are verified `done`
- Session module (Epic 4) can trust that any story received from `poll()` is safe to execute

### References

- [Source: epics.md § Story 2.2: Dependency Resolution & Execution Order] — User story, acceptance criteria
- [Source: epics.md § Epic 2: Story Watching & Dependency Management] — Epic context, daemon as pure reader
- [Source: prd.md § FR2] — Resolve story dependencies and determine correct execution order
- [Source: prd.md § FR3] — Skip stories whose dependencies are not yet completed
- [Source: architecture.md § Decision 2: Sprint-Status Mutation] — Daemon is pure reader, agent writes
- [Source: architecture.md § Error Type Pattern] — Per-module thiserror enums, CyclicDependency variant
- [Source: architecture.md § Architectural Boundaries] — watcher → session: passes StoryInfo with dependencies
- [Source: architecture.md § Data Flow] — Step 3: watcher reads sprint-status, deps computes pre-gate
- [Source: project-context.md § Daemon Lifecycle] — Pre-gate: deterministic dependency check, no LLM involved
- [Source: project-context.md § Two-Layer Dependency Model] — Daemon pre-gate + BMAD agent verification
- [Source: project-context.md § Sequential Execution] — One story at a time, in sprint order
- [Source: project-context.md § Testing Rules] — Inline tests, descriptive snake_case, mocked data
- [Source: Story 2.1] — StoryInfo with dependencies field, WatcherError, SprintStatusFile, Watcher::poll(), tempfile in dev-deps

## Dev Agent Record

<!-- This section is filled automatically by the dev agent post-implementation. Do not edit manually. -->

### Agent Model Used

_(filled post-implementation)_

### Debug Log References

### Completion Notes List

### File List