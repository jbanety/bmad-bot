# Story 2.1: Sprint-Status Polling & Story Detection

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer with stories marked ready-for-dev,
I want the daemon to automatically detect them by polling sprint-status.yaml,
So that stories are picked up for processing without manual intervention.

## Acceptance Criteria

1. **Given** a valid `sprint-status.yaml` exists at the configured output path **When** the watcher module polls the file at the configured interval (default 5 min) **Then** all stories with status `ready-for-dev` are identified and returned as `StoryInfo` structs (id, label, branch name, specs path, dependencies, status) **And** the polling interval is configurable via `bmad-bot.yaml` **And** the `dependencies` field is initialized empty (populated by Story 2.2)

2. **Given** the `sprint-status.yaml` file does not exist or is malformed **When** the watcher attempts to read it **Then** a descriptive `WatcherError` (thiserror enum) is returned **And** the error is logged via `tracing::error!()` with full context **And** the daemon continues polling on the next cycle (does not crash)

3. **Given** no stories have `ready-for-dev` status **When** the watcher polls **Then** the watcher logs an info message and sleeps until the next polling cycle

## Tasks / Subtasks

- [ ] Task 0: Verify prerequisites from Epic 1 (AC: #1, #2, #3)
  - [ ] 0.1 Verify `serde_yaml = "0.9"` is in Cargo.toml (present since Story 1.1)
  - [ ] 0.2 Verify `watcher/mod.rs` and `watcher/deps.rs` stubs exist (created in Story 1.1)
  - [ ] 0.3 Add `tempfile` to `[dev-dependencies]` in Cargo.toml (needed for unit tests with temp dirs)
  - [ ] 0.4 Run `cargo check` to confirm clean baseline

- [ ] Task 1: Define `WatcherError` thiserror enum in `src/watcher/mod.rs` (AC: #2)
  - [ ] 1.1 Create `WatcherError` with variants: `SprintStatusRead`, `SprintStatusParse`, `SprintStatusNotFound`, `NoEligibleStories`
  - [ ] 1.2 Implement `#[from]` conversions for `std::io::Error` and `serde_yaml::Error`
  - [ ] 1.3 Add `/// doc comments` on every variant

- [ ] Task 2: Define `StoryInfo` struct in `src/watcher/mod.rs` (AC: #1)
  - [ ] 2.1 Create `pub struct StoryInfo` with fields: `story_id` (String, e.g. "1.2"), `story_key` (String, e.g. "1-2-cli-framework"), `epic_num` (u32), `story_num` (u32), `label` (String, human-readable name derived from slug), `branch_name` (String, e.g. "story/1-2-cli-framework"), `specs_path` (PathBuf), `dependencies` (Vec<String>, empty for now — Story 2.2 populates), `status` (String)
  - [ ] 2.2 Derive `Debug, Clone`
  - [ ] 2.3 Implement `Display` trait for human-readable output
  - [ ] 2.4 Implement `StoryInfo::from_key_and_status(key: &str, status: &str, story_dir: &Path) -> Option<StoryInfo>` — parses the key format `N-N-slug` and derives all fields

- [ ] Task 3: Implement `SprintStatusFile` parser in `src/watcher/mod.rs` (AC: #1, #2)
  - [ ] 3.1 Create `pub struct SprintStatusFile` holding the parsed development_status mapping
  - [ ] 3.2 Implement `SprintStatusFile::load(path: &Path) -> Result<Self, WatcherError>` — reads and parses the YAML file
  - [ ] 3.3 Implement `SprintStatusFile::stories(&self) -> Vec<StoryInfo>` — extracts all story entries (skips epic-N and retrospective entries)
  - [ ] 3.4 Implement `SprintStatusFile::eligible_stories(&self) -> Vec<StoryInfo>` — returns only `ready-for-dev` stories in document order

- [ ] Task 4: Implement `Watcher` struct in `src/watcher/mod.rs` (AC: #1, #2, #3)
  - [ ] 4.1 Create `pub struct Watcher` with fields: `config: Arc<BotConfig>`, `sprint_status_path: PathBuf`
  - [ ] 4.2 Implement `Watcher::new(config: Arc<BotConfig>) -> Self` — derives sprint_status_path from config
  - [ ] 4.3 Implement `pub fn poll(&self) -> Result<Vec<StoryInfo>, WatcherError>` — loads sprint status, finds eligible stories, returns them
  - [ ] 4.4 If no eligible stories found → return `Err(WatcherError::NoEligibleStories)` (not a crash — caller handles gracefully)
  - [ ] 4.5 Log all poll results via tracing (info for found stories, debug for no stories, error for parse failures)

- [ ] Task 5: Integrate Watcher into `run_polling_loop()` in `src/cli/mod.rs` (AC: #1, #2, #3)
  - [ ] 5.1 Create `Watcher` instance in `run_start()` after config is loaded and pass to `run_polling_loop()`
  - [ ] 5.2 Replace placeholder "no watcher implemented yet" debug log with `watcher.poll()` call
  - [ ] 5.3 On `Ok(stories)` → log each eligible story at info level with story_id and story_key
  - [ ] 5.4 On `Err(WatcherError::NoEligibleStories)` → log debug message, continue to next cycle
  - [ ] 5.5 On `Err(WatcherError::SprintStatusNotFound)` → log warn, continue to next cycle
  - [ ] 5.6 On other `Err(_)` → log error with full context, continue to next cycle (never crash)
  - [ ] 5.7 Update `DaemonState` after each poll with stories found count

- [ ] Task 6: Re-export `deps` stub module (AC: #1)
  - [ ] 6.1 Ensure `src/watcher/mod.rs` declares `pub mod deps;`
  - [ ] 6.2 Ensure `deps.rs` remains a stub (Story 2.2 implements it)
  - [ ] 6.3 Add a `// TODO: Story 2.2 — Dependency Resolution` comment in `deps.rs`

- [ ] Task 7: Write unit tests (AC: #1, #2, #3)
  - [ ] 7.1 Test `StoryInfo::from_key_and_status` with valid key "1-2-cli-framework" parses correctly
  - [ ] 7.2 Test `StoryInfo::from_key_and_status` with invalid key "epic-1" returns None
  - [ ] 7.3 Test `StoryInfo::from_key_and_status` with retrospective key returns None
  - [ ] 7.4 Test `StoryInfo::from_key_and_status` derives correct branch_name format
  - [ ] 7.5 Test `SprintStatusFile::load` with valid YAML file parses successfully
  - [ ] 7.6 Test `SprintStatusFile::load` with missing file returns SprintStatusNotFound
  - [ ] 7.7 Test `SprintStatusFile::load` with malformed YAML returns SprintStatusParse
  - [ ] 7.8 Test `SprintStatusFile::stories` returns only story entries (no epic-N, no retrospective)
  - [ ] 7.9 Test `SprintStatusFile::eligible_stories` returns only ready-for-dev stories
  - [ ] 7.10 Test `SprintStatusFile::eligible_stories` returns empty vec when no ready-for-dev exists
  - [ ] 7.11 Test `SprintStatusFile::stories` preserves document order
  - [ ] 7.12 Test `Watcher::poll` returns eligible stories from a valid file
  - [ ] 7.13 Test `Watcher::poll` returns NoEligibleStories when none are ready-for-dev

- [ ] Task 8: Final quality checks
  - [ ] 8.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [ ] 8.2 Run `cargo clippy` and fix any warnings
  - [ ] 8.3 Run `cargo test` and verify all tests pass (including Epic 1 tests)
  - [ ] 8.4 Verify all public items have `///` doc comments
  - [ ] 8.5 Manual integration test: create a test `sprint-status.yaml` with mixed statuses, run the daemon, verify it logs the correct eligible stories
  - [ ] 8.6 Manual integration test: remove sprint-status.yaml, verify daemon logs warn and continues polling

## Dev Notes

### Previous Story Intelligence

**Story 1.1** established:
- `BotConfig` with `bmad_paths: BmadPathsConfig` containing `implementation_artifacts` path — this is where `sprint-status.yaml` lives
- `polling_interval_secs: u64` with default 300 (5 minutes)
- All module stubs including `src/watcher/mod.rs` and `src/watcher/deps.rs`
- `ConfigError` thiserror enum as reference pattern for `WatcherError`
- `serde_yaml = "0.9"` in Cargo.toml

**Story 1.2** established:
- `run_polling_loop()` with `tokio::select!` for graceful shutdown (SIGINT/SIGTERM)
- Polling loop sleeps for `config.polling_interval_secs` between cycles
- `Arc<BotConfig>` shared to all modules

**Story 1.3** established:
- `Serialize` derive on all config structs
- `CliError` variants: `Config`, `TracingInit`, `Signal`, `Init`, `Io`, `UserCancelled`

**Story 1.4** established:
- `DaemonState` with `stories_processed: usize` counter and `record_story_processed()` method
- `SprintSummary` in `cli/mod.rs` — **parses sprint-status.yaml for the `status` command display**. This is a DIFFERENT concern from the watcher: SprintSummary aggregates counts, while the watcher extracts individual `StoryInfo` structs for processing. Do NOT reuse SprintSummary — create dedicated watcher parsing.
- Updated `run_polling_loop(config, daemon_state, state_path)` signature with DaemonState touch on each cycle
- `init_tracing()` with dual output (stdout + Mutex\<File\> log file)

**Key pattern from Story 1.4 `run_polling_loop`** — current implementation:
```rust
async fn run_polling_loop(
    config: &std::sync::Arc<BotConfig>,
    daemon_state: &mut state::DaemonState,
    state_path: &std::path::Path,
) -> Result<(), CliError> {
    let interval = tokio::time::Duration::from_secs(config.polling_interval_secs);
    let mut sigterm = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::terminate()
    )?;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                daemon_state.touch();
                if let Err(e) = daemon_state.write(state_path) {
                    tracing::warn!(error = %e, "Failed to update daemon state file");
                }
                tracing::debug!(
                    interval_secs = config.polling_interval_secs,
                    "Polling cycle — no watcher implemented yet (placeholder)"
                );
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received SIGINT (Ctrl-C), initiating graceful shutdown");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, initiating graceful shutdown");
                break;
            }
        }
    }
    Ok(())
}
```
This story replaces the placeholder `tracing::debug!` with a real `watcher.poll()` call.

### sprint-status.yaml Format Reference

The actual sprint-status.yaml format (from the project):

```yaml
development_status:
  epic-1: in-progress              # Epic entry — skip
  1-1-project-scaffolding: done    # Story entry — parse
  1-2-cli-framework: ready-for-dev # Story entry — eligible!
  epic-1-retrospective: optional   # Retrospective — skip
  epic-2: backlog                  # Epic entry — skip
  2-1-polling: backlog             # Story entry — not eligible
```

**Parsing rules:**
- Epic entries: key starts with `epic-` and does NOT contain `retrospective` → skip
- Retrospective entries: key contains `retrospective` → skip
- Story entries: key starts with a digit and matches pattern `{epic_num}-{story_num}-{slug}` → parse as StoryInfo
- Eligible: story status == `"ready-for-dev"`

### WatcherError Implementation — `src/watcher/mod.rs`

```rust
/// Errors originating from the watcher module.
#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    /// sprint-status.yaml file not found at the expected path.
    #[error("Sprint status file not found: {path}")]
    SprintStatusNotFound { path: String },



    /// sprint-status.yaml exists but contains invalid YAML.
    #[error("Failed to parse sprint status YAML: {0}")]
    SprintStatusParse(#[from] serde_yaml::Error),

    /// Failed to read sprint-status.yaml from disk (non-NotFound I/O error).
    /// Note: NotFound is handled separately as SprintStatusNotFound.
    #[error("Failed to read sprint status file: {0}")]
    SprintStatusRead(std::io::Error),

    /// No stories with `ready-for-dev` status found in current cycle.
    /// This is NOT a failure — it's an expected state when all stories are
    /// either completed or not yet prepared.
    #[error("No eligible stories found (all stories are either done, in-progress, or backlog)")]
    NoEligibleStories,
}
```

> **NOTE:** `NoEligibleStories` is an informational error, not a failure. The caller (polling loop) handles it by logging at **info** level (per AC #3) and continuing, NOT by reporting it as a tracing::error.

### StoryInfo Implementation — `src/watcher/mod.rs`

```rust
use std::fmt;
use std::path::{Path, PathBuf};

/// Metadata for a single story extracted from sprint-status.yaml.
///
/// Used by the watcher to identify eligible stories and by the session
/// module to set up development sessions (Epic 4).
#[derive(Debug, Clone)]
pub struct StoryInfo {
    /// Dot-separated story ID (e.g., "1.2").
    pub story_id: String,
    /// Dash-separated story key matching sprint-status.yaml key (e.g., "1-2-cli-framework").
    pub story_key: String,
    /// Epic number extracted from the key.
    pub epic_num: u32,
    /// Story number within the epic.
    pub story_num: u32,
    /// Human-readable label derived from the slug portion of the key.
    pub label: String,
    /// Git branch name following convention: "story/{story_key}".
    pub branch_name: String,
    /// Path to the story specs markdown file in implementation-artifacts.
    pub specs_path: PathBuf,
    /// Story dependencies (story keys this story depends on).
    /// Empty in Story 2.1 — populated by dependency resolution in Story 2.2.
    pub dependencies: Vec<String>,
    /// Current status string from sprint-status.yaml.
    pub status: String,
}

impl StoryInfo {
    /// Parse a sprint-status.yaml key and status into a StoryInfo.
    ///
    /// Returns `None` if the key is not a valid story key (e.g., it's an
    /// epic entry like "epic-1" or a retrospective like "epic-1-retrospective").
    ///
    /// # Key format: `{epic_num}-{story_num}-{slug}`
    /// Examples: "1-2-cli-framework", "3-1-supervisor-tool-skeleton"
    pub fn from_key_and_status(key: &str, status: &str, story_dir: &Path) -> Option<Self> {
        // Skip epic entries (e.g., "epic-1", "epic-2")
        if key.starts_with("epic-") {
            return None;
        }

        // Skip retrospective entries (e.g., "epic-1-retrospective")
        if key.contains("retrospective") {
            return None;
        }

        // Must start with a digit to be a story key
        let first_char = key.chars().next()?;
        if !first_char.is_ascii_digit() {
            return None;
        }

        // Parse: {epic_num}-{story_num}-{slug}
        let mut parts = key.splitn(3, '-');
        let epic_num: u32 = parts.next()?.parse().ok()?;
        let story_num: u32 = parts.next()?.parse().ok()?;
        let slug = parts.next().unwrap_or("");

        // Derive label from slug: replace hyphens with spaces
        let label = slug.replace('-', " ");

        let story_id = format!("{epic_num}.{story_num}");
        let branch_name = format!("story/{key}");
        let specs_path = story_dir.join(format!("{key}.md"));

        Some(Self {
            story_id,
            story_key: key.to_string(),
            epic_num,
            story_num,
            label,
            branch_name,
            specs_path,
            dependencies: Vec::new(), // Populated by Story 2.2 dependency resolution
            status: status.to_string(),
        })
    }

    /// Returns true if this story has `ready-for-dev` status.
    pub fn is_eligible(&self) -> bool {
        self.status == "ready-for-dev"
    }
}

impl fmt::Display for StoryInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} (status: {}, branch: {})",
            self.story_id, self.label, self.status, self.branch_name
        )
    }
}
```

### SprintStatusFile Implementation — `src/watcher/mod.rs`

```rust
/// Parsed representation of sprint-status.yaml's development_status section.
///
/// This struct is the watcher's primary data source. It loads the YAML file,
/// extracts story entries, and identifies eligible stories for processing.
#[derive(Debug)]
pub struct SprintStatusFile {
    /// Ordered list of (key, status) pairs from development_status.
    /// Order is preserved from the YAML file (serde_yaml::Mapping preserves insertion order).
    entries: Vec<(String, String)>,
    /// Directory where story spec files live (for building specs_path).
    story_dir: PathBuf,
}

impl SprintStatusFile {
    /// Load and parse sprint-status.yaml from the given path.
    ///
    /// # Errors
    /// - `WatcherError::SprintStatusNotFound` if the file does not exist
    /// - `WatcherError::SprintStatusRead` if the file cannot be read
    /// - `WatcherError::SprintStatusParse` if the YAML is malformed
    pub fn load(path: &Path, story_dir: &Path) -> Result<Self, WatcherError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                WatcherError::SprintStatusNotFound {
                    path: path.display().to_string(),
                }
            } else {
                WatcherError::SprintStatusRead(e)
            }
        })?;
        let yaml: serde_yaml::Value = serde_yaml::from_str(&content)?;

        let mut entries = Vec::new();

        if let Some(dev_status) = yaml.get("development_status").and_then(|v| v.as_mapping()) {
            for (key, value) in dev_status {
                let key_str = key.as_str().unwrap_or("").to_string();
                let status_str = value.as_str().unwrap_or("").to_string();
                if !key_str.is_empty() {
                    entries.push((key_str, status_str));
                }
            }
        }

        Ok(Self {
            entries,
            story_dir: story_dir.to_path_buf(),
        })
    }

    /// Extract all story entries as StoryInfo structs.
    /// Skips epic entries and retrospective entries.
    /// Preserves document order from sprint-status.yaml.
    pub fn stories(&self) -> Vec<StoryInfo> {
        self.entries
            .iter()
            .filter_map(|(key, status)| {
                StoryInfo::from_key_and_status(key, status, &self.story_dir)
            })
            .collect()
    }

    /// Extract only stories with `ready-for-dev` status, in document order.
    /// These are the stories eligible for the daemon to pick up.
    pub fn eligible_stories(&self) -> Vec<StoryInfo> {
        self.stories()
            .into_iter()
            .filter(|s| s.is_eligible())
            .collect()
    }

    /// Returns the total number of entries (including epics and retrospectives).
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}
```

### Watcher Implementation — `src/watcher/mod.rs`

```rust
use std::sync::Arc;
use crate::config::BotConfig;

pub mod deps;

/// The watcher polls sprint-status.yaml and identifies stories eligible for processing.
///
/// The watcher is a pure reader — it never writes to sprint-status.yaml or any
/// BMAD artifact. All mutations are performed by the BMAD agent during sessions
/// (Architecture Decision 2).
pub struct Watcher {
    /// Shared bot configuration (paths, polling interval).
    config: Arc<BotConfig>,
    /// Resolved path to sprint-status.yaml.
    sprint_status_path: PathBuf,
    /// Directory containing story spec files.
    story_dir: PathBuf,
}

impl Watcher {
    /// Create a new Watcher from the shared config.
    ///
    /// Derives the sprint-status.yaml path from
    /// `config.bmad_paths.implementation_artifacts`.
    pub fn new(config: Arc<BotConfig>) -> Self {
        let sprint_status_path = PathBuf::from(&config.bmad_paths.implementation_artifacts)
            .join("sprint-status.yaml");
        let story_dir = PathBuf::from(&config.bmad_paths.implementation_artifacts);

        Self {
            config,
            sprint_status_path,
            story_dir,
        }
    }

    /// Poll sprint-status.yaml and return eligible stories.
    ///
    /// This is the main entry point called from the polling loop.
    /// Returns a list of `StoryInfo` with status `ready-for-dev`.
    ///
    /// # Errors
    /// - `WatcherError::SprintStatusNotFound` — file doesn't exist yet
    /// - `WatcherError::SprintStatusRead` — I/O error reading file
    /// - `WatcherError::SprintStatusParse` — malformed YAML
    /// - `WatcherError::NoEligibleStories` — no ready-for-dev stories found
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

        for story in &eligible {
            tracing::info!(
                story_id = %story.story_id,
                story_key = %story.story_key,
                branch = %story.branch_name,
                "Eligible story detected"
            );
        }

        Ok(eligible)
    }

    /// Returns the path being polled (for diagnostics/logging).
    pub fn sprint_status_path(&self) -> &Path {
        &self.sprint_status_path
    }
}
```

### Integration into `run_start()` and `run_polling_loop()`

**Updated `run_start()` in `src/cli/mod.rs`:**

```rust
pub async fn run_start(config_path: &std::path::Path) -> Result<(), CliError> {
    let config = crate::config::BotConfig::load(config_path)?;
    config.validate()?;

    init_tracing(&config)?;

    let secrets = crate::config::BotSecrets::load()?;
    secrets.validate_for_config(&config)?;

    // BMAD auto-discovery (Story 1.4)
    let discovery = crate::config::discovery::BmadDiscovery::discover(
        std::path::Path::new(&config.bmad_paths.project_root),
    );
    if discovery.bmad_detected {
        tracing::info!(
            bmad_version = ?discovery.bmad_version,
            modules = ?discovery.installed_modules,
            "BMAD installation detected"
        );
    } else {
        tracing::warn!(
            project_root = %config.bmad_paths.project_root,
            "No BMAD installation detected — _bmad/ directory not found"
        );
    }

    // Write daemon state file (Story 1.4)
    let state_path = std::path::Path::new(state::STATE_FILE_NAME);
    let mut daemon_state = state::DaemonState::new_running(
        std::path::PathBuf::from(&config.log_file),
        discovery,
    );
    daemon_state.write(state_path)?;

    let config = std::sync::Arc::new(config);

    // Create watcher (Story 2.1)
    let watcher = crate::watcher::Watcher::new(Arc::clone(&config));

    tracing::info!(
        config_path = %config_path.display(),
        polling_interval_secs = config.polling_interval_secs,
        sprint_status_path = %watcher.sprint_status_path().display(),
        git_provider = %config.git_provider.provider,
        log_format = %config.log_format,
        log_file = %config.log_file,
        "bmad-bot daemon started"
    );

    // Polling loop with graceful shutdown
    run_polling_loop(&config, &watcher, &mut daemon_state, state_path).await?;

    // Clean shutdown
    daemon_state.mark_stopped();
    daemon_state.write(state_path)?;
    state::DaemonState::cleanup(state_path)?;

    tracing::info!("bmad-bot daemon stopped cleanly");
    Ok(())
}
```

**Updated `run_polling_loop()` in `src/cli/mod.rs`:**

```rust
async fn run_polling_loop(
    config: &std::sync::Arc<BotConfig>,
    watcher: &crate::watcher::Watcher,
    daemon_state: &mut state::DaemonState,
    state_path: &std::path::Path,
) -> Result<(), CliError> {
    let interval = tokio::time::Duration::from_secs(config.polling_interval_secs);

    let mut sigterm = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::terminate()
    )?;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                // Update last activity timestamp
                daemon_state.touch();
                if let Err(e) = daemon_state.write(state_path) {
                    tracing::warn!(error = %e, "Failed to update daemon state file");
                }

                // Poll for eligible stories
                match watcher.poll() {
                    Ok(stories) => {
                        tracing::info!(
                            eligible_count = stories.len(),
                            "Found eligible stories — session launching not yet implemented (Epic 4)"
                        );
                        // TODO: Epic 4 — Launch dev session for first eligible story
                        // For now, log and continue. The watcher's job (this story) is done.
                        // Story 2.2 will add dependency resolution before this point.
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
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            "Failed to poll sprint status — will retry next cycle"
                        );
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received SIGINT (Ctrl-C), initiating graceful shutdown");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, initiating graceful shutdown");
                break;
            }
        }
    }

    Ok(())
}
```

> **CRITICAL CHANGE:** `run_polling_loop` signature adds `watcher: &crate::watcher::Watcher` parameter. This is a private function so no external callers break — only `run_start()` calls it.

> **NOTE on sequential execution:** Architecture mandates "one story at a time, in sprint order." When session launching is implemented (Epic 4), the polling loop will pick the FIRST eligible story (index 0) from the ordered list. Story 2.2 will filter by dependency satisfaction before that. This story returns ALL eligible stories in document order — the caller decides which to process.

### Module-Level Doc Comment for `src/watcher/mod.rs`

```rust
//! Watcher module — polls sprint-status.yaml and detects eligible stories.
//!
//! The watcher is the daemon's primary input source. It periodically reads
//! `sprint-status.yaml` from the configured path, identifies stories with
//! `ready-for-dev` status, and returns them as `StoryInfo` structs.
//!
//! **Architecture Decision 2:** The daemon is a pure reader of sprint-status.yaml.
//! All mutations are performed by the BMAD agent during development sessions.
```

### Files Modified/Created in This Story

| File | Change |
|------|--------|
| `src/watcher/mod.rs` | **REPLACE STUB** — Full implementation: `WatcherError`, `StoryInfo`, `SprintStatusFile`, `Watcher`, unit tests |
| `src/watcher/deps.rs` | Add `// TODO: Story 2.2` comment, ensure `pub mod deps;` compiles |
| `src/cli/mod.rs` | Update `run_start()` to create `Watcher` and pass to polling loop. Update `run_polling_loop()` signature to accept `&Watcher`. Replace placeholder with `watcher.poll()` call and match arms. |

### Relationship to Story 1.4's SprintSummary

Story 1.4 introduced `SprintSummary` in `cli/mod.rs` for the `status` command. That struct aggregates counts (total stories, backlog count, done count, etc.) for display purposes.

**Do NOT reuse `SprintSummary` for the watcher.** The concerns are different:

| Aspect | SprintSummary (Story 1.4) | Watcher (Story 2.1) |
|--------|--------------------------|---------------------|
| Purpose | Display aggregate stats | Extract individual stories for processing |
| Output | Counts by status | `Vec<StoryInfo>` with full metadata |
| Lives in | `cli/mod.rs` | `watcher/mod.rs` |
| Error handling | Returns defaults on failure | Returns typed `WatcherError` |
| Used by | `run_status()` command | Polling loop → future session launching |

Both parse the same YAML file but for completely different purposes. Duplication of the YAML read is acceptable and intentional — each module owns its own parsing logic.

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code — use `?` with `WatcherError`
- ❌ **NO** `anyhow::Result` in `watcher/mod.rs` — typed `WatcherError` only
- ❌ **NO** writing to sprint-status.yaml — the daemon is a PURE READER (Architecture Decision 2)
- ❌ **NO** crashing on missing or malformed sprint-status.yaml — log error and continue polling
- ❌ **NO** treating `NoEligibleStories` as an error in logs — it's expected state, log at debug level
- ❌ **NO** reusing `SprintSummary` from `cli/mod.rs` — different concern, different module
- ❌ **NO** implementing dependency resolution — that's Story 2.2 (`deps.rs`)
- ❌ **NO** implementing session launching — that's Epic 4
- ❌ **NO** modifying modules other than `watcher/mod.rs`, `watcher/deps.rs`, and `cli/mod.rs`
- ❌ **NO** assuming YAML mapping preserves insertion order without verification — `serde_yaml::Mapping` does preserve order, but document this assumption
- ❌ **NO** parsing story metadata beyond what's in sprint-status.yaml — don't read individual story .md files in the watcher (Story 4.2 does that during session setup)

### Scope Boundaries

**IN SCOPE for this story:**
- `src/watcher/mod.rs` — `WatcherError`, `StoryInfo`, `SprintStatusFile`, `Watcher`
- `src/watcher/deps.rs` — stub cleanup with TODO comment
- `src/cli/mod.rs` — integrate Watcher into `run_start()` and `run_polling_loop()`

**OUT OF SCOPE — do NOT implement:**
- Dependency resolution and execution order (Story 2.2)
- Cascade blocking of dependent stories (Story 2.3)
- Session launching for eligible stories (Epic 4)
- Writing to sprint-status.yaml (Architecture Decision 2 — daemon never writes)
- Reading individual story .md files for content (session module, Epic 4)
- Pre-gate dependency filtering (Story 2.2 — `deps.rs`)

### Testing Requirements

All tests go inline at the bottom of `src/watcher/mod.rs` in `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // --- StoryInfo tests ---

    #[test]
    fn test_story_info_from_valid_key() {
        let story_dir = Path::new("/tmp/artifacts");
        let info = StoryInfo::from_key_and_status(
            "1-2-cli-framework-daemon-lifecycle",
            "ready-for-dev",
            story_dir,
        );
        let info = info.expect("Should parse valid story key");
        assert_eq!(info.epic_num, 1);
        assert_eq!(info.story_num, 2);
        assert_eq!(info.story_id, "1.2");
        assert_eq!(info.story_key, "1-2-cli-framework-daemon-lifecycle");
        assert_eq!(info.label, "cli framework daemon lifecycle");
        assert_eq!(info.branch_name, "story/1-2-cli-framework-daemon-lifecycle");
        assert_eq!(info.status, "ready-for-dev");
        assert!(info.dependencies.is_empty(), "Dependencies should be empty in Story 2.1");
        assert_eq!(
            info.specs_path,
            PathBuf::from("/tmp/artifacts/1-2-cli-framework-daemon-lifecycle.md")
        );
    }

    #[test]
    fn test_story_info_from_key_single_word_slug() {
        let info = StoryInfo::from_key_and_status(
            "3-1-supervisor",
            "backlog",
            Path::new("/tmp"),
        );
        let info = info.expect("Should parse single-word slug");
        assert_eq!(info.epic_num, 3);
        assert_eq!(info.story_num, 1);
        assert_eq!(info.label, "supervisor");
    }

    #[test]
    fn test_story_info_rejects_epic_entry() {
        let result = StoryInfo::from_key_and_status(
            "epic-1",
            "in-progress",
            Path::new("/tmp"),
        );
        assert!(result.is_none(), "Should reject epic entries");
    }

    #[test]
    fn test_story_info_rejects_retrospective() {
        let result = StoryInfo::from_key_and_status(
            "epic-1-retrospective",
            "optional",
            Path::new("/tmp"),
        );
        assert!(result.is_none(), "Should reject retrospective entries");
    }

    #[test]
    fn test_story_info_rejects_non_numeric_start() {
        let result = StoryInfo::from_key_and_status(
            "alpha-1-something",
            "backlog",
            Path::new("/tmp"),
        );
        assert!(result.is_none(), "Should reject keys not starting with digit");
    }

    #[test]
    fn test_story_info_is_eligible_ready_for_dev() {
        let info = StoryInfo::from_key_and_status(
            "1-1-scaffolding",
            "ready-for-dev",
            Path::new("/tmp"),
        ).unwrap();
        assert!(info.is_eligible());
    }

    #[test]
    fn test_story_info_is_not_eligible_backlog() {
        let info = StoryInfo::from_key_and_status(
            "1-1-scaffolding",
            "backlog",
            Path::new("/tmp"),
        ).unwrap();
        assert!(!info.is_eligible());
    }

    #[test]
    fn test_story_info_is_not_eligible_done() {
        let info = StoryInfo::from_key_and_status(
            "1-1-scaffolding",
            "done",
            Path::new("/tmp"),
        ).unwrap();
        assert!(!info.is_eligible());
    }

    #[test]
    fn test_story_info_display_format() {
        let info = StoryInfo::from_key_and_status(
            "2-1-polling",
            "ready-for-dev",
            Path::new("/tmp"),
        ).unwrap();
        let display = format!("{info}");
        assert!(display.contains("2.1"));
        assert!(display.contains("polling"));
        assert!(display.contains("ready-for-dev"));
        assert!(display.contains("story/2-1-polling"));
    }

    #[test]
    fn test_story_info_dependencies_default_empty() {
        let info = StoryInfo::from_key_and_status(
            "2-1-polling",
            "ready-for-dev",
            Path::new("/tmp"),
        ).unwrap();
        assert!(info.dependencies.is_empty(), "Story 2.1 must not populate dependencies");
    }

    #[test]
    fn test_story_info_derives_correct_branch_name() {
        let info = StoryInfo::from_key_and_status(
            "4-3-pre-development-preparation",
            "backlog",
            Path::new("/artifacts"),
        ).unwrap();
        assert_eq!(info.branch_name, "story/4-3-pre-development-preparation");
    }

    // --- SprintStatusFile tests ---

    fn write_test_sprint_status(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("sprint-status.yaml");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_sprint_status_load_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: ready-for-dev
  epic-1-retrospective: optional
"#;
        let path = write_test_sprint_status(tmp.path(), content);
        let result = SprintStatusFile::load(&path, tmp.path());
        assert!(result.is_ok());
        let ssf = result.unwrap();
        assert_eq!(ssf.entry_count(), 4); // All entries including epic and retro
    }

    #[test]
    fn test_sprint_status_load_missing_file() {
        // Tests the TOCTOU-safe path: read_to_string maps NotFound directly
        let result = SprintStatusFile::load(
            Path::new("/nonexistent/sprint-status.yaml"),
            Path::new("/tmp"),
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            WatcherError::SprintStatusNotFound { path } => {
                assert!(path.contains("nonexistent"));
            }
            other => panic!("Expected SprintStatusNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_sprint_status_load_malformed_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_test_sprint_status(tmp.path(), "{{{{invalid yaml}}}}");
        let result = SprintStatusFile::load(&path, tmp.path());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WatcherError::SprintStatusParse(_)));
    }

    #[test]
    fn test_sprint_status_stories_skips_epics_and_retros() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: ready-for-dev
  epic-1-retrospective: optional
  epic-2: backlog
  2-1-polling: backlog
"#;
        let path = write_test_sprint_status(tmp.path(), content);
        let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
        let stories = ssf.stories();
        assert_eq!(stories.len(), 3); // Only: 1-1, 1-2, 2-1
        assert_eq!(stories[0].story_key, "1-1-scaffolding");
        assert_eq!(stories[1].story_key, "1-2-cli");
        assert_eq!(stories[2].story_key, "2-1-polling");
    }

    #[test]
    fn test_sprint_status_eligible_stories_filters_ready_for_dev() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: ready-for-dev
  1-3-init: ready-for-dev
  1-4-status: backlog
"#;
        let path = write_test_sprint_status(tmp.path(), content);
        let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
        let eligible = ssf.eligible_stories();
        assert_eq!(eligible.len(), 2);
        assert_eq!(eligible[0].story_key, "1-2-cli");
        assert_eq!(eligible[1].story_key, "1-3-init");
    }

    #[test]
    fn test_sprint_status_eligible_stories_empty_when_none_ready() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: in-progress
  1-3-init: backlog
"#;
        let path = write_test_sprint_status(tmp.path(), content);
        let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
        let eligible = ssf.eligible_stories();
        assert!(eligible.is_empty());
    }

    #[test]
    fn test_sprint_status_preserves_document_order() {
        let tmp = tempfile::tempdir().unwrap();
        let content = r#"
development_status:
  epic-2: backlog
  2-1-polling: ready-for-dev
  epic-1: in-progress
  1-1-scaffolding: ready-for-dev
"#;
        let path = write_test_sprint_status(tmp.path(), content);
        let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
        let stories = ssf.stories();
        // Order should match YAML file, not sorted by epic/story number
        assert_eq!(stories[0].story_key, "2-1-polling");
        assert_eq!(stories[1].story_key, "1-1-scaffolding");
    }

    #[test]
    fn test_sprint_status_handles_empty_development_status() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "development_status:\n";
        let path = write_test_sprint_status(tmp.path(), content);
        let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
        assert!(ssf.stories().is_empty());
    }

    #[test]
    fn test_sprint_status_handles_missing_development_status_key() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "some_other_key: value\n";
        let path = write_test_sprint_status(tmp.path(), content);
        let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
        assert!(ssf.stories().is_empty());
    }

    // --- Watcher tests ---

    #[test]
    fn test_watcher_poll_returns_eligible_stories() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path();
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: ready-for-dev
"#;
        fs::write(artifacts_dir.join("sprint-status.yaml"), content).unwrap();

        // Create a minimal BotConfig pointing to our temp dir
        let config = Arc::new(make_test_bot_config(artifacts_dir));
        let watcher = Watcher::new(config);
        let result = watcher.poll();
        assert!(result.is_ok());
        let stories = result.unwrap();
        assert_eq!(stories.len(), 1);
        assert_eq!(stories[0].story_key, "1-2-cli");
    }

    #[test]
    fn test_watcher_poll_returns_no_eligible_stories_error() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path();
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: in-progress
"#;
        fs::write(artifacts_dir.join("sprint-status.yaml"), content).unwrap();

        let config = Arc::new(make_test_bot_config(artifacts_dir));
        let watcher = Watcher::new(config);
        let result = watcher.poll();
        assert!(matches!(result.unwrap_err(), WatcherError::NoEligibleStories));
    }

    #[test]
    fn test_watcher_poll_handles_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        // Do NOT create sprint-status.yaml
        let config = Arc::new(make_test_bot_config(tmp.path()));
        let watcher = Watcher::new(config);
        let result = watcher.poll();
        assert!(matches!(
            result.unwrap_err(),
            WatcherError::SprintStatusNotFound { .. }
        ));
    }

    /// Helper: create a minimal BotConfig for watcher tests.
    /// Only bmad_paths.implementation_artifacts matters for watcher.
    fn make_test_bot_config(artifacts_dir: &Path) -> BotConfig {
        use crate::config::*;
        BotConfig {
            polling_interval_secs: 10,
            git_provider: GitProviderConfig {
                provider: "github".to_string(),
                repo_owner: "test".to_string(),
                repo_name: "test".to_string(),
                target_branch: "main".to_string(),
            },
            llm: LlmConfig {
                dev: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test".to_string(),
                },
                review: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test".to_string(),
                },
                supervisor: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test".to_string(),
                },
            },
            notifications: NotificationConfig {
                telegram: TelegramConfig {
                    enabled: false,
                    chat_id: String::new(),
                },
            },
            bmad_paths: BmadPathsConfig {
                project_root: artifacts_dir.parent().unwrap_or(artifacts_dir).display().to_string(),
                output_folder: artifacts_dir.display().to_string(),
                planning_artifacts: artifacts_dir.display().to_string(),
                implementation_artifacts: artifacts_dir.display().to_string(),
            },
            log_format: "pretty".to_string(),
            log_level: "info".to_string(),
            log_file: "test.log".to_string(),
        }
    }
}
```

> **NOTE on `make_test_bot_config`:** This is a copy of the pattern from Story 1.3's `make_test_config()` in `cli/mod.rs`, adapted for the watcher module. The watcher only needs `bmad_paths.implementation_artifacts` — all other fields are set to valid defaults. If a shared test config builder is created in the future, this can be replaced.

### Project Structure Notes

After this story, the watcher module goes from stub to real implementation:

```
src/watcher/
├── mod.rs      # FULL: WatcherError, StoryInfo, SprintStatusFile, Watcher, tests
└── deps.rs     # STUB: TODO comment for Story 2.2
```

The `watcher → session` interface contract (from architecture) is established by `StoryInfo`:
- `Watcher::poll()` returns `Vec<StoryInfo>`
- Epic 4's session module will consume `StoryInfo` to set up dev sessions
- Story 2.2 will add `deps.rs` with dependency resolution that filters eligible stories

### References

- [Source: epics.md § Story 2.1: Sprint-Status Polling & Story Detection] — User story, acceptance criteria
- [Source: epics.md § Epic 2: Story Watching & Dependency Management] — Epic context, daemon as pure reader
- [Source: prd.md § FR1] — Detect ready-for-dev stories by polling sprint-status.yaml
- [Source: prd.md § FR2-4] — Dependency resolution context (Stories 2.2, 2.3 — not this story)
- [Source: architecture.md § Decision 2: Sprint-Status Mutation] — Daemon is pure reader, agent writes
- [Source: architecture.md § Error Type Pattern] — Per-module thiserror enums
- [Source: architecture.md § Tracing Pattern] — Structured spans with context fields
- [Source: architecture.md § Project Structure § watcher/] — `mod.rs` (polling), `deps.rs` (pre-gate)
- [Source: architecture.md § Architectural Boundaries] — watcher → session: passes StoryInfo struct
- [Source: architecture.md § Data Flow] — Step 3: watcher reads sprint-status, deps computes pre-gate
- [Source: project-context.md § Daemon Lifecycle] — Watcher polls every 5 minutes for ready-for-dev stories
- [Source: project-context.md § Daemon Role] — Daemon is launcher not executor, pre-gate before LLM
- [Source: project-context.md § Sequential Execution] — One story at a time, in sprint order
- [Source: project-context.md § Testing Rules] — Inline tests, descriptive snake_case, mocked data
- [Source: Story 1.1] — BotConfig, BmadPathsConfig.implementation_artifacts, module stubs
- [Source: Story 1.2] — run_polling_loop with tokio::select!, graceful shutdown, Arc<BotConfig>
- [Source: Story 1.4] — DaemonState updates in polling loop, SprintSummary (separate concern), run_polling_loop signature

## Dev Agent Record

<!-- This section is filled automatically by the dev agent post-implementation. Do not edit manually. -->

### Agent Model Used

_(filled post-implementation)_

### Debug Log References

### Completion Notes List

### File List