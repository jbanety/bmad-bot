# Story 7.3: Watcher → Dependency Resolution → Story Selection Integration Tests

Status: review

## Story

As a developer,
I want integration tests that verify the full watcher → deps → eligible story selection chain,
So that I'm confident the daemon picks the right stories in the right order.

## Acceptance Criteria

1. **Given** a temp directory with a `sprint-status.yaml` containing 5 stories:
   - Story 1-1: `done`
   - Story 1-2: `ready-for-dev`, depends on 1-1
   - Story 1-3: `ready-for-dev`, depends on 1-2
   - Story 2-1: `ready-for-dev`, no deps
   - Story 2-2: `backlog`
   **When** the watcher polls and deps resolution runs
   **Then** eligible stories returned are `[1-2, 2-1]` (1-1 is done, 1-3's dep not met, 2-2 not ready)
   **And** stories are returned in dependency-valid order

2. **Given** a `sprint-status.yaml` where story 1-1 has status `blocked`
   **When** cascade blocking runs for stories depending on 1-1
   **Then** story 1-2 (depends on 1-1) is marked as cascade-blocked
   **And** story 1-3 (transitive dependency through 1-2) is also cascade-blocked

3. **Given** a `sprint-status.yaml` where ALL stories are `done`
   **When** the watcher polls
   **Then** an empty eligible list (or `NoEligibleStories` error) is returned

4. **Given** a `sprint-status.yaml` with circular dependencies (1-1 depends on 1-2, 1-2 depends on 1-1)
   **When** the dependency resolution runs
   **Then** the system handles this gracefully (no infinite loop, both stories skipped or error reported)

5. **Given** a missing `sprint-status.yaml` file
   **When** the watcher polls
   **Then** a clear error is returned (not a panic)

## Tasks / Subtasks

- [x] Task 1: Create integration test file `tests/integration/test_watcher.rs` (AC: #1–#5)
  - [x] 1.1 Add `mod test_watcher;` declaration in `tests/integration.rs`
  - [x] 1.2 Import required types: `Watcher`, `SprintStatusFile`, `StoryInfo`, `WatcherError`, `BotConfig`, deps functions

- [x] Task 2: Write watcher poll with dependency filtering test (AC: #1)
  - [x] 2.1 Create temp dir, use `write_sprint_status()` from helpers with 5 stories (1-1 done, 1-2 ready-for-dev, 1-3 ready-for-dev, 2-1 ready-for-dev, 2-2 backlog)
  - [x] 2.2 Build `BotConfig` via `make_test_config()` pointing `implementation_artifacts` to temp dir
  - [x] 2.3 Create `Watcher::new(Arc::new(config))` and call `poll()`
  - [x] 2.4 Assert returned stories are exactly `[1-2-*, 2-1-*]` (1-3 skipped because 1-2 not done, 2-2 not ready)
  - [x] 2.5 Assert dependency-valid ordering: 1-2 before any story that depends on it

- [x] Task 3: Write cascade blocking tests (AC: #2)
  - [x] 3.1 Write sprint-status with 1-1 as `blocked`, 1-2 as `ready-for-dev` (depends on 1-1), 1-3 as `ready-for-dev` (depends on 1-2)
  - [x] 3.2 Poll via `Watcher` → assert 1-2 and 1-3 are NOT in eligible results (cascade-blocked)
  - [x] 3.3 Add 2-1 as `ready-for-dev` with no deps → assert it IS returned (independent epic unaffected)
  - [x] 3.4 Test with `needs-clarification` status → verify same cascade behavior as `blocked`
  - [x] 3.5 **Negative test:** Write sprint-status with 1-1 as `in-progress`, 1-2 as `ready-for-dev` → assert 1-2 is NOT cascade-blocked (just skipped because dep not done). Repeat with `review` status. This confirms only `BLOCKING_STATUSES` (`blocked`, `needs-clarification`) trigger cascade — transient statuses do not.

- [x] Task 4: Write all-done scenario test (AC: #3)
  - [x] 4.1 Write sprint-status with all stories as `done`
  - [x] 4.2 Poll via `Watcher` → assert `WatcherError::NoEligibleStories` is returned

- [x] Task 5: Write cyclic dependency test (AC: #4)
  - [x] 5.1 Create stories with manually injected circular deps (override `dependencies` field after construction)
  - [x] 5.2 Use `DependencyGraph` + `topological_sort()` directly → assert `WatcherError::CyclicDependency` is returned
  - [x] 5.3 Verify the error contains the story keys involved in the cycle

- [x] Task 6: Write missing file test (AC: #5)
  - [x] 6.1 Create `Watcher` pointing to a temp dir with no `sprint-status.yaml`
  - [x] 6.2 Call `poll()` → assert `WatcherError::SprintStatusNotFound` is returned
  - [x] 6.3 Verify the error message contains the expected path

- [x] Task 7: Write SprintStatusFile integration tests (supplementary)
  - [x] 7.1 Test `SprintStatusFile::load()` with valid YAML → assert correct story count and order preservation
  - [x] 7.2 Test `stories()` filters out epic and retrospective entries
  - [x] 7.3 Test `eligible_stories()` returns only `ready-for-dev` stories
  - [x] 7.4 Test malformed YAML → assert `WatcherError::SprintStatusParse`

## Dev Notes

### Architecture Compliance

#### Integration Test Location
- All tests for this story go in `tests/integration/test_watcher.rs`
- Declared as `mod test_watcher;` in `tests/integration.rs` (created by Story 7.1)
- Run via `cargo test --test integration`

#### Test Strategy
- These tests exercise the real `Watcher`, `SprintStatusFile`, `DependencyGraph`, and `deps` module functions together — no mocks
- The only external dependency is the filesystem, isolated via `tempfile::tempdir()`
- Dependency resolution is deterministic (pure graph computation) — no LLM calls, no network
- `Watcher` uses `tracing` internally — tests may see log output but this is non-blocking

### Technical Requirements

#### 🚨 Prerequisite: `src/lib.rs` (from Story 7.1 Task 0)
This story requires the `lib.rs` created by Story 7.1 Task 0. Without it, `use bmad_bot::watcher::Watcher;` will not compile because the project is currently a pure binary crate (`main.rs` only). Verify that `src/lib.rs` exists with `pub mod watcher;` before writing any integration tests. If Story 7.1 has not been implemented yet, Task 0 from that story MUST be completed first.

**Import paths after lib.rs exists:**
```rust
use bmad_bot::watcher::{Watcher, SprintStatusFile, StoryInfo, WatcherError};
use bmad_bot::watcher::deps::{
    DependencyGraph, CascadeBlockInfo, derive_dependencies,
    filter_eligible, find_cascade_blocks, build_full_dependency_map,
};
use bmad_bot::config::BotConfig;
```

#### Key API Signatures (exact from codebase)

**`Watcher`** (`src/watcher/mod.rs`):
```rust
impl Watcher {
    pub fn new(config: Arc<BotConfig>) -> Self;
    pub fn poll(&self) -> Result<Vec<StoryInfo>, WatcherError>;
    pub fn sprint_status_path(&self) -> &Path;
}
```
`Watcher::new()` derives `sprint_status_path` from `config.bmad_paths.implementation_artifacts` + `/sprint-status.yaml` and `story_dir` from the same path.

**`SprintStatusFile`** (`src/watcher/mod.rs`):
```rust
impl SprintStatusFile {
    pub fn load(path: &Path, story_dir: &Path) -> Result<Self, WatcherError>;
    pub fn stories(&self) -> Vec<StoryInfo>;
    pub fn eligible_stories(&self) -> Vec<StoryInfo>;
    pub fn entries(&self) -> &[(String, String)];
    pub fn entry_count(&self) -> usize;
}
```

**`DependencyGraph`** (`src/watcher/deps.rs`):
```rust
impl DependencyGraph {
    pub fn new(stories: &[StoryInfo], all_statuses: &[(String, String)]) -> Self;
    pub fn topological_sort(&self) -> Result<Vec<String>, WatcherError>;
    pub fn deps_satisfied(&self, story_key: &str) -> (bool, Option<(String, String)>);
}
```

**Free functions** (`src/watcher/deps.rs`):
```rust
pub fn derive_dependencies(stories: &mut [StoryInfo], all_statuses: &[(String, String)]);
pub fn filter_eligible(stories: Vec<StoryInfo>, all_statuses: &[(String, String)]) -> Result<(Vec<StoryInfo>, usize), WatcherError>;
pub fn find_cascade_blocks(stories: &[StoryInfo], all_statuses: &HashMap<String, String>, all_deps: &HashMap<String, Vec<String>>) -> Vec<CascadeBlockInfo>;
pub fn build_full_dependency_map(all_statuses: &[(String, String)]) -> HashMap<String, Vec<String>>;
```

#### WatcherError Variants to Assert Against
```rust
pub enum WatcherError {
    SprintStatusNotFound { path: String },
    SprintStatusParse(serde_yml::Error),
    SprintStatusRead(std::io::Error),
    NoEligibleStories,
    CyclicDependency { cycle: Vec<String> },
}
```

Use `matches!()` for variant checking:
```rust
assert!(matches!(err, WatcherError::SprintStatusNotFound { ref path } if path.contains("sprint-status.yaml")));
assert!(matches!(err, WatcherError::NoEligibleStories));
assert!(matches!(err, WatcherError::CyclicDependency { ref cycle } if cycle.contains(&"1-1-foo".to_string())));
```

#### Dependency Resolution Rules
- **Intra-epic sequential:** Story N.M depends on N.(M-1). First story in each epic (story_num == 1) has no dependency
- **Cross-epic:** No dependency enforcement at pre-gate level
- **Dep satisfaction:** A dependency is satisfied when its status == `"done"` in sprint-status.yaml
- **Cascade blocking triggers:** `blocked` or `needs-clarification` statuses propagate transitively to all dependents (defined in `BLOCKING_STATUSES` constant)
- **Non-blocking statuses:** `in-progress`, `backlog`, `ready-for-dev`, `review` — story is skipped (dep not met) but NOT cascade-blocked. Test this explicitly (Task 3.5).

#### 🚨 Sprint-Status YAML Comments Are NOT Functional
The real `sprint-status.yaml` has comments like `# depends-on: 7-1`. These are **YAML comments stripped by the parser** — they have ZERO effect on dependency resolution. Dependencies are computed **exclusively** by `derive_dependencies()` from story numbering: story N.M depends on N.(M-1) within the same epic. Never write tests that rely on YAML comments for dependency data.

#### Sprint-Status YAML Format for Tests
Use the `write_sprint_status()` helper from Story 7.1. The YAML structure:
```yaml
generated: 2026-02-08
project: test-project
project_key: TEST
tracking_system: file-system
story_location: "{dir}"

development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli-framework: ready-for-dev
  1-3-init-command: ready-for-dev
  epic-1-retrospective: optional
  epic-2: in-progress
  2-1-polling: ready-for-dev
  2-2-deps-resolution: backlog
```

**Important:** `serde_yml::Mapping` preserves insertion order — story ordering in the YAML directly determines document order for topological sort tiebreaking.

#### Watcher Poll Flow (what happens under the hood)
1. `Watcher::poll()` calls `SprintStatusFile::load()` to parse YAML
2. `sprint_status.stories()` extracts all story entries (skips epics, retrospectives)
3. `sprint_status.eligible_stories()` filters to `ready-for-dev` only
4. `deps::filter_eligible()` is called with eligible stories + all entries:
   a. `derive_dependencies()` — populates `dependencies` field based on intra-epic ordering
   b. `DependencyGraph::new()` + `topological_sort()` — determines execution order, detects cycles
   c. `build_full_dependency_map()` + `find_cascade_blocks()` — detects cascade blocking
   d. Final filter: only stories with `deps_satisfied() == true` AND not cascade-blocked

#### Cyclic Dependency Test Approach
The `derive_dependencies()` function only generates linear intra-epic deps (N depends on N-1), so cycles cannot occur naturally. To test cycle detection:
- Create `StoryInfo` structs manually via `make_test_story()` from helpers
- Manually set `dependencies` fields to create a cycle (e.g., A depends on B, B depends on A)
- Call `DependencyGraph::new()` and `topological_sort()` directly — this bypasses `derive_dependencies()`
- Assert `WatcherError::CyclicDependency` with the offending keys

#### Cascade Blocking Test Pattern
```rust
// Arrange: 1-1 blocked, 1-2 and 1-3 ready-for-dev (sequential deps)
let stories = vec![
    ("epic-1", "in-progress"),
    ("1-1-scaffolding", "blocked"),
    ("1-2-cli-framework", "ready-for-dev"),
    ("1-3-init-command", "ready-for-dev"),
    ("epic-2", "in-progress"),
    ("2-1-polling", "ready-for-dev"),
];
write_sprint_status(tmp.path(), &stories);

// Act: poll via Watcher
let config = make_test_config(tmp.path());
let watcher = Watcher::new(Arc::new(config));
let result = watcher.poll();

// Assert: only 2-1 is eligible (1-2 and 1-3 cascade-blocked by 1-1)
let eligible = result.expect("poll should succeed");
assert_eq!(eligible.len(), 1);
assert_eq!(eligible[0].story_key, "2-1-polling");
```

### Previous Story Intelligence (Story 7.1, 7.2)

- **Cargo test convention (edition 2024):** `tests/integration.rs` is the binary entry point, `tests/integration/` is the submodule directory. Due to Rust edition 2024, **plain `mod` does NOT resolve into the subdirectory** — all test modules MUST use `#[path]` attributes. To add a new test module, add to `tests/integration.rs`: `#[path = "integration/test_watcher.rs"] mod test_watcher;`
- **`lib.rs` is fully set up** — all modules (including `cli`) are already exposed via `pub mod` in `src/lib.rs`. No Task 0 / `lib.rs` blocker work needed.
- **Fixture imports:** `use crate::helpers::fixtures::{make_test_config, make_test_story, write_sprint_status};`
- **Temp dir pattern:** Always use `tempfile::tempdir()` — cleanup is automatic via `Drop`
- **Test naming:** `test_{module}_{behavior}_{scenario}` in snake_case
- **Structure:** Arrange → Act → Assert
- **Config helper detail:** `make_test_config(dir)` sets `bmad_paths.implementation_artifacts` to `"{dir}/_bmad-output/implementation-artifacts"` (NOT just `dir`). If the watcher looks for story files relative to `implementation_artifacts`, you may need to adjust the path or write story files into the nested directory.

### Dependencies Required

All already present — no new dependencies needed:
- `tempfile = "3"` (dev-dependency)
- `std::sync::Arc` for `Watcher::new()`
- `std::collections::HashMap` for cascade blocking assertions

**Prerequisite from Story 7.1:** `src/lib.rs` must exist with `pub mod watcher;` — see Story 7.1 Task 0.

### File Structure

```
tests/
├── integration.rs                    # Add: mod test_watcher;
└── integration/
    ├── helpers/
    │   ├── mod.rs
    │   ├── mocks.rs
    │   └── fixtures.rs
    ├── test_mocks.rs
    ├── test_fixtures.rs
    ├── test_config.rs                # (Story 7.2)
    └── test_watcher.rs               ← NEW (this story)
```

### Testing Standards

- Use `#[test]` for sync tests — `Watcher::poll()` and all deps functions are synchronous
- Use `tempfile::tempdir()` for every test that touches the filesystem
- Never leave artifacts on disk — tempdir handles cleanup via Drop
- Test names: `test_watcher_{behavior}_{scenario}` (e.g., `test_watcher_poll_returns_eligible_with_deps_satisfied`, `test_watcher_cascade_blocks_transitive_dependents`)
- Use `assert!(matches!(...))` for error variant matching with field guards
- Assert on `story_key` values rather than `StoryInfo` struct equality (no `PartialEq` derive)
- **Tracing is a no-op in tests:** `Watcher::poll()` calls `tracing::info!()`, `tracing::warn!()` etc. Without a subscriber initialized, these are silent no-ops. Do NOT install a tracing subscriber in integration tests unless explicitly debugging — it adds noise without value.

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 7.3 (L933-973)]
- [Source: _bmad-output/planning-artifacts/epics.md — Integration Test Strategy (L822-856)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 2: Sprint-Status Mutation (L186-205)]
- [Source: _bmad-output/project-context.md — Daemon Role — Minimal Orchestrator with Pre-Gate]
- [Source: _bmad-output/project-context.md — Sequential Execution, Two-Layer Dependency Model]
- [Source: src/watcher/mod.rs — Watcher struct (L252-260), poll() (L289-349)]
- [Source: src/watcher/mod.rs — SprintStatusFile (L164-241), StoryInfo (L66-86)]
- [Source: src/watcher/deps.rs — DependencyGraph (L45-178), filter_eligible (L457-528)]
- [Source: src/watcher/deps.rs — find_cascade_blocks (L338-360), build_full_dependency_map (L374-404)]
- [Source: src/watcher/deps.rs — derive_dependencies (L416-443), CascadeBlockInfo (L193-204)]
- [Source: src/watcher/deps.rs — BLOCKING_STATUSES: ["blocked", "needs-clarification"] (L33)]
- [Source: src/watcher/mod.rs — WatcherError enum (L24-55)]
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md — Cargo test convention, fixture patterns]

## Dev Agent Record

### Agent Model Used
Claude Sonnet 4 (2025-07-16)

### Debug Log References
All 16 integration tests pass: `cargo test --test integration test_watcher`
Full suite: 69 passed, 0 failed.

### Completion Notes List
- Created `tests/integration/test_watcher.rs` with 16 integration tests covering all 5 ACs
- Added `#[path = "integration/test_watcher.rs"] mod test_watcher;` to `tests/integration.rs` (Rust 2024 edition path attribute)
- AC #1: `test_watcher_poll_returns_eligible_with_deps_satisfied` + `test_watcher_poll_dependency_valid_ordering` — verifies 5-story scenario, eligible=[1-2, 2-1], dep ordering
- AC #2: 5 cascade blocking tests — `blocked` triggers cascade, `needs-clarification` triggers cascade, `in-progress`/`review` do NOT cascade (negative tests per Task 3.5)
- AC #3: `test_watcher_poll_all_done_returns_no_eligible` — asserts `WatcherError::NoEligibleStories`
- AC #4: 2 cyclic dependency tests — manually injected circular deps → `DependencyGraph::topological_sort()` → `WatcherError::CyclicDependency` with key extraction
- AC #5: 2 missing file tests — `WatcherError::SprintStatusNotFound` with path verification
- Task 7: 4 SprintStatusFile integration tests — load, stories(), eligible_stories(), malformed YAML
- Helper functions `setup_sprint_status()` and `make_watcher()` abstract common patterns (nested dir creation, config wiring)
- Key design decision: `make_test_config(dir)` sets `implementation_artifacts` to `{dir}/_bmad-output/implementation-artifacts`, so `setup_sprint_status()` creates nested dirs and writes sprint-status.yaml there

### File List
- tests/integration/test_watcher.rs (NEW) — 16 integration tests for watcher/deps/story-selection chain
- tests/integration.rs (MODIFIED) — added `#[path] mod test_watcher;` declaration
- _bmad-output/implementation-artifacts/sprint-status.yaml (MODIFIED) — story 7-3 status: ready-for-dev → in-progress → review
- _bmad-output/implementation-artifacts/7-3-watcher-dependency-resolution-story-selection-integration-tests.md (MODIFIED) — task checkboxes, dev agent record, status