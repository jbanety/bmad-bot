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
        url: "https://github.com/owner/repo/pull/42".into(),
        number: 42,
    }));

    let params = CreatePrParams {
        title: "test PR".into(),
        body: "test body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await.unwrap();
    assert_eq!(result.id, "42");
    assert_eq!(result.number, 42);
    assert_eq!(result.url, "https://github.com/owner/repo/pull/42");
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "Internal error".into(),
    }));

    let params = CreatePrParams {
        title: "test".into(),
        body: "test".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(format!("{err}").contains("500"));
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    let params = CreatePrParams {
        title: "test PR".into(),
        body: "test body".into(),
        source_branch: "feature".into(),
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
async fn test_mock_git_provider_add_comment_returns_configured() {
    let mock = MockGitProvider::new().with_add_comment(Err(GitProviderError::AuthenticationFailed {
        reason: "bad token".into(),
    }));

    let result = mock.add_comment("1", "body").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured() {
    let mock =
        MockGitProvider::new().with_get_pr_url(Ok("https://custom.url/pr/99".to_string()));

    let result = mock.get_pr_url("99").await.unwrap();
    assert_eq!(result, "https://custom.url/pr/99");
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/pull/1".into()),
        reason: None,
    };

    mock.notify_story(&notification).await.unwrap();

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "7-1-test");
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

    mock.notify_run_summary(&summary).await.unwrap();

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_captures_multiple_calls() {
    let mock = MockNotifier::new();

    let n1 = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-a".into(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };
    let n2 = StoryNotification {
        story_id: "1.2".into(),
        story_key: "1-2-b".into(),
        status: StoryStatus::Blocked,
        pr_url: None,
        reason: Some("blocked by dep".into()),
    };

    mock.notify_story(&n1).await.unwrap();
    mock.notify_story(&n2).await.unwrap();

    let all_calls = mock.calls();
    assert_eq!(all_calls.len(), 2);

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 2);
    assert_eq!(story_calls[0].story_key, "1-1-a");
    assert_eq!(story_calls[1].story_key, "1-2-b");
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let mock = MockSessionRunner::new_completed();
    let story = make_test_story("7-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Completed { .. }));

    if let SessionOutcome::Completed { story_key, .. } = outcome {
        assert_eq!(story_key, "7-1-test");
    }
}

#[tokio::test]
async fn test_mock_session_runner_with_custom_outcome() {
    let mock = MockSessionRunner::with_outcome(|story| SessionOutcome::Failed {
        story_key: story.story_key.clone(),
        error: "test failure".into(),
        decisions: Vec::new(),
    });

    let story = make_test_story("2-1-watcher", "watcher", vec![]);
    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Failed { .. }));

    if let SessionOutcome::Failed { error, .. } = outcome {
        assert_eq!(error, "test failure");
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new_completed();
    let story1 = make_test_story("1-1-a", "a", vec![]);
    let story2 = make_test_story("1-2-b", "b", vec![]);

    let _ = mock.run(&story1).await;
    let _ = mock.run(&story2).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-a");
    assert_eq!(calls[1].story_key, "1-2-b");
}

#[tokio::test]
async fn test_mock_session_runner_wal_check_returns_none() {
    let mock = MockSessionRunner::new_completed();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
    assert_eq!(mock.wal_check_count(), 1);
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let mock = MockReviewRunner::new_completed();
    let story = make_test_story("5-2-review", "review", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Completed { .. }));

    if let ReviewOutcome::Completed { story_key, report, .. } = outcome {
        assert_eq!(story_key, "5-2-review");
        assert_eq!(report, "Mock review report");
    }
}

#[tokio::test]
async fn test_mock_review_runner_with_custom_outcome() {
    let mock = MockReviewRunner::with_outcome(|_| ReviewOutcome::Skipped {
        reason: "review disabled".into(),
    });

    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Skipped { .. }));

    if let ReviewOutcome::Skipped { reason } = outcome {
        assert_eq!(reason, "review disabled");
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new_completed();
    let story = make_test_story("3-1-supervisor", "supervisor", vec![]);

    let _ = mock.run(&story).await;
    let _ = mock.run(&story).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
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
