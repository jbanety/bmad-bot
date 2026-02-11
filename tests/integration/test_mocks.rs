//! Self-verification tests for mock implementations.

use crate::helpers::fixtures;
use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, StoryNotification, StoryStatus, RunSummary};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

// ---------------------------------------------------------------------------
// MockGitProvider tests (Task 7.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/owner/repo/pull/42".into(),
        number: 42,
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "test".into(),
            body: "body".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
        })
        .await;

    let info = result.expect("should succeed");
    assert_eq!(info.id, "42");
    assert_eq!(info.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 422,
        message: "Validation failed".into(),
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "test".into(),
            body: "body".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
        })
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    mock.create_pr(CreatePrParams {
        title: "PR1".into(),
        body: "".into(),
        source_branch: "a".into(),
        target_branch: "main".into(),
    })
    .await
    .unwrap();

    mock.add_comment("1", "LGTM").await.unwrap();
    mock.get_pr_url("1").await.unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);

    match &calls[0] {
        GitProviderCall::CreatePr(params) => assert_eq!(params.title, "PR1"),
        other => panic!("Expected CreatePr, got {:?}", other),
    }
    match &calls[1] {
        GitProviderCall::AddComment(id, body) => {
            assert_eq!(id, "1");
            assert_eq!(body, "LGTM");
        }
        other => panic!("Expected AddComment, got {:?}", other),
    }
    match &calls[2] {
        GitProviderCall::GetPrUrl(id) => assert_eq!(id, "1"),
        other => panic!("Expected GetPrUrl, got {:?}", other),
    }
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_configurable() {
    let mock =
        MockGitProvider::new().with_add_comment(Err(GitProviderError::AuthenticationFailed {
            reason: "bad token".into(),
        }));

    let result = mock.add_comment("1", "comment").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_configurable() {
    let mock = MockGitProvider::new()
        .with_get_pr_url(Ok("https://custom-url.com/pr/99".into()));

    let url = mock.get_pr_url("99").await.unwrap();
    assert_eq!(url, "https://custom-url.com/pr/99");
}

// ---------------------------------------------------------------------------
// MockNotifier tests (Task 7.2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/pr/1".into()),
        reason: None,
    };

    mock.notify_story(&notification).await.unwrap();

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_id, "1.1");
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
    };

    mock.notify_run_summary(&summary).await.unwrap();

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_returns_configured_error() {
    let mock = MockNotifier::new().with_story_result(Err(NotifierError::Disabled));

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Error,
        pr_url: None,
        reason: Some("test error".into()),
    };

    let result = mock.notify_story(&notification).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_notifier_calls_returns_all() {
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
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
    };

    mock.notify_story(&notification).await.unwrap();
    mock.notify_run_summary(&summary).await.unwrap();

    assert_eq!(mock.calls().len(), 2);
    assert_eq!(mock.story_calls().len(), 1);
    assert_eq!(mock.summary_calls().len(), 1);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests (Task 7.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let story = fixtures::make_test_story("1-1-test", "test", &[]);
    let mock = MockSessionRunner::new();

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "test-story");
        }
        other => panic!("Expected Completed, got {:?}", other),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_outcome() {
    let story = fixtures::make_test_story("2-1-auth", "auth", &[]);
    let mock = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "2-1-auth".into(),
        error: "test failure".into(),
        decisions: Vec::new(),
    });

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { story_key, error, .. } => {
            assert_eq!(story_key, "2-1-auth");
            assert_eq!(error, "test failure");
        }
        other => panic!("Expected Failed, got {:?}", other),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let story = fixtures::make_test_story("3-1-supervisor", "supervisor", &[]);
    let mock = MockSessionRunner::new();

    mock.run(&story).await;
    mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "3-1-supervisor");
}

#[tokio::test]
async fn test_mock_session_runner_wal_returns_none() {
    let mock = MockSessionRunner::new();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests (Task 7.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let story = fixtures::make_test_story("1-1-test", "test", &[]);
    let mock = MockReviewRunner::new();

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "test-story");
        }
        other => panic!("Expected Completed, got {:?}", other),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_outcome() {
    let story = fixtures::make_test_story("5-1-pr", "pr", &[]);
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Skipped {
        reason: "disabled".into(),
    });

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => {
            assert_eq!(reason, "disabled");
        }
        other => panic!("Expected Skipped, got {:?}", other),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let story = fixtures::make_test_story("5-2-review", "review", &[]);
    let mock = MockReviewRunner::new();

    mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "5-2-review");
}

#[tokio::test]
async fn test_mock_review_runner_failed_outcome() {
    let story = fixtures::make_test_story("5-2-review", "review", &[]);
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Failed {
        story_key: "5-2-review".into(),
        error: "crashed".into(),
    });

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Failed { story_key, error } => {
            assert_eq!(story_key, "5-2-review");
            assert_eq!(error, "crashed");
        }
        other => panic!("Expected Failed, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Send + Sync bound tests (Task 7.9)
// ---------------------------------------------------------------------------

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn test_mock_git_provider_is_send_sync() {
    assert_send_sync::<MockGitProvider>();
}

#[test]
fn test_mock_notifier_is_send_sync() {
    assert_send_sync::<MockNotifier>();
}

#[test]
fn test_mock_session_runner_is_send_sync() {
    assert_send_sync::<MockSessionRunner>();
}

#[test]
fn test_mock_review_runner_is_send_sync() {
    assert_send_sync::<MockReviewRunner>();
}
