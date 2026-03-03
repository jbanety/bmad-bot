//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{
    Notifier, NotifierError, RunSummary, StoryNotification, StoryStatus,
};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

// -----------------------------------------------------------------------
// Task 7.1 — MockGitProvider
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/test/pull/42".into(),
        number: 42,
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "title".into(),
            body: "body".into(),
            source_branch: "story/1-1-test".into(),
            target_branch: "main".into(),
        })
        .await;

    let info = result.expect("expected Ok");
    assert_eq!(info.id, "42");
    assert_eq!(info.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::NetworkError {
        reason: "timeout".into(),
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "t".into(),
            body: "b".into(),
            source_branch: "s".into(),
            target_branch: "m".into(),
        })
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(format!("{err}").contains("timeout"));
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    mock.create_pr(CreatePrParams {
        title: "t".into(),
        body: "b".into(),
        source_branch: "s".into(),
        target_branch: "m".into(),
    })
    .await
    .ok();

    mock.add_comment("1", "comment body").await.ok();
    mock.get_pr_url("1").await.ok();

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);

    match &calls[0] {
        GitProviderCall::CreatePr(params) => assert_eq!(params.title, "t"),
        other => panic!("expected CreatePr, got {other:?}"),
    }
    match &calls[1] {
        GitProviderCall::AddComment { pr_id, body } => {
            assert_eq!(pr_id, "1");
            assert_eq!(body, "comment body");
        }
        other => panic!("expected AddComment, got {other:?}"),
    }
    match &calls[2] {
        GitProviderCall::GetPrUrl { pr_id } => assert_eq!(pr_id, "1"),
        other => panic!("expected GetPrUrl, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_returns_ok() {
    let mock = MockGitProvider::new();
    let result = mock.add_comment("1", "test").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_ok() {
    let mock = MockGitProvider::new();
    let result = mock.get_pr_url("1").await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("github.com"));
}

// -----------------------------------------------------------------------
// Task 7.2 — MockNotifier
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/pr/1".into()),
        reason: None,
    };

    mock.notify_story(&notification).await.expect("should succeed");

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "1-1-test");

    assert!(mock.summary_calls().is_empty());
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

    mock.notify_run_summary(&summary)
        .await
        .expect("should succeed");

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);

    assert!(mock.story_calls().is_empty());
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
        status: StoryStatus::Error,
        pr_url: None,
        reason: Some("failed".into()),
    };

    mock.notify_story(&n1).await.ok();
    mock.notify_story(&n2).await.ok();

    assert_eq!(mock.story_calls().len(), 2);
}

// -----------------------------------------------------------------------
// Task 7.3 — MockSessionRunner
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed_by_default() {
    let mock = MockSessionRunner::new();
    let story = crate::helpers::fixtures::make_test_story("1-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => assert_eq!(story_key, "1-1-test"),
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_outcome() {
    let mock = MockSessionRunner::new().with_outcome(|story| SessionOutcome::Failed {
        story_key: story.story_key.clone(),
        error: "test error".into(),
        decisions: vec![],
    });
    let story = crate::helpers::fixtures::make_test_story("2-1-fail", "fail", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { story_key, error, .. } => {
            assert_eq!(story_key, "2-1-fail");
            assert_eq!(error, "test error");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();
    let s1 = crate::helpers::fixtures::make_test_story("1-1-a", "a", vec![]);
    let s2 = crate::helpers::fixtures::make_test_story("1-2-b", "b", vec![]);

    mock.run(&s1).await;
    mock.run(&s2).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-a");
    assert_eq!(calls[1].story_key, "1-2-b");
}

#[tokio::test]
async fn test_mock_session_runner_check_wal_returns_none() {
    let mock = MockSessionRunner::new();
    assert!(mock.check_and_recover_wal().await.is_none());
}

// -----------------------------------------------------------------------
// Task 7.4 — MockReviewRunner
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed_by_default() {
    let mock = MockReviewRunner::new();
    let story = crate::helpers::fixtures::make_test_story("1-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { story_key, report, .. } => {
            assert_eq!(story_key, "1-1-test");
            assert!(report.contains("Mock review"));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_outcome() {
    let mock = MockReviewRunner::new().with_outcome(|_story| ReviewOutcome::Skipped {
        reason: "disabled".into(),
    });
    let story = crate::helpers::fixtures::make_test_story("1-1-skip", "skip", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => assert_eq!(reason, "disabled"),
        other => panic!("expected Skipped, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let story = crate::helpers::fixtures::make_test_story("1-1-x", "x", vec![]);
    mock.run(&story).await;
    mock.run(&story).await;

    assert_eq!(mock.calls().len(), 2);
    assert_eq!(mock.calls()[0].story_key, "1-1-x");
}

// -----------------------------------------------------------------------
// Task 7.9 — Send + Sync bounds
// -----------------------------------------------------------------------

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
