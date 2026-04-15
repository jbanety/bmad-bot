//! LLM provider factory — multi-provider support for session agent construction.
//!
//! Since rig's `Chat` trait is not object-safe (each provider returns a different
//! concrete `Agent<M>` type), we cannot use `Box<dyn Chat>`. Instead, this module
//! provides:
//!
//! - [`ProviderError`] — typed errors for provider initialization
//! - [`create_completion_model()`] — resolves API key and validates provider config
//! - [`resolve_api_key()`] — extracts the correct API key from secrets for a provider
//!
//! The actual agent construction happens in the session runner via match arms on
//! the provider name (following the established pattern from `supervisor/architect.rs`).

use crate::config::{BotSecrets, LlmRoleConfig};

/// Errors originating from LLM provider initialization.
///
/// Implements `std::error::Error + Send + Sync` via `thiserror`.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// The configured LLM provider is not supported.
    #[error("Unsupported LLM provider: '{provider}'. Supported: anthropic, openai, github-copilot")]
    UnsupportedProvider {
        /// The unsupported provider name from config.
        provider: String,
    },

    /// The required API key environment variable is missing or empty.
    #[error("Missing API key for provider '{provider}': set {env_var} in .env")]
    MissingApiKey {
        /// The provider that needs the key.
        provider: String,
        /// The environment variable that should contain the key.
        env_var: String,
    },

    /// Provider client creation failed.
    #[error("Failed to create {provider} client: {reason}")]
    ClientCreation {
        /// The provider that failed.
        provider: String,
        /// Description of the failure.
        reason: String,
    },
}

/// Resolve the API key for a given LLM provider from secrets.
///
/// Returns the key string if present and non-empty, or a [`ProviderError::MissingApiKey`]
/// if the key is missing or empty.
///
/// # Supported providers
/// - `"anthropic"` → `ANTHROPIC_API_KEY`
/// - `"openai"` → `OPENAI_API_KEY`
/// - `"github-copilot"` → `GITHUB_COPILOT_OAUTH_TOKEN`
pub fn resolve_api_key(provider: &str, secrets: &BotSecrets) -> Result<String, ProviderError> {
    let (key_opt, env_var) = match provider {
        "anthropic" => (&secrets.anthropic_api_key, "ANTHROPIC_API_KEY"),
        "openai" => (&secrets.openai_api_key, "OPENAI_API_KEY"),
        "github-copilot" => (
            &secrets.github_copilot_oauth_token,
            "GITHUB_COPILOT_OAUTH_TOKEN",
        ),
        other => {
            return Err(ProviderError::UnsupportedProvider {
                provider: other.to_string(),
            });
        }
    };

    match key_opt {
        Some(key) if !key.is_empty() => Ok(key.clone()),
        _ => Err(ProviderError::MissingApiKey {
            provider: provider.to_string(),
            env_var: env_var.to_string(),
        }),
    }
}

/// Validate provider configuration and resolve the API key.
///
/// This is the primary entry point for provider setup. It validates the
/// provider name, resolves the API key from secrets, and returns the key
/// for use in agent construction.
///
/// # Note on rig's type system
///
/// rig's `Chat` trait is **not** object-safe — each provider returns a different
/// concrete `Agent<M>` type. Therefore, this function cannot return a
/// `Box<dyn Chat>` or `Box<dyn CompletionModel>`. Instead, it returns the
/// resolved API key. The caller (session runner) uses match arms on the provider
/// name to construct the correct concrete agent type.
///
/// This follows the established pattern from `supervisor/architect.rs`.
pub fn create_completion_model(
    role_config: &LlmRoleConfig,
    secrets: &BotSecrets,
) -> Result<String, ProviderError> {
    // Validate provider is supported
    match role_config.provider.as_str() {
        "anthropic" | "openai" | "github-copilot" => {}
        other => {
            return Err(ProviderError::UnsupportedProvider {
                provider: other.to_string(),
            });
        }
    }

    // Resolve API key
    resolve_api_key(&role_config.provider, secrets)
}

/// Build an [`http::HeaderMap`] with headers required by the GitHub Copilot API.
///
/// The Copilot chat-completions endpoint requires an `Editor-Version` header
/// for IDE-based authentication tokens. Without it the API returns **400**
/// `"missing Editor-Version header for IDE auth"`.
///
/// Pass the returned map via `.http_headers()` on the rig `openai::Client`
/// builder **only** when the provider is `github-copilot`.
pub fn copilot_headers() -> http::HeaderMap {
    use http::{HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert("Editor-Version", HeaderValue::from_static("bmad-bot/0.1.0"));
    headers.insert(
        "Editor-Plugin-Version",
        HeaderValue::from_static("bmad-bot/0.1.0"),
    );
    headers.insert(
        "Copilot-Integration-Id",
        HeaderValue::from_static("vscode-chat"),
    );

    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LlmRoleConfig;

    /// Helper: create BotSecrets with all keys populated.
    fn secrets_with_all_keys() -> BotSecrets {
        BotSecrets {
            anthropic_api_key: Some("sk-ant-test-key".to_string()),
            openai_api_key: Some("sk-openai-test-key".to_string()),
            github_copilot_oauth_token: Some("gh-models-test-key".to_string()),
            github_token: Some("ghp_test".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        }
    }

    /// Helper: create BotSecrets with no keys.
    fn empty_secrets() -> BotSecrets {
        BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_copilot_oauth_token: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        }
    }

    #[test]
    fn test_provider_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ProviderError>();
    }

    #[test]
    fn test_provider_error_display() {
        let err = ProviderError::UnsupportedProvider {
            provider: "gemini".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("gemini"));
        assert!(display.contains("Unsupported"));

        let err = ProviderError::MissingApiKey {
            provider: "anthropic".to_string(),
            env_var: "ANTHROPIC_API_KEY".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("anthropic"));
        assert!(display.contains("ANTHROPIC_API_KEY"));

        let err = ProviderError::ClientCreation {
            provider: "openai".to_string(),
            reason: "timeout".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("openai"));
        assert!(display.contains("timeout"));
    }

    #[test]
    fn test_unsupported_provider_returns_error() {
        let secrets = secrets_with_all_keys();
        let config = LlmRoleConfig {
            provider: "gemini".to_string(),
            model: "gemini-pro".to_string(),
            reasoning_effort: None,
            base_url: None,
        };

        let result = create_completion_model(&config, &secrets);
        assert!(result.is_err());

        match result.unwrap_err() {
            ProviderError::UnsupportedProvider { provider } => {
                assert_eq!(provider, "gemini");
            }
            other => panic!("Expected UnsupportedProvider, got: {other:?}"),
        }
    }

    #[test]
    fn test_resolve_api_key_anthropic() {
        let secrets = secrets_with_all_keys();
        let key = resolve_api_key("anthropic", &secrets).expect("should resolve");
        assert_eq!(key, "sk-ant-test-key");
    }

    #[test]
    fn test_resolve_api_key_openai() {
        let secrets = secrets_with_all_keys();
        let key = resolve_api_key("openai", &secrets).expect("should resolve");
        assert_eq!(key, "sk-openai-test-key");
    }

    #[test]
    fn test_resolve_api_key_github_copilot() {
        let secrets = secrets_with_all_keys();
        let key = resolve_api_key("github-copilot", &secrets).expect("should resolve");
        assert_eq!(key, "gh-models-test-key");
    }

    #[test]
    fn test_resolve_api_key_missing_returns_error() {
        let secrets = empty_secrets();
        let result = resolve_api_key("anthropic", &secrets);
        assert!(result.is_err());

        match result.unwrap_err() {
            ProviderError::MissingApiKey { provider, env_var } => {
                assert_eq!(provider, "anthropic");
                assert_eq!(env_var, "ANTHROPIC_API_KEY");
            }
            other => panic!("Expected MissingApiKey, got: {other:?}"),
        }
    }

    #[test]
    fn test_resolve_api_key_empty_string_returns_error() {
        let secrets = BotSecrets {
            anthropic_api_key: Some(String::new()),
            openai_api_key: None,
            github_copilot_oauth_token: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        };
        let result = resolve_api_key("anthropic", &secrets);
        assert!(result.is_err());

        match result.unwrap_err() {
            ProviderError::MissingApiKey { .. } => {}
            other => panic!("Expected MissingApiKey for empty string, got: {other:?}"),
        }
    }

    #[test]
    fn test_resolve_api_key_unsupported_provider() {
        let secrets = secrets_with_all_keys();
        let result = resolve_api_key("bedrock", &secrets);
        assert!(result.is_err());

        match result.unwrap_err() {
            ProviderError::UnsupportedProvider { provider } => {
                assert_eq!(provider, "bedrock");
            }
            other => panic!("Expected UnsupportedProvider, got: {other:?}"),
        }
    }

    #[test]
    fn test_create_completion_model_anthropic_returns_key() {
        let secrets = secrets_with_all_keys();
        let config = LlmRoleConfig {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            reasoning_effort: None,
            base_url: None,
        };
        let key = create_completion_model(&config, &secrets).expect("should succeed");
        assert_eq!(key, "sk-ant-test-key");
    }

    #[test]
    fn test_create_completion_model_openai_returns_key() {
        let secrets = secrets_with_all_keys();
        let config = LlmRoleConfig {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            reasoning_effort: None,
            base_url: None,
        };
        let key = create_completion_model(&config, &secrets).expect("should succeed");
        assert_eq!(key, "sk-openai-test-key");
    }

    #[test]
    fn test_create_completion_model_github_copilot_returns_key() {
        let secrets = secrets_with_all_keys();
        let config = LlmRoleConfig {
            provider: "github-copilot".to_string(),
            model: "gpt-4o".to_string(),
            reasoning_effort: None,
            base_url: None,
        };
        let key = create_completion_model(&config, &secrets).expect("should succeed");
        assert_eq!(key, "gh-models-test-key");
    }

    #[test]
    fn test_create_completion_model_missing_key_returns_error() {
        let secrets = empty_secrets();
        let config = LlmRoleConfig {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            reasoning_effort: None,
            base_url: None,
        };
        let result = create_completion_model(&config, &secrets);
        assert!(result.is_err());
    }
}
