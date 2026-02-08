# Story 1.1: Project Scaffolding, Configuration & Validation

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want to initialize the BMAD Bot project with a complete module structure and robust configuration loading,
So that I have a solid foundation to build all daemon features on.

## Acceptance Criteria

1. **Given** the project does not yet exist **When** I run `cargo init bmad-bot` and set up the project **Then** a Rust project is created with edition 2024, single binary target, and all required dependencies in Cargo.toml (tokio, rig-core, git2, serde, serde_yaml, dotenvy, clap, thiserror, tracing, tracing-subscriber, octocrab, reqwest, reqwest-middleware, reqwest-retry, async-trait) **And** the complete module directory structure is scaffolded with stub `mod.rs` files for all modules (cli, config, watcher, session, supervisor, review, tools, git_provider, notifier)

2. **Given** a valid `bmad-bot.yaml` configuration file exists in the project root **When** the config module loads the file **Then** a `BotConfig` struct is deserialized via serde_yaml containing all configuration fields (polling_interval_secs, git_provider, llm providers/models, notification config, BMAD paths) **And** secrets are loaded separately from `.env` via dotenvy and never stored in `BotConfig`

3. **Given** the project HTTP client is initialized **When** any module needs to make external HTTP calls (LLM providers, GitHub/GitLab API, Telegram API) **Then** a shared `reqwest` client is configured with `reqwest-middleware` and `reqwest-retry` for automatic retry with exponential backoff (max 3 retries) on transient errors (429, 500, 503, timeouts) **And** the retry client is available to all modules from project inception — no HTTP call in any epic runs without retry resilience

4. **Given** a `bmad-bot.yaml` with missing or invalid fields **When** the config module validates the configuration **Then** a descriptive `ConfigError` (thiserror enum) is returned specifying exactly which field failed and why **And** `ConfigError` follows the per-module thiserror pattern (no anyhow in library modules)

5. **Given** the project is initialized **When** I inspect the repository **Then** `bmad-bot.yaml.example` and `.env.example` template files exist and are committed **And** `.env` is listed in `.gitignore`

## Tasks / Subtasks

- [x] Task 1: Initialize Rust project (AC: #1)
  - [x] 1.1 Run `cargo init` with binary target
  - [x] 1.2 Set `edition = "2024"` in Cargo.toml
  - [x] 1.3 Add ALL dependencies to Cargo.toml (see Dev Notes for exact list, including `[dev-dependencies]`)
  - [x] 1.4 Verify `cargo check` passes with all dependencies resolved

- [x] Task 2: Scaffold module directory structure (AC: #1)
  - [x] 2.1 Create `src/cli/mod.rs` with placeholder public module
  - [x] 2.2 Create `src/config/mod.rs` with placeholder public module
  - [x] 2.3 Create `src/watcher/mod.rs` and `src/watcher/deps.rs` with placeholders
  - [x] 2.4 Create `src/session/mod.rs` and `src/session/state.rs` with placeholders
  - [x] 2.5 Create `src/supervisor/mod.rs`, `src/supervisor/rules.rs`, `src/supervisor/decisions.rs` with placeholders
  - [x] 2.6 Create `src/review/mod.rs` with placeholder
  - [x] 2.7 Create `src/tools/mod.rs`, `src/tools/git.rs`, `src/tools/fs.rs`, `src/tools/terminal.rs` with placeholders
  - [x] 2.8 Create `src/git_provider/mod.rs`, `src/git_provider/github.rs`, `src/git_provider/gitlab.rs` with placeholders
  - [x] 2.9 Create `src/notifier/mod.rs` with placeholder
  - [x] 2.10 Create `tests/e2e/mod.rs` placeholder (gated behind `BMAD_E2E=1`)
  - [x] 2.11 Wire all modules into `src/main.rs` using the main.rs skeleton (see Dev Notes)
  - [x] 2.12 Use `#![warn(dead_code)]` at crate root for this story (stubs are mostly empty); add `// FIXME: Change to #![deny(dead_code)] once all modules have real implementations`
  - [x] 2.13 Add `#![deny(clippy::all)]` at crate root
  - [x] 2.14 Verify `cargo check` passes with all modules wired

- [x] Task 3: Implement BotConfig struct and YAML loading (AC: #2)
  - [x] 3.1 Define `BotConfig` struct with all fields and serde defaults (see Dev Notes for complete struct design)
  - [x] 3.2 Define nested config structs: `LlmConfig`, `LlmRoleConfig`, `GitProviderConfig`, `NotificationConfig`, `BmadPathsConfig`
  - [x] 3.3 Implement `BotConfig::load(path: &Path) -> Result<Self, ConfigError>` using serde_yaml
  - [x] 3.4 Implement secrets loading from `.env` via dotenvy (separate from BotConfig)
  - [x] 3.5 Secrets struct: `BotSecrets` with api key fields loaded from env vars

- [x] Task 4: Implement config validation (AC: #4)
  - [x] 4.1 Define `ConfigError` thiserror enum with descriptive variants
  - [x] 4.2 Implement `BotConfig::validate(&self) -> Result<(), ConfigError>`
  - [x] 4.3 Validate: polling_interval_secs > 0
  - [x] 4.4 Validate: git_provider is recognized ("github" or "gitlab")
  - [x] 4.5 Validate: LLM provider names are recognized ("anthropic", "openai", "github-models")
  - [x] 4.6 Validate: required paths are non-empty
  - [x] 4.7 Validate: BotSecrets has all required API keys based on configured providers
  - [x] 4.8 Return descriptive error messages specifying exactly which field failed and why

- [x] Task 5: Set up shared HTTP client with retry middleware (AC: #3)
  - [x] 5.1 Create `HttpClient` wrapper or type alias using `reqwest-middleware::ClientWithMiddleware`
  - [x] 5.2 Configure `reqwest-retry` with `ExponentialBackoff`, max 3 retries, retry on 429/500/503/timeouts
  - [x] 5.3 Expose a factory function: `build_http_client() -> ClientWithMiddleware`
  - [x] 5.4 Store in config or pass alongside `Arc<BotConfig>` to all modules

- [x] Task 6: Create example and gitignore files (AC: #5)
  - [x] 6.1 Create `bmad-bot.yaml.example` (see Dev Notes for exact content)
  - [x] 6.2 Create `.env.example` (see Dev Notes for exact content)
  - [x] 6.3 Create `.gitignore` (see Dev Notes for exact content)

- [x] Task 7: Write unit tests for config module (AC: #2, #4)
  - [x] 7.1 Test valid config loads and deserializes correctly
  - [x] 7.2 Test missing required field returns descriptive ConfigError
  - [x] 7.3 Test invalid polling_interval (0) returns error
  - [x] 7.4 Test unknown git provider returns error
  - [x] 7.5 Test unknown LLM provider returns error
  - [x] 7.6 Test secrets loading from env vars
  - [x] 7.7 Test HTTP client builds with retry middleware
  - [x] 7.8 Test default values are applied when optional fields omitted

- [x] Task 8: Final quality checks
  - [x] 8.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [x] 8.2 Run `cargo clippy` and fix any warnings
  - [x] 8.3 Run `cargo test` and verify all tests pass
  - [x] 8.4 Verify all public items have `///` doc comments

## Dev Notes

### Technical Stack — Exact Versions

All crates use latest stable versions. The project uses Rust edition 2024 (requires rustc 1.86.0+).

**Cargo.toml:**

```toml
[package]
name = "bmad-bot"
version = "0.1.0"
edition = "2024"

[dependencies]
tokio = { version = "1", features = ["full"] }
rig-core = "0.30"
git2 = "0.19"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
dotenvy = "0.15"
clap = { version = "4", features = ["derive"] }
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
octocrab = "0.41"
reqwest = { version = "0.12", features = ["json"] }
reqwest-middleware = "0.4"
reqwest-retry = "0.7"
async-trait = "0.1"

[dev-dependencies]
tempfile = "3"
```

> **IMPORTANT:** Only `main.rs` may use `anyhow::Result`. All library modules use typed `thiserror` errors exclusively.

### main.rs Skeleton

The entry point must contain crate-level attributes, all module declarations, and a minimal async main with basic tracing init. Story 1.2 will enhance tracing (JSON output, env-filter, config-driven format); this story only needs `tracing_subscriber::fmt::init()` so that no `println!` is ever needed.

```rust
#![deny(clippy::all)]
#![warn(dead_code)] // FIXME: Change to #![deny(dead_code)] once all modules have real implementations

mod cli;
mod config;
mod git_provider;
mod notifier;
mod review;
mod session;
mod supervisor;
mod tools;
mod watcher;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Minimal tracing init — Story 1.2 replaces with config-driven setup
    tracing_subscriber::fmt::init();

    tracing::info!("bmad-bot starting");

    // Story 1.2 adds CLI dispatch (clap) and daemon lifecycle here
    Ok(())
}
```

### Stub Module Pattern

Every placeholder module file MUST follow this pattern to compile cleanly under `#![warn(dead_code)]`:

```rust
//! Watcher module — polls sprint-status.yaml for ready-for-dev stories.
//!
//! TODO: Implemented in Story 2.1
```

For modules with submodules (e.g., `watcher/mod.rs` that owns `deps.rs`):

```rust
//! Watcher module — polls sprint-status.yaml for ready-for-dev stories.
//!
//! TODO: Implemented in Story 2.1

mod deps;
```

And `watcher/deps.rs`:

```rust
//! Dependency graph resolution and pre-gate logic.
//!
//! TODO: Implemented in Story 2.2
```

Apply the same pattern to all stub modules — each file gets a `//!` doc comment explaining its purpose and a `// TODO: Story X.Y` indicating which story implements it.

### BotConfig Struct Design

```rust
/// Top-level daemon configuration loaded from `bmad-bot.yaml`.
#[derive(Debug, Deserialize)]
pub struct BotConfig {
    /// Polling interval in seconds. Must be > 0.
    #[serde(default = "default_polling_interval")]
    pub polling_interval_secs: u64,
    pub git_provider: GitProviderConfig,
    pub llm: LlmConfig,
    pub notifications: NotificationConfig,
    pub bmad_paths: BmadPathsConfig,
}

fn default_polling_interval() -> u64 { 300 }

/// LLM provider configuration for each agent role.
#[derive(Debug, Deserialize)]
pub struct LlmConfig {
    pub dev: LlmRoleConfig,
    pub review: LlmRoleConfig,
    pub supervisor: LlmRoleConfig,
}

/// Provider + model pair for a single LLM role.
#[derive(Debug, Deserialize)]
pub struct LlmRoleConfig {
    /// One of: "anthropic", "openai", "github-models"
    pub provider: String,
    /// Model identifier, e.g. "claude-sonnet-4-20250514", "gpt-4o"
    pub model: String,
}

/// Git hosting provider configuration.
#[derive(Debug, Deserialize)]
pub struct GitProviderConfig {
    /// One of: "github", "gitlab"
    pub provider: String,
    pub repo_owner: String,
    pub repo_name: String,
    /// Branch PRs target. Defaults to "main".
    #[serde(default = "default_target_branch")]
    pub target_branch: String,
}

fn default_target_branch() -> String { "main".to_string() }

/// Notification channel configuration.
#[derive(Debug, Deserialize)]
pub struct NotificationConfig {
    pub telegram: TelegramConfig,
}

/// Telegram notification settings.
#[derive(Debug, Deserialize)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Telegram chat ID to send notifications to.
    #[serde(default)]
    pub chat_id: String,
}

/// Paths to BMAD project artifacts.
#[derive(Debug, Deserialize)]
pub struct BmadPathsConfig {
    pub project_root: String,
    pub output_folder: String,
    pub planning_artifacts: String,
    pub implementation_artifacts: String,
}
```

### BotSecrets — Loaded Separately via dotenvy

```rust
/// Secrets loaded from .env file — NEVER stored in BotConfig or logged.
pub struct BotSecrets {
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub github_models_api_key: Option<String>,
    pub github_token: Option<String>,
    pub gitlab_token: Option<String>,
    pub telegram_bot_token: Option<String>,
}
```

Secrets are loaded via `dotenvy::dotenv()` + `std::env::var()`. The `_env` suffix convention in config maps to env var names (e.g., `api_key_env: ANTHROPIC_API_KEY` → reads `ANTHROPIC_API_KEY` from environment).

### ConfigError Pattern

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file '{path}': {source}")]
    FileRead { path: String, source: std::io::Error },

    #[error("Failed to parse config YAML: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    #[error("Invalid config value for '{field}': {reason}")]
    InvalidField { field: String, reason: String },

    #[error("Missing required config field: '{field}'")]
    MissingField { field: String },

    #[error("Missing required secret: environment variable '{env_var}' not set (needed for {purpose})")]
    MissingSecret { env_var: String, purpose: String },

    #[error("Failed to load .env file: {0}")]
    DotenvError(#[from] dotenvy::Error),
}
```

### HTTP Client with Retry Middleware

```rust
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

/// Builds a shared HTTP client with automatic retry on transient errors
/// (429, 500, 503, timeouts). Max 3 retries with exponential backoff.
/// ALL HTTP calls in the project MUST use this client.
pub fn build_http_client() -> ClientWithMiddleware {
    let retry_policy = ExponentialBackoff::builder()
        .build_with_max_retries(3);

    ClientBuilder::new(reqwest::Client::new())
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}
```

### bmad-bot.yaml.example Content

Create this file exactly — it serves as the canonical config reference:

```yaml
# BMAD Bot Configuration
# Copy this file to bmad-bot.yaml and fill in your values.

# How often (seconds) the daemon polls sprint-status.yaml for new stories.
# Default: 300 (5 minutes)
polling_interval_secs: 300

# Git hosting provider for PR creation
git_provider:
  provider: github            # "github" or "gitlab"
  repo_owner: your-org
  repo_name: your-repo
  target_branch: main         # Branch that PRs target (default: main)

# LLM provider configuration — one provider+model per role
# Supported providers: "anthropic", "openai", "github-models"
# API keys are loaded from .env (see .env.example), never from this file.
llm:
  dev:
    provider: anthropic
    model: claude-sonnet-4-20250514
  review:
    provider: anthropic
    model: claude-sonnet-4-20250514
  supervisor:
    provider: anthropic
    model: claude-sonnet-4-20250514

# Notification channels
notifications:
  telegram:
    enabled: false
    chat_id: ""               # Your Telegram chat ID

# BMAD project paths — adjust to match your project layout
bmad_paths:
  project_root: "."
  output_folder: "_bmad-output"
  planning_artifacts: "_bmad-output/planning-artifacts"
  implementation_artifacts: "_bmad-output/implementation-artifacts"
```

### .env.example Content

Create this file exactly — it lists every secret the daemon may need:

```bash
# BMAD Bot Secrets
# Copy this file to .env and fill in your API keys.
# NEVER commit .env to version control!

# --- LLM Provider API Keys ---
# Only the key(s) for your configured provider(s) are required.
ANTHROPIC_API_KEY=
OPENAI_API_KEY=
GITHUB_MODELS_API_KEY=

# --- Git Provider Tokens ---
# Only the token for your configured git provider is required.
GITHUB_TOKEN=
GITLAB_TOKEN=

# --- Notifications ---
# Required only if notifications.telegram.enabled = true
TELEGRAM_BOT_TOKEN=
```

### .gitignore Content

```gitignore
# Build artifacts
/target

# Secrets — NEVER commit
.env

# Session WAL file (transient, exists only during active session)
_bmad-output/implementation-artifacts/.bmad-bot-session.yaml

# OS files
.DS_Store
```

### rig-core Tool Trait Pattern — REFERENCE ONLY

> **⚠️ NOT IMPLEMENTED IN THIS STORY.** This section is included as architectural context so the dev agent understands the framework choices. Tools are implemented starting in Epic 4 (Story 4.1). Do NOT write any tool implementations in this story.

The rig-core crate (v0.30) uses this pattern for tools — ALL future tools in the project MUST follow this structure:

```rust
use rig::tool::{Tool, ToolDyn};
use rig::completion::ToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize)]
pub struct MyTool { /* shared state */ }

#[derive(Deserialize)]
pub struct MyToolArgs { /* action-specific params */ }

#[derive(Debug, thiserror::Error)]
pub enum MyToolError {
    #[error("...")]
    SomeError(/* ... */),
}

impl Tool for MyTool {
    const NAME: &'static str = "my_tool";
    type Error = MyToolError;
    type Args = MyToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "my_tool".to_string(),
            description: "...".to_string(),
            parameters: json!({ "type": "object", "properties": { /* ... */ } }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok("result".to_string())
    }
}
```

Agent construction follows the builder pattern:

```rust
let agent = provider
    .agent(model)
    .preamble(&preamble)
    .tool(my_tool)
    .max_tokens(1024)
    .build();

// Multi-turn chat uses the Chat trait:
// agent.chat(prompt, chat_history).await
```

### Project Structure Notes

The complete directory structure to scaffold:

```
bmad-bot/
├── Cargo.toml
├── Cargo.lock                        # Generated by cargo
├── README.md
├── .gitignore
├── .env.example                      # Template secrets (API keys)
├── bmad-bot.yaml.example             # Template config (committed)
├── src/
│   ├── main.rs                       # Entry point, mod declarations, #![deny(clippy::all)]
│   ├── cli/
│   │   └── mod.rs                    # clap: init, start, status, logs (placeholder)
│   ├── config/
│   │   └── mod.rs                    # BotConfig, BotSecrets, YAML + .env loading, validation
│   ├── watcher/
│   │   ├── mod.rs                    # Polling loop placeholder
│   │   └── deps.rs                   # Dependency graph placeholder
│   ├── session/
│   │   ├── mod.rs                    # rig agent setup placeholder
│   │   └── state.rs                  # Session WAL file placeholder
│   ├── supervisor/
│   │   ├── mod.rs                    # ask_supervisor tool placeholder
│   │   ├── rules.rs                  # Rule engine placeholder
│   │   └── decisions.rs              # Decision logging placeholder
│   ├── review/
│   │   └── mod.rs                    # Code review session placeholder
│   ├── tools/
│   │   ├── mod.rs                    # Tool registration helpers placeholder
│   │   ├── git.rs                    # Git tool placeholder
│   │   ├── fs.rs                     # Filesystem tool placeholder
│   │   └── terminal.rs              # Terminal tool placeholder
│   ├── git_provider/
│   │   ├── mod.rs                    # GitProvider trait + factory placeholder
│   │   ├── github.rs                # GitHub impl placeholder
│   │   └── gitlab.rs                # GitLab impl placeholder
│   └── notifier/
│       └── mod.rs                    # Notifier trait + Telegram impl placeholder
└── tests/
    └── e2e/
        └── mod.rs                    # E2E tests (gated behind BMAD_E2E=1)
```

### Alignment with Unified Project Structure

- All paths match the architecture document's directory structure exactly [Source: architecture.md § Project Structure & Boundaries]
- Module names follow snake_case convention per Rust standards
- `git_provider` (not `git-provider`) for Rust module naming compatibility
- Single crate, no workspace — matches architecture decision for MVP

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code (tests only)
- ❌ **NO** `anyhow::Result` in `config/mod.rs` or any library module
- ❌ **NO** `println!` or `eprintln!` anywhere — use `tracing` (basic `tracing_subscriber::fmt::init()` is set up in main.rs in this story)
- ❌ **NO** secrets (API keys, tokens) stored in `BotConfig` struct
- ❌ **NO** secrets logged via `tracing` — filter sensitive fields
- ❌ **NO** real API calls in unit tests
- ❌ **NO** skipping `///` doc comments on public structs, enums, traits, functions
- ❌ **NO** empty stub files — every file needs at least a `//!` module doc comment

### Testing Requirements

All tests must follow the naming convention `test_{module}_{behavior}_{scenario}` and use Arrange → Act → Assert pattern.

Tests go inline at the bottom of `src/config/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_load_valid_yaml() {
        // Arrange: create YAML string with all required fields
        let yaml = r#"
            polling_interval_secs: 60
            git_provider:
              provider: github
              repo_owner: test-org
              repo_name: test-repo
            llm:
              dev:
                provider: anthropic
                model: claude-sonnet-4-20250514
              review:
                provider: anthropic
                model: claude-sonnet-4-20250514
              supervisor:
                provider: openai
                model: gpt-4o
            notifications:
              telegram:
                enabled: false
                chat_id: ""
            bmad_paths:
              project_root: "."
              output_folder: "_bmad-output"
              planning_artifacts: "_bmad-output/planning-artifacts"
              implementation_artifacts: "_bmad-output/implementation-artifacts"
        "#;
        // Act: deserialize
        let config: BotConfig = serde_yaml::from_str(yaml).unwrap();
        // Assert: fields populated correctly
        assert_eq!(config.polling_interval_secs, 60);
        assert_eq!(config.git_provider.provider, "github");
    }

    #[test]
    fn test_config_validate_rejects_zero_polling_interval() {
        // Arrange: BotConfig with polling_interval_secs = 0
        // Act: config.validate()
        // Assert: Err(ConfigError::InvalidField { field: "polling_interval_secs", .. })
    }

    #[test]
    fn test_config_validate_rejects_unknown_git_provider() {
        // Arrange: BotConfig with git_provider.provider = "bitbucket"
        // Act: config.validate()
        // Assert: Err(ConfigError::InvalidField { field: "git_provider.provider", .. })
    }

    #[test]
    fn test_config_validate_rejects_unknown_llm_provider() {
        // Arrange: BotConfig with llm.dev.provider = "gemini"
        // Act: config.validate()
        // Assert: Err(ConfigError::InvalidField { field: "llm.dev.provider", .. })
    }

    #[test]
    fn test_config_default_values_applied() {
        // Arrange: YAML without polling_interval_secs and without target_branch
        // Act: deserialize
        // Assert: polling_interval_secs == 300, target_branch == "main"
    }

    #[test]
    fn test_secrets_loads_from_env() {
        // Arrange: set env var ANTHROPIC_API_KEY
        // Act: BotSecrets::load()
        // Assert: anthropic_api_key == Some(...)
    }

    #[test]
    fn test_http_client_builds_successfully() {
        // Arrange: nothing
        // Act: build_http_client()
        // Assert: returns ClientWithMiddleware (no panic)
    }
}
```

Use `tempfile` crate (available as dev-dependency) for file-based tests, or inline YAML strings with `serde_yaml::from_str` as shown above.

### References

- [Source: epics.md § Story 1.1: Project Scaffolding, Configuration & Validation] — User story, acceptance criteria
- [Source: architecture.md § Starter Template Evaluation] — cargo init approach, dependency list, project structure
- [Source: architecture.md § Core Architectural Decisions] — Decision 1-6, error propagation, agent prompt composition
- [Source: architecture.md § Implementation Patterns & Consistency Rules] — Error Type Pattern, Rig Tool Pattern, Tracing Pattern, Config Pattern, Test Mock Pattern
- [Source: architecture.md § Project Structure & Boundaries] — Complete directory tree, module communication map, configuration files
- [Source: project-context.md § Technology Stack & Versions] — Rust edition 2024, rustc 1.86.0+, all crate selections
- [Source: project-context.md § Critical Implementation Rules] — Language rules, framework rules, testing rules, code quality rules
- [Source: project-context.md § CLI Rules] — Command surface, config validation at startup
- [Source: project-context.md § Resilience Rules] — Retry with backoff, notification non-blocking
- [Source: prd.md § CLI Command Surface] — init, start, status, logs commands
- [Source: prd.md § Developer Tool Specific Requirements] — Single binary, graceful shutdown, config validation
- [Source: prd.md § Functional Requirements FR27-FR36] — CLI & Config and Error Handling FRs
- [Source: rig-core docs] — Tool trait signature (v0.30), Agent builder pattern, Chat trait for multi-turn conversations

## Dev Agent Record

### Agent Model Used

Claude Opus 4 (via Windsurf)

### Debug Log References

- Rust updated from 1.86.0 → 1.93.0 (required by rig-core 0.30 `let chains`)
- Crate versions bumped to latest: git2 0.20, octocrab 0.49, reqwest 0.13, reqwest-middleware 0.5, reqwest-retry 0.9
- Edition 2024 makes `std::env::set_var`/`remove_var` unsafe — env-dependent tests restructured to avoid parallel race conditions
- Clippy 1.93 requires `is_none_or()` instead of `map_or(true, ...)` on `Option<&T>`
- `serde_yaml 0.9` shows deprecated warning — upstream recommends migration, acceptable for now

### Completion Notes List

- ✅ All 8 tasks and 48 subtasks implemented and verified
- ✅ 22 unit tests passing (0 failures, 0 ignored)
- ✅ `cargo fmt -- --check` clean
- ✅ `cargo clippy` — zero errors, only expected `dead_code` warnings from stub modules (`#![warn(dead_code)]`)
- ✅ All public structs, enums, functions, and fields have `///` doc comments
- ✅ All stub modules have `//!` module doc comments with TODO references to implementing story
- ✅ No `anyhow` in config module — typed `ConfigError` with `thiserror` exclusively
- ✅ No `println!`/`eprintln!` — tracing only
- ✅ `build_http_client()` factory with ExponentialBackoff, max 3 retries via reqwest-middleware
- ✅ `BotSecrets::validate_for_config()` checks required API keys per configured provider

### Change Log

- 2026-02-07: Story 1.1 implementation complete — project scaffolded, config module implemented with full validation, 22 tests passing

### File List

- `Cargo.toml` (new) — project manifest with all dependencies
- `Cargo.lock` (new) — generated lockfile
- `src/main.rs` (modified) — crate root with module declarations, `#![deny(clippy::all)]`, `#![warn(dead_code)]`, async main with tracing init
- `src/config/mod.rs` (new) — BotConfig, BotSecrets, ConfigError, build_http_client, validation logic, 22 unit tests
- `src/cli/mod.rs` (new) — stub
- `src/watcher/mod.rs` (new) — stub with deps submodule
- `src/watcher/deps.rs` (new) — stub
- `src/session/mod.rs` (new) — stub with state submodule
- `src/session/state.rs` (new) — stub
- `src/supervisor/mod.rs` (new) — stub with rules and decisions submodules
- `src/supervisor/rules.rs` (new) — stub
- `src/supervisor/decisions.rs` (new) — stub
- `src/review/mod.rs` (new) — stub
- `src/tools/mod.rs` (new) — stub with git, fs, terminal submodules
- `src/tools/git.rs` (new) — stub
- `src/tools/fs.rs` (new) — stub
- `src/tools/terminal.rs` (new) — stub
- `src/git_provider/mod.rs` (new) — stub with github and gitlab submodules
- `src/git_provider/github.rs` (new) — stub
- `src/git_provider/gitlab.rs` (new) — stub
- `src/notifier/mod.rs` (new) — stub
- `tests/e2e/mod.rs` (new) — E2E test placeholder gated behind BMAD_E2E=1
- `bmad-bot.yaml.example` (new) — canonical config template
- `.env.example` (new) — secrets template
- `.gitignore` (modified) — project-specific gitignore with .env exclusion