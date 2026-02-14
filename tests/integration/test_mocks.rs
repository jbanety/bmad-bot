//! Self-verification tests for mock implementations.

use crate::helpers::fixtures;
use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{
    Notifier, NotifierError, RunSummary, StoryNotification, StoryStatus,
};
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

    let result = mock
        .create_pr(CreatePrParams {
            title: "Test PR".into(),
            body: "Test body".into(),
            source_branch: "story/1-1-test".into(),
            target_branch: "main".into(),
        })
        .await;

    let pr = result.expect("should return Ok");
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::NetworkError {
        reason: "connection refused".into(),
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "Test PR".into(),
            body: "".into(),
            source_branch: "story/1-1-test".into(),
            target_branch: "main".into(),
        })
        .await;

    assert!(result.is_err());
    let err_str = format!("{}", result.unwrap_err());
    assert!(err_str.contains("connection refused"));
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    let _ = mock
        .create_pr(CreatePrParams {
            title: "PR 1".into(),
            body: "".into(),
            source_branch: "story/1-1-a".into(),
            target_branch: "main".into(),
        })
        .await;
    let _ = mock.add_comment("1", "LGTM").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);

    // Verify call types
    assert!(matches!(calls[0], GitProviderCall::CreatePr(_)));
    assert!(matches!(calls[1], GitProviderCall::AddComment { .. }));
    assert!(matches!(calls[2], GitProviderCall::GetPrUrl { .. }));
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_tracks_args() {
    let mock = MockGitProvider::new();
    let _ = mock.add_comment("99", "Great work!").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    if let GitProviderCall::AddComment { pr_id, body } = &calls[0] {
        assert_eq!(pr_id, "99");
        assert_eq!(body, "Great work!");
    } else {
        panic!("Expected AddComment call");
    }
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured() {
    let mock = MockGitProvider::new()
        .with_get_pr_url(Ok("https://custom-url.com/pr/5".into()));

    let url = mock.get_pr_url("5").await.expect("should return Ok");
    assert_eq!(url, "https://custom-url.com/pr/5");
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
        pr_url: Some("https://github.com/test/repo/pull/1".into()),
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

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_returns_configured_error() {
    let mock = MockNotifier::new().with_story_error(NotifierError::HttpRequest {
        reason: "timeout".into(),
    });

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Error,
        pr_url: None,
        reason: Some("timeout".into()),
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

    let _ = mock.notify_story(&notification).await;
    let _ = mock.notify_run_summary(&summary).await;

    let all_calls = mock.calls();
    assert_eq!(all_calls.len(), 2);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let mock = MockSessionRunner::new();
    let story = fixtures::make_test_story("1-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_outcome() {
    let mock = MockSessionRunner::new().with_outcome(|story| SessionOutcome::Failed {
        story_key: story.story_key.clone(),
        error: "test failure".into(),
        decisions: Vec::new(),
    });
    let story = fixtures::make_test_story("2-1-failing", "failing", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Failed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();
    let story1 = fixtures::make_test_story("1-1-first", "first", vec![]);
    let story2 = fixtures::make_test_story("1-2-second", "second", vec![]);

    let _ = mock.run(&story1).await;
    let _ = mock.run(&story2).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-first");
    assert_eq!(calls[1].story_key, "1-2-second");
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_none() {
    let mock = MockSessionRunner::new();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
    assert_eq!(mock.wal_check_count(), 1);
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_some() {
    let mock = MockSessionRunner::new().with_wal_recovery(RecoveryInfo {
        story_key: "1-1-recovered".into(),
        branch: "story/1-1-recovered".into(),
    });

    let result = mock.check_and_recover_wal().await;
    assert!(result.is_some());
    let info = result.unwrap();
    assert_eq!(info.story_key, "1-1-recovered");
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let mock = MockReviewRunner::new();
    let story = fixtures::make_test_story("1-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_outcome() {
    let mock = MockReviewRunner::new().with_outcome(|_story| ReviewOutcome::Skipped {
        reason: "test skip".into(),
    });
    let story = fixtures::make_test_story("1-1-skipped", "skipped", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Skipped { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let story = fixtures::make_test_story("3-1-review", "review", vec![]);

    let _ = mock.run(&story).await;
    let _ = mock.run(&story).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "3-1-review");
}

// ---------------------------------------------------------------------------
// Send + Sync bounds tests
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
