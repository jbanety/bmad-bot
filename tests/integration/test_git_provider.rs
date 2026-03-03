//! Integration tests for `bmad_bot::git_provider` — PR creation, factory,
//! and cross-module description building.
//!
//! Story 7.6 — verifies public API surface from external-crate perspective
//! and cross-module integration (supervisor::decisions → git_provider).

// --- git_provider public API ---
use bmad_bot::git_provider::{
    build_pr_description, build_pr_title, create_provider, GitLabProvider, GitProviderError,
    PrDescriptionParams, PrSummary,
};

// --- config ---
use bmad_bot::config::GitProviderConfig;

// --- supervisor::decisions cross-module ---
use bmad_bot::supervisor::decisions::{format_pr_decisions_section, DecisionRecord, DecisionSource};

/// Install rustls crypto provider for GitHub octocrab client construction.
/// Safe to call multiple times — returns Err if already installed, which we ignore.
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Helper: build a minimal `GitProviderConfig` for testing.
fn make_config(provider: &str) -> GitProviderConfig {
    GitProviderConfig {
        provider: provider.to_string(),
        repo_owner: "test-owner".to_string(),
        repo_name: "test-repo".to_string(),
        target_branch: "main".to_string(),
    }
}

// ============================================================================
// Task 2: Provider factory integration tests (AC #1, #2, #3)
// ============================================================================

/// AC #1 — create_provider with "github" returns Ok (smoke test from external crate).
/// Needs tokio runtime because GitHubProvider::new() internally builds an Octocrab client.
#[tokio::test]
async fn test_git_provider_factory_github_returns_ok() {
    install_crypto_provider();
    let config = make_config("github");
    let result = create_provider(&config, "ghp_fake_token_for_testing");
    assert!(result.is_ok(), "GitHub factory should succeed");
}

/// AC #2 — create_provider with "gitlab" returns Ok.
#[test]
fn test_git_provider_factory_gitlab_returns_ok() {
    let config = make_config("gitlab");
    let result = create_provider(&config, "glpat-fake-token");
    assert!(result.is_ok(), "GitLab factory should succeed");
}

/// AC #3 — create_provider with unsupported "bitbucket" returns ProviderNotConfigured.
#[test]
fn test_git_provider_factory_bitbucket_returns_not_configured() {
    let config = make_config("bitbucket");
    let result = create_provider(&config, "some-token");
    match result {
        Err(GitProviderError::ProviderNotConfigured { provider }) => {
            assert_eq!(provider, "bitbucket");
        }
        Err(e) => panic!("Expected ProviderNotConfigured, got error: {e}"),
        Ok(_) => panic!("Expected ProviderNotConfigured, got Ok"),
    }
}

/// AC #3 supplementary — create_provider with empty string provider → ProviderNotConfigured.
#[test]
fn test_git_provider_factory_empty_provider_returns_not_configured() {
    let config = make_config("");
    let result = create_provider(&config, "some-token");
    match result {
        Err(GitProviderError::ProviderNotConfigured { provider }) => {
            assert_eq!(provider, "");
        }
        Err(e) => panic!("Expected ProviderNotConfigured, got error: {e}"),
        Ok(_) => panic!("Expected ProviderNotConfigured, got Ok"),
    }
}

// ============================================================================
// Task 3: GitLab empty token rejection (AC #4)
// ============================================================================

/// AC #4 — GitLabProvider::new with empty token returns AuthenticationFailed.
#[test]
fn test_git_provider_gitlab_empty_token_returns_auth_failed() {
    let config = make_config("gitlab");
    let result = GitLabProvider::new(&config, "");
    match result {
        Err(GitProviderError::AuthenticationFailed { reason }) => {
            assert!(
                reason.to_lowercase().contains("empty"),
                "Reason should mention 'empty', got: {reason}"
            );
        }
        Err(e) => panic!("Expected AuthenticationFailed, got error: {e}"),
        Ok(_) => panic!("Expected AuthenticationFailed, got Ok"),
    }
}

// ============================================================================
// Task 4: Cross-module PR description integration tests (AC #5)
// ============================================================================

/// AC #5 — build_pr_description with real DecisionRecords from supervisor module.
#[test]
fn test_git_provider_pr_description_with_real_decisions() {
    // Arrange: build real DecisionRecord instances via supervisor::decisions
    let decisions = vec![
        DecisionRecord::new(
            "Should we use async or sync for the file watcher?".to_string(),
            Some("Performance considerations".to_string()),
            "Use async with tokio for better concurrency".to_string(),
            DecisionSource::RuleEngine {
                rule_name: "concurrency_pattern".to_string(),
            },
            "Async aligns with the existing tokio runtime".to_string(),
            vec!["Sync polling".to_string(), "OS-level inotify".to_string()],
        ),
        DecisionRecord::new(
            "What error handling strategy?".to_string(),
            None,
            "Use thiserror for typed errors".to_string(),
            DecisionSource::LlmFallback,
            "Consistent with existing codebase patterns".to_string(),
            vec![],
        ),
    ];

    // Cross-module call: supervisor::decisions → format for git_provider
    let decisions_section = format_pr_decisions_section(&decisions);

    let params = PrDescriptionParams {
        story_key: "5-1-git-provider".to_string(),
        story_title: "Git Provider Trait".to_string(),
        outcome_summary: "completed successfully".to_string(),
        decisions_section: decisions_section.clone(),
        failure_details: None,
        pr_summary: None,
    };

    // Act
    let description = build_pr_description(&params);

    // Assert — story key in header (exact H1 heading format generated by build_pr_description)
    assert!(
        description.contains("# 📋 Story:"),
        "Description should contain H1 story header"
    );
    assert!(
        description.contains("5-1-git-provider"),
        "Description should contain story key"
    );

    // Assert — outcome summary
    assert!(
        description.contains("**Status:**"),
        "Description should contain status"
    );
    assert!(
        description.contains("completed successfully"),
        "Description should contain outcome summary"
    );

    // Assert — Context section (always present; falls back to DEFAULT_CONTEXT when pr_summary is None)
    assert!(
        description.contains("## 📝 Context"),
        "Description should contain Context section"
    );

    // Assert — Supervisor Decisions section with actual decision content
    assert!(
        description.contains("Supervisor Decisions"),
        "Description should contain Supervisor Decisions section"
    );
    assert!(
        description.contains("async"),
        "Decisions should contain actual decision content about async"
    );
    assert!(
        description.contains("thiserror"),
        "Decisions should contain actual decision content about thiserror"
    );

    // Assert — How to Test section (always present)
    assert!(
        description.contains("## 🧪 How to test"),
        "Description should contain How to Test section"
    );

    // Assert — Additional Information section (always present)
    assert!(
        description.contains("## ℹ️ Additional information"),
        "Description should contain Additional Information section"
    );

    // Assert — bmad-bot footer
    assert!(
        description.contains("bmad-bot"),
        "Description should contain bmad-bot footer"
    );
}

/// AC #5 — build_pr_title for success case.
#[test]
fn test_git_provider_pr_title_success() {
    let title = build_pr_title("5-1-git-provider", "Git Provider Trait", false);
    assert_eq!(title, "feat(5-1-git-provider): Git Provider Trait");
}

// ============================================================================
// Task 5: Failure PR description integration test (AC #6)
// ============================================================================

/// AC #6 — build_pr_description with failure_details includes failure section.
#[test]
fn test_git_provider_pr_description_failure_includes_details() {
    let decisions_section = format_pr_decisions_section(&[]);

    let params = PrDescriptionParams {
        story_key: "2-1-polling".to_string(),
        story_title: "Sprint Polling".to_string(),
        outcome_summary: "failed".to_string(),
        decisions_section,
        failure_details: Some("LLM timeout after 3 retries".to_string()),
        pr_summary: None,
    };

    let description = build_pr_description(&params);

    // Assert — failure details section present
    assert!(
        description.contains("⚠️ Failure Details"),
        "Description should contain failure details section"
    );
    assert!(
        description.contains("LLM timeout after 3 retries"),
        "Description should contain the failure text"
    );
}

/// AC #6 — build_pr_title for failure case.
#[test]
fn test_git_provider_pr_title_failure() {
    let title = build_pr_title("2-1-polling", "Sprint Polling", true);
    assert_eq!(title, "wip(2-1-polling): Sprint Polling [NEEDS REVIEW]");
}

// ============================================================================
// Task 6: Escalation PR description test (supplementary)
// ============================================================================

/// Supplementary — escalation-style failure_details with question, reason, and partial work.
#[test]
fn test_git_provider_pr_description_escalation_includes_all_fields() {
    let decisions = vec![DecisionRecord::new(
        "How should we handle the database migration?".to_string(),
        Some("Schema changes required".to_string()),
        String::new(), // empty answer = escalation
        DecisionSource::Escalation,
        "Neither rule engine nor LLM could determine the correct migration strategy".to_string(),
        vec![
            "Auto-migrate".to_string(),
            "Manual SQL scripts".to_string(),
        ],
    )];

    let decisions_section = format_pr_decisions_section(&decisions);

    let escalation_details = "\
**Question:** How should the database migration be structured?\n\
**Reason:** Complex schema change requires human judgment on data preservation strategy.\n\
**Partial Work:** Created migration scaffold and rollback template. \
Tests for non-migration functionality are passing.";

    let params = PrDescriptionParams {
        story_key: "3-2-db-migration".to_string(),
        story_title: "Database Migration".to_string(),
        outcome_summary: "escalated — needs clarification".to_string(),
        decisions_section,
        failure_details: Some(escalation_details.to_string()),
        pr_summary: None,
    };

    let description = build_pr_description(&params);

    // Assert all escalation fields present
    assert!(
        description.contains("⚠️ Failure Details"),
        "Should have failure details section"
    );
    assert!(
        description.contains("Question:"),
        "Should contain question field"
    );
    assert!(
        description.contains("Reason:"),
        "Should contain reason field"
    );
    assert!(
        description.contains("Partial Work:"),
        "Should contain partial work field"
    );
    assert!(
        description.contains("Escalation"),
        "Decisions section should show escalation source"
    );
    assert!(
        description.contains("⚠️ Escalated"),
        "Escalated decision should show escalation marker"
    );
}

// ============================================================================
// Task 7: End-to-end factory → trait method chain test (supplementary)
// ============================================================================

/// Supplementary — factory → trait dispatch → get_pr_url for GitLab.
#[tokio::test]
async fn test_git_provider_factory_to_get_pr_url_gitlab() {
    let config = make_config("gitlab");
    let provider = create_provider(&config, "glpat-fake-token")
        .expect("GitLab factory should succeed");

    let url = provider
        .get_pr_url("42")
        .await
        .expect("get_pr_url should succeed for valid numeric ID");

    assert_eq!(
        url,
        "https://gitlab.com/test-owner/test-repo/-/merge_requests/42"
    );
}

/// Supplementary — get_pr_url with non-numeric ID returns InvalidPrId.
#[tokio::test]
async fn test_git_provider_get_pr_url_invalid_id_returns_error() {
    let config = make_config("gitlab");
    let provider = create_provider(&config, "glpat-fake-token")
        .expect("GitLab factory should succeed");

    let result = provider.get_pr_url("not-a-number").await;
    match result {
        Err(GitProviderError::InvalidPrId { pr_id }) => {
            assert_eq!(pr_id, "not-a-number");
        }
        other => panic!("Expected InvalidPrId, got: {other:?}"),
    }
}

// ============================================================================
// Additional: pr_summary field integration tests
// ============================================================================

/// PR description with pr_summary: None uses DEFAULT_CONTEXT, DEFAULT_HOW_TO_TEST,
/// and DEFAULT_ADDITIONAL_INFO fallback constants — verifies all 3 sections
/// are present in the output when no agent-generated summary is provided.
#[test]
fn test_git_provider_pr_description_default_sections_when_no_pr_summary() {
    let params = PrDescriptionParams {
        story_key: "6-1-notify".to_string(),
        story_title: "Notification System".to_string(),
        outcome_summary: "completed successfully".to_string(),
        decisions_section: format_pr_decisions_section(&[]),
        failure_details: None,
        pr_summary: None,
    };

    let description = build_pr_description(&params);

    // All 3 structural sections must be present when pr_summary is None
    assert!(
        description.contains("## \u{1f4dd} Context"),
        "Description should contain Context section (got: {description})"
    );
    assert!(
        description.contains("## \u{1f9ea} How to test"),
        "Description should contain How to Test section"
    );
    assert!(
        description.contains("## \u{2139}\u{fe0f} Additional information"),
        "Description should contain Additional Information section"
    );
    // Fallback constants must appear
    assert!(
        description.contains("No detailed context was captured"),
        "Should contain DEFAULT_CONTEXT fallback text"
    );
    assert!(
        description.contains("cargo test"),
        "Should contain DEFAULT_HOW_TO_TEST fallback text referencing cargo test"
    );
    assert!(
        description.contains("No additional information available"),
        "Should contain DEFAULT_ADDITIONAL_INFO fallback text"
    );
}

/// PR description with pr_summary: Some(...) uses the agent-generated content
/// instead of the fallback constants — verifies enriched PR description path.
#[test]
fn test_git_provider_pr_description_enriched_with_pr_summary() {
    let summary = PrSummary {
        context: "Implemented the notification system using tokio channels.".to_string(),
        how_to_test: "Run `cargo test --test integration` then check the notifier logs."
            .to_string(),
        additional_info: "Added `tokio-stream` dependency. No migrations required.".to_string(),
    };

    let params = PrDescriptionParams {
        story_key: "6-1-notify".to_string(),
        story_title: "Notification System".to_string(),
        outcome_summary: "completed successfully".to_string(),
        decisions_section: format_pr_decisions_section(&[]),
        failure_details: None,
        pr_summary: Some(summary),
    };

    let description = build_pr_description(&params);

    // Agent-generated content must replace fallbacks
    assert!(
        description.contains("tokio channels"),
        "Should contain agent-provided context"
    );
    assert!(
        description.contains("notifier logs"),
        "Should contain agent-provided how-to-test"
    );
    assert!(
        description.contains("tokio-stream"),
        "Should contain agent-provided additional info"
    );
    // Fallback constants must NOT appear
    assert!(
        !description.contains("No detailed context was captured"),
        "Should NOT contain DEFAULT_CONTEXT when pr_summary is Some"
    );
    assert!(
        !description.contains("No additional information available"),
        "Should NOT contain DEFAULT_ADDITIONAL_INFO when pr_summary is Some"
    );
}
