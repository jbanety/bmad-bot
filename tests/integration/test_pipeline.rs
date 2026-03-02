//! Integration tests for `StoryPipeline.process_story()` with mocked dependencies.
//!
//! Each test constructs a pipeline via `PipelineTestBuilder`, calls `process_story()`
//! (or `process_eligible_stories()`), and asserts on the result plus mock captures.
//!
//! Tests that exercise `Completed` or `Failed` code paths require a real git repo
//! (push_branch runs `git push` as a subprocess). Use `PipelineTestBuilder::new_with_git()`
//! with the relevant story branches.

use bmad_bot::git_provider::{GitProviderError, PrInfo};
use bmad_bot::notifier::StoryStatus;
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::escalation::EscalationReport;
use bmad_bot::session::SessionOutcome;

use crate::helpers::fixtures::{make_test_story, PipelineTestBuilder};
use crate::helpers::mocks::{MockGitProvider, MockNotifier};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Standard test story used across most tests.
fn test_story() -> bmad_bot::watcher::StoryInfo {
    make_test_story("4-1-rig-tools", "rig-tools-implementation", vec![])
}

/// Build a `SessionOutcome::Completed` with standard fields.
fn completed_outcome() -> SessionOutcome {
    SessionOutcome::Completed {
        story_key: "4-1-rig-tools".to_string(),
        branch: "story/4-1-rig-tools".to_string(),
        decisions: vec![],
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    }
}

/// Build a `SessionOutcome::Failed` with the given error.
fn failed_outcome(error: &str) -> SessionOutcome {
    SessionOutcome::Failed {
        story_key: "4-1-rig-tools".to_string(),
        error: error.to_string(),
        decisions: vec![],
    }
}

/// Build a `SessionOutcome::Escalated` with standard fields.
fn escalated_outcome() -> SessionOutcome {
    SessionOutcome::Escalated {
        report: EscalationReport {
            story_key: "4-1-rig-tools".to_string(),
            question: "What database schema should I use?".to_string(),
            reason: "Not specified in architecture docs".to_string(),
            branch_name: "story/4-1-rig-tools".to_string(),
            partial_work_summary: "Created initial tool stubs".to_string(),
            escalated_at: "2026-02-08T19:00:00+00:00".to_string(),
        },
        decisions: vec![],
    }
}

/// Build a `ReviewOutcome::Completed` with standard report.
fn completed_review() -> ReviewOutcome {
    ReviewOutcome::Completed {
        story_key: "4-1-rig-tools".to_string(),
        branch: "story/4-1-rig-tools".to_string(),
        report: "LGTM — all tests pass, code follows patterns.".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Task 4: Happy-path test (AC #1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_happy_path_completed_with_review() {
    // Arrange — need real git repo for push_branch
    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".into(),
        url: "https://github.com/test/test/pull/42".into(),
        number: 42,
    }));

    let (pipeline, notifier, git, _tmp) =
        PipelineTestBuilder::new_with_git(&["story/4-1-rig-tools"])
            .with_session(completed_outcome())
            .with_review(completed_review())
            .with_git_provider(mock_git)
            .build();

    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — result
    assert_eq!(result.status, StoryStatus::Completed);
    assert_eq!(
        result.pr_url.as_deref(),
        Some("https://github.com/test/test/pull/42")
    );
    assert!(result.error_detail.is_none());
    assert_eq!(result.story_key, "4-1-rig-tools");

    // Assert — MockNotifier: 1 notification, correct story_key, story_id = "4.1", pr_url present
    let stories = notifier.story_calls();
    assert_eq!(stories.len(), 1);
    assert_eq!(stories[0].story_key, "4-1-rig-tools");
    assert_eq!(stories[0].story_id, "4.1");
    assert!(stories[0].pr_url.is_some());
    assert_eq!(stories[0].status, StoryStatus::Completed);

    // Assert — MockGitProvider: create_pr title starts with "feat(", add_comment body contains "LGTM"
    let pr_params = git.captured_create_pr_params();
    assert_eq!(pr_params.len(), 1);
    assert!(
        pr_params[0].title.starts_with("feat("),
        "PR title should start with 'feat(': {}",
        pr_params[0].title
    );

    let comments = git.captured_add_comment_calls();
    assert_eq!(comments.len(), 1);
    assert!(
        comments[0].1.contains("LGTM"),
        "Comment body should contain LGTM: {}",
        comments[0].1
    );
}

// ---------------------------------------------------------------------------
// Task 5: Session failure test (AC #2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_session_failure_creates_failure_pr() {
    // Arrange — need real git repo for push_branch (Failed path also pushes)
    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "99".into(),
        url: "https://github.com/test/test/pull/99".into(),
        number: 99,
    }));

    let (pipeline, notifier, git, _tmp) =
        PipelineTestBuilder::new_with_git(&["story/4-1-rig-tools"])
            .with_session(failed_outcome("LLM timeout"))
            .with_git_provider(mock_git)
            .build();

    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — result
    assert_eq!(result.status, StoryStatus::Error);
    assert!(
        result
            .error_detail
            .as_ref()
            .unwrap()
            .contains("LLM timeout"),
        "error_detail should contain 'LLM timeout': {:?}",
        result.error_detail
    );

    // Assert — MockGitProvider: create_pr title contains [NEEDS REVIEW]
    let pr_params = git.captured_create_pr_params();
    assert_eq!(pr_params.len(), 1);
    assert!(
        pr_params[0].title.contains("[NEEDS REVIEW]"),
        "PR title should contain '[NEEDS REVIEW]': {}",
        pr_params[0].title
    );

    // Assert — MockNotifier: notification with StoryStatus::Error
    let stories = notifier.story_calls();
    assert_eq!(stories.len(), 1);
    assert_eq!(stories[0].status, StoryStatus::Error);
}

// ---------------------------------------------------------------------------
// Task 6: Escalation test (AC #3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_escalation_blocks_with_escalation_pr() {
    // Arrange — escalation also pushes and creates a PR (actual code behavior)
    let (pipeline, notifier, git, _tmp) =
        PipelineTestBuilder::new_with_git(&["story/4-1-rig-tools"])
            .with_session(escalated_outcome())
            .build();

    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — result: Blocked status
    assert_eq!(result.status, StoryStatus::Blocked);
    assert!(
        result
            .error_detail
            .as_ref()
            .unwrap()
            .contains("Escalated"),
        "error_detail should contain 'Escalated': {:?}",
        result.error_detail
    );

    // Assert — PR is created for escalation (actual code behavior: escalation creates PR)
    assert_eq!(git.create_pr_call_count(), 1);

    // Assert — MockNotifier: notification with StoryStatus::Blocked
    let stories = notifier.story_calls();
    assert_eq!(stories.len(), 1);
    assert_eq!(stories[0].status, StoryStatus::Blocked);
}

// ---------------------------------------------------------------------------
// Task 7: Review-disabled test (AC #4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_review_disabled_skips_review() {
    // Arrange
    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "50".into(),
        url: "https://github.com/test/test/pull/50".into(),
        number: 50,
    }));

    let (pipeline, _notifier, git, _tmp) =
        PipelineTestBuilder::new_with_git(&["story/4-1-rig-tools"])
            .with_code_review(false)
            .with_session(completed_outcome())
            .with_git_provider(mock_git)
            // No review outcome set — MockCodeReviewer::never_called() is used
            .build();

    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — pipeline still Completed
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some());

    // Assert — MockGitProvider: add_comment NOT called (no review report to post)
    assert_eq!(git.add_comment_call_count(), 0);
}

// ---------------------------------------------------------------------------
// Task 8: PR creation failure test (AC #5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_pr_creation_failure_returns_error() {
    // Arrange
    let mock_git = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "Internal server error".to_string(),
    }));

    let (pipeline, notifier, _git, _tmp) =
        PipelineTestBuilder::new_with_git(&["story/4-1-rig-tools"])
            .with_session(completed_outcome())
            .with_git_provider(mock_git)
            .build();

    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — pr_url None, error about PR creation
    assert!(result.pr_url.is_none());
    assert_eq!(result.status, StoryStatus::Error);
    assert!(
        result.error_detail.as_ref().unwrap().contains("PR creation"),
        "error_detail should mention PR creation: {:?}",
        result.error_detail
    );

    // Assert — MockNotifier still receives a notification (best-effort)
    assert_eq!(notifier.story_notification_count(), 1);
}

// ---------------------------------------------------------------------------
// Task 9: Review failure continues test (AC #6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_review_failure_still_completes() {
    // Arrange
    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "60".into(),
        url: "https://github.com/test/test/pull/60".into(),
        number: 60,
    }));

    let (pipeline, _notifier, git, _tmp) =
        PipelineTestBuilder::new_with_git(&["story/4-1-rig-tools"])
            .with_session(completed_outcome())
            .with_review(ReviewOutcome::Failed {
                story_key: "4-1-rig-tools".to_string(),
                error: "Review agent crashed".to_string(),
            })
            .with_git_provider(mock_git)
            .build();

    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — pipeline still Completed
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some());

    // Assert — create_pr was called, add_comment NOT called (no report)
    assert_eq!(git.create_pr_call_count(), 1);
    assert_eq!(git.add_comment_call_count(), 0);
}

// ---------------------------------------------------------------------------
// Task 10: Notification failure non-blocking test (AC #7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_notification_failure_non_blocking() {
    // Arrange
    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "70".into(),
        url: "https://github.com/test/test/pull/70".into(),
        number: 70,
    }));

    let (pipeline, _notifier, _git, _tmp) =
        PipelineTestBuilder::new_with_git(&["story/4-1-rig-tools"])
            .with_code_review(false)
            .with_session(completed_outcome())
            .with_notifier(MockNotifier::failing("test notification error"))
            .with_git_provider(mock_git)
            .build();

    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — pipeline still Completed with pr_url despite notification failure
    assert_eq!(result.status, StoryStatus::Completed);
    assert_eq!(
        result.pr_url.as_deref(),
        Some("https://github.com/test/test/pull/70")
    );
}

// ---------------------------------------------------------------------------
// Task 11: process_eligible_stories batch test (supplementary)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_process_eligible_stories_batch() {
    // Arrange — 3 stories with different outcomes
    // Note: Failed story needs a branch "story/4-2-rig-agents" but the push may fail
    // if the branch doesn't exist. The pipeline handles push failures gracefully.
    let outcomes = vec![
        SessionOutcome::Completed {
            story_key: "4-1-rig-tools".to_string(),
            branch: "story/4-1-rig-tools".to_string(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        },
        SessionOutcome::Failed {
            story_key: "4-2-rig-agents".to_string(),
            error: "Compile error".to_string(),
            decisions: vec![],
        },
        SessionOutcome::Completed {
            story_key: "4-3-rig-deploy".to_string(),
            branch: "story/4-3-rig-deploy".to_string(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        },
    ];

    let (pipeline, notifier, _git, _tmp) = PipelineTestBuilder::new_with_git(&[
        "story/4-1-rig-tools",
        "story/4-2-rig-agents",
        "story/4-3-rig-deploy",
    ])
    .with_code_review(false)
    .with_sessions(outcomes)
    .build();

    let stories = vec![
        make_test_story("4-1-rig-tools", "rig-tools", vec![]),
        make_test_story("4-2-rig-agents", "rig-agents", vec![]),
        make_test_story("4-3-rig-deploy", "rig-deploy", vec![]),
    ];

    // Act
    let summary = pipeline.process_eligible_stories(stories).await;

    // Assert — RunSummary totals
    assert_eq!(summary.total_processed, 3);
    assert_eq!(summary.completed, 2);
    assert_eq!(summary.errored, 1);
    assert_eq!(summary.blocked, 0);

    // Assert — MockNotifier: 3 notify_story + 1 notify_run_summary
    assert_eq!(notifier.story_notification_count(), 3);
    assert_eq!(notifier.run_summary_count(), 1);
}
