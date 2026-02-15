//! Self-verification tests for mock implementations.

use crate::helpers::fixtures::make_test_story;
use crate::helpers::mocks::*;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

// ---------------------------------------------------------------------------
// MockGitProvider tests (Task 7.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_configured_ok() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/test/pull/42".into(),
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
    let pr = result.expect("should return Ok");
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::NetworkError {
        reason: "timeout".into(),
    }));
    let result = mock
        .create_pr(CreatePrParams {
            title: "t".into(),
            body: "b".into(),
            source_branch: "f".into(),
            target_branch: "m".into(),
        })
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_returns_configured_ok() {
    let mock = MockGitProvider::new();
    let result = mock.add_comment("1", "nice work").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_returns_configured_error() {
    let mock =
        MockGitProvider::new().with_add_comment(Err(GitProviderError::AuthenticationFailed {
            reason: "bad token".into(),
        }));
    let result = mock.add_comment("1", "comment").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured_ok() {
    let mock = MockGitProvider::new()
        .with_get_pr_url(Ok("https://example.com/pr/99".into()));
    let result = mock.get_pr_url("99").await;
    assert_eq!(result.unwrap(), "https://example.com/pr/99");
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();
    mock.create_pr(CreatePrParams {
        title: "pr1".into(),
        body: "b".into(),
        source_branch: "f".into(),
        target_branch: "m".into(),
    })
    .await
    .unwrap();
    mock.add_comment("1", "lgtm").await.unwrap();
    mock.get_pr_url("1").await.unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);
    match &calls[0] {
        GitProviderCall::CreatePr(params) => assert_eq!(params.title, "pr1"),
        _ => panic!("expected CreatePr"),
    }
    match &calls[1] {
        GitProviderCall::AddComment { pr_id, body } => {
            assert_eq!(pr_id, "1");
            assert_eq!(body, "lgtm");
        }
        _ => panic!("expected AddComment"),
    }
    match &calls[2] {
        GitProviderCall::GetPrUrl { pr_id } => assert_eq!(pr_id, "1"),
        _ => panic!("expected GetPrUrl"),
    }
}

// ---------------------------------------------------------------------------
// MockNotifier tests (Task 7.2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-integration-tests".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://example.com/pr/1".into()),
        reason: None,
    };
    mock.notify_story(&notification).await.unwrap();

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "7-1-integration-tests");
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
    mock.notify_run_summary(&summary).await.unwrap();

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 0);
}

#[tokio::test]
async fn test_mock_notifier_all_calls_mixed() {
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
    mock.notify_story(&notification).await.unwrap();
    mock.notify_run_summary(&summary).await.unwrap();

    let all_calls = mock.calls();
    assert_eq!(all_calls.len(), 2);
}

#[tokio::test]
async fn test_mock_notifier_with_story_error() {
    let mock = MockNotifier::new().with_story_error(NotifierError::Disabled);
    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Error,
        pr_url: None,
        reason: Some("test".into()),
    };
    let result = mock.notify_story(&notification).await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests (Task 7.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_default_returns_completed() {
    let mock = MockSessionRunner::new();
    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => assert_eq!(story_key, "1-1-test"),
        _ => panic!("expected Completed"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_outcome() {
    let mock = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "1-1-fail".into(),
        error: "boom".into(),
        decisions: vec![],
    });
    let story = make_test_story("1-1-fail", "fail", vec![]);
    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => assert_eq!(error, "boom"),
        _ => panic!("expected Failed"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();
    let story = make_test_story("2-1-test", "test", vec![]);
    mock.run(&story).await;
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "2-1-test");
}

#[tokio::test]
async fn test_mock_session_runner_check_and_recover_wal_returns_none() {
    let mock = MockSessionRunner::new();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests (Task 7.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_default_returns_skipped() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => assert_eq!(reason, "mock default"),
        _ => panic!("expected Skipped"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_outcome() {
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Completed {
        story_key: "1-1-review".into(),
        branch: "story/1-1-review".into(),
        report: "LGTM".into(),
    });
    let story = make_test_story("1-1-review", "review", vec![]);
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { report, .. } => assert_eq!(report, "LGTM"),
        _ => panic!("expected Completed"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("3-1-test", "test", vec![]);
    mock.run(&story).await;
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "3-1-test");
}

// ---------------------------------------------------------------------------
// Send + Sync tests (Task 7.9)
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
