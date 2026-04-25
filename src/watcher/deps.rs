//! Dependency resolution, cascade blocking, and pre-gate filtering for the watcher.
//!
//! This module implements the daemon's pre-gate: a cheap, deterministic
//! dependency check that prevents token burn on stories that cannot proceed.
//!
//! **Dependency rules:**
//! 1. **Intra-epic sequential:** Story N.M depends on story N.(M-1) being resolved.
//! 2. **Cross-epic explicit:** `# depends-on:` inline comments in sprint-status.yaml
//!    declare additional dependencies. Supported formats:
//!    - `# depends-on: 4-1` — story shorthand (matched to full key `4-1-*`)
//!    - `# depends-on: epic-8` — depends on the last story of epic 8
//!    - `# depends-on: epics 1-6` — depends on the last story of epics 1 through 6
//!    - Parenthetical annotations like `(resolved)` are stripped automatically.
//!
//! **Cascade blocking:** When a prerequisite story has a blocking status
//! (`blocked` or `needs-clarification`), all direct and transitive dependents
//! are identified as cascade-blocked and logged at warn level. This is
//! computed in-memory per poll cycle — the daemon never writes to disk.
//!
//! **Architecture Decision 2:** The daemon is a pure reader. All dependency
//! and cascade logic is computed in-memory per poll cycle. Nothing is written to disk.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::Path;

use super::{StoryInfo, WatcherError};

/// Statuses that indicate a story has failed and cannot proceed without
/// human intervention. These trigger cascade blocking of all dependents.
///
/// - `"blocked"` — Agent hit a wall, needs human fix
/// - `"needs-clarification"` — Agent couldn't answer, escalated to human
///
/// Statuses like `"in-progress"`, `"backlog"`, `"ready-for-dev"`, and `"review"`
/// are transient and do NOT trigger cascade blocking.
const BLOCKING_STATUSES: &[&str] = &["blocked", "needs-clarification"];

/// Statuses that indicate a dependency is resolved and its dependents can proceed.
///
/// - `"done"` — Story completed normally
/// - `"superseded"` — Story replaced by another approach; considered resolved
///   for dependency purposes so downstream stories are not blocked
/// - `"absorbed"` — Story's scope was absorbed into another story; considered resolved
const RESOLVED_STATUSES: &[&str] = &["done", "superseded", "absorbed"];

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

    /// Check if all dependencies of a story are resolved.
    ///
    /// A dependency is satisfied when its status in sprint-status.yaml is one
    /// of [`RESOLVED_STATUSES`] (`done` or `superseded`).
    /// Dependencies not found in all_statuses are treated as unmet.
    pub fn deps_satisfied(&self, story_key: &str) -> (bool, Option<(String, String)>) {
        if let Some(deps) = self.adjacency.get(story_key) {
            for dep in deps {
                let status = self
                    .all_statuses
                    .get(dep)
                    .map(|s| s.as_str())
                    .unwrap_or("unknown");
                if !RESOLVED_STATUSES.contains(&status) {
                    return (false, Some((dep.clone(), status.to_string())));
                }
            }
        }
        (true, None)
    }
}

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
    /// Example: \["1-1-scaffolding", "1-2-cli", "1-3-init"\] means
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

        // If dep is resolved (done/superseded), stop traversal: a resolved story
        // succeeded regardless of its ancestors' current status — its dependents
        // are not affected. Only traverse deeper for non-resolved, non-blocking
        // statuses (transient states like "in-progress", "backlog",
        // "ready-for-dev", "review") to find a potential blocking ancestor
        // further up the chain.
        if !RESOLVED_STATUSES.contains(&dep_status)
            && let Some(transitive_deps) = all_deps.get(&dep_key)
        {
            for td in transitive_deps {
                if !visited.contains(td) {
                    stack.push(td.clone());
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
/// Returns the chain in order: \[root_cause, ..., intermediate, blocked_story\].
fn reconstruct_chain(
    root_cause: &str,
    blocked_story: &str,
    all_deps: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    // Build reverse map: story_key → stories that depend on it
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for (key, deps) in all_deps {
        for dep in deps {
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(key.as_str());
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
        if let Some((root_key, root_status)) =
            find_root_blocker(&story.dependencies, all_statuses, all_deps)
        {
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

/// Build a dependency map for ALL story entries in sprint-status.yaml.
///
/// This extends `derive_dependencies` (which only populates eligible stories)
/// to cover the full sprint-status. Needed for transitive cascade detection:
/// if 1-1 is blocked and 1-2 depends on 1-1, we need to know 1-2's deps
/// even if 1-2 is not in the eligible set.
///
/// **DRY note:** This function duplicates the intra-epic sequential dependency
/// logic from `derive_dependencies()` (Story 2.2). Both iterate `all_statuses`,
/// parse keys with `from_key_and_status()`, and compute N.(M-1) predecessors.
/// The duplication is intentional — `derive_dependencies` mutates a `&mut [StoryInfo]`
/// slice in-place (Story 2.2 API contract), while this returns a standalone `HashMap`.
pub fn build_full_dependency_map(
    all_statuses: &[(String, String)],
    comment_deps: &HashMap<String, Vec<String>>,
) -> HashMap<String, Vec<String>> {
    let dummy_dir = Path::new("");
    let key_lookup: HashMap<(u32, u32), String> = all_statuses
        .iter()
        .filter_map(|(key, status)| {
            let info = StoryInfo::from_key_and_status(key, status, dummy_dir)?;
            Some(((info.epic_num, info.story_num), key.clone()))
        })
        .collect();

    // Pre-resolve comment deps once for the entire map
    let resolved_comment = resolve_comment_deps(comment_deps, all_statuses);

    let mut dep_map: HashMap<String, Vec<String>> = HashMap::new();

    for (key, status) in all_statuses {
        let info = match StoryInfo::from_key_and_status(key, status, dummy_dir) {
            Some(i) => i,
            None => continue, // Skip epics, retrospectives
        };

        let mut deps = Vec::new();
        if info.story_num > 1
            && let Some(pred_key) = key_lookup.get(&(info.epic_num, info.story_num - 1))
        {
            deps.push(pred_key.clone());
        }

        // Merge resolved comment deps
        if let Some(extra) = resolved_comment.get(key) {
            for dep in extra {
                if !deps.contains(dep) {
                    deps.push(dep.clone());
                }
            }
        }

        dep_map.insert(key.clone(), deps);
    }

    dep_map
}

/// Resolve raw `# depends-on:` comment specs into actual story keys.
///
/// Handles three dependency target formats:
/// - **Story shorthand** (`4-1`): matched to the first full story key starting with `4-1-`
/// - **Epic reference** (`epic-8`): resolved to the last story of that epic (by document order)
/// - **Epic range** (`epics 1-6`): resolved to the last story of each epic 1 through 6
///
/// Unknown or unresolvable specs are logged and skipped.
pub fn resolve_comment_deps(
    comment_deps: &HashMap<String, Vec<String>>,
    all_statuses: &[(String, String)],
) -> HashMap<String, Vec<String>> {
    if comment_deps.is_empty() {
        return HashMap::new();
    }

    let dummy_dir = Path::new("");

    // Build epic_num → last story key (by document order = position in all_statuses)
    let mut last_story_per_epic: HashMap<u32, String> = HashMap::new();
    for (key, status) in all_statuses {
        if let Some(info) = StoryInfo::from_key_and_status(key, status, dummy_dir) {
            // Later entries overwrite earlier ones → keeps the last story in doc order
            last_story_per_epic.insert(info.epic_num, key.clone());
        }
    }

    // Build story shorthand lookup: "4-1" → "4-1-atomic-state-writer"
    // Uses the `epic_num-story_num` prefix to match full keys.
    let shorthand_lookup: HashMap<String, String> = all_statuses
        .iter()
        .filter_map(|(key, status)| {
            let info = StoryInfo::from_key_and_status(key, status, dummy_dir)?;
            let shorthand = format!("{}-{}", info.epic_num, info.story_num);
            Some((shorthand, key.clone()))
        })
        .collect();

    let mut resolved: HashMap<String, Vec<String>> = HashMap::new();

    for (source_key, specs) in comment_deps {
        let mut deps_for_key: Vec<String> = Vec::new();

        for spec in specs {
            let spec_trimmed = spec.trim();

            if let Some(rest) = spec_trimmed.strip_prefix("epics ") {
                // Range format: "epics 1-6"
                let parts: Vec<&str> = rest.splitn(2, '-').collect();
                if parts.len() == 2
                    && let (Ok(start), Ok(end)) = (
                        parts[0].trim().parse::<u32>(),
                        parts[1].trim().parse::<u32>(),
                    )
                {
                    for epic_n in start..=end {
                        if let Some(last_key) = last_story_per_epic.get(&epic_n)
                            && !deps_for_key.contains(last_key)
                        {
                            deps_for_key.push(last_key.clone());
                        }
                    }
                } else {
                    tracing::debug!(
                        source = %source_key,
                        spec = %spec_trimmed,
                        "Unresolvable epic range in depends-on comment — skipping"
                    );
                }
            } else if let Some(rest) = spec_trimmed.strip_prefix("epic-") {
                // Single epic: "epic-8"
                if let Ok(epic_n) = rest.trim().parse::<u32>() {
                    if let Some(last_key) = last_story_per_epic.get(&epic_n)
                        && !deps_for_key.contains(last_key)
                    {
                        deps_for_key.push(last_key.clone());
                    }
                } else {
                    tracing::debug!(
                        source = %source_key,
                        spec = %spec_trimmed,
                        "Unresolvable epic ref in depends-on comment — skipping"
                    );
                }
            } else {
                // Story shorthand: "4-1" → look up full key
                // Also try as a full key directly (e.g., "4-1-atomic-state-writer")
                if let Some(full_key) = shorthand_lookup.get(spec_trimmed) {
                    if !deps_for_key.contains(full_key) {
                        deps_for_key.push(full_key.clone());
                    }
                } else if all_statuses.iter().any(|(k, _)| k == spec_trimmed) {
                    // Full key match
                    if !deps_for_key.contains(&spec_trimmed.to_string()) {
                        deps_for_key.push(spec_trimmed.to_string());
                    }
                } else {
                    tracing::debug!(
                        source = %source_key,
                        spec = %spec_trimmed,
                        "Unresolvable story ref in depends-on comment — skipping"
                    );
                }
            }
        }

        if !deps_for_key.is_empty() {
            resolved.insert(source_key.clone(), deps_for_key);
        }
    }

    resolved
}

/// Derive dependencies for a set of stories: intra-epic sequential + explicit comment deps.
///
/// **Rules:**
/// 1. Within an epic, story N.M depends on story N.(M-1).
/// 2. `# depends-on:` comment deps are resolved and merged.
///
/// # Arguments
/// * `stories` — Mutable slice of stories; their `dependencies` field will be populated
/// * `all_statuses` — Complete (key, status) pairs from sprint-status.yaml
/// * `comment_deps` — Raw comment dependency specs parsed from YAML comments
pub fn derive_dependencies(
    stories: &mut [StoryInfo],
    all_statuses: &[(String, String)],
    comment_deps: &HashMap<String, Vec<String>>,
) {
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

    // Resolve comment deps once
    let resolved_comment = resolve_comment_deps(comment_deps, all_statuses);

    for story in stories.iter_mut() {
        // Intra-epic sequential: story N.M depends on N.(M-1)
        if story.story_num > 1 {
            let predecessor_key = (story.epic_num, story.story_num - 1);
            if let Some(dep_key) = key_lookup.get(&predecessor_key)
                && !story.dependencies.contains(dep_key)
            {
                story.dependencies.push(dep_key.clone());
            }
        }

        // Cross-epic / explicit comment deps
        if let Some(extra) = resolved_comment.get(&story.story_key) {
            for dep in extra {
                if !story.dependencies.contains(dep) {
                    story.dependencies.push(dep.clone());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Retrospective Gate
// ---------------------------------------------------------------------------

/// Build a map of epic_num → retrospective status for all `epic-X-retrospective` entries.
///
/// Scans the full status list for keys matching `epic-{N}-retrospective` and returns
/// a map from the epic number to its retrospective status string.
pub fn compute_retrospective_gates(statuses: &[(String, String)]) -> HashMap<u32, String> {
    let mut gates = HashMap::new();
    for (key, status) in statuses {
        if let Some(rest) = key.strip_prefix("epic-")
            && let Some(num_str) = rest.strip_suffix("-retrospective")
            && let Ok(epic_num) = num_str.parse::<u32>()
        {
            gates.insert(epic_num, status.clone());
        }
    }
    gates
}

/// Check if all retrospective gates for previous epics are clear.
///
/// Gate statuses:
/// - `"optional"`: gate clear (default/initial state — not yet reviewed)
/// - `"done"`: gate clear (human has validated)
/// - `"review"`: gate **BLOCKED** (daemon activated gate, awaiting human)
/// - absent: gate clear (no retro entry, backward compat)
///
/// Blocks if ANY epic X < story_epic has `epic-X-retrospective: review`.
/// Epic 1 stories are never blocked (no predecessor gate).
pub fn is_retrospective_gate_clear(story_epic: u32, retro_gates: &HashMap<u32, String>) -> bool {
    if story_epic <= 1 {
        return true; // Epic 1 has no predecessor gate
    }
    // Block if ANY previous epic has an active review gate
    retro_gates
        .iter()
        .filter(|(epic_num, _)| **epic_num < story_epic)
        .all(|(_, status)| status != "review")
}

/// Pre-gate filter: resolve dependencies, detect cascade blocks, return eligible stories.
///
/// This is the main entry point for the dependency pre-gate (updated for cascade blocking
/// and cross-epic comment deps). It derives dependencies (intra-epic sequential + explicit
/// comment deps), builds the graph, checks for cycles, detects cascade blocks, and filters
/// out stories with unmet or blocked dependencies.
///
/// Stories are also filtered by the retrospective gate: if `epic-X-retrospective` is
/// `"review"`, all stories from epic X+1 and beyond are excluded.
///
/// # Arguments
/// * `stories` — Eligible stories from the watcher (status == `ready-for-dev`)
/// * `all_statuses` — Complete (key, status) pairs from sprint-status.yaml
/// * `comment_deps` — Raw dependency specs parsed from `# depends-on:` YAML comments
///
/// # Returns
/// Tuple of (filtered eligible stories in topological order, cascade-blocked count).
pub fn filter_eligible(
    mut stories: Vec<StoryInfo>,
    all_statuses: &[(String, String)],
    comment_deps: &HashMap<String, Vec<String>>,
) -> Result<(Vec<StoryInfo>, usize), WatcherError> {
    if stories.is_empty() {
        return Ok((stories, 0));
    }

    // Step 1: Derive dependencies from sprint-status ordering + comment deps
    derive_dependencies(&mut stories, all_statuses, comment_deps);

    // Step 2: Build dependency graph
    let graph = DependencyGraph::new(&stories, all_statuses);

    // Step 3: Topological sort (detects cycles)
    let sorted_keys = graph.topological_sort()?;

    // Step 4: Build full dependency map for transitive cascade detection
    let all_statuses_map: HashMap<String, String> = all_statuses.iter().cloned().collect();
    let full_dep_map = build_full_dependency_map(all_statuses, comment_deps);

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

    // Step 6b: Compute retrospective gates
    let retro_gates = compute_retrospective_gates(all_statuses);

    let mut eligible: Vec<StoryInfo> = Vec::new();
    for key in &sorted_keys {
        // Skip cascade-blocked stories (already logged at warn)
        if cascade_blocked_keys.contains(key) {
            continue;
        }

        let (satisfied, unmet) = graph.deps_satisfied(key);
        if satisfied {
            if let Some(story) = story_map.get(key) {
                // Check retrospective gate: blocked if ANY previous epic-X-retro is "review"
                if !is_retrospective_gate_clear(story.epic_num, &retro_gates) {
                    let blocking_epics: Vec<u32> = retro_gates
                        .iter()
                        .filter(|(epic_num, status)| {
                            **epic_num < story.epic_num && *status == "review"
                        })
                        .map(|(n, _)| *n)
                        .collect();
                    tracing::info!(
                        story_key = %key,
                        epic_num = story.epic_num,
                        blocking_gates = ?blocking_epics,
                        "Story skipped — retrospective gate active (status: review) for epics {:?}",
                        blocking_epics
                    );
                    continue;
                }
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

    eligible.sort_by_key(|s| status_priority(&s.status));

    Ok((eligible, cascade_count))
}

/// Priority order for eligible statuses: resume interrupted work first, then
/// ready stories, then fresh backlog stories.
fn status_priority(status: &str) -> u8 {
    match status {
        "review" => 0,
        "ready-for-dev" => 1,
        "backlog" => 2,
        _ => 3,
    }
}

#[cfg(test)]
#[allow(clippy::needless_pass_by_value)]
mod tests {
    use std::collections::HashMap;

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

        derive_dependencies(&mut stories, &all_statuses, &HashMap::new());

        assert_eq!(stories[0].dependencies, vec!["1-1-scaffolding"]);
        assert_eq!(stories[1].dependencies, vec!["1-2-cli"]);
    }

    #[test]
    fn test_derive_deps_first_story_no_deps() {
        let all_statuses = vec![("1-1-scaffolding".to_string(), "ready-for-dev".to_string())];
        let mut stories = vec![make_story("1-1-scaffolding", "ready-for-dev")];

        derive_dependencies(&mut stories, &all_statuses, &HashMap::new());

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

        derive_dependencies(&mut stories, &all_statuses, &HashMap::new());

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
        derive_dependencies(&mut stories, &all_statuses, &HashMap::new());
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

    // --- filter_eligible tests (updated for tuple return — Story 2.3) ---

    #[test]
    fn test_filter_eligible_returns_stories_with_deps_done() {
        let all_statuses = vec![
            ("1-1-scaffolding".to_string(), "done".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![make_story("1-2-cli", "ready-for-dev")];

        let (result, _cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
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

        let (result, _cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
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

        let (result, _cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
        assert!(
            result.is_empty(),
            "1-2 should be skipped because 1-1 is in-progress, not done"
        );
    }

    #[test]
    fn test_filter_eligible_first_story_always_eligible() {
        let all_statuses = vec![("1-1-scaffolding".to_string(), "ready-for-dev".to_string())];
        let stories = vec![make_story("1-1-scaffolding", "ready-for-dev")];

        let (result, _cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
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

        let (result, _cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
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

        let (result, _cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
        assert_eq!(result.len(), 2);
        // Both should be eligible: 1-3 (deps 1-2 done) and 2-1 (no deps)
        let keys: Vec<&str> = result.iter().map(|s| s.story_key.as_str()).collect();
        assert!(keys.contains(&"1-3-c"));
        assert!(keys.contains(&"2-1-x"));
    }

    #[test]
    fn test_filter_eligible_empty_input_returns_empty() {
        let (result, cascade_count) = filter_eligible(vec![], &[], &HashMap::new()).unwrap();
        assert!(result.is_empty());
        assert_eq!(cascade_count, 0);
    }

    // --- Integration test with Watcher::poll (requires watcher test infra) ---

    #[test]
    fn test_watcher_poll_with_deps_filtering() {
        // Note: poll() passes comment_deps from SprintStatusFile automatically.
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

    // =========================================================================
    // Story 2.3: Cascade blocking tests
    // =========================================================================

    // --- Test helpers for cascade blocking tests ---

    /// Helper: build a HashMap<String, String> from a slice of (&str, &str) pairs.
    /// Reduces boilerplate in cascade blocking tests that construct status maps.
    fn make_statuses_map(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
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

        let all_statuses =
            make_statuses_map(&[("1-1-scaffolding", "blocked"), ("1-2-cli", "ready-for-dev")]);
        let all_deps =
            make_deps_map(&[("1-1-scaffolding", &[]), ("1-2-cli", &["1-1-scaffolding"])]);

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
        assert_eq!(
            blocks[0].chain,
            vec!["1-1-scaffolding", "1-2-cli", "1-3-init"]
        );
    }

    #[test]
    fn test_find_cascade_needs_clarification() {
        let mut story = make_story("1-2-cli", "ready-for-dev");
        story.dependencies = vec!["1-1-scaffolding".to_string()];

        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "needs-clarification"),
            ("1-2-cli", "ready-for-dev"),
        ]);
        let all_deps =
            make_deps_map(&[("1-1-scaffolding", &[]), ("1-2-cli", &["1-1-scaffolding"])]);

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
        let all_deps =
            make_deps_map(&[("1-1-scaffolding", &[]), ("1-2-cli", &["1-1-scaffolding"])]);

        let blocks = find_cascade_blocks(&[story], &all_statuses, &all_deps);
        assert!(
            blocks.is_empty(),
            "in-progress is transient, not a cascade blocker"
        );
    }

    #[test]
    fn test_find_cascade_no_cascade_on_review() {
        // "review" is a transient status (code review in progress) — NOT a blocker
        let mut story = make_story("1-2-cli", "ready-for-dev");
        story.dependencies = vec!["1-1-scaffolding".to_string()];

        let all_statuses =
            make_statuses_map(&[("1-1-scaffolding", "review"), ("1-2-cli", "ready-for-dev")]);
        let all_deps =
            make_deps_map(&[("1-1-scaffolding", &[]), ("1-2-cli", &["1-1-scaffolding"])]);

        let blocks = find_cascade_blocks(&[story], &all_statuses, &all_deps);
        assert!(
            blocks.is_empty(),
            "review is transient, not a cascade blocker"
        );
    }

    #[test]
    fn test_find_cascade_no_blocks_when_all_done() {
        let mut story = make_story("1-2-cli", "ready-for-dev");
        story.dependencies = vec!["1-1-scaffolding".to_string()];

        let all_statuses =
            make_statuses_map(&[("1-1-scaffolding", "done"), ("1-2-cli", "ready-for-dev")]);

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
        assert!(
            blocks.is_empty(),
            "Unknown dep is unmet but not cascade-blocked"
        );
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

        let (result, cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
        // 1-2 is cascade-blocked (dep 1-1 is "blocked")
        // 2-1 has no deps → eligible
        assert_eq!(cascade_count, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].story_key, "2-1-polling");
    }

    #[test]
    fn test_filter_eligible_returns_cascade_count() {
        let all_statuses = vec![
            (
                "1-1-scaffolding".to_string(),
                "needs-clarification".to_string(),
            ),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
            ("1-3-init".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![
            make_story("1-2-cli", "ready-for-dev"),
            make_story("1-3-init", "ready-for-dev"),
        ];

        let (result, cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
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
        let (result1, cc1) =
            filter_eligible(stories1, &all_statuses_blocked, &HashMap::new()).unwrap();
        assert!(result1.is_empty());
        assert_eq!(cc1, 1);

        // Second call: 1-1 is now done → 1-2 becomes eligible
        let all_statuses_resolved = vec![
            ("1-1-scaffolding".to_string(), "done".to_string()),
            ("1-2-cli".to_string(), "ready-for-dev".to_string()),
        ];
        let stories2 = vec![make_story("1-2-cli", "ready-for-dev")];
        let (result2, cc2) =
            filter_eligible(stories2, &all_statuses_resolved, &HashMap::new()).unwrap();
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

        let (result, cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
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

        let config =
            std::sync::Arc::new(crate::watcher::tests::make_test_bot_config(artifacts_dir));
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

        let dep_map = build_full_dependency_map(&all_statuses, &HashMap::new());

        // Skips epics and retrospectives
        assert!(!dep_map.contains_key("epic-1"));
        assert!(!dep_map.contains_key("epic-1-retrospective"));
        assert!(!dep_map.contains_key("epic-2"));

        // First story in each epic has no deps
        assert_eq!(
            dep_map.get("1-1-scaffolding").unwrap(),
            &Vec::<String>::new()
        );
        assert_eq!(dep_map.get("2-1-polling").unwrap(), &Vec::<String>::new());

        // Sequential deps within epic
        assert_eq!(
            dep_map.get("1-2-cli").unwrap(),
            &vec!["1-1-scaffolding".to_string()]
        );
        assert_eq!(
            dep_map.get("1-3-init").unwrap(),
            &vec!["1-2-cli".to_string()]
        );
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
        assert_eq!(
            chain,
            vec!["1-1-scaffolding", "1-2-cli", "1-3-init", "1-4-status"]
        );
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
        let all_statuses = make_statuses_map(&[("1-1-scaffolding", "in-progress")]);

        let all_deps: HashMap<String, Vec<String>> = HashMap::new();

        let result = find_root_blocker(&["1-1-scaffolding".to_string()], &all_statuses, &all_deps);
        assert!(result.is_none(), "in-progress is not a blocker");
    }

    #[test]
    fn test_find_root_blocker_finds_transitive_blocker() {
        let all_statuses = make_statuses_map(&[
            ("1-1-scaffolding", "needs-clarification"),
            ("1-2-cli", "ready-for-dev"),
        ]);
        let all_deps = make_deps_map(&[("1-2-cli", &["1-1-scaffolding"])]);

        let result = find_root_blocker(&["1-2-cli".to_string()], &all_statuses, &all_deps);
        assert!(result.is_some());
        let (root, status) = result.unwrap();
        assert_eq!(root, "1-1-scaffolding");
        assert_eq!(status, "needs-clarification");
    }

    // --- CascadeBlockInfo Display test ---

    #[test]
    fn test_cascade_block_info_display() {
        let info = CascadeBlockInfo {
            blocked_story: "1-3-init".to_string(),
            root_cause_story: "1-1-scaffolding".to_string(),
            root_cause_status: "blocked".to_string(),
            chain: vec![
                "1-1-scaffolding".to_string(),
                "1-2-cli".to_string(),
                "1-3-init".to_string(),
            ],
        };
        let display = format!("{info}");
        assert!(display.contains("1-3-init cascade-blocked by 1-1-scaffolding"));
        assert!(display.contains("status: blocked"));
        assert!(display.contains("1-1-scaffolding → 1-2-cli → 1-3-init"));
    }

    // --- superseded status tests ---

    #[test]
    fn test_deps_satisfied_when_dep_is_superseded() {
        let stories = vec![
            make_story("1-1-old-approach", "superseded"),
            make_story("1-2-next-step", "ready-for-dev"),
        ];
        let all_statuses = vec![
            ("1-1-old-approach".into(), "superseded".into()),
            ("1-2-next-step".into(), "ready-for-dev".into()),
        ];
        let mut stories_with_deps = stories.clone();
        derive_dependencies(&mut stories_with_deps, &all_statuses, &HashMap::new());
        let graph = DependencyGraph::new(&stories_with_deps, &all_statuses);

        let (satisfied, unmet) = graph.deps_satisfied("1-2-next-step");
        assert!(satisfied, "superseded dep should satisfy dependency");
        assert!(unmet.is_none());
    }

    #[test]
    fn test_filter_eligible_includes_story_after_superseded_dep() {
        let stories = vec![
            make_story("1-1-old-approach", "superseded"),
            make_story("1-2-next-step", "ready-for-dev"),
        ];
        let all_statuses = vec![
            ("epic-1".into(), "in-progress".into()),
            ("1-1-old-approach".into(), "superseded".into()),
            ("1-2-next-step".into(), "ready-for-dev".into()),
        ];
        let (eligible, _) = filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
        let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
        assert!(
            keys.contains(&"1-2-next-step"),
            "story after superseded dep should be eligible, got: {:?}",
            keys
        );
    }

    #[test]
    fn test_find_cascade_stops_at_superseded() {
        // 1-1 is blocked, 1-2 is superseded (resolved), 1-3 depends on 1-2.
        // Cascade should NOT propagate through superseded — 1-3 is safe.
        let stories = vec![
            make_story("1-1-broken", "blocked"),
            make_story("1-2-replaced", "superseded"),
            make_story("1-3-next", "ready-for-dev"),
        ];
        let all_statuses_map: HashMap<String, String> = vec![
            ("1-1-broken".into(), "blocked".into()),
            ("1-2-replaced".into(), "superseded".into()),
            ("1-3-next".into(), "ready-for-dev".into()),
        ]
        .into_iter()
        .collect();
        let full_dep_map: HashMap<String, Vec<String>> = vec![
            ("1-2-replaced".into(), vec!["1-1-broken".into()]),
            ("1-3-next".into(), vec!["1-2-replaced".into()]),
        ]
        .into_iter()
        .collect();

        let cascades = find_cascade_blocks(&stories, &all_statuses_map, &full_dep_map);
        let blocked_keys: Vec<&str> = cascades
            .iter()
            .map(|cb| cb.blocked_story.as_str())
            .collect();
        assert!(
            !blocked_keys.contains(&"1-3-next"),
            "superseded should stop cascade propagation, but 1-3-next was blocked: {:?}",
            cascades
        );
    }

    #[test]
    fn test_filter_eligible_superseded_mixed_with_done() {
        // 1-1 done, 1-2 superseded, 1-3 ready-for-dev — both deps resolved
        let stories = vec![
            make_story("1-1-first", "done"),
            make_story("1-2-replaced", "superseded"),
            make_story("1-3-final", "ready-for-dev"),
        ];
        let all_statuses = vec![
            ("epic-1".into(), "in-progress".into()),
            ("1-1-first".into(), "done".into()),
            ("1-2-replaced".into(), "superseded".into()),
            ("1-3-final".into(), "ready-for-dev".into()),
        ];
        let (eligible, cascade_count) =
            filter_eligible(stories, &all_statuses, &HashMap::new()).unwrap();
        assert_eq!(cascade_count, 0);
        let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();
        assert!(
            keys.contains(&"1-3-final"),
            "story after done + superseded deps should be eligible, got: {:?}",
            keys
        );
    }

    #[test]
    fn test_deps_satisfied_when_dep_is_absorbed() {
        let stories = vec![
            make_story("1-1-absorbed-story", "absorbed"),
            make_story("1-2-next-step", "ready-for-dev"),
        ];
        let all_statuses = vec![
            ("1-1-absorbed-story".into(), "absorbed".into()),
            ("1-2-next-step".into(), "ready-for-dev".into()),
        ];
        let mut stories_with_deps = stories.clone();
        derive_dependencies(&mut stories_with_deps, &all_statuses, &HashMap::new());
        let graph = DependencyGraph::new(&stories_with_deps, &all_statuses);

        let (satisfied, unmet) = graph.deps_satisfied("1-2-next-step");
        assert!(satisfied, "absorbed dep should satisfy dependency");
        assert!(unmet.is_none());
    }

    #[test]
    // -----------------------------------------------------------------------
    // resolve_comment_deps tests
    // -----------------------------------------------------------------------
    #[test]
    fn test_resolve_comment_deps_story_shorthand() {
        let all_statuses = vec![
            ("1-1-scaffolding".into(), "done".into()),
            ("1-2-cli".into(), "ready-for-dev".into()),
        ];
        let mut comment = HashMap::new();
        comment.insert("1-2-cli".to_string(), vec!["1-1".to_string()]);

        let resolved = resolve_comment_deps(&comment, &all_statuses);
        assert_eq!(
            resolved.get("1-2-cli").unwrap(),
            &vec!["1-1-scaffolding".to_string()]
        );
    }

    #[test]
    fn test_resolve_comment_deps_epic_ref() {
        let all_statuses = vec![
            ("epic-1".into(), "done".into()),
            ("1-1-scaffolding".into(), "done".into()),
            ("1-2-cli".into(), "done".into()),
            ("epic-2".into(), "in-progress".into()),
            ("2-1-auth".into(), "ready-for-dev".into()),
        ];
        let mut comment = HashMap::new();
        comment.insert("2-1-auth".to_string(), vec!["epic-1".to_string()]);

        let resolved = resolve_comment_deps(&comment, &all_statuses);
        // Last story of epic 1 is 1-2-cli
        assert_eq!(
            resolved.get("2-1-auth").unwrap(),
            &vec!["1-2-cli".to_string()]
        );
    }

    #[test]
    fn test_resolve_comment_deps_epic_range() {
        let all_statuses = vec![
            ("epic-1".into(), "done".into()),
            ("1-1-scaffolding".into(), "done".into()),
            ("1-2-cli".into(), "done".into()),
            ("epic-2".into(), "done".into()),
            ("2-1-auth".into(), "done".into()),
            ("2-2-sessions".into(), "done".into()),
            ("epic-3".into(), "in-progress".into()),
            ("3-1-data".into(), "ready-for-dev".into()),
        ];
        let mut comment = HashMap::new();
        comment.insert("3-1-data".to_string(), vec!["epics 1-2".to_string()]);

        let resolved = resolve_comment_deps(&comment, &all_statuses);
        let deps = resolved.get("3-1-data").unwrap();
        assert!(deps.contains(&"1-2-cli".to_string()));
        assert!(deps.contains(&"2-2-sessions".to_string()));
        assert_eq!(deps.len(), 2);
    }

    #[test]
    fn test_resolve_comment_deps_unknown_spec_skipped() {
        let all_statuses = vec![("1-1-scaffolding".into(), "done".into())];
        let mut comment = HashMap::new();
        comment.insert(
            "1-1-scaffolding".to_string(),
            vec!["nonexistent".to_string()],
        );

        let resolved = resolve_comment_deps(&comment, &all_statuses);
        assert!(resolved.is_empty());
    }

    #[test]
    fn test_resolve_comment_deps_empty_input() {
        let all_statuses = vec![("1-1-scaffolding".into(), "done".into())];
        let resolved = resolve_comment_deps(&HashMap::new(), &all_statuses);
        assert!(resolved.is_empty());
    }

    #[test]
    fn test_filter_eligible_with_cross_epic_comment_dep() {
        // 2-1 depends on epic-1 (last story = 1-2). Epic 1 not done → 2-1 not eligible.
        let stories = vec![
            make_story("1-1-scaffolding", "ready-for-dev"),
            make_story("2-1-auth", "ready-for-dev"),
        ];
        let all_statuses = vec![
            ("epic-1".into(), "in-progress".into()),
            ("1-1-scaffolding".into(), "ready-for-dev".into()),
            ("1-2-cli".into(), "ready-for-dev".into()),
            ("epic-2".into(), "in-progress".into()),
            ("2-1-auth".into(), "ready-for-dev".into()),
        ];
        let mut comment = HashMap::new();
        comment.insert("2-1-auth".to_string(), vec!["epic-1".to_string()]);

        let (eligible, _) = filter_eligible(stories, &all_statuses, &comment).unwrap();
        // Only 1-1 should be eligible; 2-1 is blocked by epic-1 (1-2 not done)
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].story_key, "1-1-scaffolding");
    }

    #[test]
    fn test_filter_eligible_cross_epic_dep_satisfied() {
        // 2-1 depends on epic-1 (last story = 1-2). Epic 1 all done → 2-1 eligible.
        let stories = vec![make_story("2-1-auth", "ready-for-dev")];
        let all_statuses = vec![
            ("epic-1".into(), "done".into()),
            ("1-1-scaffolding".into(), "done".into()),
            ("1-2-cli".into(), "done".into()),
            ("epic-2".into(), "in-progress".into()),
            ("2-1-auth".into(), "ready-for-dev".into()),
        ];
        let mut comment = HashMap::new();
        comment.insert("2-1-auth".to_string(), vec!["epic-1".to_string()]);

        let (eligible, _) = filter_eligible(stories, &all_statuses, &comment).unwrap();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].story_key, "2-1-auth");
    }

    #[test]
    fn test_filter_eligible_cross_epic_cascade_block() {
        // 2-1 depends on epic-1 (last story = 1-2). 1-1 is blocked → 1-2 cascade → 2-1 cascade.
        let stories = vec![
            make_story("1-2-cli", "ready-for-dev"),
            make_story("2-1-auth", "ready-for-dev"),
        ];
        let all_statuses = vec![
            ("epic-1".into(), "in-progress".into()),
            ("1-1-scaffolding".into(), "blocked".into()),
            ("1-2-cli".into(), "ready-for-dev".into()),
            ("epic-2".into(), "in-progress".into()),
            ("2-1-auth".into(), "ready-for-dev".into()),
        ];
        let mut comment = HashMap::new();
        comment.insert("2-1-auth".to_string(), vec!["epic-1".to_string()]);

        let (eligible, cascade) = filter_eligible(stories, &all_statuses, &comment).unwrap();
        assert!(eligible.is_empty());
        assert!(cascade >= 2); // 1-2 cascade + 2-1 cascade
    }

    #[test]
    fn test_filter_eligible_multi_epic_comma_deps_autoscalp_scenario() {
        // Exact autoscalp3000 scenario:
        //   8-1 depends-on: epic-3, epic-5
        //   9-1 depends-on: epic-7, epic-8
        // When epics 3 and 5 are NOT done, 8-1 should be blocked.
        // When epics 3 and 5 ARE done but epic-7 isn't, 9-1 still blocked.
        let all_statuses = vec![
            ("epic-3".into(), "in-progress".into()),
            ("3-1-candles".into(), "done".into()),
            ("3-2-indicators".into(), "done".into()),
            ("3-3-regime".into(), "ready-for-dev".into()), // not done!
            ("epic-5".into(), "in-progress".into()),
            ("5-1-backtest".into(), "done".into()),
            ("5-2-paper".into(), "done".into()),
            ("5-3-live".into(), "ready-for-dev".into()), // not done!
            ("epic-7".into(), "in-progress".into()),
            ("7-1-grid".into(), "ready-for-dev".into()),
            ("epic-8".into(), "backlog".into()),
            ("8-1-walkforward".into(), "ready-for-dev".into()),
            ("8-2-scorer".into(), "ready-for-dev".into()),
            ("epic-9".into(), "backlog".into()),
            ("9-1-logging".into(), "ready-for-dev".into()),
            ("9-2-telegram".into(), "ready-for-dev".into()),
        ];

        let stories = vec![
            make_story("3-3-regime", "ready-for-dev"),
            make_story("5-3-live", "ready-for-dev"),
            make_story("7-1-grid", "ready-for-dev"),
            make_story("8-1-walkforward", "ready-for-dev"),
            make_story("8-2-scorer", "ready-for-dev"),
            make_story("9-1-logging", "ready-for-dev"),
            make_story("9-2-telegram", "ready-for-dev"),
        ];

        let mut comment = HashMap::new();
        comment.insert(
            "8-1-walkforward".to_string(),
            vec!["epic-3".to_string(), "epic-5".to_string()],
        );
        comment.insert(
            "9-1-logging".to_string(),
            vec!["epic-7".to_string(), "epic-8".to_string()],
        );

        let (eligible, _) = filter_eligible(stories, &all_statuses, &comment).unwrap();
        let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

        // 8-1 blocked: epic-3 last story (3-3) not done, epic-5 last story (5-3) not done
        // 8-2 blocked: intra-epic dep on 8-1
        // 9-1 blocked: epic-7 last story (7-1) not done, epic-8 last story (8-2) not done
        // 9-2 blocked: intra-epic dep on 9-1
        // Only 3-3, 5-3, 7-1 should be eligible (first unfinished in their epics)
        assert!(
            !keys.contains(&"8-1-walkforward"),
            "8-1 should be blocked (epic-3 + epic-5 not done)"
        );
        assert!(
            !keys.contains(&"9-1-logging"),
            "9-1 should be blocked (epic-7 + epic-8 not done)"
        );
        assert!(
            keys.contains(&"3-3-regime"),
            "3-3 should be eligible (intra-epic dep 3-2 done)"
        );
        assert!(
            keys.contains(&"5-3-live"),
            "5-3 should be eligible (intra-epic dep 5-2 done)"
        );
        assert!(
            keys.contains(&"7-1-grid"),
            "7-1 should be eligible (no deps)"
        );
    }

    #[test]
    fn test_filter_eligible_multi_epic_deps_all_satisfied() {
        // Same scenario but epics 3 and 5 are fully done → 8-1 becomes eligible.
        let all_statuses = vec![
            ("epic-3".into(), "done".into()),
            ("3-1-candles".into(), "done".into()),
            ("3-2-indicators".into(), "done".into()),
            ("3-3-regime".into(), "done".into()),
            ("epic-5".into(), "done".into()),
            ("5-1-backtest".into(), "done".into()),
            ("5-2-paper".into(), "done".into()),
            ("5-3-live".into(), "done".into()),
            ("epic-7".into(), "in-progress".into()),
            ("7-1-grid".into(), "ready-for-dev".into()),
            ("epic-8".into(), "in-progress".into()),
            ("8-1-walkforward".into(), "ready-for-dev".into()),
            ("8-2-scorer".into(), "ready-for-dev".into()),
            ("epic-9".into(), "backlog".into()),
            ("9-1-logging".into(), "ready-for-dev".into()),
        ];

        let stories = vec![
            make_story("7-1-grid", "ready-for-dev"),
            make_story("8-1-walkforward", "ready-for-dev"),
            make_story("8-2-scorer", "ready-for-dev"),
            make_story("9-1-logging", "ready-for-dev"),
        ];

        let mut comment = HashMap::new();
        comment.insert(
            "8-1-walkforward".to_string(),
            vec!["epic-3".to_string(), "epic-5".to_string()],
        );
        comment.insert(
            "9-1-logging".to_string(),
            vec!["epic-7".to_string(), "epic-8".to_string()],
        );

        let (eligible, _) = filter_eligible(stories, &all_statuses, &comment).unwrap();
        let keys: Vec<&str> = eligible.iter().map(|s| s.story_key.as_str()).collect();

        // epic-3 done, epic-5 done → 8-1 eligible
        assert!(
            keys.contains(&"8-1-walkforward"),
            "8-1 should be eligible (epic-3 + epic-5 both done)"
        );
        // 8-2 blocked: intra-epic dep on 8-1 (not done yet, just ready-for-dev)
        assert!(
            !keys.contains(&"8-2-scorer"),
            "8-2 should be blocked (dep 8-1 not done)"
        );
        // 9-1 blocked: epic-7 (7-1 not done), epic-8 (8-2 not done)
        assert!(
            !keys.contains(&"9-1-logging"),
            "9-1 should be blocked (epic-7 + epic-8 not done)"
        );
        // 7-1 eligible (no deps)
        assert!(keys.contains(&"7-1-grid"), "7-1 should be eligible");
    }

    // -----------------------------------------------------------------------
    // Retrospective gate tests (Task 3 — Story 4.8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_compute_retrospective_gates_extracts_entries() {
        let statuses = vec![
            ("epic-1".to_string(), "done".to_string()),
            ("1-1-scaffolding".to_string(), "done".to_string()),
            ("epic-1-retrospective".to_string(), "optional".to_string()),
            ("epic-2".to_string(), "done".to_string()),
            ("2-1-polling".to_string(), "done".to_string()),
            ("epic-2-retrospective".to_string(), "review".to_string()),
            ("epic-3".to_string(), "in-progress".to_string()),
            ("3-1-story".to_string(), "done".to_string()),
            ("epic-3-retrospective".to_string(), "done".to_string()),
        ];
        let gates = compute_retrospective_gates(&statuses);
        assert_eq!(gates.len(), 3);
        assert_eq!(gates.get(&1).unwrap(), "optional");
        assert_eq!(gates.get(&2).unwrap(), "review");
        assert_eq!(gates.get(&3).unwrap(), "done");
    }

    #[test]
    fn test_compute_retrospective_gates_empty_when_no_entries() {
        let statuses = vec![
            ("epic-1".to_string(), "done".to_string()),
            ("1-1-scaffolding".to_string(), "done".to_string()),
        ];
        let gates = compute_retrospective_gates(&statuses);
        assert!(gates.is_empty());
    }

    #[test]
    fn test_is_retrospective_gate_clear_epic_1_always_clear() {
        let gates = HashMap::new();
        assert!(is_retrospective_gate_clear(1, &gates));

        // Even if there were a hypothetical gate at epic 0, epic 1 is always clear
        let mut gates_with_zero = HashMap::new();
        gates_with_zero.insert(0, "review".to_string());
        assert!(is_retrospective_gate_clear(1, &gates_with_zero));
    }

    #[test]
    fn test_is_retrospective_gate_clear_review_blocks() {
        let mut gates = HashMap::new();
        gates.insert(2, "review".to_string());

        // Epic 3 stories blocked by epic-2-retrospective: review
        assert!(!is_retrospective_gate_clear(3, &gates));
        // Epic 4 stories also blocked — any previous epic in review blocks all later epics
        assert!(!is_retrospective_gate_clear(4, &gates));
        // Epic 5 stories also blocked
        assert!(!is_retrospective_gate_clear(5, &gates));
    }

    #[test]
    fn test_is_retrospective_gate_clear_done_allows() {
        let mut gates = HashMap::new();
        gates.insert(2, "done".to_string());

        assert!(is_retrospective_gate_clear(3, &gates));
    }

    #[test]
    fn test_is_retrospective_gate_clear_optional_allows() {
        let mut gates = HashMap::new();
        gates.insert(2, "optional".to_string());

        // CRITICAL: optional must NOT block — backward compatibility
        assert!(is_retrospective_gate_clear(3, &gates));
    }

    #[test]
    fn test_is_retrospective_gate_clear_absent_allows() {
        let gates = HashMap::new();

        // No retro entry at all → no gate → clear
        assert!(is_retrospective_gate_clear(3, &gates));
    }

    #[test]
    fn test_is_retrospective_gate_blocks_current_epic_not_own() {
        let mut gates = HashMap::new();
        gates.insert(3, "review".to_string());

        // Epic 3 stories are NOT blocked by their own retro gate
        assert!(is_retrospective_gate_clear(3, &gates));
        // Epic 4 stories ARE blocked
        assert!(!is_retrospective_gate_clear(4, &gates));
        // Epic 5 stories are also blocked (any previous epic in review)
        assert!(!is_retrospective_gate_clear(5, &gates));
    }

    #[test]
    fn test_is_retrospective_gate_clear_skips_two_epics() {
        // epic-2-retrospective: review should block epic 4 stories even though
        // epic-3-retrospective is optional (the old N-1 logic would miss this)
        let mut gates = HashMap::new();
        gates.insert(2, "review".to_string());
        gates.insert(3, "optional".to_string());

        assert!(!is_retrospective_gate_clear(4, &gates));
    }

    #[test]
    fn test_is_retrospective_gate_clear_all_done_allows() {
        let mut gates = HashMap::new();
        gates.insert(1, "done".to_string());
        gates.insert(2, "done".to_string());
        gates.insert(3, "done".to_string());

        assert!(is_retrospective_gate_clear(4, &gates));
    }

    #[test]
    fn test_filter_eligible_respects_retrospective_gate_review_blocks() {
        let stories = vec![make_story("3-1-story-a", "ready-for-dev")];
        let statuses: Vec<(String, String)> = vec![
            ("epic-2".to_string(), "done".to_string()),
            ("2-1-last".to_string(), "done".to_string()),
            ("epic-2-retrospective".to_string(), "review".to_string()),
            ("epic-3".to_string(), "in-progress".to_string()),
            ("3-1-story-a".to_string(), "ready-for-dev".to_string()),
        ];
        let comment_deps = HashMap::new();
        let (eligible, _) = filter_eligible(stories, &statuses, &comment_deps).unwrap();
        // Epic 3 story should be blocked by epic-2-retrospective: review
        assert!(
            eligible.is_empty(),
            "Expected no eligible stories when retro gate is active"
        );
    }

    #[test]
    fn test_filter_eligible_respects_retrospective_gate_optional_allows() {
        let stories = vec![make_story("3-1-story-a", "ready-for-dev")];
        let statuses: Vec<(String, String)> = vec![
            ("epic-2".to_string(), "done".to_string()),
            ("2-1-last".to_string(), "done".to_string()),
            ("epic-2-retrospective".to_string(), "optional".to_string()),
            ("epic-3".to_string(), "in-progress".to_string()),
            ("3-1-story-a".to_string(), "ready-for-dev".to_string()),
        ];
        let comment_deps = HashMap::new();
        let (eligible, _) = filter_eligible(stories, &statuses, &comment_deps).unwrap();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].story_key, "3-1-story-a");
    }

    #[test]
    fn test_filter_eligible_respects_retrospective_gate_done_allows() {
        let stories = vec![make_story("3-1-story-a", "ready-for-dev")];
        let statuses: Vec<(String, String)> = vec![
            ("epic-2".to_string(), "done".to_string()),
            ("2-1-last".to_string(), "done".to_string()),
            ("epic-2-retrospective".to_string(), "done".to_string()),
            ("epic-3".to_string(), "in-progress".to_string()),
            ("3-1-story-a".to_string(), "ready-for-dev".to_string()),
        ];
        let comment_deps = HashMap::new();
        let (eligible, _) = filter_eligible(stories, &statuses, &comment_deps).unwrap();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].story_key, "3-1-story-a");
    }

    #[test]
    fn test_filter_eligible_retro_gate_does_not_block_current_epic() {
        // epic-3-retrospective: review should NOT block epic 3 stories
        let stories = vec![make_story("3-2-story-b", "ready-for-dev")];
        let statuses: Vec<(String, String)> = vec![
            ("epic-3".to_string(), "in-progress".to_string()),
            ("3-1-story-a".to_string(), "done".to_string()),
            ("3-2-story-b".to_string(), "ready-for-dev".to_string()),
            ("epic-3-retrospective".to_string(), "review".to_string()),
        ];
        let comment_deps = HashMap::new();
        let (eligible, _) = filter_eligible(stories, &statuses, &comment_deps).unwrap();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].story_key, "3-2-story-b");
    }

    #[test]
    fn test_filter_eligible_epic4_blocked_by_epic2_gate_skipping_epic3() {
        // Regression test: epic-2-retrospective: review must block epic 4 stories
        // even when epic-3-retrospective is optional (old N-1 logic missed this)
        let stories = vec![make_story("4-1-story-a", "ready-for-dev")];
        let statuses: Vec<(String, String)> = vec![
            ("epic-2".to_string(), "done".to_string()),
            ("2-1-last".to_string(), "done".to_string()),
            ("epic-2-retrospective".to_string(), "review".to_string()),
            ("epic-3".to_string(), "in-progress".to_string()),
            ("3-1-story-b".to_string(), "done".to_string()),
            ("epic-3-retrospective".to_string(), "optional".to_string()),
            ("epic-4".to_string(), "in-progress".to_string()),
            ("4-1-story-a".to_string(), "ready-for-dev".to_string()),
        ];
        let comment_deps = HashMap::new();
        let (eligible, _) = filter_eligible(stories, &statuses, &comment_deps).unwrap();
        assert!(
            eligible.is_empty(),
            "Epic 4 story should be blocked by epic-2-retrospective: review"
        );
    }

    #[test]
    fn test_filter_eligible_incomplete_epic_optional_retro_allows_next_epic() {
        // Even if epic 2 retro is optional and epic 2 is not fully done yet,
        // epic 3 stories should be eligible if their deps are satisfied
        let stories = vec![make_story("3-1-story-a", "ready-for-dev")];
        let statuses: Vec<(String, String)> = vec![
            ("epic-2".to_string(), "in-progress".to_string()),
            ("2-1-polling".to_string(), "done".to_string()),
            ("2-2-deps".to_string(), "in-progress".to_string()),
            ("epic-2-retrospective".to_string(), "optional".to_string()),
            ("epic-3".to_string(), "in-progress".to_string()),
            ("3-1-story-a".to_string(), "ready-for-dev".to_string()),
        ];
        let comment_deps = HashMap::new();
        let (eligible, _) = filter_eligible(stories, &statuses, &comment_deps).unwrap();
        assert_eq!(eligible.len(), 1);
    }

    #[test]
    fn test_find_cascade_stops_at_absorbed() {
        let stories = vec![
            make_story("1-1-broken", "blocked"),
            make_story("1-2-absorbed", "absorbed"),
            make_story("1-3-next", "ready-for-dev"),
        ];
        let all_statuses_map: HashMap<String, String> = vec![
            ("1-1-broken".into(), "blocked".into()),
            ("1-2-absorbed".into(), "absorbed".into()),
            ("1-3-next".into(), "ready-for-dev".into()),
        ]
        .into_iter()
        .collect();
        let full_dep_map: HashMap<String, Vec<String>> = vec![
            ("1-2-absorbed".into(), vec!["1-1-broken".into()]),
            ("1-3-next".into(), vec!["1-2-absorbed".into()]),
        ]
        .into_iter()
        .collect();

        let cascades = find_cascade_blocks(&stories, &all_statuses_map, &full_dep_map);
        let blocked_keys: Vec<&str> = cascades
            .iter()
            .map(|cb| cb.blocked_story.as_str())
            .collect();
        assert!(
            !blocked_keys.contains(&"1-3-next"),
            "absorbed should stop cascade propagation, but 1-3-next was blocked: {:?}",
            cascades
        );
    }

    // --- status priority sorting tests (Story 13.1) ---

    #[test]
    fn test_filter_eligible_priority_review_before_ready_for_dev() {
        let all_statuses = vec![
            ("epic-1".to_string(), "done".to_string()),
            ("1-1-foo".to_string(), "done".to_string()),
            ("epic-2".to_string(), "done".to_string()),
            ("2-1-bar".to_string(), "ready-for-dev".to_string()),
            ("epic-3".to_string(), "done".to_string()),
            ("3-1-baz".to_string(), "review".to_string()),
        ];
        let stories = vec![
            make_story("2-1-bar", "ready-for-dev"),
            make_story("3-1-baz", "review"),
        ];
        let comment_deps = HashMap::new();
        let (result, _) = filter_eligible(stories, &all_statuses, &comment_deps).unwrap();
        assert_eq!(
            result[0].story_key, "3-1-baz",
            "review must come before ready-for-dev"
        );
        assert_eq!(result[1].story_key, "2-1-bar");
    }

    #[test]
    fn test_filter_eligible_priority_ready_for_dev_before_backlog() {
        let all_statuses = vec![
            ("epic-1".to_string(), "done".to_string()),
            ("1-1-foo".to_string(), "done".to_string()),
            ("epic-2".to_string(), "done".to_string()),
            ("2-1-bar".to_string(), "backlog".to_string()),
            ("epic-3".to_string(), "done".to_string()),
            ("3-1-baz".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![
            make_story("2-1-bar", "backlog"),
            make_story("3-1-baz", "ready-for-dev"),
        ];
        let comment_deps = HashMap::new();
        let (result, _) = filter_eligible(stories, &all_statuses, &comment_deps).unwrap();
        assert_eq!(
            result[0].story_key, "3-1-baz",
            "ready-for-dev must come before backlog"
        );
        assert_eq!(result[1].story_key, "2-1-bar");
    }

    #[test]
    fn test_filter_eligible_priority_preserves_order_within_group() {
        let all_statuses = vec![
            ("epic-1".to_string(), "done".to_string()),
            ("1-1-foo".to_string(), "done".to_string()),
            ("epic-2".to_string(), "done".to_string()),
            ("2-1-bar".to_string(), "ready-for-dev".to_string()),
            ("epic-3".to_string(), "done".to_string()),
            ("3-1-baz".to_string(), "ready-for-dev".to_string()),
        ];
        let stories = vec![
            make_story("2-1-bar", "ready-for-dev"),
            make_story("3-1-baz", "ready-for-dev"),
        ];
        let comment_deps = HashMap::new();
        let (result, _) = filter_eligible(stories, &all_statuses, &comment_deps).unwrap();
        assert_eq!(
            result[0].story_key, "2-1-bar",
            "document order preserved within group"
        );
        assert_eq!(result[1].story_key, "3-1-baz");
    }

    #[test]
    fn test_backlog_story_deps_enforced() {
        let all_statuses = vec![
            ("epic-1".to_string(), "in-progress".to_string()),
            ("1-1-foo".to_string(), "in-progress".to_string()),
            ("1-2-bar".to_string(), "backlog".to_string()),
        ];
        let stories = vec![make_story("1-2-bar", "backlog")];
        let comment_deps = HashMap::new();
        let (result, _) = filter_eligible(stories, &all_statuses, &comment_deps).unwrap();
        assert!(
            result.is_empty(),
            "backlog story with unmet dep should be filtered out"
        );
    }
}
