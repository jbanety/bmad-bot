//! Configuration module — loads `bmad-bot.yaml` and `.env` secrets.
//!
//! Provides [`BotConfig`] for YAML-based daemon configuration and [`BotSecrets`]
//! for environment-variable-based secret loading via dotenvy. All validation is
//! performed through typed [`ConfigError`] variants (no `anyhow` in this module).

use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ---------------------------------------------------------------------------
// ConfigError
// ---------------------------------------------------------------------------

/// Typed error enum for configuration loading and validation failures.
///
/// Every variant carries enough context to produce a human-readable message
/// that pinpoints the exact field or environment variable that caused the error.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read the configuration file from disk.
    #[error("Failed to read config file '{path}': {source}")]
    FileRead {
        /// Path that was attempted.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// The YAML content could not be parsed into [`BotConfig`].
    #[error("Failed to parse config YAML: {0}")]
    YamlParse(#[from] serde_yml::Error),

    /// A field was present but contained an invalid value.
    #[error("Invalid config value for '{field}': {reason}")]
    InvalidField {
        /// Dotted field path (e.g. `"git_provider.provider"`).
        field: String,
        /// Human-readable explanation.
        reason: String,
    },

    /// A required field was missing from the configuration.
    #[error("Missing required config field: '{field}'")]
    MissingField {
        /// Dotted field path.
        field: String,
    },

    /// A required secret environment variable is not set.
    #[error(
        "Missing required secret: environment variable '{env_var}' not set (needed for {purpose})"
    )]
    MissingSecret {
        /// The expected environment variable name.
        env_var: String,
        /// What the secret is used for (e.g. `"Anthropic LLM provider"`).
        purpose: String,
    },

    /// The `.env` file could not be loaded by dotenvy.
    #[error("Failed to load .env file: {0}")]
    DotenvError(#[from] dotenvy::Error),
}

// ---------------------------------------------------------------------------
// BotConfig + nested structs
// ---------------------------------------------------------------------------

/// Top-level daemon configuration loaded from `bmad-bot.yaml`.
#[derive(Debug, Deserialize, Serialize)]
pub struct BotConfig {
    /// Polling interval in seconds. Must be > 0.
    #[serde(default = "default_polling_interval")]
    pub polling_interval_secs: u64,
    /// Git hosting provider settings.
    pub git_provider: GitProviderConfig,
    /// LLM provider configuration for each agent role.
    pub llm: LlmConfig,
    /// Notification channel configuration.
    pub notifications: NotificationConfig,
    /// Paths to BMAD project artifacts.
    pub bmad_paths: BmadPathsConfig,

    /// Log output format: `"json"` or `"pretty"`. Default: `"pretty"`.
    #[serde(default = "default_log_format")]
    pub log_format: String,

    /// Log level filter: `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`. Default: `"info"`.
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

/// Default polling interval — 5 minutes.
fn default_polling_interval() -> u64 {
    300
}

/// Default log format — human-readable pretty-print.
fn default_log_format() -> String {
    "pretty".to_string()
}

/// Default log level — info.
fn default_log_level() -> String {
    "info".to_string()
}

/// LLM provider configuration for each agent role.
#[derive(Debug, Deserialize, Serialize)]
pub struct LlmConfig {
    /// Provider + model for the dev agent (Amelia).
    pub dev: LlmRoleConfig,
    /// Provider + model for the code-review agent.
    pub review: LlmRoleConfig,
    /// Provider + model for the supervisor fallback.
    pub supervisor: LlmRoleConfig,
}

/// Provider + model pair for a single LLM role.
#[derive(Debug, Deserialize, Serialize)]
pub struct LlmRoleConfig {
    /// One of: `"anthropic"`, `"openai"`, `"github-models"`.
    pub provider: String,
    /// Model identifier, e.g. `"claude-sonnet-4-20250514"`, `"gpt-4o"`.
    pub model: String,
}

/// Git hosting provider configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct GitProviderConfig {
    /// One of: `"github"`, `"gitlab"`.
    pub provider: String,
    /// Repository owner / organisation.
    pub repo_owner: String,
    /// Repository name.
    pub repo_name: String,
    /// Branch that PRs target. Defaults to `"main"`.
    #[serde(default = "default_target_branch")]
    pub target_branch: String,
}

/// Default target branch for PRs.
fn default_target_branch() -> String {
    "main".to_string()
}

/// Notification channel configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct NotificationConfig {
    /// Telegram notification settings.
    pub telegram: TelegramConfig,
}

/// Telegram notification settings.
#[derive(Debug, Deserialize, Serialize)]
pub struct TelegramConfig {
    /// Whether Telegram notifications are enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Telegram chat ID to send notifications to.
    #[serde(default)]
    pub chat_id: String,
}

/// Paths to BMAD project artifacts.
#[derive(Debug, Deserialize, Serialize)]
pub struct BmadPathsConfig {
    /// Root of the project repository.
    pub project_root: String,
    /// BMAD output folder.
    pub output_folder: String,
    /// Path to planning artifacts.
    pub planning_artifacts: String,
    /// Path to implementation artifacts (stories, sprint-status).
    pub implementation_artifacts: String,
}

// ---------------------------------------------------------------------------
// BotConfig — loading & validation
// ---------------------------------------------------------------------------

/// Recognised git provider identifiers.
const VALID_GIT_PROVIDERS: &[&str] = &["github", "gitlab"];

/// Recognised LLM provider identifiers.
const VALID_LLM_PROVIDERS: &[&str] = &["anthropic", "openai", "github-models"];

impl BotConfig {
    /// Loads and deserializes a [`BotConfig`] from a YAML file at `path`.
    ///
    /// This does **not** call [`validate`](Self::validate) automatically so
    /// callers can inspect the raw deserialized values first if needed.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|source| ConfigError::FileRead {
            path: path.display().to_string(),
            source,
        })?;
        let config: Self = serde_yml::from_str(&content)?;
        Ok(config)
    }

    /// Validates all fields in the configuration.
    ///
    /// Returns `Ok(())` when every field passes, or the **first**
    /// [`ConfigError`] encountered.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // polling_interval_secs > 0
        if self.polling_interval_secs == 0 {
            return Err(ConfigError::InvalidField {
                field: "polling_interval_secs".to_string(),
                reason: "must be greater than 0".to_string(),
            });
        }

        // log_format
        let valid_log_formats = ["json", "pretty"];
        if !valid_log_formats.contains(&self.log_format.as_str()) {
            return Err(ConfigError::InvalidField {
                field: "log_format".to_string(),
                reason: format!("must be one of: {}", valid_log_formats.join(", ")),
            });
        }

        // log_level
        let valid_log_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_log_levels.contains(&self.log_level.as_str()) {
            return Err(ConfigError::InvalidField {
                field: "log_level".to_string(),
                reason: format!("must be one of: {}", valid_log_levels.join(", ")),
            });
        }

        // git_provider.provider
        if !VALID_GIT_PROVIDERS.contains(&self.git_provider.provider.as_str()) {
            return Err(ConfigError::InvalidField {
                field: "git_provider.provider".to_string(),
                reason: format!(
                    "unknown provider '{}'; expected one of: {}",
                    self.git_provider.provider,
                    VALID_GIT_PROVIDERS.join(", ")
                ),
            });
        }

        // LLM providers
        self.validate_llm_role("llm.dev", &self.llm.dev)?;
        self.validate_llm_role("llm.review", &self.llm.review)?;
        self.validate_llm_role("llm.supervisor", &self.llm.supervisor)?;

        // Required paths must be non-empty
        self.validate_non_empty("bmad_paths.project_root", &self.bmad_paths.project_root)?;
        self.validate_non_empty("bmad_paths.output_folder", &self.bmad_paths.output_folder)?;
        self.validate_non_empty(
            "bmad_paths.planning_artifacts",
            &self.bmad_paths.planning_artifacts,
        )?;
        self.validate_non_empty(
            "bmad_paths.implementation_artifacts",
            &self.bmad_paths.implementation_artifacts,
        )?;

        Ok(())
    }

    /// Validates that a single LLM role has a recognised provider.
    fn validate_llm_role(
        &self,
        field_prefix: &str,
        role: &LlmRoleConfig,
    ) -> Result<(), ConfigError> {
        if !VALID_LLM_PROVIDERS.contains(&role.provider.as_str()) {
            return Err(ConfigError::InvalidField {
                field: format!("{field_prefix}.provider"),
                reason: format!(
                    "unknown provider '{}'; expected one of: {}",
                    role.provider,
                    VALID_LLM_PROVIDERS.join(", ")
                ),
            });
        }
        Ok(())
    }

    /// Validates that a string field is non-empty.
    fn validate_non_empty(&self, field: &str, value: &str) -> Result<(), ConfigError> {
        if value.trim().is_empty() {
            return Err(ConfigError::MissingField {
                field: field.to_string(),
            });
        }
        Ok(())
    }

    /// Creates a minimal `BotConfig` for CLI/tracing tests.
    /// Not public API — only used by `cli::tests`.
    #[doc(hidden)]
    pub fn _test_minimal(log_format: &str, log_level: &str) -> Self {
        Self {
            polling_interval_secs: 300,
            git_provider: GitProviderConfig {
                provider: "github".to_string(),
                repo_owner: "test".to_string(),
                repo_name: "test".to_string(),
                target_branch: "main".to_string(),
            },
            llm: LlmConfig {
                dev: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test".to_string(),
                },
                review: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test".to_string(),
                },
                supervisor: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test".to_string(),
                },
            },
            notifications: NotificationConfig {
                telegram: TelegramConfig {
                    enabled: false,
                    chat_id: String::new(),
                },
            },
            bmad_paths: BmadPathsConfig {
                project_root: ".".to_string(),
                output_folder: "_bmad-output".to_string(),
                planning_artifacts: "_bmad-output/planning-artifacts".to_string(),
                implementation_artifacts: "_bmad-output/implementation-artifacts".to_string(),
            },
            log_format: log_format.to_string(),
            log_level: log_level.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// BotSecrets
// ---------------------------------------------------------------------------

/// Secrets loaded from `.env` file — NEVER stored in [`BotConfig`] or logged.
#[derive(Debug)]
pub struct BotSecrets {
    /// Anthropic API key (`ANTHROPIC_API_KEY`).
    pub anthropic_api_key: Option<String>,
    /// OpenAI API key (`OPENAI_API_KEY`).
    pub openai_api_key: Option<String>,
    /// GitHub Models API key (`GITHUB_MODELS_API_KEY`).
    pub github_models_api_key: Option<String>,
    /// GitHub personal-access token (`GITHUB_TOKEN`).
    pub github_token: Option<String>,
    /// GitLab personal-access token (`GITLAB_TOKEN`).
    pub gitlab_token: Option<String>,
    /// Telegram bot token (`TELEGRAM_BOT_TOKEN`).
    pub telegram_bot_token: Option<String>,
}

impl BotSecrets {
    /// Loads secrets from environment variables.
    ///
    /// Calls `dotenvy::dotenv()` first so that a `.env` file (if present) is
    /// sourced. Missing variables become `None` — call
    /// [`validate`](Self::validate_for_config) afterwards to ensure required
    /// secrets are present for the active providers.
    pub fn load() -> Result<Self, ConfigError> {
        // `.env` may not exist (e.g. in CI); that is fine.
        let _ = dotenvy::dotenv();

        Ok(Self {
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            github_models_api_key: std::env::var("GITHUB_MODELS_API_KEY").ok(),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            gitlab_token: std::env::var("GITLAB_TOKEN").ok(),
            telegram_bot_token: std::env::var("TELEGRAM_BOT_TOKEN").ok(),
        })
    }

    /// Validates that the required secrets are set for the configured providers.
    ///
    /// Checks LLM provider keys, git provider tokens, and (if enabled)
    /// Telegram bot token.
    pub fn validate_for_config(&self, config: &BotConfig) -> Result<(), ConfigError> {
        // Collect all unique LLM providers in use
        let llm_roles: &[(&str, &LlmRoleConfig)] = &[
            ("dev", &config.llm.dev),
            ("review", &config.llm.review),
            ("supervisor", &config.llm.supervisor),
        ];

        for (role_name, role_config) in llm_roles {
            match role_config.provider.as_str() {
                "anthropic" => {
                    if self.anthropic_api_key.as_ref().is_none_or(|k| k.is_empty()) {
                        return Err(ConfigError::MissingSecret {
                            env_var: "ANTHROPIC_API_KEY".to_string(),
                            purpose: format!("Anthropic LLM provider (llm.{role_name})"),
                        });
                    }
                }
                "openai" => {
                    if self.openai_api_key.as_ref().is_none_or(|k| k.is_empty()) {
                        return Err(ConfigError::MissingSecret {
                            env_var: "OPENAI_API_KEY".to_string(),
                            purpose: format!("OpenAI LLM provider (llm.{role_name})"),
                        });
                    }
                }
                "github-models" => {
                    if self
                        .github_models_api_key
                        .as_ref()
                        .is_none_or(|k| k.is_empty())
                    {
                        return Err(ConfigError::MissingSecret {
                            env_var: "GITHUB_MODELS_API_KEY".to_string(),
                            purpose: format!("GitHub Models LLM provider (llm.{role_name})"),
                        });
                    }
                }
                _ => {} // Unknown provider — config validation catches this
            }
        }

        // Git provider token
        match config.git_provider.provider.as_str() {
            "github" => {
                if self.github_token.as_ref().is_none_or(|t| t.is_empty()) {
                    return Err(ConfigError::MissingSecret {
                        env_var: "GITHUB_TOKEN".to_string(),
                        purpose: "GitHub git provider".to_string(),
                    });
                }
            }
            "gitlab" => {
                if self.gitlab_token.as_ref().is_none_or(|t| t.is_empty()) {
                    return Err(ConfigError::MissingSecret {
                        env_var: "GITLAB_TOKEN".to_string(),
                        purpose: "GitLab git provider".to_string(),
                    });
                }
            }
            _ => {}
        }

        // Telegram
        if config.notifications.telegram.enabled
            && self
                .telegram_bot_token
                .as_ref()
                .is_none_or(|t| t.is_empty())
        {
            return Err(ConfigError::MissingSecret {
                env_var: "TELEGRAM_BOT_TOKEN".to_string(),
                purpose: "Telegram notifications".to_string(),
            });
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HTTP client with retry middleware
// ---------------------------------------------------------------------------

/// Builds a shared HTTP client with automatic retry on transient errors
/// (429, 500, 503, timeouts). Max 3 retries with exponential backoff.
///
/// **ALL** HTTP calls in the project **MUST** use this client.
pub fn build_http_client() -> ClientWithMiddleware {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);

    ClientBuilder::new(reqwest::Client::new())
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Minimal valid YAML that deserializes into a complete [`BotConfig`].
    const VALID_YAML: &str = r#"
polling_interval_secs: 60
log_format: pretty
log_level: info
git_provider:
  provider: github
  repo_owner: test-org
  repo_name: test-repo
llm:
  dev:
    provider: anthropic
    model: claude-sonnet-4-20250514
  review:
    provider: anthropic
    model: claude-sonnet-4-20250514
  supervisor:
    provider: openai
    model: gpt-4o
notifications:
  telegram:
    enabled: false
    chat_id: ""
bmad_paths:
  project_root: "."
  output_folder: "_bmad-output"
  planning_artifacts: "_bmad-output/planning-artifacts"
  implementation_artifacts: "_bmad-output/implementation-artifacts"
"#;

    // -----------------------------------------------------------------------
    // log_format / log_level tests (Story 1.2)
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_validate_rejects_invalid_log_format() {
        let mut config = valid_config();
        config.log_format = "xml".to_string();
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::InvalidField { ref field, .. } if field == "log_format"),
            "Expected InvalidField for log_format, got: {err}"
        );
    }

    #[test]
    fn test_config_validate_rejects_invalid_log_level() {
        let mut config = valid_config();
        config.log_level = "verbose".to_string();
        let err = config.validate().unwrap_err();
        assert!(
            matches!(err, ConfigError::InvalidField { ref field, .. } if field == "log_level"),
            "Expected InvalidField for log_level, got: {err}"
        );
    }

    #[test]
    fn test_config_default_log_format_is_pretty() {
        // YAML without log_format — should default to "pretty"
        let yaml = VALID_YAML.replace("log_format: pretty\n", "");
        let config: BotConfig = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(config.log_format, "pretty");
    }

    #[test]
    fn test_config_default_log_level_is_info() {
        // YAML without log_level — should default to "info"
        let yaml = VALID_YAML.replace("log_level: info\n", "");
        let config: BotConfig = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(config.log_level, "info");
    }

    #[test]
    fn test_config_log_format_json_accepted() {
        let mut config = valid_config();
        config.log_format = "json".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_log_level_all_valid_values() {
        for level in ["trace", "debug", "info", "warn", "error"] {
            let mut config = valid_config();
            config.log_level = level.to_string();
            assert!(
                config.validate().is_ok(),
                "Expected log_level '{level}' to be valid"
            );
        }
    }

    #[test]
    fn test_secrets_validate_for_config_missing_anthropic_key() {
        let config = valid_config();
        let secrets = BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_models_api_key: None,
            github_token: Some("ghp_test".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        };
        let err = secrets.validate_for_config(&config).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "ANTHROPIC_API_KEY"),
            "Expected MissingSecret for ANTHROPIC_API_KEY, got: {err}"
        );
    }

    #[test]
    fn test_secrets_validate_for_config_missing_github_token() {
        let config = valid_config();
        let secrets = BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: Some("sk-openai-test".to_string()),
            github_models_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        };
        let err = secrets.validate_for_config(&config).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "GITHUB_TOKEN"),
            "Expected MissingSecret for GITHUB_TOKEN, got: {err}"
        );
    }

    #[test]
    fn test_secrets_validate_for_config_telegram_not_required_when_disabled() {
        let mut config = valid_config();
        config.notifications.telegram.enabled = false;
        let secrets = BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: Some("sk-openai-test".to_string()),
            github_models_api_key: None,
            github_token: Some("ghp_test".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        };
        assert!(
            secrets.validate_for_config(&config).is_ok(),
            "Telegram token should not be required when notifications are disabled"
        );
    }

    /// Helper — parse YAML string into BotConfig.
    fn config_from_str(yaml: &str) -> Result<BotConfig, ConfigError> {
        Ok(serde_yml::from_str(yaml)?)
    }

    /// Helper — build a known-valid BotConfig for mutation-based tests.
    fn valid_config() -> BotConfig {
        config_from_str(VALID_YAML).expect("VALID_YAML must parse")
    }

    // ---- Task 7.1: Valid config loads and deserializes correctly ----------

    #[test]
    fn test_config_load_valid_yaml() {
        let config: BotConfig = serde_yml::from_str(VALID_YAML).unwrap();

        assert_eq!(config.polling_interval_secs, 60);
        assert_eq!(config.git_provider.provider, "github");
        assert_eq!(config.git_provider.repo_owner, "test-org");
        assert_eq!(config.git_provider.repo_name, "test-repo");
        assert_eq!(config.git_provider.target_branch, "main"); // default
        assert_eq!(config.llm.dev.provider, "anthropic");
        assert_eq!(config.llm.supervisor.provider, "openai");
        assert_eq!(config.notifications.telegram.enabled, false);
        assert_eq!(config.bmad_paths.project_root, ".");
    }

    #[test]
    fn test_config_load_from_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(VALID_YAML.as_bytes()).unwrap();

        let config = BotConfig::load(tmp.path()).unwrap();
        assert_eq!(config.polling_interval_secs, 60);
        assert_eq!(config.git_provider.provider, "github");
    }

    #[test]
    fn test_config_load_nonexistent_file() {
        let result = BotConfig::load(Path::new("/tmp/does-not-exist-bmad-bot.yaml"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::FileRead { .. }));
    }

    #[test]
    fn test_config_valid_passes_validation() {
        let config = valid_config();
        assert!(config.validate().is_ok());
    }

    // ---- Task 7.2: Missing required field returns descriptive error -------

    #[test]
    fn test_config_missing_git_provider_field() {
        let yaml = r#"
polling_interval_secs: 60
llm:
  dev:
    provider: anthropic
    model: m
  review:
    provider: anthropic
    model: m
  supervisor:
    provider: openai
    model: m
notifications:
  telegram:
    enabled: false
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "p"
  implementation_artifacts: "i"
"#;
        let result: Result<BotConfig, _> = serde_yml::from_str(yaml);
        assert!(
            result.is_err(),
            "missing git_provider should fail deserialization"
        );
    }

    // ---- Task 7.3: Invalid polling_interval (0) returns error -------------

    #[test]
    fn test_config_validate_rejects_zero_polling_interval() {
        let yaml = VALID_YAML.replace("polling_interval_secs: 60", "polling_interval_secs: 0");
        let config: BotConfig = serde_yml::from_str(&yaml).unwrap();
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::InvalidField { ref field, .. } => {
                assert_eq!(field, "polling_interval_secs");
            }
            other => panic!("expected InvalidField, got: {other}"),
        }
    }

    // ---- Task 7.4: Unknown git provider returns error ---------------------

    #[test]
    fn test_config_validate_rejects_unknown_git_provider() {
        let yaml = VALID_YAML.replace("provider: github", "provider: bitbucket");
        let config: BotConfig = serde_yml::from_str(&yaml).unwrap();
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::InvalidField { ref field, .. } => {
                assert_eq!(field, "git_provider.provider");
            }
            other => panic!("expected InvalidField for git_provider.provider, got: {other}"),
        }
    }

    // ---- Task 7.5: Unknown LLM provider returns error ---------------------

    #[test]
    fn test_config_validate_rejects_unknown_llm_provider() {
        let yaml = VALID_YAML.replace("provider: anthropic", "provider: gemini");
        let config: BotConfig = serde_yml::from_str(&yaml).unwrap();
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::InvalidField { ref field, .. } => {
                assert!(
                    field.starts_with("llm."),
                    "expected llm.*.provider field, got: {field}"
                );
            }
            other => panic!("expected InvalidField for llm provider, got: {other}"),
        }
    }

    // ---- Task 7.6: Secrets loading from env vars --------------------------

    #[test]
    fn test_secrets_load_returns_struct() {
        // BotSecrets::load() reads env vars — we just verify it doesn't panic
        // and returns a valid struct. Actual env var values are environment-dependent.
        let secrets = BotSecrets::load().unwrap();

        // The struct should be constructed (fields are all Option, so any combo is valid)
        // Just verify the Debug impl works (struct is well-formed)
        let _debug = format!("{secrets:?}");
    }

    #[test]
    fn test_secrets_struct_construction() {
        // Verify BotSecrets can be constructed with known values
        let secrets = BotSecrets {
            anthropic_api_key: Some("sk-test-key-12345".to_string()),
            openai_api_key: None,
            github_models_api_key: None,
            github_token: Some("ghp-token".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        };

        assert_eq!(
            secrets.anthropic_api_key.as_deref(),
            Some("sk-test-key-12345")
        );
        assert!(secrets.openai_api_key.is_none());
        assert_eq!(secrets.github_token.as_deref(), Some("ghp-token"));
    }

    #[test]
    fn test_secrets_validate_missing_required_key() {
        let config = valid_config();
        let secrets = BotSecrets {
            anthropic_api_key: None,
            openai_api_key: Some("sk-openai".to_string()),
            github_models_api_key: None,
            github_token: Some("ghp-token".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        };

        let err = secrets.validate_for_config(&config).unwrap_err();
        match err {
            ConfigError::MissingSecret { ref env_var, .. } => {
                assert_eq!(env_var, "ANTHROPIC_API_KEY");
            }
            other => panic!("expected MissingSecret, got: {other}"),
        }
    }

    #[test]
    fn test_secrets_validate_missing_github_token() {
        let config = valid_config();
        let secrets = BotSecrets {
            anthropic_api_key: Some("sk-ant".to_string()),
            openai_api_key: Some("sk-oai".to_string()),
            github_models_api_key: None,
            github_token: None, // missing!
            gitlab_token: None,
            telegram_bot_token: None,
        };

        let err = secrets.validate_for_config(&config).unwrap_err();
        match err {
            ConfigError::MissingSecret { ref env_var, .. } => {
                assert_eq!(env_var, "GITHUB_TOKEN");
            }
            other => panic!("expected MissingSecret for GITHUB_TOKEN, got: {other}"),
        }
    }

    #[test]
    fn test_secrets_validate_passes_when_all_present() {
        let config = valid_config();
        let secrets = BotSecrets {
            anthropic_api_key: Some("sk-ant".to_string()),
            openai_api_key: Some("sk-oai".to_string()),
            github_models_api_key: None,
            github_token: Some("ghp-token".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        };

        assert!(secrets.validate_for_config(&config).is_ok());
    }

    #[test]
    fn test_secrets_validate_telegram_token_required_when_enabled() {
        let yaml = VALID_YAML.replace("enabled: false", "enabled: true");
        let config: BotConfig = serde_yml::from_str(&yaml).unwrap();
        let secrets = BotSecrets {
            anthropic_api_key: Some("sk-ant".to_string()),
            openai_api_key: Some("sk-oai".to_string()),
            github_models_api_key: None,
            github_token: Some("ghp-token".to_string()),
            gitlab_token: None,
            telegram_bot_token: None, // missing!
        };

        let err = secrets.validate_for_config(&config).unwrap_err();
        match err {
            ConfigError::MissingSecret { ref env_var, .. } => {
                assert_eq!(env_var, "TELEGRAM_BOT_TOKEN");
            }
            other => panic!("expected MissingSecret for TELEGRAM_BOT_TOKEN, got: {other}"),
        }
    }

    // ---- Task 7.7: HTTP client builds with retry middleware ---------------

    #[test]
    fn test_http_client_builds_successfully() {
        let _client = build_http_client();
        // If we reach here without panic, the client built correctly.
    }

    // ---- Task 7.8: Default values are applied when optional fields omitted -

    #[test]
    fn test_config_default_values_applied() {
        let yaml = r#"
git_provider:
  provider: github
  repo_owner: test-org
  repo_name: test-repo
llm:
  dev:
    provider: anthropic
    model: claude-sonnet-4-20250514
  review:
    provider: anthropic
    model: claude-sonnet-4-20250514
  supervisor:
    provider: openai
    model: gpt-4o
notifications:
  telegram:
    enabled: false
bmad_paths:
  project_root: "."
  output_folder: "_bmad-output"
  planning_artifacts: "_bmad-output/planning-artifacts"
  implementation_artifacts: "_bmad-output/implementation-artifacts"
"#;
        let config: BotConfig = serde_yml::from_str(yaml).unwrap();

        // polling_interval_secs defaults to 300
        assert_eq!(config.polling_interval_secs, 300);
        // target_branch defaults to "main"
        assert_eq!(config.git_provider.target_branch, "main");
        // telegram.chat_id defaults to ""
        assert_eq!(config.notifications.telegram.chat_id, "");
    }

    // ---- Additional edge-case coverage -----------------------------------

    #[test]
    fn test_config_validate_rejects_empty_project_root() {
        let yaml = VALID_YAML.replace("project_root: \".\"", "project_root: \"\"");
        let config: BotConfig = serde_yml::from_str(&yaml).unwrap();
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::MissingField { ref field } => {
                assert_eq!(field, "bmad_paths.project_root");
            }
            other => panic!("expected MissingField for bmad_paths.project_root, got: {other}"),
        }
    }

    #[test]
    fn test_config_validate_rejects_whitespace_only_path() {
        let yaml = VALID_YAML.replace("output_folder: \"_bmad-output\"", "output_folder: \"   \"");
        let config: BotConfig = serde_yml::from_str(&yaml).unwrap();
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::MissingField { ref field } => {
                assert_eq!(field, "bmad_paths.output_folder");
            }
            other => panic!("expected MissingField for bmad_paths.output_folder, got: {other}"),
        }
    }

    #[test]
    fn test_config_invalid_yaml_returns_parse_error() {
        let bad_yaml = "this: is: not: valid: yaml: [[[";
        let result: Result<BotConfig, ConfigError> =
            serde_yml::from_str::<BotConfig>(bad_yaml).map_err(ConfigError::from);
        assert!(matches!(result, Err(ConfigError::YamlParse(_))));
    }

    #[test]
    fn test_config_validate_all_three_llm_roles_checked() {
        // Only the supervisor has a bad provider — verify validation catches it
        let yaml = r#"
polling_interval_secs: 60
git_provider:
  provider: github
  repo_owner: test-org
  repo_name: test-repo
llm:
  dev:
    provider: anthropic
    model: m
  review:
    provider: openai
    model: m
  supervisor:
    provider: bad-provider
    model: m
notifications:
  telegram:
    enabled: false
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "p"
  implementation_artifacts: "i"
"#;
        let config: BotConfig = serde_yml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::InvalidField { ref field, .. } => {
                assert_eq!(field, "llm.supervisor.provider");
            }
            other => panic!("expected InvalidField for llm.supervisor.provider, got: {other}"),
        }
    }

    #[test]
    fn test_config_github_models_provider_accepted() {
        let yaml = VALID_YAML.replace("provider: openai", "provider: github-models");
        let config: BotConfig = serde_yml::from_str(&yaml).unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_gitlab_provider_accepted() {
        let yaml = VALID_YAML
            .replace("provider: github", "provider: gitlab")
            .replacen("provider: gitlab", "provider: gitlab", 1);
        // Only the first replacement matters (git_provider); LLM providers stay valid
        let reparsed = yaml.replace(
            "git_provider:\n  provider: gitlab",
            "git_provider:\n  provider: gitlab",
        );
        let config: BotConfig = serde_yml::from_str(&reparsed).unwrap();
        assert!(config.validate().is_ok());
    }
}
