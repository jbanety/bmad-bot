# 🤖 BMAD Bot

**Autonomous AI Developer Daemon — powered by the BMAD methodology**

BMAD Bot is a Rust daemon that autonomously picks up user stories from a sprint backlog, implements them via LLM-powered coding sessions, runs tests, creates pull requests, and notifies you when it's done. It turns your sprint board into a self-driving development pipeline.

---

## Table of Contents

- [How It Works](#how-it-works)
- [Key Features](#key-features)
- [Architecture](#architecture)
- [Prerequisites](#prerequisites)
- [Quick Start](#quick-start)
  - [1. Build](#1-build)
  - [2. Initialize](#2-initialize)
  - [3. Configure Sprint Status](#3-configure-sprint-status)
  - [4. Start the Daemon](#4-start-the-daemon)
- [CLI Reference](#cli-reference)
- [Configuration](#configuration)
  - [bmad-bot.yaml](#bmad-botyaml)
  - [.env (Secrets)](#env-secrets)
- [Sprint Status Format](#sprint-status-format)
- [The Pipeline in Detail](#the-pipeline-in-detail)
  - [1. Story Detection](#1-story-detection)
  - [2. Dependency Resolution](#2-dependency-resolution)
  - [3. Pre-Development Preparation](#3-pre-development-preparation)
  - [4. Development Session](#4-development-session)
  - [5. Supervisor](#5-supervisor)
  - [6. Code Review](#6-code-review)
  - [7. Pull Request Creation](#7-pull-request-creation)
  - [8. Notifications](#8-notifications)
- [Resilience & Recovery](#resilience--recovery)
- [Project Structure](#project-structure)
- [Development](#development)
- [License](#license)

---

## How It Works

```text
┌─────────────────────────────────────────────────────────────────────┐
│                         BMAD Bot Daemon                             │
│                                                                     │
│  sprint-status.yaml ──► Watcher ──► Dependency Resolver             │
│                                          │                          │
│                                          ▼                          │
│                                    Story Pipeline                   │
│                                          │                          │
│                          ┌───────────────┼───────────────┐          │
│                          ▼               ▼               ▼          │
│                    Branch Setup    Dev Session     Code Review       │
│                          │          (LLM Agent)    (LLM Agent)      │
│                          │               │               │          │
│                          │               ▼               │          │
│                          │          Supervisor           │          │
│                          │        (Rules + LLM)          │          │
│                          │               │               │          │
│                          └───────────────┼───────────────┘          │
│                                          ▼                          │
│                                    PR Creation ──► Notifications    │
│                                  (GitHub/GitLab)    (Telegram)      │
└─────────────────────────────────────────────────────────────────────┘
```

1. The daemon polls `sprint-status.yaml` at a configurable interval
2. Stories marked `ready-for-dev` with satisfied dependencies are picked up
3. For each story: branch → LLM dev session → optional code review → PR → notify
4. The daemon never stops — a single story failure doesn't halt the run

---

## Key Features

- **Autonomous Story Implementation** — LLM agents execute the full BMAD `dev-story` workflow with git, filesystem, and terminal tools
- **Multi-Provider LLM Support** — Anthropic (Claude), OpenAI (GPT), and GitHub Copilot — configure different providers per role (dev, review, supervisor)
- **Intelligent Supervisor** — Three-tier question handling: deterministic rule engine → LLM fallback with project context → human escalation
- **Automated Code Review** — Optional adversarial review by a separate LLM session before PR creation
- **Dependency Resolution** — Topological sort with cascade blocking — stories are processed in the correct order
- **Crash Recovery** — Write-Ahead Log (WAL) ensures no work is lost on unexpected shutdown
- **Context Window Recovery** — Detects context limit errors, summarizes history, and bootstraps a fresh session to continue
- **Pull Request Automation** — Creates PRs on GitHub or GitLab with agent-written descriptions and supervisor decision logs
- **Telegram Notifications** — Run summaries with story status and PR links
- **Graceful Shutdown** — SIGTERM/SIGINT triggers clean completion of the current step
- **BMAD Auto-Discovery** — Detects BMAD version and installed modules from the project repository

---

## Architecture

BMAD Bot is built as a modular Rust daemon with clear separation of concerns:

| Module | Responsibility |
|--------|---------------|
| `cli` | CLI interface (init, start, status, logs), daemon lifecycle |
| `config` | YAML config loading, `.env` secrets, BMAD auto-discovery |
| `watcher` | Sprint status polling, story detection, dependency resolution |
| `pipeline` | Story lifecycle orchestration (session → review → PR → notify) |
| `session` | Rig agent construction, chat loop, WAL state, branch management |
| `supervisor` | Rule engine + LLM fallback + human escalation for agent questions |
| `review` | Adversarial code review via separate LLM session |
| `git_provider` | GitHub/GitLab PR creation via trait abstraction |
| `notifier` | Telegram notifications via trait abstraction |
| `tools` | Rig tools exposed to the agent: git, filesystem, terminal |

The daemon uses [rig](https://github.com/0xPlaygrounds/rig) as the LLM orchestration framework, with tool-calling agents that have access to git, filesystem, and terminal operations.

---

## Prerequisites

- **Rust** — Edition 2024 (install via [rustup](https://rustup.rs/))
- **Git** — For branch management and repository operations
- **LLM API Key** — At least one of:
  - [Anthropic API Key](https://console.anthropic.com/) (recommended: Claude Sonnet)
  - [OpenAI API Key](https://platform.openai.com/)
  - [GitHub Copilot](https://github.com/marketplace/models) access
- **Git Provider Token** — One of:
  - [GitHub Personal Access Token](https://github.com/settings/tokens) with `repo` scope
  - [GitLab Personal Access Token](https://gitlab.com/-/user_settings/personal_access_tokens) with `api` scope
- **BMAD Project** — A project with the [BMAD methodology](https://github.com/bmadcode/BMAD-METHOD) set up (`_bmad/` directory)
- **Sprint Status** — A `sprint-status.yaml` file with stories in `ready-for-dev` status

---

## Quick Start

### 1. Build

```sh
git clone <repo-url> && cd bmad-bot
cargo build --release
```

The binary is available at `./target/release/bmad-bot`.

### 2. Initialize

Run the interactive setup wizard to generate your configuration:

```sh
./target/release/bmad-bot init
```

This will walk you through creating:
- **`bmad-bot.yaml`** — Daemon configuration (polling interval, LLM providers/models, git provider, BMAD paths)
- **`.env`** — Secret credentials (API keys, tokens) — automatically gitignored

You'll be asked to choose:
- LLM provider and model for each role (dev, review, supervisor)
- Git provider (GitHub or GitLab) and repository details
- Whether to enable Telegram notifications
- Polling interval (how often to check for new stories)

### 3. Configure Sprint Status

Ensure your project has a `sprint-status.yaml` in the implementation artifacts folder. Stories you want the bot to implement should be set to `ready-for-dev`:

```yaml
generated: 2026-02-08
project: my-project
project_key: MYPROJ
tracking_system: file-system
story_location: "{project-root}/_bmad-output/implementation-artifacts"

development_status:
  epic-1: in-progress
  1-1-my-first-story: ready-for-dev
  1-2-my-second-story: backlog          # depends-on: 1-1
```

### 4. Start the Daemon

```sh
./target/release/bmad-bot start
```

The daemon will:
1. Load and validate configuration and secrets
2. Auto-discover your BMAD installation
3. Check for any interrupted sessions (crash recovery)
4. Begin polling `sprint-status.yaml` for eligible stories
5. Process stories autonomously: branch → implement → test → review → PR → notify

**Tip:** For faster iteration during initial setup, set `polling_interval_secs: 30` in your config.

Monitor the daemon with:

```sh
# Check daemon status
./target/release/bmad-bot status

# Tail logs with filtering
./target/release/bmad-bot logs --level info --tail 100
```

Stop the daemon gracefully with `Ctrl-C` (SIGINT) or `kill <pid>` (SIGTERM).

---

## CLI Reference

| Command | Description |
|---------|-------------|
| `bmad-bot init` | Interactive setup — generates `bmad-bot.yaml` and `.env` |
| `bmad-bot start` | Start the daemon — polls and processes stories |
| `bmad-bot status` | Show daemon state: uptime, stories processed, BMAD info |
| `bmad-bot logs` | Display structured logs with filtering |

### Global Options

| Flag | Description | Default |
|------|-------------|---------|
| `-c, --config <PATH>` | Path to configuration file | `bmad-bot.yaml` |

### Logs Options

| Flag | Description | Default |
|------|-------------|---------|
| `-l, --level <LEVEL>` | Minimum log level (trace, debug, info, warn, error) | `info` |
| `-t, --tail <N>` | Number of recent log lines to display | `50` |

---

## Configuration

### bmad-bot.yaml

```yaml
# Polling interval in seconds (how often to check for new stories)
polling_interval_secs: 300

# Logging
log_format: pretty            # "pretty" or "json"
log_level: info               # "trace", "debug", "info", "warn", "error"
log_file: bmad-bot.log

# Git provider for PR creation
git_provider:
  provider: github            # "github" or "gitlab"
  repo_owner: your-org
  repo_name: your-repo
  target_branch: main

# LLM providers — one provider+model per role
# Supported: "anthropic", "openai", "github-copilot"
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

# Notifications
notifications:
  telegram:
    enabled: false
    chat_id: ""

# Automated code review (separate LLM reviews code before PR creation)
code_review_enabled: true

# BMAD project paths
bmad_paths:
  project_root: "."
  output_folder: "_bmad-output"
  planning_artifacts: "_bmad-output/planning-artifacts"
  implementation_artifacts: "_bmad-output/implementation-artifacts"
```

### .env (Secrets)

Secrets are loaded from a `.env` file via [dotenvy](https://crates.io/crates/dotenvy). This file should be gitignored and **never committed**.

```env
# LLM API Keys (only the ones you use)
ANTHROPIC_API_KEY=sk-ant-...
OPENAI_API_KEY=sk-...

# Git Provider Token
GITHUB_TOKEN=ghp_...
# GITLAB_TOKEN=glpat-...

# Telegram (if enabled)
# TELEGRAM_BOT_TOKEN=123456:ABC-...
```

The `init` command generates both files with the correct variables for your chosen providers.

---

## Sprint Status Format

The `sprint-status.yaml` file is the daemon's input contract. It follows this structure:

```yaml
generated: 2026-02-08
project: my-project
project_key: MYPROJ
tracking_system: file-system
story_location: "{project-root}/_bmad-output/implementation-artifacts"

development_status:
  # Epic entries
  epic-1: in-progress           # backlog | in-progress | done

  # Story entries
  1-1-story-slug: ready-for-dev # backlog | ready-for-dev | in-progress | review | done | needs-clarification
  1-2-another-story: backlog    # depends-on: 1-1

  # Retrospectives (ignored by the bot)
  epic-1-retrospective: optional
```

### Story Status Lifecycle

```text
backlog → ready-for-dev → in-progress → review → done
                              │
                              └──► needs-clarification → ready-for-dev (after human input)
```

### Dependency Comments

Dependencies are declared as YAML comments on the story line:

```yaml
1-2-another-story: ready-for-dev  # depends-on: 1-1
```

The daemon's dependency resolver performs topological sorting and will only pick up stories whose dependencies have reached `done` status. If a story fails, its dependents are cascade-blocked.

---

## The Pipeline in Detail

### 1. Story Detection

The **Watcher** module reads `sprint-status.yaml` and identifies stories with `ready-for-dev` status. Epic entries, retrospectives, and non-story lines are filtered out.

### 2. Dependency Resolution

The **Dependency Resolver** performs topological sorting on eligible stories, resolving `depends-on` comments. Stories with unmet dependencies are deferred. Cyclic dependencies are detected and reported.

### 3. Pre-Development Preparation

For each eligible story, the daemon:
- Reviews previously completed stories and their implementations
- Creates and checks out a git branch (`story/{epic}-{story}`)
- Loads the story's implementation artifact (acceptance criteria, tasks, dev notes)

### 4. Development Session

A **rig agent** is constructed with:
- The BMAD dev agent persona (loaded from `_bmad/`)
- Four tools: `git`, `filesystem`, `terminal`, and `ask_supervisor`
- The story's implementation artifact as context

The agent receives the `"DS"` command (Dev Story) and autonomously executes the full BMAD development workflow: reading the story, implementing code, running tests, and committing changes.

### 5. Supervisor

When the agent encounters questions or decision points, it calls the `ask_supervisor` tool. The supervisor uses a three-tier resolution strategy:

1. **Rule Engine** (instant, free) — Pattern-matching for common questions: confirmations, step-by-step detection, story selection
2. **LLM Fallback** (context-aware) — A separate BMAD Architect session with full project documentation as context
3. **Human Escalation** — Stops the session, marks the story as `needs-clarification`, and notifies the human

Every supervisor decision is logged with the question, answer, reasoning, and source.

### 6. Code Review

When `code_review_enabled: true`, a **separate LLM session** runs the BMAD adversarial code review workflow (`"CR"` command). The review agent:
- Examines all changes made by the dev agent
- Can commit fixes in a separate commit
- Produces a review report for the PR

### 7. Pull Request Creation

The daemon creates a PR on GitHub (via [octocrab](https://crates.io/crates/octocrab)) or GitLab (via REST API) with:
- An agent-written PR title and description
- A "Supervisor Decisions" section listing all decisions made during the session
- For failed/escalated stories: partial code with a description of the failure

### 8. Notifications

When Telegram is enabled, the daemon sends:
- **Per-story notifications** — Story ID, status (completed/blocked/escalated), and PR link
- **Run summaries** — Total stories processed, completed, blocked, and errored

---

## Resilience & Recovery

### Crash Recovery (WAL)

Every active session writes a **Write-Ahead Log** (`.bmad-bot-session.yaml`) containing chat history and session state. On startup, the daemon checks for an interrupted WAL file and resumes the session by reloading history and reconstructing the agent.

### Context Window Recovery

If the LLM returns a context window limit error during a session, the daemon:
1. Summarizes the conversation history via a separate LLM call
2. Bootstraps a fresh session with the compressed context
3. Continues implementation from where it left off (up to 3 recovery attempts)

### Graceful Shutdown

`SIGTERM` and `SIGINT` (Ctrl-C) trigger graceful shutdown:
- The current polling cycle completes
- Partial work is committed
- The daemon state file is cleaned up

### Error Isolation

The "never stop the run" principle: no single story failure halts the daemon. Failed stories get a PR with partial code and error context. The daemon moves on to the next eligible story.

### HTTP Retry

All external HTTP calls (LLM providers, GitHub/GitLab API, Telegram) use exponential backoff with up to 3 retries for transient errors.

---

## Project Structure

```text
bmad-bot/
├── src/
│   ├── main.rs                       # CLI entry point (clap)
│   ├── pipeline.rs                   # Story pipeline orchestrator
│   ├── cli/                          # CLI commands: init, start, status, logs
│   │   ├── mod.rs
│   │   └── state.rs                  # Daemon state file management
│   ├── config/                       # Configuration loading & validation
│   │   ├── mod.rs                    # BotConfig, BotSecrets, HTTP client
│   │   └── discovery.rs             # BMAD auto-discovery
│   ├── watcher/                      # Sprint status polling
│   │   ├── mod.rs                    # Watcher, StoryInfo
│   │   └── deps.rs                  # Dependency resolution, topological sort
│   ├── session/                      # LLM agent session management
│   │   ├── mod.rs
│   │   ├── runner.rs                # SessionRunner — agent build + chat loop
│   │   ├── state.rs                 # WAL state persistence
│   │   ├── analyzer.rs             # Response analysis (completion detection)
│   │   ├── branch.rs               # Git branch management
│   │   ├── cleanup.rs              # Partial work preservation
│   │   ├── escalation.rs           # Human escalation types
│   │   └── provider.rs             # LLM provider factory
│   ├── supervisor/                   # Agent question handling
│   │   ├── mod.rs                    # AskSupervisor rig tool
│   │   ├── rules.rs                 # Deterministic rule engine
│   │   ├── architect.rs            # LLM fallback (Architect session)
│   │   ├── decisions.rs            # Decision logging & traceability
│   │   └── read_tool.rs            # Read-only file tool for supervisor
│   ├── review/                       # Automated code review
│   │   └── mod.rs                    # ReviewRunner
│   ├── git_provider/                 # PR creation abstraction
│   │   ├── mod.rs                    # GitProvider trait
│   │   ├── github.rs               # GitHub implementation (octocrab)
│   │   └── gitlab.rs               # GitLab implementation (REST API)
│   ├── notifier/                     # Notification abstraction
│   │   └── mod.rs                    # Notifier trait + Telegram + Noop
│   └── tools/                        # Rig tools for the agent
│       ├── mod.rs
│       ├── git.rs                   # Git operations (via git2)
│       ├── fs.rs                    # Filesystem read/write
│       └── terminal.rs             # Shell command execution
├── tests/
│   └── e2e/                          # E2E tests (gated behind BMAD_E2E=1)
│       └── mod.rs
├── _bmad/                            # BMAD methodology resources
├── _bmad-output/                     # Planning & implementation artifacts
│   ├── planning-artifacts/
│   │   └── epics.md
│   └── implementation-artifacts/
│       ├── sprint-status.yaml
│       └── *.md                     # Story implementation files
├── Cargo.toml
├── bmad-bot.yaml.example            # Configuration template
├── LICENSE                           # Apache 2.0
└── .gitignore
```

---

## Development

### Running Tests

```sh
# Run all unit tests (573 tests)
cargo test

# Run with output
cargo test -- --nocapture

# Run a specific module's tests
cargo test watcher::tests
cargo test supervisor::rules::tests

# Run E2E tests (requires API keys, costs tokens)
BMAD_E2E=1 cargo test --test e2e
```

### Building

```sh
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run clippy lints
cargo clippy -- -D warnings
```

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| [rig-core](https://crates.io/crates/rig-core) | LLM orchestration framework with tool-calling agents |
| [tokio](https://crates.io/crates/tokio) | Async runtime |
| [clap](https://crates.io/crates/clap) | CLI argument parsing |
| [git2](https://crates.io/crates/git2) | Native git operations (libgit2 bindings) |
| [octocrab](https://crates.io/crates/octocrab) | GitHub REST API client |
| [serde](https://crates.io/crates/serde) + [serde_yml](https://crates.io/crates/serde_yml) | YAML serialization/deserialization |
| [tracing](https://crates.io/crates/tracing) | Structured logging |
| [reqwest](https://crates.io/crates/reqwest) + [reqwest-retry](https://crates.io/crates/reqwest-retry) | HTTP client with retry middleware |
| [dotenvy](https://crates.io/crates/dotenvy) | `.env` file loading |
| [thiserror](https://crates.io/crates/thiserror) | Typed error enums |
| [dialoguer](https://crates.io/crates/dialoguer) | Interactive CLI prompts |

---

## License

Licensed under the [Apache License 2.0](LICENSE).
