//! Integration tests for Git Provider & PR Creation.
//!
//! Validates the public API surface of `bmad_bot::git_provider` from an
//! external crate perspective, plus cross-module integration with
//! `bmad_bot::supervisor::decisions`.
//!
//! **NOT duplicating** the 35+ unit tests in `src/git_provider/mod.rs`,
//! `github.rs`, and `gitlab.rs`. These tests exercise:
//! 1. Public API smoke tests (factory, constructors) from integration crate
//! 2. Cross-module boundary: supervisor decisions → git_provider PR descriptions
//! 3. Factory → trait dispatch → method execution chain

use bmad_bot::config::GitProviderConfig;
use bmad_bot::git_provider::{
    build_pr_description, build_pr_title, create_provider, GitLabProvider,
    GitProvider, GitProviderError, PrDescriptionParams,
};
use bmad_bot::supervisor::decisions::{format_pr_decisions_section, DecisionRecord, DecisionSource};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Install rustls crypto provider for GitHub octocrab client construction.
/// Safe to call multiple times — returns Err if already installed, which we ignore.
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Build a minimal `GitProviderConfig` for testing.
fn test_config(provider: &str) -> GitProviderConfig {
    GitProviderConfig {
        provider: provider.to_string(),
        repo_owner: "test-owner".to_string(),
        repo_name: "test-repo".to_string(),
        target_branch: "main".to_string(),
    }
}

/// Build a `PrDescriptionParams` with sensible defaults for testing.
fn test_pr_description_params(
    story_key: &str,
    title: &str,
    outcome: &str,
    decisions: &str,
    failure: Option<&str>,
) -> PrDescriptionParams {
    PrDescriptionParams {
        story_key: story_key.to_string(),
        story_title: title.to_string(),
        outcome_summary: outcome.to_string(),
        decisions_section: decisions.to_string(),
        failure_details: failure.map(|s| s.to_string()),
        pr_summary: None,
    }
}

// ===========================================================================
// Task 2: Provider factory integration tests (AC #1, #2, #3)
// ===========================================================================

#[tokio::test]
async fn test_git_provider_factory_github_returns_ok() {
    install_crypto_provider();
    let config = test_config("github");
    let result = create_provider(&config, "ghp_test_token_12345");
    assert!(result.is_ok(), "create_provider(github) should succeed");
}

#[test]
fn test_git_provider_factory_gitlab_returns_ok() {
    let config = test_config("gitlab");
    let result = create_provider(&config, "glpat-test-token-12345");
    assert!(result.is_ok(), "create_provider(gitlab) should succeed");
}

#[test]
fn test_git_provider_factory_unsupported_provider_returns_not_configured() {
    let config = test_config("bitbucket");
    let result = create_provider(&config, "some-token");
    match result {
        Err(GitProviderError::ProviderNotConfigured { provider }) => {
            assert_eq!(provider, "bitbucket");
        }
        Err(other) => panic!("Expected ProviderNotConfigured, got: {other:?}"),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

#[test]
fn test_git_provider_factory_empty_provider_returns_not_configured() {
    let config = test_config("");
    let result = create_provider(&config, "some-token");
    match result {
        Err(GitProviderError::ProviderNotConfigured { provider }) => {
            assert_eq!(provider, "");
        }
        Err(other) => panic!("Expected ProviderNotConfigured, got: {other:?}"),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

// ===========================================================================
// Task 3: GitLab empty token rejection (AC #4)
// ===========================================================================

#[test]
fn test_git_provider_gitlab_empty_token_rejected() {
    let config = test_config("gitlab");
    let result = GitLabProvider::new(&config, "");
    match result {
        Err(GitProviderError::AuthenticationFailed { reason }) => {
            assert!(
                reason.to_lowercase().contains("empty"),
                "Reason should mention 'empty', got: {reason}"
            );
        }
        Err(other) => panic!("Expected AuthenticationFailed, got: {other:?}"),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

// ===========================================================================
// Task 4: Cross-module PR description integration tests (AC #5)
// ===========================================================================

#[test]
fn test_git_provider_pr_description_with_real_decisions() {
    // Build real DecisionRecord instances (cross-module: supervisor → git_provider)
    let decisions = vec![
        DecisionRecord::new(
            "Should we use async or sync for file I/O?".to_string(),
            Some("Performance-sensitive path".to_string()),
            "Use async with tokio::fs".to_string(),
            DecisionSource::RuleEngine {
                rule_name: "io_pattern".to_string(),
            },
            "Async I/O aligns with the project's tokio runtime".to_string(),
            vec!["sync std::fs".to_string(), "blocking threadpool".to_string()],
        ),
        DecisionRecord::new(
            "Which error handling strategy?".to_string(),
            None,
            "Use thiserror for typed errors".to_string(),
            DecisionSource::LlmFallback,
            "thiserror is already used throughout the codebase".to_string(),
            vec!["anyhow".to_string()],
        ),
    ];

    // Cross-module call: supervisor::decisions → git_provider
    let decisions_section = format_pr_decisions_section(&decisions);

    let params = test_pr_description_params(
        "5-1-git-provider",
        "Git Provider Trait",
        "completed successfully",
        &decisions_section,
        None,
    );

    let description = build_pr_description(&params);

    // AC #5: Story key and title in header
    assert!(
        description.contains("📋 Story: 5-1-git-provider"),
        "Description should contain story key in header"
    );
    assert!(
        description.contains("Git Provider Trait"),
        "Description should contain story title"
    );

    // AC #5: Outcome summary
    assert!(
        description.contains("**Status:** completed successfully"),
        "Description should contain outcome summary"
    );

    // AC #5: Supervisor Decisions section with real decision content
    assert!(
        description.contains("Supervisor Decisions"),
        "Description should contain 'Supervisor Decisions' section"
    );
    assert!(
        description.contains("async or sync"),
        "Description should contain first decision question"
    );
    assert!(
        description.contains("error handling"),
        "Description should contain second decision question"
    );
    assert!(
        description.contains("Rule Engine"),
        "Description should contain Rule Engine source"
    );
    assert!(
        description.contains("LLM Fallback"),
        "Description should contain LLM Fallback source"
    );

    // No failure details for success case
    assert!(
        !description.contains("⚠️ Failure Details"),
        "Success description should not contain failure details"
    );
}

#[test]
fn test_git_provider_pr_title_success() {
    let title = build_pr_title("5-1-git-provider", "Git Provider Trait", false);
    assert_eq!(title, "feat(5-1-git-provider): Git Provider Trait");
}

// ===========================================================================
// Task 5: Failure PR description integration test (AC #6)
// ===========================================================================

#[test]
fn test_git_provider_pr_description_with_failure_details() {
    let decisions_section = format_pr_decisions_section(&[]);
    let params = test_pr_description_params(
        "2-1-polling",
        "Sprint Polling",
        "failed",
        &decisions_section,
        Some("LLM timeout after 3 retries"),
    );

    let description = build_pr_description(&params);

    // AC #6: Failure Details section
    assert!(
        description.contains("⚠️ Failure Details"),
        "Failed description should contain '⚠️ Failure Details' section"
    );
    assert!(
        description.contains("LLM timeout after 3 retries"),
        "Failed description should contain failure text"
    );
}

#[test]
fn test_git_provider_pr_title_failure() {
    let title = build_pr_title("2-1-polling", "Sprint Polling", true);
    assert_eq!(title, "wip(2-1-polling): Sprint Polling [NEEDS REVIEW]");
}

// ===========================================================================
// Task 6: Escalation PR description test (supplementary)
// ===========================================================================

#[test]
fn test_git_provider_pr_description_escalation() {
    let decisions = vec![DecisionRecord::new(
        "How should we handle the flaky CI?".to_string(),
        Some("CI has been failing intermittently".to_string()),
        String::new(), // empty answer = escalation
        DecisionSource::Escalation,
        "Neither rule engine nor LLM could determine the right approach".to_string(),
        vec![],
    )];

    let decisions_section = format_pr_decisions_section(&decisions);

    let escalation_details =
        "**Question:** How should we handle the flaky CI?\n\
         **Reason:** Neither rule engine nor LLM could determine the right approach\n\
         **Partial work:** Implemented polling loop but CI validation is incomplete";

    let params = test_pr_description_params(
        "3-3-escalation",
        "Human Escalation",
        "escalated — needs clarification",
        &decisions_section,
        Some(escalation_details),
    );

    let description = build_pr_description(&params);

    // Escalation fields present
    assert!(
        description.contains("⚠️ Failure Details"),
        "Escalation description should contain failure details section"
    );
    assert!(
        description.contains("How should we handle the flaky CI?"),
        "Escalation description should contain the question"
    );
    assert!(
        description.contains("Neither rule engine nor LLM"),
        "Escalation description should contain the reason"
    );
    assert!(
        description.contains("Partial work:"),
        "Escalation description should contain partial work summary"
    );

    // Escalation decision in decisions section
    assert!(
        description.contains("Escalation"),
        "Escalation description should show Escalation source"
    );
    assert!(
        description.contains("⚠️ Escalated"),
        "Empty answer should show as '⚠️ Escalated' in decisions table"
    );
}

// ===========================================================================
// Task 7: End-to-end factory → trait method chain test (supplementary)
// ===========================================================================

#[tokio::test]
async fn test_git_provider_factory_to_trait_dispatch_gitlab_get_pr_url() {
    let config = test_config("gitlab");
    let provider: Box<dyn GitProvider> =
        create_provider(&config, "glpat-test-token-12345").expect("factory should succeed");

    // Call trait method on boxed dynamic dispatch
    let url = provider
        .get_pr_url("42")
        .await
        .expect("get_pr_url should succeed for valid numeric ID");

    assert_eq!(
        url, "https://gitlab.com/test-owner/test-repo/-/merge_requests/42",
        "URL should match GitLab MR pattern"
    );
}

#[tokio::test]
async fn test_git_provider_factory_to_trait_dispatch_gitlab_invalid_pr_id() {
    let config = test_config("gitlab");
    let provider: Box<dyn GitProvider> =
        create_provider(&config, "glpat-test-token-12345").expect("factory should succeed");

    let result = provider.get_pr_url("not-a-number").await;
    assert!(result.is_err());
    match result.unwrap_err() {
        GitProviderError::InvalidPrId { pr_id } => {
            assert_eq!(pr_id, "not-a-number");
        }
        other => panic!("Expected InvalidPrId, got: {other:?}"),
    }
}

#[test]
fn test_git_provider_pr_description_contains_footer() {
    let decisions_section = format_pr_decisions_section(&[]);
    let params = test_pr_description_params(
        "1-1-scaffold",
        "Scaffolding",
        "completed",
        &decisions_section,
        None,
    );
    let description = build_pr_description(&params);
    assert!(
        description.contains("bmad-bot"),
        "Description should contain bmad-bot footer"
    );
}
