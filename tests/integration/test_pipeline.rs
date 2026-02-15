//! Integration tests for `StoryPipeline.process_story()` and
//! `StoryPipeline.process_eligible_stories()`.
//!
//! Each test builds its own pipeline via `PipelineTestBuilder` — no shared
//! mutable state. Git push requires a local bare remote, set up by
//! `create_test_repo_with_remote`.

use bmad_bot::git_provider::{GitProviderError, PrInfo};
use bmad_bot::notifier::{NotifierError, StoryStatus};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::escalation::EscalationReport;
use bmad_bot::session::SessionOutcome;

use crate::helpers::fixtures::{
    create_test_repo_with_remote, make_test_config, make_test_story, PipelineTestBuilder,
};
use crate::helpers::mocks::{MockGitProvider, MockNotifier};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a completed session outcome for the given story key/branch.
fn completed_outcome(story_key: &str, branch: &str) -> SessionOutcome {
    SessionOutcome::Completed {
        story_key: story_key.to_string(),
        branch: branch.to_string(),
        decisions: vec![],
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    }
}

/// Build a failed session outcome.
fn failed_outcome(story_key: &str, error: &str) -> SessionOutcome {
    SessionOutcome::Failed {
        story_key: story_key.to_string(),
        error: error.to_string(),
        decisions: vec![],
    }
}

/// Build an escalated session outcome.
fn escalated_outcome(story_key: &str) -> SessionOutcome {
    SessionOutcome::Escalated {
        report: EscalationReport {
            story_key: story_key.to_string(),
            question: "What database schema should I use?".to_string(),
            reason: "Not specified in architecture docs".to_string(),
            branch_name: format!("story/{story_key}"),
            partial_work_summary: "Created initial tool stubs".to_string(),
            escalated_at: "2026-02-08T19:00:00+00:00".to_string(),
        },
        decisions: vec![],
    }
}

/// Create a temp dir with a git repo + local remote, returning the project root path
/// and a `BotConfig` pointing to it.
fn setup_git_env(story_branch: &str) -> (tempfile::TempDir, bmad_bot::config::BotConfig) {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    create_test_repo_with_remote(&repo_dir, story_branch);

    let mut config = make_test_config(&repo_dir);
    // Ensure the implementation_artifacts subdir exists (for sprint-status path)
    let impl_dir = repo_dir.join("implementation-artifacts");
    std::fs::create_dir_all(&impl_dir).unwrap();
    config.bmad_paths.implementation_artifacts = impl_dir.display().to_string();
    (tmp, config)
}

// ===========================================================================
// AC #1 — Happy Path: Completed session → PR → review → notification
// ===========================================================================

#[tokio::test]
async fn test_pipeline_happy_path_completed() {
    let story_key = "4-1-rig-tools";
    let branch = "story/4-1-rig-tools";
    let (_tmp, config) = setup_git_env(branch);

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".to_string(),
        url: "https://github.com/test/test/pull/42".to_string(),
        number: 42,
    }));

    let (pipeline, notifier, git) = PipelineTestBuilder::new()
        .with_code_review(true)
        .with_session(completed_outcome(story_key, branch))
        .with_review(ReviewOutcome::Completed {
            story_key: story_key.to_string(),
            branch: branch.to_string(),
            report: "LGTM — all tests pass, code follows patterns.".to_string(),
        })
        .with_git_provider(mock_git)
        .build_with_config(config);

    let story = make_test_story(story_key, "rig-tools-implementation", vec![]);
    let result = pipeline.process_story(&story).await;

    // Assert PipelineResult
    assert_eq!(result.status, StoryStatus::Completed);
    assert_eq!(
        result.pr_url,
        Some("https://github.com/test/test/pull/42".to_string())
    );
    assert!(result.error_detail.is_none());
    assert!(!result.fatal);

    // Assert MockGitProvider: create_pr was called with title starting with "feat("
    let pr_params = git.captured_create_pr_params();
    assert_eq!(pr_params.len(), 1);
    assert!(
        pr_params[0].title.starts_with("feat("),
        "PR title should start with 'feat(': {}",
        pr_params[0].title
    );

    // Assert add_comment was called with review report
    let comments = git.captured_add_comment_calls();
    assert_eq!(comments.len(), 1);
    assert!(
        comments[0].1.contains("LGTM"),
        "Comment body should contain review report"
    );

    // Assert MockNotifier: exactly 1 story notification
    let story_notifs = notifier.story_calls();
    assert_eq!(story_notifs.len(), 1);
    assert_eq!(story_notifs[0].story_key, story_key);
    assert_eq!(story_notifs[0].story_id, "4.1");
    assert_eq!(story_notifs[0].status, StoryStatus::Completed);
    assert!(story_notifs[0].pr_url.is_some());
}

// ===========================================================================
// AC #2 — Session failure → partial work PR with [NEEDS REVIEW]
// ===========================================================================

#[tokio::test]
async fn test_pipeline_session_failure_creates_partial_pr() {
    let story_key = "4-1-rig-tools";
    let branch = &format!("story/{story_key}");
    let (_tmp, config) = setup_git_env(branch);

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "99".to_string(),
        url: "https://github.com/test/test/pull/99".to_string(),
        number: 99,
    }));

    let (pipeline, notifier, git) = PipelineTestBuilder::new()
        .with_session(failed_outcome(story_key, "LLM timeout"))
        .with_git_provider(mock_git)
        .build_with_config(config);

    let story = make_test_story(story_key, "rig-tools-implementation", vec![]);
    let result = pipeline.process_story(&story).await;

    // Assert status and error detail
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

    // Assert PR title contains [NEEDS REVIEW]
    let pr_params = git.captured_create_pr_params();
    assert_eq!(pr_params.len(), 1);
    assert!(
        pr_params[0].title.contains("[NEEDS REVIEW]"),
        "PR title should contain [NEEDS REVIEW]: {}",
        pr_params[0].title
    );

    // Assert notification with Error status
    let story_notifs = notifier.story_calls();
    assert_eq!(story_notifs.len(), 1);
    assert_eq!(story_notifs[0].status, StoryStatus::Error);
}

// ===========================================================================
// AC #3 — Escalation → Blocked, PR is created (actual behavior)
// ===========================================================================

#[tokio::test]
async fn test_pipeline_escalation_creates_pr_and_blocks() {
    let story_key = "4-1-rig-tools";
    let branch = &format!("story/{story_key}");
    let (_tmp, config) = setup_git_env(branch);

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "77".to_string(),
        url: "https://github.com/test/test/pull/77".to_string(),
        number: 77,
    }));

    let (pipeline, notifier, git) = PipelineTestBuilder::new()
        .with_session(escalated_outcome(story_key))
        .with_git_provider(mock_git)
        .build_with_config(config);

    let story = make_test_story(story_key, "rig-tools-implementation", vec![]);
    let result = pipeline.process_story(&story).await;

    // Assert Blocked status
    assert_eq!(result.status, StoryStatus::Blocked);
    // Escalation DOES create a PR in the actual implementation
    assert!(
        result.pr_url.is_some(),
        "Escalation creates a PR in current codebase"
    );
    assert!(
        result
            .error_detail
            .as_ref()
            .unwrap()
            .contains("Escalated"),
        "error_detail should contain 'Escalated'"
    );

    // Assert create_pr WAS called (escalation creates a WIP PR)
    assert_eq!(
        git.create_pr_call_count(),
        1,
        "Escalation should create one PR"
    );

    // Assert notification with Blocked status
    let story_notifs = notifier.story_calls();
    assert_eq!(story_notifs.len(), 1);
    assert_eq!(story_notifs[0].status, StoryStatus::Blocked);
}

// ===========================================================================
// AC #4 — Code review disabled → no review, PR still created
// ===========================================================================

#[tokio::test]
async fn test_pipeline_review_disabled_skips_review() {
    let story_key = "4-1-rig-tools";
    let branch = "story/4-1-rig-tools";
    let (_tmp, config) = setup_git_env(branch);

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "10".to_string(),
        url: "https://github.com/test/test/pull/10".to_string(),
        number: 10,
    }));

    let (pipeline, notifier, git) = PipelineTestBuilder::new()
        .with_code_review(false)
        .with_session(completed_outcome(story_key, branch))
        // Note: no .with_review() — MockCodeReviewer::never_called() is default
        .with_git_provider(mock_git)
        .build_with_config(config);

    let story = make_test_story(story_key, "rig-tools-implementation", vec![]);
    let result = pipeline.process_story(&story).await;

    // Assert Completed
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some());

    // Assert add_comment NOT called (no review report)
    assert_eq!(
        git.add_comment_call_count(),
        0,
        "No review means no add_comment"
    );

    // PR was created
    assert_eq!(git.create_pr_call_count(), 1);

    // Notification sent
    assert_eq!(notifier.story_notification_count(), 1);
}

// ===========================================================================
// AC #5 — PR creation failure → pr_url None, review not called
// ===========================================================================

#[tokio::test]
async fn test_pipeline_pr_creation_failure() {
    let story_key = "4-1-rig-tools";
    let branch = "story/4-1-rig-tools";
    let (_tmp, config) = setup_git_env(branch);

    let mock_git = MockGitProvider::new().with_create_pr(Err(GitProviderError::ApiError {
        status: 500,
        message: "Internal Server Error".to_string(),
    }));

    let (pipeline, notifier, git) = PipelineTestBuilder::new()
        .with_session(completed_outcome(story_key, branch))
        .with_git_provider(mock_git)
        .build_with_config(config);

    let story = make_test_story(story_key, "rig-tools-implementation", vec![]);
    let result = pipeline.process_story(&story).await;

    // Assert Error with no PR url
    assert_eq!(result.status, StoryStatus::Error);
    assert!(result.pr_url.is_none());
    assert!(result.error_detail.as_ref().unwrap().contains("PR creation failed"));

    // Review NOT called — PR failure short-circuits before review
    assert_eq!(
        git.add_comment_call_count(),
        0,
        "No PR means no review comment"
    );

    // Notification still sent (non-blocking)
    assert_eq!(
        notifier.story_notification_count(),
        1,
        "Notification is best-effort, always sent"
    );
}

// ===========================================================================
// AC #6 — Review failure → PR still exists, no comment posted
// ===========================================================================

#[tokio::test]
async fn test_pipeline_review_failure_still_completes() {
    let story_key = "4-1-rig-tools";
    let branch = "story/4-1-rig-tools";
    let (_tmp, config) = setup_git_env(branch);

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "55".to_string(),
        url: "https://github.com/test/test/pull/55".to_string(),
        number: 55,
    }));

    let (pipeline, _notifier, git) = PipelineTestBuilder::new()
        .with_code_review(true)
        .with_session(completed_outcome(story_key, branch))
        .with_review(ReviewOutcome::Failed {
            story_key: story_key.to_string(),
            error: "Review agent crashed".to_string(),
        })
        .with_git_provider(mock_git)
        .build_with_config(config);

    let story = make_test_story(story_key, "rig-tools-implementation", vec![]);
    let result = pipeline.process_story(&story).await;

    // Assert Completed despite review failure
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some());

    // PR was created
    assert_eq!(git.create_pr_call_count(), 1);

    // No review comment posted (review failed, no report)
    assert_eq!(
        git.add_comment_call_count(),
        0,
        "Failed review produces no report to post"
    );
}

// ===========================================================================
// AC #7 — Notification failure is non-blocking
// ===========================================================================

#[tokio::test]
async fn test_pipeline_notification_failure_non_blocking() {
    let story_key = "4-1-rig-tools";
    let branch = "story/4-1-rig-tools";
    let (_tmp, config) = setup_git_env(branch);

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "88".to_string(),
        url: "https://github.com/test/test/pull/88".to_string(),
        number: 88,
    }));

    let mock_notifier = MockNotifier::new().with_story_error(NotifierError::HttpRequest {
        reason: "test error — network down".to_string(),
    });

    let (pipeline, _notifier, _git) = PipelineTestBuilder::new()
        .with_code_review(false)
        .with_session(completed_outcome(story_key, branch))
        .with_git_provider(mock_git)
        .with_notifier(mock_notifier)
        .build_with_config(config);

    let story = make_test_story(story_key, "rig-tools-implementation", vec![]);
    let result = pipeline.process_story(&story).await;

    // Pipeline still Completed — notification failure is swallowed
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some());
}

// ===========================================================================
// Supplementary — process_eligible_stories batch test
// ===========================================================================

#[tokio::test]
async fn test_pipeline_process_eligible_stories_batch() {
    let (_tmp, config) = setup_git_env("story/4-1-rig-tools");

    // Create additional branches for stories 2 and 3
    {
        use std::process::Command;
        let repo_dir = std::path::Path::new(&config.bmad_paths.project_root);
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(repo_dir)
                .output()
                .expect("git command failed");
            assert!(
                output.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        };
        // Create branches for stories 2 and 3
        run(&["checkout", "-b", "story/4-2-agent-session"]);
        run(&["commit", "--allow-empty", "-m", "story 2 work"]);
        run(&["checkout", "-b", "story/4-3-pre-dev-prep"]);
        run(&["commit", "--allow-empty", "-m", "story 3 work"]);
    }

    let outcomes = vec![
        completed_outcome("4-1-rig-tools", "story/4-1-rig-tools"),
        failed_outcome("4-2-agent-session", "timeout"),
        completed_outcome("4-3-pre-dev-prep", "story/4-3-pre-dev-prep"),
    ];

    let (pipeline, notifier, _git) = PipelineTestBuilder::new()
        .with_code_review(false)
        .with_sessions(outcomes)
        .with_git_provider(MockGitProvider::new())
        .build_with_config(config);

    let stories = vec![
        make_test_story("4-1-rig-tools", "rig-tools-implementation", vec![]),
        make_test_story("4-2-agent-session", "agent-session-setup", vec![]),
        make_test_story("4-3-pre-dev-prep", "pre-dev-preparation", vec![]),
    ];

    let summary = pipeline.process_eligible_stories(stories).await;

    // Assert RunSummary totals
    assert_eq!(summary.total_processed, 3);
    // completed count: 4-1 completes, 4-2 errors, 4-3 completes = 2 completed
    assert_eq!(summary.completed, 2);
    assert_eq!(summary.errored, 1);
    assert_eq!(summary.blocked, 0);

    // Assert MockNotifier: 3 story notifications + 1 run summary
    assert_eq!(
        notifier.story_notification_count(),
        3,
        "One notification per story"
    );
    assert_eq!(
        notifier.run_summary_count(),
        1,
        "One run summary after all stories"
    );
}
