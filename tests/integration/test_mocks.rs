//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::watcher::StoryInfo;

use crate::helpers::fixtures::make_test_story;

// ---------------------------------------------------------------------------
// MockGitProvider tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
    let pr = PrInfo {
        id: "42".to_string(),
        url: "https://github.com/test/repo/pull/42".to_string(),
        number: 42,
    };
    let mock = MockGitProvider::new().with_create_pr(Ok(pr.clone()));
    let result = mock
        .create_pr(CreatePrParams {
            title: "Test PR".to_string(),
            body: "Body".to_string(),
            source_branch: "feature".to_string(),
            target_branch: "main".to_string(),
        })
        .await;

    let info = result.expect("Expected Ok");
    assert_eq!(info.id, "42");
    assert_eq!(info.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new()
        .with_create_pr(Err(GitProviderError::ApiError {
            status: 500,
            message: "Internal Server Error".to_string(),
        }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "Test PR".to_string(),
            body: "Body".to_string(),
            source_branch: "feature".to_string(),
            target_branch: "main".to_string(),
        })
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    mock.create_pr(CreatePrParams {
        title: "PR1".to_string(),
        body: "Body1".to_string(),
        source_branch: "f1".to_string(),
        target_branch: "main".to_string(),
    })
    .await
    .unwrap();

    mock.add_comment("42", "LGTM").await.unwrap();
    mock.get_pr_url("42").await.unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);

    match &calls[0] {
        GitProviderCall::CreatePr(params) => assert_eq!(params.title, "PR1"),
        _ => panic!("Expected CreatePr call"),
    }
    match &calls[1] {
        GitProviderCall::AddComment { pr_id, body } => {
            assert_eq!(pr_id, "42");
            assert_eq!(body, "LGTM");
        }
        _ => panic!("Expected AddComment call"),
    }
    match &calls[2] {
        GitProviderCall::GetPrUrl(id) => assert_eq!(id, "42"),
        _ => panic!("Expected GetPrUrl call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_returns_configured() {
    let mock = MockGitProvider::new()
        .with_add_comment(Err(GitProviderError::AuthenticationFailed {
            reason: "bad token".to_string(),
        }));

    let result = mock.add_comment("1", "hello").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured() {
    let mock = MockGitProvider::new()
        .with_get_pr_url(Ok("https://custom-url.com/pr/99".to_string()));

    let url = mock.get_pr_url("99").await.unwrap();
    assert_eq!(url, "https://custom-url.com/pr/99");
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    let mock = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "7.1".to_string(),
        story_key: "7-1-test".to_string(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/pr/1".to_string()),
        reason: None,
    };

    mock.notify_story(&notification).await.unwrap();

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_id, "7.1");
    assert_eq!(story_calls[0].story_key, "7-1-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summary() {
    let mock = MockNotifier::new();
    let summary = RunSummary {
        stories: vec![],
        total_processed: 5,
        completed: 3,
        blocked: 1,
        errored: 1,
        fatal: false,
    };

    mock.notify_run_summary(&summary).await.unwrap();

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 5);
}

#[tokio::test]
async fn test_mock_notifier_returns_configured_error() {
    let mock = MockNotifier::new()
        .with_notify_story(Err(NotifierError::HttpRequest {
            reason: "connection refused".to_string(),
        }));

    let notification = StoryNotification {
        story_id: "1.1".to_string(),
        story_key: "1-1-test".to_string(),
        status: StoryStatus::Error,
        pr_url: None,
        reason: Some("failed".to_string()),
    };

    let result = mock.notify_story(&notification).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_notifier_all_calls_accessor() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".to_string(),
        story_key: "1-1-test".to_string(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };
    let summary = RunSummary {
        stories: vec![],
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
        fatal: false,
    };

    mock.notify_story(&notification).await.unwrap();
    mock.notify_run_summary(&summary).await.unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_default_completed() {
    let mock = MockSessionRunner::new();
    let story = make_test_story("7-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        MockSessionOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "test-story");
        }
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_outcome() {
    let mock = MockSessionRunner::new().with_outcome(MockSessionOutcome::Escalated {
        story_key: "7-1-test".to_string(),
        reason: "need human input".to_string(),
    });
    let story = make_test_story("7-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        MockSessionOutcome::Escalated { story_key, reason } => {
            assert_eq!(story_key, "7-1-test");
            assert_eq!(reason, "need human input");
        }
        _ => panic!("Expected Escalated outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();
    let story = make_test_story("7-1-test", "test", vec![]);

    mock.run(&story).await;
    mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "7-1-test");
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_returns_none() {
    let mock = MockSessionRunner::new();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_default_completed() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("7-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        MockReviewOutcome::Completed { report, .. } => {
            assert_eq!(report, "LGTM");
        }
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_outcome() {
    let mock = MockReviewRunner::new().with_outcome(MockReviewOutcome::Skipped {
        story_key: "7-1-test".to_string(),
        reason: "review disabled".to_string(),
    });
    let story = make_test_story("7-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        MockReviewOutcome::Skipped { reason, .. } => {
            assert_eq!(reason, "review disabled");
        }
        _ => panic!("Expected Skipped outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("7-1-test", "test", vec![]);

    mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-test");
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
