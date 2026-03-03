//! Integration tests for the config loading → validation pipeline.
//!
//! Covers: BotConfig load/validate round-trip, invalid config rejection,
//! BotSecrets validate_for_config, BmadDiscovery, and build_http_client.

use std::path::{Path, PathBuf};

use bmad_bot::config::discovery::BmadDiscovery;
use bmad_bot::config::{build_http_client, BotConfig, ConfigError};

use crate::helpers::fixtures::{make_test_config, make_test_secrets};

// ---------------------------------------------------------------------------
// Local helper
// ---------------------------------------------------------------------------

/// Write a valid BotConfig YAML to a temp directory and return the file path.
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

    // Act — load → validate
    let loaded = BotConfig::load(&path).expect("load should succeed");
    loaded.validate().expect("validate should succeed");

    // Act — secrets validate_for_config
    let secrets = make_test_secrets();
    secrets
        .validate_for_config(&loaded)
        .expect("secrets validation should succeed");
}

// ===========================================================================
// Task 3 — Invalid config rejection (AC #2)
// ===========================================================================

#[test]
fn test_config_zero_polling_rejected() {
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
    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "polling_interval_secs"),
        "expected InvalidField for polling_interval_secs, got: {err}"
    );
}

#[test]
fn test_config_unknown_git_provider_rejected() {
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

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "git_provider.provider"),
        "expected InvalidField for git_provider.provider, got: {err}"
    );
}

#[test]
fn test_config_unknown_llm_provider_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let bad_yaml = r#"
polling_interval_secs: 60
git_provider:
  provider: github
  repo_owner: test
  repo_name: test
llm:
  dev: { provider: unknown-llm, model: test }
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
    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "llm.dev.provider"),
        "expected InvalidField for llm.dev.provider, got: {err}"
    );
}

#[test]
fn test_config_empty_project_root_rejected() {
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

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingField { ref field } if field == "bmad_paths.project_root"),
        "expected MissingField for bmad_paths.project_root, got: {err}"
    );
}

#[test]
fn test_config_invalid_yaml_syntax_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let bad_yaml = "{{{{invalid yaml content: [unclosed";
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    let err = BotConfig::load(&path).unwrap_err();
    assert!(
        matches!(err, ConfigError::YamlParse(_)),
        "expected YamlParse, got: {err}"
    );
}

#[test]
fn test_config_load_nonexistent_file_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("does-not-exist.yaml");

    let err = BotConfig::load(&path).unwrap_err();
    assert!(
        matches!(err, ConfigError::FileRead { .. }),
        "expected FileRead, got: {err}"
    );
}

#[test]
fn test_config_invalid_field_error_contains_field_name() {
    // Verify that every InvalidField/MissingField error contains the offending field name.
    let tmp = tempfile::tempdir().unwrap();

    // Zero polling
    let yaml_zero_polling = r#"
polling_interval_secs: 0
git_provider: { provider: github, repo_owner: t, repo_name: t }
llm:
  dev: { provider: anthropic, model: m }
  review: { provider: anthropic, model: m }
  supervisor: { provider: anthropic, model: m }
notifications: { telegram: { enabled: false } }
bmad_paths: { project_root: ".", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
"#;
    let p = tmp.path().join("test.yaml");
    std::fs::write(&p, yaml_zero_polling).expect("write");
    let err = BotConfig::load(&p)
        .expect("load")
        .validate()
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("polling_interval_secs"),
        "error should mention field name: {msg}"
    );
}

// ===========================================================================
// Task 4 — Secrets validation (AC #3)
// ===========================================================================

#[test]
fn test_config_secrets_missing_anthropic_key() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path()); // uses provider: "anthropic" for all LLM roles

    let mut secrets = make_test_secrets();
    secrets.anthropic_api_key = None;

    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "ANTHROPIC_API_KEY"),
        "expected MissingSecret for ANTHROPIC_API_KEY, got: {err}"
    );
}

#[test]
fn test_config_secrets_missing_github_token() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = make_test_config(tmp.path());
    config.git_provider.provider = "github".to_string();

    let mut secrets = make_test_secrets();
    secrets.github_token = None;

    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "GITHUB_TOKEN"),
        "expected MissingSecret for GITHUB_TOKEN, got: {err}"
    );
}

#[test]
fn test_config_secrets_missing_telegram_token() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = make_test_config(tmp.path());
    config.notifications.telegram.enabled = true;
    config.notifications.telegram.chat_id = "12345".to_string();

    let mut secrets = make_test_secrets();
    secrets.telegram_bot_token = None;

    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "TELEGRAM_BOT_TOKEN"),
        "expected MissingSecret for TELEGRAM_BOT_TOKEN, got: {err}"
    );
}

#[test]
fn test_config_secrets_error_contains_env_var_name() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());

    let mut secrets = make_test_secrets();
    secrets.anthropic_api_key = None;

    let err = secrets.validate_for_config(&config).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ANTHROPIC_API_KEY"),
        "error message should contain env var name: {msg}"
    );
}

// ===========================================================================
// Task 5 — BMAD discovery integration tests (AC #4)
// ===========================================================================

#[test]
fn test_config_discovery_full_bmad_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Create full _bmad/ structure
    std::fs::create_dir_all(root.join("_bmad/bmm")).expect("mkdir bmm");
    std::fs::create_dir_all(root.join("_bmad/core")).expect("mkdir core");

    // Write config.yaml with version comment
    std::fs::write(
        root.join("_bmad/bmm/config.yaml"),
        "# Version: 6.0.0-Beta.7\nproject_name: test\n",
    )
    .expect("write config");

    let result = BmadDiscovery::discover(root);
    assert!(result.bmad_detected, "should detect _bmad directory");
    assert!(
        !result.installed_modules.is_empty(),
        "should find installed modules"
    );
    assert!(
        result.bmad_version.is_some(),
        "should extract version from config"
    );
    let version = result.bmad_version.unwrap();
    assert!(
        version.contains("6.0.0"),
        "version should contain 6.0.0, got: {version}"
    );
}

#[test]
fn test_config_discovery_no_bmad_directory() {
    let tmp = tempfile::tempdir().unwrap();

    let result = BmadDiscovery::discover(tmp.path());
    assert!(!result.bmad_detected, "should not detect _bmad");
    assert!(
        result.installed_modules.is_empty(),
        "should have no modules"
    );
}

#[test]
fn test_config_discovery_partial_bmad_no_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Create partial _bmad/ (no config.yaml)
    std::fs::create_dir_all(root.join("_bmad/core")).expect("mkdir core");

    let result = BmadDiscovery::discover(root);
    assert!(result.bmad_detected, "should detect _bmad directory");
    assert!(
        result.bmad_version.is_none(),
        "should have no version without config.yaml"
    );
}

// ===========================================================================
// Task 6 — HTTP client builder test (AC #5)
// ===========================================================================

#[test]
fn test_config_http_client_builds_successfully() {
    // Act — just ensure it returns without panicking
    let _client: reqwest_middleware::ClientWithMiddleware = build_http_client();
}
