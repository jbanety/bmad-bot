//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, StoryNotification, StoryStatus, RunSummary};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;

use std::path::PathBuf;

/// Helper: create a minimal `StoryInfo` for mock tests.
fn test_story() -> StoryInfo {
    StoryInfo {
        story_id: "7.1".to_string(),
        story_key: "7-1-integration-test-infrastructure".to_string(),
        epic_num: 7,
        story_num: 1,
        label: "integration test infrastructure".to_string(),
        branch_name: "story/7-1-integration-test-infrastructure".to_string(),
        specs_path: PathBuf::from(
            "_bmad-output/implementation-artifacts/7-1-integration-test-infrastructure.md",
        ),
        dependencies: vec![],
        status: "ready-for-dev".to_string(),
    }
}

// ---------------------------------------------------------------------------
// MockGitProvider tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
    let provider = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/repo/pull/42".into(),
        number: 42,
    }));

    let result = provider
        .create_pr(CreatePrParams {
            title: "Test PR".into(),
            body: "body".into(),
            source_branch: "story/7-1-test".into(),
            target_branch: "main".into(),
        })
        .await;

    let pr = result.expect("should succeed");
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let provider = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 422,
        message: "Validation failed".into(),
    }));

    let result = provider
        .create_pr(CreatePrParams {
            title: "Test".into(),
            body: "body".into(),
            source_branch: "test".into(),
            target_branch: "main".into(),
        })
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let provider = MockGitProvider::new();

    let _ = provider
        .create_pr(CreatePrParams {
            title: "PR1".into(),
            body: "body".into(),
            source_branch: "branch-1".into(),
            target_branch: "main".into(),
        })
        .await;

    let _ = provider.add_comment("1", "LGTM").await;
    let _ = provider.get_pr_url("1").await;

    let calls = provider.calls();
    assert_eq!(calls.len(), 3);
    assert!(matches!(calls[0], GitProviderCall::CreatePr(_)));
    assert!(matches!(calls[1], GitProviderCall::AddComment { .. }));
    assert!(matches!(calls[2], GitProviderCall::GetPrUrl { .. }));
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_tracks_args() {
    let provider = MockGitProvider::new();
    let _ = provider.add_comment("99", "Review comment").await;

    let calls = provider.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::AddComment { pr_id, body } => {
            assert_eq!(pr_id, "99");
            assert_eq!(body, "Review comment");
        }
        _ => panic!("expected AddComment call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_get_pr_url_returns_configured() {
    let provider = MockGitProvider::new()
        .with_get_pr_url(Ok("https://custom-url.com/pr/5".into()));

    let url = provider.get_pr_url("5").await.expect("should succeed");
    assert_eq!(url, "https://custom-url.com/pr/5");
}

// ---------------------------------------------------------------------------
// MockNotifier tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_calls() {
    let notifier = MockNotifier::new();

    notifier
        .notify_story(&StoryNotification {
            story_id: "7.1".into(),
            story_key: "7-1-test".into(),
            status: StoryStatus::Completed,
            pr_url: Some("https://pr".into()),
            reason: None,
        })
        .await
        .expect("should succeed");

    let story_calls = notifier.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_id, "7.1");
}

#[tokio::test]
async fn test_mock_notifier_captures_summary_calls() {
    let notifier = MockNotifier::new();

    notifier
        .notify_run_summary(&RunSummary {
            stories: vec![],
            total_processed: 0,
            completed: 0,
            blocked: 0,
            errored: 0,
        })
        .await
        .expect("should succeed");

    let summary_calls = notifier.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 0);
}

#[tokio::test]
async fn test_mock_notifier_captures_multiple_calls() {
    let notifier = MockNotifier::new();

    notifier
        .notify_story(&StoryNotification {
            story_id: "1.1".into(),
            story_key: "1-1-test".into(),
            status: StoryStatus::Completed,
            pr_url: None,
            reason: None,
        })
        .await
        .unwrap();

    notifier
        .notify_story(&StoryNotification {
            story_id: "1.2".into(),
            story_key: "1-2-test".into(),
            status: StoryStatus::Blocked,
            pr_url: None,
            reason: Some("blocked".into()),
        })
        .await
        .unwrap();

    notifier
        .notify_run_summary(&RunSummary {
            stories: vec![],
            total_processed: 2,
            completed: 1,
            blocked: 1,
            errored: 0,
        })
        .await
        .unwrap();

    let all_calls = notifier.calls();
    assert_eq!(all_calls.len(), 3);
    assert_eq!(notifier.story_calls().len(), 2);
    assert_eq!(notifier.summary_calls().len(), 1);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_default_completed() {
    let runner = MockSessionRunner::new();
    let story = test_story();

    let outcome = runner.run(&story).await;
    assert!(matches!(outcome, SessionOutcome::Completed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_returns_configured_outcome() {
    let runner = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "7-1-test".into(),
        error: "mock error".into(),
        decisions: vec![],
    });

    let story = test_story();
    let outcome = runner.run(&story).await;

    match outcome {
        SessionOutcome::Failed { error, .. } => {
            assert_eq!(error, "mock error");
        }
        _ => panic!("expected Failed outcome"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let runner = MockSessionRunner::new();
    let story = test_story();

    let _ = runner.run(&story).await;
    let _ = runner.run(&story).await;

    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].story_key, "7-1-integration-test-infrastructure");
}

#[tokio::test]
async fn test_mock_session_runner_check_wal_returns_none() {
    let runner = MockSessionRunner::new();
    let result = runner.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_default_skipped() {
    let runner = MockReviewRunner::new();
    let story = test_story();

    let outcome = runner.run(&story).await;
    assert!(matches!(outcome, ReviewOutcome::Skipped { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_configured_outcome() {
    let runner = MockReviewRunner::new().with_outcome(ReviewOutcome::Completed {
        story_key: "7-1-test".into(),
        branch: "story/7-1-test".into(),
        report: "All good".into(),
    });

    let story = test_story();
    let outcome = runner.run(&story).await;

    match outcome {
        ReviewOutcome::Completed { report, .. } => {
            assert_eq!(report, "All good");
        }
        _ => panic!("expected Completed outcome"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let runner = MockReviewRunner::new();
    let story = test_story();

    let _ = runner.run(&story).await;

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "7-1-integration-test-infrastructure");
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
