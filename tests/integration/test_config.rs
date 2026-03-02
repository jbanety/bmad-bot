//! Integration tests for the config loading, validation, and secrets pipeline.
//!
//! Covers: `BotConfig::load()` → `validate()`, `BotSecrets::validate_for_config()`,
//! `BmadDiscovery::discover()`, and `build_http_client()`.

use std::path::{Path, PathBuf};

use bmad_bot::config::{build_http_client, BotConfig, BotSecrets, ConfigError};
use bmad_bot::config::discovery::BmadDiscovery;

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
// Task 2: Valid config round-trip (AC #1)
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
    assert!(validate_result.is_ok(), "validate failed: {validate_result:?}");
}

#[test]
fn test_config_valid_roundtrip_secrets_succeed() {
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

// ===========================================================================
// Task 3: Invalid config rejection (AC #2)
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
  target_branch: main
llm:
  dev: { provider: anthropic, model: test }
  review: { provider: anthropic, model: test }
  supervisor: { provider: anthropic, model: test }
notifications:
  telegram: { enabled: false, chat_id: "" }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_file: "bmad-bot.log"
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
  target_branch: main
llm:
  dev: { provider: anthropic, model: test }
  review: { provider: anthropic, model: test }
  supervisor: { provider: anthropic, model: test }
notifications:
  telegram: { enabled: false, chat_id: "" }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_file: "bmad-bot.log"
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
  target_branch: main
llm:
  dev: { provider: deepseek, model: test }
  review: { provider: anthropic, model: test }
  supervisor: { provider: anthropic, model: test }
notifications:
  telegram: { enabled: false, chat_id: "" }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_file: "bmad-bot.log"
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
  target_branch: main
llm:
  dev: { provider: anthropic, model: test }
  review: { provider: anthropic, model: test }
  supervisor: { provider: anthropic, model: test }
notifications:
  telegram: { enabled: false, chat_id: "" }
bmad_paths:
  project_root: ""
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_file: "bmad-bot.log"
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
    let bad_yaml = "{{{{not valid yaml at all:::";
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
fn test_config_load_nonexistent_file() {
    // Arrange — reference a path inside a fresh tempdir that was never created
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("this-file-does-not-exist.yaml");
    // Do NOT write anything — path must be absent

    // Act
    let err = BotConfig::load(&path).unwrap_err();

    // Assert
    assert!(
        matches!(err, ConfigError::FileRead { .. }),
        "expected FileRead, got: {err:?}"
    );
}

#[test]
fn test_config_errors_contain_field_names() {
    // Verify each error variant includes the offending field name in the Display output.
    let tmp = tempfile::tempdir().unwrap();

    // Zero polling → field = "polling_interval_secs"
    let bad_yaml = r#"
polling_interval_secs: 0
git_provider: { provider: github, repo_owner: t, repo_name: t, target_branch: main }
llm:
  dev: { provider: anthropic, model: t }
  review: { provider: anthropic, model: t }
  supervisor: { provider: anthropic, model: t }
notifications: { telegram: { enabled: false, chat_id: "" } }
bmad_paths: { project_root: ".", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
log_file: "bmad-bot.log"
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");
    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("polling_interval_secs"), "error message should contain field name: {msg}");

    // Unknown git provider → field = "git_provider.provider"
    let bad_yaml2 = bad_yaml.replace("polling_interval_secs: 0", "polling_interval_secs: 60")
        .replace("provider: github", "provider: bitbucket");
    std::fs::write(&path, bad_yaml2).expect("write");
    let config2 = BotConfig::load(&path).expect("load");
    let err2 = config2.validate().unwrap_err();
    let msg2 = err2.to_string();
    assert!(msg2.contains("git_provider.provider"), "error message should contain field name: {msg2}");

    // Empty project_root → field = "bmad_paths.project_root"
    let bad_yaml3 = r#"
polling_interval_secs: 60
git_provider: { provider: github, repo_owner: t, repo_name: t, target_branch: main }
llm:
  dev: { provider: anthropic, model: t }
  review: { provider: anthropic, model: t }
  supervisor: { provider: anthropic, model: t }
notifications: { telegram: { enabled: false, chat_id: "" } }
bmad_paths: { project_root: "", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
log_file: "bmad-bot.log"
"#;
    std::fs::write(&path, bad_yaml3).expect("write");
    let config3 = BotConfig::load(&path).expect("load");
    let err3 = config3.validate().unwrap_err();
    let msg3 = err3.to_string();
    assert!(msg3.contains("bmad_paths.project_root"), "error message should contain field name: {msg3}");
}

// ===========================================================================
// Task 4: Secrets validation (AC #3)
// ===========================================================================

#[test]
fn test_config_secrets_missing_anthropic_key() {
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
fn test_config_secrets_missing_github_token() {
    // Arrange — config uses github git provider
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
fn test_config_secrets_missing_telegram_token() {
    // Arrange — enable Telegram in config
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
fn test_config_secrets_errors_contain_env_var_name() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());

    // Missing anthropic key
    let secrets = BotSecrets {
        anthropic_api_key: None,
        ..make_test_secrets()
    };
    let err = secrets.validate_for_config(&config).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("ANTHROPIC_API_KEY"), "error should mention env var: {msg}");

    // Missing github token
    let secrets2 = BotSecrets {
        github_token: None,
        ..make_test_secrets()
    };
    let err2 = secrets2.validate_for_config(&config).unwrap_err();
    let msg2 = err2.to_string();
    assert!(msg2.contains("GITHUB_TOKEN"), "error should mention env var: {msg2}");

    // Missing telegram token (telegram must be enabled in config)
    let mut config_tg = make_test_config(tmp.path());
    config_tg.notifications.telegram.enabled = true;
    config_tg.notifications.telegram.chat_id = "12345".to_string();
    let secrets3 = BotSecrets {
        telegram_bot_token: None,
        ..make_test_secrets()
    };
    let err3 = secrets3.validate_for_config(&config_tg).unwrap_err();
    let msg3 = err3.to_string();
    assert!(msg3.contains("TELEGRAM_BOT_TOKEN"), "error should mention env var: {msg3}");
}

// ===========================================================================
// Task 5: BMAD discovery integration (AC #4)
// ===========================================================================

#[test]
fn test_config_discovery_detects_full_bmad_structure() {
    // Arrange — create _bmad/ with config.yaml containing version + known modules
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
    assert!(result.bmad_detected, "bmad_detected should be true");
    assert!(
        result.installed_modules.contains(&"bmm".to_string()),
        "should find bmm module: {:?}",
        result.installed_modules
    );
    assert!(
        result.installed_modules.contains(&"core".to_string()),
        "should find core module: {:?}",
        result.installed_modules
    );
    assert!(
        result.bmad_version.is_some(),
        "version should be extracted"
    );
    assert_eq!(
        result.bmad_version.as_deref(),
        Some("6.0.0-Beta.7"),
        "version mismatch"
    );
}

#[test]
fn test_config_discovery_no_bmad_directory() {
    // Arrange — empty temp directory
    let tmp = tempfile::tempdir().unwrap();

    // Act
    let result = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(!result.bmad_detected, "bmad_detected should be false");
    assert!(
        result.installed_modules.is_empty(),
        "modules should be empty: {:?}",
        result.installed_modules
    );
}

#[test]
fn test_config_discovery_partial_bmad_no_config() {
    // Arrange — _bmad/ exists with core/ but no config.yaml
    let tmp = tempfile::tempdir().unwrap();
    let bmad = tmp.path().join("_bmad");
    std::fs::create_dir_all(bmad.join("core")).unwrap();

    // Act
    let result = BmadDiscovery::discover(tmp.path());

    // Assert
    assert!(result.bmad_detected, "bmad_detected should be true (directory exists)");
    assert!(
        result.bmad_version.is_none(),
        "version should be None without config.yaml"
    );
}

// ===========================================================================
// Task 6: HTTP client builder (AC #5)
// ===========================================================================

#[test]
fn test_config_build_http_client_succeeds() {
    // Act — should not panic
    let _client: reqwest_middleware::ClientWithMiddleware = build_http_client();
    // If we reach here, the client was built successfully.
}
