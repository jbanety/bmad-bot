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
async fn test_mock_git_provider_returns_configured_create_pr() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/pr/42".into(),
        number: 42,
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "Test PR".into(),
            body: "body".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
        })
        .await;

    let pr = result.expect("should succeed");
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::AuthenticationFailed {
        reason: "bad token".into(),
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "t".into(),
            body: "b".into(),
            source_branch: "s".into(),
            target_branch: "m".into(),
        })
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(format!("{err}").contains("bad token"));
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    mock.create_pr(CreatePrParams {
        title: "PR 1".into(),
        body: "b".into(),
        source_branch: "s".into(),
        target_branch: "m".into(),
    })
    .await
    .unwrap();

    mock.add_comment("1", "comment body").await.unwrap();
    mock.get_pr_url("1").await.unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);

    assert!(matches!(&calls[0], GitProviderCall::CreatePr(p) if p.title == "PR 1"));
    assert!(
        matches!(&calls[1], GitProviderCall::AddComment { pr_id, body } if pr_id == "1" && body == "comment body")
    );
    assert!(matches!(&calls[2], GitProviderCall::GetPrUrl { pr_id } if pr_id == "1"));
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_returns_configured_value() {
    let mock = MockGitProvider::new().with_add_comment(Ok(()));
    let result = mock.add_comment("1", "hello").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured_value() {
    let mock =
        MockGitProvider::new().with_get_pr_url(Ok("https://custom.example.com/pr/5".into()));
    let url = mock.get_pr_url("5").await.expect("should succeed");
    assert_eq!(url, "https://custom.example.com/pr/5");
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://example.com/pr/1".into()),
        reason: None,
    };

    mock.notify_story(&notification).await.unwrap();

    let calls = mock.story_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-test");
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

    mock.notify_run_summary(&summary).await.unwrap();

    let summaries = mock.summary_calls();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_calls_includes_both_types() {
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

    assert_eq!(mock.calls().len(), 2);
    assert_eq!(mock.story_calls().len(), 1);
    assert_eq!(mock.summary_calls().len(), 1);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_configured_completed() {
    let story = make_test_story("1-1-test", "test", vec![]);
    let mock = MockSessionRunner::new();

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_failed() {
    let story = make_test_story("1-1-test", "test", vec![]);
    let mock = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "mock error".into(),
        decisions: vec![],
    });

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Failed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let story = make_test_story("2-1-test", "test", vec![]);
    let mock = MockSessionRunner::new();

    mock.run(&story).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], "2-1-test");
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_returns_none() {
    let mock = MockSessionRunner::new();
    assert!(mock.check_and_recover_wal().await.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_configured_completed() {
    let story = make_test_story("1-1-test", "test", vec![]);
    let mock = MockReviewRunner::new();

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_skipped() {
    let story = make_test_story("1-1-test", "test", vec![]);
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Skipped {
        reason: "disabled".into(),
    });

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Skipped { reason } if reason == "disabled"));
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_failed() {
    let story = make_test_story("1-1-test", "test", vec![]);
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "crash".into(),
    });

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Failed { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let story = make_test_story("3-1-test", "test", vec![]);
    let mock = MockReviewRunner::new();

    mock.run(&story).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], "3-1-test");
}

// ---------------------------------------------------------------------------
// Send + Sync bound tests
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
