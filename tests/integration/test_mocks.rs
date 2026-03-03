//! Self-verification tests for mock implementations.

use crate::helpers::fixtures::make_test_story;
use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

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
        body: "body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await;
    assert!(result.is_ok());
    let pr = result.unwrap();
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 422,
        message: "Validation failed".into(),
    }));

    let params = CreatePrParams {
        title: "test".into(),
        body: "".into(),
        source_branch: "x".into(),
        target_branch: "main".into(),
    };

    let result = mock.create_pr(params).await;
    assert!(result.is_err());
    let err_str = format!("{}", result.unwrap_err());
    assert!(err_str.contains("422"));
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new();

    let params = CreatePrParams {
        title: "PR1".into(),
        body: "b".into(),
        source_branch: "feat".into(),
        target_branch: "main".into(),
    };
    let _ = mock.create_pr(params).await;
    let _ = mock.add_comment("1", "looks good").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);

    // Verify CreatePr captured params
    if let GitProviderCall::CreatePr(p) = &calls[0] {
        assert_eq!(p.title, "PR1");
        assert_eq!(p.source_branch, "feat");
    } else {
        panic!("Expected CreatePr call at index 0");
    }

    // Verify AddComment captured pr_id and body
    if let GitProviderCall::AddComment { pr_id, body } = &calls[1] {
        assert_eq!(pr_id, "1");
        assert_eq!(body, "looks good");
    } else {
        panic!("Expected AddComment call at index 1");
    }

    // Verify GetPrUrl captured pr_id
    if let GitProviderCall::GetPrUrl { pr_id } = &calls[2] {
        assert_eq!(pr_id, "1");
    } else {
        panic!("Expected GetPrUrl call at index 2");
    }
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_success() {
    let mock = MockGitProvider::new().with_add_comment(Ok(()));
    let result = mock.add_comment("10", "nice work").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_success() {
    let mock = MockGitProvider::new()
        .with_get_pr_url(Ok("https://github.com/test/test/pull/99".into()));
    let result = mock.get_pr_url("99").await;
    assert_eq!(result.unwrap(), "https://github.com/test/test/pull/99");
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

    let result = mock.notify_story(&notification).await;
    assert!(result.is_ok());

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "7-1-test");
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
        reason: Some("dependency".into()),
    };

    let _ = mock.notify_story(&n1).await;
    let _ = mock.notify_story(&n2).await;

    let all = mock.calls();
    assert_eq!(all.len(), 2);
    let stories = mock.story_calls();
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-a");
    assert_eq!(stories[1].story_key, "1-2-b");
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let mock = MockSessionRunner::new_completed();
    let story = make_test_story("7-1-test", "Test", vec![]);

    let outcome = mock.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Completed { .. }));

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-test");
}

#[tokio::test]
async fn test_mock_session_runner_returns_failed() {
    let mock = MockSessionRunner::new_failed("test error");
    let story = make_test_story("7-1-test", "Test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => {
            assert_eq!(error, "test error");
        }
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_custom_outcome() {
    let mock = MockSessionRunner::with_outcome(|story| SessionOutcome::Completed {
        story_key: story.story_key.clone(),
        branch: "custom-branch".into(),
        decisions: vec![],
        pr_context: Some("custom context".into()),
        pr_how_to_test: None,
        pr_additional_info: None,
    });
    let story = make_test_story("7-1-test", "Test", vec![]);
    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { pr_context, .. } => {
            assert_eq!(pr_context, Some("custom context".into()));
        }
        _ => panic!("Expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_check_wal_returns_none() {
    let mock = MockSessionRunner::new_completed();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let mock = MockReviewRunner::new_completed();
    let story = make_test_story("7-1-test", "Test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { report, .. } => {
            assert!(!report.is_empty());
        }
        _ => panic!("Expected Completed outcome"),
    }

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-test");
}

#[tokio::test]
async fn test_mock_review_runner_returns_failed() {
    let mock = MockReviewRunner::new_failed("review error");
    let story = make_test_story("7-1-test", "Test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Failed { error, .. } => {
            assert_eq!(error, "review error");
        }
        _ => panic!("Expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_returns_skipped() {
    let mock = MockReviewRunner::new_skipped("provider unavailable");
    let story = make_test_story("7-1-test", "Test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => {
            assert_eq!(reason, "provider unavailable");
        }
        _ => panic!("Expected Skipped outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new_completed();
    let s1 = make_test_story("7-1-a", "A", vec![]);
    let s2 = make_test_story("7-2-b", "B", vec![]);

    let _ = mock.run(&s1).await;
    let _ = mock.run(&s2).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "7-1-a");
    assert_eq!(calls[1].story_key, "7-2-b");
}

#[tokio::test]
async fn test_mock_review_runner_with_outcome() {
    let mock = MockReviewRunner::with_outcome(|story| ReviewOutcome::Completed {
        story_key: story.story_key.clone(),
        branch: "custom-review-branch".into(),
        report: "Custom review report from with_outcome.".to_string(),
    });
    let story = make_test_story("7-1-test", "Test", vec![]);
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { report, branch, .. } => {
            assert_eq!(report, "Custom review report from with_outcome.");
            assert_eq!(branch, "custom-review-branch");
        }
        _ => panic!("Expected Completed outcome from with_outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_returns_escalated() {
    let mock = MockSessionRunner::new_escalated("7-1-test", "What should I do here?");
    let story = make_test_story("7-1-test", "Test", vec![]);

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Escalated { report, .. } => {
            assert_eq!(report.story_key, "7-1-test");
            assert_eq!(report.question, "What should I do here?");
            assert_eq!(report.reason, "mock escalation");
        }
        _ => panic!("Expected Escalated outcome"),
    }

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-test");
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
