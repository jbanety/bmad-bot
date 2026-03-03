//! Self-verification tests for fixture builders.

use crate::helpers::fixtures;
use bmad_bot::session::SessionState;
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config (7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let tmp = tempfile::tempdir().unwrap();
    let config = fixtures::make_test_config(tmp.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert!(!config.bmad_paths.project_root.is_empty());
    assert!(config.validate().is_ok(), "config should pass validation");
}

// ---------------------------------------------------------------------------
// make_test_secrets (7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_has_dummy_tokens() {
    let secrets = fixtures::make_test_secrets();
    assert!(secrets.anthropic_api_key.as_ref().unwrap().contains("DO-NOT-USE"));
    assert!(secrets.openai_api_key.as_ref().unwrap().contains("DO-NOT-USE"));
    assert!(secrets.github_token.as_ref().unwrap().contains("DO-NOT-USE"));
    assert!(secrets.gitlab_token.as_ref().unwrap().contains("DO-NOT-USE"));
    assert!(secrets.telegram_bot_token.as_ref().unwrap().contains("DO-NOT-USE"));
    assert!(
        secrets
            .github_copilot_oauth_token
            .as_ref()
            .unwrap()
            .contains("DO-NOT-USE")
    );
}

// ---------------------------------------------------------------------------
// make_test_story (7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key_correctly() {
    let story = fixtures::make_test_story(
        "7-1-integration-test-infrastructure",
        "integration test infrastructure",
        vec!["6-3-crash-recovery".into()],
    );

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test-infrastructure");
    assert_eq!(story.label, "integration test infrastructure");
    assert_eq!(
        story.branch_name,
        "story/7-1-integration-test-infrastructure"
    );
    assert_eq!(story.status, "ready-for-dev");
    assert_eq!(story.dependencies.len(), 1);
    assert_eq!(story.dependencies[0], "6-3-crash-recovery");
    assert!(story
        .specs_path
        .to_str()
        .unwrap()
        .contains("7-1-integration-test-infrastructure.md"));
}

#[test]
fn test_make_test_story_no_deps() {
    let story = fixtures::make_test_story("1-1-scaffolding", "scaffolding", vec![]);
    assert!(story.dependencies.is_empty());
    assert_eq!(story.epic_num, 1);
    assert_eq!(story.story_num, 1);
}

// ---------------------------------------------------------------------------
// write_sprint_status (7.6)
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = fixtures::write_sprint_status(tmp.path(), entries);

    assert!(path.exists(), "sprint-status.yaml should exist");

    // Parse with the real loader
    let ssf = SprintStatusFile::load(&path, tmp.path()).expect("should parse");
    let stories = ssf.stories();
    assert_eq!(stories.len(), 2); // Only stories, not epics/retros
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[0].status, "done");
    assert_eq!(stories[1].story_key, "1-2-cli");
    assert_eq!(stories[1].status, "ready-for-dev");
}

#[test]
fn test_write_sprint_status_all_entry_types() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-slug", "ready-for-dev"),
        ("1-2-another-story", "backlog"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "backlog"),
        ("2-1-next-epic", "ready-for-dev"),
    ];

    let path = fixtures::write_sprint_status(tmp.path(), entries);
    let ssf = SprintStatusFile::load(&path, tmp.path()).expect("should parse");

    // Total entries include epics and retros
    assert_eq!(ssf.entry_count(), 6);
    // Only stories
    assert_eq!(ssf.stories().len(), 3);
    // Eligible = ready-for-dev
    assert_eq!(ssf.eligible_stories().len(), 2);
}

// ---------------------------------------------------------------------------
// write_wal_file (7.7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_write_wal_file_creates_parseable_yaml() {
    let tmp = tempfile::tempdir().unwrap();
    let state = fixtures::make_test_session_state("7-1-integration-test");
    let path = fixtures::write_wal_file(tmp.path(), &state);

    assert!(path.exists(), "WAL file should exist");
    assert!(path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with(".bmad-bot-session"));

    // Load it back with the real loader
    let loaded = SessionState::load(&path).await.expect("should load WAL");
    assert_eq!(loaded.story_key, "7-1-integration-test");
    assert_eq!(loaded.story_id, "7.1");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[1].role, "assistant");
    assert_eq!(loaded.branch_name, "story/7-1-integration-test");
    assert_eq!(loaded.base_branch, "main");
}

// ---------------------------------------------------------------------------
// create_test_repo (7.8)
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_creates_valid_git_repo() {
    let tmp = tempfile::tempdir().unwrap();
    fixtures::create_test_repo(tmp.path());

    // Check .git directory exists
    assert!(tmp.path().join(".git").exists(), ".git dir should exist");

    // Check that HEAD has a commit
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("git rev-parse should work");
    assert!(
        output.status.success(),
        "HEAD should have a commit: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the branch is named "main"
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(tmp.path())
        .output()
        .expect("git branch should work");
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(branch, "main", "branch should be 'main'");
}

#[test]
fn test_write_sprint_status_empty_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fixtures::write_sprint_status(tmp.path(), vec![]);
    assert!(path.exists(), "sprint-status.yaml should exist even with no entries");
    let ssf = SprintStatusFile::load(&path, tmp.path()).expect("should parse empty entries");
    assert_eq!(ssf.stories().len(), 0);
    assert_eq!(ssf.entry_count(), 0);
}
