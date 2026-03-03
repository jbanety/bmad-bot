//! Self-verification tests for mock implementations.

use super::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{
    Notifier, NotifierError, RunSummary, StoryNotification, StoryStatus,
};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_story() -> bmad_bot::watcher::StoryInfo {
    super::helpers::fixtures::make_test_story(
        "7-1-integration-test-infrastructure",
        "integration test infrastructure",
        vec![],
    )
}

// ---------------------------------------------------------------------------
// 7.1 — MockGitProvider returns configured values and tracks calls
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_configured_ok() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".to_string(),
        url: "https://github.com/test/pr/42".to_string(),
        number: 42,
    }));
    let params = CreatePrParams {
        title: "test pr".into(),
        body: "body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };
    let result = mock.create_pr(params).await;
    assert!(result.is_ok());
    let pr = result.unwrap();
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
    assert_eq!(mock.calls().len(), 1);
}

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "server error".into(),
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
async fn test_mock_git_provider_add_comment_tracks_call() {
    let mock = MockGitProvider::new().with_add_comment(Ok(()));
    let result = mock.add_comment("pr-1", "nice work").await;
    assert!(result.is_ok());
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::AddComment { pr_id, body } => {
            assert_eq!(pr_id, "pr-1");
            assert_eq!(body, "nice work");
        }
        _ => panic!("Expected AddComment call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured_value() {
    let mock =
        MockGitProvider::new().with_get_pr_url(Ok("https://custom-url.com/pr/99".to_string()));
    let result = mock.get_pr_url("99").await;
    assert_eq!(result.unwrap(), "https://custom-url.com/pr/99");
}

#[tokio::test]
async fn test_mock_git_provider_default_fallback_on_second_call() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "first".to_string(),
        url: "https://first".to_string(),
        number: 1,
    }));
    let params = || CreatePrParams {
        title: "t".into(),
        body: "b".into(),
        source_branch: "s".into(),
        target_branch: "m".into(),
    };
    // First call returns configured value
    let r1 = mock.create_pr(params()).await.unwrap();
    assert_eq!(r1.id, "first");
    // Second call returns default fallback
    let r2 = mock.create_pr(params()).await.unwrap();
    assert_eq!(r2.id, "mock-1");
    assert_eq!(mock.calls().len(), 2);
}

// ---------------------------------------------------------------------------
// 7.2 — MockNotifier captures notifications correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://pr".into()),
        reason: None,
    };
    let result = mock.notify_story(&notification).await;
    assert!(result.is_ok());
    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_id, "7.1");
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
    let result = mock.notify_run_summary(&summary).await;
    assert!(result.is_ok());
    let summaries = mock.summary_calls();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_returns_configured_error() {
    let mock = MockNotifier::new().with_notify_story(Err(NotifierError::Disabled));
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
async fn test_mock_notifier_calls_returns_all_call_types() {
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
        fatal: false,
    };
    mock.notify_story(&notification).await.unwrap();
    mock.notify_run_summary(&summary).await.unwrap();
    assert_eq!(mock.calls().len(), 2);
    assert_eq!(mock.story_calls().len(), 1);
    assert_eq!(mock.summary_calls().len(), 1);
}

// ---------------------------------------------------------------------------
// 7.3 — MockSessionRunner returns configured outcomes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let story = make_story();
    let mock = MockSessionRunner::new().with_run_result(SessionOutcome::Completed {
        story_key: "7-1-test".into(),
        branch: "story/7-1-test".into(),
        decisions: vec![],
        pr_context: Some("test context".into()),
        pr_how_to_test: None,
        pr_additional_info: None,
    });
    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed {
            story_key,
            pr_context,
            ..
        } => {
            assert_eq!(story_key, "7-1-test");
            assert_eq!(pr_context, Some("test context".into()));
        }
        _ => panic!("Expected Completed outcome"),
    }
    assert_eq!(mock.calls().len(), 1);
}

#[tokio::test]
async fn test_mock_session_runner_returns_failed() {
    let story = make_story();
    let mock = MockSessionRunner::new().with_run_result(SessionOutcome::Failed {
        story_key: "7-1-test".into(),
        error: "boom".into(),
        decisions: vec![],
    });
    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => assert_eq!(error, "boom"),
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_check_and_recover_wal_returns_none() {
    let mock = MockSessionRunner::new();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
    assert_eq!(mock.calls().len(), 1);
    match &mock.calls()[0] {
        SessionRunnerCall::CheckAndRecoverWal => {}
        _ => panic!("Expected CheckAndRecoverWal call"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_check_and_recover_wal_returns_info() {
    let mock = MockSessionRunner::new().with_recovery(RecoveryInfo {
        story_key: "7-1-test".into(),
        branch: "story/7-1-test".into(),
    });
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_some());
    let info = result.unwrap();
    assert_eq!(info.story_key, "7-1-test");
}

#[tokio::test]
async fn test_mock_session_runner_default_outcome_is_completed() {
    let story = make_story();
    let mock = MockSessionRunner::new();
    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "7-1-integration-test-infrastructure");
        }
        _ => panic!("Expected default Completed outcome"),
    }
}

// ---------------------------------------------------------------------------
// 7.4 — MockReviewRunner returns configured outcomes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let story = make_story();
    let mock = MockReviewRunner::new().with_run_result(ReviewOutcome::Completed {
        story_key: "7-1-test".into(),
        branch: "story/7-1-test".into(),
        report: "All good".into(),
    });
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { report, .. } => assert_eq!(report, "All good"),
        _ => panic!("Expected Completed outcome"),
    }
    assert_eq!(mock.calls().len(), 1);
}

#[tokio::test]
async fn test_mock_review_runner_returns_failed() {
    let story = make_story();
    let mock = MockReviewRunner::new().with_run_result(ReviewOutcome::Failed {
        story_key: "7-1-test".into(),
        error: "review crash".into(),
    });
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Failed { error, .. } => assert_eq!(error, "review crash"),
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_skipped() {
    let story = make_story();
    let mock = MockReviewRunner::new().with_run_result(ReviewOutcome::Skipped {
        reason: "disabled".into(),
    });
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => assert_eq!(reason, "disabled"),
        _ => panic!("Expected Skipped outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_default_outcome_is_completed() {
    let story = make_story();
    let mock = MockReviewRunner::new();
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "7-1-integration-test-infrastructure");
        }
        _ => panic!("Expected default Completed outcome"),
    }
}

// ---------------------------------------------------------------------------
// 7.9 — All mock types satisfy Send + Sync bounds
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
