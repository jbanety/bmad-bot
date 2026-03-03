//! Integration tests for the config loading → validation → secrets pipeline.
//!
//! Covers: `BotConfig::load`, `BotConfig::validate`, `BotSecrets::validate_for_config`,
//! `BmadDiscovery::discover`, and `build_http_client`.

use std::path::{Path, PathBuf};

use bmad_bot::config::discovery::BmadDiscovery;
use bmad_bot::config::{build_http_client, BotConfig, BotSecrets, ConfigError};

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

// ===========================================================================
// Task 2 — Valid config round-trip (AC #1)
// ===========================================================================

#[test]
fn test_config_valid_roundtrip_succeeds() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let path = write_valid_config_yaml(tmp.path());

    // Act
    let loaded = BotConfig::load(&path).expect("load");
    let validate_result = loaded.validate();

    // Assert
    assert!(validate_result.is_ok(), "valid config should pass validation");
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
    assert!(
        result.is_ok(),
        "valid secrets should pass validation for valid config"
    );
}

// ===========================================================================
// Task 3 — Invalid config rejection (AC #2)
// ===========================================================================

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
polling_interval_secs: 30
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
polling_interval_secs: 30
git_provider:
  provider: github
  repo_owner: test
  repo_name: test
llm:
  dev: { provider: deepseek, model: test }
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
polling_interval_secs: 30
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
    let bad_yaml = "{{{{not: valid: yaml: at all\n  - broken: [";
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
    let path = Path::new("/tmp/nonexistent-bmad-bot-config-file-abc123.yaml");

    // Act
    let err = BotConfig::load(path).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::FileRead { .. }),
        "expected FileRead, got: {err:?}"
    );
}

#[test]
fn test_config_error_messages_contain_field_names() {
    // Arrange — zero polling
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

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();

    // Assert — the error message contains the field name
    let msg = err.to_string();
    assert!(
        msg.contains("polling_interval_secs"),
        "error message should contain field name, got: {msg}"
    );
}

// ===========================================================================
// Task 4 — Secrets validation (AC #3)
// ===========================================================================

#[test]
fn test_secrets_missing_anthropic_key_rejected() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path()); // uses anthropic as LLM provider
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
    // Arrange — config uses github as git provider
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
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
fn test_secrets_missing_telegram_token_rejected_when_enabled() {
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
fn test_secrets_error_messages_contain_env_var_name() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    let secrets = BotSecrets {
        anthropic_api_key: None,
        ..make_test_secrets()
    };

    // Act
    let err = secrets.validate_for_config(&config).unwrap_err();
    let msg = err.to_string();

    // Assert
    assert!(
        msg.contains("ANTHROPIC_API_KEY"),
        "error message should contain env var name, got: {msg}"
    );
}

// ===========================================================================
// Task 5 — BMAD discovery (AC #4)
// ===========================================================================

#[test]
fn test_discovery_valid_bmad_directory() {
    // Arrange — full _bmad/ structure with version comment
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
    let discovery = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(discovery.bmad_detected, "bmad should be detected");
    assert!(
        !discovery.installed_modules.is_empty(),
        "should find installed modules"
    );
    assert!(
        discovery.installed_modules.contains(&"bmm".to_string()),
        "should detect bmm module"
    );
    assert!(
        discovery.installed_modules.contains(&"core".to_string()),
        "should detect core module"
    );
    assert_eq!(
        discovery.bmad_version.as_deref(),
        Some("6.0.0-Beta.7"),
        "should extract version from config.yaml"
    );
}

#[test]
fn test_discovery_no_bmad_directory() {
    // Arrange — empty temp dir, no _bmad/
    let tmp = tempfile::tempdir().unwrap();

    // Act
    let discovery = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(!discovery.bmad_detected, "bmad should NOT be detected");
    assert!(
        discovery.installed_modules.is_empty(),
        "no modules should be found"
    );
    assert!(
        discovery.bmad_version.is_none(),
        "no version should be found"
    );
}

#[test]
fn test_discovery_partial_bmad_no_config() {
    // Arrange — _bmad/ exists but no config.yaml
    let tmp = tempfile::tempdir().unwrap();
    let bmad_dir = tmp.path().join("_bmad");
    std::fs::create_dir_all(bmad_dir.join("core")).unwrap();

    // Act
    let discovery = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(discovery.bmad_detected, "bmad directory should be detected");
    assert!(
        discovery.installed_modules.contains(&"core".to_string()),
        "should detect core module"
    );
    assert!(
        discovery.bmad_version.is_none(),
        "no version without config.yaml"
    );
}

// ===========================================================================
// Task 6 — HTTP client builder (AC #5)
// ===========================================================================

#[test]
fn test_build_http_client_succeeds() {
    // Act — should not panic
    let _client: reqwest_middleware::ClientWithMiddleware = build_http_client();
}
