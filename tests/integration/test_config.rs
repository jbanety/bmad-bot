//! Integration tests for config loading, validation, secrets, BMAD discovery,
//! and HTTP client builder.
//!
//! Covers Story 7.2 — AC #1 through #5.

use std::path::{Path, PathBuf};

use bmad_bot::config::discovery::BmadDiscovery;
use bmad_bot::config::{BotConfig, BotSecrets, ConfigError, build_http_client};

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

// ===========================================================================
// Task 2 — AC #1: Valid config round-trip
// ===========================================================================

#[test]
fn test_config_valid_roundtrip_succeeds() {
    // Arrange
    let tmp = tempfile::tempdir().unwrap();
    let path = write_valid_config_yaml(tmp.path());

    // Act — load → validate
    let loaded = BotConfig::load(&path).expect("load");
    loaded.validate().expect("validate");

    // Act — secrets validate
    let secrets = make_test_secrets();
    secrets.validate_for_config(&loaded).expect("secrets validate");
}

// ===========================================================================
// Task 3 — AC #2: Invalid config rejection tests
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
  telegram: { enabled: false, chat_id: "x" }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_file: "bot.log"
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

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
  telegram: { enabled: false, chat_id: "x" }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_file: "bot.log"
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

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
    let bad_yaml = r#"
polling_interval_secs: 60
git_provider:
  provider: github
  repo_owner: test
  repo_name: test
llm:
  dev: { provider: deepmind, model: test }
  review: { provider: anthropic, model: test }
  supervisor: { provider: anthropic, model: test }
notifications:
  telegram: { enabled: false, chat_id: "x" }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_file: "bot.log"
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "llm.dev.provider"),
        "expected InvalidField for llm.dev.provider, got: {err:?}"
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
  telegram: { enabled: false, chat_id: "x" }
bmad_paths:
  project_root: ""
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_file: "bot.log"
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

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
    let bad_yaml = "{{{{ not valid yaml at all ::::";
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

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
fn test_config_invalid_log_format_rejected() {
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
  telegram: { enabled: false, chat_id: "x" }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_format: "xml"
log_file: "bot.log"
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "log_format"),
        "expected InvalidField for log_format, got: {err:?}"
    );
}

#[test]
fn test_config_invalid_log_level_rejected() {
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
  telegram: { enabled: false, chat_id: "x" }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
log_level: "verbose"
log_file: "bot.log"
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    let config = BotConfig::load(&path).expect("load");
    let err = config.validate().unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidField { ref field, .. } if field == "log_level"),
        "expected InvalidField for log_level, got: {err:?}"
    );
}

#[test]
fn test_config_error_messages_contain_field_names() {
    // Verify each error variant includes the offending field name in the Display output
    let tmp = tempfile::tempdir().unwrap();

    // polling_interval_secs: 0
    let yaml = r#"
polling_interval_secs: 0
git_provider: { provider: github, repo_owner: t, repo_name: t }
llm:
  dev: { provider: anthropic, model: m }
  review: { provider: anthropic, model: m }
  supervisor: { provider: anthropic, model: m }
notifications: { telegram: { enabled: false, chat_id: "x" } }
bmad_paths: { project_root: ".", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
log_file: "f"
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, yaml).expect("write");
    let config = BotConfig::load(&path).expect("load");
    let err_msg = config.validate().unwrap_err().to_string();
    assert!(
        err_msg.contains("polling_interval_secs"),
        "error message should contain field name: {err_msg}"
    );

    // Unknown git provider
    let yaml2 = yaml.replace("polling_interval_secs: 0", "polling_interval_secs: 60")
        .replace("provider: github", "provider: bitbucket");
    std::fs::write(&path, yaml2).expect("write");
    let config = BotConfig::load(&path).expect("load");
    let err_msg = config.validate().unwrap_err().to_string();
    assert!(
        err_msg.contains("git_provider.provider"),
        "error message should contain field name: {err_msg}"
    );

    // Unknown LLM provider
    let yaml3 = yaml.replace("polling_interval_secs: 0", "polling_interval_secs: 60")
        .replace("provider: anthropic", "provider: deepmind");
    std::fs::write(&path, yaml3).expect("write");
    let config = BotConfig::load(&path).expect("load");
    let err_msg = config.validate().unwrap_err().to_string();
    assert!(
        err_msg.contains("llm.dev.provider"),
        "error message should contain field name: {err_msg}"
    );

    // Empty project_root
    let yaml4 = yaml.replace("polling_interval_secs: 0", "polling_interval_secs: 60")
        .replace("project_root: \".\"", "project_root: \"\"");
    std::fs::write(&path, yaml4).expect("write");
    let config = BotConfig::load(&path).expect("load");
    let err_msg = config.validate().unwrap_err().to_string();
    assert!(
        err_msg.contains("bmad_paths.project_root"),
        "error message should contain field name: {err_msg}"
    );
}

// ===========================================================================
// Task 4 — AC #3: Secrets validation tests
// ===========================================================================

#[test]
fn test_config_secrets_missing_anthropic_key() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    // dev and review roles both use anthropic
    let secrets = BotSecrets {
        anthropic_api_key: None,
        ..make_test_secrets()
    };

    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "ANTHROPIC_API_KEY"),
        "expected MissingSecret for ANTHROPIC_API_KEY, got: {err:?}"
    );
}

#[test]
fn test_config_secrets_missing_github_token() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    // git_provider is github
    let secrets = BotSecrets {
        github_token: None,
        ..make_test_secrets()
    };

    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "GITHUB_TOKEN"),
        "expected MissingSecret for GITHUB_TOKEN, got: {err:?}"
    );
}

#[test]
fn test_config_secrets_missing_telegram_token() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = make_test_config(tmp.path());
    config.notifications.telegram.enabled = true;

    let secrets = BotSecrets {
        telegram_bot_token: None,
        ..make_test_secrets()
    };

    let err = secrets.validate_for_config(&config).unwrap_err();
    assert!(
        matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "TELEGRAM_BOT_TOKEN"),
        "expected MissingSecret for TELEGRAM_BOT_TOKEN, got: {err:?}"
    );
}

#[test]
fn test_config_secrets_error_contains_env_var_name() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());

    // Missing anthropic key
    let secrets = BotSecrets {
        anthropic_api_key: None,
        ..make_test_secrets()
    };
    let err_msg = secrets.validate_for_config(&config).unwrap_err().to_string();
    assert!(
        err_msg.contains("ANTHROPIC_API_KEY"),
        "error message should contain env var name: {err_msg}"
    );

    // Missing github token
    let secrets2 = BotSecrets {
        github_token: None,
        ..make_test_secrets()
    };
    let err_msg2 = secrets2.validate_for_config(&config).unwrap_err().to_string();
    assert!(
        err_msg2.contains("GITHUB_TOKEN"),
        "error message should contain env var name: {err_msg2}"
    );
}

// ===========================================================================
// Task 5 — AC #4: BMAD discovery integration tests
// ===========================================================================

#[test]
fn test_config_discovery_full_bmad_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Create full _bmad structure with all known modules
    std::fs::create_dir_all(root.join("_bmad/bmm")).unwrap();
    std::fs::create_dir_all(root.join("_bmad/core")).unwrap();
    std::fs::create_dir_all(root.join("_bmad/_config")).unwrap();
    std::fs::create_dir_all(root.join("_bmad/_memory")).unwrap();
    std::fs::write(
        root.join("_bmad/bmm/config.yaml"),
        "# Version: 6.0.0-Beta.7\nproject_name: test\n",
    )
    .unwrap();

    let discovery = BmadDiscovery::discover(root);

    assert!(discovery.bmad_detected);
    assert_eq!(discovery.bmad_version.as_deref(), Some("6.0.0-Beta.7"));
    assert!(discovery.installed_modules.contains(&"bmm".to_string()));
    assert!(discovery.installed_modules.contains(&"core".to_string()));
    assert!(discovery.installed_modules.contains(&"_config".to_string()));
    assert!(discovery.installed_modules.contains(&"_memory".to_string()));
    assert!(discovery.config_path.is_some());
}

#[test]
fn test_config_discovery_no_bmad_directory() {
    let tmp = tempfile::tempdir().unwrap();

    let discovery = BmadDiscovery::discover(tmp.path());

    assert!(!discovery.bmad_detected);
    assert!(discovery.installed_modules.is_empty());
    assert!(discovery.bmad_version.is_none());
    assert!(discovery.config_path.is_none());
}

#[test]
fn test_config_discovery_partial_bmad_no_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Create _bmad dir with core but no config.yaml
    std::fs::create_dir_all(root.join("_bmad/core")).unwrap();

    let discovery = BmadDiscovery::discover(root);

    assert!(discovery.bmad_detected);
    assert!(discovery.bmad_version.is_none());
    assert!(discovery.installed_modules.contains(&"core".to_string()));
    assert!(discovery.config_path.is_none());
}

// ===========================================================================
// Task 6 — AC #5: HTTP client builder test
// ===========================================================================

#[test]
fn test_config_http_client_builds_successfully() {
    // Act — should not panic
    let _client: reqwest_middleware::ClientWithMiddleware = build_http_client();
}
