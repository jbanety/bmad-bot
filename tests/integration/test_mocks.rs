//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

use crate::helpers::fixtures::make_test_story;

// ---------------------------------------------------------------------------
// MockGitProvider tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_configured_value() {
    let expected = PrInfo {
        id: "42".to_string(),
        url: "https://github.com/test/test/pull/42".to_string(),
        number: 42,
    };
    let mock = MockGitProvider::new().with_create_pr(Ok(expected));

    let params = CreatePrParams {
        title: "Test PR".to_string(),
        body: "Test body".to_string(),
        source_branch: "story/1-1-test".to_string(),
        target_branch: "main".to_string(),
    };
    let result = mock.create_pr(params).await.expect("should succeed");
    assert_eq!(result.id, "42");
    assert_eq!(result.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::AuthenticationFailed {
        reason: "bad token".into(),
    }));

    let params = CreatePrParams {
        title: "Test".to_string(),
        body: "".to_string(),
        source_branch: "story/1-1-test".to_string(),
        target_branch: "main".to_string(),
    };
    let result = mock.create_pr(params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new()
        .with_create_pr(Ok(PrInfo {
            id: "1".into(),
            url: "https://example.com".into(),
            number: 1,
        }))
        .with_add_comment(Ok(()))
        .with_get_pr_url(Ok("https://example.com/pr/1".into()));

    let params = CreatePrParams {
        title: "Test".into(),
        body: "Body".into(),
        source_branch: "story/1-1-test".into(),
        target_branch: "main".into(),
    };
    let _ = mock.create_pr(params).await;
    let _ = mock.add_comment("1", "looks good").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);
    assert!(matches!(calls[0], GitProviderCall::CreatePr(_)));
    assert!(matches!(
        calls[1],
        GitProviderCall::AddComment { ref pr_id, .. } if pr_id == "1"
    ));
    assert!(matches!(
        calls[2],
        GitProviderCall::GetPrUrl { ref pr_id } if pr_id == "1"
    ));
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_returns_configured() {
    let mock = MockGitProvider::new().with_add_comment(Ok(()));
    let result = mock.add_comment("1", "test comment").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured() {
    let mock =
        MockGitProvider::new().with_get_pr_url(Ok("https://github.com/test/test/pull/1".into()));
    let result = mock.get_pr_url("1").await.expect("should succeed");
    assert_eq!(result, "https://github.com/test/test/pull/1");
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "1.1".to_string(),
        story_key: "1-1-test".to_string(),
        status: StoryStatus::Completed,
        pr_url: Some("https://example.com/pr/1".into()),
        reason: None,
    };

    mock.notify_story(&notification).await.expect("should succeed");

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "1-1-test");
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

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 0);
}

#[tokio::test]
async fn test_mock_notifier_calls_returns_all() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".to_string(),
        story_key: "1-1-test".to_string(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };
    mock.notify_story(&notification).await.expect("should succeed");

    let summary = RunSummary {
        stories: vec![],
        total_processed: 1,
        completed: 1,
        blocked: 0,
        errored: 0,
        fatal: false,
    };
    mock.notify_run_summary(&summary).await.expect("should succeed");

    let all_calls = mock.calls();
    assert_eq!(all_calls.len(), 2);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let outcome = SessionOutcome::Completed {
        story_key: "1-1-test".to_string(),
        branch: "story/1-1-test".to_string(),
        decisions: vec![],
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    };
    let mock = MockSessionRunner::new(outcome);
    let story = make_test_story("1-1-test", "test", vec![]);

    let result = mock.run(&story).await;
    assert!(matches!(result, SessionOutcome::Completed { .. }));

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_session_runner_returns_failed() {
    let outcome = SessionOutcome::Failed {
        story_key: "1-1-test".to_string(),
        error: "something went wrong".to_string(),
        decisions: vec![],
    };
    let mock = MockSessionRunner::new(outcome);
    let story = make_test_story("1-1-test", "test", vec![]);

    let result = mock.run(&story).await;
    assert!(matches!(result, SessionOutcome::Failed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_check_and_recover_wal_returns_none() {
    let outcome = SessionOutcome::Completed {
        story_key: "1-1-test".to_string(),
        branch: "story/1-1-test".to_string(),
        decisions: vec![],
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    };
    let mock = MockSessionRunner::new(outcome);
    assert!(mock.check_and_recover_wal().await.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let outcome = ReviewOutcome::Completed {
        story_key: "1-1-test".to_string(),
        branch: "story/1-1-test".to_string(),
        report: "LGTM".to_string(),
    };
    let mock = MockReviewRunner::new(outcome);
    let story = make_test_story("1-1-test", "test", vec![]);

    let result = mock.run(&story).await;
    assert!(matches!(result, ReviewOutcome::Completed { .. }));

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_review_runner_returns_failed() {
    let outcome = ReviewOutcome::Failed {
        story_key: "1-1-test".to_string(),
        error: "review crashed".to_string(),
    };
    let mock = MockReviewRunner::new(outcome);
    let story = make_test_story("1-1-test", "test", vec![]);

    let result = mock.run(&story).await;
    assert!(matches!(result, ReviewOutcome::Failed { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_skipped() {
    let outcome = ReviewOutcome::Skipped {
        reason: "provider down".to_string(),
    };
    let mock = MockReviewRunner::new(outcome);
    let story = make_test_story("1-1-test", "test", vec![]);

    let result = mock.run(&story).await;
    assert!(matches!(result, ReviewOutcome::Skipped { .. }));
}

// ---------------------------------------------------------------------------
// Send + Sync bound verification
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
