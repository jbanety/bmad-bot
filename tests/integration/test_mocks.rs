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
        url: "https://github.com/test/repo/pull/42".into(),
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

    let pr = result.expect("should return Ok");
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "test error".into(),
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "test".into(),
            body: "body".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
        })
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_create_pr_calls() {
    let mock = MockGitProvider::new();

    mock.create_pr(CreatePrParams {
        title: "PR1".into(),
        body: "body1".into(),
        source_branch: "branch1".into(),
        target_branch: "main".into(),
    })
    .await
    .expect("should succeed");

    mock.create_pr(CreatePrParams {
        title: "PR2".into(),
        body: "body2".into(),
        source_branch: "branch2".into(),
        target_branch: "main".into(),
    })
    .await
    .expect("should succeed");

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    match &calls[0] {
        GitProviderCall::CreatePr(params) => assert_eq!(params.title, "PR1"),
        other => panic!("expected CreatePr, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_tracks_add_comment_calls() {
    let mock = MockGitProvider::new();
    mock.add_comment("1", "test comment")
        .await
        .expect("should succeed");

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::AddComment { pr_id, body } => {
            assert_eq!(pr_id, "1");
            assert_eq!(body, "test comment");
        }
        other => panic!("expected AddComment, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_tracks_get_pr_url_calls() {
    let mock = MockGitProvider::new();
    let url = mock.get_pr_url("5").await.expect("should succeed");
    assert!(!url.is_empty());

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::GetPrUrl(id) => assert_eq!(id, "5"),
        other => panic!("expected GetPrUrl, got {other:?}"),
    }
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

    mock.notify_story(&notification)
        .await
        .expect("should succeed");

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summaries() {
    let mock = MockNotifier::new();
    let summary = RunSummary {
        stories: vec![],
        total_processed: 1,
        completed: 1,
        blocked: 0,
        errored: 0,
        fatal: false,
    };

    mock.notify_run_summary(&summary)
        .await
        .expect("should succeed");

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 1);
}

#[tokio::test]
async fn test_mock_notifier_separates_story_and_summary_calls() {
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
    let mock = MockSessionRunner::new();
    let story = make_test_story("1-1-test-story", "", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "1-1-test-story");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_configurable_outcome() {
    let mock = MockSessionRunner::new().with_outcome(|story| SessionOutcome::Failed {
        story_key: story.story_key.clone(),
        error: "test failure".into(),
        decisions: vec![],
    });
    let story = make_test_story("2-1-failing", "", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => {
            assert_eq!(error, "test failure");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();
    let story1 = make_test_story("1-1-first", "", vec![]);
    let story2 = make_test_story("1-2-second", "", vec![]);

    mock.run(&story1).await;
    mock.run(&story2).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-first");
    assert_eq!(calls[1].story_key, "1-2-second");
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
async fn test_mock_review_runner_returns_completed() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("1-1-test-review", "", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "1-1-test-review");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_configurable_outcome() {
    let mock = MockReviewRunner::new().with_outcome(|story| ReviewOutcome::Failed {
        story_key: story.story_key.clone(),
        error: "review crash".into(),
    });
    let story = make_test_story("3-1-failing-review", "", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Failed { error, .. } => {
            assert_eq!(error, "review crash");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("1-1-reviewed", "", vec![]);

    mock.run(&story).await;
    mock.run(&story).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-reviewed");
}

// ---------------------------------------------------------------------------
// Send + Sync verification
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
