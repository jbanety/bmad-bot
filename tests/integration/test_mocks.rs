//! Self-verification tests for mock implementations.
//!
//! Validates that all mocks satisfy trait bounds, return configured values,
//! and correctly track calls.

use crate::helpers::mocks::*;

use crate::helpers::fixtures::make_test_story;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

// ---------------------------------------------------------------------------
// Send + Sync bound verification
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

    let params = CreatePrParams {
        title: "test PR".into(),
        body: "test body".into(),
        source_branch: "feature/test".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await;
    assert!(result.is_ok());
    let pr = result.expect("should be Ok");
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::AuthenticationFailed {
        reason: "bad token".into(),
    }));

    let params = CreatePrParams {
        title: "test".into(),
        body: "test".into(),
        source_branch: "feat".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    let params = CreatePrParams {
        title: "t".into(),
        body: "b".into(),
        source_branch: "s".into(),
        target_branch: "m".into(),
    };
    let _ = mock.create_pr(params).await;
    let _ = mock.add_comment("1", "comment").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);
    assert!(matches!(calls[0], GitProviderCall::CreatePr(_)));
    assert!(matches!(calls[1], GitProviderCall::AddComment(_, _)));
    assert!(matches!(calls[2], GitProviderCall::GetPrUrl(_)));
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_ok() {
    let mock = MockGitProvider::new();
    let result = mock.add_comment("1", "looks good").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_ok() {
    let mock = MockGitProvider::new();
    let result = mock.get_pr_url("1").await;
    assert!(result.is_ok());
    assert!(result.expect("ok").contains("github.com"));
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "7.1".into(),
        story_key: "7-1-infra".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/test/pull/1".into()),
        reason: None,
    };

    let result = mock.notify_story(&notification).await;
    assert!(result.is_ok());

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "7-1-infra");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summary() {
    let mock = MockNotifier::new();

    let summary = RunSummary {
        stories: Vec::new(),
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
    };

    let result = mock.notify_run_summary(&summary).await;
    assert!(result.is_ok());

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 0);
}

#[tokio::test]
async fn test_mock_notifier_returns_configured_error() {
    let mock = MockNotifier::new().with_story_error(NotifierError::HttpRequest {
        reason: "connection refused".into(),
    });

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Error,
        pr_url: None,
        reason: Some("test".into()),
    };

    let result = mock.notify_story(&notification).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_notifier_all_calls_accessor() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };

    let summary = RunSummary {
        stories: Vec::new(),
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
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
    let story = make_test_story("7-1-infra", "infra", Vec::new());

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_failed() {
    let mock = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "7-1-infra".into(),
        error: "test error".into(),
        decisions: Vec::new(),
    });
    let story = make_test_story("7-1-infra", "infra", Vec::new());

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Failed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_escalated() {
    use bmad_bot::session::escalation::EscalationReport;

    let report = EscalationReport::new(
        "7-1-infra".into(),
        "How do I proceed?".into(),
        "Documentation unclear".into(),
        "story/7-1-infra".into(),
        "Partial work preserved".into(),
    );
    let mock = MockSessionRunner::new().with_outcome(SessionOutcome::Escalated {
        report,
        decisions: Vec::new(),
    });
    let story = make_test_story("7-1-infra", "infra", Vec::new());

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Escalated { .. }));
    if let SessionOutcome::Escalated { report, .. } = outcome {
        assert_eq!(report.story_key, "7-1-infra");
        assert_eq!(report.question, "How do I proceed?");
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_run_calls() {
    let mock = MockSessionRunner::new();
    let story1 = make_test_story("7-1-infra", "infra", Vec::new());
    let story2 = make_test_story("7-2-config", "config", Vec::new());

    let _ = mock.run(&story1).await;
    let _ = mock.run(&story2).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "7-1-infra");
    assert_eq!(calls[1].story_key, "7-2-config");
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_returns_none() {
    let mock = MockSessionRunner::new();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
    assert_eq!(mock.wal_call_count(), 1);
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("7-1-infra", "infra", Vec::new());

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_skipped() {
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Skipped {
        reason: "review disabled".into(),
    });
    let story = make_test_story("7-1-infra", "infra", Vec::new());

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Skipped { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_failed() {
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Failed {
        story_key: "7-1-infra".into(),
        error: "crash".into(),
    });
    let story = make_test_story("7-1-infra", "infra", Vec::new());

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Failed { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let story = make_test_story("7-1-infra", "infra", Vec::new());

    let _ = mock.run(&story).await;
    let _ = mock.run(&story).await;

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "7-1-infra");
}
