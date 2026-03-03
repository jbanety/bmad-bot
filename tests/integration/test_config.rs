//! Integration tests for the config loading, validation, secrets, discovery,
//! and HTTP client builder pipeline.
//!
//! Covers Story 7.2 — AC #1 through #5.

use std::path::{Path, PathBuf};

use bmad_bot::config::{build_http_client, BotConfig, ConfigError};
use bmad_bot::config::discovery::BmadDiscovery;

use super::helpers::fixtures::{make_test_config, make_test_secrets};

// ---------------------------------------------------------------------------
// Local helper: write a valid BotConfig YAML to a temp directory
// ---------------------------------------------------------------------------

/// Serialize a valid [`BotConfig`] to `{dir}/bmad-bot.yaml` and return the path.
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

    // Act — secrets validate
    let secrets = make_test_secrets();
    secrets
        .validate_for_config(&loaded)
        .expect("secrets validate should succeed");
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
    let file = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&file, bad_yaml).expect("write");

    let config = BotConfig::load(&file).expect("load");
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
    let file = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&file, bad_yaml).expect("write");

    let config = BotConfig::load(&file).expect("load");
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
  dev: { provider: banana, model: test }
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
    let file = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&file, bad_yaml).expect("write");

    let config = BotConfig::load(&file).expect("load");
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
    let file = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&file, bad_yaml).expect("write");

    let config = BotConfig::load(&file).expect("load");
    let err = config.validate().unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingField { ref field } if field == "bmad_paths.project_root"),
        "expected MissingField for bmad_paths.project_root, got: {err}"
    );
}

#[test]
fn test_config_invalid_yaml_syntax_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let bad_yaml = "polling_interval_secs: [invalid yaml\n  broken:";
    let file = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&file, bad_yaml).expect("write");

    let err = BotConfig::load(&file).unwrap_err();
    assert!(
        matches!(err, ConfigError::YamlParse(_)),
        "expected YamlParse, got: {err}"
    );
}

#[test]
fn test_config_load_nonexistent_file_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("does-not-exist.yaml");

    let err = BotConfig::load(&missing).unwrap_err();
    assert!(
        matches!(err, ConfigError::FileRead { .. }),
        "expected FileRead, got: {err}"
    );
}

#[test]
fn test_config_error_messages_contain_field_names() {
    // Verify that each error type embeds the offending field in its Display output.
    let tmp = tempfile::tempdir().unwrap();

    // Zero polling
    let yaml = r#"
polling_interval_secs: 0
git_provider: { provider: github, repo_owner: t, repo_name: t }
llm:
  dev: { provider: anthropic, model: t }
  review: { provider: anthropic, model: t }
  supervisor: { provider: anthropic, model: t }
notifications: { telegram: { enabled: false } }
bmad_paths: { project_root: ".", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
"#;
    let file = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&file, yaml).expect("write");
    let err = BotConfig::load(&file).unwrap().validate().unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("polling_interval_secs"),
        "error message should contain field name: {msg}"
    );

    // Unknown git provider
    let yaml2 = yaml.replace("provider: github", "provider: svn");
    let yaml2 = yaml2.replacen("polling_interval_secs: 0", "polling_interval_secs: 60", 1);
    std::fs::write(&file, &yaml2).expect("write");
    let err2 = BotConfig::load(&file).unwrap().validate().unwrap_err();
    let msg2 = err2.to_string();
    assert!(
        msg2.contains("git_provider.provider"),
        "error message should contain field name: {msg2}"
    );

    // Unknown LLM provider
    let yaml3 = r#"
polling_interval_secs: 60
git_provider: { provider: github, repo_owner: t, repo_name: t }
llm:
  dev: { provider: unknown-llm, model: t }
  review: { provider: anthropic, model: t }
  supervisor: { provider: anthropic, model: t }
notifications: { telegram: { enabled: false } }
bmad_paths: { project_root: ".", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
"#;
    std::fs::write(&file, yaml3).expect("write");
    let err3 = BotConfig::load(&file).unwrap().validate().unwrap_err();
    let msg3 = err3.to_string();
    assert!(
        msg3.contains("llm.dev.provider"),
        "error message should contain field name: {msg3}"
    );

    // Empty project_root
    let yaml4 = r#"
polling_interval_secs: 60
git_provider: { provider: github, repo_owner: t, repo_name: t }
llm:
  dev: { provider: anthropic, model: t }
  review: { provider: anthropic, model: t }
  supervisor: { provider: anthropic, model: t }
notifications: { telegram: { enabled: false } }
bmad_paths: { project_root: "", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
"#;
    std::fs::write(&file, yaml4).expect("write");
    let err4 = BotConfig::load(&file).unwrap().validate().unwrap_err();
    let msg4 = err4.to_string();
    assert!(
        msg4.contains("bmad_paths.project_root"),
        "error message should contain field name: {msg4}"
    );
}

// ===========================================================================
// Task 4 — Secrets validation (AC #3)
// ===========================================================================

#[test]
fn test_secrets_missing_anthropic_key() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    // Config uses anthropic for all roles by default

    let mut secrets = make_test_secrets();
    secrets.anthropic_api_key = None;

    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "ANTHROPIC_API_KEY"),
        "expected MissingSecret for ANTHROPIC_API_KEY, got: {err}"
    );
}

#[test]
fn test_secrets_missing_github_token() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    // Config uses github git provider by default

    let mut secrets = make_test_secrets();
    secrets.github_token = None;

    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "GITHUB_TOKEN"),
        "expected MissingSecret for GITHUB_TOKEN, got: {err}"
    );
}

#[test]
fn test_secrets_missing_telegram_token() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = make_test_config(tmp.path());
    config.notifications.telegram.enabled = true;

    let mut secrets = make_test_secrets();
    secrets.telegram_bot_token = None;

    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "TELEGRAM_BOT_TOKEN"),
        "expected MissingSecret for TELEGRAM_BOT_TOKEN, got: {err}"
    );
}

#[test]
fn test_secrets_error_contains_env_var_name() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());

    // Anthropic key error message contains env var name
    let mut secrets = make_test_secrets();
    secrets.anthropic_api_key = None;
    let err = secrets.validate_for_config(&config).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ANTHROPIC_API_KEY"),
        "error message should contain ANTHROPIC_API_KEY: {msg}"
    );

    // GitHub token error message contains env var name
    let mut secrets2 = make_test_secrets();
    secrets2.github_token = None;
    let err2 = secrets2.validate_for_config(&config).unwrap_err();
    let msg2 = err2.to_string();
    assert!(
        msg2.contains("GITHUB_TOKEN"),
        "error message should contain GITHUB_TOKEN: {msg2}"
    );

    // Telegram token error message contains env var name
    let mut config3 = make_test_config(tmp.path());
    config3.notifications.telegram.enabled = true;
    let mut secrets3 = make_test_secrets();
    secrets3.telegram_bot_token = None;
    let err3 = secrets3.validate_for_config(&config3).unwrap_err();
    let msg3 = err3.to_string();
    assert!(
        msg3.contains("TELEGRAM_BOT_TOKEN"),
        "error message should contain TELEGRAM_BOT_TOKEN: {msg3}"
    );
}

// ===========================================================================
// Task 5 — BMAD discovery (AC #4)
// ===========================================================================

#[test]
fn test_discovery_full_bmad_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Create _bmad/ structure
    std::fs::create_dir_all(root.join("_bmad/bmm")).expect("mkdir bmm");
    std::fs::create_dir_all(root.join("_bmad/core")).expect("mkdir core");
    std::fs::write(
        root.join("_bmad/bmm/config.yaml"),
        "# Version: 6.0.0-Beta.7\nproject_name: test\n",
    )
    .expect("write config.yaml");

    let result = BmadDiscovery::discover(root);

    assert!(result.bmad_detected, "should detect _bmad directory");
    assert!(
        result.installed_modules.contains(&"bmm".to_string()),
        "should find bmm module"
    );
    assert!(
        result.installed_modules.contains(&"core".to_string()),
        "should find core module"
    );
    assert_eq!(
        result.bmad_version,
        Some("6.0.0-Beta.7".to_string()),
        "should extract version from config.yaml"
    );
}

#[test]
fn test_discovery_no_bmad_directory() {
    let tmp = tempfile::tempdir().unwrap();
    // Empty directory — no _bmad/

    let result = BmadDiscovery::discover(tmp.path());

    assert!(!result.bmad_detected, "should not detect _bmad");
    assert!(
        result.installed_modules.is_empty(),
        "should have no modules"
    );
}

#[test]
fn test_discovery_partial_bmad_no_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Create _bmad/ with core but no config.yaml
    std::fs::create_dir_all(root.join("_bmad/core")).expect("mkdir core");

    let result = BmadDiscovery::discover(root);

    assert!(result.bmad_detected, "should detect _bmad directory");
    assert!(
        result.bmad_version.is_none(),
        "should have no version without config.yaml"
    );
}

// ===========================================================================
// Task 6 — HTTP client builder (AC #5)
// ===========================================================================

#[test]
fn test_http_client_builds_successfully() {
    // Act — should not panic
    let _client: reqwest_middleware::ClientWithMiddleware = build_http_client();
}
