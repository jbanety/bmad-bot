//! Integration tests for config loading, validation, secrets, BMAD discovery, and HTTP client.
//!
//! Story 7.2 — Config → Startup Validation Integration Tests.

use std::path::{Path, PathBuf};

use bmad_bot::config::discovery::BmadDiscovery;
use bmad_bot::config::{build_http_client, BotConfig, ConfigError};

use crate::helpers::fixtures::{make_test_config, make_test_secrets};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write a valid `BotConfig` YAML to a temp directory and return the file path.
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
    assert!(validate_result.is_ok(), "validation failed: {validate_result:?}");
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
    assert!(result.is_ok(), "secrets validation failed: {result:?}");
}

// ---------------------------------------------------------------------------
// Task 3: Invalid config rejection (AC #2)
// ---------------------------------------------------------------------------

#[test]
fn test_config_zero_polling_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let bad_yaml = r#"
polling_interval_secs: 0
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
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    // Act
    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "polling_interval_secs"),
        "expected InvalidField for polling_interval_secs, got: {err:?}"
    );
}

#[test]
fn test_config_unknown_git_provider_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let bad_yaml = r#"
polling_interval_secs: 60
git_provider:
  provider: bitbucket
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
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    // Act
    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "git_provider.provider"),
        "expected InvalidField for git_provider.provider, got: {err:?}"
    );
}

#[test]
fn test_config_unknown_llm_provider_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let bad_yaml = r#"
polling_interval_secs: 60
git_provider:
  provider: github
  repo_owner: test
  repo_name: test
llm:
  dev: { provider: llama, model: test }
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
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    // Act
    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "llm.dev.provider"),
        "expected InvalidField for llm.dev.provider, got: {err:?}"
    );
}

#[test]
fn test_config_empty_project_root_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let bad_yaml = r#"
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
  project_root: ""
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    // Act
    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::MissingField { ref field } if field == "bmad_paths.project_root"),
        "expected MissingField for bmad_paths.project_root, got: {err:?}"
    );
}

#[test]
fn test_config_invalid_yaml_syntax_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let bad_yaml = "this is not: [valid: yaml: {{{";
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    // Act
    let err = BotConfig::load(&path).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::YamlParse(_)),
        "expected YamlParse, got: {err:?}"
    );
}

#[test]
fn test_config_load_nonexistent_file_rejected() {
    // Arrange
    let path = Path::new("/tmp/this-path-does-not-exist-bmad-test/bmad-bot.yaml");

    // Act
    let err = BotConfig::load(path).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::FileRead { .. }),
        "expected FileRead, got: {err:?}"
    );
}

#[test]
fn test_config_errors_contain_field_names() {
    // Verify each error type contains the offending field name in its message
    let tmp = tempfile::tempdir().unwrap();

    // Zero polling → InvalidField with "polling_interval_secs"
    let bad_yaml = r#"
polling_interval_secs: 0
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
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");
    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("polling_interval_secs"),
        "error message should contain field name, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Task 4: Secrets validation (AC #3)
// ---------------------------------------------------------------------------

#[test]
fn test_secrets_missing_anthropic_key_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path()); // uses anthropic for all roles
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
    // Arrange — config uses github git provider
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
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
    // Arrange
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
    // Verify each missing-secret error contains the expected env var
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    let mut secrets = make_test_secrets();
    secrets.anthropic_api_key = None;

    let err = secrets.validate_for_config(&config).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ANTHROPIC_API_KEY"),
        "error should contain env var name, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Task 5: BMAD discovery (AC #4)
// ---------------------------------------------------------------------------

#[test]
fn test_discovery_valid_bmad_directory() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Create full _bmad structure
    std::fs::create_dir_all(root.join("_bmad/bmm")).unwrap();
    std::fs::create_dir_all(root.join("_bmad/core")).unwrap();
    std::fs::write(
        root.join("_bmad/bmm/config.yaml"),
        "# Version: 6.0.0-Beta.7\nproject_name: test\n",
    )
    .unwrap();

    // Act
    let result = BmadDiscovery::discover(root);

    // Assert
    assert!(result.bmad_detected, "expected bmad_detected: true");
    assert!(
        result.installed_modules.contains(&"bmm".to_string()),
        "expected 'bmm' in installed_modules: {:?}",
        result.installed_modules
    );
    assert!(
        result.installed_modules.contains(&"core".to_string()),
        "expected 'core' in installed_modules: {:?}",
        result.installed_modules
    );
    assert_eq!(
        result.bmad_version.as_deref(),
        Some("6.0.0-Beta.7"),
        "expected version 6.0.0-Beta.7"
    );
}

#[test]
fn test_discovery_no_bmad_directory() {
    // Arrange — empty dir, no _bmad
    let tmp = tempfile::tempdir().unwrap();

    // Act
    let result = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(!result.bmad_detected, "expected bmad_detected: false");
    assert!(
        result.installed_modules.is_empty(),
        "expected empty modules: {:?}",
        result.installed_modules
    );
}

#[test]
fn test_discovery_partial_bmad_no_config() {
    // Arrange — _bmad dir exists but no config.yaml
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("_bmad/bmm")).unwrap();
    std::fs::create_dir_all(root.join("_bmad/core")).unwrap();

    // Act
    let result = BmadDiscovery::discover(root);

    // Assert
    assert!(result.bmad_detected, "expected bmad_detected: true");
    assert!(
        result.bmad_version.is_none(),
        "expected no version without config.yaml, got: {:?}",
        result.bmad_version
    );
    assert!(
        !result.installed_modules.is_empty(),
        "expected some modules even without config"
    );
}

// ---------------------------------------------------------------------------
// Task 6: HTTP client builder (AC #5)
// ---------------------------------------------------------------------------

#[test]
fn test_http_client_builds_successfully() {
    // Act — should not panic; type is ClientWithMiddleware (binding verifies return)
    let _client = build_http_client();

    // Assert — if we reached here, build succeeded without panic
}
