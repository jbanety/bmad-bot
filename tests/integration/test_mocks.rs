//! Self-verification tests for mock implementations.
//!
//! Ensures mocks return configured values, track calls correctly,
//! and satisfy `Send + Sync` bounds.

use crate::helpers::fixtures;
use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

// ---------------------------------------------------------------------------
// MockGitProvider tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/repo/pull/42".into(),
        number: 42,
    }));

    let params = CreatePrParams {
        title: "test PR".into(),
        body: "body".into(),
        source_branch: "story/1-1-test".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await;
    assert!(result.is_ok());
    let info = result.unwrap();
    assert_eq!(info.id, "42");
    assert_eq!(info.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "internal error".into(),
    }));

    let params = CreatePrParams {
        title: "test".into(),
        body: "body".into(),
        source_branch: "story/1-1-test".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    let params = CreatePrParams {
        title: "test".into(),
        body: "body".into(),
        source_branch: "story/1-1-test".into(),
        target_branch: "main".into(),
    };

    let _ = mock.create_pr(params).await;
    let _ = mock.add_comment("1", "LGTM").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);

    assert!(matches!(&calls[0], GitProviderCall::CreatePr(_)));
    assert!(matches!(
        &calls[1],
        GitProviderCall::AddComment { pr_id, body } if pr_id == "1" && body == "LGTM"
    ));
    assert!(matches!(
        &calls[2],
        GitProviderCall::GetPrUrl(id) if id == "1"
    ));
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_returns_configured() {
    let mock = MockGitProvider::new().with_add_comment(Ok(()));
    let result = mock.add_comment("1", "test").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured() {
    let mock =
        MockGitProvider::new().with_get_pr_url(Ok("https://github.com/org/repo/pull/5".into()));
    let result = mock.get_pr_url("5").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "https://github.com/org/repo/pull/5");
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
        pr_url: Some("https://github.com/test/pull/1".into()),
        reason: None,
    };

    let result = mock.notify_story(&notification).await;
    assert!(result.is_ok());

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summary() {
    use bmad_bot::notifier::RunSummary;

    let mock = MockNotifier::new();

    let summary = RunSummary {
        stories: Vec::new(),
        total_processed: 3,
        completed: 2,
        blocked: 1,
        errored: 0,
        fatal: false,
    };

    let result = mock.notify_run_summary(&summary).await;
    assert!(result.is_ok());

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_returns_configured_error() {
    let mock = MockNotifier::new().with_story_error(NotifierError::Disabled);

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
async fn test_mock_notifier_all_calls_accessor() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };

    let _ = mock.notify_story(&notification).await;

    let all = mock.calls();
    assert_eq!(all.len(), 1);
    assert!(matches!(&all[0], NotifierCall::Story(_)));
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let mock = MockSessionRunner::new();
    let story = fixtures::make_test_story("1-1-test", "test story", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_failed() {
    let mock = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "boom".into(),
        decisions: Vec::new(),
    });

    let story = fixtures::make_test_story("1-1-test", "test story", vec![]);
    let outcome = mock.run(&story).await;

    assert!(matches!(
        outcome,
        SessionOutcome::Failed { ref error, .. } if error == "boom"
    ));
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();

    let story1 = fixtures::make_test_story("1-1-test", "test", vec![]);
    let story2 = fixtures::make_test_story("2-1-other", "other", vec![]);

    let _ = mock.run(&story1).await;
    let _ = mock.run(&story2).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-test");
    assert_eq!(calls[1].story_key, "2-1-other");
}

#[tokio::test]
async fn test_mock_session_runner_check_and_recover_wal_returns_none() {
    let mock = MockSessionRunner::new();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let mock = MockReviewRunner::new();
    let story = fixtures::make_test_story("1-1-test", "test story", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_skipped() {
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Skipped {
        reason: "disabled".into(),
    });

    let story = fixtures::make_test_story("1-1-test", "test story", vec![]);
    let outcome = mock.run(&story).await;

    assert!(matches!(
        outcome,
        ReviewOutcome::Skipped { ref reason } if reason == "disabled"
    ));
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();

    let story = fixtures::make_test_story("3-1-review", "review", vec![]);
    let _ = mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "3-1-review");
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
