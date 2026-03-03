//! Integration tests for config loading, validation, and startup pipeline.
//!
//! Covers: BotConfig::load → validate, BotSecrets::validate_for_config,
//! BmadDiscovery::discover, and build_http_client.

use std::path::{Path, PathBuf};

use bmad_bot::config::{build_http_client, BotConfig, BotSecrets, ConfigError};
use bmad_bot::config::discovery::BmadDiscovery;

use crate::helpers::fixtures::{make_test_config, make_test_secrets};

// ---------------------------------------------------------------------------
// Helper: write a valid BotConfig YAML to a temp directory
// ---------------------------------------------------------------------------

/// Serialize a valid `BotConfig` to `{dir}/bmad-bot.yaml` and return the path.
fn write_valid_config_yaml(dir: &Path) -> PathBuf {
    let config = make_test_config(dir);
    let yaml = serde_yml::to_string(&config).expect("serialize");
    let path = dir.join("bmad-bot.yaml");
    std::fs::write(&path, &yaml).expect("write");
    path
}

// ---------------------------------------------------------------------------
// Task 2: Valid config round-trip (AC #1)
// ---------------------------------------------------------------------------

#[test]
fn test_config_valid_roundtrip_succeeds() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let path = write_valid_config_yaml(tmp.path());

    // Act
    let loaded = BotConfig::load(&path).expect("load");
    let validate_result = loaded.validate();

    // Assert
    assert!(validate_result.is_ok(), "validate failed: {validate_result:?}");
}

#[test]
fn test_config_valid_roundtrip_secrets_validate() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let path = write_valid_config_yaml(tmp.path());
    let config = BotConfig::load(&path).expect("load");
    config.validate().expect("validate");
    let secrets = make_test_secrets();

    // Act
    let result = secrets.validate_for_config(&config);

    // Assert
    assert!(result.is_ok(), "secrets validate_for_config failed: {result:?}");
}

// ---------------------------------------------------------------------------
// Task 3: Invalid config rejection tests (AC #2)
// ---------------------------------------------------------------------------

/// Helper: write raw YAML string to `{dir}/bmad-bot.yaml`.
fn write_raw_yaml(dir: &Path, yaml: &str) -> PathBuf {
    let path = dir.join("bmad-bot.yaml");
    std::fs::write(&path, yaml).expect("write");
    path
}

/// Minimal valid YAML template — modify one field per test.
const BASE_YAML: &str = r#"
polling_interval_secs: 60
git_provider:
  provider: github
  repo_owner: test
  repo_name: test
llm:
  dev: { provider: anthropic, model: test }
  review: { provider: anthropic, model: test }
  supervisor: { provider: anthropic, model: test }
notifications:
  telegram: { enabled: false }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
"#;

#[test]
fn test_config_zero_polling_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let yaml = BASE_YAML.replace("polling_interval_secs: 60", "polling_interval_secs: 0");
    let path = write_raw_yaml(tmp.path(), &yaml);

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();

    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "polling_interval_secs"),
        "expected InvalidField for polling_interval_secs, got: {err:?}"
    );
}

#[test]
fn test_config_unknown_git_provider_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let yaml = BASE_YAML.replace("provider: github", "provider: bitbucket");
    let path = write_raw_yaml(tmp.path(), &yaml);

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();

    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "git_provider.provider"),
        "expected InvalidField for git_provider.provider, got: {err:?}"
    );
}

#[test]
fn test_config_unknown_llm_provider_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let yaml = BASE_YAML.replace("provider: anthropic", "provider: deepseek");
    let path = write_raw_yaml(tmp.path(), &yaml);

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();

    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field.contains("provider")),
        "expected InvalidField for llm provider, got: {err:?}"
    );
}

#[test]
fn test_config_empty_project_root_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let yaml = BASE_YAML.replace("project_root: \".\"", "project_root: \"\"");
    let path = write_raw_yaml(tmp.path(), &yaml);

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();

    assert!(
        matches!(err, ConfigError::MissingField { ref field } if field == "bmad_paths.project_root"),
        "expected MissingField for bmad_paths.project_root, got: {err:?}"
    );
}

#[test]
fn test_config_invalid_yaml_syntax_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_raw_yaml(tmp.path(), "{{{{ invalid yaml ::::");

    let err = BotConfig::load(&path).unwrap_err();

    assert!(
        matches!(err, ConfigError::YamlParse(_)),
        "expected YamlParse, got: {err:?}"
    );
}

#[test]
fn test_config_load_nonexistent_file_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("does-not-exist.yaml");

    let err = BotConfig::load(&path).unwrap_err();

    assert!(
        matches!(err, ConfigError::FileRead { .. }),
        "expected FileRead, got: {err:?}"
    );
}

#[test]
fn test_config_error_messages_contain_field_names() {
    // Verify the error Display output contains the offending field name
    let tmp = tempfile::tempdir().unwrap();

    // Zero polling
    let yaml = BASE_YAML.replace("polling_interval_secs: 60", "polling_interval_secs: 0");
    let path = write_raw_yaml(tmp.path(), &yaml);
    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("polling_interval_secs"), "error msg should contain field name: {msg}");

    // Empty project_root
    let yaml2 = BASE_YAML.replace("project_root: \".\"", "project_root: \"\"");
    let path2 = write_raw_yaml(tmp.path(), &yaml2);
    let config2 = BotConfig::load(&path2).expect("load");
    let err2 = config2.validate().unwrap_err();
    let msg2 = err2.to_string();
    assert!(msg2.contains("project_root"), "error msg should contain field name: {msg2}");
}

// ---------------------------------------------------------------------------
// Task 4: Secrets validation tests (AC #3)
// ---------------------------------------------------------------------------

#[test]
fn test_secrets_missing_anthropic_key_rejected() {
    // Arrange: config uses anthropic (default), secrets has anthropic_api_key = None
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path()); // llm providers are all "anthropic"
    let mut secrets = make_test_secrets();
    secrets.anthropic_api_key = None;

    // Act
    let err = secrets.validate_for_config(&config).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "ANTHROPIC_API_KEY"),
        "expected MissingSecret for ANTHROPIC_API_KEY, got: {err:?}"
    );
}

#[test]
fn test_secrets_missing_github_token_rejected() {
    // Arrange: config uses github git provider, secrets has github_token = None
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path()); // git_provider.provider == "github"
    let mut secrets = make_test_secrets();
    secrets.github_token = None;

    // Act
    let err = secrets.validate_for_config(&config).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "GITHUB_TOKEN"),
        "expected MissingSecret for GITHUB_TOKEN, got: {err:?}"
    );
}

#[test]
fn test_secrets_missing_telegram_token_rejected_when_enabled() {
    // Arrange: config with telegram enabled, secrets has telegram_bot_token = None
    let tmp = tempfile::tempdir().unwrap();
    let mut config = make_test_config(tmp.path());
    config.notifications.telegram.enabled = true;
    config.notifications.telegram.chat_id = "12345".to_string();
    let mut secrets = make_test_secrets();
    secrets.telegram_bot_token = None;

    // Act
    let err = secrets.validate_for_config(&config).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "TELEGRAM_BOT_TOKEN"),
        "expected MissingSecret for TELEGRAM_BOT_TOKEN, got: {err:?}"
    );
}

#[test]
fn test_secrets_error_contains_env_var_name() {
    // Verify the error Display output contains the env var name
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    let mut secrets = make_test_secrets();
    secrets.anthropic_api_key = None;

    let err = secrets.validate_for_config(&config).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("ANTHROPIC_API_KEY"), "error msg should contain env var name: {msg}");
}

// ---------------------------------------------------------------------------
// Task 5: BMAD discovery integration tests (AC #4)
// ---------------------------------------------------------------------------

#[test]
fn test_discovery_detects_full_bmad_structure() {
    // Arrange: create _bmad/ with config.yaml + modules
    let tmp = tempfile::tempdir().unwrap();
    let bmad = tmp.path().join("_bmad");
    std::fs::create_dir_all(bmad.join("bmm")).unwrap();
    std::fs::create_dir_all(bmad.join("core")).unwrap();
    std::fs::write(
        bmad.join("bmm/config.yaml"),
        "# Version: 6.0.0-Beta.7\nproject_name: test\n",
    )
    .unwrap();

    // Act
    let result = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(result.bmad_detected, "bmad should be detected");
    assert_eq!(result.bmad_version.as_deref(), Some("6.0.0-Beta.7"));
    assert!(result.installed_modules.contains(&"bmm".to_string()));
    assert!(result.installed_modules.contains(&"core".to_string()));
    assert!(result.config_path.is_some());
}

#[test]
fn test_discovery_no_bmad_directory() {
    // Arrange: empty temp dir, no _bmad/
    let tmp = tempfile::tempdir().unwrap();

    // Act
    let result = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(!result.bmad_detected, "bmad should not be detected");
    assert!(result.installed_modules.is_empty());
    assert!(result.bmad_version.is_none());
}

#[test]
fn test_discovery_partial_bmad_no_config() {
    // Arrange: _bmad/ exists but no config.yaml
    let tmp = tempfile::tempdir().unwrap();
    let bmad = tmp.path().join("_bmad");
    std::fs::create_dir_all(bmad.join("core")).unwrap();

    // Act
    let result = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(result.bmad_detected, "bmad dir exists → detected");
    assert!(result.bmad_version.is_none(), "no config → no version");
    assert!(result.config_path.is_none());
    assert!(result.installed_modules.contains(&"core".to_string()));
}

// ---------------------------------------------------------------------------
// Task 6: HTTP client builder test (AC #5)
// ---------------------------------------------------------------------------

#[test]
fn test_build_http_client_returns_successfully() {
    // Act — should not panic
    let _client = build_http_client();
    // Assert: binding succeeds → type is ClientWithMiddleware
}
