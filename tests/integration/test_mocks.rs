//! Self-verification tests for mock implementations.

use crate::helpers::mocks::*;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;

use std::path::PathBuf;

fn make_story(key: &str) -> StoryInfo {
    StoryInfo {
        story_id: "1.1".to_string(),
        story_key: key.to_string(),
        epic_num: 1,
        story_num: 1,
        label: "test".to_string(),
        branch_name: format!("story/{key}"),
        specs_path: PathBuf::from(format!("{key}.md")),
        dependencies: vec![],
        status: "ready-for-dev".to_string(),
    }
}

// ---------------------------------------------------------------------------
// MockGitProvider tests (Task 7.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_configured_value() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".to_string(),
        url: "https://github.com/test/test/pull/42".to_string(),
        number: 42,
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "test PR".to_string(),
            body: "body".to_string(),
            source_branch: "story/1-1-test".to_string(),
            target_branch: "main".to_string(),
        })
        .await;

    let pr = result.expect("should return Ok");
    assert_eq!(pr.id, "42");
    assert_eq!(pr.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_create_pr_returns_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(GitProviderError::ProviderNotConfigured {
        provider: "test".to_string(),
    }));

    let result = mock
        .create_pr(CreatePrParams {
            title: "t".to_string(),
            body: "b".to_string(),
            source_branch: "s".to_string(),
            target_branch: "m".to_string(),
        })
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new()
        .with_create_pr(Ok(PrInfo {
            id: "1".to_string(),
            url: "u".to_string(),
            number: 1,
        }))
        .with_add_comment(Ok(()))
        .with_get_pr_url(Ok("url".to_string()));

    let _ = mock
        .create_pr(CreatePrParams {
            title: "t".to_string(),
            body: "b".to_string(),
            source_branch: "s".to_string(),
            target_branch: "m".to_string(),
        })
        .await;
    let _ = mock.add_comment("1", "comment").await;
    let _ = mock.get_pr_url("1").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 3);
    assert!(matches!(calls[0], GitProviderCall::CreatePr(_)));
    assert!(matches!(calls[1], GitProviderCall::AddComment { .. }));
    assert!(matches!(calls[2], GitProviderCall::GetPrUrl { .. }));
}

#[tokio::test]
async fn test_mock_git_provider_add_comment_tracks_args() {
    let mock = MockGitProvider::new().with_add_comment(Ok(()));

    let _ = mock.add_comment("pr-99", "my comment body").await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    if let GitProviderCall::AddComment { pr_id, body } = &calls[0] {
        assert_eq!(pr_id, "pr-99");
        assert_eq!(body, "my comment body");
    } else {
        panic!("expected AddComment call");
    }
}

// ---------------------------------------------------------------------------
// MockNotifier tests (Task 7.2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notification() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".to_string(),
        story_key: "1-1-test".to_string(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/pull/1".to_string()),
        reason: None,
    };

    mock.notify_story(&notification).await.expect("should succeed");

    let calls = mock.story_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summary() {
    let mock = MockNotifier::new();

    let summary = RunSummary {
        stories: vec![],
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
        fatal: false,
    };

    mock.notify_run_summary(&summary).await.expect("should succeed");

    let summaries = mock.summary_calls();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].total_processed, 0);
}

#[tokio::test]
async fn test_mock_notifier_captures_multiple_calls() {
    let mock = MockNotifier::new();

    let n1 = StoryNotification {
        story_id: "1.1".to_string(),
        story_key: "1-1-a".to_string(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };
    let n2 = StoryNotification {
        story_id: "1.2".to_string(),
        story_key: "1-2-b".to_string(),
        status: StoryStatus::Blocked,
        pr_url: None,
        reason: Some("dep missing".to_string()),
    };

    mock.notify_story(&n1).await.unwrap();
    mock.notify_story(&n2).await.unwrap();

    let all = mock.calls();
    assert_eq!(all.len(), 2);
    let stories = mock.story_calls();
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-a");
    assert_eq!(stories[1].story_key, "1-2-b");
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests (Task 7.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_completed() {
    let outcome = SessionOutcome::Completed {
        story_key: "1-1-test".to_string(),
        branch: "story/1-1-test".to_string(),
        decisions: vec![],
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    };
    let mock = MockSessionRunner::new(outcome);
    let story = make_story("1-1-test");

    let result = mock.run(&story).await;
    assert!(matches!(result, SessionOutcome::Completed { .. }));

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_session_runner_returns_failed() {
    let outcome = SessionOutcome::Failed {
        story_key: "2-1-fail".to_string(),
        error: "boom".to_string(),
        decisions: vec![],
    };
    let mock = MockSessionRunner::new(outcome);
    let story = make_story("2-1-fail");

    let result = mock.run(&story).await;
    assert!(matches!(result, SessionOutcome::Failed { .. }));
}

#[tokio::test]
async fn test_mock_session_runner_wal_returns_none() {
    let outcome = SessionOutcome::Completed {
        story_key: "x".to_string(),
        branch: "b".to_string(),
        decisions: vec![],
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    };
    let mock = MockSessionRunner::new(outcome);

    let recovery = mock.check_and_recover_wal().await;
    assert!(recovery.is_none());
    assert_eq!(mock.wal_call_count(), 1);
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests (Task 7.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_completed() {
    let outcome = ReviewOutcome::Completed {
        story_key: "1-1-test".to_string(),
        branch: "story/1-1-test".to_string(),
        report: "LGTM".to_string(),
    };
    let mock = MockReviewRunner::new(outcome);
    let story = make_story("1-1-test");

    let result = mock.run(&story).await;
    assert!(matches!(result, ReviewOutcome::Completed { .. }));

    let calls = mock.run_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_review_runner_returns_skipped() {
    let outcome = ReviewOutcome::Skipped {
        reason: "disabled".to_string(),
    };
    let mock = MockReviewRunner::new(outcome);
    let story = make_story("1-1-test");

    let result = mock.run(&story).await;
    assert!(matches!(result, ReviewOutcome::Skipped { .. }));
}

#[tokio::test]
async fn test_mock_review_runner_returns_failed() {
    let outcome = ReviewOutcome::Failed {
        story_key: "1-1-test".to_string(),
        error: "crash".to_string(),
    };
    let mock = MockReviewRunner::new(outcome);
    let story = make_story("1-1-test");

    let result = mock.run(&story).await;
    assert!(matches!(result, ReviewOutcome::Failed { .. }));
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
