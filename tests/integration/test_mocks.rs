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
    let provider = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/test/pull/42".into(),
        number: 42,
    }));

    let params = CreatePrParams {
        title: "Test PR".into(),
        body: "Body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let result = provider.create_pr(params).await;
    assert!(result.is_ok());
    let pr = result.unwrap();
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_error() {
    let provider = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "test error".into(),
    }));

    let params = CreatePrParams {
        title: "Test PR".into(),
        body: "Body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let result = provider.create_pr(params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_create_pr_calls() {
    let provider = MockGitProvider::new();

    let params = CreatePrParams {
        title: "Test PR".into(),
        body: "Body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let _ = provider.create_pr(params).await;

    let calls = provider.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::CreatePr(p) => assert_eq!(p.title, "Test PR"),
        _ => panic!("Expected CreatePr call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_tracks_calls() {
    let provider = MockGitProvider::new();
    let _ = provider.add_comment("1", "Nice work!").await;

    let calls = provider.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::AddComment(id, body) => {
            assert_eq!(id, "1");
            assert_eq!(body, "Nice work!");
        }
        _ => panic!("Expected AddComment call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_tracks_calls() {
    let provider = MockGitProvider::new();
    let result = provider.get_pr_url("5").await;
    assert!(result.is_ok());

    let calls = provider.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::GetPrUrl(id) => assert_eq!(id, "5"),
        _ => panic!("Expected GetPrUrl call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_multiple_calls_tracked() {
    let provider = MockGitProvider::new();

    let params = CreatePrParams {
        title: "PR".into(),
        body: "B".into(),
        source_branch: "f".into(),
        target_branch: "m".into(),
    };
    let _ = provider.create_pr(params).await;
    let _ = provider.add_comment("1", "comment").await;
    let _ = provider.get_pr_url("1").await;

    assert_eq!(provider.calls().len(), 3);
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let notifier = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/pull/1".into()),
        reason: None,
    };

    let result = notifier.notify_story(&notification).await;
    assert!(result.is_ok());

    let story_calls = notifier.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summary() {
    let notifier = MockNotifier::new();
    let summary = RunSummary {
        stories: vec![],
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
    };

    let result = notifier.notify_run_summary(&summary).await;
    assert!(result.is_ok());

    let summary_calls = notifier.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 0);
}

#[tokio::test]
async fn test_mock_notifier_mixed_calls() {
    let notifier = MockNotifier::new();
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
    };

    let _ = notifier.notify_story(&notification).await;
    let _ = notifier.notify_run_summary(&summary).await;
    let _ = notifier.notify_story(&notification).await;

    assert_eq!(notifier.calls().len(), 3);
    assert_eq!(notifier.story_calls().len(), 2);
    assert_eq!(notifier.summary_calls().len(), 1);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_configured_completed() {
    let story = make_test_story("1-1-test", "", vec![]);
    let runner = MockSessionRunner::new();

    let outcome = runner.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => assert_eq!(story_key, "test"),
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_failed() {
    let story = make_test_story("1-1-test", "", vec![]);
    let runner = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "boom".into(),
        decisions: vec![],
    });

    let outcome = runner.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => assert_eq!(error, "boom"),
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let story = make_test_story("2-1-watcher", "", vec![]);
    let runner = MockSessionRunner::new();

    let _ = runner.run(&story).await;

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "2-1-watcher");
}

#[tokio::test]
async fn test_mock_session_runner_check_and_recover_wal_returns_none() {
    let runner = MockSessionRunner::new();
    assert!(runner.check_and_recover_wal().await.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_configured_completed() {
    let story = make_test_story("1-1-test", "", vec![]);
    let runner = MockReviewRunner::new();

    let outcome = runner.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { story_key, .. } => assert_eq!(story_key, "test"),
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_failed() {
    let story = make_test_story("1-1-test", "", vec![]);
    let runner = MockReviewRunner::new().with_outcome(ReviewOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "review boom".into(),
    });

    let outcome = runner.run(&story).await;
    match outcome {
        ReviewOutcome::Failed { error, .. } => assert_eq!(error, "review boom"),
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_skipped() {
    let story = make_test_story("1-1-test", "", vec![]);
    let runner = MockReviewRunner::new().with_outcome(ReviewOutcome::Skipped {
        reason: "disabled".into(),
    });

    let outcome = runner.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => assert_eq!(reason, "disabled"),
        _ => panic!("Expected Skipped outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let story = make_test_story("3-1-supervisor", "", vec![]);
    let runner = MockReviewRunner::new();

    let _ = runner.run(&story).await;

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "3-1-supervisor");
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
