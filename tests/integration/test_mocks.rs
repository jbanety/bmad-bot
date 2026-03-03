//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use crate::helpers::fixtures;

// ---------------------------------------------------------------------------
// MockGitProvider tests (7.1, 7.9)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_configured_ok() {
    let mock = MockGitProvider::new().with_create_pr(|| {
        Ok(PrInfo {
            id: "42".into(),
            url: "https://example.com/pr/42".into(),
            number: 42,
        })
    });

    let result = mock
        .create_pr(CreatePrParams {
            title: "test".into(),
            body: "body".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
        })
        .await;

    let info = result.expect("should be Ok");
    assert_eq!(info.id, "42");
    assert_eq!(info.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_returns_configured_error() {
    let mock = MockGitProvider::new().with_add_comment(|| {
        Err(GitProviderError::AuthenticationFailed {
            reason: "bad token".into(),
        })
    });
    let result = mock.add_comment("1", "body").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured_error() {
    let mock = MockGitProvider::new().with_get_pr_url(|| {
        Err(GitProviderError::AuthenticationFailed {
            reason: "bad token".into(),
        })
    });
    let result = mock.get_pr_url("1").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(|| {
        Err(GitProviderError::AuthenticationFailed {
            reason: "bad token".into(),
        })
    });

    let result = mock
        .create_pr(CreatePrParams {
            title: "t".into(),
            body: "b".into(),
            source_branch: "f".into(),
            target_branch: "m".into(),
        })
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_tracks_calls() {
    let mock = MockGitProvider::new();
    let _ = mock.add_comment("99", "looks good").await;
    let _ = mock.add_comment("100", "needs work").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    match &calls[0] {
        GitProviderCall::AddComment { pr_id, body } => {
            assert_eq!(pr_id, "99");
            assert_eq!(body, "looks good");
        }
        other => panic!("expected AddComment, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_default() {
    let mock = MockGitProvider::new();
    let url = mock.get_pr_url("1").await.expect("should be Ok");
    assert!(url.starts_with("https://"));
}

#[tokio::test]
async fn test_mock_git_provider_tracks_all_call_types() {
    let mock = MockGitProvider::new();
    let _ = mock
        .create_pr(CreatePrParams {
            title: "t".into(),
            body: "b".into(),
            source_branch: "s".into(),
            target_branch: "m".into(),
        })
        .await;
    let _ = mock.add_comment("1", "c").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);
    assert!(matches!(calls[0], GitProviderCall::CreatePr(_)));
    assert!(matches!(calls[1], GitProviderCall::AddComment { .. }));
    assert!(matches!(calls[2], GitProviderCall::GetPrUrl(_)));
}

// ---------------------------------------------------------------------------
// MockNotifier tests (7.2, 7.9)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    let mock = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-integration-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://example.com/pr/1".into()),
        reason: None,
    };

    mock.notify_story(&notification).await.expect("should succeed");

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_id, "7.1");
    assert_eq!(story_calls[0].story_key, "7-1-integration-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summary() {
    let mock = MockNotifier::new();
    let summary = RunSummary {
        stories: vec![],
        total_processed: 3,
        completed: 2,
        blocked: 0,
        errored: 1,
        fatal: false,
    };

    mock.notify_run_summary(&summary).await.expect("should succeed");

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);
    assert_eq!(summary_calls[0].completed, 2);
}

#[tokio::test]
async fn test_mock_notifier_calls_returns_both_types() {
    let mock = MockNotifier::new();

    let story = StoryNotification {
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

    mock.notify_story(&story).await.unwrap();
    mock.notify_run_summary(&summary).await.unwrap();

    let all = mock.calls();
    assert_eq!(all.len(), 2);
    assert!(matches!(all[0], NotifierCall::Story(_)));
    assert!(matches!(all[1], NotifierCall::RunSummary(_)));
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests (7.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_completed() {
    let mock = MockSessionRunner::completed();
    let story = fixtures::make_test_story("7-1-integration-test", "integration test", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Completed { .. }));

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-integration-test");
}

#[tokio::test]
async fn test_mock_session_runner_escalated() {
    let mock = MockSessionRunner::escalated();
    let story = fixtures::make_test_story("3-3-escalation", "escalation", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Escalated { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_failed() {
    let mock = MockSessionRunner::failed("compile error");
    let story = fixtures::make_test_story("2-1-polling", "polling", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => assert_eq!(error, "compile error"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_returns_none() {
    // check_and_recover_wal must return Option<RecoveryInfo>, matching the real SessionRunner API.
    let mock = MockSessionRunner::completed();
    let result: Option<bmad_bot::session::runner::RecoveryInfo> =
        mock.check_and_recover_wal().await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_mock_session_runner_tracks_multiple_calls() {
    let mock = MockSessionRunner::completed();
    let s1 = fixtures::make_test_story("1-1-first", "first", vec![]);
    let s2 = fixtures::make_test_story("1-2-second", "second", vec![]);

    let _ = mock.run(&s1).await;
    let _ = mock.run(&s2).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "1-1-first");
    assert_eq!(calls[1].story_key, "1-2-second");
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests (7.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_completed() {
    let mock = MockReviewRunner::completed();
    let story = fixtures::make_test_story("5-1-review", "review", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed {
            story_key, report, ..
        } => {
            assert_eq!(story_key, "5-1-review");
            assert!(!report.is_empty());
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_skipped() {
    let mock = MockReviewRunner::skipped("disabled");
    let story = fixtures::make_test_story("5-2-skip", "skip", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => assert_eq!(reason, "disabled"),
        other => panic!("expected Skipped, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_failed() {
    let mock = MockReviewRunner::failed("timeout");
    let story = fixtures::make_test_story("5-3-fail", "fail", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Failed { error, .. } => assert_eq!(error, "timeout"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::completed();
    let story = fixtures::make_test_story("5-1-review", "review", vec![]);
    let _ = mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "5-1-review");
}

// ---------------------------------------------------------------------------
// Send + Sync bound verification (7.9)
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
