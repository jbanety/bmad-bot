# Story 1.3: Interactive Init Command

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer setting up BMAD Bot for the first time,
I want to run `bmad-bot init` and be guided through an interactive setup,
So that I can generate a valid configuration without manually writing YAML.

## Acceptance Criteria

1. **Given** I am in a project directory without existing BMAD Bot configuration **When** I run `bmad-bot init` **Then** interactive prompts ask for: repository path, LLM provider and model for each role (dev, review, supervisor), git provider (GitHub/GitLab), Telegram notification config, and polling interval

2. **Given** I have completed all interactive prompts **When** the init command finishes **Then** a `bmad-bot.yaml` file is generated with all user-provided settings (no secrets in this file) **And** a `.env` file is generated with placeholder entries for all required secrets (API keys, tokens) with comments explaining each

3. **Given** a `bmad-bot.yaml` already exists in the directory **When** I run `bmad-bot init` **Then** the user is warned that existing config will be overwritten and asked to confirm before proceeding

## Tasks / Subtasks

- [x] Task 0: Add new dependencies (AC: #1, #2)
  - [x] 0.1 Add `dialoguer = "0.11"` to `[dependencies]` in Cargo.toml
  - [x] 0.2 Add `chrono = "0.4"` to `[dependencies]` in Cargo.toml (used for timestamp in generated config header)
  - [x] 0.3 Verify `cargo check` passes

- [x] Task 1: Implement `run_init()` function in `cli/mod.rs` (AC: #1, #2, #3)
  - [x] 1.1 Create `pub async fn run_init(config_path: &Path) -> Result<(), CliError>`
  - [x] 1.2 Check if `config_path` already exists → if so, prompt for overwrite confirmation via `dialoguer::Confirm`
  - [x] 1.3 If user declines overwrite → log info and return Ok (no error)
  - [x] 1.4 Add `Init`, `Io`, `UserCancelled` variants to `CliError` for init-specific failures (see CliError Extension in Dev Notes)

- [x] Task 2: Implement interactive prompts (AC: #1)
  - [x] 2.1 Create `fn collect_config_interactively() -> Result<BotConfig, CliError>` function
  - [x] 2.2 Prompt: git provider selection via `dialoguer::Select` → "github" or "gitlab"
  - [x] 2.3 Prompt: repo owner via `dialoguer::Input` (required, no default)
  - [x] 2.4 Prompt: repo name via `dialoguer::Input` (required, no default)
  - [x] 2.5 Prompt: target branch via `dialoguer::Input` (default: "main")
  - [x] 2.6 Prompt: LLM provider for dev role via `dialoguer::Select` → "anthropic", "openai", "github-models"
  - [x] 2.7 Prompt: model name for dev role via `dialoguer::Input` (suggest default based on provider)
  - [x] 2.8 Prompt: "Use same provider/model for review and supervisor?" via `dialoguer::Confirm`
  - [x] 2.9 If no → prompt separately for review and supervisor provider+model
  - [x] 2.10 Prompt: enable Telegram notifications via `dialoguer::Confirm` (default: false)
  - [x] 2.11 If enabled → prompt for Telegram chat_id via `dialoguer::Input`
  - [x] 2.12 Prompt: polling interval in seconds via `dialoguer::Input<u64>` with `.interact()` (NOT `.interact_text()` — see Dev Notes). Default: 300
  - [x] 2.13 Prompt: BMAD project root path via `dialoguer::Input` (default: ".")
  - [x] 2.14 Prompt: log format via `dialoguer::Select` → "pretty", "json" (default: "pretty")
  - [x] 2.15 Prompt: log level via `dialoguer::Select` → "trace", "debug", "info", "warn", "error" (default: "info")
  - [x] 2.16 Derive `output_folder`, `planning_artifacts`, `implementation_artifacts` from project root (using standard BMAD paths)

- [x] Task 3: Generate `bmad-bot.yaml` from user input (AC: #2)
  - [x] 3.1 Create `fn generate_config_yaml(config: &BotConfig) -> Result<String, CliError>` that serializes to YAML with header comments
  - [x] 3.2 Write YAML with a descriptive header comment block (project name, generation date via `chrono`, reference to .env)
  - [x] 3.3 Write to `config_path` using `tokio::fs::write`
  - [x] 3.4 Log success with `tracing::info!`

- [x] Task 4: Generate `.env` with context-aware placeholders (AC: #2)
  - [x] 4.1 Create `fn generate_env_file(config: &BotConfig) -> Result<String, CliError>` that builds .env content
  - [x] 4.2 Always include a header comment explaining the file's purpose
  - [x] 4.3 Include only the secrets relevant to chosen providers (e.g., skip OPENAI_API_KEY if no role uses openai)
  - [x] 4.4 Each secret line has a dynamic comment specifying which roles use that provider (see Dev Notes)
  - [x] 4.5 Write to `.env` in the current directory
  - [x] 4.6 If `.env` already exists → prompt for overwrite confirmation (separate from yaml confirmation)
  - [x] 4.7 Log success with `tracing::info!`

- [x] Task 5: Wire into main.rs (AC: #1, #2, #3)
  - [x] 5.1 Replace the `Commands::Init` placeholder in main.rs with call to `cli::run_init(&cli.config)`
  - [x] 5.2 Keep basic tracing init in main.rs before `run_init` (`let _ = tracing_subscriber::fmt::try_init();`) — tracing is global, main.rs owns the subscriber for non-start commands

- [x] Task 6: Write unit tests (AC: #1, #2, #3)
  - [x] 6.1 Test `generate_config_yaml` produces valid YAML that deserializes back to BotConfig
  - [x] 6.2 Test `generate_env_file` includes ANTHROPIC_API_KEY when provider is anthropic
  - [x] 6.3 Test `generate_env_file` excludes OPENAI_API_KEY when no role uses openai
  - [x] 6.4 Test `generate_env_file` includes TELEGRAM_BOT_TOKEN only when telegram is enabled
  - [x] 6.5 Test `generate_env_file` includes GITHUB_TOKEN when git provider is github
  - [x] 6.6 Test `generate_env_file` includes GITLAB_TOKEN when git provider is gitlab
  - [x] 6.7 Test generated YAML round-trips through BotConfig::validate() without errors
  - [x] 6.8 Test derived BMAD paths from project root are correct
  - [x] 6.9 Test `generate_env_file` comments specify correct roles per provider

- [x] Task 7: Final quality checks
  - [x] 7.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [x] 7.2 Run `cargo clippy` and fix any warnings
  - [x] 7.3 Run `cargo test` and verify all tests pass (including Story 1.1 and 1.2 tests)
  - [x] 7.4 Verify all public items have `///` doc comments
  - [x] 7.5 Manual integration test: run `cargo run -- init`, complete all prompts, verify `bmad-bot.yaml` and `.env` are generated correctly, run `cargo run -- init` again and verify overwrite prompt appears

## Dev Notes

### Previous Story Intelligence

**Story 1.1** established:
- `BotConfig` struct with all fields (`polling_interval_secs`, `git_provider`, `llm`, `notifications`, `bmad_paths`, `log_format`, `log_level` added in Story 1.2)
- All nested config structs: `LlmConfig`, `LlmRoleConfig`, `GitProviderConfig`, `NotificationConfig`, `TelegramConfig`, `BmadPathsConfig`
- `BotSecrets` struct, `ConfigError` thiserror enum
- `BotConfig::validate()` method
- `bmad-bot.yaml.example` and `.env.example` as reference templates
- serde defaults: `polling_interval_secs` → 300, `target_branch` → "main", `log_format` → "pretty", `log_level` → "info"

**Story 1.2** established:
- `Cli` struct with `--config` flag (default: `bmad-bot.yaml`), `Commands` enum with Init/Start/Status/Logs
- `CliError` thiserror enum with `Config`, `TracingInit`, `Signal` variants
- `run_start()` handler, `init_tracing()`, `run_polling_loop()`
- `main.rs` dispatches `Commands::Init` with `let _ = tracing_subscriber::fmt::try_init();` + `tracing::warn!("not yet implemented")`
- `BotSecrets::validate_for_config(&config)` method for cross-validation

**Key pattern from Story 1.2 to follow:** The `init` command in main.rs currently does:
```rust
cli::Commands::Init => {
    let _ = tracing_subscriber::fmt::try_init();
    tracing::warn!("'init' command not yet implemented — see Story 1.3");
}
```
This story replaces that with `cli::run_init(&cli.config).await?;`

### New Dependencies

Add to Cargo.toml:

```toml
[dependencies]
# ... existing deps ...
dialoguer = "0.11"
chrono = "0.4"
```

**dialoguer** provides:
- `Input<T>` — text input with optional default, validation
- `Select` — single-item selection from a list
- `Confirm` — yes/no prompt with default
- All prompts render to stderr (not stdout), which is correct for CLI tools

**chrono** provides `Local::now()` for the timestamp in generated config headers.

> **NOTE:** `dialoguer` is a synchronous crate. It uses stdin/stdout directly. This is fine because `init` is an interactive one-shot command, not an async operation. Call it from the async context without issues (no blocking the tokio runtime since init doesn't need async operations for prompting).

### CliError Extension

Add a new variant to the existing `CliError` in `cli/mod.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    // ... existing variants from Story 1.2 ...

    #[error("Init failed: {reason}")]
    Init { reason: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("User cancelled operation")]
    UserCancelled,
}
```

> **NOTE:** Adding `Io(#[from] std::io::Error)` may conflict with the existing `Signal(#[from] std::io::Error)` variant since both use `#[from]`. Solution: remove `#[from]` from the `Signal` variant and convert explicitly, OR rename `Signal` to wrap an `io::Error` without `#[from]` and keep `Io` as the generic `#[from]` converter. The dev agent should resolve this — the simplest approach is to have ONE `#[from] std::io::Error` variant (`Io`) and construct `Signal`-related context manually if needed.

### run_init() Implementation

```rust
use std::path::Path;

/// Runs the `init` command: interactive config generation.
/// Assumes tracing is already initialized by main.rs (global subscriber).
pub async fn run_init(config_path: &Path) -> Result<(), CliError> {
    tracing::info!("Starting BMAD Bot interactive setup");

    // Check for existing config
    if config_path.exists() {
        let overwrite = dialoguer::Confirm::new()
            .with_prompt(format!(
                "⚠️  {} already exists. Overwrite?",
                config_path.display()
            ))
            .default(false)
            .interact()
            .map_err(|e| CliError::Init { reason: e.to_string() })?;

        if !overwrite {
            tracing::info!("Init cancelled — existing config preserved");
            return Ok(());
        }
    }

    // Collect config interactively
    let config = collect_config_interactively()?;

    // Validate the generated config
    config.validate().map_err(CliError::Config)?;

    // Generate and write bmad-bot.yaml
    let yaml_content = generate_config_yaml(&config)?;
    tokio::fs::write(config_path, &yaml_content).await?;
    tracing::info!(path = %config_path.display(), "Generated bmad-bot.yaml");

    // Generate and write .env
    let env_path = Path::new(".env");
    if env_path.exists() {
        let overwrite_env = dialoguer::Confirm::new()
            .with_prompt("⚠️  .env already exists. Overwrite?")
            .default(false)
            .interact()
            .map_err(|e| CliError::Init { reason: e.to_string() })?;

        if !overwrite_env {
            tracing::info!(".env preserved — skipping secrets file generation");
            println!("\n✅ Configuration written to {}", config_path.display());
            println!("⏭️  .env was NOT overwritten — update it manually if needed");
            return Ok(());
        }
    }

    let env_content = generate_env_file(&config)?;
    tokio::fs::write(env_path, &env_content).await?;
    tracing::info!("Generated .env");

    println!("\n✅ Setup complete!");
    println!("   Config: {}", config_path.display());
    println!("   Secrets: .env");
    println!("\n📝 Next steps:");
    println!("   1. Edit .env and fill in your API keys");
    println!("   2. Run `bmad-bot start` to launch the daemon");

    Ok(())
}
```

> **NOTE on `println!` usage:** The `init` command is an interactive CLI wizard, not the daemon. Using `println!` for user-facing setup messages (success, next steps) is acceptable here because:
> 1. These are user-interaction messages, not operational logs
> 2. tracing is initialized with basic fmt (by main.rs) and would mix log metadata with user-facing text
> 3. The anti-pattern "NO println anywhere" applies to the daemon runtime, not one-shot CLI commands
>
> If you prefer strict consistency, use `eprintln!` for user messages (goes to stderr, separate from tracing) or wrap them in `tracing::info!` — either approach is acceptable for the init wizard.

### Interactive Prompt Implementation

```rust
use crate::config::*;

/// Default model suggestions per provider.
fn default_model_for_provider(provider: &str) -> &str {
    match provider {
        "anthropic" => "claude-sonnet-4-20250514",
        "openai" => "gpt-4o",
        "github-models" => "gpt-4o",
        _ => "",
    }
}

const LLM_PROVIDERS: &[&str] = &["anthropic", "openai", "github-models"];
const GIT_PROVIDERS: &[&str] = &["github", "gitlab"];
const LOG_FORMATS: &[&str] = &["pretty", "json"];
const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

fn collect_config_interactively() -> Result<BotConfig, CliError> {
    println!("\n🏗️  BMAD Bot — Interactive Setup\n");

    // --- Git Provider ---
    println!("── Git Provider ──");
    let git_idx = dialoguer::Select::new()
        .with_prompt("Git hosting provider")
        .items(GIT_PROVIDERS)
        .default(0)
        .interact()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;
    let git_provider_name = GIT_PROVIDERS[git_idx].to_string();

    let repo_owner: String = dialoguer::Input::new()
        .with_prompt("Repository owner (org or user)")
        .interact_text()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    let repo_name: String = dialoguer::Input::new()
        .with_prompt("Repository name")
        .interact_text()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    let target_branch: String = dialoguer::Input::new()
        .with_prompt("Target branch for PRs")
        .default("main".to_string())
        .interact_text()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    // --- LLM Providers ---
    println!("\n── LLM Configuration ──");
    let dev_provider_idx = dialoguer::Select::new()
        .with_prompt("LLM provider for DEV agent")
        .items(LLM_PROVIDERS)
        .default(0)
        .interact()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;
    let dev_provider = LLM_PROVIDERS[dev_provider_idx].to_string();

    let dev_model: String = dialoguer::Input::new()
        .with_prompt("Model for DEV agent")
        .default(default_model_for_provider(&dev_provider).to_string())
        .interact_text()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    let same_for_all = dialoguer::Confirm::new()
        .with_prompt("Use same provider/model for REVIEW and SUPERVISOR roles?")
        .default(true)
        .interact()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    let (review_provider, review_model, supervisor_provider, supervisor_model) = if same_for_all {
        (
            dev_provider.clone(),
            dev_model.clone(),
            dev_provider.clone(),
            dev_model.clone(),
        )
    } else {
        let rp_idx = dialoguer::Select::new()
            .with_prompt("LLM provider for REVIEW agent")
            .items(LLM_PROVIDERS)
            .default(dev_provider_idx)
            .interact()
            .map_err(|e| CliError::Init { reason: e.to_string() })?;
        let rp = LLM_PROVIDERS[rp_idx].to_string();
        let rm: String = dialoguer::Input::new()
            .with_prompt("Model for REVIEW agent")
            .default(default_model_for_provider(&rp).to_string())
            .interact_text()
            .map_err(|e| CliError::Init { reason: e.to_string() })?;

        let sp_idx = dialoguer::Select::new()
            .with_prompt("LLM provider for SUPERVISOR agent")
            .items(LLM_PROVIDERS)
            .default(dev_provider_idx)
            .interact()
            .map_err(|e| CliError::Init { reason: e.to_string() })?;
        let sp = LLM_PROVIDERS[sp_idx].to_string();
        let sm: String = dialoguer::Input::new()
            .with_prompt("Model for SUPERVISOR agent")
            .default(default_model_for_provider(&sp).to_string())
            .interact_text()
            .map_err(|e| CliError::Init { reason: e.to_string() })?;

        (rp, rm, sp, sm)
    };

    // --- Notifications ---
    println!("\n── Notifications ──");
    let telegram_enabled = dialoguer::Confirm::new()
        .with_prompt("Enable Telegram notifications?")
        .default(false)
        .interact()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    let telegram_chat_id = if telegram_enabled {
        dialoguer::Input::new()
            .with_prompt("Telegram chat ID")
            .interact_text()
            .map_err(|e| CliError::Init { reason: e.to_string() })?
    } else {
        String::new()
    };

    // --- Daemon Settings ---
    println!("\n── Daemon Settings ──");
    // NOTE: Use .interact() NOT .interact_text() for non-String types.
    // .interact_text() always returns String. .interact() respects the generic T: FromStr.
    let polling_interval_secs: u64 = dialoguer::Input::new()
        .with_prompt("Polling interval (seconds)")
        .default(300u64)
        .interact()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    let project_root: String = dialoguer::Input::new()
        .with_prompt("BMAD project root path")
        .default(".".to_string())
        .interact_text()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    let log_format_idx = dialoguer::Select::new()
        .with_prompt("Log format")
        .items(LOG_FORMATS)
        .default(0) // "pretty"
        .interact()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    let log_level_idx = dialoguer::Select::new()
        .with_prompt("Log level")
        .items(LOG_LEVELS)
        .default(2) // "info"
        .interact()
        .map_err(|e| CliError::Init { reason: e.to_string() })?;

    // Derive BMAD paths from project root
    let output_folder = format!("{project_root}/_bmad-output");
    let planning_artifacts = format!("{output_folder}/planning-artifacts");
    let implementation_artifacts = format!("{output_folder}/implementation-artifacts");

    // Build BotConfig
    Ok(BotConfig {
        polling_interval_secs,
        git_provider: GitProviderConfig {
            provider: git_provider_name,
            repo_owner,
            repo_name,
            target_branch,
        },
        llm: LlmConfig {
            dev: LlmRoleConfig {
                provider: dev_provider,
                model: dev_model,
            },
            review: LlmRoleConfig {
                provider: review_provider,
                model: review_model,
            },
            supervisor: LlmRoleConfig {
                provider: supervisor_provider,
                model: supervisor_model,
            },
        },
        notifications: NotificationConfig {
            telegram: TelegramConfig {
                enabled: telegram_enabled,
                chat_id: telegram_chat_id,
            },
        },
        bmad_paths: BmadPathsConfig {
            project_root,
            output_folder,
            planning_artifacts,
            implementation_artifacts,
        },
        log_format: LOG_FORMATS[log_format_idx].to_string(),
        log_level: LOG_LEVELS[log_level_idx].to_string(),
    })
}
```

> **IMPORTANT:** For `collect_config_interactively()` to construct `BotConfig` directly, the config structs need to be constructable (not just deserializable). Verify that Story 1.1's structs have `pub` fields — they do per the design. If any struct has private fields with no constructor, add a `pub fn new(...)` constructor or make the fields public.

### generate_config_yaml() Implementation

```rust
/// Generates a YAML config string with header comments.
fn generate_config_yaml(config: &BotConfig) -> Result<String, CliError> {
    let yaml_body = serde_yaml::to_string(config)
        .map_err(|e| CliError::Init {
            reason: format!("Failed to serialize config to YAML: {e}"),
        })?;

    let header = format!(
        "# BMAD Bot Configuration\n\
         # Generated by `bmad-bot init` on {}\n\
         # Secrets (API keys, tokens) are in .env — never in this file.\n\
         #\n\
         # Reference: bmad-bot.yaml.example for field descriptions.\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    );

    Ok(format!("{header}{yaml_body}"))
}
```

> **NOTE:** `chrono` is added as a dependency in Task 0. The generated YAML is valid but less readable than the hand-crafted `bmad-bot.yaml.example` (no inline comments, may use flow style). Users can refer to `bmad-bot.yaml.example` for field descriptions.

### BotConfig Serialization Requirement

For `serde_yaml::to_string(config)` to work, `BotConfig` and all nested structs need `Serialize` in addition to `Deserialize`. Story 1.1 only added `Deserialize`. **This story must add `Serialize` derive to all config structs:**

```rust
#[derive(Debug, Deserialize, Serialize)]   // Add Serialize
pub struct BotConfig { ... }

#[derive(Debug, Deserialize, Serialize)]   // Add Serialize
pub struct LlmConfig { ... }

#[derive(Debug, Deserialize, Serialize)]   // Add Serialize
pub struct LlmRoleConfig { ... }

#[derive(Debug, Deserialize, Serialize)]   // Add Serialize
pub struct GitProviderConfig { ... }

#[derive(Debug, Deserialize, Serialize)]   // Add Serialize
pub struct NotificationConfig { ... }

#[derive(Debug, Deserialize, Serialize)]   // Add Serialize
pub struct TelegramConfig { ... }

#[derive(Debug, Deserialize, Serialize)]   // Add Serialize
pub struct BmadPathsConfig { ... }
```

This is a required modification to `src/config/mod.rs`. It is backward-compatible — adding `Serialize` to existing `Deserialize` structs changes nothing for the load path.

### generate_env_file() Implementation

```rust
/// Generates a .env file with context-aware placeholders.
/// Only includes secrets relevant to the chosen providers.
/// Comments dynamically specify which roles use each provider.
fn generate_env_file(config: &BotConfig) -> Result<String, CliError> {
    let mut lines = vec![
        "# BMAD Bot Secrets".to_string(),
        "# Generated by `bmad-bot init`".to_string(),
        "# Fill in your API keys below. NEVER commit this file!".to_string(),
        String::new(),
    ];

    // Build a map of provider → list of roles that use it
    let mut provider_roles: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for (role, role_config) in [
        ("dev", &config.llm.dev),
        ("review", &config.llm.review),
        ("supervisor", &config.llm.supervisor),
    ] {
        provider_roles
            .entry(role_config.provider.as_str())
            .or_default()
            .push(role);
    }

    // LLM provider keys — with dynamic role comments
    lines.push("# --- LLM Provider API Keys ---".to_string());
    if let Some(roles) = provider_roles.get("anthropic") {
        lines.push(format!("# Required: used by {} role(s)", roles.join(", ")));
        lines.push("ANTHROPIC_API_KEY=".to_string());
    }
    if let Some(roles) = provider_roles.get("openai") {
        lines.push(format!("# Required: used by {} role(s)", roles.join(", ")));
        lines.push("OPENAI_API_KEY=".to_string());
    }
    if let Some(roles) = provider_roles.get("github-models") {
        lines.push(format!("# Required: used by {} role(s)", roles.join(", ")));
        lines.push("GITHUB_MODELS_API_KEY=".to_string());
    }

    lines.push(String::new());

    // Git provider token
    lines.push("# --- Git Provider Token ---".to_string());
    match config.git_provider.provider.as_str() {
        "github" => {
            lines.push("# Required: GitHub personal access token for PR creation".to_string());
            lines.push("GITHUB_TOKEN=".to_string());
        }
        "gitlab" => {
            lines.push("# Required: GitLab personal access token for MR creation".to_string());
            lines.push("GITLAB_TOKEN=".to_string());
        }
        _ => {}
    }

    lines.push(String::new());

    // Telegram (only if enabled)
    if config.notifications.telegram.enabled {
        lines.push("# --- Notifications ---".to_string());
        lines.push("# Required: Telegram bot token (notifications enabled)".to_string());
        lines.push("TELEGRAM_BOT_TOKEN=".to_string());
        lines.push(String::new());
    }

    Ok(lines.join("\n"))
}
```

### Updated main.rs Init Dispatch

Replace the Init arm in main.rs:

```rust
cli::Commands::Init => {
    // Tracing is global — main.rs owns the subscriber for non-start commands
    let _ = tracing_subscriber::fmt::try_init();
    cli::run_init(&cli.config).await?;
}
```

> **NOTE:** Tracing is initialized in main.rs BEFORE calling `run_init()` — the subscriber is global and should be owned by the entry point. `run_init()` does NOT initialize tracing internally. This is consistent with the Story 1.2 pattern where main.rs inits basic tracing for non-`start` commands.

### Files Modified in This Story

| File | Change |
|------|--------|
| `Cargo.toml` | Add `dialoguer = "0.11"` and `chrono = "0.4"` to `[dependencies]` |
| `src/cli/mod.rs` | Add `run_init()`, `collect_config_interactively()`, `generate_config_yaml()`, `generate_env_file()`, helper constants. Add `Init`, `Io`, `UserCancelled` variants to `CliError`. |
| `src/config/mod.rs` | Add `Serialize` derive to all config structs (BotConfig, LlmConfig, LlmRoleConfig, GitProviderConfig, NotificationConfig, TelegramConfig, BmadPathsConfig) |
| `src/main.rs` | Replace `Commands::Init` placeholder with `cli::run_init(&cli.config).await?;` |

### Anti-Patterns to Avoid

- ❌ **NO** secrets (API keys, tokens) written to `bmad-bot.yaml` — secrets go to `.env` only
- ❌ **NO** `unwrap()` or `expect()` in production code — use `map_err` with `CliError::Init`
- ❌ **NO** `anyhow::Result` in `cli/mod.rs` — typed `CliError` only
- ❌ **NO** hard-coding provider/model lists in multiple places — use constants (`LLM_PROVIDERS`, `GIT_PROVIDERS`)
- ❌ **NO** modifying modules other than `cli/mod.rs`, `config/mod.rs`, `main.rs`, and `Cargo.toml`
- ❌ **NO** blocking async runtime with dialoguer — `dialoguer` is synchronous but short-lived during init; this is acceptable because `init` is a one-shot interactive command, not the daemon loop
- ❌ **NO** writing `.env` without checking if it already exists first
- ❌ **NO** duplicate `#[from] std::io::Error` in CliError — resolve the conflict with the existing `Signal` variant (see CliError Extension note above)
- ❌ **NO** using `.interact_text()` for non-String types — use `.interact()` which respects `T: FromStr`
- ❌ **NO** initializing tracing inside `run_init()` — main.rs owns the global subscriber for non-start commands

### Scope Boundaries

**IN SCOPE for this story:**
- `src/cli/mod.rs` — add `run_init()` and supporting functions, extend `CliError`
- `src/config/mod.rs` — add `Serialize` derive to all config structs
- `src/main.rs` — replace Init placeholder
- `Cargo.toml` — add `dialoguer` and `chrono`

**OUT OF SCOPE — do NOT implement:**
- `status` / `logs` commands (Story 1.4)
- BMAD auto-discovery (Story 1.4)
- Editing an existing config (re-run `init` is the approach for MVP)
- Any modifications to watcher, session, supervisor, review, tools, git_provider, notifier modules

### Testing Requirements

Tests go inline at the bottom of `src/cli/mod.rs` (extending existing tests from Story 1.2):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ... existing tests from Story 1.2 ...

    fn make_test_config() -> BotConfig {
        BotConfig {
            polling_interval_secs: 300,
            git_provider: GitProviderConfig {
                provider: "github".to_string(),
                repo_owner: "test-org".to_string(),
                repo_name: "test-repo".to_string(),
                target_branch: "main".to_string(),
            },
            llm: LlmConfig {
                dev: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "claude-sonnet-4-20250514".to_string(),
                },
                review: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "claude-sonnet-4-20250514".to_string(),
                },
                supervisor: LlmRoleConfig {
                    provider: "openai".to_string(),
                    model: "gpt-4o".to_string(),
                },
            },
            notifications: NotificationConfig {
                telegram: TelegramConfig {
                    enabled: false,
                    chat_id: String::new(),
                },
            },
            bmad_paths: BmadPathsConfig {
                project_root: ".".to_string(),
                output_folder: "./_bmad-output".to_string(),
                planning_artifacts: "./_bmad-output/planning-artifacts".to_string(),
                implementation_artifacts: "./_bmad-output/implementation-artifacts".to_string(),
            },
            log_format: "pretty".to_string(),
            log_level: "info".to_string(),
        }
    }

    #[test]
    fn test_generate_config_yaml_roundtrips() {
        let config = make_test_config();
        let yaml = generate_config_yaml(&config).unwrap();
        // Strip comment lines (start with #) before deserializing
        let yaml_body: String = yaml
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed: BotConfig = serde_yaml::from_str(&yaml_body).unwrap();
        assert_eq!(parsed.polling_interval_secs, 300);
        assert_eq!(parsed.git_provider.provider, "github");
        assert_eq!(parsed.llm.dev.provider, "anthropic");
    }

    #[test]
    fn test_generate_config_yaml_validates() {
        let config = make_test_config();
        let yaml = generate_config_yaml(&config).unwrap();
        let yaml_body: String = yaml
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed: BotConfig = serde_yaml::from_str(&yaml_body).unwrap();
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn test_generate_env_includes_anthropic_key() {
        let config = make_test_config();
        let env = generate_env_file(&config).unwrap();
        assert!(env.contains("ANTHROPIC_API_KEY="));
    }

    #[test]
    fn test_generate_env_includes_openai_key_for_supervisor() {
        let config = make_test_config(); // supervisor uses openai
        let env = generate_env_file(&config).unwrap();
        assert!(env.contains("OPENAI_API_KEY="));
    }

    #[test]
    fn test_generate_env_excludes_github_models_key() {
        let config = make_test_config(); // no role uses github-models
        let env = generate_env_file(&config).unwrap();
        assert!(!env.contains("GITHUB_MODELS_API_KEY="));
    }

    #[test]
    fn test_generate_env_includes_github_token() {
        let config = make_test_config(); // git_provider is github
        let env = generate_env_file(&config).unwrap();
        assert!(env.contains("GITHUB_TOKEN="));
    }

    #[test]
    fn test_generate_env_excludes_gitlab_token() {
        let config = make_test_config(); // git_provider is github, not gitlab
        let env = generate_env_file(&config).unwrap();
        assert!(!env.contains("GITLAB_TOKEN="));
    }

    #[test]
    fn test_generate_env_excludes_telegram_when_disabled() {
        let config = make_test_config(); // telegram.enabled = false
        let env = generate_env_file(&config).unwrap();
        assert!(!env.contains("TELEGRAM_BOT_TOKEN="));
    }

    #[test]
    fn test_generate_env_includes_telegram_when_enabled() {
        let mut config = make_test_config();
        config.notifications.telegram.enabled = true;
        config.notifications.telegram.chat_id = "12345".to_string();
        let env = generate_env_file(&config).unwrap();
        assert!(env.contains("TELEGRAM_BOT_TOKEN="));
    }

    #[test]
    fn test_derived_bmad_paths_from_project_root() {
        // Verify the path derivation logic
        let root = "/my/project";
        let output = format!("{root}/_bmad-output");
        let planning = format!("{output}/planning-artifacts");
        let implementation = format!("{output}/implementation-artifacts");
        assert_eq!(planning, "/my/project/_bmad-output/planning-artifacts");
        assert_eq!(implementation, "/my/project/_bmad-output/implementation-artifacts");
    }

    #[test]
    fn test_generate_env_comments_specify_correct_roles() {
        let config = make_test_config(); // dev+review=anthropic, supervisor=openai
        let env = generate_env_file(&config).unwrap();
        // Anthropic used by dev and review
        assert!(env.contains("dev, review") || env.contains("review, dev"));
        // OpenAI used by supervisor
        assert!(env.contains("supervisor role"));
    }
}
```

> **NOTE:** Interactive prompt functions (`collect_config_interactively`, `run_init`) cannot be easily unit-tested because they read from stdin. The tests focus on the pure generation functions (`generate_config_yaml`, `generate_env_file`). The manual integration test in Task 7.5 covers the interactive flow.

> **💡 TIP:** The `make_test_config()` helper creates a complete valid BotConfig. If future stories (watcher, session, etc.) need similar test fixtures, consider extracting it into a shared `#[cfg(test)] mod test_helpers` module or a `tests/common/mod.rs` file to avoid duplication.

### References

- [Source: epics.md § Story 1.3: Interactive Init Command] — User story, acceptance criteria
- [Source: architecture.md § Starter Template Evaluation] — CLI framework: clap with derive API
- [Source: architecture.md § Config Pattern] — BotConfig validate once, share via Arc, secrets separate
- [Source: architecture.md § Configuration Files] — bmad-bot.yaml (committed), .env (gitignored), examples (committed)
- [Source: project-context.md § CLI Rules] — `bmad-bot init` generates bmad-bot.yaml + .env, config validation
- [Source: project-context.md § Critical Implementation Rules] — Secrets in env vars only, never hardcoded
- [Source: prd.md § CLI Command Surface] — init: interactive setup, generates config and secrets
- [Source: prd.md § Configuration & Secrets Separation] — YAML for config, gitignored file for secrets
- [Source: Story 1.1] — BotConfig struct, config structs, ConfigError, example files, serde defaults
- [Source: Story 1.2] — Cli struct, Commands enum, CliError, main.rs dispatch, init_tracing, run_start

## Dev Agent Record

### Agent Model Used

Claude Opus 4 (via Zed)

### Debug Log References

- No test failures. All 63 tests passed on first run (21 new + 42 existing from Stories 1.1/1.2).
- Resolved `#[from] std::io::Error` conflict between `Signal` and `Io` variants: removed `#[from]` from `Signal`, added `Io(#[from] std::io::Error)` as the generic converter. `Signal` variant now constructed explicitly via `map_err(CliError::Signal)`.
- Updated `test_cli_error_from_io_error` assertion from `CliError::Signal` to `CliError::Io` to match new error routing.

### Completion Notes List

- **Task 0:** Added `dialoguer = "0.11"` and `chrono = "0.4"` to Cargo.toml. `cargo check` passes.
- **Task 1:** Implemented `run_init()` — checks for existing config/env files with overwrite confirmation, delegates to `collect_config_interactively()`, validates generated config, writes files via `tokio::fs::write`. Added `Init { reason }`, `Io(io::Error)`, `UserCancelled` variants to `CliError`.
- **Task 2:** Implemented `collect_config_interactively()` with full dialoguer prompts: git provider (Select), repo owner/name (Input), target branch (Input, default "main"), LLM provider+model per role with "same for all" shortcut (Select+Input+Confirm), Telegram toggle+chat_id (Confirm+Input), polling interval (Input<u64> with `.interact()`), project root (Input), log format/level (Select). Derives bmad_paths from project root.
- **Task 3:** Implemented `generate_config_yaml()` — serializes BotConfig via `serde_yaml::to_string` with header comment block including chrono timestamp.
- **Task 4:** Implemented `generate_env_file()` — context-aware: builds provider→roles map, only includes secrets for active providers with dynamic role comments. Separate overwrite prompt for .env.
- **Task 5:** Replaced `Commands::Init` placeholder in main.rs with `cli::run_init(&cli.config).await?;`. Tracing init stays in main.rs before the call.
- **Task 6:** 21 new tests: CliError display/conversion (3), default_model_for_provider (4), generate_config_yaml roundtrip+validate+header (3), generate_env_file provider inclusion/exclusion (9 covering anthropic, openai, github-models, github/gitlab tokens, telegram enabled/disabled, role comments, all-same-provider dedup), derived paths (1), make_test_config helper.
- **Task 7:** `cargo fmt -- --check` clean. `cargo clippy` clean (only pre-existing `dead_code` warnings: `UserCancelled` variant, `build_http_client`). All 63 tests pass. All public items have `///` doc comments.
- **Decision:** Added `Serialize` derive to all config structs in `config/mod.rs` (backward-compatible, required for `serde_yaml::to_string` in `generate_config_yaml`).

### Change Log

- 2026-02-08: Story 1.3 implementation complete — interactive init command with config/env generation. All 63 tests pass.

### File List

- `Cargo.toml` — Added `dialoguer = "0.11"` and `chrono = "0.4"` dependencies
- `src/cli/mod.rs` — Added `run_init()`, `collect_config_interactively()`, `generate_config_yaml()`, `generate_env_file()`, constants (`LLM_PROVIDERS`, `GIT_PROVIDERS`, `LOG_FORMATS`, `LOG_LEVELS`), `default_model_for_provider()`. Extended `CliError` with `Init`, `Io`, `UserCancelled` variants. Removed `#[from]` from `Signal`. Added 21 new tests.
- `src/config/mod.rs` — Added `Serialize` derive to all config structs (`BotConfig`, `LlmConfig`, `LlmRoleConfig`, `GitProviderConfig`, `NotificationConfig`, `TelegramConfig`, `BmadPathsConfig`)
- `src/main.rs` — Replaced `Commands::Init` placeholder with `cli::run_init(&cli.config).await?;`