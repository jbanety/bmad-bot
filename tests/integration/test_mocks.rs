//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use crate::helpers::fixtures::make_test_story;

// ---------------------------------------------------------------------------
// MockGitProvider tests (7.1 / 7.2 / 7.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_default_ok() {
    let mock = MockGitProvider::new();
    let params = CreatePrParams {
        title: "test PR".into(),
        body: "body".into(),
        source_branch: "story/1-1-test".into(),
        target_branch: "main".into(),
    };
    let result = mock.create_pr(params).await;
    assert!(result.is_ok());
    let pr = result.unwrap();
    assert_eq!(pr.id, "1");
    assert_eq!(pr.number, 1);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_values() {
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
    let pr = mock.create_pr(params).await.unwrap();
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
    assert_eq!(pr.url, "https://github.com/test/test/pull/42");
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();
    let params = CreatePrParams {
        title: "t".into(),
        body: "b".into(),
        source_branch: "s".into(),
        target_branch: "m".into(),
    };
    let _ = mock.create_pr(params).await;
    let _ = mock.add_comment("1", "comment").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);
    assert!(matches!(&calls[0], GitProviderCall::CreatePr(_)));
    assert!(matches!(
        &calls[1],
        GitProviderCall::AddComment { pr_id, body }
        if pr_id == "1" && body == "comment"
    ));
    assert!(matches!(&calls[2], GitProviderCall::GetPrUrl(id) if id == "1"));
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "internal error".into(),
    }));
    let params = CreatePrParams {
        title: "t".into(),
        body: "b".into(),
        source_branch: "s".into(),
        target_branch: "m".into(),
    };
    let result = mock.create_pr(params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_ok() {
    let mock = MockGitProvider::new();
    let result = mock.add_comment("42", "test comment").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_ok() {
    let mock = MockGitProvider::new();
    let result = mock.get_pr_url("1").await;
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// MockNotifier tests (7.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    let mock = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://example.com/pr/1".into()),
        reason: None,
    };
    mock.notify_story(&notification).await.unwrap();

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_id, "7.1");
    assert_eq!(story_calls[0].story_key, "7-1-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summaries() {
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
async fn test_mock_notifier_all_calls() {
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

// ---------------------------------------------------------------------------
// MockSessionRunner tests (7.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_default_completed() {
    let mock = MockSessionRunner::new();
    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_failed() {
    let mock = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "boom".into(),
        decisions: vec![],
    });
    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;
    assert!(
        matches!(outcome, SessionOutcome::Failed { ref error, .. } if error == "boom")
    );
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();
    let story = make_test_story("2-1-test", "test", vec![]);
    let _ = mock.run(&story).await;
    let _ = mock.check_and_recover_wal().await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert!(matches!(&calls[0], SessionRunnerCall::Run(key) if key == "2-1-test"));
    assert!(matches!(&calls[1], SessionRunnerCall::CheckAndRecoverWal));
}

#[tokio::test]
async fn test_mock_session_runner_check_and_recover_wal_returns_none() {
    let mock = MockSessionRunner::new();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests (7.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_default_skipped() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Skipped { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_completed() {
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Completed {
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        report: "all good".into(),
    });
    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;
    assert!(
        matches!(outcome, ReviewOutcome::Completed { ref report, .. } if report == "all good")
    );
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("3-1-test", "test", vec![]);
    let _ = mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert!(matches!(&calls[0], ReviewRunnerCall::Run(key) if key == "3-1-test"));
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_failed() {
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "crash".into(),
    });
    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Failed { ref error, .. } if error == "crash"));
}

// ---------------------------------------------------------------------------
// Send + Sync bounds (AC #3)
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
