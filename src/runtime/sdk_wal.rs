//! SDK-specific WAL (Write-Ahead Log) operations.
//!
//! Manages WAL lifecycle for SDK sessions: create before subprocess spawn,
//! record session ID after capture, cleanup after session outcome.

use std::path::{Path, PathBuf};

use crate::session::state::SessionState;
use crate::watcher::StoryInfo;

pub struct SdkWal;

impl SdkWal {
    pub fn wal_path(implementation_artifacts: &str) -> PathBuf {
        Path::new(implementation_artifacts).join(".bmad-bot-session.yaml")
    }

    pub async fn create(
        story: &StoryInfo,
        provider: &str,
        model: &str,
        phase: &str,
    ) -> Result<(SessionState, PathBuf), crate::session::state::StateError> {
        let mut state = SessionState::new(story, provider, model);
        state.runtime_type = "sdk".to_string();
        state.set_pipeline_phase(phase);
        Ok((state, PathBuf::new()))
    }

    pub async fn record_session_id(
        wal_path: &Path,
        phase: &str,
        session_id: &str,
    ) -> Result<(), crate::session::state::StateError> {
        let mut state = SessionState::load(wal_path).await?;
        state
            .sdk_session_ids
            .insert(phase.to_string(), session_id.to_string());
        state.save(wal_path).await
    }

    pub async fn cleanup(wal_path: &Path) -> Result<(), crate::session::state::StateError> {
        SessionState::delete(wal_path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_test_story() -> StoryInfo {
        StoryInfo {
            story_id: "15.7".to_string(),
            story_key: "15-7-pipeline-dual-runtime".to_string(),
            epic_num: 15,
            story_num: 7,
            label: "pipeline-dual-runtime".to_string(),
            branch_name: "story/15-7-pipeline-dual-runtime".to_string(),
            specs_path: PathBuf::from("/tmp/15-7-pipeline-dual-runtime.md"),
            dependencies: vec![],
            status: "in-progress".to_string(),
        }
    }

    #[test]
    fn test_wal_path_construction() {
        let path = SdkWal::wal_path("/some/artifacts");
        assert_eq!(path, PathBuf::from("/some/artifacts/.bmad-bot-session.yaml"));
    }

    #[tokio::test]
    async fn test_sdk_wal_create_sets_runtime_type() {
        let story = make_test_story();
        let (state, _) = SdkWal::create(&story, "claude-code", "claude-sonnet-4-20250514", "dev")
            .await
            .unwrap();
        assert_eq!(state.runtime_type, "sdk");
        assert_eq!(state.pipeline_phase, "dev");
        assert_eq!(state.provider, "claude-code");
        assert_eq!(state.story_key, "15-7-pipeline-dual-runtime");
    }

    #[tokio::test]
    async fn test_sdk_wal_record_session_id_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let wal_path = dir.path().join(".bmad-bot-session.yaml");

        let story = make_test_story();
        let (state, _) = SdkWal::create(&story, "claude-code", "model", "dev")
            .await
            .unwrap();
        state.save(&wal_path).await.unwrap();

        SdkWal::record_session_id(&wal_path, "dev", "sess-abc-123")
            .await
            .unwrap();

        let loaded = SessionState::load(&wal_path).await.unwrap();
        assert_eq!(loaded.sdk_session_ids.get("dev").unwrap(), "sess-abc-123");
        assert_eq!(loaded.runtime_type, "sdk");
    }

    #[tokio::test]
    async fn test_sdk_wal_cleanup_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let wal_path = dir.path().join(".bmad-bot-session.yaml");

        let story = make_test_story();
        let (state, _) = SdkWal::create(&story, "codex", "o4-mini", "create")
            .await
            .unwrap();
        state.save(&wal_path).await.unwrap();
        assert!(wal_path.exists());

        SdkWal::cleanup(&wal_path).await.unwrap();
        assert!(!wal_path.exists());
    }

    #[tokio::test]
    async fn test_sdk_wal_cleanup_missing_file_no_error() {
        let path = PathBuf::from("/tmp/nonexistent-sdk-wal-test.yaml");
        assert!(SdkWal::cleanup(&path).await.is_ok());
    }
}
