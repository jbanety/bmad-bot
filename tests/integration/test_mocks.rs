//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::Notifier;
use bmad_bot::notifier::{RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

use crate::helpers::fixtures::make_test_story;

// ---------------------------------------------------------------------------
// MockGitProvider tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
    let provider = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/org/repo/pull/42".into(),
        number: 42,
    }));

    let result = provider
        .create_pr(CreatePrParams {
            title: "test".into(),
            body: "body".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
        })
        .await;

    let info = result.expect("should succeed");
    assert_eq!(info.id, "42");
    assert_eq!(info.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let provider = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "Internal Server Error".into(),
    }));

    let result = provider
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
async fn test_mock_git_provider_tracks_calls() {
    let provider = MockGitProvider::new();

    provider
        .create_pr(CreatePrParams {
            title: "PR Title".into(),
            body: "PR Body".into(),
            source_branch: "story/1-1-test".into(),
            target_branch: "main".into(),
        })
        .await
        .expect("should succeed");

    provider
        .add_comment("1", "LGTM")
        .await
        .expect("should succeed");

    provider.get_pr_url("1").await.expect("should succeed");

    let calls = provider.calls();
    assert_eq!(calls.len(), 3);

    match &calls[0] {
        GitProviderCall::CreatePr(params) => {
            assert_eq!(params.title, "PR Title");
            assert_eq!(params.source_branch, "story/1-1-test");
        }
        _ => panic!("Expected CreatePr call"),
    }

    match &calls[1] {
        GitProviderCall::AddComment(pr_id, body) => {
            assert_eq!(pr_id, "1");
            assert_eq!(body, "LGTM");
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
async fn test_mock_git_provider_add_comment_configurable() {
    let provider =
        MockGitProvider::new().with_add_comment(Err(GitProviderError::AuthenticationFailed {
            reason: "bad token".into(),
        }));

    let result = provider.add_comment("1", "comment").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_configurable() {
    let provider = MockGitProvider::new().with_get_pr_url(Ok("https://custom.url/pr/99".into()));

    let url = provider.get_pr_url("99").await.expect("should succeed");
    assert_eq!(url, "https://custom.url/pr/99");
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    let notifier = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/pull/1".into()),
        reason: None,
    };

    notifier
        .notify_story(&notification)
        .await
        .expect("should succeed");

    let calls = notifier.calls();
    assert_eq!(calls.len(), 1);

    let story_calls = notifier.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "1-1-test");

    let summary_calls = notifier.summary_calls();
    assert_eq!(summary_calls.len(), 0);
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summaries() {
    let notifier = MockNotifier::new();

    let summary = RunSummary {
        stories: vec![],
        total_processed: 3,
        completed: 2,
        blocked: 1,
        errored: 0,
    };

    notifier
        .notify_run_summary(&summary)
        .await
        .expect("should succeed");

    let summary_calls = notifier.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_captures_multiple_calls() {
    let notifier = MockNotifier::new();

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
        reason: Some("dependency".into()),
    };

    notifier.notify_story(&n1).await.expect("ok");
    notifier.notify_story(&n2).await.expect("ok");

    let calls = notifier.story_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-a");
    assert_eq!(calls[1].story_key, "1-2-b");
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let mock = MockSessionRunner::new(SessionOutcome::Completed {
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        decisions: vec![],
    });

    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;

    match outcome {
        SessionOutcome::Completed {
            story_key, branch, ..
        } => {
            assert_eq!(story_key, "1-1-test");
            assert_eq!(branch, "story/1-1-test");
        }
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_failed() {
    let mock = MockSessionRunner::new(SessionOutcome::Failed {
        story_key: "1-1-fail".into(),
        error: "something broke".into(),
        decisions: vec![],
    });

    let story = make_test_story("1-1-fail", "fail", vec![]);
    let outcome = mock.run(&story).await;

    match outcome {
        SessionOutcome::Failed { error, .. } => {
            assert_eq!(error, "something broke");
        }
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new(SessionOutcome::Completed {
        story_key: "2-1-watcher".into(),
        branch: "story/2-1-watcher".into(),
        decisions: vec![],
    });

    let story1 = make_test_story("2-1-watcher", "watcher", vec![]);
    let story2 = make_test_story("2-2-deps", "deps", vec!["2-1-watcher".into()]);

    mock.run(&story1).await;
    mock.run(&story2).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "2-1-watcher");
    assert_eq!(calls[1].story_key, "2-2-deps");
}

#[tokio::test]
async fn test_mock_session_runner_check_and_recover_wal_returns_none() {
    let mock = MockSessionRunner::new(SessionOutcome::Completed {
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        decisions: vec![],
    });

    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let mock = MockReviewRunner::new(ReviewOutcome::Completed {
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        report: "All good".into(),
    });

    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;

    match outcome {
        ReviewOutcome::Completed { report, .. } => {
            assert_eq!(report, "All good");
        }
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_skipped() {
    let mock = MockReviewRunner::new(ReviewOutcome::Skipped {
        reason: "review disabled".into(),
    });

    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;

    match outcome {
        ReviewOutcome::Skipped { reason } => {
            assert_eq!(reason, "review disabled");
        }
        _ => panic!("Expected Skipped outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new(ReviewOutcome::Completed {
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        report: "OK".into(),
    });

    let story = make_test_story("1-1-test", "test", vec![]);
    mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_review_runner_returns_failed() {
    let mock = MockReviewRunner::new(ReviewOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "review crashed".into(),
    });

    let story = make_test_story("1-1-test", "test", vec![]);
    let outcome = mock.run(&story).await;

    match outcome {
        ReviewOutcome::Failed { error, .. } => {
            assert_eq!(error, "review crashed");
        }
        _ => panic!("Expected Failed outcome"),
    }
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
