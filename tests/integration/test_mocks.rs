//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, PrInfo};
use bmad_bot::notifier::Notifier;
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

use crate::helpers::fixtures::make_test_story;

// ---------------------------------------------------------------------------
// MockGitProvider tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
    let pr = PrInfo {
        id: "42".into(),
        url: "https://github.com/org/repo/pull/42".into(),
        number: 42,
    };
    let mock = MockGitProvider::new().with_create_pr(Ok(pr.clone()));

    let params = CreatePrParams {
        title: "test PR".into(),
        body: "test body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };
    let result = mock.create_pr(params).await.expect("should succeed");
    assert_eq!(result.id, "42");
    assert_eq!(result.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err("forced failure".into()));

    let params = CreatePrParams {
        title: "test".into(),
        body: "test".into(),
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
        title: "PR title".into(),
        body: "PR body".into(),
        source_branch: "feat".into(),
        target_branch: "main".into(),
    };
    let _ = mock.create_pr(params).await;
    let _ = mock.add_comment("1", "comment body").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);
    assert!(matches!(calls[0], GitProviderCall::CreatePr(_)));
    assert!(matches!(calls[1], GitProviderCall::AddComment { .. }));
    assert!(matches!(calls[2], GitProviderCall::GetPrUrl { .. }));
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
    assert!(result.expect("url").contains("github.com"));
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    use bmad_bot::notifier::{StoryNotification, StoryStatus};

    let mock = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-infra".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/pr/1".into()),
        reason: None,
    };

    mock.notify_story(&notification).await.expect("should ok");

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "7-1-infra");
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

    mock.notify_run_summary(&summary)
        .await
        .expect("should ok");

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_tracks_all_calls() {
    use bmad_bot::notifier::{RunSummary, StoryNotification, StoryStatus};

    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };
    let summary = RunSummary {
        stories: Vec::new(),
        total_processed: 1,
        completed: 1,
        blocked: 0,
        errored: 0,
        fatal: false,
    };

    mock.notify_story(&notification).await.expect("ok");
    mock.notify_run_summary(&summary).await.expect("ok");

    assert_eq!(mock.calls().len(), 2);
    assert_eq!(mock.story_calls().len(), 1);
    assert_eq!(mock.summary_calls().len(), 1);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let outcome = SessionOutcome::Completed {
        story_key: "7-1-infra".into(),
        branch: "story/7-1-infra".into(),
        decisions: Vec::new(),
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    };
    let runner = MockSessionRunner::new(outcome);
    let story = make_test_story("7-1-infra", "Integration Test Infra", vec![]);

    let result = runner.run(&story).await;
    assert!(matches!(result, SessionOutcome::Completed { .. }));

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], "7-1-infra");
}

#[tokio::test]
async fn test_mock_session_runner_returns_failed() {
    let outcome = SessionOutcome::Failed {
        story_key: "7-1-infra".into(),
        error: "test error".into(),
        decisions: Vec::new(),
    };
    let runner = MockSessionRunner::new(outcome);
    let story = make_test_story("7-1-infra", "Test", vec![]);

    let result = runner.run(&story).await;
    assert!(matches!(result, SessionOutcome::Failed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_returns_none() {
    let outcome = SessionOutcome::Completed {
        story_key: "7-1-infra".into(),
        branch: "story/7-1-infra".into(),
        decisions: Vec::new(),
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    };
    let runner = MockSessionRunner::new(outcome);
    assert!(runner.check_and_recover_wal().await.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let outcome = ReviewOutcome::Completed {
        story_key: "7-1-infra".into(),
        branch: "story/7-1-infra".into(),
        report: "LGTM".into(),
    };
    let runner = MockReviewRunner::new(outcome);
    let story = make_test_story("7-1-infra", "Test", vec![]);

    let result = runner.run(&story).await;
    assert!(matches!(result, ReviewOutcome::Completed { .. }));

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], "7-1-infra");
}

#[tokio::test]
async fn test_mock_review_runner_returns_skipped() {
    let outcome = ReviewOutcome::Skipped {
        reason: "review disabled".into(),
    };
    let runner = MockReviewRunner::new(outcome);
    let story = make_test_story("7-1-infra", "Test", vec![]);

    let result = runner.run(&story).await;
    assert!(matches!(result, ReviewOutcome::Skipped { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_failed() {
    let outcome = ReviewOutcome::Failed {
        story_key: "7-1-infra".into(),
        error: "crash".into(),
    };
    let runner = MockReviewRunner::new(outcome);
    let story = make_test_story("7-1-infra", "Test", vec![]);

    let result = runner.run(&story).await;
    assert!(matches!(result, ReviewOutcome::Failed { .. }));
}

// ---------------------------------------------------------------------------
// Send + Sync verification tests
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
