//! Integration tests for `StoryPipeline.process_story()` and `process_eligible_stories()`.
//!
//! Each test constructs a pipeline via `PipelineTestBuilder`, calls the async method,
//! and asserts on the returned `PipelineResult` plus mock capture buffers.

use bmad_bot::git_provider::GitProviderError;
use bmad_bot::notifier::StoryStatus;
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::escalation::EscalationReport;
use bmad_bot::session::SessionOutcome;

use crate::helpers::fixtures::{make_test_story, PipelineTestBuilder};
use crate::helpers::mocks::{MockGitProvider, MockNotifier};

// ---------------------------------------------------------------------------
// Helper: standard test story
// ---------------------------------------------------------------------------

fn test_story() -> bmad_bot::watcher::StoryInfo {
    make_test_story("4-1-rig-tools", "rig-tools-implementation", vec![])
}

fn completed_session() -> SessionOutcome {
    SessionOutcome::Completed {
        story_key: "4-1-rig-tools".to_string(),
        branch: "story/4-1-rig-tools".to_string(),
        decisions: vec![],
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    }
}

fn failed_session(error: &str) -> SessionOutcome {
    SessionOutcome::Failed {
        story_key: "4-1-rig-tools".to_string(),
        error: error.to_string(),
        decisions: vec![],
    }
}

fn escalated_session() -> SessionOutcome {
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
async fn test_pipeline_happy_path_completed() {
    // Arrange
    let (pipeline, notifier, git, _env) = PipelineTestBuilder::new()
        .with_session(completed_session())
        .with_review(completed_review())
        .build();
    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — PipelineResult
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some(), "pr_url should be present");
    assert!(result.error_detail.is_none(), "no error expected");

    // Assert — MockNotifier: 1 notification with correct fields
    let notifications = notifier.story_calls();
    assert_eq!(notifications.len(), 1, "exactly 1 story notification");
    assert_eq!(notifications[0].story_key, "4-1-rig-tools");
    assert_eq!(notifications[0].story_id, "4.1");
    assert!(notifications[0].pr_url.is_some());

    // Assert — MockGitProvider: create_pr title is feat({story_key}): ...
    let pr_params = git.captured_create_pr_params();
    assert_eq!(pr_params.len(), 1, "exactly 1 create_pr call");
    assert!(
        pr_params[0].title.starts_with("feat(4-1-rig-tools)"),
        "PR title should start with 'feat(4-1-rig-tools)' — got: {}",
        pr_params[0].title
    );

    // Assert — MockGitProvider: add_comment body contains review report
    let comments = git.captured_add_comment_calls();
    assert_eq!(comments.len(), 1, "exactly 1 add_comment call");
    assert!(
        comments[0].1.contains("LGTM"),
        "comment body should contain review report"
    );

    // Assert — ordering: create_pr happened BEFORE run_review (AC #1)
    let events = git.call_events();
    let create_pr_pos = events
        .iter()
        .position(|e| e == "create_pr")
        .expect("create_pr event not found");
    let run_review_pos = events
        .iter()
        .position(|e| e == "run_review")
        .expect("run_review event not found");
    assert!(
        create_pr_pos < run_review_pos,
        "create_pr must be called before run_review — positions: create_pr={create_pr_pos}, run_review={run_review_pos}"
    );
}

// ---------------------------------------------------------------------------
// Task 5: Session-failure test (AC #2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_session_failed_creates_failure_pr() {
    // Arrange
    let (pipeline, notifier, git, _env) = PipelineTestBuilder::new()
        .with_session(failed_session("LLM timeout"))
        .build();
    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — PipelineResult
    assert_eq!(result.status, StoryStatus::Error);
    assert!(
        result
            .error_detail
            .as_deref()
            .unwrap_or("")
            .contains("LLM timeout"),
        "error_detail should contain 'LLM timeout' — got: {:?}",
        result.error_detail
    );

    // Assert — MockGitProvider: failure PR title contains "[NEEDS REVIEW]"
    let pr_params = git.captured_create_pr_params();
    assert_eq!(pr_params.len(), 1, "failure PR should be created");
    assert!(
        pr_params[0].title.contains("[NEEDS REVIEW]"),
        "failure PR title should contain '[NEEDS REVIEW]' — got: {}",
        pr_params[0].title
    );

    // Assert — MockNotifier: notification with Error status
    let notifications = notifier.story_calls();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].status, StoryStatus::Error);
}

// ---------------------------------------------------------------------------
// Task 6: Escalation test (AC #3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_escalated_no_pr_blocked_status() {
    // Arrange — escalation now creates a PR (push + create_pr)
    let (pipeline, notifier, git, _env) = PipelineTestBuilder::new()
        .with_session(escalated_session())
        .build();
    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — PipelineResult
    assert_eq!(result.status, StoryStatus::Blocked);
    assert!(
        result
            .error_detail
            .as_deref()
            .unwrap_or("")
            .contains("Escalated"),
        "error_detail should contain 'Escalated' — got: {:?}",
        result.error_detail
    );

    // Assert — MockGitProvider: escalation creates a PR (push attempted, PR created)
    // The current codebase creates a PR for escalation (with wip title)
    let pr_params = git.captured_create_pr_params();
    assert_eq!(
        pr_params.len(),
        1,
        "escalation should create a PR in current codebase"
    );
    // Escalation PR uses wip({story_key}): ... [NEEDS REVIEW] title format (Task 6.2)
    assert!(
        pr_params[0].title.starts_with("wip(4-1-rig-tools)"),
        "escalation PR title should start with 'wip(4-1-rig-tools)' — got: {}",
        pr_params[0].title
    );
    assert!(
        pr_params[0].title.contains("[NEEDS REVIEW]"),
        "escalation PR title should contain '[NEEDS REVIEW]' — got: {}",
        pr_params[0].title
    );

    // Assert — MockNotifier: notification with Blocked status
    let notifications = notifier.story_calls();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].status, StoryStatus::Blocked);
}

// ---------------------------------------------------------------------------
// Task 7: Review-disabled test (AC #4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_review_disabled_skips_review() {
    // Arrange
    let (pipeline, notifier, git, _env) = PipelineTestBuilder::new()
        .with_code_review(false)
        .with_session(completed_session())
        // No review outcome — review should not be called
        .build();
    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — still Completed
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some());

    // Assert — no add_comment (no review report to post)
    assert_eq!(
        git.add_comment_call_count(),
        0,
        "no add_comment when review disabled"
    );

    // Assert — PR was created
    assert_eq!(git.create_pr_call_count(), 1, "PR should still be created");

    // Assert — MockCodeReviewer was NOT called (Task 7.2 / AC #4)
    let events = git.call_events();
    assert!(
        !events.contains(&"run_review".to_string()),
        "MockCodeReviewer.run_review should not be called when review is disabled"
    );
    let _ = notifier; // bind to suppress unused warning
}

// ---------------------------------------------------------------------------
// Task 8: PR-creation-failure test (AC #5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_pr_creation_failure() {
    // Arrange
    let failing_git = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "Internal server error".to_string(),
    }));

    let (pipeline, notifier, git, _env) = PipelineTestBuilder::new()
        .with_session(completed_session())
        .with_review(completed_review())
        .with_git_provider(failing_git)
        .build();
    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — PipelineResult
    assert_eq!(result.status, StoryStatus::Error);
    assert!(result.pr_url.is_none(), "pr_url should be None on PR failure");

    // Assert — MockNotifier still captured a notification
    assert_eq!(
        notifier.story_notification_count(),
        1,
        "notification should still be sent even when PR fails"
    );

    // Assert — no add_comment (no PR → no review → no comment)
    assert_eq!(git.add_comment_call_count(), 0);
}

// ---------------------------------------------------------------------------
// Task 9: Review-failure-continues test (AC #6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_review_failed_still_completes() {
    // Arrange
    let (pipeline, _notifier, git, _env) = PipelineTestBuilder::new()
        .with_session(completed_session())
        .with_review(ReviewOutcome::Failed {
            story_key: "4-1-rig-tools".to_string(),
            error: "Review agent crashed".to_string(),
        })
        .build();
    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — still Completed (review failure is non-blocking)
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some(), "PR should exist");

    // Assert — create_pr was called
    assert_eq!(git.create_pr_call_count(), 1);

    // Assert — add_comment NOT called (no review report to post)
    assert_eq!(
        git.add_comment_call_count(),
        0,
        "no add_comment when review fails"
    );
}

// ---------------------------------------------------------------------------
// Task 10: Notification-failure-non-blocking test (AC #7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_notification_failure_non_blocking() {
    // Arrange
    let (pipeline, _notifier, _git, _env) = PipelineTestBuilder::new()
        .with_session(completed_session())
        .with_review(completed_review())
        .with_notifier(MockNotifier::failing())
        .build();
    let story = test_story();

    // Act
    let result = pipeline.process_story(&story).await;

    // Assert — pipeline still Completed despite notification failure
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some());
}

// ---------------------------------------------------------------------------
// Task 11: process_eligible_stories batch test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_process_eligible_stories_batch() {
    // Arrange — 3 stories with different outcomes
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
            story_key: "4-2-rig-ui".to_string(),
            error: "timeout".to_string(),
            decisions: vec![],
        },
        SessionOutcome::Completed {
            story_key: "4-3-rig-api".to_string(),
            branch: "story/4-3-rig-api".to_string(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        },
    ];

    let (pipeline, notifier, _git, _env) = PipelineTestBuilder::new()
        .with_sessions(outcomes)
        .with_code_review(false) // simplify — skip review
        .build();

    let stories = vec![
        make_test_story("4-1-rig-tools", "rig-tools", vec![]),
        make_test_story("4-2-rig-ui", "rig-ui", vec![]),
        make_test_story("4-3-rig-api", "rig-api", vec![]),
    ];

    // Act
    let summary = pipeline.process_eligible_stories(stories).await;

    // Assert — RunSummary totals
    assert_eq!(summary.total_processed, 3);
    assert_eq!(summary.completed, 2);
    assert_eq!(summary.errored, 1);
    assert_eq!(summary.blocked, 0);

    // Assert — MockNotifier: 3 story notifications + 1 run summary
    assert_eq!(
        notifier.story_notification_count(),
        3,
        "should have 3 story notifications"
    );
    assert_eq!(
        notifier.run_summary_count(),
        1,
        "should have 1 run summary notification"
    );
}
