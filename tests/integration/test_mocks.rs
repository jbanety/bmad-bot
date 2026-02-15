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
async fn test_mock_git_provider_returns_configured_create_pr() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://example.com/pr/42".into(),
        number: 42,
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "test".into(),
            body: "body".into(),
            source_branch: "story/1-1-test".into(),
            target_branch: "main".into(),
        })
        .await;

    let pr = result.expect("should succeed");
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
    assert_eq!(pr.url, "https://example.com/pr/42");
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::AuthenticationFailed {
        reason: "bad token".into(),
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "test".into(),
            body: "body".into(),
            source_branch: "story/1-1-test".into(),
            target_branch: "main".into(),
        })
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        format!("{err}").contains("bad token"),
        "error should contain 'bad token': {err}"
    );
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    mock.create_pr(CreatePrParams {
        title: "PR title".into(),
        body: "PR body".into(),
        source_branch: "story/1-1-test".into(),
        target_branch: "main".into(),
    })
    .await
    .unwrap();

    mock.add_comment("1", "nice work").await.unwrap();
    mock.get_pr_url("1").await.unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);

    match &calls[0] {
        GitProviderCall::CreatePr { title, .. } => assert_eq!(title, "PR title"),
        other => panic!("expected CreatePr, got {other:?}"),
    }
    match &calls[1] {
        GitProviderCall::AddComment { pr_id, body } => {
            assert_eq!(pr_id, "1");
            assert_eq!(body, "nice work");
        }
        other => panic!("expected AddComment, got {other:?}"),
    }
    match &calls[2] {
        GitProviderCall::GetPrUrl { pr_id } => assert_eq!(pr_id, "1"),
        other => panic!("expected GetPrUrl, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_configurable() {
    let mock =
        MockGitProvider::new().with_add_comment(Err(GitProviderError::NetworkError {
            reason: "timeout".into(),
        }));

    let result = mock.add_comment("1", "text").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_configurable() {
    let mock = MockGitProvider::new()
        .with_get_pr_url(Ok("https://custom.url/pr/99".into()));

    let url = mock.get_pr_url("99").await.unwrap();
    assert_eq!(url, "https://custom.url/pr/99");
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://example.com/pr/1".into()),
        reason: None,
    };

    mock.notify_story(&notification).await.unwrap();

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "1-1-test");
    assert_eq!(story_calls[0].story_id, "1.1");
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
async fn test_mock_notifier_calls_method_returns_all() {
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
        total_processed: 1,
        completed: 1,
        blocked: 0,
        errored: 0,
        fatal: false,
    };

    mock.notify_story(&notification).await.unwrap();
    mock.notify_run_summary(&summary).await.unwrap();

    assert_eq!(mock.calls().len(), 2);
}

#[tokio::test]
async fn test_mock_notifier_error_returns_disabled() {
    let mock = MockNotifier::new()
        .with_story_result(Err(NotifierError::Disabled));

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

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let mock = MockSessionRunner::new();
    let story = make_test_story("1-1-test-story", "test story", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "1-1-test-story");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_custom_outcome() {
    let mock = MockSessionRunner::new().with_outcome(|story| SessionOutcome::Failed {
        story_key: story.story_key.clone(),
        error: "boom".into(),
        decisions: vec![],
    });

    let story = make_test_story("2-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;

    match outcome {
        SessionOutcome::Failed { error, .. } => assert_eq!(error, "boom"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();

    let s1 = make_test_story("1-1-a", "a", vec![]);
    let s2 = make_test_story("2-1-b", "b", vec![]);

    mock.run(&s1).await;
    mock.run(&s2).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-a");
    assert_eq!(calls[1].story_key, "2-1-b");
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
    let story = make_test_story("1-1-test", "test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed {
            story_key, report, ..
        } => {
            assert_eq!(story_key, "1-1-test");
            assert_eq!(report, "Mock review report");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_custom_outcome() {
    let mock = MockReviewRunner::new().with_outcome(|_| ReviewOutcome::Skipped {
        reason: "disabled".into(),
    });

    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;

    match outcome {
        ReviewOutcome::Skipped { reason } => assert_eq!(reason, "disabled"),
        other => panic!("expected Skipped, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();

    let s1 = make_test_story("1-1-a", "a", vec![]);
    let s2 = make_test_story("2-1-b", "b", vec![]);

    mock.run(&s1).await;
    mock.run(&s2).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-a");
    assert_eq!(calls[1].story_key, "2-1-b");
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
