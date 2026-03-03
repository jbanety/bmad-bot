//! Integration tests for the `git_provider` module — Story 7.6.
//!
//! These tests verify the public API surface of `bmad_bot::git_provider` and
//! cross-module integration with `bmad_bot::supervisor::decisions`.
//!
//! **Integration value over unit tests:**
//! - External crate perspective: imports via `bmad_bot::*` — any visibility regression breaks immediately
//! - Cross-module boundary: real `DecisionRecord` → `format_pr_decisions_section()` → `build_pr_description()`
//! - Factory → trait dispatch chain: `create_provider()` → `Box<dyn GitProvider>` → trait methods
//! - Crypto provider initialization from external crate context

use bmad_bot::config::GitProviderConfig;
use bmad_bot::git_provider::{
    build_pr_description, build_pr_title, create_provider, GitLabProvider, GitProviderError,
    PrDescriptionParams,
};
use bmad_bot::supervisor::decisions::{format_pr_decisions_section, DecisionRecord, DecisionSource};

// ---------------------------------------------------------------------------
// Helper: crypto provider for GitHub (octocrab requires rustls)
// ---------------------------------------------------------------------------

/// Install rustls crypto provider for GitHub octocrab client construction.
/// Safe to call multiple times — returns Err if already installed, which we ignore.
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Build a `GitProviderConfig` for testing.
fn test_config(provider: &str) -> GitProviderConfig {
    GitProviderConfig {
        provider: provider.to_string(),
        repo_owner: "test-owner".to_string(),
        repo_name: "test-repo".to_string(),
        target_branch: "main".to_string(),
    }
}

// ===========================================================================
// Task 2: Provider factory integration tests (AC #1, #2, #3)
// ===========================================================================

/// AC #1: `create_provider()` with "github" + valid token → Ok(Box<dyn GitProvider>)
#[tokio::test]
async fn test_git_provider_factory_github_returns_ok() {
    install_crypto_provider();
    let config = test_config("github");
    let result = create_provider(&config, "ghp_valid_token_123");
    assert!(result.is_ok(), "GitHub factory should return Ok");
}

/// AC #2: `create_provider()` with "gitlab" + valid token → Ok(Box<dyn GitProvider>)
#[test]
fn test_git_provider_factory_gitlab_returns_ok() {
    let config = test_config("gitlab");
    let result = create_provider(&config, "glpat-valid-token-456");
    assert!(result.is_ok(), "GitLab factory should return Ok");
}

/// AC #3: `create_provider()` with unsupported provider → Err(ProviderNotConfigured)
#[test]
fn test_git_provider_factory_unsupported_provider_returns_not_configured() {
    // "bitbucket" — explicitly unsupported
    let config = test_config("bitbucket");
    let result = create_provider(&config, "some-token");
    assert!(result.is_err());
    match result {
        Err(GitProviderError::ProviderNotConfigured { ref provider }) => {
            assert_eq!(provider, "bitbucket");
        }
        _ => panic!("Expected Err(ProviderNotConfigured)"),
    }

    // Empty string provider — also unsupported
    let config = test_config("");
    let result = create_provider(&config, "some-token");
    assert!(result.is_err());
    match result {
        Err(GitProviderError::ProviderNotConfigured { .. }) => {}
        _ => panic!("Empty provider should return ProviderNotConfigured"),
    }
}

// ===========================================================================
// Task 3: GitLab empty token rejection (AC #4)
// ===========================================================================

/// AC #4: `GitLabProvider::new()` with empty token → Err(AuthenticationFailed)
#[test]
fn test_git_provider_gitlab_empty_token_returns_auth_failed() {
    let config = test_config("gitlab");
    let result = GitLabProvider::new(&config, "");
    assert!(result.is_err());
    match result {
        Err(GitProviderError::AuthenticationFailed { ref reason }) => {
            assert!(
                reason.contains("empty"),
                "Reason should mention 'empty', got: {reason}"
            );
        }
        _ => panic!("Expected Err(AuthenticationFailed)"),
    }
}

// ===========================================================================
// Task 4: Cross-module PR description integration tests (AC #5)
// ===========================================================================

/// AC #5: Real DecisionRecord → format_pr_decisions_section() → PrDescriptionParams → build_pr_description()
#[test]
fn test_git_provider_pr_description_with_real_decisions() {
    // Build real DecisionRecord instances (cross-module: supervisor → git_provider)
    let decisions = vec![
        DecisionRecord::new(
            "Should I proceed with the refactor?".to_string(),
            Some("Code is getting complex".to_string()),
            "Yes, proceed with the refactor".to_string(),
            DecisionSource::RuleEngine {
                rule_name: "confirmation_proceed".to_string(),
            },
            "Standard confirmation pattern matched".to_string(),
            vec!["Wait for review".to_string()],
        ),
        DecisionRecord::new(
            "Which database adapter should I use?".to_string(),
            None,
            "Use sqlx for async compatibility".to_string(),
            DecisionSource::LlmFallback,
            "Architecture docs recommend sqlx for async Rust".to_string(),
            vec![
                "diesel".to_string(),
                "sea-orm".to_string(),
            ],
        ),
    ];

    // Cross-module call: supervisor::decisions → git_provider
    let decisions_section = format_pr_decisions_section(&decisions);

    let params = PrDescriptionParams {
        story_key: "5-1-git-provider".to_string(),
        story_title: "Git Provider Trait".to_string(),
        outcome_summary: "completed successfully".to_string(),
        decisions_section: decisions_section.clone(),
        failure_details: None,
        pr_summary: None,
    };

    let description = build_pr_description(&params);

    // AC #5 assertions
    assert!(
        description.contains("# 📋 Story:"),
        "Description should contain h1 story header"
    );
    assert!(
        description.contains("5-1-git-provider"),
        "Description should contain story key"
    );
    assert!(
        description.contains("Git Provider Trait"),
        "Description should contain story title"
    );
    assert!(
        description.contains("**Status:** completed successfully"),
        "Description should contain outcome summary"
    );
    assert!(
        description.contains("Supervisor Decisions"),
        "Description should contain Supervisor Decisions section"
    );
    // Verify real decision content is present (not hardcoded strings)
    assert!(
        description.contains("confirmation_proceed")
            || description.contains("Rule Engine"),
        "Description should contain actual decision source"
    );
    assert!(
        description.contains("Should I proceed"),
        "Description should contain actual decision question"
    );
    assert!(
        description.contains("bmad-bot"),
        "Description should contain bmad-bot footer"
    );
}

/// AC #5: build_pr_title() for success case
#[test]
fn test_git_provider_pr_title_success() {
    let title = build_pr_title("5-1-git-provider", "Git Provider Trait", false);
    assert_eq!(title, "feat(5-1-git-provider): Git Provider Trait");
}

// ===========================================================================
// Task 5: Failure PR description integration test (AC #6)
// ===========================================================================

/// AC #6: build_pr_description() with failure_details → contains ⚠️ Failure Details section
#[test]
fn test_git_provider_pr_description_failure_includes_details() {
    let decisions_section =
        format_pr_decisions_section(&[]); // empty decisions for failure case

    let params = PrDescriptionParams {
        story_key: "2-1-polling".to_string(),
        story_title: "Sprint Polling".to_string(),
        outcome_summary: "failed".to_string(),
        decisions_section,
        failure_details: Some("LLM timeout after 3 retries".to_string()),
        pr_summary: None,
    };

    let description = build_pr_description(&params);

    assert!(
        description.contains("⚠️ Failure Details"),
        "Description should contain failure details section"
    );
    assert!(
        description.contains("LLM timeout after 3 retries"),
        "Description should contain the failure text"
    );
}

/// AC #6: build_pr_title() for failure case
#[test]
fn test_git_provider_pr_title_failure() {
    let title = build_pr_title("2-1-polling", "Sprint Polling", true);
    assert_eq!(title, "wip(2-1-polling): Sprint Polling [NEEDS REVIEW]");
}

// ===========================================================================
// Task 6: Escalation PR description test (supplementary)
// ===========================================================================

/// Escalation-style failure details with question, reason, and partial work summary
#[test]
fn test_git_provider_pr_description_escalation_includes_fields() {
    let escalation_details = "\
**Question:** How should the authentication flow handle expired tokens?\n\
**Reason:** No matching rule and LLM supervisor could not determine answer from project docs\n\
**Partial Work:** Implemented token refresh skeleton, missing retry logic";

    let decisions = vec![DecisionRecord::new(
        "How should the authentication flow handle expired tokens?".to_string(),
        None,
        String::new(), // empty answer = escalation
        DecisionSource::Escalation,
        "Neither rule engine nor LLM could answer".to_string(),
        vec![],
    )];

    let decisions_section = format_pr_decisions_section(&decisions);

    let params = PrDescriptionParams {
        story_key: "4-2-agent-session".to_string(),
        story_title: "Agent Session Setup".to_string(),
        outcome_summary: "escalated — needs clarification".to_string(),
        decisions_section,
        failure_details: Some(escalation_details.to_string()),
        pr_summary: None,
    };

    let description = build_pr_description(&params);

    assert!(
        description.contains("⚠️ Failure Details"),
        "Escalation should have failure details section"
    );
    assert!(
        description.contains("How should the authentication flow handle expired tokens?"),
        "Should contain the escalation question"
    );
    assert!(
        description.contains("Partial Work"),
        "Should contain partial work summary"
    );
    assert!(
        description.contains("Escalation"),
        "Decisions table should list 'Escalation' as source"
    );
    assert!(
        description.contains("\u{26a0}\u{fe0f} Escalated"),
        "Answer cell should show '⚠️ Escalated' for empty-answer DecisionRecord"
    );
}

// ===========================================================================
// CR Fix: enriched PrSummary path in build_pr_description()
// ===========================================================================

/// Verify build_pr_description() uses PrSummary fields when Some, not fallback constants.
#[test]
fn test_git_provider_pr_description_enriched_with_pr_summary() {
    use bmad_bot::git_provider::PrSummary;

    let summary = PrSummary {
        context: "Implemented the git provider trait with full GitHub/GitLab support.".to_string(),
        how_to_test: "Run `cargo test --test integration` and inspect test_git_provider results.".to_string(),
        additional_info: "Added rustls dev-dependency for crypto provider in integration tests.".to_string(),
    };

    let params = PrDescriptionParams {
        story_key: "5-1-git-provider".to_string(),
        story_title: "Git Provider Trait".to_string(),
        outcome_summary: "completed successfully".to_string(),
        decisions_section: format_pr_decisions_section(&[]),
        failure_details: None,
        pr_summary: Some(summary),
    };

    let description = build_pr_description(&params);

    // Agent context replaces the fallback constant
    assert!(
        description.contains("Implemented the git provider trait"),
        "Should use PrSummary.context, not default fallback"
    );
    assert!(
        !description.contains("Development session completed. No detailed context was captured."),
        "Default context fallback must NOT appear when PrSummary is provided"
    );

    // How-to-test replaces the fallback constant
    assert!(
        description.contains("cargo test --test integration"),
        "Should use PrSummary.how_to_test, not default fallback"
    );
    assert!(
        !description.contains("Run `cargo test` to verify all tests pass."),
        "Default how_to_test fallback must NOT appear when PrSummary is provided"
    );

    // Additional info replaces the fallback constant
    assert!(
        description.contains("Added rustls dev-dependency"),
        "Should use PrSummary.additional_info, not default fallback"
    );
    assert!(
        !description.contains("No additional information available."),
        "Default additional_info fallback must NOT appear when PrSummary is provided"
    );
}

// ===========================================================================
// Task 7: End-to-end factory → trait method chain test (supplementary)
// ===========================================================================

/// Factory → trait dispatch → method execution chain: GitLab get_pr_url()
#[tokio::test]
async fn test_git_provider_factory_to_trait_method_gitlab_get_pr_url() {
    let config = test_config("gitlab");
    let provider = create_provider(&config, "glpat-test-token")
        .expect("GitLab factory should succeed");

    // Valid PR ID → correct URL
    let url = provider.get_pr_url("42").await;
    assert!(url.is_ok());
    assert_eq!(
        url.unwrap(),
        "https://gitlab.com/test-owner/test-repo/-/merge_requests/42"
    );
}

/// Factory → trait dispatch → invalid PR ID → Err(InvalidPrId)
#[tokio::test]
async fn test_git_provider_factory_to_trait_method_gitlab_invalid_pr_id() {
    let config = test_config("gitlab");
    let provider = create_provider(&config, "glpat-test-token")
        .expect("GitLab factory should succeed");

    // Invalid PR ID
    let result = provider.get_pr_url("not-a-number").await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, GitProviderError::InvalidPrId { .. }),
        "Expected InvalidPrId, got: {err:?}"
    );
}
