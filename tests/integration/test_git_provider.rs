//! Integration tests for the git_provider module — PR creation, factory, and description building.
//!
//! Story 7.6: Git Provider & PR Creation Integration Tests
//!
//! These tests verify the public API surface from an external crate perspective,
//! cross-module integration (supervisor::decisions → git_provider), and the full
//! factory → trait dispatch → method execution chain.

// Task 0.2: Verify git_provider public API is accessible
use bmad_bot::config::GitProviderConfig;
#[allow(unused_imports)] // GitProvider trait import validates public API accessibility (Task 0.2)
use bmad_bot::git_provider::{
    build_pr_description, build_pr_title, create_provider, GitProvider, GitProviderError,
    PrDescriptionParams,
};
// Task 0.3: Verify GitHubProvider and GitLabProvider are accessible
#[allow(unused_imports)]
use bmad_bot::git_provider::{GitHubProvider, GitLabProvider};
// Task 0.4: Verify supervisor::decisions cross-module types are accessible
use bmad_bot::supervisor::decisions::{format_pr_decisions_section, DecisionRecord, DecisionSource};

/// Install rustls crypto provider for GitHub octocrab client construction.
/// Safe to call multiple times — returns Err if already installed, which we ignore.
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Build a test GitProviderConfig with the given provider name.
fn test_config(provider: &str) -> GitProviderConfig {
    GitProviderConfig {
        provider: provider.to_string(),
        repo_owner: "test-owner".to_string(),
        repo_name: "test-repo".to_string(),
        target_branch: "main".to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 2: Provider factory integration tests — public API smoke tests (AC #1, #2, #3)
// ─────────────────────────────────────────────────────────────────────────────

/// Task 2.1: Happy path — create_provider with "github" + valid token → Ok
/// Note: Requires tokio runtime because Octocrab uses tower::Buffer internally.
#[tokio::test]
async fn test_git_provider_factory_github_returns_ok() {
    install_crypto_provider();
    let config = test_config("github");
    let result = create_provider(&config, "ghp_test_token_123");
    assert!(result.is_ok(), "Expected Ok for github provider, got Err");
}

/// Task 2.1: Happy path — create_provider with "gitlab" + valid token → Ok
#[test]
fn test_git_provider_factory_gitlab_returns_ok() {
    let config = test_config("gitlab");
    let result = create_provider(&config, "glpat-test-token-123");
    assert!(result.is_ok(), "Expected Ok for gitlab provider, got Err");
}

/// Task 2.2: Error path — create_provider with "bitbucket" → Err(ProviderNotConfigured)
#[test]
fn test_git_provider_factory_bitbucket_returns_not_configured() {
    let config = test_config("bitbucket");
    let result = create_provider(&config, "some-token");
    match result {
        Err(GitProviderError::ProviderNotConfigured { provider }) => {
            assert_eq!(provider, "bitbucket");
        }
        other => panic!("Expected ProviderNotConfigured for bitbucket, got unexpected result: {}", if other.is_ok() { "Ok" } else { "different Err" }),
    }
}

/// Task 2.2: Error path — create_provider with empty string provider → Err(ProviderNotConfigured)
#[test]
fn test_git_provider_factory_empty_provider_returns_not_configured() {
    let config = test_config("");
    let result = create_provider(&config, "some-token");
    match result {
        Err(GitProviderError::ProviderNotConfigured { provider }) => {
            assert_eq!(provider, "");
        }
        other => panic!("Expected ProviderNotConfigured for empty provider, got unexpected result: {}", if other.is_ok() { "Ok" } else { "different Err" }),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 3: GitLab empty token rejection test (AC #4)
// ─────────────────────────────────────────────────────────────────────────────

/// Task 3.1: GitLabProvider::new with empty token → Err(AuthenticationFailed) with reason containing "empty"
#[test]
fn test_git_provider_gitlab_empty_token_returns_auth_failed() {
    let config = test_config("gitlab");
    let result = GitLabProvider::new(&config, "");
    match result {
        Err(GitProviderError::AuthenticationFailed { reason }) => {
            assert!(
                reason.to_lowercase().contains("empty"),
                "Expected reason to contain 'empty', got: {reason}"
            );
        }
        other => panic!("Expected AuthenticationFailed for empty token, got unexpected result: {}", if other.is_ok() { "Ok" } else { "different Err" }),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 4: Cross-module PR description integration tests (AC #5)
// ─────────────────────────────────────────────────────────────────────────────

/// Task 4.1–4.4: Build real DecisionRecord instances, format via supervisor module,
/// pass into PrDescriptionParams, verify cross-module chain produces correct PR description.
#[test]
fn test_git_provider_pr_description_with_real_decisions() {
    // Task 4.1: Build real DecisionRecord instances via DecisionRecord::new()
    let decision1 = DecisionRecord::new(
        "Should I use async for the factory?".to_string(),
        Some("Factory currently returns sync Result".to_string()),
        "No, keep factory synchronous".to_string(),
        DecisionSource::RuleEngine {
            rule_name: "sync_factory_pattern".to_string(),
        },
        "Factory is a thin constructor wrapper — no I/O needed".to_string(),
        vec!["Make factory async".to_string()],
    );

    let decision2 = DecisionRecord::new(
        "Which error type for unsupported providers?".to_string(),
        None,
        "Use ProviderNotConfigured variant".to_string(),
        DecisionSource::LlmFallback,
        "Matches existing error taxonomy in GitProviderError".to_string(),
        vec![
            "Use anyhow".to_string(),
            "Add new variant".to_string(),
        ],
    );

    let decisions = vec![decision1, decision2];

    // Task 4.2: Call format_pr_decisions_section (cross-module: supervisor → git_provider)
    let decisions_section = format_pr_decisions_section(&decisions);

    // Task 4.3: Pass into PrDescriptionParams and build description
    let params = PrDescriptionParams {
        story_key: "5-1-git-provider".to_string(),
        story_title: "Git Provider Trait".to_string(),
        outcome_summary: "completed successfully".to_string(),
        decisions_section: decisions_section.clone(),
        failure_details: None,
        pr_summary: None,
    };

    let description = build_pr_description(&params);

    // Task 4.4: Assert description contains required elements
    // Story key in header
    assert!(
        description.contains("📋 Story:"),
        "Missing story header emoji marker"
    );
    assert!(
        description.contains("5-1-git-provider"),
        "Missing story key in header"
    );
    assert!(
        description.contains("Git Provider Trait"),
        "Missing story title in header"
    );

    // Outcome summary
    assert!(
        description.contains("**Status:**"),
        "Missing Status label"
    );
    assert!(
        description.contains("completed successfully"),
        "Missing outcome summary"
    );

    // Supervisor Decisions section with actual decision content
    assert!(
        description.contains("Supervisor Decisions"),
        "Missing Supervisor Decisions section"
    );
    assert!(
        description.contains("sync_factory_pattern"),
        "Missing rule engine decision content"
    );
    assert!(
        description.contains("LLM Fallback"),
        "Missing LLM Fallback decision source"
    );

    // bmad-bot footer
    assert!(
        description.contains("bmad-bot"),
        "Missing bmad-bot footer"
    );

    // No failure details section for success
    assert!(
        !description.contains("⚠️ Failure Details"),
        "Success description should not contain Failure Details"
    );
}

/// Task 4.5: Assert build_pr_title for success case
#[test]
fn test_git_provider_pr_title_success() {
    let title = build_pr_title("5-1-git-provider", "Git Provider Trait", false);
    assert_eq!(title, "feat(5-1-git-provider): Git Provider Trait");
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 5: Failure PR description integration test (AC #6)
// ─────────────────────────────────────────────────────────────────────────────

/// Task 5.1–5.2: Build PrDescriptionParams with failure_details, assert Failure Details section
#[test]
fn test_git_provider_pr_description_failure_includes_details() {
    let decisions_section =
        format_pr_decisions_section(&[]); // No decisions for failure case

    let params = PrDescriptionParams {
        story_key: "2-1-polling".to_string(),
        story_title: "Sprint Polling".to_string(),
        outcome_summary: "failed".to_string(),
        decisions_section,
        failure_details: Some("LLM timeout after 3 retries".to_string()),
        pr_summary: None,
    };

    let description = build_pr_description(&params);

    // Task 5.2: Assert description contains Failure Details section
    assert!(
        description.contains("⚠️ Failure Details"),
        "Missing Failure Details section"
    );
    assert!(
        description.contains("LLM timeout after 3 retries"),
        "Missing failure text in description"
    );
}

/// Task 5.3: Assert build_pr_title for failure case
#[test]
fn test_git_provider_pr_title_failure() {
    let title = build_pr_title("2-1-polling", "Sprint Polling", true);
    assert_eq!(title, "wip(2-1-polling): Sprint Polling [NEEDS REVIEW]");
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 6: Escalation PR description test (supplementary)
// ─────────────────────────────────────────────────────────────────────────────

/// Task 6.1–6.2: Build PrDescriptionParams with escalation-style failure_details
#[test]
fn test_git_provider_pr_description_escalation_contains_fields() {
    let escalation_decision = DecisionRecord::new(
        "How should authentication tokens be rotated?".to_string(),
        Some("Token rotation policy unclear in project docs".to_string()),
        String::new(), // Empty answer = escalation
        DecisionSource::Escalation,
        "Neither rule engine nor LLM could determine token rotation policy".to_string(),
        vec![],
    );

    let decisions_section = format_pr_decisions_section(&[escalation_decision]);

    let escalation_details = concat!(
        "**Question:** How should authentication tokens be rotated?\n",
        "**Reason:** Token rotation policy not documented\n",
        "**Partial work:** Implemented provider factory and basic PR creation"
    );

    let params = PrDescriptionParams {
        story_key: "1-6-oauth".to_string(),
        story_title: "GitHub OAuth Device Flow".to_string(),
        outcome_summary: "escalated — needs clarification".to_string(),
        decisions_section,
        failure_details: Some(escalation_details.to_string()),
        pr_summary: None,
    };

    let description = build_pr_description(&params);

    // Task 6.2: Assert description contains all escalation fields
    assert!(
        description.contains("⚠️ Failure Details"),
        "Missing Failure Details section for escalation"
    );
    assert!(
        description.contains("How should authentication tokens be rotated?"),
        "Missing escalation question"
    );
    assert!(
        description.contains("Token rotation policy not documented"),
        "Missing escalation reason"
    );
    assert!(
        description.contains("Implemented provider factory"),
        "Missing partial work summary"
    );
    assert!(
        description.contains("Escalation"),
        "Missing Escalation decision source"
    );
    assert!(
        description.contains("escalated"),
        "Missing escalation status"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 7: End-to-end factory → trait method chain test (supplementary)
// ─────────────────────────────────────────────────────────────────────────────

/// Task 7.1–7.2: Factory → GitLab provider → get_pr_url("42") → correct URL
#[tokio::test]
async fn test_git_provider_factory_gitlab_get_pr_url_chain() {
    let config = test_config("gitlab");
    let provider = create_provider(&config, "glpat-test-token-456")
        .expect("GitLab provider creation should succeed");

    // Task 7.1-7.2: Call trait method on Box<dyn GitProvider> — validates dynamic dispatch
    let url = provider
        .get_pr_url("42")
        .await
        .expect("get_pr_url should succeed for valid numeric ID");

    assert_eq!(
        url,
        "https://gitlab.com/test-owner/test-repo/-/merge_requests/42",
        "URL should match GitLab merge request pattern"
    );
}

/// Task 7.3: get_pr_url with non-numeric ID → Err(InvalidPrId)
#[tokio::test]
async fn test_git_provider_factory_gitlab_get_pr_url_invalid_id() {
    let config = test_config("gitlab");
    let provider = create_provider(&config, "glpat-test-token-789")
        .expect("GitLab provider creation should succeed");

    let result = provider.get_pr_url("not-a-number").await;
    match result {
        Err(GitProviderError::InvalidPrId { pr_id }) => {
            assert_eq!(pr_id, "not-a-number");
        }
        other => panic!("Expected InvalidPrId for non-numeric ID, got: {other:?}"),
    }
}
