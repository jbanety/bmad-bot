//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

use crate::helpers::fixtures;

// ---------------------------------------------------------------------------
// MockGitProvider tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
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

    let info = result.expect("create_pr should succeed");
    assert_eq!(info.id, "42");
    assert_eq!(info.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "internal".into(),
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "test".into(),
            body: "body".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
        })
        .await;

    assert!(result.is_err(), "create_pr should return error");
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    mock.create_pr(CreatePrParams {
        title: "PR title".into(),
        body: "PR body".into(),
        source_branch: "feat".into(),
        target_branch: "main".into(),
    })
    .await
    .unwrap();

    mock.add_comment("1", "looks good").await.unwrap();
    mock.get_pr_url("1").await.unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 3, "Should have recorded 3 calls");

    match &calls[0] {
        GitProviderCall::CreatePr(params) => {
            assert_eq!(params.title, "PR title");
        }
        _ => panic!("Expected CreatePr call"),
    }

    match &calls[1] {
        GitProviderCall::AddComment(pr_id, body) => {
            assert_eq!(pr_id, "1");
            assert_eq!(body, "looks good");
        }
        _ => panic!("Expected AddComment call"),
    }

    match &calls[2] {
        GitProviderCall::GetPrUrl(pr_id) => {
            assert_eq!(pr_id, "1");
        }
        _ => panic!("Expected GetPrUrl call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_configured() {
    let mock =
        MockGitProvider::new().with_add_comment(Err(GitProviderError::AuthenticationFailed {
            reason: "bad token".into(),
        }));

    let result = mock.add_comment("1", "test").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_configured() {
    let mock = MockGitProvider::new().with_get_pr_url(Ok("https://custom.url/pr/99".into()));

    let url = mock.get_pr_url("99").await.expect("should succeed");
    assert_eq!(url, "https://custom.url/pr/99");
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/pull/1".into()),
        reason: None,
    };

    mock.notify_story(&notification)
        .await
        .expect("notify_story should succeed");

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "7-1-test");

    let summary_calls = mock.summary_calls();
    assert!(summary_calls.is_empty());
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

    mock.notify_run_summary(&summary)
        .await
        .expect("notify_run_summary should succeed");

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_captures_multiple_calls() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-first".into(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };

    mock.notify_story(&notification).await.unwrap();
    mock.notify_story(&notification).await.unwrap();

    let summary = RunSummary {
        stories: vec![],
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
        fatal: false,
    };
    mock.notify_run_summary(&summary).await.unwrap();

    assert_eq!(mock.calls().len(), 3);
    assert_eq!(mock.story_calls().len(), 2);
    assert_eq!(mock.summary_calls().len(), 1);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed_by_default() {
    let mock = MockSessionRunner::new();
    let story = fixtures::make_test_story("7-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "7-1-test");
        }
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_outcome() {
    let mock = MockSessionRunner::new().with_outcome(|story| SessionOutcome::Failed {
        story_key: story.story_key.clone(),
        error: "test failure".into(),
        decisions: vec![],
    });

    let story = fixtures::make_test_story("2-1-watcher", "watcher", vec![]);
    let outcome = mock.run(&story).await;

    match outcome {
        SessionOutcome::Failed {
            story_key, error, ..
        } => {
            assert_eq!(story_key, "2-1-watcher");
            assert_eq!(error, "test failure");
        }
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();
    let s1 = fixtures::make_test_story("1-1-first", "first", vec![]);
    let s2 = fixtures::make_test_story("1-2-second", "second", vec!["1-1-first"]);

    mock.run(&s1).await;
    mock.run(&s2).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-first");
    assert_eq!(calls[1].story_key, "1-2-second");
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
async fn test_mock_review_runner_returns_skipped_by_default() {
    let mock = MockReviewRunner::new();
    let story = fixtures::make_test_story("7-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => {
            assert_eq!(reason, "mock review skipped");
        }
        _ => panic!("Expected Skipped outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_outcome() {
    let mock = MockReviewRunner::new().with_outcome(|story| ReviewOutcome::Completed {
        story_key: story.story_key.clone(),
        branch: story.branch_name.clone(),
        report: "LGTM".into(),
    });

    let story = fixtures::make_test_story("3-1-supervisor", "supervisor", vec![]);
    let outcome = mock.run(&story).await;

    match outcome {
        ReviewOutcome::Completed {
            story_key, report, ..
        } => {
            assert_eq!(story_key, "3-1-supervisor");
            assert_eq!(report, "LGTM");
        }
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let s1 = fixtures::make_test_story("1-1-first", "first", vec![]);

    mock.run(&s1).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-first");
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
