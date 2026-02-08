//! GitLab merge-request provider implementation.
//!
//! Implements [`GitProvider`] for GitLab using `reqwest-middleware` with automatic
//! retry via [`build_http_client`](crate::config::build_http_client).
//! All errors are mapped to [`GitProviderError`] via [`map_gitlab_error`].

use async_trait::async_trait;
use reqwest_middleware::ClientWithMiddleware;

use super::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use crate::config::{GitProviderConfig, build_http_client};

/// GitLab API response for merge request creation.
///
/// Only the fields we need are deserialized — `serde` ignores unknown fields by default.
/// Uses `iid` (project-scoped internal ID), **not** `id` (global database ID).
#[derive(serde::Deserialize)]
struct CreateMrResponse {
    /// Merge request internal ID scoped to the project (the number in the URL).
    iid: u64,
    /// Browser URL for the merge request.
    web_url: String,
}

/// GitLab implementation of the [`GitProvider`] trait.
///
/// Uses `reqwest-middleware` (`ClientWithMiddleware`) for authenticated GitLab REST API v4 access
/// with automatic retry on transient errors. Construct via [`GitLabProvider::new`].
pub struct GitLabProvider {
    /// Shared HTTP client from [`build_http_client`] with retry middleware.
    client: ClientWithMiddleware,
    /// GitLab API base URL (e.g., `https://gitlab.com/api/v4`).
    base_url: String,
    /// URL-encoded `owner%2Frepo` project path for API endpoint construction.
    project_path: String,
    /// GitLab personal access token.
    token: String,
    /// Repository owner (for web URL construction).
    owner: String,
    /// Repository name (for web URL construction).
    repo: String,
}

impl GitLabProvider {
    /// Create a new `GitLabProvider` from configuration and a personal access token.
    ///
    /// # Errors
    /// Returns [`GitProviderError::AuthenticationFailed`] if `token` is empty.
    pub fn new(config: &GitProviderConfig, token: &str) -> Result<Self, GitProviderError> {
        if token.is_empty() {
            return Err(GitProviderError::AuthenticationFailed {
                reason: "GitLab token is empty".into(),
            });
        }

        let client = build_http_client();
        let project_path = format!("{}%2F{}", config.repo_owner, config.repo_name);

        Ok(Self {
            client,
            base_url: "https://gitlab.com/api/v4".to_string(),
            project_path,
            token: token.to_string(),
            owner: config.repo_owner.clone(),
            repo: config.repo_name.clone(),
        })
    }
}

#[async_trait]
impl GitProvider for GitLabProvider {
    /// Create a merge request on GitLab.
    ///
    /// Sends `POST /projects/:id/merge_requests` and returns a [`PrInfo`]
    /// with the MR's `iid` and `web_url`.
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        let url = format!(
            "{}/projects/{}/merge_requests",
            self.base_url, self.project_path
        );

        let json_body = serde_json::to_vec(&serde_json::json!({
            "source_branch": params.source_branch,
            "target_branch": params.target_branch,
            "title": params.title,
            "description": params.body,
        }))
        .map_err(|e| GitProviderError::InvalidResponse {
            reason: format!("Failed to serialize MR request body: {e}"),
        })?;

        let response = self
            .client
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .header("Content-Type", "application/json")
            .body(json_body)
            .send()
            .await
            .map_err(|e| GitProviderError::NetworkError {
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(map_gitlab_error(status, body));
        }

        let resp_bytes = response
            .bytes()
            .await
            .map_err(|e| GitProviderError::InvalidResponse {
                reason: format!("Failed to read GitLab MR response body: {e}"),
            })?;

        let mr: CreateMrResponse =
            serde_json::from_slice(&resp_bytes).map_err(|e| GitProviderError::InvalidResponse {
                reason: format!("Failed to parse GitLab MR response: {e}"),
            })?;

        tracing::info!(
            action = "mr_created",
            mr_iid = %mr.iid,
            url = %mr.web_url,
            "Merge request created"
        );

        Ok(PrInfo {
            id: mr.iid.to_string(),
            url: mr.web_url,
            number: mr.iid,
        })
    }

    /// Add a note (comment) to an existing merge request.
    ///
    /// Sends `POST /projects/:id/merge_requests/:iid/notes`.
    ///
    /// # Errors
    /// Returns [`GitProviderError::InvalidPrId`] if `pr_id` is not a valid `u64`.
    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        let mr_iid = pr_id
            .parse::<u64>()
            .map_err(|_| GitProviderError::InvalidPrId {
                pr_id: pr_id.to_string(),
            })?;

        let url = format!(
            "{}/projects/{}/merge_requests/{}/notes",
            self.base_url, self.project_path, mr_iid
        );

        let json_body = serde_json::to_vec(&serde_json::json!({
            "body": body,
        }))
        .map_err(|e| GitProviderError::InvalidResponse {
            reason: format!("Failed to serialize note request body: {e}"),
        })?;

        let response = self
            .client
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .header("Content-Type", "application/json")
            .body(json_body)
            .send()
            .await
            .map_err(|e| GitProviderError::NetworkError {
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let resp_body = response.text().await.unwrap_or_default();
            return Err(map_gitlab_error(status, resp_body));
        }

        tracing::info!(
            action = "mr_comment_added",
            pr_id = %pr_id,
            "Note added to merge request"
        );

        Ok(())
    }

    /// Get the URL for a merge request by its `iid`.
    ///
    /// Constructs the URL deterministically — no API call needed.
    ///
    /// # Errors
    /// Returns [`GitProviderError::InvalidPrId`] if `pr_id` is not a valid `u64`.
    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        let mr_iid = pr_id
            .parse::<u64>()
            .map_err(|_| GitProviderError::InvalidPrId {
                pr_id: pr_id.to_string(),
            })?;

        Ok(format!(
            "https://gitlab.com/{}/{}/-/merge_requests/{}",
            self.owner, self.repo, mr_iid
        ))
    }
}

/// Map an HTTP status code and response body to a [`GitProviderError`].
///
/// Called when the GitLab API returns a non-success status. Transient errors
/// (429, 500, 503) are already retried by `reqwest-middleware` before reaching
/// this function — only permanent failures or exhausted retries arrive here.
fn map_gitlab_error(status: reqwest::StatusCode, body: String) -> GitProviderError {
    match status.as_u16() {
        401 | 403 => GitProviderError::AuthenticationFailed { reason: body },
        404 => GitProviderError::ApiError {
            status: status.as_u16(),
            message: body,
        },
        422 => GitProviderError::BranchNotFound { branch: body },
        429 => GitProviderError::RateLimited {
            retry_after_secs: None,
        },
        _ => GitProviderError::ApiError {
            status: status.as_u16(),
            message: body,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a valid `GitProviderConfig` for tests.
    fn test_config() -> GitProviderConfig {
        GitProviderConfig {
            provider: "gitlab".to_string(),
            repo_owner: "test-owner".to_string(),
            repo_name: "test-repo".to_string(),
            target_branch: "main".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Constructor tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gitlab_provider_new_success() {
        let config = test_config();
        let result = GitLabProvider::new(&config, "glpat-valid-token");
        assert!(result.is_ok(), "Expected Ok for valid (non-empty) token");
    }

    #[test]
    fn test_gitlab_provider_new_empty_token_fails() {
        let config = test_config();
        let result = GitLabProvider::new(&config, "");
        match result {
            Err(GitProviderError::AuthenticationFailed { reason }) => {
                assert!(
                    reason.contains("empty"),
                    "Expected 'empty' in reason, got: {reason}"
                );
            }
            Err(other) => panic!("Expected AuthenticationFailed, got: {other}"),
            Ok(_) => panic!("Expected Err for empty token, got Ok"),
        }
    }

    #[test]
    fn test_gitlab_provider_new_stores_fields() {
        let config = GitProviderConfig {
            provider: "gitlab".to_string(),
            repo_owner: "my-org".to_string(),
            repo_name: "my-repo".to_string(),
            target_branch: "develop".to_string(),
        };
        let provider = GitLabProvider::new(&config, "glpat-token").expect("should build");
        assert_eq!(provider.owner, "my-org");
        assert_eq!(provider.repo, "my-repo");
        assert_eq!(provider.base_url, "https://gitlab.com/api/v4");
        assert_eq!(provider.project_path, "my-org%2Fmy-repo");
    }

    #[test]
    fn test_gitlab_provider_project_path_encoding() {
        let config = GitProviderConfig {
            provider: "gitlab".to_string(),
            repo_owner: "acme-corp".to_string(),
            repo_name: "awesome-project".to_string(),
            target_branch: "main".to_string(),
        };
        let provider = GitLabProvider::new(&config, "glpat-token").expect("should build");
        assert_eq!(
            provider.project_path, "acme-corp%2Fawesome-project",
            "Project path should be URL-encoded with %2F"
        );
    }

    #[test]
    fn test_gitlab_provider_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<GitLabProvider>();
    }

    // -----------------------------------------------------------------------
    // Error mapping tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_map_gitlab_error_401_authentication() {
        let err = map_gitlab_error(reqwest::StatusCode::UNAUTHORIZED, "401 Unauthorized".into());
        match err {
            GitProviderError::AuthenticationFailed { reason } => {
                assert_eq!(reason, "401 Unauthorized");
            }
            other => panic!("Expected AuthenticationFailed, got: {other}"),
        }
    }

    #[test]
    fn test_map_gitlab_error_403_authentication() {
        let err = map_gitlab_error(reqwest::StatusCode::FORBIDDEN, "403 Forbidden".into());
        match err {
            GitProviderError::AuthenticationFailed { reason } => {
                assert_eq!(reason, "403 Forbidden");
            }
            other => panic!("Expected AuthenticationFailed, got: {other}"),
        }
    }

    #[test]
    fn test_map_gitlab_error_404_api_error() {
        let err = map_gitlab_error(
            reqwest::StatusCode::NOT_FOUND,
            "404 Project Not Found".into(),
        );
        match err {
            GitProviderError::ApiError { status, message } => {
                assert_eq!(status, 404);
                assert_eq!(message, "404 Project Not Found");
            }
            other => panic!("Expected ApiError, got: {other}"),
        }
    }

    #[test]
    fn test_map_gitlab_error_422_branch_not_found() {
        let err = map_gitlab_error(
            reqwest::StatusCode::UNPROCESSABLE_ENTITY,
            "Source branch does not exist".into(),
        );
        match err {
            GitProviderError::BranchNotFound { branch } => {
                assert_eq!(branch, "Source branch does not exist");
            }
            other => panic!("Expected BranchNotFound, got: {other}"),
        }
    }

    #[test]
    fn test_map_gitlab_error_429_rate_limited() {
        let err = map_gitlab_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded".into(),
        );
        match err {
            GitProviderError::RateLimited { retry_after_secs } => {
                assert_eq!(retry_after_secs, None);
            }
            other => panic!("Expected RateLimited, got: {other}"),
        }
    }

    #[test]
    fn test_map_gitlab_error_500_api_error() {
        let err = map_gitlab_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error".into(),
        );
        match err {
            GitProviderError::ApiError { status, message } => {
                assert_eq!(status, 500);
                assert_eq!(message, "Internal Server Error");
            }
            other => panic!("Expected ApiError, got: {other}"),
        }
    }

    // -----------------------------------------------------------------------
    // get_pr_url tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_pr_url_constructs_correct_url() {
        let config = test_config();
        let provider = GitLabProvider::new(&config, "glpat-token").expect("should build");
        let url = provider.get_pr_url("34").await.expect("should succeed");
        assert_eq!(
            url, "https://gitlab.com/test-owner/test-repo/-/merge_requests/34",
            "URL should use /-/ separator and iid"
        );
    }

    #[tokio::test]
    async fn test_get_pr_url_invalid_pr_id() {
        let config = test_config();
        let provider = GitLabProvider::new(&config, "glpat-token").expect("should build");
        let result = provider.get_pr_url("not-a-number").await;
        match result {
            Err(GitProviderError::InvalidPrId { pr_id }) => {
                assert_eq!(pr_id, "not-a-number");
            }
            Err(other) => panic!("Expected InvalidPrId, got: {other}"),
            Ok(url) => panic!("Expected Err, got Ok({url})"),
        }
    }

    // -----------------------------------------------------------------------
    // add_comment invalid pr_id test
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_add_comment_invalid_pr_id() {
        let config = test_config();
        let provider = GitLabProvider::new(&config, "glpat-token").expect("should build");
        let result = provider.add_comment("abc", "some comment").await;
        match result {
            Err(GitProviderError::InvalidPrId { pr_id }) => {
                assert_eq!(pr_id, "abc");
            }
            Err(other) => panic!("Expected InvalidPrId, got: {other}"),
            Ok(()) => panic!("Expected Err, got Ok"),
        }
    }

    // -----------------------------------------------------------------------
    // CreateMrResponse deserialization test
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_mr_response_deserializes_from_gitlab_json() {
        let json = r#"{
            "id": 52,
            "iid": 34,
            "title": "Test MR",
            "description": "A merge request",
            "state": "opened",
            "web_url": "https://gitlab.com/test-owner/test-repo/-/merge_requests/34",
            "source_branch": "story/5-3-gitlab",
            "target_branch": "main",
            "author": { "id": 1, "username": "admin" },
            "merge_status": "can_be_merged"
        }"#;

        let mr: CreateMrResponse =
            serde_json::from_str(json).expect("Should deserialize GitLab MR response");
        assert_eq!(mr.iid, 34, "Should extract iid (not id)");
        assert_eq!(
            mr.web_url, "https://gitlab.com/test-owner/test-repo/-/merge_requests/34",
            "Should extract web_url"
        );
    }
}
