//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;
use bmad_bot::git_provider::{CreatePrParams, GitProviderError, PrInfo};
use bmad_bot::notifier::{NotifierError, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;
use std::path::PathBuf;

/// Helper: create a minimal `StoryInfo` for tests.
fn test_story() -> StoryInfo {
    StoryInfo {
        story_id: "7.1".to_string(),
        story_key: "7-1-integration-test-infrastructure".to_string(),
        epic_num: 7,
        story_num: 1,
        label: "integration-test-infrastructure".to_string(),
        branch_name: "story/7-1-integration-test-infrastructure".to_string(),
        specs_path: PathBuf::from(
            "_bmad-output/implementation-artifacts/7-1-integration-test-infrastructure.md",
        ),
        dependencies: vec![],
        status: "ready-for-dev".to_string(),
    }
}

// ---------------------------------------------------------------------------
// MockGitProvider tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/test/pull/42".into(),
        number: 42,
    }));

    let params = CreatePrParams {
        title: "feat: test".into(),
        body: "test body".into(),
        source_branch: "story/7-1-test".into(),
        target_branch: "main".into(),
    };

    let result = <MockGitProvider as bmad_bot::git_provider::GitProvider>::create_pr(&mock, params)
        .await
        .expect("should succeed");
    assert_eq!(result.id, "42");
    assert_eq!(result.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "Internal Server Error".into(),
    }));

    let params = CreatePrParams {
        title: "feat: test".into(),
        body: "test body".into(),
        source_branch: "story/7-1-test".into(),
        target_branch: "main".into(),
    };

    let result =
        <MockGitProvider as bmad_bot::git_provider::GitProvider>::create_pr(&mock, params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_create_pr_calls() {
    let mock = MockGitProvider::new();

    let params = CreatePrParams {
        title: "feat: test".into(),
        body: "test body".into(),
        source_branch: "story/7-1-test".into(),
        target_branch: "main".into(),
    };

    let _ =
        <MockGitProvider as bmad_bot::git_provider::GitProvider>::create_pr(&mock, params).await;

    let calls = mock.create_pr_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].params.title, "feat: test");
}

#[tokio::test]
async fn test_mock_git_provider_tracks_add_comment_calls() {
    let mock = MockGitProvider::new();

    let _ = <MockGitProvider as bmad_bot::git_provider::GitProvider>::add_comment(
        &mock,
        "42",
        "review comment",
    )
    .await;

    let calls = mock.add_comment_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].pr_id, "42");
    assert_eq!(calls[0].body, "review comment");
}

#[tokio::test]
async fn test_mock_git_provider_tracks_get_pr_url_calls() {
    let mock = MockGitProvider::new();

    let _ =
        <MockGitProvider as bmad_bot::git_provider::GitProvider>::get_pr_url(&mock, "42").await;

    let calls = mock.get_pr_url_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].pr_id, "42");
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/test/pull/1".into()),
        reason: None,
    };

    <MockNotifier as bmad_bot::notifier::Notifier>::notify_story(&mock, &notification)
        .await
        .expect("should succeed");

    let calls = mock.story_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summary() {
    let mock = MockNotifier::new();

    let summary = RunSummary {
        stories: vec![],
        total_processed: 3,
        completed: 2,
        blocked: 1,
        errored: 0,
        fatal: false,
    };

    <MockNotifier as bmad_bot::notifier::Notifier>::notify_run_summary(&mock, &summary)
        .await
        .expect("should succeed");

    let calls = mock.summary_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_calls_returns_all_calls() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };
    let summary = RunSummary {
        stories: vec![],
        total_processed: 1,
        completed: 1,
        blocked: 0,
        errored: 0,
        fatal: false,
    };

    <MockNotifier as bmad_bot::notifier::Notifier>::notify_story(&mock, &notification)
        .await
        .expect("ok");
    <MockNotifier as bmad_bot::notifier::Notifier>::notify_run_summary(&mock, &summary)
        .await
        .expect("ok");

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
}

#[tokio::test]
async fn test_mock_notifier_returns_configured_error() {
    let mock = MockNotifier::new().with_story_result(Err(NotifierError::Disabled));

    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };

    let result =
        <MockNotifier as bmad_bot::notifier::Notifier>::notify_story(&mock, &notification).await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed_by_default() {
    let mock = MockSessionRunner::new();
    let story = test_story();

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "7-1-integration-test-infrastructure");
        }
        _ => panic!("Expected SessionOutcome::Completed"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_outcome() {
    let mock = MockSessionRunner::new().with_outcome(|story| SessionOutcome::Failed {
        story_key: story.story_key.clone(),
        error: "test failure".into(),
        decisions: vec![],
    });
    let story = test_story();

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => {
            assert_eq!(error, "test failure");
        }
        _ => panic!("Expected SessionOutcome::Failed"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();
    let story = test_story();

    let _ = mock.run(&story).await;
    let _ = mock.run(&story).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "7-1-integration-test-infrastructure");
}

#[tokio::test]
async fn test_mock_session_runner_check_and_recover_wal_returns_none() {
    let mock = MockSessionRunner::new();
    assert!(mock.check_and_recover_wal().await.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed_by_default() {
    let mock = MockReviewRunner::new();
    let story = test_story();

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "7-1-integration-test-infrastructure");
        }
        _ => panic!("Expected ReviewOutcome::Completed"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_outcome() {
    let mock = MockReviewRunner::new().with_outcome(|story| ReviewOutcome::Failed {
        story_key: story.story_key.clone(),
        error: "review crash".into(),
    });
    let story = test_story();

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Failed { error, .. } => {
            assert_eq!(error, "review crash");
        }
        _ => panic!("Expected ReviewOutcome::Failed"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let story = test_story();

    let _ = mock.run(&story).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-integration-test-infrastructure");
}

// ---------------------------------------------------------------------------
// Send + Sync bounds
// ---------------------------------------------------------------------------

#[test]
fn test_mock_git_provider_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockGitProvider>();
}

#[test]
fn test_mock_notifier_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockNotifier>();
}

#[test]
fn test_mock_session_runner_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockSessionRunner>();
}

#[test]
fn test_mock_review_runner_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockReviewRunner>();
}
