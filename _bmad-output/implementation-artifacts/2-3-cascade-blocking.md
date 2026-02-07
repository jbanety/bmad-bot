# Story 2.3: Cascade Blocking

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want dependent stories to be automatically identified as blocked when a prerequisite story fails,
So that the daemon doesn't waste time attempting stories that cannot succeed.

## Acceptance Criteria

1. **Given** a story has been processed and resulted in a `blocked` or `needs-clarification` status **When** the pre-gate logic runs on the next polling cycle **Then** all stories that depend (directly or transitively) on the failed story are identified as ineligible **And** a tracing warn message logs each cascade-blocked story with the reason (which prerequisite failed)

2. **Given** the blocking prerequisite story is later resolved (status changes to `done`) **When** the next polling cycle runs **Then** the previously cascade-blocked dependents are re-evaluated based on current statuses **And** stories whose dependencies are now all `done` become eligible again

3. **Given** the daemon has identified cascade-blocked stories **When** the pre-gate completes **Then** only truly eligible stories (all dependencies met, status `ready-for-dev`) are passed to the session module **And** the daemon never writes to `sprint-status.yaml` — all blocking logic is computed in-memory per cycle

## Tasks / Subtasks

- [ ] Task 0: Verify prerequisites from Story 2.2 (AC: #1, #2, #3)
  - [ ] 0.1 Verify `src/watcher/deps.rs` contains `DependencyGraph`, `derive_dependencies()`, `filter_eligible()` (from Story 2.2)
  - [ ] 0.2 Verify `DependencyGraph` has `adjacency`, `all_statuses`, `doc_order` fields
  - [ ] 0.3 Verify `DependencyGraph::deps_satisfied()` returns `(bool, Option<(String, String)>)` — the unmet dep key and its status
  - [ ] 0.4 Verify `WatcherError::CyclicDependency` variant exists (from Story 2.2)
  - [ ] 0.5 Run `cargo check` to confirm clean baseline

- [ ] Task 1: Define cascade-blocking status constants in `src/watcher/deps.rs` (AC: #1)
  - [ ] 1.1 Add `const BLOCKING_STATUSES: &[&str] = &["blocked", "needs-clarification"]` — statuses that trigger cascade blocking
  - [ ] 1.2 Add `/// doc comment` explaining that these statuses indicate a story cannot proceed without human intervention, as opposed to "in-progress" or "backlog" which are transient

- [ ] Task 2: Implement `CascadeBlockInfo` struct in `src/watcher/deps.rs` (AC: #1)
  - [ ] 2.1 Create `#[derive(Debug, Clone)] pub struct CascadeBlockInfo` with fields: `blocked_story: String` (the story being blocked), `root_cause_story: String` (the original story with blocking status), `root_cause_status: String` (e.g. "blocked" or "needs-clarification"), `chain: Vec<String>` (full dependency chain from root cause to blocked story)
  - [ ] 2.2 Implement `Display` for `CascadeBlockInfo` for human-readable log output

- [ ] Task 3: Implement `find_cascade_blocks()` in `src/watcher/deps.rs` (AC: #1, #2)
  - [ ] 3.1 Implement `pub fn find_cascade_blocks(stories: &[StoryInfo], all_statuses: &HashMap<String, String>) -> Vec<CascadeBlockInfo>` — discovers all cascade blocks by traversing dependency chains
  - [ ] 3.2 For each story, walk its dependency chain transitively: if any ancestor has a blocking status, record a `CascadeBlockInfo` with the full chain
  - [ ] 3.3 Handle transitive chains: if 1-1 is blocked, 1-2 depends on 1-1, 1-3 depends on 1-2 → both 1-2 AND 1-3 are cascade-blocked with root_cause = 1-1
  - [ ] 3.4 Use iterative traversal (not recursion) to avoid stack overflow on deep chains
  - [ ] 3.5 If a dependency is not in `all_statuses` at all, treat it as unmet but NOT cascade-blocked (it's an unknown, not a failure)

- [ ] Task 4: Integrate cascade detection into `filter_eligible()` in `src/watcher/deps.rs` (AC: #1, #2, #3)
  - [ ] 4.1 After `derive_dependencies()` and `topological_sort()`, call `find_cascade_blocks()` to identify cascade-blocked stories
  - [ ] 4.2 For each cascade-blocked story, log at **warn** level: `tracing::warn!(story_key = %info.blocked_story, root_cause = %info.root_cause_story, root_status = %info.root_cause_status, chain = ?info.chain, "Story cascade-blocked — prerequisite failed")`
  - [ ] 4.3 For stories skipped due to deps not yet `done` (but not cascade-blocked), keep existing **info** level log from Story 2.2
  - [ ] 4.4 Exclude cascade-blocked stories from the eligible result — they are a subset of "deps not satisfied" but with distinct logging
  - [ ] 4.5 Return only stories that pass BOTH checks: deps satisfied AND not cascade-blocked

- [ ] Task 5: Add `cascade_blocked_count` to pre-gate log in `Watcher::poll()` (AC: #1, #3)
  - [ ] 5.1 Update the pre-gate summary log in `Watcher::poll()` to include cascade-blocked count: `tracing::info!(pre_gate_input = eligible.len(), pre_gate_output = filtered.len(), cascade_blocked = cascade_count, "Pre-gate dependency filter applied")`
  - [ ] 5.2 `filter_eligible` should return cascade block count alongside filtered stories — update return type to `Result<(Vec<StoryInfo>, usize), WatcherError>` OR pass cascade count through a separate mechanism (simplest: return tuple)

- [ ] Task 6: Write unit tests (AC: #1, #2, #3)
  - [ ] 6.1 Test `find_cascade_blocks` detects direct cascade: story 1-2 blocked when dep 1-1 is `blocked`
  - [ ] 6.2 Test `find_cascade_blocks` detects transitive cascade: 1-3 cascade-blocked through 1-2 → 1-1(blocked)
  - [ ] 6.3 Test `find_cascade_blocks` returns correct root cause across chain
  - [ ] 6.4 Test `find_cascade_blocks` detects `needs-clarification` as blocking status
  - [ ] 6.5 Test `find_cascade_blocks` does NOT cascade on `in-progress` or `backlog` (these are transient, not failures)
  - [ ] 6.5b Test `find_cascade_blocks` does NOT cascade on `review` status (transient — code review in progress)
  - [ ] 6.6 Test `find_cascade_blocks` returns empty when no blocking statuses exist
  - [ ] 6.7 Test `find_cascade_blocks` handles story with unknown/missing dependency gracefully
  - [ ] 6.8 Test `filter_eligible` excludes cascade-blocked stories from result
  - [ ] 6.9 Test `filter_eligible` returns cascade count correctly
  - [ ] 6.10 Test re-evaluation: if blocking dep changes to `done`, cascade-blocked stories become eligible on next call
  - [ ] 6.11 Test multiple independent cascades: epic 1 has a blocker, epic 2 is unaffected
  - [ ] 6.12 Test full integration: `Watcher::poll()` with cascade-blocked stories logged at warn
  - [ ] 6.13 Test `build_full_dependency_map` builds correct dep map from sprint-status entries (skips epics, retros, includes sequential deps)
  - [ ] 6.14 Test `reconstruct_chain` produces correct ordered chain from root cause to blocked story
  - [ ] 6.15 Test `find_root_blocker` returns None when no blocking ancestor exists

- [ ] Task 7: Final quality checks
  - [ ] 7.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [ ] 7.2 Run `cargo clippy` and fix any warnings
  - [ ] 7.3 Run `cargo test` and verify all tests pass (including Story 2.1 and 2.2 tests)
  - [ ] 7.4 Verify all public items have `///` doc comments
  - [ ] 7.5 Manual integration test: create sprint-status with 1-1 as `blocked`, 1-2 and 1-3 as `ready-for-dev` → verify both are cascade-blocked with correct root cause
  - [ ] 7.6 Manual integration test: change 1-1 to `done` → verify 1-2 becomes eligible and 1-3 remains skipped (dep 1-2 not done yet)

## Dev Notes

### Previous Story Intelligence

**Story 2.2** established:
- `DependencyGraph` struct with `adjacency: HashMap<String, Vec<String>>`, `all_statuses: HashMap<String, String>`, `doc_order: HashMap<String, usize>`
- `DependencyGraph::new(stories, all_statuses)` — builds graph with document-order tracking
- `DependencyGraph::topological_sort()` — Kahn's algorithm with `BinaryHeap<Reverse<(usize, String)>>` for deterministic sprint-order tiebreaker
- `DependencyGraph::deps_satisfied(story_key)` → `(bool, Option<(String, String)>)` — returns unmet dep key and its status
- `derive_dependencies(stories, all_statuses)` — populates `StoryInfo.dependencies` using intra-epic sequential rule (N.M depends on N.(M-1)). Uses `StoryInfo::from_key_and_status()` for DRY key parsing
- `filter_eligible(stories, all_statuses)` → `Result<Vec<StoryInfo>, WatcherError>` — main pre-gate entry point: derive deps → build graph → topo sort → filter by deps_satisfied
- `CyclicDependency { cycle: Vec<String> }` variant in `WatcherError`
- Integration in `Watcher::poll()`: calls `deps::filter_eligible(eligible, entries)` after getting eligible stories
- `SprintStatusFile::entries()` → `&[(String, String)]` exposes all sprint-status entries
- `make_test_bot_config` marked `pub(crate)` in `watcher/mod.rs` tests for cross-module test access
- Pre-gate summary log: `tracing::info!(pre_gate_input, pre_gate_output, "Pre-gate dependency filter applied")`

**Story 2.1** established:
- `StoryInfo` with `dependencies: Vec<String>`, `story_key`, `epic_num`, `story_num`, `label`, `status`, etc.
- `WatcherError` enum: `SprintStatusNotFound`, `SprintStatusRead`, `SprintStatusParse`, `NoEligibleStories`
- `SprintStatusFile::load()` — TOCTOU-safe file read with error mapping
- `Watcher::poll()` returns `Result<Vec<StoryInfo>, WatcherError>`
- `tempfile` in `[dev-dependencies]`

**Stories 1.1–1.4** established:
- `Arc<BotConfig>` sharing pattern
- Per-module `thiserror` enum pattern (no `anyhow` in library modules)
- Tracing patterns: structured fields, warn for degraded, error for failures, info for normal operations

### Cascade Blocking Design

**⚠️ FR4 vs Architecture Decision 2 — Design Reconciliation:**
FR4 in the PRD says the daemon can "mark dependent stories as `blocked`", which implies writing to `sprint-status.yaml`. However, **Architecture Decision 2** explicitly overrides this: the daemon is a **pure reader** — all mutations are performed by the BMAD agent. The epics ACs confirm: "the daemon never writes to `sprint-status.yaml` — all blocking logic is computed in-memory per cycle." Therefore, this story implements **in-memory cascade identification** (log + skip), NOT disk writes. Do NOT attempt to write `blocked` status to sprint-status.yaml.

**What cascade blocking adds beyond Story 2.2:**

Story 2.2's `filter_eligible` already skips stories whose dependencies are not `done`. Story 2.3 adds:

1. **Root cause awareness**: Distinguish between "dep not done yet" (transient — `in-progress`, `backlog`, `ready-for-dev`) vs "dep has failed" (`blocked`, `needs-clarification`). The former is a normal skip (info log); the latter is a cascade block (warn log).

2. **Transitive chain discovery**: Trace the full chain from blocked story to root cause. Example: if 1-1 is `blocked`, 1-2 depends on 1-1, 1-3 depends on 1-2 → 1-3's root cause is 1-1 (not 1-2).

3. **Richer logging**: `tracing::warn!` with root cause story, root cause status, and full chain — enables operators to quickly identify WHY a story isn't being processed.

4. **Re-evaluation is automatic**: Since all blocking logic is computed in-memory per poll cycle (Architecture Decision 2: daemon is pure reader), when a blocking story is resolved (status changes to `done`), the next cycle re-computes everything from scratch and cascade-blocked stories become eligible again.

**Why this is a separate story from 2.2:**
- Story 2.2: "Can the story proceed?" (binary deps check)
- Story 2.3: "WHY can't it proceed, and is it permanently stuck?" (root cause analysis + actionable logging)

**Blocking vs non-blocking statuses:**
```
BLOCKING (cascade trigger):
  - "blocked"              → Agent hit a wall, needs human fix
  - "needs-clarification"  → Agent couldn't answer, escalated to human

NON-BLOCKING (transient, will resolve):
  - "backlog"              → Not started yet
  - "ready-for-dev"        → Story file created, waiting for dev
  - "in-progress"          → Agent actively working
  - "review"               → Code review in progress
  - "done"                 → Dependency satisfied ✓
```

### `CascadeBlockInfo` Implementation — `src/watcher/deps.rs`

```rust
use std::fmt;

/// Information about a cascade-blocked story.
///
/// When a prerequisite story has a blocking status (`blocked` or
/// `needs-clarification`), all direct and transitive dependents
/// are cascade-blocked. This struct records the full chain from
/// root cause to affected story for diagnostic logging.
///
/// **Forward-compatibility:** This struct is intentionally `pub` because
/// Epic 6 Story 6.1 (Telegram Notifications — FR25/FR26) will use it to
/// include cascade-block details in human notifications. Do not restrict
/// visibility or tightly couple to watcher internals.
#[derive(Debug, Clone)]
pub struct CascadeBlockInfo {
    /// The story that is being blocked.
    pub blocked_story: String,
    /// The original story with the blocking status (root cause).
    pub root_cause_story: String,
    /// The blocking status of the root cause (e.g., "blocked", "needs-clarification").
    pub root_cause_status: String,
    /// Full dependency chain from root cause to blocked story.
    /// Example: ["1-1-scaffolding", "1-2-cli", "1-3-init"] means
    /// 1-1 is the root cause, 1-2 depends on 1-1, 1-3 depends on 1-2.
    pub chain: Vec<String>,
}

impl fmt::Display for CascadeBlockInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} cascade-blocked by {} (status: {}, chain: {})",
            self.blocked_story,
            self.root_cause_story,
            self.root_cause_status,
            self.chain.join(" → ")
        )
    }
}
```

### `find_cascade_blocks` Implementation — `src/watcher/deps.rs`

The implementation uses a two-phase approach: first find the root blocker via DFS, then reconstruct the correct chain by walking forward from root cause to blocked story.

```rust
/// Statuses that indicate a story has failed and cannot proceed without
/// human intervention. These trigger cascade blocking of all dependents.
const BLOCKING_STATUSES: &[&str] = &["blocked", "needs-clarification"];

/// Walk the dependency chain of `start_deps` transitively to find the
/// first ancestor with a blocking status.
///
/// Returns `Some((root_cause_key, root_cause_status))` if found, `None` otherwise.
/// Uses iterative DFS to avoid stack overflow on deep chains.
fn find_root_blocker(
    start_deps: &[String],
    all_statuses: &HashMap<String, String>,
    all_deps: &HashMap<String, Vec<String>>,
) -> Option<(String, String)> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = start_deps.to_vec();

    while let Some(dep_key) = stack.pop() {
        if !visited.insert(dep_key.clone()) {
            continue;
        }

        let dep_status = all_statuses
            .get(&dep_key)
            .map(|s| s.as_str())
            .unwrap_or("unknown");

        if BLOCKING_STATUSES.contains(&dep_status) {
            return Some((dep_key, dep_status.to_string()));
        }

        // If dep is "done", stop traversal: a completed story succeeded regardless
        // of its ancestors' current status — its dependents are not affected.
        // Only traverse deeper for non-done, non-blocking statuses (transient states
        // like "in-progress", "backlog", "ready-for-dev", "review") to find a
        // potential blocking ancestor further up the chain.
        if dep_status != "done" {
            if let Some(transitive_deps) = all_deps.get(&dep_key) {
                for td in transitive_deps {
                    if !visited.contains(td) {
                        stack.push(td.clone());
                    }
                }
            }
        }
    }

    None
}

/// Reconstruct the dependency chain from `root_cause` to `blocked_story`.
///
/// Walks forward from the root cause using the reverse dependency map
/// (dependents), building the path to the blocked story.
/// Returns the chain in order: [root_cause, ..., intermediate, blocked_story].
fn reconstruct_chain(
    root_cause: &str,
    blocked_story: &str,
    all_deps: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    // Build reverse map: story_key → stories that depend on it
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for (key, deps) in all_deps {
        for dep in deps {
            dependents.entry(dep.as_str()).or_default().push(key.as_str());
        }
    }

    // BFS from root_cause to blocked_story through dependents
    let mut visited: HashSet<&str> = HashSet::new();
    let mut parent: HashMap<&str, &str> = HashMap::new();
    let mut queue: VecDeque<&str> = VecDeque::new();

    visited.insert(root_cause);
    queue.push_back(root_cause);

    while let Some(current) = queue.pop_front() {
        if current == blocked_story {
            break;
        }
        if let Some(deps) = dependents.get(current) {
            for &dependent in deps {
                if visited.insert(dependent) {
                    parent.insert(dependent, current);
                    queue.push_back(dependent);
                }
            }
        }
    }

    // Reconstruct path from blocked_story back to root_cause
    let mut chain = Vec::new();
    let mut current = blocked_story;
    chain.push(current.to_string());
    while let Some(&prev) = parent.get(current) {
        chain.push(prev.to_string());
        current = prev;
        if current == root_cause {
            break;
        }
    }
    chain.reverse(); // Now in order: root_cause → ... → blocked_story
    chain
}

/// Discover cascade-blocked stories by traversing dependency chains.
///
/// For each story, walks its dependency chain transitively to find a
/// blocking root cause (`blocked` or `needs-clarification`). If found,
/// reconstructs the correct chain from root cause to blocked story.
///
/// Uses two-phase approach:
/// 1. `find_root_blocker()` — DFS to find the first blocking ancestor
/// 2. `reconstruct_chain()` — BFS forward from root cause to build correct path
///
/// # Arguments
/// * `stories` — Stories to check for cascade blocking (typically eligible stories)
/// * `all_statuses` — Complete status map from sprint-status.yaml
/// * `all_deps` — Dependency map: story_key → list of dependency story_keys
///   (includes ALL stories, not just eligible ones — needed for transitive traversal)
pub fn find_cascade_blocks(
    stories: &[StoryInfo],
    all_statuses: &HashMap<String, String>,
    all_deps: &HashMap<String, Vec<String>>,
) -> Vec<CascadeBlockInfo> {
    let mut cascade_blocks: Vec<CascadeBlockInfo> = Vec::new();

    for story in stories {
        if let Some((root_key, root_status)) = find_root_blocker(
            &story.dependencies,
            all_statuses,
            all_deps,
        ) {
            let chain = reconstruct_chain(&root_key, &story.story_key, all_deps);
            cascade_blocks.push(CascadeBlockInfo {
                blocked_story: story.story_key.clone(),
                root_cause_story: root_key,
                root_cause_status: root_status,
                chain,
            });
        }
    }

    cascade_blocks
}
```

### Build Full Dependency Map — `src/watcher/deps.rs`

The `find_cascade_blocks` function needs deps for ALL stories (not just eligible ones) to do transitive traversal. The `reconstruct_chain` helper also needs a `VecDeque` import. Add a helper:

> **DRY note:** This function duplicates the intra-epic sequential dependency logic from `derive_dependencies()` (Story 2.2). Both iterate `all_statuses`, parse keys with `from_key_and_status()`, and compute N.(M-1) predecessors. Ideally `build_full_dependency_map` would be the single source of truth and `derive_dependencies` would call it. However, `derive_dependencies` mutates a `&mut [StoryInfo]` slice in-place (Story 2.2 API contract), while this function returns a standalone `HashMap`. Refactoring would break Story 2.2's interface. The duplication is intentional and acceptable — both functions are small, pure, and tested independently. Do NOT attempt to merge them.

```rust
/// Build a dependency map for ALL story entries in sprint-status.yaml.
///
/// This extends `derive_dependencies` (which only populates eligible stories)
/// to cover the full sprint-status. Needed for transitive cascade detection:
/// if 1-1 is blocked and 1-2 depends on 1-1, we need to know 1-2's deps
/// even if 1-2 is not in the eligible set.
pub fn build_full_dependency_map(
    all_statuses: &[(String, String)],
) -> HashMap<String, Vec<String>> {
    let dummy_dir = std::path::Path::new("");
    let key_lookup: HashMap<(u32, u32), String> = all_statuses
        .iter()
        .filter_map(|(key, status)| {
            let info = StoryInfo::from_key_and_status(key, status, dummy_dir)?;
            Some(((info.epic_num, info.story_num), key.clone()))
        })
        .collect();

    let mut dep_map: HashMap<String, Vec<String>> = HashMap::new();

    for (key, status) in all_statuses {
        let info = match StoryInfo::from_key_and_status(key, status, dummy_dir) {
            Some(i) => i,
            None => continue, // Skip epics, retrospectives
        };

        let mut deps = Vec::new();
        if info.story_num > 1 {
            if let Some(pred_key) = key_lookup.get(&(info.epic_num, info.story_num - 1)) {
                deps.push(pred_key.clone());
            }
        }
        dep_map.insert(key.clone(), deps);
    }

    dep_map
}
```

### Updated `filter_eligible` — `src/watcher/deps.rs`

**⚠️ Breaking signature change:**
- **Before (Story 2.2):** `pub fn filter_eligible(...) -> Result<Vec<StoryInfo>, WatcherError>`
- **After (Story 2.3):** `pub fn filter_eligible(...) -> Result<(Vec<StoryInfo>, usize), WatcherError>`

The second tuple element is the cascade-blocked count. All callers must be updated to destructure the tuple (see "Impact on Story 2.2 Tests" section below).

```rust
/// Pre-gate filter: resolve dependencies, detect cascade blocks, return eligible stories.
///
/// This is the main entry point for the dependency pre-gate (updated for cascade blocking).
/// It derives dependencies, builds the graph, checks for cycles, detects cascade blocks,
/// and filters out stories with unmet or blocked dependencies.
///
/// # Returns
/// Tuple of (filtered eligible stories in topological order, cascade-blocked count).
pub fn filter_eligible(
    mut stories: Vec<StoryInfo>,
    all_statuses: &[(String, String)],
) -> Result<(Vec<StoryInfo>, usize), WatcherError> {
    if stories.is_empty() {
        return Ok((stories, 0));
    }

    // Step 1: Derive dependencies from sprint-status ordering
    derive_dependencies(&mut stories, all_statuses);

    // Step 2: Build dependency graph
    let graph = DependencyGraph::new(&stories, all_statuses);

    // Step 3: Topological sort (detects cycles)
    let sorted_keys = graph.topological_sort()?;

    // Step 4: Build full dependency map for transitive cascade detection
    let all_statuses_map: HashMap<String, String> = all_statuses.iter().cloned().collect();
    let full_dep_map = build_full_dependency_map(all_statuses);

    // Step 5: Detect cascade blocks
    let cascade_blocks = find_cascade_blocks(&stories, &all_statuses_map, &full_dep_map);
    let cascade_blocked_keys: HashSet<String> = cascade_blocks
        .iter()
        .map(|cb| cb.blocked_story.clone())
        .collect();

    // Log cascade blocks at warn level
    for cb in &cascade_blocks {
        tracing::warn!(
            story_key = %cb.blocked_story,
            root_cause = %cb.root_cause_story,
            root_status = %cb.root_cause_status,
            chain = ?cb.chain,
            "Story cascade-blocked — prerequisite failed"
        );
    }

    let cascade_count = cascade_blocks.len();

    // Step 6: Filter — only include stories with all deps satisfied AND not cascade-blocked
    let story_map: HashMap<String, StoryInfo> = stories
        .into_iter()
        .map(|s| (s.story_key.clone(), s))
        .collect();

    let mut eligible: Vec<StoryInfo> = Vec::new();
    for key in &sorted_keys {
        // Skip cascade-blocked stories (already logged at warn)
        if cascade_blocked_keys.contains(key) {
            continue;
        }

        let (satisfied, unmet) = graph.deps_satisfied(key);
        if satisfied {
            if let Some(story) = story_map.get(key) {
                eligible.push(story.clone());
            }
        } else if let Some((dep_key, dep_status)) = unmet {
            // Non-blocking skip (dep is in-progress, backlog, etc. — transient)
            tracing::info!(
                story_key = %key,
                unmet_dep = %dep_key,
                dep_status = %dep_status,
                "Story skipped — dependency not yet done"
            );
        }
    }

    Ok((eligible, cascade_count))
}
```

### Updated `Watcher::poll()` — `src/watcher/mod.rs`

Update to handle the new tuple return from `filter_eligible`:

```rust
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

    // Pre-gate: dependency resolution, cascade detection, and filtering
    let entries = sprint_status.entries();
    let (filtered, cascade_count) = deps::filter_eligible(eligible, entries)?;

    tracing::info!(
        pre_gate_input = all_stories.len(),
        pre_gate_output = filtered.len(),
        cascade_blocked = cascade_count,
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
            "Eligible story detected (deps satisfied, not cascade-blocked)"
        );
    }

    Ok(filtered)
}
```

### Files Modified/Created in This Story

| File | Change |
|------|--------|
| `src/watcher/deps.rs` | Add `BLOCKING_STATUSES`, `CascadeBlockInfo`, `find_cascade_blocks()`, `build_full_dependency_map()`. Update `filter_eligible()` return type to `(Vec<StoryInfo>, usize)` and integrate cascade detection |
| `src/watcher/mod.rs` | Update `Watcher::poll()` to destructure tuple from `filter_eligible()`, add `cascade_blocked` to pre-gate log |
| `src/cli/mod.rs` | No changes needed — `Watcher::poll()` return type is unchanged (`Result<Vec<StoryInfo>, WatcherError>`) |

### Imports Required in `src/watcher/deps.rs`

After this story, the imports at the top of `deps.rs` should be:

```rust
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::cmp::Reverse;
use std::fmt;
use std::path::Path;
use super::{StoryInfo, WatcherError};
```

> **NOTE:** `HashSet` is now used (by `find_cascade_blocks` / `find_root_blocker` / `reconstruct_chain` for visited tracking, and by `filter_eligible` for cascade_blocked_keys). `VecDeque` is now used by `reconstruct_chain` for BFS. `BinaryHeap` remains for `topological_sort` from Story 2.2.

### Impact on Story 2.2 Tests

The return type of `filter_eligible` changes from `Result<Vec<StoryInfo>, WatcherError>` to `Result<(Vec<StoryInfo>, usize), WatcherError>`. This is a **backwards-incompatible change**.

**All Story 2.2 tests that must be updated to destructure the tuple:**

1. `test_filter_eligible_returns_stories_with_deps_done`
2. `test_filter_eligible_skips_story_with_dep_ready_for_dev`
3. `test_filter_eligible_skips_story_with_dep_in_progress`
4. `test_filter_eligible_first_story_always_eligible`
5. `test_filter_eligible_empty_when_all_deps_unmet`
6. `test_filter_eligible_preserves_topological_order`
7. `test_filter_eligible_empty_input_returns_empty`
8. `test_watcher_poll_with_deps_filtering` (calls `watcher.poll()` which internally calls `filter_eligible`)

**Update pattern for each:**

```rust
// Before (Story 2.2):
let result = filter_eligible(stories, &all_statuses).unwrap();

// After (Story 2.3):
let (result, _cascade_count) = filter_eligible(stories, &all_statuses).unwrap();
```

For `test_watcher_poll_with_deps_filtering`, no change is needed to the test code itself since it calls `watcher.poll()` which still returns `Result<Vec<StoryInfo>, WatcherError>`. The destructuring change is internal to `Watcher::poll()`.

### Anti-Patterns to Avoid

- ❌ **NO** writing to sprint-status.yaml — daemon is a PURE READER
- ❌ **NO** persisting cascade state between poll cycles — recompute from scratch each cycle
- ❌ **NO** treating `in-progress` or `backlog` as blocking — these are transient states
- ❌ **NO** recursive traversal in `find_cascade_blocks` — use iterative to avoid stack overflow
- ❌ **NO** `unwrap()` or `expect()` in production code
- ❌ **NO** `anyhow::Result` in `deps.rs` — typed `WatcherError` only
- ❌ **NO** adding new `WatcherError` variants for cascade blocking — cascade blocks are informational (warn log), not error conditions. They're a refinement of the existing "deps not satisfied" path
- ❌ **NO** complex cascade config — keep it simple: hardcoded `BLOCKING_STATUSES` list
- ❌ **NO** modifying modules other than `watcher/deps.rs` and `watcher/mod.rs`

### Scope Boundaries

**IN SCOPE for this story:**
- `src/watcher/deps.rs` — `CascadeBlockInfo`, `find_cascade_blocks`, `build_full_dependency_map`, updated `filter_eligible`
- `src/watcher/mod.rs` — Updated `Watcher::poll()` for new `filter_eligible` return type
- Update existing Story 2.2 tests to handle new `filter_eligible` tuple return

**OUT OF SCOPE — do NOT implement:**
- Writing cascade status to sprint-status.yaml (daemon never writes)
- Cross-epic cascade detection (stays intra-epic only, same as dependency model)
- Notification of cascade blocks to human (Story 6.1: Telegram notifications)
- Session launching for eligible stories (Epic 4)
- Any new WatcherError variants (cascade is logged, not errored)

### Testing Requirements

All tests go inline at the bottom of `src/watcher/deps.rs` in `#[cfg(test)] mod tests`, added after Story 2.2's existing tests:

```rust
#[cfg(test)]
mod tests {
    // ... (existing Story 2.2 tests — updated for tuple return) ...

    // --- Test helpers for cascade blocking tests ---

    /// Helper: build a HashMap<String, String> from a slice of (&str, &str) pairs.
    /// Reduces boilerplate in cascade blocking tests that construct status maps.
    fn make_statuses_map(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// Helper: build a HashMap<String, Vec<String>> dependency map from (key, deps) pairs.
    fn make_deps_map(entries: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        entries
            .iter()
            .map(|(k, deps)| (k.to_string(), deps.iter().map(|d| d.to_string()).collect()))
            .collect()
    }

    // --- Cascade blocking tests ---

    #[test]
    fn test_find_cascade_direct_blocked() {
        let mut story = make_story("1-2-cli", "ready-for-dev");
        story.dependencies = vec!["1-1-scaffolding".to_string()];

        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "blocked"),
            ("1-2-cli", "ready-for-dev"),
        ]);
        let all_deps = make_deps_map(&[
            ("1-1-scaffolding", &[]),
            ("1-2-cli", &["1-1-scaffolding"]),
        ]);

        let blocks = find_cascade_blocks(&[story], &all_statuses, &all_deps);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].blocked_story, "1-2-cli");
        assert_eq!(blocks[0].root_cause_story, "1-1-scaffolding");
        assert_eq!(blocks[0].root_cause_status, "blocked");
        // Chain should be in correct order: root → blocked
        assert_eq!(blocks[0].chain, vec!["1-1-scaffolding", "1-2-cli"]);
    }

    #[test]
    fn test_find_cascade_transitive_chain() {
        // 1-1 is blocked, 1-2 depends on 1-1, 1-3 depends on 1-2
        // 1-3 should be cascade-blocked with root cause 1-1
        let mut story_3 = make_story("1-3-init", "ready-for-dev");
        story_3.dependencies = vec!["1-2-cli".to_string()];

        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "blocked"),
            ("1-2-cli", "ready-for-dev"),
            ("1-3-init", "ready-for-dev"),
        ]);
        let all_deps = make_deps_map(&[
            ("1-1-scaffolding", &[]),
            ("1-2-cli", &["1-1-scaffolding"]),
            ("1-3-init", &["1-2-cli"]),
        ]);

        let blocks = find_cascade_blocks(&[story_3], &all_statuses, &all_deps);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].blocked_story, "1-3-init");
        assert_eq!(blocks[0].root_cause_story, "1-1-scaffolding");
        // Chain must be in correct order: root → intermediate → blocked
        assert_eq!(blocks[0].chain, vec!["1-1-scaffolding", "1-2-cli", "1-3-init"]);
    }

    #[test]
    fn test_find_cascade_needs_clarification() {
        let mut story = make_story("1-2-cli", "ready-for-dev");
        story.dependencies = vec!["1-1-scaffolding".to_string()];

        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "needs-clarification"),
            ("1-2-cli", "ready-for-dev"),
        ]);
        let all_deps = make_deps_map(&[
            ("1-1-scaffolding", &[]),
            ("1-2-cli", &["1-1-scaffolding"]),
        ]);

        let blocks = find_cascade_blocks(&[story], &all_statuses, &all_deps);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].root_cause_status, "needs-clarification");
    }

    #[test]
    fn test_find_cascade_no_cascade_on_in_progress() {
        let mut story = make_story("1-2-cli", "ready-for-dev");
        story.dependencies = vec!["1-1-scaffolding".to_string()];

        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "in-progress"),
            ("1-2-cli", "ready-for-dev"),
        ]);
        let all_deps = make_deps_map(&[
            ("1-1-scaffolding", &[]),
            ("1-2-cli", &["1-1-scaffolding"]),
        ]);

        let blocks = find_cascade_blocks(&[story], &all_statuses, &all_deps);
        assert!(blocks.is_empty(), "in-progress is transient, not a cascade blocker");
    }

    #[test]
    fn test_find_cascade_no_cascade_on_review() {
        // "review" is a transient status (code review in progress) — NOT a blocker
        let mut story = make_story("1-2-cli", "ready-for-dev");
        story.dependencies = vec!["1-1-scaffolding".to_string()];

        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "review"),
            ("1-2-cli", "ready-for-dev"),
        ]);
        let all_deps = make_deps_map(&[
            ("1-1-scaffolding", &[]),
            ("1-2-cli", &["1-1-scaffolding"]),
        ]);

        let blocks = find_cascade_blocks(&[story], &all_statuses, &all_deps);
        assert!(blocks.is_empty(), "review is transient, not a cascade blocker");
    }

    #[test]
    fn test_find_cascade_no_blocks_when_all_done() {
        let mut story = make_story("1-2-cli", "ready-for-dev");
        story.dependencies = vec!["1-1-scaffolding".to_string()];

        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "done"),
            ("1-2-cli", "ready-for-dev"),
        ]);

        let all_deps = HashMap::new();
        let blocks = find_cascade_blocks(&[story], &all_statuses, &all_deps);
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_find_cascade_unknown_dep_not_cascade() {
        let mut story = make_story("1-2-cli", "ready-for-dev");
        story.dependencies = vec!["1-1-scaffolding".to_string()];

        // 1-1-scaffolding not in all_statuses at all
        let all_statuses: HashMap<String, String> = HashMap::new();
        let all_deps: HashMap<String, Vec<String>> = HashMap::new();

        let blocks = find_cascade_blocks(&[story], &all_statuses, &all_deps);
        assert!(blocks.is_empty(), "Unknown dep is unmet but not cascade-blocked");
    }

    #[test]
    fn test_filter_eligible_excludes_cascade_blocked() {
        let all_statuses = vec![
            ("epic-1".to_string(), "in-progress".to_string()),
            ("1-1-scaffolding".to_string(), "blocked".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
            ("2-1-polling".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![
            make_story("1-2-cli", "ready-for-dev"),
            make_story("2-1-polling", "ready-for-dev"),
        ];

        let (result, cascade_count) = filter_eligible(stories, &all_statuses).unwrap();
        // 1-2 is cascade-blocked (dep 1-1 is "blocked")
        // 2-1 has no deps → eligible
        assert_eq!(cascade_count, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].story_key, "2-1-polling");
    }

    #[test]
    fn test_filter_eligible_returns_cascade_count() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "needs-clarification".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
            ("1-3-init".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![
            make_story("1-2-cli", "ready-for-dev"),
            make_story("1-3-init", "ready-for-dev"),
        ];

        let (result, cascade_count) = filter_eligible(stories, &all_statuses).unwrap();
        // Both 1-2 and 1-3 cascade-blocked (1-1 is needs-clarification)
        // 1-2 directly blocked, 1-3 transitively blocked through 1-2
        assert_eq!(cascade_count, 2);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_eligible_re_evaluation_on_resolution() {
        // First call: 1-1 is blocked → 1-2 cascade-blocked
        let all_statuses_blocked = vec![
            ("1-1-scaffolding".to_string(), "blocked".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
        ];
        let stories1 = vec![make_story("1-2-cli", "ready-for-dev")];
        let (result1, cc1) = filter_eligible(stories1, &all_statuses_blocked).unwrap();
        assert!(result1.is_empty());
        assert_eq!(cc1, 1);

        // Second call: 1-1 is now done → 1-2 becomes eligible
        let all_statuses_resolved = vec![
            ("1-1-scaffolding".to_string(), "done".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
        ];
        let stories2 = vec![make_story("1-2-cli", "ready-for-dev")];
        let (result2, cc2) = filter_eligible(stories2, &all_statuses_resolved).unwrap();
        assert_eq!(result2.len(), 1);
        assert_eq!(result2[0].story_key, "1-2-cli");
        assert_eq!(cc2, 0);
    }

    #[test]
    fn test_filter_eligible_independent_epics_unaffected() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "blocked".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
            ("2-1-polling".to_string(), "ready-for-dev".to_string()),
            ("2-2-deps".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![
            make_story("1-2-cli", "ready-for-dev"),
            make_story("2-1-polling", "ready-for-dev"),
            make_story("2-2-deps", "ready-for-dev"),
        ];

        let (result, cascade_count) = filter_eligible(stories, &all_statuses).unwrap();
        // Epic 1: 1-2 cascade-blocked
        // Epic 2: 2-1 eligible (no deps), 2-2 skipped (dep 2-1 not done — but NOT cascade-blocked)
        assert_eq!(cascade_count, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].story_key, "2-1-polling");
    }

    #[test]
    fn test_watcher_poll_with_cascade_blocking() {
        use std::fs;
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path();
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: blocked
  1-2-cli: ready-for-dev
  epic-2: in-progress
  2-1-polling: ready-for-dev
"#;
        fs::write(artifacts_dir.join("sprint-status.yaml"), content).unwrap();

        let config = std::sync::Arc::new(crate::watcher::tests::make_test_bot_config(artifacts_dir));
        let watcher = crate::watcher::Watcher::new(config);
        let result = watcher.poll();
        assert!(result.is_ok());
        let stories = result.unwrap();

        // 1-2 cascade-blocked (dep 1-1 is "blocked")
        // 2-1 eligible (no deps)
        assert_eq!(stories.len(), 1);
        assert_eq!(stories[0].story_key, "2-1-polling");
    }

    // --- build_full_dependency_map tests ---

    #[test]
    fn test_build_full_dep_map_correct_deps() {
        let all_statuses = vec![
            ("epic-1".to_string(), "in-progress".to_string()),
            ("1-1-scaffolding".to_string(), "done".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
            ("1-3-init".to_string(), "backlog".to_string()),
            ("epic-1-retrospective".to_string(), "optional".to_string()),
            ("epic-2".to_string(), "backlog".to_string()),
            ("2-1-polling".to_string(), "backlog".to_string()),
        ];

        let dep_map = build_full_dependency_map(&all_statuses);

        // Skips epics and retrospectives
        assert!(!dep_map.contains_key("epic-1"));
        assert!(!dep_map.contains_key("epic-1-retrospective"));
        assert!(!dep_map.contains_key("epic-2"));

        // First story in each epic has no deps
        assert_eq!(dep_map.get("1-1-scaffolding").unwrap(), &Vec::<String>::new());
        assert_eq!(dep_map.get("2-1-polling").unwrap(), &Vec::<String>::new());

        // Sequential deps within epic
        assert_eq!(dep_map.get("1-2-cli").unwrap(), &vec!["1-1-scaffolding".to_string()]);
        assert_eq!(dep_map.get("1-3-init").unwrap(), &vec!["1-2-cli".to_string()]);
    }

    // --- reconstruct_chain tests ---

    #[test]
    fn test_reconstruct_chain_correct_order() {
        let mut all_deps = HashMap::new();
        all_deps.insert("1-1-scaffolding".to_string(), vec![]);
        all_deps.insert("1-2-cli".to_string(), vec!["1-1-scaffolding".to_string()]);
        all_deps.insert("1-3-init".to_string(), vec!["1-2-cli".to_string()]);
        all_deps.insert("1-4-status".to_string(), vec!["1-3-init".to_string()]);

        let chain = reconstruct_chain("1-1-scaffolding", "1-4-status", &all_deps);
        assert_eq!(chain, vec![
            "1-1-scaffolding", "1-2-cli", "1-3-init", "1-4-status"
        ]);
    }

    #[test]
    fn test_reconstruct_chain_direct_dep() {
        let mut all_deps = HashMap::new();
        all_deps.insert("1-1-scaffolding".to_string(), vec![]);
        all_deps.insert("1-2-cli".to_string(), vec!["1-1-scaffolding".to_string()]);

        let chain = reconstruct_chain("1-1-scaffolding", "1-2-cli", &all_deps);
        assert_eq!(chain, vec!["1-1-scaffolding", "1-2-cli"]);
    }

    // --- find_root_blocker tests ---

    #[test]
    fn test_find_root_blocker_returns_none_when_no_blocker() {
        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "in-progress"),
        ]);

        let all_deps: HashMap<String, Vec<String>> = HashMap::new();

        let result = find_root_blocker(
            &["1-1-scaffolding".to_string()],
            &all_statuses,
            &all_deps,
        );
        assert!(result.is_none(), "in-progress is not a blocker");
    }

    #[test]
    fn test_find_root_blocker_finds_transitive_blocker() {
        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "needs-clarification"),
            ("1-2-cli", "ready-for-dev"),
        ]);
        let all_deps = make_deps_map(&[
            ("1-2-cli", &["1-1-scaffolding"]),
        ]);

        let result = find_root_blocker(
            &["1-2-cli".to_string()],
            &all_statuses,
            &all_deps,
        );
        assert!(result.is_some());
        let (root, status) = result.unwrap();
        assert_eq!(root, "1-1-scaffolding");
        assert_eq!(status, "needs-clarification");
    }
}
```

### Project Structure Notes

After this story, the watcher module is **fully complete** for Epic 2:

```
src/watcher/
├── mod.rs      # WatcherError, StoryInfo, SprintStatusFile, Watcher — feature-complete
└── deps.rs     # DependencyGraph, derive/filter/cascade — feature-complete
```

Epic 2 delivery:
- **Story 2.1:** Sprint-status polling & story detection ✓
- **Story 2.2:** Dependency resolution & execution order ✓
- **Story 2.3:** Cascade blocking ✓ (this story)

The watcher → session interface contract is now fully defined:
- `Watcher::poll()` returns stories that are `ready-for-dev`, have all deps `done`, and are not cascade-blocked
- Session module (Epic 4) can trust that any story from `poll()` is safe to execute

### References

- [Source: epics.md § Story 2.3: Cascade Blocking] — User story, acceptance criteria
- [Source: epics.md § Epic 2: Story Watching & Dependency Management] — Epic context, all blocking logic in-memory
- [Source: prd.md § FR4] — Mark dependent stories as blocked when prerequisite fails
- [Source: architecture.md § Decision 2: Sprint-Status Mutation] — Daemon is pure reader, in-memory computation
- [Source: architecture.md § Data Flow] — Step 3: deps computes pre-gate, cascade blocking
- [Source: architecture.md § Error Type Pattern] — Per-module thiserror enums
- [Source: project-context.md § Daemon Lifecycle § Pre-gate] — Deterministic dependency check, cascade blocked status
- [Source: project-context.md § Two-Layer Dependency Model] — Daemon pre-gate + BMAD agent
- [Source: project-context.md § Testing Rules] — Inline tests, descriptive snake_case
- [Source: Story 2.2] — DependencyGraph, derive_dependencies, filter_eligible, topological_sort
- [Source: Story 2.1] — StoryInfo with dependencies, WatcherError, SprintStatusFile, Watcher::poll()

## Dev Agent Record

<!-- This section is filled automatically by the dev agent post-implementation. Do not edit manually. -->

### Agent Model Used

_(filled post-implementation)_

### Debug Log References

### Completion Notes List

### File List