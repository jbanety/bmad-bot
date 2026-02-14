//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use crate::helpers::fixtures::make_test_story;

// ---------------------------------------------------------------------------
// MockGitProvider tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_configured_value() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/test/pull/42".into(),
        number: 42,
    }));

    let params = CreatePrParams {
        title: "test".into(),
        body: "body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await.expect("should succeed");
    assert_eq!(result.id, "42");
    assert_eq!(result.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "internal error".into(),
    }));

    let params = CreatePrParams {
        title: "test".into(),
        body: "body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    let params = CreatePrParams {
        title: "test pr".into(),
        body: "body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let _ = mock.create_pr(params).await;
    let _ = mock.add_comment("1", "comment body").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);

    match &calls[0] {
        GitProviderCall::CreatePr(p) => assert_eq!(p.title, "test pr"),
        _ => panic!("Expected CreatePr call"),
    }
    match &calls[1] {
        GitProviderCall::AddComment(id, body) => {
            assert_eq!(id, "1");
            assert_eq!(body, "comment body");
        }
        _ => panic!("Expected AddComment call"),
    }
    match &calls[2] {
        GitProviderCall::GetPrUrl(id) => assert_eq!(id, "1"),
        _ => panic!("Expected GetPrUrl call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_default_ok() {
    let mock = MockGitProvider::new();
    let result = mock.add_comment("1", "test").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_default_ok() {
    let mock = MockGitProvider::new();
    let result = mock.get_pr_url("1").await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("github.com"));
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/pull/1".into()),
        reason: None,
    };

    mock.notify_story(&notification).await.expect("should succeed");

    let calls = mock.story_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summary() {
    let mock = MockNotifier::new();

    let summary = RunSummary {
        stories: vec![],
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
        fatal: false,
    };

    mock.notify_run_summary(&summary).await.expect("should succeed");

    let calls = mock.summary_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].total_processed, 0);
}

#[tokio::test]
async fn test_mock_notifier_mixed_calls() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
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

    mock.notify_story(&notification).await.expect("should succeed");
    mock.notify_run_summary(&summary).await.expect("should succeed");

    let all_calls = mock.calls();
    assert_eq!(all_calls.len(), 2);
    assert_eq!(mock.story_calls().len(), 1);
    assert_eq!(mock.summary_calls().len(), 1);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_default_completed() {
    let story = make_test_story("7-1-test-story", "test story", vec![]);
    let runner = MockSessionRunner::new();

    let outcome = runner.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "7-1-test-story");
        }
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_failed() {
    let story = make_test_story("7-1-test-story", "test story", vec![]);
    let runner = MockSessionRunner::new().with_outcome(|s| SessionOutcome::Failed {
        story_key: s.story_key.clone(),
        error: "test error".into(),
        decisions: vec![],
    });

    let outcome = runner.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => {
            assert_eq!(error, "test error");
        }
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let story = make_test_story("7-1-test-story", "test story", vec![]);
    let runner = MockSessionRunner::new();

    runner.run(&story).await;
    runner.run(&story).await;

    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "7-1-test-story");
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_returns_none() {
    let runner = MockSessionRunner::new();
    let result = runner.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_default_completed() {
    let story = make_test_story("7-1-test-story", "test story", vec![]);
    let runner = MockReviewRunner::new();

    let outcome = runner.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { story_key, report, .. } => {
            assert_eq!(story_key, "7-1-test-story");
            assert_eq!(report, "Mock review report");
        }
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_failed() {
    let story = make_test_story("7-1-test-story", "test story", vec![]);
    let runner = MockReviewRunner::new().with_outcome(|s| ReviewOutcome::Failed {
        story_key: s.story_key.clone(),
        error: "review failure".into(),
    });

    let outcome = runner.run(&story).await;
    match outcome {
        ReviewOutcome::Failed { error, .. } => {
            assert_eq!(error, "review failure");
        }
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let story = make_test_story("7-1-test-story", "test story", vec![]);
    let runner = MockReviewRunner::new();

    runner.run(&story).await;

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-test-story");
}

// ---------------------------------------------------------------------------
// Send + Sync verification
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
