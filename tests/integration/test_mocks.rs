//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;

use std::path::PathBuf;

fn make_story(key: &str) -> StoryInfo {
    StoryInfo {
        story_id: "1.1".into(),
        story_key: key.into(),
        epic_num: 1,
        story_num: 1,
        label: "test".into(),
        branch_name: format!("story/{key}"),
        specs_path: PathBuf::from(format!("_bmad-output/implementation-artifacts/{key}.md")),
        dependencies: vec![],
        status: "ready-for-dev".into(),
    }
}

// ---------------------------------------------------------------------------
// MockGitProvider tests (Task 7.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_default_success() {
    let mock = MockGitProvider::new();
    let params = CreatePrParams {
        title: "test PR".into(),
        body: "test body".into(),
        source_branch: "story/test".into(),
        target_branch: "main".into(),
    };
    let result = mock.create_pr(params).await;
    assert!(result.is_ok());
    let info = result.unwrap();
    assert_eq!(info.id, "1");
    assert_eq!(info.number, 1);
}

#[tokio::test]
async fn test_mock_git_provider_tracks_create_pr_calls() {
    let mock = MockGitProvider::new();
    let params = CreatePrParams {
        title: "test PR".into(),
        body: "test body".into(),
        source_branch: "story/test".into(),
        target_branch: "main".into(),
    };
    mock.create_pr(params).await.unwrap();
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::CreatePr(p) => {
            assert_eq!(p.title, "test PR");
            assert_eq!(p.source_branch, "story/test");
        }
        _ => panic!("Expected CreatePr call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_tracks_add_comment_calls() {
    let mock = MockGitProvider::new();
    mock.add_comment("42", "looks good").await.unwrap();
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::AddComment(id, body) => {
            assert_eq!(id, "42");
            assert_eq!(body, "looks good");
        }
        _ => panic!("Expected AddComment call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_tracks_get_pr_url_calls() {
    let mock = MockGitProvider::new();
    let url = mock.get_pr_url("42").await.unwrap();
    assert!(url.contains("github.com"));
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        GitProviderCall::GetPrUrl(id) => assert_eq!(id, "42"),
        _ => panic!("Expected GetPrUrl call"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_configurable_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::NetworkError {
        reason: "connection refused".into(),
    }));
    let params = CreatePrParams {
        title: "test".into(),
        body: "test".into(),
        source_branch: "story/test".into(),
        target_branch: "main".into(),
    };
    let result = mock.create_pr(params).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("connection refused")
    );
}

#[tokio::test]
async fn test_mock_git_provider_configurable_success() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "99".into(),
        url: "https://custom.url/99".into(),
        number: 99,
    }));
    let params = CreatePrParams {
        title: "test".into(),
        body: "test".into(),
        source_branch: "story/test".into(),
        target_branch: "main".into(),
    };
    let result = mock.create_pr(params).await.unwrap();
    assert_eq!(result.id, "99");
    assert_eq!(result.number, 99);
}

#[tokio::test]
async fn test_mock_git_provider_multiple_calls_tracked() {
    let mock = MockGitProvider::new();
    let params = CreatePrParams {
        title: "test".into(),
        body: "test".into(),
        source_branch: "story/test".into(),
        target_branch: "main".into(),
    };
    mock.create_pr(params).await.unwrap();
    mock.add_comment("1", "comment").await.unwrap();
    mock.get_pr_url("1").await.unwrap();
    assert_eq!(mock.calls().len(), 3);
}

// ---------------------------------------------------------------------------
// MockNotifier tests (Task 7.2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();
    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/pull/1".into()),
        reason: None,
    };
    mock.notify_story(&notification).await.unwrap();
    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "1-1-test");
    assert_eq!(story_calls[0].status, StoryStatus::Completed);
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
    assert_eq!(summary_calls[0].completed, 2);
}

#[tokio::test]
async fn test_mock_notifier_tracks_all_calls() {
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
    assert_eq!(mock.story_calls().len(), 1);
    assert_eq!(mock.summary_calls().len(), 1);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests (Task 7.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_default_completed() {
    let mock = MockSessionRunner::new();
    let story = make_story("1-1-test");
    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => {
            assert_eq!(story_key, "test");
        }
        other => panic!("Expected Completed, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_configurable_failed() {
    let mock = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "intentional failure".into(),
        decisions: vec![],
    });
    let story = make_story("1-1-test");
    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => {
            assert_eq!(error, "intentional failure");
        }
        other => panic!("Expected Failed, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let mock = MockSessionRunner::new();
    let story = make_story("1-1-test");
    mock.run(&story).await;
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_returns_none() {
    let mock = MockSessionRunner::new();
    let result = mock.check_and_recover_wal().await;
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests (Task 7.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_default_skipped() {
    let mock = MockReviewRunner::new();
    let story = make_story("1-1-test");
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => {
            assert!(reason.contains("mock"));
        }
        other => panic!("Expected Skipped, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_configurable_completed() {
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Completed {
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        report: "LGTM".into(),
    });
    let story = make_story("1-1-test");
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { report, .. } => {
            assert_eq!(report, "LGTM");
        }
        other => panic!("Expected Completed, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_configurable_failed() {
    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "review crashed".into(),
    });
    let story = make_story("1-1-test");
    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Failed { error, .. } => {
            assert_eq!(error, "review crashed");
        }
        other => panic!("Expected Failed, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let mock = MockReviewRunner::new();
    let story = make_story("1-1-test");
    mock.run(&story).await;
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-test");
}

// ---------------------------------------------------------------------------
// Send + Sync bound tests (Task 7.9)
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
