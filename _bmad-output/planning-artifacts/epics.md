---
stepsCompleted: ['step-01-validate-prerequisites', 'step-02-design-epics', 'step-03-create-stories', 'step-04-final-validation']
inputDocuments: ['_bmad-output/planning-artifacts/prd.md', '_bmad-output/planning-artifacts/architecture.md']
---

# BMAD Bot - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for BMAD Bot, decomposing the requirements from the PRD and Architecture requirements into implementable stories.

## Requirements Inventory

### Functional Requirements

**Story Management**
- FR1: The daemon can detect stories with `ready-for-dev` status by polling `sprint-status.yaml` at a configurable interval
- FR2: The daemon can resolve story dependencies and determine correct execution order
- FR3: The daemon can skip stories whose dependencies are not yet completed
- FR4: The daemon can mark dependent stories as `blocked` when a prerequisite story fails

**Pre-Development Preparation**
- FR5: The agent can review previously completed stories and their implementation before starting a new story
- FR6: The agent can update the current story's specs and acceptance criteria based on actual implementation of prior stories
- FR7: The agent can create and checkout a git branch following the `story/{epic}-{story}` naming convention

**Development Session**
- FR8: The daemon can instantiate a rig agent session with the BMAD dev agent persona
- FR9: The daemon can expose git, filesystem, and terminal tools to the agent via rig tool calling
- FR10: The agent can execute the full BMAD `dev-story` workflow autonomously
- FR11: The daemon can inject a session language override (English) via the system prompt without modifying repo files

**Supervision**
- FR12: The supervisor can intercept agent questions during a development session
- FR13: The supervisor can answer predictable questions via a deterministic rule engine (confirmations, step-by-step detection, story selection)
- FR14: The supervisor can answer substantive questions via LLM fallback using full project documentation as context
- FR15: The supervisor can escalate to human when neither rules nor LLM can answer confidently
- FR16: The supervisor can log every decision with the question, chosen answer, reasoning, and alternatives considered
- FR17: The supervisor can commit a decisions file at `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md`

**Code Review**
- FR18: The daemon can optionally launch a code review using a separate LLM after the development session (configurable: enabled/disabled)
- FR19: When enabled, the review agent can commit fixes in a separate commit (distinct from dev commits)
- FR20: When enabled, the review agent can post its review as a comment on the PR

**Pull Request Management**
- FR21: The daemon can create a Pull Request on GitHub with an agent-written description
- FR22: The PR description includes a dedicated "Supervisor Decisions" section listing all decisions made during the session
- FR23: The daemon can create a PR for blocked/failed stories with partial code and a description of the failure
- FR24: When code review is disabled, the daemon proceeds directly to PR creation after the development session

**Notifications**
- FR25: The daemon can send Telegram notifications with run summaries (stories completed, blocked, errored)
- FR26: Notifications include story ID, status, and a link to the PR

**CLI & Configuration**
- FR27: The user can run `bmad-bot init` to interactively generate a project configuration file
- FR28: The user can run `bmad-bot start` to launch the daemon
- FR29: The user can run `bmad-bot status` to view current daemon state
- FR30: The user can run `bmad-bot logs` to view structured daemon logs
- FR31: The daemon can load configuration from a YAML file with secrets separated in a gitignored file
- FR32: The daemon can auto-discover BMAD version and installed modules from the project repo
- FR39: The user can authenticate with GitHub Copilot via OAuth Device Flow during `bmad-bot init` to automatically obtain an LLM access token, and the daemon can transparently exchange it for short-lived Copilot session tokens at runtime

**Error Handling & Resilience**
- FR33: The daemon can handle LLM provider rate limits with retry and exponential backoff
- FR34: The daemon can handle graceful shutdown on SIGTERM/SIGINT (complete current step, commit partial work, notify)
- FR35: The daemon can notify the human of any blocking error (session crash, git failure, LLM provider down)
- FR36: The daemon can validate configuration at startup and report missing or invalid settings
- FR37: The daemon can detect an interrupted session at startup (presence of WAL file) and resume the session by reloading chat history and reconstructing the agent
- FR38: The daemon can detect a context window limit error during a session, summarize the history via a separate LLM call, and bootstrap a fresh session with compressed context to continue the work

### NonFunctional Requirements

**Security**
- NFR-SEC1: API keys and tokens never stored in committed config — secrets loaded from gitignored `.env` or secrets file
- NFR-SEC2: Secrets never logged by `tracing` — structured logging filters sensitive fields
- NFR-SEC3: Git credentials from environment, never hardcoded

**Integration**
- NFR-INT1: LLM provider connection failures and unexpected responses handled without crashing
- NFR-INT2: GitHub API rate limiting (5000 req/hour authenticated) handled with retry
- NFR-INT3: Telegram API failures do not block the pipeline — logged but do not stop story processing

**Reliability**
- NFR-REL1: Transient LLM errors (timeouts, 500s, rate limits) recovered with exponential backoff, max 3 retries per call
- NFR-REL2: No work lost on unexpected shutdown — SIGTERM triggers graceful completion, commit, notification
- NFR-REL3: Crash recovery produces clean state — no corrupted branches, no half-committed files. Watcher re-reads `sprint-status.yaml` and resumes
- NFR-REL4: All errors logged via `tracing::error!()` with full context (story_id, step, error details)

**Scalability (Future — v2/v3)**
- NFR-SCA1: MVP: single daemon per project, sequential execution. No scaling requirements.
- NFR-SCA2: Future: master daemon orchestrating workers, story parallelization, Kubernetes deployment. MVP architecture decisions must not preclude this evolution.

### Additional Requirements

**From Architecture — Starter/Foundation:**
- Project initialized via `cargo init bmad-bot` + curated dependencies (no template framework)
- Rust edition 2024, single binary target, full async tokio runtime (`features = ["full"]`)
- Single crate with modular directory structure (not a Cargo workspace for MVP)
- CLI framework: clap with derive API (init, start, status, logs subcommands)
- Config loading: serde + serde_yaml for YAML, dotenvy for `.env` secrets
- Signal handling: tokio::signal for SIGTERM/SIGINT graceful shutdown

**From Architecture — Core Decisions:**
- Decision 1 — Supervisor Interception: Hybrid Chat Loop + `ask_supervisor` rig Tool. Chat loop handles workflow-level interaction; supervisor tool handles technical/business questions. Rule engine → LLM fallback → human escalation.
- Decision 2 — Sprint-Status Mutation: Daemon is pure reader. All mutations performed by the BMAD agent via tools.
- Decision 3 — Session State Persistence: WAL file (`_bmad-output/implementation-artifacts/.bmad-bot-session.yaml`) persisted after each chat turn. Crash recovery reloads history. Context limit recovery summarizes history and bootstraps fresh session.
- Decision 4 — Error Propagation: Three-tier layered. Layer 1 (HTTP transport): reqwest-middleware auto-retry. Layer 2 (Tools): domain-specific handling + bubble-up. Layer 3 (Session/Daemon): commit partial work, create PR with failure, notify.
- Decision 5 — Agent Prompt Composition: Load BMAD dev agent file as-is as rig preamble. Append language override. First message: `"DS"`.
- Decision 6 — Deployment Model: Foreground process via `bmad-bot start`. No self-daemonization. Logs to stdout/stderr.

**From Architecture — Implementation Patterns (mandatory for all stories):**
- Error Type Pattern: Per-module `thiserror` enums. `anyhow` only in `main.rs` / CLI layer.
- Rig Tool Pattern: Standard structure (serializable struct + dedicated args struct + dedicated error enum + Tool trait impl).
- Tracing Pattern: Structured spans with `story_id` context. Never `println!`/`eprintln!`.
- Config Pattern: Validate once at startup, share via `Arc<BotConfig>`. Secrets loaded separately from `.env`.
- Git Provider Trait Pattern: Params and returns as dedicated structs. Async trait methods.
- Test Mock Pattern: Deterministic LLM responses. Arrange-Act-Assert. Naming: `test_{module}_{behavior}_{scenario}`.

**From Architecture — External Integration Points:**
- LLM Providers (Anthropic, OpenAI, GitHub Models) via rig-core — API key from `.env`
- GitHub API via octocrab — Token from `.env`
- GitLab API via reqwest — Token from `.env`
- Telegram API via reqwest — Bot token from `.env`
- Local git repo via git2 (libgit2) — SSH key or credential helper
- Local filesystem via std::fs / tokio::fs
- Local terminal via tokio::process

**From Architecture — Anti-Patterns (NEVER do these):**
- No `unwrap()` or `expect()` in production code
- No `anyhow::Result` in library modules
- No loose primitives as function params when 3+ params exist
- No logging of sensitive data (API keys, tokens, passwords)
- No real LLM API calls in unit tests
- No skipping doc comments on public items

### FR Coverage Map

- FR1: Epic 2 — Detect ready-for-dev stories by polling sprint-status.yaml
- FR2: Epic 2 — Resolve story dependencies and execution order
- FR3: Epic 2 — Skip stories with unmet dependencies
- FR4: Epic 2 — Cascade blocked status to dependent stories
- FR5: Epic 4 — Review previously completed stories before starting new one
- FR6: Epic 4 — Update current story specs based on prior implementations
- FR7: Epic 4 — Create and checkout git branch (story/{epic}-{story})
- FR8: Epic 4 — Instantiate rig agent session with BMAD dev agent persona
- FR9: Epic 4 — Expose git, filesystem, terminal tools via rig tool calling
- FR10: Epic 4 — Execute full BMAD dev-story workflow autonomously
- FR11: Epic 4 — Inject session language override (English) via system prompt
- FR12: Epic 3 — Intercept agent questions during development session
- FR13: Epic 3 — Answer predictable questions via deterministic rule engine
- FR14: Epic 3 — Answer substantive questions via LLM fallback with project docs context
- FR15: Epic 3 — Escalate to human when neither rules nor LLM can answer confidently
- FR16: Epic 3 — Log every decision with question, answer, reasoning, and alternatives
- FR17: Epic 3 — Commit decisions file at implementation-artifacts path
- FR18: Epic 5 — Optionally launch code review using separate LLM (configurable)
- FR19: Epic 5 — Review agent commits fixes in separate commit
- FR20: Epic 5 — Review agent posts review as PR comment
- FR21: Epic 5 — Create Pull Request on GitHub with agent-written description
- FR22: Epic 5 — PR description includes Supervisor Decisions section
- FR23: Epic 5 — Create PR for blocked/failed stories with partial code and failure description
- FR24: Epic 5 — Proceed directly to PR creation when code review is disabled
- FR25: Epic 6 — Send Telegram notifications with run summaries
- FR26: Epic 6 — Notifications include story ID, status, and PR link
- FR27: Epic 1 — Run bmad-bot init for interactive config generation
- FR28: Epic 1 — Run bmad-bot start to launch daemon
- FR29: Epic 1 — Run bmad-bot status to view daemon state
- FR30: Epic 1 — Run bmad-bot logs to view structured logs
- FR31: Epic 1 — Load config from YAML with secrets separated in gitignored file
- FR32: Epic 1 — Auto-discover BMAD version and installed modules
- FR33: Epic 6 — Handle LLM provider rate limits with retry and exponential backoff
- FR34: Epic 1 — Handle graceful shutdown on SIGTERM/SIGINT
- FR35: Epic 6 — Notify human of any blocking error
- FR36: Epic 1 — Validate configuration at startup and report issues
- FR37: Epic 6 — Detect interrupted session at startup (WAL file) and resume
- FR38: Epic 6 — Detect context window limit error and bootstrap fresh session with compressed context
- FR39: Epic 1 — Authenticate with GitHub Copilot via OAuth Device Flow and exchange tokens at runtime

## Epic List

### Epic 1: Project Foundation & CLI
The user can install, configure, launch, and monitor the BMAD Bot daemon. This epic delivers the complete CLI interface (init, start, status, logs), configuration loading with secrets separation, BMAD auto-discovery, config validation, graceful shutdown, smart git auto-detection during setup, and GitHub Copilot OAuth Device Flow authentication for zero-friction LLM provider onboarding. After this epic, the daemon runs, stops cleanly, and the user can observe its state.
**FRs covered:** FR27, FR28, FR29, FR30, FR31, FR32, FR34, FR36, FR39

### Epic 2: Story Watching & Dependency Management
The daemon automatically detects stories with ready-for-dev status by polling sprint-status.yaml, resolves dependency order, skips blocked stories, and cascades blocked status to dependents. After this epic, the daemon knows WHAT to work on and in what order.
**FRs covered:** FR1, FR2, FR3, FR4

### Epic 3: Intelligent Supervision
The supervisor can intercept agent questions and answer them via a deterministic rule engine or a dedicated BMAD Architect agent session (multi-turn chat with full project context loaded autonomously), escalate to human when unsure, and log every decision with reasoning and alternatives to a committed decisions file. After this epic, the ask_supervisor rig tool is built, tested, and ready to be registered with the agent.
**FRs covered:** FR12, FR13, FR14, FR15, FR16, FR17

### Epic 4: Autonomous Development Session
The daemon launches a rig agent session with the BMAD dev agent persona and registered tools (git, filesystem, terminal, ask_supervisor). The agent reviews prior stories, updates specs, creates a branch, and executes the full dev-story workflow autonomously with English language override. After this epic, stories are developed end-to-end by the agent.
**FRs covered:** FR5, FR6, FR7, FR8, FR9, FR10, FR11

### Epic 5: Code Review & Pull Request Delivery
The daemon optionally launches a code review via a separate LLM after the dev session, with fixes in separate commits and review posted as a PR comment. It creates a Pull Request on GitHub with an agent-written description including a Supervisor Decisions section. PRs are also created for blocked/failed stories with partial code and failure context. After this epic, the user wakes up to PRs ready for human review.
**FRs covered:** FR18, FR19, FR20, FR21, FR22, FR23, FR24

### Epic 6: Notifications & Error Resilience
The daemon sends Telegram notifications with story status, ID, and PR links. It handles LLM rate limits with retry/backoff, notifies the human of blocking errors, detects interrupted sessions via WAL file for crash recovery, and recovers from context window limit errors by summarizing history and bootstrapping a fresh session. After this epic, the user can trust the daemon to run overnight without supervision.
**FRs covered:** FR25, FR26, FR33, FR35, FR37, FR38

### Epic 7: Integration Tests
All 6 functional epics have been implemented and pass 573 unit tests. This epic introduces integration tests that validate the interactions between modules at their boundaries — ensuring the daemon works as a cohesive system, not just as isolated pieces. These tests are deterministic (no real LLM calls), run in CI, and use mocked external dependencies.

---

## Epic 1: Project Foundation & CLI

The user can install, configure, launch, and monitor the BMAD Bot daemon. This epic delivers the complete CLI interface (init, start, status, logs), configuration loading with secrets separation, BMAD auto-discovery, config validation, and graceful shutdown.

### Story 1.1: Project Scaffolding, Configuration & Validation

As a developer,
I want to initialize the BMAD Bot project with a complete module structure and robust configuration loading,
So that I have a solid foundation to build all daemon features on.

**Acceptance Criteria:**

**Given** the project does not yet exist
**When** I run `cargo init bmad-bot` and set up the project
**Then** a Rust project is created with edition 2024, single binary target, and all required dependencies in Cargo.toml (tokio, rig-core, git2, serde, serde_yaml, dotenvy, clap, thiserror, tracing, tracing-subscriber, octocrab, reqwest, reqwest-middleware, reqwest-retry, async-trait)
**And** the complete module directory structure is scaffolded with stub mod.rs files for all modules (cli, config, watcher, session, supervisor, review, tools, git_provider, notifier)

**Given** a valid `bmad-bot.yaml` configuration file exists in the project root
**When** the config module loads the file
**Then** a `BotConfig` struct is deserialized via serde_yaml containing all configuration fields (polling_interval_secs, git_provider, llm providers/models, notification config, BMAD paths)
**And** secrets are loaded separately from `.env` via dotenvy and never stored in `BotConfig`

**Given** the project HTTP client is initialized
**When** any module needs to make external HTTP calls (LLM providers, GitHub/GitLab API, Telegram API)
**Then** a shared `reqwest` client is configured with `reqwest-middleware` and `reqwest-retry` for automatic retry with exponential backoff (max 3 retries) on transient errors (429, 500, 503, timeouts)
**And** the retry client is available to all modules from project inception — no HTTP call in any epic runs without retry resilience

**Given** a `bmad-bot.yaml` with missing or invalid fields
**When** the config module validates the configuration
**Then** a descriptive `ConfigError` (thiserror enum) is returned specifying exactly which field failed and why
**And** `ConfigError` follows the per-module thiserror pattern (no anyhow in library modules)

**Given** the project is initialized
**When** I inspect the repository
**Then** `bmad-bot.yaml.example` and `.env.example` template files exist and are committed
**And** `.env` is listed in `.gitignore`

### Story 1.2: CLI Framework & Daemon Lifecycle

As a developer,
I want to launch the daemon with `bmad-bot start` and have it run with structured logging and clean shutdown,
So that I have a controllable long-running process as the foundation for all pipeline features.

**Acceptance Criteria:**

**Given** the project has the clap dependency configured
**When** I build the CLI module
**Then** clap with derive API defines four subcommands: `init`, `start`, `status`, `logs`
**And** each subcommand has auto-generated `--help` documentation

**Given** a valid configuration file exists
**When** I run `bmad-bot start`
**Then** the daemon loads and validates the config, initializes structured tracing (JSON or pretty-print based on config) to stdout/stderr, and enters a polling loop (placeholder that sleeps for the configured interval)
**And** tracing is the only logging mechanism — no `println!` or `eprintln!` anywhere
**And** sensitive fields (API keys, tokens) are never present in log output

**Given** the daemon is running
**When** a SIGTERM or SIGINT signal is received
**Then** the daemon initiates graceful shutdown via tokio::signal, logs the shutdown event, and exits cleanly with code 0
**And** no partial state is left behind

### Story 1.3: Interactive Init Command

As a developer setting up BMAD Bot for the first time,
I want to run `bmad-bot init` and be guided through an interactive setup,
So that I can generate a valid configuration without manually writing YAML.

**Acceptance Criteria:**

**Given** I am in a project directory without existing BMAD Bot configuration
**When** I run `bmad-bot init`
**Then** interactive prompts ask for: repository path, LLM provider and model for each role (dev, review, supervisor), git provider (GitHub/GitLab), Telegram notification config, and polling interval

**Given** I have completed all interactive prompts
**When** the init command finishes
**Then** a `bmad-bot.yaml` file is generated with all user-provided settings (no secrets in this file)
**And** a `.env` file is generated with placeholder entries for all required secrets (API keys, tokens) with comments explaining each

**Given** a `bmad-bot.yaml` already exists in the directory
**When** I run `bmad-bot init`
**Then** the user is warned that existing config will be overwritten and asked to confirm before proceeding

### Story 1.4: Status, Logs & BMAD Discovery

As a developer operating BMAD Bot,
I want to check the daemon's state, review logs, and have BMAD auto-detected,
So that I can monitor operations and trust the daemon knows my project setup.

**Acceptance Criteria:**

**Given** the daemon is running or has run previously
**When** I run `bmad-bot status`
**Then** a summary is displayed showing: current state (running/stopped), stories processed count, stories in progress, stories blocked, and last activity timestamp

**Given** the daemon has been running with structured tracing
**When** I run `bmad-bot logs`
**Then** structured logs are displayed with story_id, timestamps, and action fields
**And** logs can be filtered by level (info, warn, error)

**Given** the daemon starts in a project with BMAD installed
**When** the config module initializes
**Then** the daemon auto-discovers the BMAD version and installed modules by scanning the project repo (e.g., `_bmad/` directory structure)
**And** the discovered information is logged at startup and available via `bmad-bot status`

### Story 1.5: Git Remote Auto-Detection in Init Command

As a developer setting up BMAD Bot for the first time,
I want the `bmad-bot init` command to auto-detect my git provider, repository owner, repository name, and default branch from the local `.git` configuration,
So that I can complete the setup faster with fewer manual inputs and zero risk of typos on repository information.

**Acceptance Criteria:**

**Given** I am in a directory with an initialized git repository that has an `origin` remote configured
**When** I run `bmad-bot init`
**Then** the git provider, repo owner, repo name, and target branch are auto-detected from the `origin` remote URL
**And** a summary of detected values is displayed for confirmation before proceeding

**Given** the auto-detected git settings are displayed
**When** I confirm them (default: Yes)
**Then** the init command skips the individual git provider/owner/repo/branch prompts and uses the detected values

**Given** the auto-detected git settings are displayed
**When** I decline them
**Then** the init command falls back to the standard manual prompts for git provider, repo owner, repo name, and target branch (existing Story 1.3 behavior)

**Given** I am in a directory without a `.git` directory or without any remote configured
**When** I run `bmad-bot init`
**Then** the init command silently skips auto-detection and falls back to manual prompts without any error message

**Given** the `origin` remote URL uses SSH format (`git@github.com:owner/repo.git`) or HTTPS format (`https://github.com/owner/repo.git`)
**When** auto-detection runs
**Then** the provider, owner, and repo name are correctly parsed from either format

**Given** the `origin` remote URL points to an unrecognized host (not `github.com` or `gitlab.com`)
**When** auto-detection runs
**Then** the owner and repo name are still pre-filled
**And** the git provider prompt falls back to manual selection with a note that the host was not recognized

**Given** the repository has multiple remotes but no `origin`
**When** auto-detection runs
**Then** the available remote names are listed and the user is prompted to select one

**Given** auto-detection successfully identifies git settings
**When** the final `bmad-bot.yaml` is generated
**Then** the generated config contains the correct git_provider, repo_owner, repo_name, and target_branch values identical to what was confirmed by the user

### Story 1.6: GitHub Copilot OAuth Device Flow Authentication

As a developer setting up BMAD Bot,
I want to authenticate with GitHub Copilot via an OAuth Device Flow when I choose `github-copilot` as my LLM provider,
So that I can get a token automatically without manually creating a Personal Access Token, and the daemon can transparently exchange it for short-lived Copilot session tokens at runtime.

**References:**
- OAuth Device Flow: [RFC 8628](https://datatracker.ietf.org/doc/html/rfc8628)
- Implementation reference: [openclaw github-copilot-auth.ts](https://github.com/openclaw/openclaw/blob/main/src/providers/github-copilot-auth.ts), [openclaw github-copilot-token.ts](https://github.com/openclaw/openclaw/blob/main/src/providers/github-copilot-token.ts)
- Covers: **FR39** (GitHub Copilot OAuth Device Flow authentication)

**Acceptance Criteria:**

---

**Part 1 — Rename `github-models` → `github-copilot` (do this first)**

**Given** all existing code references to `github-models` and `GITHUB_MODELS_API_KEY`
**When** this story is implemented
**Then** every occurrence of `github-models` is replaced with `github-copilot` across the following files:
- `src/cli/mod.rs` — `LLM_PROVIDERS`, `default_model_for_provider`, `generate_env_file`, and all tests referencing `github_models`
- `src/config/mod.rs` — `VALID_LLM_PROVIDERS`, `BotSecrets.github_models_api_key` → `BotSecrets.github_copilot_oauth_token`, `load()`, `validate_for_config`, and all tests
- `src/session/provider.rs` — `resolve_api_key` match arm and all tests
- `src/session/runner.rs` — `run()` match arm for `"github-models"`
- `src/supervisor/architect.rs` — `env_var_for_provider` match arm and tests
- `bmad-bot.yaml.example` — provider comments
- `README.md` — all references to `github-models` provider
- `_bmad-output/project-context.md` — multi-provider LLM config section and external integration points
**And** `GITHUB_MODELS_API_KEY` is replaced with `GITHUB_COPILOT_OAUTH_TOKEN` everywhere
**And** `default_model_for_provider("github-copilot")` returns `"gpt-4o"`
**And** all existing tests compile and pass with the renamed provider

---

**Part 2 — OAuth Device Flow in `bmad-bot init`**

**Given** the LLM provider list in `bmad-bot init`
**When** I view the available providers
**Then** the options are `anthropic`, `openai`, and `github-copilot`

**Given** I select `github-copilot` as an LLM provider for one or more roles during `bmad-bot init`
**When** all three LLM role selections (dev, review, supervisor) are complete
**Then** the GitHub Copilot OAuth Device Flow is triggered exactly once, regardless of how many roles use `github-copilot`
**And** a device code is requested from `https://github.com/login/device/code` with client ID `Iv1.b507a08c87ecfe98` and scope `read:user`
**And** the terminal displays the verification URL and user code for me to authorize in my browser
**And** the init flow polls `https://github.com/login/oauth/access_token` for the token with the interval specified by GitHub's response

**Given** no role uses `github-copilot` as its provider
**When** all three LLM role selections are complete
**Then** the Device Flow is not triggered at all

**Given** I authorize the device in my browser
**When** the polling receives a valid access token
**Then** the OAuth token is stored in memory and pre-filled as `GITHUB_COPILOT_OAUTH_TOKEN=<token>` in the generated `.env` file
**And** the init flow continues normally with remaining configuration steps (notifications, daemon settings)

**Given** the Device Flow is polling for authorization
**When** GitHub responds with `slow_down`
**Then** the polling interval is increased by 2 seconds as per the OAuth spec

**Given** the Device Flow is polling for authorization
**When** the device code expires (GitHub responds with `expired_token`)
**Then** an error message is displayed explaining the code expired
**And** `GITHUB_COPILOT_OAUTH_TOKEN=` is written empty in `.env` with a comment instructing the user to re-run init or obtain a token manually
**And** the init flow continues without aborting

**Given** the Device Flow is polling for authorization
**When** I cancel the authorization in the browser (GitHub responds with `access_denied`)
**Then** an error message is displayed explaining the authorization was denied
**And** `GITHUB_COPILOT_OAUTH_TOKEN=` is written empty in `.env`
**And** the init flow continues without aborting

**Given** the terminal is not interactive (no TTY)
**When** `github-copilot` is configured as a provider
**Then** the Device Flow is skipped with a warning message
**And** `GITHUB_COPILOT_OAUTH_TOKEN=` is written empty in `.env` with instructions to obtain the token manually

---

**Part 3 — Runtime Copilot Token Exchange and Caching**

**Given** the daemon starts with `bmad-bot start` and `github-copilot` is configured as a provider
**When** `BotSecrets` loads secrets from the environment
**Then** `GITHUB_COPILOT_OAUTH_TOKEN` is loaded
**And** validation fails with a descriptive error if the token is missing or empty

**Given** a session is about to run with the `github-copilot` provider
**When** the daemon needs an API token for the LLM client
**Then** it exchanges the long-lived OAuth token for a short-lived Copilot session token by calling `GET https://api.github.com/copilot_internal/v2/token` with `Authorization: Bearer {oauth_token}`
**And** the response `{ token: string, expires_at: number }` is parsed and cached in memory
**And** if the exchange fails (HTTP error, missing fields), the session fails with a descriptive `ProviderError`

**Given** a cached Copilot session token exists in memory
**When** a new session is about to start
**Then** the daemon checks whether the cached token is still valid (with a 5-minute safety margin before expiry)
**And** if valid, the cached token is reused without making a new exchange request
**And** if expired or within the safety margin, a fresh token is obtained via the exchange endpoint

**Given** a valid Copilot session token has been obtained
**When** the token contains a `proxy-ep=<host>` field (semicolon-delimited key-value pairs)
**Then** the base URL is derived by extracting the `proxy-ep` value, stripping the protocol, replacing `proxy.` prefix with `api.`, and prepending `https://`
**And** if no `proxy-ep` is found, the default base URL `https://api.individual.githubcopilot.com` is used

**Given** the Copilot session token and derived base URL are resolved
**When** the agent is built
**Then** it uses the OpenAI-compatible client with the dynamically derived base URL and the Copilot session token (not the OAuth token) as the API key

---

**Part 4 — Module Structure and New Files**

**Given** the new `src/auth/` module
**When** I inspect the project structure
**Then** the following files exist:
- `src/auth/mod.rs` — `pub mod github_copilot;`
- `src/auth/github_copilot.rs` — Device Flow functions (`request_device_code()`, `poll_for_access_token()`, `run_device_flow()`) and Copilot token exchange functions (`exchange_copilot_token()`, `derive_base_url_from_token()`, `CopilotTokenCache` struct with `resolve()` method)
- `src/main.rs` — `mod auth;` added

**Given** the auth module depends on HTTP calls
**When** the module is designed
**Then** HTTP calls are abstracted behind an `async` trait (e.g. `CopilotHttpClient`) to enable deterministic mocking in unit tests, consistent with the project's existing mock patterns (no external mock crate required)

---

**Part 5 — Unit Tests**

**Given** the `src/auth/github_copilot.rs` module
**When** I inspect the unit tests
**Then** the following tests exist with trait-based HTTP mocks (no real network calls):

*Device Flow tests:*
- `test_request_device_code_success` — mock HTTP 200 with valid JSON, verify parsed fields
- `test_request_device_code_http_error` — mock HTTP 500, verify error
- `test_request_device_code_missing_fields` — mock HTTP 200 with incomplete JSON, verify error
- `test_poll_authorization_pending_then_success` — mock sequential responses (`authorization_pending` × N, then `access_token`), verify final token
- `test_poll_slow_down_increases_interval` — verify interval increases by 2 seconds on `slow_down` response
- `test_poll_expired_token_returns_error` — mock `expired_token`, verify error
- `test_poll_access_denied_returns_error` — mock `access_denied`, verify error

*Token exchange tests:*
- `test_exchange_copilot_token_success` — mock valid exchange response, verify token and expiry parsed
- `test_exchange_copilot_token_http_error` — mock HTTP 401/403, verify error
- `test_exchange_copilot_token_missing_fields` — mock incomplete response, verify error
- `test_copilot_token_cache_returns_cached_when_valid` — verify no HTTP call when cache is fresh
- `test_copilot_token_cache_refreshes_when_expired` — verify HTTP call when cache is stale
- `test_derive_base_url_from_proxy_ep` — verify `proxy.example.com` → `https://api.example.com`
- `test_derive_base_url_fallback_when_no_proxy_ep` — verify default `https://api.individual.githubcopilot.com`
- `test_derive_base_url_strips_protocol_from_proxy_ep` — verify `https://proxy.foo.bar` → `https://api.foo.bar`

---

## Epic 2: Story Watching & Dependency Management

The daemon automatically detects stories with ready-for-dev status by polling sprint-status.yaml, resolves dependency order, skips blocked stories, and cascades blocked status to dependents. The daemon is a pure reader — it never writes to sprint-status.yaml.

### Story 2.1: Sprint-Status Polling & Story Detection

As a developer with stories marked ready-for-dev,
I want the daemon to automatically detect them by polling sprint-status.yaml,
So that stories are picked up for processing without manual intervention.

**Acceptance Criteria:**

**Given** a valid `sprint-status.yaml` exists at the configured output path
**When** the watcher module polls the file at the configured interval (default 5 min)
**Then** all stories with status `ready-for-dev` are identified and returned as `StoryInfo` structs (id, label, branch name, specs path, dependencies, status)
**And** the polling interval is configurable via `bmad-bot.yaml`

**Given** the `sprint-status.yaml` file does not exist or is malformed
**When** the watcher attempts to read it
**Then** a descriptive `WatcherError` (thiserror enum) is returned
**And** the error is logged via `tracing::error!()` with full context
**And** the daemon continues polling on the next cycle (does not crash)

**Given** no stories have `ready-for-dev` status
**When** the watcher polls
**Then** the watcher logs an info message and sleeps until the next polling cycle

### Story 2.2: Dependency Resolution & Execution Order

As a developer with interdependent stories,
I want the daemon to resolve dependencies and determine the correct execution order,
So that stories are processed in a sequence that respects their prerequisites.

**Acceptance Criteria:**

**Given** the watcher has detected multiple `ready-for-dev` stories
**When** the dependency resolution module (`deps.rs`) processes them
**Then** a directed acyclic graph of dependencies is computed in-memory
**And** stories are returned in topological order (prerequisites first)

**Given** a story has dependencies that are not yet in `done` status
**When** the pre-gate logic evaluates it
**Then** the story is skipped for this cycle (not marked, not modified — pure read)
**And** a tracing info message logs which story was skipped and which dependency is unmet

**Given** a story has all dependencies in `done` status
**When** the pre-gate logic evaluates it
**Then** the story is marked as eligible and included in the execution queue

**Given** a circular dependency exists in sprint-status.yaml
**When** the dependency graph is computed
**Then** a `WatcherError::CyclicDependency` error is returned with the cycle path
**And** the error is logged and the affected stories are skipped

### Story 2.3: Cascade Blocking

As a developer,
I want dependent stories to be automatically identified as blocked when a prerequisite story fails,
So that the daemon doesn't waste time attempting stories that cannot succeed.

**Acceptance Criteria:**

**Given** a story has been processed and resulted in a `blocked` or `needs-clarification` status
**When** the pre-gate logic runs on the next polling cycle
**Then** all stories that depend (directly or transitively) on the failed story are identified as ineligible
**And** a tracing warn message logs each cascade-blocked story with the reason (which prerequisite failed)

**Given** the blocking prerequisite story is later resolved (status changes to `done`)
**When** the next polling cycle runs
**Then** the previously cascade-blocked dependents are re-evaluated based on current statuses
**And** stories whose dependencies are now all `done` become eligible again

**Given** the daemon has identified cascade-blocked stories
**When** the pre-gate completes
**Then** only truly eligible stories (all dependencies met, status `ready-for-dev`) are passed to the session module
**And** the daemon never writes to `sprint-status.yaml` — all blocking logic is computed in-memory per cycle

---

## Epic 3: Intelligent Supervision

The supervisor can intercept agent questions and answer them via a deterministic rule engine or a dedicated BMAD Architect agent session (multi-turn chat with full project context loaded autonomously), escalate to human when unsure, and log every decision with reasoning and alternatives to a committed decisions file. After this epic, the ask_supervisor rig tool is built, tested, and ready to be registered with the agent.

### Story 3.1: Supervisor Tool Skeleton & Rule Engine

As a daemon operator,
I want agent questions to be automatically intercepted and answered by a deterministic rule engine,
So that predictable questions are resolved instantly without LLM cost.

**Acceptance Criteria:**

**Given** the supervisor module is initialized
**When** the `ask_supervisor` rig tool is built
**Then** it follows the standard rig Tool pattern (serializable struct + `AskSupervisorArgs` + `SupervisorError` thiserror enum + Tool trait impl)
**And** the tool NAME is `ask_supervisor` (snake_case)
**And** the tool definition description is detailed enough for the LLM agent to know when and how to call it

**Given** the agent calls `ask_supervisor` with a question matching a known pattern
**When** the rule engine in `rules.rs` evaluates the question
**Then** the rule engine matches against deterministic patterns: confirmations ("Should I proceed?"), step-by-step detection, story selection prompts, and other predictable BMAD workflow interactions
**And** the matched rule returns an answer immediately without any LLM call

**Given** the agent calls `ask_supervisor` with a question that does not match any rule
**When** the rule engine evaluates the question
**Then** the rule engine returns a `NoMatch` result indicating LLM fallback is needed
**And** the question and attempted match are logged via `tracing::info!()` with `action = "rule_engine_miss"`

**Given** the rule engine is deployed
**When** new patterns are identified from decision file analysis
**Then** rules can be added to `rules.rs` without modifying the tool interface or supervisor module structure

### Story 3.2: LLM Fallback with Project Context

As a daemon operator,
I want substantive agent questions to be answered by a dedicated BMAD Architect agent session with full project context,
So that the developer agent gets expert, context-aware architectural answers when the rule engine cannot help.

**Acceptance Criteria:**

**Given** the rule engine returns `NoMatch` for a question
**When** the supervisor LLM fallback is triggered
**Then** a fresh BMAD Architect agent session is created using the supervisor provider/model configured in `bmad-bot.yaml`
**And** the Architect agent file (`_bmad/bmm/agents/architect.md`) is loaded as the full preamble (activation steps, persona, menu, rules — the complete file)
**And** a minimal read-only `ReadFile` rig tool is registered so the Architect can load project files autonomously

**Given** a fresh Architect session is created
**When** the supervisor drives the multi-turn conversation
**Then** the following messages are sent in sequence via rig's `chat()` API: (1) `"CH"` to enter free chat mode, (2) `"Load the project context"` so the Architect loads relevant project docs via the ReadFile tool, (3) `"A developer agent working on this project has the following question: {question}"` with optional context
**And** the Architect's final response is returned to the dev agent as the tool output
**And** the session is discarded after each question (no persistence between supervisor calls)
**And** the fallback is logged via `tracing::warn!()` with `action = "supervisor_fallback"` and the question

**Given** the LLM provider is unavailable or the Architect session fails
**When** the supervisor attempts the fallback
**Then** the entire session is retried with exponential backoff (max 2 retries, 3 total attempts)
**And** if all retries fail, the supervisor proceeds to human escalation

### Story 3.3: Human Escalation

As a developer,
I want the supervisor to stop and escalate to me when it cannot answer a question confidently,
So that no incorrect decision is made autonomously.

**Acceptance Criteria:**

**Given** the rule engine returns `NoMatch` and the Architect session either fails or cannot answer confidently
**When** the supervisor determines it cannot answer
**Then** the `ask_supervisor` tool returns a `SupervisorError::EscalationRequired` error
**And** this error stops the rig agent loop, returning control to the daemon session module

**Given** the supervisor has escalated
**When** the session module receives the escalation error
**Then** the story status is set to `needs-clarification` (via the agent's last actions or session cleanup)
**And** the escalation event is logged via `tracing::warn!()` with `action = "supervisor_escalation"`, the question, and the reason for escalation

**Given** the supervisor escalates
**When** the session handles the escalation
**Then** partial work is preserved (commits, branch state) so the story can be resumed after human intervention

### Story 3.4: Decision Logging & Traceability

As a developer reviewing automated work,
I want every supervisor decision logged with full reasoning and alternatives,
So that I can audit, understand, and improve the supervisor's behavior over time.

**Acceptance Criteria:**

**Given** the supervisor answers a question (via rule engine or LLM fallback)
**When** the decision is made
**Then** a `DecisionRecord` is created in `decisions.rs` containing: question, chosen answer, source (rule_engine or llm_fallback), reasoning, and alternatives considered
**And** the record is appended to an in-memory decisions list for the current session

**Given** a development session completes or is interrupted
**When** the decision logging module finalizes
**Then** a decisions file is written to `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md` containing all decisions from the session in a human-readable markdown format
**And** the file is committed to the git branch

**Given** decisions have been logged during a session
**When** a PR is created (Epic 5)
**Then** the decisions list is available as structured data for inclusion in the PR description's "Supervisor Decisions" section
**And** each decision entry shows: question, decision, reasoning, and alternatives

---

## Epic 4: Autonomous Development Session

The daemon launches a rig agent session with the BMAD dev agent persona and registered tools (git, filesystem, terminal, ask_supervisor). The agent reviews prior stories, updates specs, creates a branch, and executes the full dev-story workflow autonomously with English language override. After this epic, stories are developed end-to-end by the agent.

### Story 4.1: Rig Tools Implementation (Git, Filesystem, Terminal)

As a daemon operator,
I want the agent to have access to git, filesystem, and terminal tools during development sessions,
So that the agent can perform all operations needed to develop a story autonomously.

**Acceptance Criteria:**

**Given** the tools module is initialized
**When** the git tool (`tools/git.rs`) is built
**Then** it follows the standard rig Tool pattern (serializable struct + `GitToolArgs` + `GitToolError` thiserror enum + Tool trait impl)
**And** it exposes git operations via git2: clone, checkout, branch create, add, commit, push, diff, status, log
**And** the tool NAME is `git` and the definition description is detailed enough for the LLM to use correctly
**And** every `call()` logs the action and result via `tracing` with story context

**Given** the tools module is initialized
**When** the filesystem tool (`tools/fs.rs`) is built
**Then** it follows the standard rig Tool pattern with `FsToolArgs` + `FsToolError`
**And** it exposes file operations via std::fs / tokio::fs: read file, write file, list directory, create directory, delete, check existence
**And** every `call()` logs the action and result via `tracing`

**Given** the tools module is initialized
**When** the terminal tool (`tools/terminal.rs`) is built
**Then** it follows the standard rig Tool pattern with `TerminalToolArgs` + `TerminalToolError`
**And** it exposes command execution via tokio::process: run command, capture stdout/stderr, return exit code
**And** every `call()` logs the command and result via `tracing`

**Given** any tool encounters an error
**When** the error is handled
**Then** it never panics — always returns `Result` with a descriptive error
**And** errors bubble up to the rig agent loop which decides how to proceed

### Story 4.2: Agent Session Setup & Chat Loop

As a daemon operator,
I want the daemon to launch an autonomous LLM agent session with the BMAD dev persona and all registered tools,
So that stories are developed without human intervention.

**Acceptance Criteria:**

**Given** the session module is initialized with a `StoryInfo` from the watcher
**When** an agent session is created
**Then** the BMAD dev agent file is loaded from the project's `_bmad/` directory and used as-is as the rig agent preamble
**And** a language override (`communication_language = English`) is appended to the preamble
**And** four tools are registered: git, filesystem, terminal, and ask_supervisor
**And** the agent is built using the dev LLM provider/model from `BotConfig`

**Given** an agent session is ready
**When** the chat loop starts
**Then** the first message sent is `"DS"` (triggers the dev-story workflow in the BMAD agent's menu system)
**And** the daemon manages the chat loop via `agent.chat(message, history)`, analyzing each agent response for workflow interaction points (confirmations, "should I proceed?", step transitions)
**And** the daemon responds automatically to workflow-level interactions

**Given** the agent is working in a chat loop
**When** the agent completes the dev-story workflow and signals completion
**Then** the session module detects completion and exits the chat loop
**And** the session result (success, blocked, or error) is returned to the daemon for downstream processing (review, PR, notification)

**Given** a session is active
**When** the entire session lifecycle runs
**Then** a `story_session` tracing span wraps the whole session with `story_id` and `branch` fields
**And** the daemon knows nothing about BMAD workflow internals — it only loads the agent file, registers tools, and manages the chat loop

### Story 4.3: Pre-Development Preparation & Branch Management

As a developer,
I want the agent to review prior implementations and refresh the current story's specs before coding,
So that each story is developed with up-to-date context and on a clean dedicated branch.

**Acceptance Criteria:**

**Given** a story has been selected for development and the agent session is active
**When** the agent begins the dev-story workflow
**Then** the agent uses the filesystem tool to read previously completed stories and their implementations
**And** the agent updates the current story's specs and acceptance criteria based on actual implementation of prior stories (if applicable)

**Given** the agent is ready to start coding
**When** the agent prepares the development environment
**Then** the agent uses the git tool to create and checkout a new branch following the `story/{epic}-{story}` naming convention (e.g., `story/1-2-account-management`)
**And** the branch is created from the configured base branch (e.g., `main`)

**Given** the branch already exists (e.g., from a previous interrupted session)
**When** the agent attempts to create it
**Then** the agent detects the existing branch, checks it out, and continues from the current state
**And** the situation is logged via `tracing::info!()` with `action = "branch_reuse"`

---

## Epic 5: Code Review & Pull Request Delivery

The daemon optionally launches a code review via a separate LLM after the dev session, with fixes in separate commits and review posted as a PR comment. It creates a Pull Request on GitHub or GitLab with an agent-written description including a Supervisor Decisions section. PRs are also created for blocked/failed stories with partial code and failure context. After this epic, the user wakes up to PRs ready for human review.

### Story 5.1: Git Provider Trait & GitHub PR Creation

As a developer using GitHub,
I want the daemon to create Pull Requests with comprehensive descriptions after each story session,
So that I wake up to reviewable PRs with full context on what was done and why.

**Acceptance Criteria:**

**Given** the git_provider module is initialized
**When** the `GitProvider` trait is defined
**Then** it exposes async methods: `create_pr(params: CreatePrParams) -> Result<PrInfo, GitProviderError>`, `add_comment(pr_id: &str, body: &str) -> Result<(), GitProviderError>`, `get_pr_url(pr_id: &str) -> Result<String, GitProviderError>`
**And** `CreatePrParams`, `PrInfo`, and `GitProviderError` are dedicated structs/enums following the git provider trait pattern
**And** the provider is selected via `bmad-bot.yaml` config (`git_provider: github | gitlab`)

**Given** a development session has completed successfully
**When** the daemon creates a PR via the GitHub implementation (octocrab)
**Then** the PR is created with: agent-written title and description body, source branch (`story/{epic}-{story}`), target branch (configured base branch)
**And** the PR description includes a dedicated "🤖 Supervisor Decisions" section listing all decisions from the session (question, decision, reasoning, alternatives)

**Given** a development session has been blocked or failed
**When** the daemon creates a PR for the failed story
**Then** a PR is still created with partial code committed to the branch
**And** the PR description includes a clear failure/blockage description explaining what happened, where it stopped, and all decisions made before the failure

**Given** code review is disabled in configuration
**When** the development session completes
**Then** the daemon proceeds directly to PR creation without launching a review session

### Story 5.2: Automated Code Review Session

As a developer,
I want an optional automated code review by a separate LLM before the PR is finalized,
So that code quality issues are caught and fixed before I review.

**Acceptance Criteria:**

**Given** code review is enabled in `bmad-bot.yaml` configuration
**When** a development session completes successfully (`SessionOutcome::Completed`)
**Then** the daemon launches a **new rig agent session** using the review LLM provider/model from `BotConfig.llm.review`
**And** the session loads the same BMAD dev agent persona (`dev.md`) and sends `"CR"` as the initial command
**And** the `ResponseAnalyzer` auto-responds to the story selection prompt with the story specs file path

**Given** the BMAD CR workflow asks how to handle findings (fix automatically / create action items / show details)
**When** the daemon's `ResponseAnalyzer` detects this decision prompt
**Then** it auto-responds with `"1"` (fix them automatically)
**And** the review agent applies fixes to the code (but does not commit yet)

**Given** the CR workflow completes (step 5 "Review Complete")
**When** the daemon detects the review completion output
**Then** the daemon sends a post-review message asking the agent to commit all review fixes with descriptive commit messages referencing the findings, and to provide a complete markdown review report
**And** the agent commits the fixes in separate commits (distinct from dev agent commits) with full context
**And** the agent's review report response is captured in `ReviewOutcome::Completed { report }`

**Given** a PR is created by the orchestrator after the review
**When** the `ReviewOutcome::Completed` contains a report
**Then** the orchestrator posts the review report as a comment on the PR via `GitProvider::add_comment()`
**And** the review comment includes: summary of findings, severity levels, fixes applied, and any remaining concerns

**Given** the review LLM provider is unavailable or errors out
**When** the review session fails
**Then** the daemon logs the error, returns `ReviewOutcome::Skipped` with a reason
**And** the orchestrator proceeds to PR creation without review
**And** the PR description notes that automated code review was skipped due to an error

**Given** code review is disabled in configuration (`code_review_enabled: false`)
**When** a development session completes
**Then** the daemon skips the review entirely and proceeds directly to PR creation

### Story 5.3: GitLab Merge Request Support

As a developer using GitLab,
I want the daemon to create Merge Requests with the same comprehensive descriptions as GitHub PRs,
So that I get the same experience regardless of my git provider.

**Acceptance Criteria:**

**Given** the `bmad-bot.yaml` config specifies `git_provider: gitlab`
**When** the GitLab implementation of `GitProvider` is initialized
**Then** it uses reqwest direct calls to the GitLab REST API (v4) with the token loaded from `.env`
**And** it implements all trait methods: `create_pr` (creates a Merge Request), `add_comment` (posts a note on the MR), `get_pr_url` (returns the MR web URL)

**Given** a development session has completed
**When** the daemon creates a Merge Request via the GitLab implementation
**Then** the MR is created with: agent-written title and description, source branch, target branch
**And** the MR description includes the "🤖 Supervisor Decisions" section, identical in format to the GitHub implementation

**Given** code review is enabled and completes
**When** the review is posted on GitLab
**Then** the review is posted as a note (comment) on the Merge Request via the GitLab notes API
**And** the format and content are consistent with the GitHub PR comment implementation

**Given** the GitLab API returns rate limit or transient errors
**When** the git_provider makes API calls
**Then** errors are handled by the reqwest-middleware retry layer (exponential backoff, max 3 retries)
**And** permanent failures return a descriptive `GitProviderError` with the HTTP status and response body

---

## Epic 6: Notifications & Error Resilience

The daemon sends Telegram notifications with story status, ID, and PR links. It handles LLM rate limits with retry/backoff, notifies the human of blocking errors, detects interrupted sessions via WAL file for crash recovery, and recovers from context window limit errors by summarizing history and bootstrapping a fresh session. After this epic, the user can trust the daemon to run overnight without supervision.

### Story 6.1: Telegram Notifications

As a developer running BMAD Bot overnight,
I want to receive Telegram notifications summarizing what happened,
So that I know the results without checking GitHub/GitLab manually.

**Acceptance Criteria:**

**Given** a development session completes successfully
**When** the notifier module sends a notification
**Then** a Telegram message is sent via reqwest direct HTTP call to the Telegram Bot API using the bot token from `.env`
**And** the message includes: story ID, status (✅ completed), and a direct link to the PR/MR

**Given** a story is blocked or encounters an error
**When** the notifier module sends a notification
**Then** a Telegram message is sent with: story ID, status (⚠️ blocked or ❌ error), reason for blockage/error, and a link to the PR if one was created
**And** the message provides enough context to understand the issue without opening the PR

**Given** a full daemon run completes (all eligible stories processed)
**When** the run summary is generated
**Then** a summary notification is sent with: total stories processed, count by status (completed, blocked, errored), and links to all PRs created

**Given** the Telegram API is unavailable or returns an error
**When** the notifier attempts to send a message
**Then** the failure is logged via `tracing::error!()` with full context
**And** the notification failure does NOT block the pipeline — story processing continues normally
**And** no retry is attempted for notification failures (non-critical path)

### Story 6.2: HTTP Retry & Error Resilience

As a daemon operator,
I want all external HTTP calls to be resilient to transient failures,
So that temporary provider outages don't derail overnight runs.

**Acceptance Criteria:**

**Given** all 3 retries are exhausted for an LLM provider call (retry middleware configured in Story 1.1)
**When** the final retry fails
**Then** the error bubbles up to the session/daemon layer (Layer 3 error propagation)
**And** the session commits partial work, creates a PR with failure description, and notifies the human

**Given** all 3 retries are exhausted for a GitHub/GitLab API call
**When** PR creation or comment posting fails permanently
**Then** the error is logged with full context (HTTP status, response body, story_id)
**And** the human is notified via Telegram with the failure details and the branch name so they can create the PR manually

**Given** a blocking error occurs at any point in the pipeline (session crash, git failure, all LLM providers down)
**When** the daemon's Layer 3 error handler catches it
**Then** a notification is sent to the human with: story ID, error type, error details, and recovery guidance
**And** the daemon moves on to the next eligible story (does not stop the entire run)

### Story 6.3: Crash Recovery via Session WAL

As a developer,
I want the daemon to recover from crashes by resuming interrupted sessions,
So that no work is lost if the process dies unexpectedly.

**Acceptance Criteria:**

**Given** a development session is active
**When** the chat loop completes a turn
**Then** the session state is persisted to a WAL file at `_bmad-output/implementation-artifacts/.bmad-bot-session.yaml`
**And** the WAL contains: story_id, branch name, started_at, last_activity, provider/model config, and complete chat_history (Vec<Message> serialized with role + content for each turn)

**Given** a session completes successfully (PR created)
**When** the session cleanup runs
**Then** the WAL file is deleted
**And** the daemon returns to the polling loop

**Given** the daemon starts up
**When** a WAL file exists from a previous interrupted session
**Then** the daemon detects the interrupted session and logs it via `tracing::warn!()` with `action = "crash_recovery"`
**And** the git state is verified (branch exists, dirty files confirm crash mid-session)
**And** the chat history is reloaded from the WAL file
**And** the agent is reconstructed with the same provider/model config from the WAL
**And** the chat loop resumes with the loaded history — the agent has full context and continues where it left off

**Given** the daemon starts up and no WAL file exists
**When** the initialization check completes
**Then** the daemon proceeds to normal polling (clean start)

### Story 6.4: Context Window Limit Recovery

As a developer,
I want the daemon to recover from context window limit errors without losing session progress,
So that long or complex stories can still be completed autonomously.

**Acceptance Criteria:**

**Given** the agent session is active and the chat history has grown large
**When** the LLM API returns a context limit error
**Then** the error is detected from the provider response in the chat loop
**And** the recovery process is initiated (not a crash — a controlled recovery)

**Given** a context limit error has been detected
**When** the recovery process starts
**Then** the full chat_history is read from the WAL file (already persisted after each turn)
**And** the last N exchanges are extracted verbatim from the WAL as immediate context
**And** a separate, fresh LLM call is made (new context, not the exhausted one) to summarize the full chat_history into a compact session summary

**Given** the summary has been generated
**When** the new session is bootstrapped
**Then** a fresh agent is constructed with the same provider/model config and the standard dev preamble + tools
**And** the daemon drives the BMAD activation flow as a simulated human: sends "CH" to enter chat mode, then sends "Load the project context" so the agent loads what it needs via its tools (same pattern as Story 3.2 Architect session)
**And** the daemon then sends a recovery message containing the session summary, last N verbatim exchanges, and instruction to continue on the current story
**And** the session enters direct chat mode (not re-entering the full dev-story workflow pipeline, since checkboxes and Dev Agent Record are already up to date on disk)

**Given** the new session is bootstrapped
**When** the chat loop resumes
**Then** the agent picks up the current task with full awareness of prior work
**And** the recovery event is logged via `tracing::info!()` with `action = "context_limit_recovery"`, original history length, and summary length
**And** the WAL file is updated with the new (compressed) session state

---

## Epic 7: Integration Tests

### Overview

All 6 functional epics have been implemented and pass 573 unit tests. This epic introduces **integration tests** that validate the interactions between modules at their boundaries — ensuring the daemon works as a cohesive system, not just as isolated pieces. These tests are deterministic (no real LLM calls), run in CI, and use mocked external dependencies.

**Why now:** Unit tests verify each module in isolation. Integration tests verify that the contracts between modules hold — that `watcher` feeds the right data to `pipeline`, that `pipeline` orchestrates `session → review → git_provider → notifier` correctly, that crash recovery actually reconstructs a valid session from a WAL file, and that the CLI lifecycle works end-to-end.

**Scope boundary:** This epic covers **deterministic integration tests only**. True E2E tests involving live LLM calls remain gated behind `BMAD_E2E=1` and are out of scope.

### Integration Test Strategy

#### What We're Testing

| Test Category | Modules Under Test | External Deps |
|---------------|-------------------|---------------|
| Config → Startup | `config/`, `cli/`, `config/discovery.rs` | Filesystem (temp dirs) |
| Watcher → Pipeline Dispatch | `watcher/`, `watcher/deps.rs`, `pipeline.rs` | Filesystem (temp YAML) |
| Pipeline Orchestration | `pipeline.rs`, `session/runner.rs`, `review/`, `git_provider/`, `notifier/` | All mocked via traits |
| Session WAL Recovery | `session/state.rs`, `session/runner.rs`, `pipeline.rs` | Filesystem (temp WAL) |
| Git Provider PR Flow | `git_provider/mod.rs`, `git_provider/github.rs`, `git_provider/gitlab.rs` | HTTP mocked |
| Notifier Flow | `notifier/mod.rs` | HTTP mocked |
| CLI Lifecycle | `cli/mod.rs`, `cli/state.rs` | Filesystem, process signals |
| Branch Management | `session/branch.rs`, `tools/git.rs` | Real git2 on temp repos |

#### Mock Strategy

All integration tests follow the architecture's **Test Mock Pattern**:
- LLM responses: static/deterministic — never call real providers
- GitHub/GitLab API: mock HTTP server (or trait mock returning canned responses)
- Telegram API: mock HTTP server (or NoopNotifier verification)
- Filesystem: `tempfile` crate for isolated temp directories
- Git repos: real `git2` operations on temp repos (fast, deterministic)

#### Test Infrastructure Needed

- **Mock `GitProvider` implementation** — returns canned `PrInfo` responses
- **Mock `Notifier` implementation** — captures sent notifications for assertion
- **Mock `SessionRunner` wrapper** — returns configurable `SessionOutcome` without LLM calls
- **Mock `ReviewRunner` wrapper** — returns configurable `ReviewOutcome` without LLM calls
- **Test fixture helpers** — functions to create valid `BotConfig`, `BotSecrets`, `StoryInfo`, sprint-status YAML, and WAL files

---

### Story 7.1: Integration Test Infrastructure & Fixtures

As a developer,
I want a shared test infrastructure with mock implementations and fixture builders,
So that all integration tests can be written concisely and consistently.

**Acceptance Criteria:**

**Given** a new `tests/integration/` directory is created
**When** I inspect the test helpers module
**Then** the following mock implementations exist:
- `MockGitProvider` implementing `GitProvider` trait — configurable to return `Ok(PrInfo { ... })` or `Err(GitProviderError::...)` for `create_pr`, `add_comment`, and `get_pr_url`
- `MockNotifier` implementing `Notifier` trait — captures all `notify_story` and `notify_run_summary` calls into a `Vec` for later assertion
- `MockSessionRunner` — wraps `SessionRunner` or provides a standalone struct that returns a configurable `SessionOutcome` (Completed / Escalated / Failed)
- `MockReviewRunner` — returns a configurable `ReviewOutcome` (Completed / Skipped / Failed)

**Given** the fixture module exists
**When** I call fixture builder functions
**Then** the following helpers are available:
- `make_test_config()` → valid `BotConfig` with sensible defaults (polling=60, provider=github, review=enabled)
- `make_test_secrets()` → valid `BotSecrets` with dummy tokens (never real keys)
- `make_test_story(key, label, deps)` → valid `StoryInfo` with specified key, label, branch, and dependency list
- `write_sprint_status(dir, stories)` → writes a valid `sprint-status.yaml` to a temp directory with given story entries and statuses
- `write_wal_file(dir, state)` → writes a valid `.bmad-bot-session.yaml` WAL file to a temp directory
- `create_test_repo(dir)` → initializes a git repo with an initial commit in a temp directory

**Given** the test infrastructure is built
**When** I run `cargo test --test integration`
**Then** all infrastructure tests compile and pass
**And** the mock implementations satisfy the trait bounds (`Send + Sync`)

**Technical Notes:**
- Place all test code under `tests/integration/` with a `mod.rs` entry point
- Use `#[cfg(test)]` and feature gate if needed, but prefer the `tests/` directory convention
- Mock implementations must be `Send + Sync` to satisfy async trait bounds
- All fixtures use `tempfile::tempdir()` for filesystem isolation

**Story Points:** 3

---

### Story 7.2: Config → Startup Validation Integration Tests

As a developer,
I want integration tests that verify the full config loading and validation pipeline,
So that I'm confident the daemon rejects bad configs and accepts good ones end-to-end.

**Acceptance Criteria:**

**Given** a temp directory with a valid `bmad-bot.yaml` and `.env` file
**When** the integration test loads config via `BotConfig::load()` then `BotConfig::validate()` then `BotSecrets::load()` then `BotSecrets::validate_for_config()`
**Then** the full pipeline succeeds and returns a valid `Arc<BotConfig>` and `Arc<BotSecrets>`

**Given** a temp directory with a `bmad-bot.yaml` missing a required field (e.g., `polling_interval_secs: 0`)
**When** the integration test runs the full load → validate pipeline
**Then** a descriptive `ConfigError` is returned at the validation step
**And** the error message identifies the exact field that failed

**Given** a temp directory with valid config but `.env` missing a required API key for the configured LLM provider
**When** the integration test runs load → validate → secrets-validate pipeline
**Then** a `ConfigError::MissingSecret` (or equivalent) is returned
**And** the error identifies which provider key is missing

**Given** a temp directory with a valid config
**When** `BmadDiscovery::discover()` is called on a directory with a `_bmad/` structure
**Then** the discovery detects BMAD, finds installed modules, and extracts the version
**And** calling it on a directory without `_bmad/` returns `bmad_detected: false`

**Given** a valid config is loaded
**When** `build_http_client()` is called
**Then** a `ClientWithMiddleware` is returned with retry middleware configured (3 retries, exponential backoff)

**Dependencies:** Story 7.1
**Story Points:** 2

---

### Story 7.3: Watcher → Dependency Resolution → Story Selection Integration Tests

As a developer,
I want integration tests that verify the full watcher → deps → eligible story selection chain,
So that I'm confident the daemon picks the right stories in the right order.

**Acceptance Criteria:**

**Given** a temp directory with a `sprint-status.yaml` containing 5 stories:
- Story 1-1: `done`
- Story 1-2: `ready-for-dev`, depends on 1-1
- Story 1-3: `ready-for-dev`, depends on 1-2
- Story 2-1: `ready-for-dev`, no deps
- Story 2-2: `backlog`
**When** the watcher polls and deps resolution runs
**Then** eligible stories returned are `[1-2, 2-1]` (1-1 is done, 1-3's dep not met, 2-2 not ready)
**And** stories are returned in dependency-valid order

**Given** a `sprint-status.yaml` where story 1-1 has status `blocked`
**When** cascade blocking runs for stories depending on 1-1
**Then** story 1-2 (depends on 1-1) is marked as cascade-blocked
**And** story 1-3 (transitive dependency through 1-2) is also cascade-blocked

**Given** a `sprint-status.yaml` where ALL stories are `done`
**When** the watcher polls
**Then** an empty eligible list (or `NoEligibleStories` error) is returned

**Given** a `sprint-status.yaml` with circular dependencies (1-1 depends on 1-2, 1-2 depends on 1-1)
**When** the dependency resolution runs
**Then** the system handles this gracefully (no infinite loop, both stories skipped or error reported)

**Given** a missing `sprint-status.yaml` file
**When** the watcher polls
**Then** a clear error is returned (not a panic)

**Dependencies:** Story 7.1
**Story Points:** 3

---

### Story 7.4: Pipeline Orchestration Integration Tests

As a developer,
I want integration tests that verify the full `StoryPipeline.process_story()` flow with mocked dependencies,
So that I'm confident the orchestration logic correctly chains session → review → PR → notification.

**Acceptance Criteria:**

**Given** a `StoryPipeline` constructed with:
- MockSessionRunner returning `SessionOutcome::Completed`
- MockReviewRunner returning `ReviewOutcome::Completed { report: "LGTM" }`
- MockGitProvider returning `Ok(PrInfo { id: "42", url: "https://...", number: 42 })`
- MockNotifier capturing notifications
**When** `process_story()` is called with a valid `StoryInfo`
**Then** the pipeline returns `PipelineResult` with `status: Completed` and `pr_url: Some("https://...")`
**And** MockNotifier captured exactly one story notification with the correct story key and PR link
**And** MockGitProvider received a `create_pr` call with a title matching `feat({story_key}): ...`
**And** MockGitProvider received an `add_comment` call with the review report as body

**Given** the same setup but MockSessionRunner returns `SessionOutcome::Failed { error: "LLM timeout" }`
**When** `process_story()` is called
**Then** the pipeline returns `PipelineResult` with `status: Failed` and `error_detail: Some("LLM timeout")`
**And** a PR is still created (partial work PR) with title containing `[NEEDS REVIEW]`
**And** MockNotifier captured a notification with failure status

**Given** the same setup but MockSessionRunner returns `SessionOutcome::Escalated { question: "..." }`
**When** `process_story()` is called
**Then** the pipeline returns `PipelineResult` with `status: Blocked`
**And** a PR is created with the escalation context in the description
**And** MockNotifier captured a notification with blocked/escalated status

**Given** a `StoryPipeline` with `code_review_enabled: false` in config
**When** `process_story()` is called and session succeeds
**Then** MockReviewRunner is NOT called (review skipped)
**And** PR is created without a review comment
**And** the pipeline result is still `Completed`

**Given** a `StoryPipeline` where MockGitProvider's `create_pr` returns an error
**When** `process_story()` is called and session succeeds
**Then** the pipeline returns `PipelineResult` with `pr_url: None` and an error detail about PR creation failure
**And** MockNotifier still receives a notification (notification is best-effort, never blocks)

**Dependencies:** Story 7.1
**Story Points:** 5

---

### Story 7.5: Session WAL Crash Recovery Integration Tests

As a developer,
I want integration tests that verify crash recovery from WAL files reconstructs a valid session,
So that I'm confident the daemon can survive crashes and resume work without data loss.

**Acceptance Criteria:**

**Given** a temp directory with a valid `.bmad-bot-session.yaml` WAL file containing:
- `story_key: "1-2-cli"`, `branch_name: "story/1-2-cli"`, `base_branch: "main"`
- Chat history with 4 messages (2 user, 2 assistant)
- `provider: "anthropic"`, `model: "claude-sonnet-4-20250514"`
**When** `SessionRunner::check_and_recover_wal()` is called
**Then** it returns `Some(RecoveryInfo)` with the correct story info and state
**And** `story_info_from_wal()` produces a `StoryInfo` with matching key, branch, and label

**Given** a recovered WAL state
**When** `SessionState::to_rig_messages()` is called
**Then** the returned `Vec<Message>` contains all 4 messages in the correct order with correct roles

**Given** a WAL file with corrupted/invalid YAML content
**When** `check_and_recover_wal()` is called
**Then** the WAL file is deleted (preventing infinite recovery loops)
**And** `None` is returned (clean start)

**Given** NO WAL file exists
**When** `check_and_recover_wal()` is called
**Then** `None` is returned immediately

**Given** a valid WAL file exists
**When** `recover_and_process()` runs with mocked session/review/git_provider/notifier
**Then** the full pipeline executes for the recovered story
**And** the WAL file is deleted after processing (regardless of success or failure)

**Given** a WAL file exists AND new eligible stories are found in sprint-status
**When** the daemon startup sequence runs
**Then** crash recovery is processed FIRST, before any new stories are polled

**Dependencies:** Story 7.1
**Story Points:** 3

---

### Story 7.6: Git Provider & PR Creation Integration Tests

As a developer,
I want integration tests that verify PR creation, commenting, and description building work correctly,
So that I'm confident the daemon produces well-formed PRs on both GitHub and GitLab.

**Acceptance Criteria:**

**Given** a `GitProviderConfig` with `provider: "github"`
**When** `create_provider()` is called with a valid token
**Then** a `Box<dyn GitProvider>` is returned containing a `GitHubProvider`

**Given** a `GitProviderConfig` with `provider: "gitlab"`
**When** `create_provider()` is called with a valid token
**Then** a `Box<dyn GitProvider>` is returned containing a `GitLabProvider`

**Given** a `GitProviderConfig` with `provider: "bitbucket"` (unsupported)
**When** `create_provider()` is called
**Then** a `GitProviderError::UnsupportedProvider` error is returned

**Given** a `GitLabProvider` constructed with an empty token
**When** `new()` is called
**Then** `GitProviderError::AuthenticationFailed` is returned

**Given** a successful story with supervisor decisions
**When** `build_pr_description()` is called with `PrDescriptionParams` including decisions text
**Then** the generated description contains:
- Story key and title in the header
- Outcome summary
- A "Supervisor Decisions" section with the decisions content
**And** `build_pr_title()` returns `feat({story_key}): {title}`

**Given** a failed story
**When** `build_pr_description()` is called with failure details
**Then** the description contains a "⚠️ Failure Details" section
**And** `build_pr_title()` returns `wip({story_key}): {title} [NEEDS REVIEW]`

**Dependencies:** Story 7.1
**Story Points:** 2

---

### Story 7.7: Notification Flow Integration Tests

As a developer,
I want integration tests that verify notification construction and delivery logic,
So that I'm confident the daemon sends correct, well-formatted notifications.

**Acceptance Criteria:**

**Given** a `TelegramNotifier` constructed with a valid config and bot token
**When** `notify_story()` is called with a `StoryNotification` (completed, with PR link)
**Then** the formatted message contains the story ID, "completed" status, and the PR URL

**Given** a `NotificationConfig` with `telegram.enabled: false`
**When** `create_notifier()` is called
**Then** a `NoopNotifier` is returned (not a `TelegramNotifier`)
**And** calling `notify_story()` on the noop notifier succeeds silently

**Given** a `NotificationConfig` with `telegram.enabled: true` but no bot token in secrets
**When** `create_notifier()` is called
**Then** a `NoopNotifier` is returned as graceful fallback
**And** a warning is logged (not an error — notifications are non-blocking)

**Given** a list of `PipelineResult` items (2 completed, 1 failed, 1 blocked)
**When** `build_run_summary()` constructs the `RunSummary`
**Then** the summary correctly counts: 4 total, 2 completed, 1 failed, 1 blocked
**And** `notify_run_summary()` on MockNotifier captures a message with all counts

**Dependencies:** Story 7.1
**Story Points:** 2

---

### Story 7.8: Branch Management & Git Tools Integration Tests

As a developer,
I want integration tests that verify branch creation, base branch resolution, and git tool operations on real (temp) repositories,
So that I'm confident the daemon manages git state correctly.

**Acceptance Criteria:**

**Given** a temp git repo with a `main` branch and an initial commit
**When** `ensure_story_branch("story/1-2-cli", "main")` is called
**Then** a new branch `story/1-2-cli` is created from `main`
**And** the repo HEAD is on `story/1-2-cli`

**Given** a temp git repo where branch `story/1-2-cli` already exists
**When** `ensure_story_branch("story/1-2-cli", "main")` is called again
**Then** the existing branch is checked out (not duplicated)
**And** no error is returned

**Given** a `StoryInfo` with dependencies `["1-1-scaffolding"]`
**And** a temp git repo with branches `main` and `story/1-1-scaffolding`
**When** `determine_base_branch()` is called
**Then** it returns `"story/1-1-scaffolding"` (last dependency's branch)

**Given** a `StoryInfo` with no dependencies
**When** `determine_base_branch()` is called
**Then** it returns the default branch (`"main"`)

**Given** a temp git repo with uncommitted changes
**When** `preserve_partial_work()` is called
**Then** all changes are staged and committed with a descriptive message containing the story key
**And** the commit exists in the repo's log

**Dependencies:** Story 7.1
**Story Points:** 3

---

### Story 7.9: CLI Lifecycle Integration Tests

As a developer,
I want integration tests that verify the CLI commands interact correctly with daemon state,
So that I'm confident the user experience of init → start → status → logs → stop is coherent.

**Acceptance Criteria:**

**Given** a temp directory with no daemon state file
**When** `DaemonState::read()` is called
**Then** `Ok(None)` is returned

**Given** a `DaemonState::new_running()` is created and written to a temp state file
**When** `DaemonState::read()` is called on that file
**Then** the state is deserialized correctly with matching PID, started_at, and status "running"

**Given** a running state is written
**When** `touch()` is called, then `record_story_processed()` twice, then state is re-written and re-read
**Then** `stories_processed == 2` and `last_activity` is updated

**Given** a running state is written
**When** `mark_stopped()` is called and state is re-written
**Then** re-reading shows `status: "stopped"`

**Given** a state file exists
**When** `cleanup()` is called
**Then** the file is removed
**And** subsequent `read()` returns `Ok(None)`

**Given** a valid `bmad-bot.yaml` is generated via the init flow helpers
**When** `BotConfig::load()` is called on the generated file
**Then** the config loads and validates successfully (round-trip test)

**Dependencies:** Story 7.1
**Story Points:** 2

---

### Story 7.10: Response Analyzer & Supervisor Rules Integration Tests

As a developer,
I want integration tests that verify the response analyzer and supervisor rule engine work correctly together,
So that I'm confident the chat loop handles all agent response patterns.

**Acceptance Criteria:**

**Given** an agent response containing a completion signal (e.g., "Implementation complete. All acceptance criteria met.")
**When** `ResponseAnalyzer::analyze()` processes it
**Then** it returns an action indicating session completion

**Given** an agent response asking which story to work on (e.g., "Which story should I implement?")
**When** `ResponseAnalyzer::analyze()` processes it
**Then** it returns a `Continue` action with the correct story key to reply with

**Given** an agent response asking for confirmation (e.g., "Should I proceed with the implementation?")
**When** the supervisor rule engine processes it
**Then** a deterministic "Yes, proceed." response is returned without LLM fallback

**Given** an agent response with a substantive question that doesn't match any rule
**When** the supervisor rule engine processes it
**Then** it falls through to LLM fallback (verified by checking that rules returned no match)

**Given** an agent response indicating step-by-step detection (e.g., "I'll work through this step by step...")
**When** `ResponseAnalyzer::analyze()` processes it
**Then** it returns a `Continue` action (agent should keep working)

**Dependencies:** Story 7.1
**Story Points:** 3

---

### Epic Summary

| Story | Title | Points | Dependencies |
|-------|-------|--------|--------------|
| 7.1 | Integration Test Infrastructure & Fixtures | 3 | — |
| 7.2 | Config → Startup Validation | 2 | 7.1 |
| 7.3 | Watcher → Deps → Story Selection | 3 | 7.1 |
| 7.4 | Pipeline Orchestration | 5 | 7.1 |
| 7.5 | Session WAL Crash Recovery | 3 | 7.1 |
| 7.6 | Git Provider & PR Creation | 2 | 7.1 |
| 7.7 | Notification Flow | 2 | 7.1 |
| 7.8 | Branch Management & Git Tools | 3 | 7.1 |
| 7.9 | CLI Lifecycle | 2 | 7.1 |
| 7.10 | Response Analyzer & Supervisor Rules | 3 | 7.1 |

**Total Story Points:** 28

**Execution Strategy:**
- Story 7.1 must be completed first (all others depend on the test infrastructure)
- Stories 7.2–7.10 can be parallelized (independent module boundaries)
- Recommended priority order: 7.4 (pipeline — highest risk) → 7.5 (crash recovery — critical path) → 7.3 (watcher — core loop) → 7.8 (git — data integrity) → 7.10 (analyzer — chat correctness) → 7.6, 7.7, 7.9, 7.2

**CI Integration:**
- All integration tests run via `cargo test --test integration` (no special env vars needed)
- Tests must complete in < 30 seconds total (no network calls, no LLM, only temp filesystem + git2)
- E2E tests (with real LLM) remain separate, gated behind `BMAD_E2E=1`