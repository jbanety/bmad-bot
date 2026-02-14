//! Git provider abstraction — trait + factory for GitHub/GitLab PR creation.
//!
//! This module defines the [`GitProvider`] async trait and shared types used by
//! all provider implementations. The [`create_provider`] factory function selects
//! the correct implementation based on configuration.
//!
//! Implemented in Story 5.1 (GitHub) and Story 5.3 (GitLab).

mod github;
mod gitlab;

pub use github::GitHubProvider;
pub use gitlab::GitLabProvider;

use async_trait::async_trait;

use crate::config::GitProviderConfig;

/// Errors originating from git provider operations.
///
/// Each variant carries structured context for logging and error handling.
/// Uses `thiserror` for `Display` and `Error` derive — no `anyhow` in this module.
#[derive(Debug, thiserror::Error)]
pub enum GitProviderError {
    /// HTTP-level failure after retries exhausted.
    #[error("API error (HTTP {status}): {message}")]
    ApiError {
        /// HTTP status code.
        status: u16,
        /// Error description from the API.
        message: String,
    },

    /// Token missing or invalid (401/403).
    #[error("Authentication failed: {reason}")]
    AuthenticationFailed {
        /// Why authentication failed.
        reason: String,
    },

    /// Source or target branch doesn't exist on remote, or no commits between them.
    #[error("Branch not found: {branch}")]
    BranchNotFound {
        /// The branch that was not found or the validation message.
        branch: String,
    },

    /// A pull request already exists for this source → target branch combination.
    #[error("PR already exists for branch '{branch}': {details}")]
    DuplicatePr {
        /// The source branch that already has an open PR.
        branch: String,
        /// Detail message from the API (e.g. "A pull request already exists for ...").
        details: String,
    },

    /// GitHub 422 validation error that doesn't match known patterns.
    #[error("Validation failed (HTTP 422): {message}")]
    ValidationFailed {
        /// The top-level message from the API.
        message: String,
        /// Raw error details from the API response (JSON array as string).
        details: String,
    },

    /// Rate limit exceeded (429).
    #[error("Rate limited{}", match .retry_after_secs { Some(s) => format!(", retry after {s}s"), None => String::new() })]
    RateLimited {
        /// Optional retry-after hint in seconds.
        retry_after_secs: Option<u64>,
    },

    /// Connection or DNS failure.
    #[error("Network error: {reason}")]
    NetworkError {
        /// Description of the network failure.
        reason: String,
    },

    /// Unexpected response format from the API.
    #[error("Invalid response: {reason}")]
    InvalidResponse {
        /// Why the response was invalid.
        reason: String,
    },

    /// `pr_id` string could not be parsed as `u64`.
    #[error("Invalid PR ID '{pr_id}': expected a numeric value")]
    InvalidPrId {
        /// The invalid PR ID string.
        pr_id: String,
    },

    /// Factory called with unsupported provider.
    #[error("Provider not configured: {provider}")]
    ProviderNotConfigured {
        /// The unsupported provider name.
        provider: String,
    },

    /// Octocrab client construction failed.
    #[error("Provider build error: {reason}")]
    BuildError {
        /// Why the client could not be built.
        reason: String,
    },
}

/// Parameters for creating a pull request.
///
/// Dedicated struct — no loose primitives as function params.
#[derive(Debug, Clone)]
pub struct CreatePrParams {
    /// PR title.
    pub title: String,
    /// PR description body (markdown).
    pub body: String,
    /// Source branch name (e.g., `story/1-2-user-auth`).
    pub source_branch: String,
    /// Target branch name (e.g., `main`).
    pub target_branch: String,
}

/// Information about a created pull request.
///
/// Returned by [`GitProvider::create_pr`] on success.
#[derive(Debug, Clone)]
pub struct PrInfo {
    /// Provider-specific PR identifier (stringified number for GitHub).
    pub id: String,
    /// Full URL to the PR on the hosting platform.
    pub url: String,
    /// Numeric PR number.
    pub number: u64,
}

/// Async trait for git hosting provider operations.
///
/// Implementations must be `Send + Sync` to support concurrent usage.
/// Currently implemented for GitHub via [`GitHubProvider`].
#[async_trait]
pub trait GitProvider: Send + Sync {
    /// Create a pull request with the given parameters.
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError>;

    /// Add a comment to an existing pull request.
    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError>;

    /// Get the URL for a pull request by its ID.
    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError>;
}

/// Factory function to create a provider based on configuration.
///
/// # Arguments
/// - `config` — Git provider configuration from `bmad-bot.yaml`
/// - `token` — Personal access token for the provider
///
/// # Returns
/// A boxed [`GitProvider`] implementation, or [`GitProviderError`] if the provider
/// is not supported or client construction fails.
///
/// # Examples
/// ```ignore
/// let provider = create_provider(&config, "ghp_token123")?;
/// let pr = provider.create_pr(params).await?;
/// ```
pub fn create_provider(
    config: &GitProviderConfig,
    token: &str,
) -> Result<Box<dyn GitProvider>, GitProviderError> {
    match config.provider.as_str() {
        "github" => {
            let provider = GitHubProvider::new(config, token)?;
            Ok(Box::new(provider) as Box<dyn GitProvider>)
        }
        "gitlab" => {
            let provider = GitLabProvider::new(config, token)?;
            Ok(Box::new(provider) as Box<dyn GitProvider>)
        }
        other => Err(GitProviderError::ProviderNotConfigured {
            provider: other.into(),
        }),
    }
}

/// Parameters for building a PR description.
/// Agent-generated PR summary with structured context for enriched PR descriptions.
///
/// Constructed by the pipeline from raw `Option<String>` fields in
/// [`SessionOutcome::Completed`](crate::session::SessionOutcome) or auto-generated
/// from [`EscalationReport`](crate::session::escalation::EscalationReport) fields.
///
/// Lives in `git_provider` alongside [`PrDescriptionParams`] — NOT in the `session`
/// module — to avoid a `session` → `git_provider` dependency.
#[derive(Debug, Clone)]
pub struct PrSummary {
    /// What was implemented and why, referencing the story requirements.
    pub context: String,
    /// Concrete commands and steps to verify the changes (e.g., `cargo test`, specific test names).
    pub how_to_test: String,
    /// Notable design decisions, dependencies added, tech debt created, migration notes.
    pub additional_info: String,
}

/// Fallback text when no agent-generated context is available.
pub const DEFAULT_CONTEXT: &str =
    "Development session completed. No detailed context was captured.";
/// Fallback text when no agent-generated testing instructions are available.
pub const DEFAULT_HOW_TO_TEST: &str =
    "Run `cargo test` to verify all tests pass. Review the changed files for correctness.";
/// Fallback text when no agent-generated additional info is available.
pub const DEFAULT_ADDITIONAL_INFO: &str = "No additional information available.";

///
/// Used by [`build_pr_description`] to generate a structured markdown body.
#[derive(Debug, Clone)]
pub struct PrDescriptionParams {
    /// Story key (e.g., `"5-1-git-provider-trait-github-pr-creation"`).
    pub story_key: String,
    /// Story title (e.g., `"Git Provider Trait & GitHub PR Creation"`).
    pub story_title: String,
    /// Outcome summary (e.g., `"completed successfully"`, `"failed"`, `"escalated — needs clarification"`).
    pub outcome_summary: String,
    /// Formatted decisions section from `format_pr_decisions_section()`.
    pub decisions_section: String,
    /// Failure/escalation details — only present for failed or escalated stories.
    pub failure_details: Option<String>,
    /// Agent-generated PR summary with context, testing instructions, and additional info.
    /// When `None`, fallback constants are used in the PR description.
    pub pr_summary: Option<PrSummary>,
}

/// Build a structured PR description from session outcome data.
///
/// Produces an enriched markdown body with:
/// - Story header and status
/// - Agent-generated context (or fallback)
/// - Optional failure/escalation details
/// - Supervisor decisions
/// - Agent-generated testing instructions (or fallback)
/// - Agent-generated additional info (or fallback)
/// - bmad-bot footer with link
pub fn build_pr_description(params: &PrDescriptionParams) -> String {
    let mut body = String::new();

    // Header
    body.push_str(&format!(
        "# 📋 Story: {} — {}\n\n",
        params.story_key, params.story_title
    ));
    body.push_str(&format!("**Status:** {}\n\n", params.outcome_summary));

    // Context section
    let context = params
        .pr_summary
        .as_ref()
        .map_or(DEFAULT_CONTEXT, |s| s.context.as_str());
    body.push_str(&format!("## 📝 Context\n\n{context}\n\n"));

    // Failure details (only if present)
    if let Some(ref details) = params.failure_details {
        body.push_str(&format!("## ⚠️ Failure Details\n\n{details}\n\n"));
    }

    // Supervisor decisions
    body.push_str(&params.decisions_section);
    body.push('\n');

    // How to test section
    let how_to_test = params
        .pr_summary
        .as_ref()
        .map_or(DEFAULT_HOW_TO_TEST, |s| s.how_to_test.as_str());
    body.push_str(&format!("## 🧪 How to test\n\n{how_to_test}\n\n"));

    // Additional information section
    let additional_info = params
        .pr_summary
        .as_ref()
        .map_or(DEFAULT_ADDITIONAL_INFO, |s| s.additional_info.as_str());
    body.push_str(&format!(
        "## ℹ️ Additional information\n\n{additional_info}\n\n"
    ));

    // Footer
    body.push_str("---\n*Generated by [bmad-bot](https://github.com/jbanety/bmad-bot)*\n");

    body
}

/// Build a conventional-commit-style PR title.
///
/// - Success: `"feat({story_key}): {story_title}"`
/// - Failure: `"wip({story_key}): {story_title} [NEEDS REVIEW]"`
pub fn build_pr_title(story_key: &str, story_title: &str, is_failure: bool) -> String {
    if is_failure {
        format!("wip({story_key}): {story_title} [NEEDS REVIEW]")
    } else {
        format!("feat({story_key}): {story_title}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Factory tests
    // -----------------------------------------------------------------------

    /// Install a rustls CryptoProvider for tests that build an Octocrab client.
    /// Safe to call multiple times — returns `Err` if already installed, which we ignore.
    fn install_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[tokio::test]
    async fn test_create_provider_github_returns_ok() {
        install_crypto_provider();
        let config = GitProviderConfig {
            provider: "github".to_string(),
            repo_owner: "test-owner".to_string(),
            repo_name: "test-repo".to_string(),
            target_branch: "main".to_string(),
        };
        let result = create_provider(&config, "ghp_test_token_123");
        assert!(result.is_ok(), "Expected Ok for github provider");
    }

    #[test]
    fn test_create_provider_gitlab_returns_ok() {
        let config = GitProviderConfig {
            provider: "gitlab".to_string(),
            repo_owner: "test-owner".to_string(),
            repo_name: "test-repo".to_string(),
            target_branch: "main".to_string(),
        };
        let result = create_provider(&config, "glpat-test");
        assert!(
            result.is_ok(),
            "Expected Ok for gitlab provider with valid token"
        );
    }

    #[test]
    fn test_create_provider_unknown_returns_not_configured() {
        let config = GitProviderConfig {
            provider: "bitbucket".to_string(),
            repo_owner: "test-owner".to_string(),
            repo_name: "test-repo".to_string(),
            target_branch: "main".to_string(),
        };
        let result = create_provider(&config, "token");
        match result {
            Err(GitProviderError::ProviderNotConfigured { provider }) => {
                assert_eq!(provider, "bitbucket");
            }
            Err(other) => panic!("Expected ProviderNotConfigured, got: {other}"),
            Ok(_) => panic!("Expected Err, got Ok"),
        }
    }

    // -----------------------------------------------------------------------
    // GitProviderError Display tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_git_provider_error_display_variants() {
        let cases: Vec<(GitProviderError, &str)> = vec![
            (
                GitProviderError::ApiError {
                    status: 500,
                    message: "Internal Server Error".into(),
                },
                "API error (HTTP 500): Internal Server Error",
            ),
            (
                GitProviderError::AuthenticationFailed {
                    reason: "token expired".into(),
                },
                "Authentication failed: token expired",
            ),
            (
                GitProviderError::BranchNotFound {
                    branch: "story/1-1".into(),
                },
                "Branch not found: story/1-1",
            ),
            (
                GitProviderError::RateLimited {
                    retry_after_secs: Some(60),
                },
                "Rate limited, retry after 60s",
            ),
            (
                GitProviderError::RateLimited {
                    retry_after_secs: None,
                },
                "Rate limited",
            ),
            (
                GitProviderError::NetworkError {
                    reason: "DNS resolution failed".into(),
                },
                "Network error: DNS resolution failed",
            ),
            (
                GitProviderError::InvalidResponse {
                    reason: "missing field".into(),
                },
                "Invalid response: missing field",
            ),
            (
                GitProviderError::ProviderNotConfigured {
                    provider: "bitbucket".into(),
                },
                "Provider not configured: bitbucket",
            ),
        ];

        for (error, expected) in cases {
            let display = format!("{error}");
            assert_eq!(display, expected, "Mismatch for error: {error:?}");
        }
    }

    #[test]
    fn test_git_provider_error_display_invalid_pr_id() {
        let err = GitProviderError::InvalidPrId {
            pr_id: "not-a-number".into(),
        };
        let display = format!("{err}");
        assert!(display.contains("not-a-number"));
        assert!(display.contains("Invalid PR ID"));
    }

    #[test]
    fn test_git_provider_error_display_duplicate_pr() {
        let err = GitProviderError::DuplicatePr {
            branch: "story/7-1".into(),
            details: "A pull request already exists for jbanety:story/7-1".into(),
        };
        let display = format!("{err}");
        assert!(display.contains("story/7-1"));
        assert!(display.contains("already exists"));
    }

    #[test]
    fn test_git_provider_error_display_validation_failed() {
        let err = GitProviderError::ValidationFailed {
            message: "Validation Failed".into(),
            details: r#"{"resource":"PullRequest","code":"custom","message":"some error"}"#.into(),
        };
        let display = format!("{err}");
        assert!(display.contains("Validation failed"));
        assert!(display.contains("422"));
    }

    #[test]
    fn test_git_provider_error_display_build_error() {
        let err = GitProviderError::BuildError {
            reason: "invalid base URL".into(),
        };
        let display = format!("{err}");
        assert!(display.contains("invalid base URL"));
        assert!(display.contains("build error"));
    }

    // -----------------------------------------------------------------------
    // PR Description Builder tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_pr_description_success_no_failure_details() {
        let params = PrDescriptionParams {
            story_key: "5-1-git-provider".to_string(),
            story_title: "Git Provider Trait".to_string(),
            outcome_summary: "completed successfully".to_string(),
            decisions_section: "## 🤖 Supervisor Decisions\n\nNo decisions.\n".to_string(),
            failure_details: None,
            pr_summary: None,
        };
        let body = build_pr_description(&params);
        assert!(body.contains("# 📋 Story: 5-1-git-provider — Git Provider Trait"));
        assert!(body.contains("**Status:** completed successfully"));
        assert!(!body.contains("⚠️ Failure Details"));
        assert!(body.contains("Supervisor Decisions"));
        assert!(body.contains("## 📝 Context"));
        assert!(body.contains(DEFAULT_CONTEXT));
        assert!(body.contains("## 🧪 How to test"));
        assert!(body.contains(DEFAULT_HOW_TO_TEST));
        assert!(body.contains("## ℹ️ Additional information"));
        assert!(body.contains(DEFAULT_ADDITIONAL_INFO));
        assert!(body.contains("Generated by [bmad-bot](https://github.com/jbanety/bmad-bot)"));
    }

    #[test]
    fn test_pr_description_failure_includes_details() {
        let params = PrDescriptionParams {
            story_key: "2-1-polling".to_string(),
            story_title: "Sprint Status Polling".to_string(),
            outcome_summary: "failed".to_string(),
            decisions_section: "## 🤖 Supervisor Decisions\n\nNo decisions.\n".to_string(),
            failure_details: Some("Build failed: missing dependency".to_string()),
            pr_summary: None,
        };
        let body = build_pr_description(&params);
        assert!(body.contains("## ⚠️ Failure Details"));
        assert!(body.contains("Build failed: missing dependency"));
        assert!(body.contains("**Status:** failed"));
        // Failure details should appear between Context and Supervisor Decisions
        let context_pos = body.find("## 📝 Context").unwrap();
        let failure_pos = body.find("## ⚠️ Failure Details").unwrap();
        let decisions_pos = body.find("## 🤖 Supervisor Decisions").unwrap();
        assert!(
            context_pos < failure_pos,
            "Context should come before Failure Details"
        );
        assert!(
            failure_pos < decisions_pos,
            "Failure Details should come before Supervisor Decisions"
        );
    }

    #[test]
    fn test_pr_description_escalation_includes_details() {
        let pr_summary = PrSummary {
            context: "Session escalated to human. Question: Which ORM? Reason: Architect failed"
                .to_string(),
            how_to_test: "N/A — session was escalated and requires human clarification."
                .to_string(),
            additional_info: "Partial work summary: 3 files modified".to_string(),
        };
        let params = PrDescriptionParams {
            story_key: "3-3-escalation".to_string(),
            story_title: "Human Escalation".to_string(),
            outcome_summary: "escalated — needs clarification".to_string(),
            decisions_section: "## 🤖 Supervisor Decisions\n\n| # | Source |\n".to_string(),
            failure_details: Some(
                "**Question:** Which ORM?\n**Reason:** Architect failed\n**Partial work:** 3 files"
                    .to_string(),
            ),
            pr_summary: Some(pr_summary),
        };
        let body = build_pr_description(&params);
        assert!(body.contains("escalated — needs clarification"));
        assert!(body.contains("Which ORM?"));
        assert!(body.contains("Architect failed"));
        assert!(body.contains("Partial work"));
        assert!(body.contains("N/A — session was escalated"));
    }

    #[test]
    fn test_pr_description_includes_decisions_section() {
        let decisions = "## 🤖 Supervisor Decisions\n\n| # | Source | Question | Decision | Reasoning |\n|---|--------|----------|----------|-----------|\n| 1 | Rule Engine (auto) | Use tokio? | Yes | Standard async runtime |\n";
        let params = PrDescriptionParams {
            story_key: "1-1-scaffolding".to_string(),
            story_title: "Scaffolding".to_string(),
            outcome_summary: "completed successfully".to_string(),
            decisions_section: decisions.to_string(),
            failure_details: None,
            pr_summary: None,
        };
        let body = build_pr_description(&params);
        assert!(body.contains("Supervisor Decisions"));
        assert!(body.contains("Use tokio?"));
        assert!(body.contains("Standard async runtime"));
    }

    #[test]
    fn test_pr_description_includes_footer() {
        let params = PrDescriptionParams {
            story_key: "key".to_string(),
            story_title: "title".to_string(),
            outcome_summary: "done".to_string(),
            decisions_section: String::new(),
            failure_details: None,
            pr_summary: None,
        };
        let body = build_pr_description(&params);
        assert!(
            body.contains("---\n*Generated by [bmad-bot](https://github.com/jbanety/bmad-bot)*")
        );
    }

    #[test]
    fn test_pr_description_enriched_all_fields() {
        let pr_summary = PrSummary {
            context: "Implemented enriched PR descriptions with agent-generated context."
                .to_string(),
            how_to_test: "1. Run `cargo test`\n2. Verify PR template includes all sections"
                .to_string(),
            additional_info: "Added `regex` dependency for XML tag parsing.".to_string(),
        };
        let params = PrDescriptionParams {
            story_key: "5-4-enriched-pr".to_string(),
            story_title: "Enriched PR Description".to_string(),
            outcome_summary: "completed successfully".to_string(),
            decisions_section: "## 🤖 Supervisor Decisions\n\nNo decisions.\n".to_string(),
            failure_details: None,
            pr_summary: Some(pr_summary),
        };
        let body = build_pr_description(&params);
        assert!(body.contains("# 📋 Story: 5-4-enriched-pr — Enriched PR Description"));
        assert!(body.contains("## 📝 Context"));
        assert!(body.contains("Implemented enriched PR descriptions"));
        assert!(!body.contains(DEFAULT_CONTEXT));
        assert!(body.contains("## 🧪 How to test"));
        assert!(body.contains("Run `cargo test`"));
        assert!(!body.contains(DEFAULT_HOW_TO_TEST));
        assert!(body.contains("## ℹ️ Additional information"));
        assert!(body.contains("Added `regex` dependency"));
        assert!(!body.contains(DEFAULT_ADDITIONAL_INFO));
        assert!(!body.contains("⚠️ Failure Details"));
    }

    #[test]
    fn test_pr_description_enriched_with_none_summary() {
        let params = PrDescriptionParams {
            story_key: "5-4-test".to_string(),
            story_title: "Test".to_string(),
            outcome_summary: "completed successfully".to_string(),
            decisions_section: "## 🤖 Supervisor Decisions\n\nNo decisions.\n".to_string(),
            failure_details: None,
            pr_summary: None,
        };
        let body = build_pr_description(&params);
        assert!(body.contains(DEFAULT_CONTEXT));
        assert!(body.contains(DEFAULT_HOW_TO_TEST));
        assert!(body.contains(DEFAULT_ADDITIONAL_INFO));
    }

    #[test]
    fn test_pr_description_failure_uses_fallback() {
        let params = PrDescriptionParams {
            story_key: "2-1-polling".to_string(),
            story_title: "Sprint Polling".to_string(),
            outcome_summary: "failed".to_string(),
            decisions_section: "## 🤖 Supervisor Decisions\n\nNo decisions.\n".to_string(),
            failure_details: Some("Session crashed: OOM".to_string()),
            pr_summary: None,
        };
        let body = build_pr_description(&params);
        assert!(body.contains("## ⚠️ Failure Details"));
        assert!(body.contains("Session crashed: OOM"));
        assert!(body.contains(DEFAULT_CONTEXT));
        assert!(body.contains(DEFAULT_HOW_TO_TEST));
        assert!(body.contains(DEFAULT_ADDITIONAL_INFO));
    }

    // -----------------------------------------------------------------------
    // PR Title Builder tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_pr_title_success() {
        let title = build_pr_title("5-1-git-provider", "Git Provider Trait", false);
        assert_eq!(title, "feat(5-1-git-provider): Git Provider Trait");
    }

    #[test]
    fn test_build_pr_title_failure() {
        let title = build_pr_title("2-1-polling", "Sprint Polling", true);
        assert_eq!(title, "wip(2-1-polling): Sprint Polling [NEEDS REVIEW]");
    }

    // -----------------------------------------------------------------------
    // Struct construction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_pr_params_fields() {
        let params = CreatePrParams {
            title: "feat(5-1): Git Provider".to_string(),
            body: "PR body".to_string(),
            source_branch: "story/5-1-git-provider".to_string(),
            target_branch: "main".to_string(),
        };
        assert_eq!(params.title, "feat(5-1): Git Provider");
        assert_eq!(params.body, "PR body");
        assert_eq!(params.source_branch, "story/5-1-git-provider");
        assert_eq!(params.target_branch, "main");
    }

    #[test]
    fn test_pr_info_fields() {
        let info = PrInfo {
            id: "42".to_string(),
            url: "https://github.com/owner/repo/pull/42".to_string(),
            number: 42,
        };
        assert_eq!(info.id, "42");
        assert_eq!(info.url, "https://github.com/owner/repo/pull/42");
        assert_eq!(info.number, 42);
    }
}
