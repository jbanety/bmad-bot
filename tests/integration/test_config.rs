//! Integration tests for the config loading, validation, secrets, discovery, and HTTP client
//! pipeline.
//!
//! Story 7.2 — Config → Startup Validation Integration Tests

use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{Duration, timeout};

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
// Task 2: Valid config round-trip (AC #1)
// ---------------------------------------------------------------------------

#[test]
fn test_config_valid_roundtrip_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_valid_config_yaml(tmp.path());

    let loaded = BotConfig::load(&path).expect("load should succeed");
    loaded.validate().expect("validate should succeed");

    let secrets = make_test_secrets();
    secrets
        .validate_for_config(&loaded)
        .expect("secrets validate should succeed");
}

// ---------------------------------------------------------------------------
// Task 3: Invalid config rejection (AC #2)
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
    let bad_yaml = "polling_interval_secs: [invalid yaml syntax";
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, bad_yaml).expect("write");

    let err = BotConfig::load(&path).unwrap_err();
    assert!(
        matches!(err, ConfigError::YamlParse(_)),
        "expected YamlParse, got: {err}"
    );
}

#[test]
fn test_config_load_nonexistent_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nonexistent.yaml");

    let err = BotConfig::load(&path).unwrap_err();
    assert!(
        matches!(err, ConfigError::FileRead { ref path, .. } if path.contains("nonexistent.yaml")),
        "expected FileRead for nonexistent file, got: {err}"
    );
}

#[test]
fn test_config_error_messages_contain_field_names() {
    let tmp = tempfile::tempdir().unwrap();

    // zero polling
    let yaml = r#"
polling_interval_secs: 0
git_provider: { provider: github, repo_owner: o, repo_name: r }
llm:
  dev: { provider: anthropic, model: m }
  review: { provider: anthropic, model: m }
  supervisor: { provider: anthropic, model: m }
notifications: { telegram: { enabled: false } }
bmad_paths: { project_root: ".", output_folder: "o", planning_artifacts: "p", implementation_artifacts: "i" }
"#;
    let path = tmp.path().join("bmad-bot.yaml");
    std::fs::write(&path, yaml).expect("write");
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
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());

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
    let mut config = make_test_config(tmp.path());
    config.git_provider.provider = "github".to_string();

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
fn test_secrets_missing_telegram_token_rejected() {
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
// Task 5: BMAD Discovery (AC #4)
// ---------------------------------------------------------------------------

#[test]
fn test_discovery_valid_bmad_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let bmad_dir = tmp.path().join("_bmad");
    let bmm_dir = bmad_dir.join("bmm");
    let core_dir = bmad_dir.join("core");
    std::fs::create_dir_all(&bmm_dir).unwrap();
    std::fs::create_dir_all(&core_dir).unwrap();

    // Write config.yaml with version comment
    let config_path = bmm_dir.join("config.yaml");
    std::fs::write(&config_path, "# Version: 6.0.0-Beta.7\nproject_name: test\n").unwrap();

    let discovery = BmadDiscovery::discover(tmp.path());
    assert!(discovery.bmad_detected, "should detect _bmad directory");
    assert!(
        !discovery.installed_modules.is_empty(),
        "should find installed modules"
    );
    assert!(
        discovery.installed_modules.contains(&"bmm".to_string()),
        "should find bmm module"
    );
    assert!(
        discovery.installed_modules.contains(&"core".to_string()),
        "should find core module"
    );
    assert!(
        discovery.bmad_version.is_some(),
        "should extract version from config"
    );
    assert_eq!(
        discovery.bmad_version.as_deref(),
        Some("6.0.0-Beta.7"),
        "version should match"
    );
}

#[test]
fn test_discovery_no_bmad_directory() {
    let tmp = tempfile::tempdir().unwrap();

    let discovery = BmadDiscovery::discover(tmp.path());
    assert!(!discovery.bmad_detected, "should not detect _bmad");
    assert!(
        discovery.installed_modules.is_empty(),
        "modules should be empty"
    );
}

#[test]
fn test_discovery_partial_bmad_no_config() {
    let tmp = tempfile::tempdir().unwrap();
    let bmad_dir = tmp.path().join("_bmad");
    let bmm_dir = bmad_dir.join("bmm");
    std::fs::create_dir_all(&bmm_dir).unwrap();
    // No config.yaml written

    let discovery = BmadDiscovery::discover(tmp.path());
    assert!(discovery.bmad_detected, "should detect _bmad directory");
    assert!(
        discovery.bmad_version.is_none(),
        "version should be None without config.yaml"
    );
}

// ---------------------------------------------------------------------------
// Task 6: HTTP client builder (AC #5)
// ---------------------------------------------------------------------------

#[test]
fn test_http_client_builds_successfully() {
    // build_http_client() should not panic and should return a ClientWithMiddleware
    let _client = build_http_client();
}

#[test]
fn test_http_client_returns_client_with_middleware() {
    let client: reqwest_middleware::ClientWithMiddleware = build_http_client();
    // Type assertion via binding — if this compiles, the type is correct.
    drop(client);
}

#[tokio::test]
async fn test_http_client_retries_on_transient_errors() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_server = Arc::clone(&attempts);

    let mut server_handle = tokio::spawn(async move {
        for _ in 0..4 {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buffer = [0u8; 1024];
                let _ = socket.read(&mut buffer).await;
                let _ = attempts_for_server.fetch_add(1, Ordering::SeqCst);
                let response = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(response).await;
            }
        }
    });

    let client = build_http_client();
    let url = format!("http://{addr}/");
    let _ = client.get(url).send().await;

    if timeout(Duration::from_secs(5), &mut server_handle)
        .await
        .is_err()
    {
        server_handle.abort();
    }

    assert_eq!(
        attempts.load(Ordering::SeqCst),
        4,
        "expected initial request plus three retries"
    );
}

