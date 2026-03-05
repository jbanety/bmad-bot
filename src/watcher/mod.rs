//! Watcher module — polls sprint-status.yaml and detects eligible stories.
//!
//! The watcher is the daemon's primary input source. It periodically reads
//! `sprint-status.yaml` from the configured path, identifies stories with
//! `ready-for-dev` status, and returns them as `StoryInfo` structs.
//!
//! **Architecture Decision 2:** The daemon is a pure reader of sprint-status.yaml.
//! All mutations are performed by the BMAD agent during development sessions.

pub mod deps;

use std::collections::HashMap;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::BotConfig;

// ---------------------------------------------------------------------------
// WatcherError
// ---------------------------------------------------------------------------

/// Errors originating from the watcher module.
#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    /// sprint-status.yaml file not found at the expected path.
    #[error("Sprint status file not found: {path}")]
    SprintStatusNotFound {
        /// The path that was searched.
        path: String,
    },

    /// sprint-status.yaml exists but contains invalid YAML.
    #[error("Failed to parse sprint status YAML: {0}")]
    SprintStatusParse(#[from] serde_yml::Error),

    /// Failed to read sprint-status.yaml from disk (non-NotFound I/O error).
    /// Note: NotFound is handled separately as `SprintStatusNotFound`.
    #[error("Failed to read sprint status file: {0}")]
    SprintStatusRead(std::io::Error),

    /// No stories with `ready-for-dev` status found in current cycle.
    /// This is NOT a failure — it's an expected state when all stories are
    /// either completed or not yet prepared.
    #[error("No eligible stories found (all stories are either done, in-progress, or backlog)")]
    NoEligibleStories,

    /// A cyclic dependency was detected among stories in sprint-status.yaml.
    /// Contains the list of story keys involved in the cycle. The affected
    /// stories are skipped for this polling cycle.
    #[error("Cyclic dependency detected: {cycle:?}")]
    CyclicDependency {
        /// Story keys forming the dependency cycle.
        cycle: Vec<String>,
    },
}

// ---------------------------------------------------------------------------
// StoryInfo
// ---------------------------------------------------------------------------

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
    /// Parse a sprint-status.yaml key and status into a `StoryInfo`.
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

// ---------------------------------------------------------------------------
// SprintStatusFile
// ---------------------------------------------------------------------------

/// Parsed representation of sprint-status.yaml's `development_status` section.
///
/// This struct is the watcher's primary data source. It loads the YAML file,
/// extracts story entries, and identifies eligible stories for processing.
///
/// **Comment dependencies:** Sprint-status.yaml supports `# depends-on:` inline
/// comments on story entries. These declare cross-epic or non-sequential
/// dependencies that the daemon enforces at the pre-gate level. Example:
/// ```text
/// 7-1-test-infra: ready-for-dev  # depends-on: epic-8
/// 4-2-session:    done           # depends-on: 4-1
/// ```
#[derive(Debug)]
pub struct SprintStatusFile {
    /// Ordered list of (key, status) pairs from `development_status`.
    /// Order is preserved from the YAML file (`serde_yml::Mapping` preserves insertion order).
    entries: Vec<(String, String)>,
    /// Directory where story spec files live (for building `specs_path`).
    story_dir: PathBuf,
    /// Explicit dependencies parsed from `# depends-on:` inline comments.
    /// Maps story_key → list of raw dependency specs (e.g., `["epic-8"]`, `["4-1"]`).
    /// These are resolved to actual story keys by [`deps::resolve_comment_deps`].
    comment_deps: HashMap<String, Vec<String>>,
}

impl SprintStatusFile {
    /// Load and parse sprint-status.yaml from the given path.
    ///
    /// # Errors
    /// - [`WatcherError::SprintStatusNotFound`] if the file does not exist
    /// - [`WatcherError::SprintStatusRead`] if the file cannot be read
    /// - [`WatcherError::SprintStatusParse`] if the YAML is malformed
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

        // Pre-parse: extract `# depends-on:` comments from raw text BEFORE
        // serde_yml strips them. Format:
        //   story-key: status  # depends-on: dep-spec
        let comment_deps = Self::parse_comment_deps(&content);

        let yaml: serde_yml::Value = serde_yml::from_str(&content)?;

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
            comment_deps,
        })
    }

    /// Parse `# depends-on:` inline comments from raw YAML text.
    ///
    /// Scans each line for the pattern `key: value # depends-on: specs` and
    /// extracts the dependency specs. Supports comma-separated specs.
    ///
    /// Parenthetical annotations like `(resolved)` are stripped from each spec.
    ///
    /// # Supported formats
    /// - `# depends-on: 4-1` — story shorthand
    /// - `# depends-on: epic-8` — depends on all stories in epic 8
    /// - `# depends-on: epics 1-6` — depends on all stories in epics 1 through 6
    /// - `# depends-on: epic-8 (resolved)` — annotation stripped
    /// - `# depends-on: 4-1, 4-2` — comma-separated multiple deps
    fn parse_comment_deps(content: &str) -> HashMap<String, Vec<String>> {
        let mut deps: HashMap<String, Vec<String>> = HashMap::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip pure comment lines and empty lines
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Look for `key: value # depends-on: specs`
            let Some(hash_pos) = trimmed.find('#') else {
                continue;
            };

            let comment = trimmed[hash_pos + 1..].trim();
            let dep_prefix = "depends-on:";
            if !comment.to_lowercase().starts_with(dep_prefix) {
                continue;
            }

            // Extract the key (first non-whitespace token before `:`)
            let key_part = trimmed[..hash_pos].trim();
            let Some(colon_pos) = key_part.find(':') else {
                continue;
            };
            let key = key_part[..colon_pos].trim().to_string();
            if key.is_empty() {
                continue;
            }

            // Extract the raw dep specs after "depends-on:"
            let raw_specs = comment[dep_prefix.len()..].trim();
            if raw_specs.is_empty() {
                continue;
            }

            // Split by comma for multiple deps, strip parenthetical annotations
            let specs: Vec<String> = raw_specs
                .split(',')
                .map(|s| {
                    let s = s.trim();
                    // Strip parenthetical annotations: "epic-8 (resolved)" → "epic-8"
                    match s.find('(') {
                        Some(paren_pos) => s[..paren_pos].trim().to_string(),
                        None => s.to_string(),
                    }
                })
                .filter(|s| !s.is_empty())
                .collect();

            if !specs.is_empty() {
                deps.insert(key, specs);
            }
        }

        deps
    }

    /// Extract all story entries as [`StoryInfo`] structs.
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

    /// Returns all entries as (key, status) pairs.
    /// Used by the dependency resolution module to check status of
    /// non-eligible stories (e.g., whether a dependency is `done`).
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    /// Returns explicit dependencies parsed from `# depends-on:` inline comments.
    ///
    /// Maps story/epic key → list of raw dependency specs. These specs must be
    /// resolved to actual story keys via [`deps::resolve_comment_deps`] before
    /// they can be used in the dependency graph.
    pub fn comment_deps(&self) -> &HashMap<String, Vec<String>> {
        &self.comment_deps
    }

    /// Returns the total number of entries (including epics and retrospectives).
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// Watcher
// ---------------------------------------------------------------------------

/// The watcher polls sprint-status.yaml and identifies stories eligible for processing.
///
/// The watcher is a pure reader — it never writes to sprint-status.yaml or any
/// BMAD artifact. All mutations are performed by the BMAD agent during sessions
/// (Architecture Decision 2).
pub struct Watcher {
    /// Shared bot configuration (paths, polling interval).
    #[allow(dead_code)]
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
        let sprint_status_path =
            PathBuf::from(&config.bmad_paths.implementation_artifacts).join("sprint-status.yaml");
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
    /// Returns a list of [`StoryInfo`] with status `ready-for-dev`.
    ///
    /// # Errors
    /// - [`WatcherError::SprintStatusNotFound`] — file doesn't exist yet
    /// - [`WatcherError::SprintStatusRead`] — I/O error reading file
    /// - [`WatcherError::SprintStatusParse`] — malformed YAML
    /// - [`WatcherError::NoEligibleStories`] — no ready-for-dev stories found
    pub fn poll(&self) -> Result<Vec<StoryInfo>, WatcherError> {
        tracing::debug!(
            path = %self.sprint_status_path.display(),
            "Polling sprint-status.yaml"
        );

        let sprint_status = SprintStatusFile::load(&self.sprint_status_path, &self.story_dir)?;

        let all_stories = sprint_status.stories();
        let eligible = sprint_status.eligible_stories();

        // Log stories with needs-clarification status for observability
        for story in &all_stories {
            if story.status == "needs-clarification" {
                tracing::debug!(
                    action = "watcher_skip",
                    story_id = %story.story_id,
                    story_key = %story.story_key,
                    status = "needs-clarification",
                    "Skipping story — awaiting human clarification"
                );
            }
        }

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
        let comment_deps = sprint_status.comment_deps();
        let (filtered, cascade_count) = deps::filter_eligible(eligible, entries, comment_deps)?;

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

    /// Returns the path being polled (for diagnostics/logging).
    pub fn sprint_status_path(&self) -> &Path {
        &self.sprint_status_path
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
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
        assert!(
            info.dependencies.is_empty(),
            "Dependencies should be empty in Story 2.1"
        );
        assert_eq!(
            info.specs_path,
            PathBuf::from("/tmp/artifacts/1-2-cli-framework-daemon-lifecycle.md")
        );
    }

    #[test]
    fn test_story_info_from_key_single_word_slug() {
        let info = StoryInfo::from_key_and_status("3-1-supervisor", "backlog", Path::new("/tmp"));
        let info = info.expect("Should parse single-word slug");
        assert_eq!(info.epic_num, 3);
        assert_eq!(info.story_num, 1);
        assert_eq!(info.label, "supervisor");
    }

    #[test]
    fn test_story_info_rejects_epic_entry() {
        let result = StoryInfo::from_key_and_status("epic-1", "in-progress", Path::new("/tmp"));
        assert!(result.is_none(), "Should reject epic entries");
    }

    #[test]
    fn test_story_info_rejects_retrospective() {
        let result =
            StoryInfo::from_key_and_status("epic-1-retrospective", "optional", Path::new("/tmp"));
        assert!(result.is_none(), "Should reject retrospective entries");
    }

    #[test]
    fn test_story_info_rejects_non_numeric_start() {
        let result =
            StoryInfo::from_key_and_status("alpha-1-something", "backlog", Path::new("/tmp"));
        assert!(
            result.is_none(),
            "Should reject keys not starting with digit"
        );
    }

    #[test]
    fn test_story_info_is_eligible_ready_for_dev() {
        let info =
            StoryInfo::from_key_and_status("1-1-scaffolding", "ready-for-dev", Path::new("/tmp"))
                .unwrap();
        assert!(info.is_eligible());
    }

    #[test]
    fn test_story_info_is_not_eligible_backlog() {
        let info = StoryInfo::from_key_and_status("1-1-scaffolding", "backlog", Path::new("/tmp"))
            .unwrap();
        assert!(!info.is_eligible());
    }

    #[test]
    fn test_story_info_is_not_eligible_done() {
        let info =
            StoryInfo::from_key_and_status("1-1-scaffolding", "done", Path::new("/tmp")).unwrap();
        assert!(!info.is_eligible());
    }

    #[test]
    fn test_story_info_is_not_eligible_needs_clarification() {
        let story = StoryInfo::from_key_and_status(
            "3-3-human-escalation",
            "needs-clarification",
            Path::new("/tmp/stories"),
        )
        .unwrap();
        assert!(!story.is_eligible());
        assert_eq!(story.status, "needs-clarification");
    }

    #[test]
    fn test_story_info_display_format() {
        let info =
            StoryInfo::from_key_and_status("2-1-polling", "ready-for-dev", Path::new("/tmp"))
                .unwrap();
        let display = format!("{info}");
        assert!(display.contains("2.1"));
        assert!(display.contains("polling"));
        assert!(display.contains("ready-for-dev"));
        assert!(display.contains("story/2-1-polling"));
    }

    #[test]
    fn test_story_info_dependencies_default_empty() {
        let info =
            StoryInfo::from_key_and_status("2-1-polling", "ready-for-dev", Path::new("/tmp"))
                .unwrap();
        assert!(
            info.dependencies.is_empty(),
            "Story 2.1 must not populate dependencies"
        );
    }

    #[test]
    fn test_story_info_derives_correct_branch_name() {
        let info = StoryInfo::from_key_and_status(
            "4-3-pre-development-preparation",
            "backlog",
            Path::new("/artifacts"),
        )
        .unwrap();
        assert_eq!(info.branch_name, "story/4-3-pre-development-preparation");
    }

    // --- SprintStatusFile tests ---

    // -----------------------------------------------------------------------
    // parse_comment_deps tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_comment_deps_story_shorthand() {
        let content = indoc(
            "development_status:\n\
             \x20   4-2-session: done # depends-on: 4-1\n",
        );
        let deps = SprintStatusFile::parse_comment_deps(content);
        assert_eq!(deps.get("4-2-session").unwrap(), &vec!["4-1"]);
    }

    #[test]
    fn test_parse_comment_deps_epic_with_annotation() {
        let content = "  7-1-test-infra: ready-for-dev # depends-on: epic-8 (resolved)\n";
        let deps = SprintStatusFile::parse_comment_deps(content);
        assert_eq!(deps.get("7-1-test-infra").unwrap(), &vec!["epic-8"]);
    }

    #[test]
    fn test_parse_comment_deps_epics_range() {
        let content = "  epic-10: in-progress # depends-on: epics 1-6 (all implemented)\n";
        let deps = SprintStatusFile::parse_comment_deps(content);
        assert_eq!(deps.get("epic-10").unwrap(), &vec!["epics 1-6"]);
    }

    #[test]
    fn test_parse_comment_deps_comma_separated() {
        let content = "  5-3-live: ready-for-dev # depends-on: 5-1, 5-2\n";
        let deps = SprintStatusFile::parse_comment_deps(content);
        assert_eq!(deps.get("5-3-live").unwrap(), &vec!["5-1", "5-2"]);
    }

    #[test]
    fn test_parse_comment_deps_ignores_pure_comments() {
        let content = "# This is a header comment\n\
                       # depends-on: something\n\
                       1-1-foo: done\n";
        let deps = SprintStatusFile::parse_comment_deps(content);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_parse_comment_deps_ignores_non_depends_on_comments() {
        let content = "  1-1-foo: done # this is a normal comment\n";
        let deps = SprintStatusFile::parse_comment_deps(content);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_parse_comment_deps_empty_content() {
        let deps = SprintStatusFile::parse_comment_deps("");
        assert!(deps.is_empty());
    }

    #[test]
    fn test_parse_comment_deps_case_insensitive_prefix() {
        let content = "  1-2-bar: ready-for-dev # Depends-On: 1-1\n";
        let deps = SprintStatusFile::parse_comment_deps(content);
        assert_eq!(deps.get("1-2-bar").unwrap(), &vec!["1-1"]);
    }

    // helper for readability in tests
    fn indoc(s: &str) -> &str {
        s
    }

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
        let path = write_test_sprint_status(tmp.path(), "development_status:\n  - :\n  [invalid");
        let result = SprintStatusFile::load(&path, tmp.path());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            WatcherError::SprintStatusParse(_)
        ));
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
    fn test_sprint_status_eligible_stories_excludes_needs_clarification() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "development_status:\n  \
             epic-1: in-progress\n  \
             1-1-scaffolding: done\n  \
             1-2-cli: needs-clarification\n  \
             1-3-init: ready-for-dev\n";
        let path = write_test_sprint_status(tmp.path(), content);
        let ssf = SprintStatusFile::load(&path, tmp.path()).unwrap();
        let eligible = ssf.eligible_stories();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].story_key, "1-3-init");
        // Verify the needs-clarification story is in all stories but not eligible
        let all = ssf.stories();
        let nc = all.iter().find(|s| s.story_key == "1-2-cli").unwrap();
        assert_eq!(nc.status, "needs-clarification");
        assert!(!nc.is_eligible());
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
        assert!(matches!(
            result.unwrap_err(),
            WatcherError::NoEligibleStories
        ));
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
    /// Only `bmad_paths.implementation_artifacts` matters for watcher.
    pub(crate) fn make_test_bot_config(artifacts_dir: &Path) -> BotConfig {
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
                    reasoning_effort: None,
                },
                review: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test".to_string(),
                    reasoning_effort: None,
                },
                supervisor: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test".to_string(),
                    reasoning_effort: None,
                },
            },
            notifications: NotificationConfig {
                telegram: TelegramConfig {
                    enabled: false,
                    chat_id: String::new(),
                },
            },
            bmad_paths: BmadPathsConfig {
                project_root: artifacts_dir
                    .parent()
                    .unwrap_or(artifacts_dir)
                    .display()
                    .to_string(),
                output_folder: artifacts_dir.display().to_string(),
                planning_artifacts: artifacts_dir.display().to_string(),
                implementation_artifacts: artifacts_dir.display().to_string(),
            },
            log_format: "pretty".to_string(),
            log_level: "info".to_string(),
            log_file: "test.log".to_string(),
            code_review_enabled: true,
            mcp_servers: vec![],
        }
    }
}
