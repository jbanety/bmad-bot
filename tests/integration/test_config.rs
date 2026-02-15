//! Integration tests: Config → Startup Validation Pipeline (Story 7.2)
//!
//! Tests the full config loading, validation, secrets validation,
//! BMAD discovery, and HTTP client builder pipelines end-to-end.

use std::path::{Path, PathBuf};

use bmad_bot::config::discovery::BmadDiscovery;
use bmad_bot::config::{build_http_client, BotConfig, BotSecrets, ConfigError};

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
    let tmp = tempfile::tempdir().unwrap();
    let path = write_valid_config_yaml(tmp.path());

    // Load → validate
    let loaded = BotConfig::load(&path).expect("load");
    loaded.validate().expect("validate");

    // Secrets validate
    let secrets = make_test_secrets();
    secrets.validate_for_config(&loaded).expect("secrets validate");
}

// ---------------------------------------------------------------------------
// Task 3 — Invalid config rejection (AC #2)
// ---------------------------------------------------------------------------

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
    let bad_yaml = "not: [valid: yaml: {{{\n";
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
fn test_config_error_messages_contain_field_names() {
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
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, yaml).expect("write");
    let err = BotConfig::load(&path).unwrap().validate().unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("polling_interval_secs"),
        "error message should contain field name, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Task 4 — Secrets validation (AC #3)
// ---------------------------------------------------------------------------

#[test]
fn test_secrets_missing_anthropic_key_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    // All roles use "anthropic" by default
    let secrets = BotSecrets {
        anthropic_api_key: None,
        ..make_test_secrets()
    };
    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "ANTHROPIC_API_KEY"),
        "expected MissingSecret for ANTHROPIC_API_KEY, got: {err}"
    );
}

#[test]
fn test_secrets_missing_github_token_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path()); // git_provider.provider = "github"
    let secrets = BotSecrets {
        github_token: None,
        ..make_test_secrets()
    };
    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "GITHUB_TOKEN"),
        "expected MissingSecret for GITHUB_TOKEN, got: {err}"
    );
}

#[test]
fn test_secrets_missing_telegram_token_when_enabled_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = make_test_config(tmp.path());
    config.notifications.telegram.enabled = true;
    config.notifications.telegram.chat_id = "12345".to_string();
    let secrets = BotSecrets {
        telegram_bot_token: None,
        ..make_test_secrets()
    };
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
    let secrets = BotSecrets {
        anthropic_api_key: None,
        ..make_test_secrets()
    };
    let err = secrets.validate_for_config(&config).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ANTHROPIC_API_KEY"),
        "error message should contain env var name, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Task 5 — BMAD Discovery (AC #4)
// ---------------------------------------------------------------------------

#[test]
fn test_discovery_detects_full_bmad_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let bmad = tmp.path().join("_bmad");
    let bmm = bmad.join("bmm");
    let core = bmad.join("core");

    std::fs::create_dir_all(&bmm).unwrap();
    std::fs::create_dir_all(&core).unwrap();
    std::fs::write(
        bmm.join("config.yaml"),
        "# Version: 6.0.0-Beta.7\nproject_name: test\n",
    )
    .unwrap();

    let result = BmadDiscovery::discover(tmp.path());
    assert!(result.bmad_detected, "should detect _bmad/");
    assert_eq!(result.bmad_version.as_deref(), Some("6.0.0-Beta.7"));
    assert!(result.installed_modules.contains(&"bmm".to_string()));
    assert!(result.installed_modules.contains(&"core".to_string()));
}

#[test]
fn test_discovery_no_bmad_directory() {
    let tmp = tempfile::tempdir().unwrap();
    // No _bmad/ at all
    let result = BmadDiscovery::discover(tmp.path());
    assert!(!result.bmad_detected);
    assert!(result.installed_modules.is_empty());
    assert!(result.bmad_version.is_none());
}

#[test]
fn test_discovery_partial_bmad_no_config() {
    let tmp = tempfile::tempdir().unwrap();
    let bmad = tmp.path().join("_bmad");
    let bmm = bmad.join("bmm");
    std::fs::create_dir_all(&bmm).unwrap();
    // No config.yaml → detected but no version

    let result = BmadDiscovery::discover(tmp.path());
    assert!(result.bmad_detected, "should detect _bmad/ even without config");
    assert!(result.bmad_version.is_none(), "no config.yaml → no version");
    assert!(result.installed_modules.contains(&"bmm".to_string()));
}

// ---------------------------------------------------------------------------
// Task 6 — HTTP client builder (AC #5)
// ---------------------------------------------------------------------------

#[test]
fn test_http_client_builds_without_panicking() {
    // build_http_client() returns ClientWithMiddleware — if it panics, test fails
    let _client = build_http_client();
}

#[test]
fn test_http_client_returns_client_with_middleware() {
    let client: reqwest_middleware::ClientWithMiddleware = build_http_client();
    // Type assertion via binding — if build_http_client returned something else, this won't compile
    drop(client);
}
