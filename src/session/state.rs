//! Session WAL (Write-Ahead Log) file management.
//!
//! Tracks in-progress session state for crash recovery. The WAL file is written
//! after every chat turn using an atomic write pattern (write to `.tmp` then
//! rename) to prevent corruption if the daemon crashes mid-write.
//!
//! Key types:
//! - [`SessionState`] — full session state including chat history
//! - [`ChatMessage`] — a single user or assistant message
//! - [`StateError`] — typed errors for WAL operations

use crate::watcher::StoryInfo;
use rig::completion::Message;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Pipeline phase: create-story session.
pub const PHASE_CREATE: &str = "create";
/// Pipeline phase: adversarial consultation during create-story.
pub const PHASE_CREATE_ADVERSARIAL_CONSULT: &str = "create-adversarial-consult";
/// Pipeline phase: critic consultation during create-story.
pub const PHASE_CREATE_CRITIC_CONSULT: &str = "create-critic-consult";
/// Pipeline phase: dev-story session.
pub const PHASE_DEV: &str = "dev";
/// Pipeline phase: code-review session.
pub const PHASE_REVIEW: &str = "review";
/// Pipeline phase: critic consultation during code-review.
pub const PHASE_REVIEW_CRITIC_CONSULT: &str = "review-critic-consult";

/// A single message in the chat history.
///
/// Stores the role (`"user"` or `"assistant"`) and content of each message
/// exchanged between the daemon and the LLM agent. Used for WAL serialization
/// and reconstruction of rig `Message` vectors on crash recovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    /// Message role — `"user"` or `"assistant"`.
    pub role: String,
    /// Message content text.
    pub content: String,
}

/// Errors originating from WAL state file operations.
///
/// Each variant carries the file path and a human-readable reason for
/// structured logging via `tracing`.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// Failed to write the state file (includes atomic write failures).
    #[error("Failed to write state file '{path}': {reason}")]
    Write {
        /// Path to the state file.
        path: String,
        /// Description of the write failure.
        reason: String,
    },

    /// Failed to read the state file from disk.
    #[error("Failed to read state file '{path}': {reason}")]
    Read {
        /// Path to the state file.
        path: String,
        /// Description of the read failure.
        reason: String,
    },

    /// Failed to parse the state file content (invalid YAML).
    #[error("Failed to parse state file '{path}': {reason}")]
    Parse {
        /// Path to the state file.
        path: String,
        /// Description of the parse failure.
        reason: String,
    },

    /// Failed to delete the state file.
    #[error("Failed to delete state file '{path}': {reason}")]
    Delete {
        /// Path to the state file.
        path: String,
        /// Description of the delete failure.
        reason: String,
    },
}

/// Full session state persisted as a WAL file for crash recovery.
///
/// Written to disk after every chat turn using [`save()`](Self::save). On daemon
/// restart, [`load()`](Self::load) reconstructs the state and
/// [`to_rig_messages()`](Self::to_rig_messages) rebuilds the rig chat history.
///
/// Location: `{implementation_artifacts}/.bmad-bot-session.yaml` (dot-prefixed,
/// transient file).
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionState {
    /// Story ID (dot-separated, e.g., "4.2").
    pub story_id: String,
    /// Story key (dash-separated, e.g., "4-2-agent-session-setup-chat-loop").
    pub story_key: String,
    /// Git branch name (e.g., "story/4-2-agent-session-setup-chat-loop").
    pub branch: String,
    /// ISO 8601 timestamp of session start.
    pub started_at: String,
    /// ISO 8601 timestamp of last activity (updated each turn).
    pub last_activity: String,
    /// LLM provider name for reconstruction on recovery.
    pub provider: String,
    /// LLM model name for reconstruction on recovery.
    pub model: String,
    /// The story branch name used for this session (e.g., "story/4-3-branch-mgmt").
    ///
    /// Added in Story 4.3. `serde(default)` ensures backward-compatibility with
    /// WAL files from Story 4.2 that lack this field.
    #[serde(default)]
    pub branch_name: String,
    /// The base branch this story branch was created from (e.g., "main" or "story/4-2-...").
    ///
    /// Used by Epic 5 (PR creation) to set the PR target branch, and by Story 6.3
    /// (crash recovery) to verify branch state on restart.
    #[serde(default)]
    pub base_branch: String,
    /// The skill path used for this session (e.g., ".claude/skills/bmad-create-story/SKILL.md").
    ///
    /// Persisted so crash recovery can rebuild the correct preamble and activation
    /// for the session phase (create-story vs dev-story).
    #[serde(default)]
    pub skill_path: String,
    /// The active pipeline phase for crash recovery routing.
    ///
    /// Updated at each phase transition BEFORE the phase starts (write-ahead).
    /// Recovery uses this to determine whether to restart from scratch (create/review)
    /// or attempt mid-session recovery (dev).
    #[serde(default)]
    pub pipeline_phase: String,
    /// Complete serialized chat history.
    pub chat_history: Vec<ChatMessage>,
}

impl SessionState {
    /// Create a new session state with empty chat history.
    ///
    /// The `started_at` and `last_activity` fields are set to the current
    /// UTC time in RFC 3339 format.
    pub fn new(story: &StoryInfo, provider: &str, model: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            story_id: story.story_id.clone(),
            story_key: story.story_key.clone(),
            branch: story.branch_name.clone(),
            started_at: now.clone(),
            last_activity: now,
            provider: provider.to_string(),
            model: model.to_string(),
            branch_name: String::new(),
            base_branch: String::new(),
            skill_path: String::new(),
            pipeline_phase: String::new(),
            chat_history: Vec::new(),
        }
    }

    /// Set the pipeline phase (structural metadata, not chat activity).
    ///
    /// Updates `pipeline_phase` without touching `last_activity` — phase
    /// transitions are WAL bookkeeping, not user/assistant interactions.
    pub fn set_pipeline_phase(&mut self, phase: &str) {
        self.pipeline_phase = phase.to_string();
    }

    /// Set the branch info after successful branch setup.
    ///
    /// Called by `SessionRunner::run()` after `ensure_story_branch()` succeeds.
    /// The WAL should be saved immediately after calling this method.
    pub fn set_branch_info(&mut self, branch_name: &str, base_branch: &str) {
        self.branch_name = branch_name.to_string();
        self.base_branch = base_branch.to_string();
    }

    /// Append a user message to the chat history and update `last_activity`.
    pub fn add_user_message(&mut self, content: &str) {
        self.last_activity = chrono::Utc::now().to_rfc3339();
        self.chat_history.push(ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
        });
    }

    /// Append an assistant message to the chat history and update `last_activity`.
    pub fn add_assistant_message(&mut self, content: &str) {
        self.last_activity = chrono::Utc::now().to_rfc3339();
        self.chat_history.push(ChatMessage {
            role: "assistant".to_string(),
            content: content.to_string(),
        });
    }

    /// Convert the stored chat history to a vector of rig [`Message`] values.
    ///
    /// Used to rebuild the history parameter for `agent.chat()` on each turn
    /// or after crash recovery.
    pub fn to_rig_messages(&self) -> Vec<Message> {
        self.chat_history
            .iter()
            .map(|msg| match msg.role.as_str() {
                "user" => Message::user(&msg.content),
                _ => Message::assistant(&msg.content),
            })
            .collect()
    }

    /// Persist the session state to disk using atomic write.
    ///
    /// Writes to `{path}.tmp` first, then renames to the final path. This
    /// prevents corruption if the daemon crashes mid-write.
    pub async fn save(&self, path: &Path) -> Result<(), StateError> {
        let yaml = serde_yml::to_string(self).map_err(|e| StateError::Write {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

        let tmp_path = path.with_extension("yaml.tmp");

        tokio::fs::write(&tmp_path, &yaml)
            .await
            .map_err(|e| StateError::Write {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;

        tokio::fs::rename(&tmp_path, path)
            .await
            .map_err(|e| StateError::Write {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;

        Ok(())
    }

    /// Load session state from a YAML file on disk.
    pub async fn load(path: &Path) -> Result<Self, StateError> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| StateError::Read {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;

        let state: Self = serde_yml::from_str(&content).map_err(|e| StateError::Parse {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

        Ok(state)
    }

    /// Delete the session state file from disk.
    ///
    /// Ignores `NotFound` errors (file already gone). Returns an error only
    /// for other I/O failures.
    pub async fn delete(path: &Path) -> Result<(), StateError> {
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StateError::Delete {
                path: path.display().to_string(),
                reason: e.to_string(),
            }),
        }
    }

    /// Check whether a state file exists at the given path.
    ///
    /// Used at daemon startup to detect incomplete sessions (crash recovery).
    pub fn exists(path: &Path) -> bool {
        path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Helper: create a minimal `StoryInfo` for tests.
    fn make_test_story() -> StoryInfo {
        StoryInfo {
            story_id: "4.2".to_string(),
            story_key: "4-2-agent-session-setup-chat-loop".to_string(),
            epic_num: 4,
            story_num: 2,
            label: "agent-session-setup-chat-loop".to_string(),
            branch_name: "story/4-2-agent-session-setup-chat-loop".to_string(),
            specs_path: PathBuf::from(
                "_bmad-output/implementation-artifacts/4-2-agent-session-setup-chat-loop.md",
            ),
            dependencies: vec!["4-1-rig-tools-implementation-git-filesystem-terminal".to_string()],
            status: "in-progress".to_string(),
        }
    }

    #[test]
    fn test_session_state_new_has_empty_history() {
        let story = make_test_story();
        let state = SessionState::new(&story, "anthropic", "claude-sonnet-4-20250514");

        assert_eq!(state.story_id, "4.2");
        assert_eq!(state.story_key, "4-2-agent-session-setup-chat-loop");
        assert_eq!(state.branch, "story/4-2-agent-session-setup-chat-loop");
        assert_eq!(state.provider, "anthropic");
        assert_eq!(state.model, "claude-sonnet-4-20250514");
        assert!(state.chat_history.is_empty());
        assert!(!state.started_at.is_empty());
        assert!(!state.last_activity.is_empty());
        // Branch fields default to empty (set later by set_branch_info)
        assert!(state.branch_name.is_empty());
        assert!(state.base_branch.is_empty());
    }

    #[test]
    fn test_session_state_add_messages_preserves_order() {
        let story = make_test_story();
        let mut state = SessionState::new(&story, "anthropic", "test-model");

        state.add_user_message("DS");
        state.add_assistant_message("Starting dev-story workflow...");
        state.add_user_message("Continue.");
        state.add_assistant_message("Task 1 complete.");

        assert_eq!(state.chat_history.len(), 4);
        assert_eq!(state.chat_history[0].role, "user");
        assert_eq!(state.chat_history[0].content, "DS");
        assert_eq!(state.chat_history[1].role, "assistant");
        assert_eq!(
            state.chat_history[1].content,
            "Starting dev-story workflow..."
        );
        assert_eq!(state.chat_history[2].role, "user");
        assert_eq!(state.chat_history[2].content, "Continue.");
        assert_eq!(state.chat_history[3].role, "assistant");
        assert_eq!(state.chat_history[3].content, "Task 1 complete.");
    }

    #[test]
    fn test_session_state_to_rig_messages_converts_correctly() {
        let story = make_test_story();
        let mut state = SessionState::new(&story, "anthropic", "test-model");

        state.add_user_message("DS");
        state.add_assistant_message("Hello!");
        state.add_user_message("Continue.");

        let messages = state.to_rig_messages();
        assert_eq!(messages.len(), 3);
        // Verify we get Message values (we can't inspect internals, but
        // we verify the conversion completes without panic for all roles).
    }

    #[tokio::test]
    async fn test_session_state_save_load_roundtrip() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join(".bmad-bot-session.yaml");

        let story = make_test_story();
        let mut state = SessionState::new(&story, "openai", "gpt-4o");
        state.add_user_message("DS");
        state.add_assistant_message("Starting...");

        state.save(&path).await.expect("save should succeed");

        let loaded = SessionState::load(&path)
            .await
            .expect("load should succeed");
        assert_eq!(loaded.story_id, state.story_id);
        assert_eq!(loaded.story_key, state.story_key);
        assert_eq!(loaded.branch, state.branch);
        assert_eq!(loaded.provider, state.provider);
        assert_eq!(loaded.model, state.model);
        assert_eq!(loaded.chat_history.len(), 2);
        assert_eq!(loaded.chat_history[0].role, "user");
        assert_eq!(loaded.chat_history[0].content, "DS");
        assert_eq!(loaded.chat_history[1].role, "assistant");
        assert_eq!(loaded.chat_history[1].content, "Starting...");
    }

    #[tokio::test]
    async fn test_session_state_save_creates_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join(".bmad-bot-session.yaml");

        assert!(!path.exists());

        let story = make_test_story();
        let state = SessionState::new(&story, "anthropic", "test");
        state.save(&path).await.expect("save should succeed");

        assert!(path.exists());
    }

    #[tokio::test]
    async fn test_session_state_load_missing_file_returns_error() {
        let path = PathBuf::from("/tmp/nonexistent-bmad-state-12345.yaml");
        let result = SessionState::load(&path).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            StateError::Read { path: p, .. } => {
                assert!(p.contains("nonexistent-bmad-state-12345"));
            }
            other => panic!("Expected Read, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_session_state_delete_removes_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join(".bmad-bot-session.yaml");

        // Create the file first
        let story = make_test_story();
        let state = SessionState::new(&story, "anthropic", "test");
        state.save(&path).await.expect("save should succeed");
        assert!(path.exists());

        // Delete it
        SessionState::delete(&path)
            .await
            .expect("delete should succeed");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn test_session_state_delete_missing_file_no_error() {
        let path = PathBuf::from("/tmp/nonexistent-bmad-state-delete-99999.yaml");
        let result = SessionState::delete(&path).await;
        assert!(result.is_ok(), "Deleting a missing file should not error");
    }

    #[test]
    fn test_session_state_exists_true_when_present() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("test-state.yaml");
        std::fs::write(&path, "dummy").expect("write dummy");
        assert!(SessionState::exists(&path));
    }

    #[test]
    fn test_session_state_exists_false_when_absent() {
        let path = PathBuf::from("/tmp/nonexistent-bmad-state-exists-77777.yaml");
        assert!(!SessionState::exists(&path));
    }

    #[tokio::test]
    async fn test_session_state_serializable_yaml_roundtrip() {
        let story = make_test_story();
        let mut state = SessionState::new(&story, "openai", "gpt-4o");
        state.add_user_message("Hello");
        state.add_assistant_message("Hi there");

        let yaml = serde_yml::to_string(&state).expect("serialize to YAML");
        let deserialized: SessionState = serde_yml::from_str(&yaml).expect("deserialize from YAML");

        assert_eq!(deserialized.story_id, state.story_id);
        assert_eq!(deserialized.story_key, state.story_key);
        assert_eq!(deserialized.branch, state.branch);
        assert_eq!(deserialized.provider, state.provider);
        assert_eq!(deserialized.model, state.model);
        assert_eq!(deserialized.started_at, state.started_at);
        assert_eq!(deserialized.chat_history.len(), 2);
        assert_eq!(deserialized.chat_history[0], state.chat_history[0]);
        assert_eq!(deserialized.chat_history[1], state.chat_history[1]);
    }

    #[test]
    fn test_state_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<StateError>();
    }

    // -----------------------------------------------------------------------
    // Branch info tests (Story 4.3)
    // -----------------------------------------------------------------------

    #[test]
    fn test_session_state_branch_fields_default_empty() {
        // Simulate deserializing a WAL from Story 4.2 (no branch fields)
        let yaml = r#"
story_id: "4.2"
story_key: "4-2-agent-session-setup-chat-loop"
branch: "story/4-2-agent-session-setup-chat-loop"
started_at: "2026-02-07T00:00:00Z"
last_activity: "2026-02-07T00:00:00Z"
provider: "anthropic"
model: "claude-sonnet-4-20250514"
chat_history: []
"#;
        let state: SessionState = serde_yml::from_str(yaml).expect("deserialize old WAL");
        assert!(
            state.branch_name.is_empty(),
            "branch_name should default to empty"
        );
        assert!(
            state.base_branch.is_empty(),
            "base_branch should default to empty"
        );
    }

    // -----------------------------------------------------------------------
    // Pipeline phase tests (Story 13.10)
    // -----------------------------------------------------------------------

    #[test]
    fn test_session_state_pipeline_phase_serialization_roundtrip() {
        let story = make_test_story();
        let mut state = SessionState::new(&story, "anthropic", "test-model");
        state.set_pipeline_phase(super::PHASE_CREATE);

        let yaml = serde_yml::to_string(&state).expect("serialize");
        let loaded: SessionState = serde_yml::from_str(&yaml).expect("deserialize");

        assert_eq!(loaded.pipeline_phase, "create");
        assert_eq!(loaded.story_id, state.story_id);
        assert_eq!(loaded.story_key, state.story_key);
    }

    #[test]
    fn test_session_state_legacy_wal_without_pipeline_phase() {
        let yaml = r#"
story_id: "4.2"
story_key: "4-2-agent-session-setup-chat-loop"
branch: "story/4-2-agent-session-setup-chat-loop"
started_at: "2026-02-07T00:00:00Z"
last_activity: "2026-02-07T00:00:00Z"
provider: "anthropic"
model: "claude-sonnet-4-20250514"
chat_history: []
"#;
        let state: SessionState = serde_yml::from_str(yaml).expect("deserialize legacy WAL");
        assert!(
            state.pipeline_phase.is_empty(),
            "pipeline_phase should default to empty string for legacy WAL"
        );
    }

    #[test]
    fn test_set_pipeline_phase_does_not_update_last_activity() {
        let story = make_test_story();
        let state = SessionState::new(&story, "anthropic", "test-model");
        let original_last_activity = state.last_activity.clone();

        let mut state = state;
        std::thread::sleep(std::time::Duration::from_millis(10));
        state.set_pipeline_phase(super::PHASE_DEV);

        assert_eq!(state.pipeline_phase, "dev");
        assert_eq!(
            state.last_activity, original_last_activity,
            "set_pipeline_phase must NOT update last_activity"
        );
    }

    #[test]
    fn test_session_state_new_has_empty_pipeline_phase() {
        let story = make_test_story();
        let state = SessionState::new(&story, "anthropic", "test-model");
        assert!(
            state.pipeline_phase.is_empty(),
            "new() should initialize pipeline_phase as empty string"
        );
    }

    #[test]
    fn test_session_state_set_branch_info_roundtrip() {
        let story = make_test_story();
        let mut state = SessionState::new(&story, "anthropic", "test-model");

        state.set_branch_info("story/4-3-branch-mgmt", "story/4-2-session-setup");

        assert_eq!(state.branch_name, "story/4-3-branch-mgmt");
        assert_eq!(state.base_branch, "story/4-2-session-setup");

        // Serialize and deserialize to verify roundtrip
        let yaml = serde_yml::to_string(&state).expect("serialize");
        let loaded: SessionState = serde_yml::from_str(&yaml).expect("deserialize");

        assert_eq!(loaded.branch_name, "story/4-3-branch-mgmt");
        assert_eq!(loaded.base_branch, "story/4-2-session-setup");
        // Verify other fields preserved
        assert_eq!(loaded.story_id, state.story_id);
        assert_eq!(loaded.story_key, state.story_key);
    }
}
