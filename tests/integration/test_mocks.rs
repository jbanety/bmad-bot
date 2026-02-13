//! Self-verification tests for mock implementations.

use crate::helpers::fixtures::make_test_story;
use crate::helpers::mocks::*;

use bmad_bot::git_provider::PrInfo;
use bmad_bot::notifier::{StoryNotification, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;

// ---------------------------------------------------------------------------
// MockGitProvider tests (Task 7.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_git_provider_returns_configured_create_pr() {
    let mock = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/pr/42".into(),
        number: 42,
    }));

    let params = bmad_bot::git_provider::CreatePrParams {
        title: "test PR".into(),
        body: "body".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
    };

    let result = <MockGitProvider as bmad_bot::git_provider::GitProvider>::create_pr(&mock, params)
        .await
        .expect("should succeed");
    assert_eq!(result.id, "42");
    assert_eq!(result.number, 42);
}

#[tokio::test]
async fn test_mock_git_provider_returns_configured_error() {
    let mock = MockGitProvider::new().with_create_pr(Err(
        bmad_bot::git_provider::GitProviderError::ProviderNotConfigured {
            provider: "test".into(),
        },
    ));

    let params = bmad_bot::git_provider::CreatePrParams {
        title: "t".into(),
        body: "b".into(),
        source_branch: "s".into(),
        target_branch: "m".into(),
    };

    let result = <MockGitProvider as bmad_bot::git_provider::GitProvider>::create_pr(&mock, params)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_git_provider_tracks_calls() {
    let mock = MockGitProvider::new()
        .with_add_comment(Ok(()))
        .with_get_pr_url(Ok("https://url".into()));

    use bmad_bot::git_provider::GitProvider;
    mock.add_comment("pr-1", "nice work").await.unwrap();
    mock.get_pr_url("pr-1").await.unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 2);
    match &calls[0] {
        GitProviderCall::AddComment { pr_id, body } => {
            assert_eq!(pr_id, "pr-1");
            assert_eq!(body, "nice work");
        }
        other => panic!("expected AddComment, got {other:?}"),
    }
    match &calls[1] {
        GitProviderCall::GetPrUrl(id) => assert_eq!(id, "pr-1"),
        other => panic!("expected GetPrUrl, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_git_provider_default_returns() {
    let mock = MockGitProvider::new();
    use bmad_bot::git_provider::GitProvider;

    let params = bmad_bot::git_provider::CreatePrParams {
        title: "t".into(),
        body: "b".into(),
        source_branch: "s".into(),
        target_branch: "m".into(),
    };
    let result = mock.create_pr(params).await.expect("default should succeed");
    assert_eq!(result.id, "mock-1");
}

// ---------------------------------------------------------------------------
// MockNotifier tests (Task 7.2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_notifier_captures_story_notifications() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        status: StoryStatus::Completed,
        pr_url: Some("https://pr/1".into()),
        reason: None,
    };

    use bmad_bot::notifier::Notifier;
    mock.notify_story(&notification).await.unwrap();

    let story_calls = mock.story_calls();
    assert_eq!(story_calls.len(), 1);
    assert_eq!(story_calls[0].story_key, "1-1-test");
}

#[tokio::test]
async fn test_mock_notifier_captures_run_summary() {
    let mock = MockNotifier::new();

    let summary = bmad_bot::notifier::RunSummary {
        stories: vec![],
        total_processed: 3,
        completed: 2,
        blocked: 1,
        errored: 0,
        fatal: false,
    };

    use bmad_bot::notifier::Notifier;
    mock.notify_run_summary(&summary).await.unwrap();

    let summary_calls = mock.summary_calls();
    assert_eq!(summary_calls.len(), 1);
    assert_eq!(summary_calls[0].total_processed, 3);
}

#[tokio::test]
async fn test_mock_notifier_all_calls() {
    let mock = MockNotifier::new();

    let notification = StoryNotification {
        story_id: "2.1".into(),
        story_key: "2-1-dep".into(),
        status: StoryStatus::Blocked,
        pr_url: None,
        reason: Some("blocked by X".into()),
    };

    let summary = bmad_bot::notifier::RunSummary {
        stories: vec![],
        total_processed: 1,
        completed: 0,
        blocked: 1,
        errored: 0,
        fatal: false,
    };

    use bmad_bot::notifier::Notifier;
    mock.notify_story(&notification).await.unwrap();
    mock.notify_run_summary(&summary).await.unwrap();

    assert_eq!(mock.calls().len(), 2);
}

// ---------------------------------------------------------------------------
// MockSessionRunner tests (Task 7.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_session_runner_returns_configured_outcome() {
    let story = make_test_story("1-1-test", "test", vec![]);

    let mock = MockSessionRunner::new().with_outcome(SessionOutcome::Failed {
        story_key: "1-1-test".into(),
        error: "boom".into(),
        decisions: vec![],
    });

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Failed { error, .. } => assert_eq!(error, "boom"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_default_outcome() {
    let story = make_test_story("2-1-dep", "dep", vec![]);
    let mock = MockSessionRunner::new();

    let outcome = mock.run(&story).await;
    match outcome {
        SessionOutcome::Completed { story_key, .. } => assert_eq!(story_key, "2-1-dep"),
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_session_runner_tracks_calls() {
    let story = make_test_story("3-1-sup", "sup", vec![]);
    let mock = MockSessionRunner::new();
    mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "3-1-sup");
}

#[tokio::test]
async fn test_mock_session_runner_wal_recovery_returns_none() {
    let mock = MockSessionRunner::new();
    assert!(mock.check_and_recover_wal().await.is_none());
}

// ---------------------------------------------------------------------------
// MockReviewRunner tests (Task 7.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_review_runner_returns_configured_outcome() {
    let story = make_test_story("1-1-test", "test", vec![]);

    let mock = MockReviewRunner::new().with_outcome(ReviewOutcome::Completed {
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        report: "LGTM".into(),
    });

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Completed { report, .. } => assert_eq!(report, "LGTM"),
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_default_outcome() {
    let story = make_test_story("2-1-dep", "dep", vec![]);
    let mock = MockReviewRunner::new();

    let outcome = mock.run(&story).await;
    match outcome {
        ReviewOutcome::Skipped { reason } => assert_eq!(reason, "mock default"),
        other => panic!("expected Skipped, got {other:?}"),
    }
}

#[tokio::test]
async fn test_mock_review_runner_tracks_calls() {
    let story = make_test_story("4-1-tools", "tools", vec![]);
    let mock = MockReviewRunner::new();
    mock.run(&story).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].story_key, "4-1-tools");
}

// ---------------------------------------------------------------------------
// Send + Sync verification (Task 7.9)
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
