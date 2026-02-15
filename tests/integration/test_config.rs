//! Integration tests for the config → startup validation pipeline.
//!
//! Covers: BotConfig load/validate, BotSecrets validate_for_config,
//! BmadDiscovery, and build_http_client.

use std::path::{Path, PathBuf};

use bmad_bot::config::{BotConfig, BotSecrets, ConfigError, build_http_client};
use bmad_bot::config::discovery::BmadDiscovery;

use crate::helpers::fixtures::{make_test_config, make_test_secrets};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write a valid BotConfig YAML to a temp directory and return the file path.
fn write_valid_config_yaml(dir: &Path) -> PathBuf {
    let config = make_test_config(dir);
    let yaml = serde_yml::to_string(&config).expect("serialize");
    let path = dir.join("bmad-bot.yaml");
    std::fs::write(&path, &yaml).expect("write");
    path
}

// ---------------------------------------------------------------------------
// Task 2 — Valid config round-trip (AC #1)
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
    assert!(validate_result.is_ok(), "validation should pass for valid config");
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
    assert!(result.is_ok(), "secrets validation should pass with all keys present");
}

// ---------------------------------------------------------------------------
// Task 3 — Invalid config rejection (AC #2)
// ---------------------------------------------------------------------------

#[test]
fn test_config_zero_polling_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let yaml = r#"
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
    std::fs::write(&path, yaml).expect("write");

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
    let yaml = r#"
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
    std::fs::write(&path, yaml).expect("write");

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
    let yaml = r#"
polling_interval_secs: 60
git_provider:
  provider: github
  repo_owner: test
  repo_name: test
llm:
  dev: { provider: unknown-ai, model: test }
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
    std::fs::write(&path, yaml).expect("write");

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
    let yaml = r#"
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
    std::fs::write(&path, yaml).expect("write");

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
    let bad_yaml = "{{{{not yaml at all: [[[";
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    // Act
    let err = BotConfig::load(&path).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::YamlParse(_)),
        "expected YamlParse error, got: {err:?}"
    );
}

#[test]
fn test_config_load_nonexistent_file_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("does-not-exist-bmad-bot-test-7-2.yaml");

    // Act
    let err = BotConfig::load(&path).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::FileRead { .. }),
        "expected FileRead error, got: {err:?}"
    );
}

#[test]
fn test_config_invalid_errors_contain_field_name() {
    // Verify that invalid-config errors include the offending field names.
    let tmp = tempfile::tempdir().unwrap();

    let cases = [
        (
            r#"
polling_interval_secs: 0
git_provider: { provider: github, repo_owner: t, repo_name: t }
llm:
  dev: { provider: anthropic, model: t }
  review: { provider: anthropic, model: t }
  supervisor: { provider: anthropic, model: t }
notifications: { telegram: { enabled: false } }
bmad_paths: { project_root: ".", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
"#,
            "polling_interval_secs",
        ),
        (
            r#"
polling_interval_secs: 60
git_provider: { provider: bitbucket, repo_owner: t, repo_name: t }
llm:
  dev: { provider: anthropic, model: t }
  review: { provider: anthropic, model: t }
  supervisor: { provider: anthropic, model: t }
notifications: { telegram: { enabled: false } }
bmad_paths: { project_root: ".", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
"#,
            "git_provider.provider",
        ),
        (
            r#"
polling_interval_secs: 60
git_provider: { provider: github, repo_owner: t, repo_name: t }
llm:
  dev: { provider: anthropic, model: t }
  review: { provider: anthropic, model: t }
  supervisor: { provider: anthropic, model: t }
notifications: { telegram: { enabled: false } }
bmad_paths: { project_root: "", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
"#,
            "bmad_paths.project_root",
        ),
    ];

    for (index, (yaml, expected_field)) in cases.iter().enumerate() {
        let path = tmp.path().join(format!("bmad-bot-{index}.yaml"));
        std::fs::write(&path, yaml).expect("write");

        let config = BotConfig::load(&path).expect("load");
        let err = config.validate().unwrap_err();
        let msg = format!("{err}");

        assert!(
            msg.contains(expected_field),
            "error message should contain field `{expected_field}`, got: {msg}"
        );
    }
}

// ---------------------------------------------------------------------------
// Task 4 — Secrets validation (AC #3)
// ---------------------------------------------------------------------------

#[test]
fn test_secrets_missing_anthropic_key_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path()); // uses anthropic for all LLM roles
    let secrets = BotSecrets {
        anthropic_api_key: None,
        ..make_test_secrets()
    };

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
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path()); // uses github git provider
    let secrets = BotSecrets {
        github_token: None,
        ..make_test_secrets()
    };

    // Act
    let err = secrets.validate_for_config(&config).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "GITHUB_TOKEN"),
        "expected MissingSecret for GITHUB_TOKEN, got: {err:?}"
    );
}

#[test]
fn test_secrets_missing_telegram_token_when_enabled() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let mut config = make_test_config(tmp.path());
    config.notifications.telegram.enabled = true;
    config.notifications.telegram.chat_id = "12345".to_string();
    let secrets = BotSecrets {
        telegram_bot_token: None,
        ..make_test_secrets()
    };

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
    // Verify that missing-secret errors include the expected env var names.
    let tmp = tempfile::tempdir().unwrap();

    // Anthropic key missing
    let anthropic_config = make_test_config(tmp.path());
    let anthropic_secrets = BotSecrets {
        anthropic_api_key: None,
        ..make_test_secrets()
    };
    let anthropic_err = anthropic_secrets
        .validate_for_config(&anthropic_config)
        .unwrap_err();
    assert!(
        format!("{anthropic_err}").contains("ANTHROPIC_API_KEY"),
        "error message should contain ANTHROPIC_API_KEY"
    );

    // Git provider token missing
    let github_config = make_test_config(tmp.path());
    let github_secrets = BotSecrets {
        github_token: None,
        ..make_test_secrets()
    };
    let github_err = github_secrets.validate_for_config(&github_config).unwrap_err();
    assert!(
        format!("{github_err}").contains("GITHUB_TOKEN"),
        "error message should contain GITHUB_TOKEN"
    );

    // Telegram token missing when notifications enabled
    let mut telegram_config = make_test_config(tmp.path());
    telegram_config.notifications.telegram.enabled = true;
    telegram_config.notifications.telegram.chat_id = "12345".to_string();
    let telegram_secrets = BotSecrets {
        telegram_bot_token: None,
        ..make_test_secrets()
    };
    let telegram_err = telegram_secrets
        .validate_for_config(&telegram_config)
        .unwrap_err();
    assert!(
        format!("{telegram_err}").contains("TELEGRAM_BOT_TOKEN"),
        "error message should contain TELEGRAM_BOT_TOKEN"
    );
}

// ---------------------------------------------------------------------------
// Task 5 — BMAD discovery (AC #4)
// ---------------------------------------------------------------------------

#[test]
fn test_discovery_detects_valid_bmad_directory() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let bmad_dir = tmp.path().join("_bmad");
    std::fs::create_dir_all(bmad_dir.join("bmm")).unwrap();
    std::fs::create_dir_all(bmad_dir.join("core")).unwrap();
    std::fs::write(
        bmad_dir.join("bmm/config.yaml"),
        "# Version: 6.0.0-Beta.7\nproject_name: test\n",
    )
    .unwrap();

    // Act
    let result = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(result.bmad_detected, "should detect _bmad directory");
    assert!(
        !result.installed_modules.is_empty(),
        "should find installed modules"
    );
    assert!(
        result.installed_modules.contains(&"bmm".to_string()),
        "should detect bmm module"
    );
    assert!(
        result.installed_modules.contains(&"core".to_string()),
        "should detect core module"
    );
    assert_eq!(
        result.bmad_version,
        Some("6.0.0-Beta.7".to_string()),
        "should extract version from config.yaml"
    );
}

#[test]
fn test_discovery_no_bmad_directory() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    // No _bmad/ directory created

    // Act
    let result = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(!result.bmad_detected, "should not detect _bmad");
    assert!(
        result.installed_modules.is_empty(),
        "modules should be empty"
    );
    assert!(result.bmad_version.is_none(), "version should be None");
}

#[test]
fn test_discovery_partial_bmad_no_config() {
    // Arrange — _bmad/ exists but no config.yaml
    let tmp = tempfile::tempdir().unwrap();
    let bmad_dir = tmp.path().join("_bmad");
    std::fs::create_dir_all(bmad_dir.join("bmm")).unwrap();
    std::fs::create_dir_all(bmad_dir.join("core")).unwrap();
    // No config.yaml written

    // Act
    let result = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(result.bmad_detected, "should still detect _bmad directory");
    assert!(result.bmad_version.is_none(), "version should be None without config.yaml");
    assert!(
        !result.installed_modules.is_empty(),
        "should still find module directories"
    );
}

// ---------------------------------------------------------------------------
// Task 6 — HTTP client builder (AC #5)
// ---------------------------------------------------------------------------

#[test]
fn test_http_client_builds_without_panic() {
    // Act
    let _client: reqwest_middleware::ClientWithMiddleware = build_http_client();

    // Assert — reaching here without panic is the test
}
