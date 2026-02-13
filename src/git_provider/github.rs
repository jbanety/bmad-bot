//! GitHub provider implementation — creates PRs via octocrab.
//!
//! Implements [`GitProvider`] for GitHub using the [`octocrab`] crate.
//! All errors are mapped to [`GitProviderError`] via [`map_octocrab_error`].

use async_trait::async_trait;
use octocrab::Octocrab;

use super::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use crate::config::GitProviderConfig;

/// GitHub implementation of the [`GitProvider`] trait.
///
/// Uses [`octocrab`] for authenticated GitHub API access.
/// Construct via [`GitHubProvider::new`] with a [`GitProviderConfig`] and personal access token.
pub struct GitHubProvider {
    /// Authenticated octocrab instance.
    octocrab: Octocrab,
    /// Repository owner (user or organisation).
    owner: String,
    /// Repository name.
    repo: String,
}

impl GitHubProvider {
    /// Create a new `GitHubProvider` from configuration and a personal access token.
    ///
    /// # Errors
    /// Returns [`GitProviderError::BuildError`] if the octocrab client cannot be constructed.
    pub fn new(config: &GitProviderConfig, token: &str) -> Result<Self, GitProviderError> {
        let octocrab = Octocrab::builder()
            .personal_token(token.to_string())
            .build()
            .map_err(|e| GitProviderError::BuildError {
                reason: e.to_string(),
            })?;

        Ok(Self {
            octocrab,
            owner: config.repo_owner.clone(),
            repo: config.repo_name.clone(),
        })
    }
}

#[async_trait]
impl GitProvider for GitHubProvider {
    /// Create a pull request on GitHub.
    ///
    /// Uses `octocrab.pulls().create().body().send()` and maps the response
    /// to [`PrInfo`]. Falls back to a constructed URL if `html_url` is absent.
    ///
    /// If a PR already exists for this branch (GitHub 422 "already exists"),
    /// fetches the existing open PR and returns its info instead of failing.
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        match self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .create(&params.title, &params.source_branch, &params.target_branch)
            .body(&params.body)
            .send()
            .await
        {
            Ok(pr) => {
                let url = pr.html_url.map(|u| u.to_string()).unwrap_or_else(|| {
                    format!(
                        "https://github.com/{}/{}/pull/{}",
                        self.owner, self.repo, pr.number
                    )
                });

                tracing::info!(
                    action = "pr_created",
                    pr_number = %pr.number,
                    url = %url,
                    "Pull request created"
                );

                Ok(PrInfo {
                    id: pr.number.to_string(),
                    url,
                    number: pr.number,
                })
            }
            Err(e) => {
                let provider_err = map_octocrab_error(e);
                if matches!(provider_err, GitProviderError::DuplicatePr { .. }) {
                    // PR already exists — fetch it instead of failing.
                    tracing::info!(
                        action = "pr_already_exists",
                        source_branch = %params.source_branch,
                        "PR already exists for branch — fetching existing PR"
                    );
                    self.find_open_pr(&params.source_branch).await
                } else {
                    Err(provider_err)
                }
            }
        }
    }

    /// Add a comment to a pull request (uses the GitHub Issues API).
    ///
    /// # Errors
    /// Returns [`GitProviderError::InvalidPrId`] if `pr_id` is not a valid `u64`.
    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        let pr_number = pr_id
            .parse::<u64>()
            .map_err(|_| GitProviderError::InvalidPrId {
                pr_id: pr_id.to_string(),
            })?;

        self.octocrab
            .issues(&self.owner, &self.repo)
            .create_comment(pr_number, body)
            .await
            .map_err(map_octocrab_error)?;

        tracing::info!(
            action = "pr_comment_added",
            pr_id = %pr_id,
            "Comment added to PR"
        );

        Ok(())
    }

    /// Get the URL for a pull request by its numeric ID.
    ///
    /// Constructs the URL deterministically — no API call needed.
    ///
    /// # Errors
    /// Returns [`GitProviderError::InvalidPrId`] if `pr_id` is not a valid `u64`.
    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        let pr_number = pr_id
            .parse::<u64>()
            .map_err(|_| GitProviderError::InvalidPrId {
                pr_id: pr_id.to_string(),
            })?;

        Ok(format!(
            "https://github.com/{}/{}/pull/{}",
            self.owner, self.repo, pr_number
        ))
    }
}

impl GitHubProvider {
    /// Find an existing open PR for the given source branch.
    ///
    /// Lists open PRs filtered by `head` (owner:branch) and returns the first match.
    /// Falls back to a constructed URL if the API doesn't return `html_url`.
    async fn find_open_pr(&self, source_branch: &str) -> Result<PrInfo, GitProviderError> {
        let head = format!("{}:{}", self.owner, source_branch);
        let prs = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .list()
            .state(octocrab::params::State::Open)
            .head(&head)
            .send()
            .await
            .map_err(map_octocrab_error)?;

        let pr = prs
            .items
            .into_iter()
            .next()
            .ok_or_else(|| GitProviderError::BranchNotFound {
                branch: format!("PR reported as duplicate but no open PR found for head '{head}'"),
            })?;

        let url = pr.html_url.map(|u| u.to_string()).unwrap_or_else(|| {
            format!(
                "https://github.com/{}/{}/pull/{}",
                self.owner, self.repo, pr.number
            )
        });

        tracing::info!(
            action = "pr_found_existing",
            pr_number = %pr.number,
            url = %url,
            source_branch = %source_branch,
            "Found existing open PR for branch"
        );

        Ok(PrInfo {
            id: pr.number.to_string(),
            url,
            number: pr.number,
        })
    }
}

/// Map an [`octocrab::Error`] to a [`GitProviderError`].
///
/// Handles the `GitHub` variant by inspecting the HTTP status code:
/// - 401/403 → [`GitProviderError::AuthenticationFailed`]
/// - 422 → inspects `errors` array to distinguish:
///   - "already exists" → [`GitProviderError::DuplicatePr`]
///   - other → [`GitProviderError::BranchNotFound`] or [`GitProviderError::ValidationFailed`]
/// - 429 → [`GitProviderError::RateLimited`]
/// - Other → [`GitProviderError::ApiError`]
///
/// All other octocrab error variants are mapped to [`GitProviderError::NetworkError`].
fn map_octocrab_error(e: octocrab::Error) -> GitProviderError {
    match e {
        octocrab::Error::GitHub { source, .. } => {
            let status = source.status_code.as_u16();
            match status {
                401 | 403 => GitProviderError::AuthenticationFailed {
                    reason: source.message.clone(),
                },
                422 => map_422_error(&source),
                429 => GitProviderError::RateLimited {
                    retry_after_secs: None,
                },
                _ => GitProviderError::ApiError {
                    status,
                    message: source.message.clone(),
                },
            }
        }
        other => GitProviderError::NetworkError {
            reason: other.to_string(),
        },
    }
}

/// Inspect a GitHub 422 response to classify the validation error.
///
/// GitHub 422 responses include an `errors` array with structured details.
/// Common patterns:
/// - `"message": "A pull request already exists for ..."` → [`GitProviderError::DuplicatePr`]
/// - `"message": "No commits between ..."` → [`GitProviderError::BranchNotFound`]
/// - Other → [`GitProviderError::ValidationFailed`] with full error details
fn map_422_error(source: &octocrab::GitHubError) -> GitProviderError {
    // Serialize the errors array for logging/debugging
    let errors_str = source
        .errors
        .as_ref()
        .map(|errors| {
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default();

    // Combine message + errors into a single searchable string
    let full_text = format!("{} {}", source.message, errors_str).to_lowercase();

    if full_text.contains("already exists") {
        // Extract the branch name from error messages if possible
        let branch = source
            .errors
            .as_ref()
            .and_then(|errors| {
                errors.iter().find_map(|e| {
                    e.get("message")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
            })
            .unwrap_or_else(|| source.message.clone());

        GitProviderError::DuplicatePr {
            branch: String::new(),
            details: branch,
        }
    } else if full_text.contains("no commits between") || full_text.contains("branch not found") {
        GitProviderError::BranchNotFound {
            branch: if errors_str.is_empty() {
                source.message.clone()
            } else {
                errors_str
            },
        }
    } else {
        tracing::warn!(
            action = "github_422_unclassified",
            message = %source.message,
            errors = %errors_str,
            "Unclassified GitHub 422 — inspect errors for root cause"
        );
        GitProviderError::ValidationFailed {
            message: source.message.clone(),
            details: errors_str,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snafu::GenerateImplicitData;

    /// Install a rustls CryptoProvider for tests that build an Octocrab client.
    /// Safe to call multiple times — returns `Err` if already installed, which we ignore.
    fn install_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[tokio::test]
    async fn test_github_provider_new_success() {
        install_crypto_provider();
        let config = GitProviderConfig {
            provider: "github".to_string(),
            repo_owner: "test-owner".to_string(),
            repo_name: "test-repo".to_string(),
            target_branch: "main".to_string(),
        };
        let result = GitHubProvider::new(&config, "ghp_test_token_123");
        assert!(result.is_ok(), "Expected Ok for valid token");
    }

    #[tokio::test]
    async fn test_github_provider_new_stores_owner_repo() {
        install_crypto_provider();
        let config = GitProviderConfig {
            provider: "github".to_string(),
            repo_owner: "my-org".to_string(),
            repo_name: "my-repo".to_string(),
            target_branch: "develop".to_string(),
        };
        let provider = GitHubProvider::new(&config, "ghp_token").expect("should build");
        assert_eq!(provider.owner, "my-org");
        assert_eq!(provider.repo, "my-repo");
    }

    #[test]
    fn test_github_provider_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<GitHubProvider>();
    }

    #[test]
    fn test_map_octocrab_error_handles_variants() {
        // Test network/other error mapping — the `Other` variant is the only
        // octocrab::Error variant we can construct from outside the crate
        // because `GitHubError` is #[non_exhaustive].
        //
        // GitHub-specific status code mapping (401→AuthenticationFailed,
        // 422→BranchNotFound, 429→RateLimited, etc.) is verified implicitly
        // via E2E tests against real/mock HTTP responses.
        let other_err = octocrab::Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "connection refused",
            )),
            backtrace: snafu::Backtrace::generate(),
        };
        let mapped = map_octocrab_error(other_err);
        match mapped {
            GitProviderError::NetworkError { reason } => {
                assert!(
                    !reason.is_empty(),
                    "Network error reason should not be empty"
                );
            }
            other => panic!("Expected NetworkError, got: {other}"),
        }
    }
}
