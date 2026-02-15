---
stepsCompleted: ['step-01-init', 'step-02-discovery', 'step-03-success', 'step-04-journeys', 'step-05-domain', 'step-06-innovation', 'step-07-project-type', 'step-08-scoping', 'step-09-functional', 'step-10-nonfunctional', 'step-11-polish']
inputDocuments: ['_bmad-output/project-context.md']
workflowType: 'prd'
project_name: 'bmad-bot'
user_name: 'JB'
date: '2026-02-07'
documentCounts:
  briefs: 0
  research: 0
  projectDocs: 0
  projectContext: 1
classification:
  projectType: developer_tool
  domain: ai_developer_tooling
  complexity: medium
  projectContext: greenfield
---

# Product Requirements Document — BMAD Bot

**Author:** JB
**Date:** 2026-02-07

## Executive Summary

**BMAD Bot** is an autonomous Rust daemon that replaces the human developer in BMAD methodology workflows overnight. It watches a project's sprint backlog, picks up ready stories, launches a streaming LLM agent session (via the `rig` crate) with the BMAD dev agent persona activated via Zed-style XML context and exposed tools (git, filesystem, terminal, think), supervises the session with a hybrid rule engine + LLM fallback, runs an optional code review with a separate LLM, creates a Pull Request with full decision traceability, and notifies the developer.

**Vision:** Developers work with BMAD agents during the day to refine specs. At night, BMAD Bot executes the stories autonomously. In the morning, PRs are ready for human review and merge.

**Target Users:** Solo developers and small teams using the BMAD methodology who want to automate the development execution phase.

**Differentiator:** Unlike interactive coding assistants (Claude Code, Cursor, Aider) or generalist autonomous agents (Devin, SWE-Agent), BMAD Bot integrates with a structured methodology, provides full supervisor decision traceability (reasoning + alternatives in every PR), and operates in a nightly batch model with a PR-centric human review interface.

## Success Criteria

### User Success

- Developer wakes up to notifications summarizing completed work: stories developed, PRs ready for review
- Human code review is smooth — generated code is clean, understandable, tests pass, merges without friction
- Zero babysitting: the daemon runs autonomously overnight without intervention
- Story specs are always current — the agent updates specs based on previous implementations before starting development (pre-dev), and propagates implementation reality forward to downstream dependent stories after completing each story (post-impl)

### Business Success

- Open source internal tool — success measured by real daily time savings
- Reliability and trust: consistently produces mergeable code, reducing the review-fix-review cycle
- Future community adoption driven by a solid, proven product

### Technical Success

- End-to-end pipeline works reliably: story pickup → dependency check → spec update → dev session → code review → PR creation → notification
- Story dependencies respected — no story started before its prerequisites complete
- No silent failures — pipeline issues trigger immediate human notification with full context
- Supervisor answers agent questions correctly; when in doubt, stops and escalates
- Reliability over throughput: one story processed cleanly beats five done poorly

### Measurable Outcomes

- A single story can be picked up, developed, reviewed, and PR'd without human intervention
- Human merge rate on first review: target >80%
- Zero unnotified failures: every pipeline issue results in a notification
- Dependency violations: zero

## Product Scope

### MVP (Phase 1)

**Core pipeline:**
- Daemon polls `sprint-status.yaml` for `ready-for-dev` stories (configurable interval, default 5 min)
- Dependency resolution: respects execution order, skips blocked stories, cascades `blocked` status to dependents
- Pre-dev spec update: agent reviews completed stories and refreshes current story specs/AC based on actual implementation
- Post-impl impact analysis: after story completion, agent evaluates downstream dependent stories and updates their "Previous Story Intelligence" sections when actual implementation deviates from planned assumptions. Optionally updates architecture documentation. Best-effort — failures do not block story completion or PR creation
- Streaming rig agent session with BMAD dev agent persona activated via Zed-style XML context + tools (git, filesystem, terminal, think)
- Supervisor hybrid: deterministic rule engine → LLM fallback (project docs context) → human escalation (`needs-clarification` + notification)
- Supervisor decision logging: decisions file committed at `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md` + dedicated section in PR description with reasoning and alternatives
- Optional code review via separate LLM (configurable: enabled/disabled) — review fixes in separate commits, review posted as PR comment
- PR creation on GitHub with agent-written description
- Telegram notifications (success, blocked, error) with story ID, status, PR link
- Sequential execution: one story at a time
- Session language override via minimal system preamble (English for agent sessions, no repo file modification)
- LLM request/response logging for debugging and operations visibility

**CLI & Configuration:**
- `bmad-bot init` — interactive setup, generates `bmad-bot.yaml` + `.env`
- `bmad-bot start` — launches daemon
- `bmad-bot status` — current state summary
- `bmad-bot logs` — structured tracing logs
- YAML config (committed) with secrets separation (`.env`, gitignored)
- Auto-discovery of BMAD version and installed modules from the project repo
- Cooperative shutdown on SIGTERM/SIGINT via shared AtomicBool flag — can interrupt mid-streaming and mid-tool-call loops

**Distribution:**
- Build from source via `cargo install`. Targets Linux and macOS.
- **System prerequisite:** `git` must be installed on the host. The daemon validates git availability at startup and fails fast with a clear error if missing.

### Growth (Phase 2)

- GitLab support for MR creation
- Additional notification channels (email, webhook, WhatsApp)
- Improved error recovery and retry logic
- Automatic chaining of multiple stories per run
- Automatic detection of spec conflicts between dependent stories
- GitHub Releases with precompiled binaries (Linux x86_64/aarch64, macOS aarch64)

### Vision (Phase 3)

- Web dashboard for monitoring daemon runs and history
- Self-improving supervisor rules based on recurring question patterns
- Multi-project support: master daemon orchestrating workers, Kubernetes deployment
- Quality metrics: first-review merge rate, time per story, supervisor escalation rate
- Community plugin system for custom tools and notification providers
- BMAD "PR Rework" workflow: pick up a PR after human review, load context (decisions, review comments, story), continue the work
- Code integrity safeguards: static analysis, dependency scanning, sandboxed execution
- Windows support

## User Journeys

### Journey 1 — The Augmented Solo Dev (Happy Path)

**Persona:** JB — experienced developer using BMAD to structure projects. Works with BMAD agents during the day to create well-defined stories with precise acceptance criteria.

**Opening Scene:** End of the day. JB has refined 4 stories to `ready-for-dev` in the sprint. He closes his laptop. The BMAD Bot daemon is running.

**Rising Action:** At 10pm, the daemon detects ready stories. It picks the first, checks dependencies, reviews previously completed stories, and updates the current story's specs. It creates branch `story/1-2-account-management`, launches a rig session with Amelia. The supervisor answers routine questions — each decision logged with reasoning and alternatives. Amelia codes, tests, completes. The bot commits, then Amelia analyzes downstream stories — stories 1-3 and 1-4 depend on 1-2, so she reads their Dev Notes, compares what was planned against what she actually built, and updates their "Previous Story Intelligence" sections with the real module structure and API patterns. She commits those spec updates with a `docs(stories):` prefix. The bot creates a PR with a detailed description including a **"🤖 Supervisor Decisions"** section. A decisions file is committed at `_bmad-output/implementation-artifacts/1-2-account-management-DECISIONS.md`. The bot launches code review with an alternate LLM. The reviewer finds two issues — fixes committed separately. The reviewer posts its full review as a PR comment. On to the next story.

**Climax:** 7am. JB opens Telegram. Three notifications: "✅ story/1-2 — PR #12 ready", "✅ story/1-3 — PR #13 ready", "⚠️ story/1-4 — blocked, see PR #14".

**Resolution:** JB opens GitHub. First two PRs are clean — dev commits, then separate review fix commits. He checks Supervisor Decisions, validates choices, merges. For the third, the supervisor described exactly where it got stuck. He fixes the spec, unblocks the story. Net gain: a full night of automated development.

### Journey 2 — The Pipeline Derailment (Edge Case)

**Persona:** JB — same developer, things go wrong.

**Opening Scene:** The daemon processes story 2-1. Amelia asks a technical question about which pattern to use. The rule engine doesn't match. The supervisor LLM reads the architecture doc — but the doc doesn't cover this case.

**Rising Action:** The supervisor cannot answer confidently. It does not guess. It marks story 2-1 as `blocked`. Stories 2-2 and 2-3 that depend on it are automatically `blocked`. The supervisor commits partial code, creates a PR describing the situation and showing all decisions made before the blockage. Decisions file committed.

**Climax:** JB receives: "⚠️ story/2-1 blocked — supervisor escalation — PR #15 open".

**Resolution:** JB reads the PR, understands the doc gap, updates the architecture doc, unblocks the story. The daemon picks it up on the next cycle.

### Journey 3 — The New Open Source Contributor (Setup)

**Persona:** Alex — developer already using BMAD, discovers BMAD Bot on GitHub.

**Opening Scene:** README is clear: install, init, configure, start.

**Rising Action:** Alex runs `bmad-bot init`. Interactive prompts: repo path, LLM providers (dev, review, supervisor), Telegram token, Git provider (GitHub/GitLab). Config generated.

**Climax:** `bmad-bot start`. The daemon polls sprint-status.

**Resolution:** Next morning, first automatically generated PR. Alex reviews Supervisor Decisions, sees clean code, merges. Convinced.

### Journey 4 — Monitoring and Debugging (Operations)

**Persona:** JB — daemon running for 3 days, checking on things.

**Rising Action:** `bmad-bot status` — stories processed, in progress, blocked, last activity. `bmad-bot logs` — structured tracing with story_id, timestamps, actions. He checks the LLM request/response logs to debug a strange agent behavior. He notices the supervisor LLM is called too often for a pattern the rule engine should handle.

**Resolution:** JB adds a rule to the rule engine for that pattern. Reviews decisions files to spot trends. Next run is more efficient and cheaper.

### Journey Requirements Summary

| Journey | Capabilities Revealed |
|---|---|
| Happy Path | Watcher, rig session, supervisor with decision logging, optional code review with separate commits, PR with agent description + Supervisor Decisions section, review as PR comment, decisions file, Telegram notifications |
| Pipeline Fail | Supervisor escalation, cascade dependency blocking, PR with partial code and failure description, decisions file traceability |
| Setup | CLI `bmad-bot init` with prompts, YAML + `.env` generation, `bmad-bot start`, GitHub/GitLab selection |
| Operations | `bmad-bot status`, `bmad-bot logs`, structured tracing, LLM request/response logging, decision file pattern analysis |

## Decision Tracking

- **Decision file path:** `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md` — same directory as story files, following BMAD convention
- **PR description:** Dedicated "🤖 Supervisor Decisions" section: question, decision, reasoning, alternatives
- **Code review:** Posted as PR comment, fixes in separate commits for visibility
- **Traceability:** Decision files committed to repo, loadable by BMAD agents for iterative work

## Domain-Specific Requirements

### Configuration & Secrets Separation

- **Config file** (`bmad-bot.yaml`): committed — project settings, LLM provider/model selection, notification config, polling interval. No secrets.
- **Secrets file** (`.env` or `bmad-bot.secrets.yaml`): gitignored — API keys, bot tokens, credentials. Referenced by env var names in config.
- Separation allows teams to share configuration while keeping secrets local.

### Rate Limiting & API Resilience

- LLM provider APIs have rate limits and may return transient errors (429, 503). Retry with exponential backoff required.
- Token cost management is the user's responsibility — the daemon does not enforce budget limits.

### Code Integrity (Future — v2/v3)

- LLM-generated code carries inherent risks: malicious patterns, vulnerable dependencies, data exfiltration. Mitigated by LLM code review + mandatory human review before merge.
- Future: static analysis, dependency scanning, sandboxed execution.

## Developer Tool Specific Requirements

### Project-Type Overview

BMAD Bot is a Rust binary distributed as a standalone daemon. Not a library, SDK, or IDE plugin — an autonomous process consuming BMAD methodology artifacts and producing code via LLM agent sessions.

### CLI Command Surface

| Command | Description |
|---|---|
| `bmad-bot init` | Interactive setup: repo path, LLM providers, notifications. Generates `bmad-bot.yaml` and `.env` |
| `bmad-bot start` | Starts daemon. Polls `sprint-status.yaml`, processes stories until stopped |
| `bmad-bot status` | Current state: stories processed, in progress, blocked, last activity |
| `bmad-bot logs` | Structured `tracing` logs with story_id, timestamps, actions |

No CLI flags for config override in MVP — all configuration via YAML file.

### Implementation Considerations

- **Single binary:** No runtime dependencies beyond the OS. `git2` embeds libgit2, `rig-core` handles LLM connections. Self-contained.
- **Graceful shutdown:** SIGTERM/SIGINT handled — finishes current step if possible, commits partial work, notifies, exits.
- **Config validation:** `init` and `start` validate configuration before proceeding — missing keys, unreachable repos, invalid YAML all reported clearly.

### Documentation

- **MVP:** README with setup guide, configuration reference, usage examples. `--help` on every command.
- **Post-MVP:** Dedicated doc site (mdbook) with architecture overview, troubleshooting, contribution guide.

## Innovation & Novel Patterns

### Detected Innovation Areas

- **Autonomous agent in a structured methodology:** Not a copilot or assistant. A headless runner that plays the developer role within BMAD. Paradigm shift from interactive AI coding tools to methodology-driven autonomous execution.
- **Hybrid supervision with decision traceability:** Rule engine → LLM fallback → human escalation, combined with full decision logging (reasoning + alternatives) in PRs and committed decision files. Novel approach to transparent AI autonomy.
- **Spec-to-PR automated pipeline:** End-to-end from story pickup through dependency resolution, spec refresh, development, code review, and PR creation — with separate review commits and review comments for human visibility.

### Market Context

- **Claude Code / Cursor / Aider:** Interactive, human-in-the-loop. Not autonomous, not methodology-driven.
- **Devin / SWE-Agent:** Autonomous but generalist — no structured methodology, no decision traceability, no supervisor with escalation.
- **BMAD Bot:** Methodology integration (BMAD), decision traceability, nightly batch model, PR-centric review interface, hybrid supervision that knows when to stop.

### Validation Approach

- Dogfood with bmad-bot itself before generalizing
- Measure: first-review merge rate, supervisor escalation frequency, decision override rate
- Success = human trusts output enough to merge without rework in >80% of cases

## Functional Requirements

### Story Management

- **FR1:** The daemon can detect stories with `ready-for-dev` status by polling `sprint-status.yaml` at a configurable interval
- **FR2:** The daemon can resolve story dependencies and determine correct execution order
- **FR3:** The daemon can skip stories whose dependencies are not yet completed
- **FR4:** The daemon can mark dependent stories as `blocked` when a prerequisite story fails

### Pre-Development Preparation

- **FR5:** The agent can review previously completed stories and their implementation before starting a new story
- **FR6:** The agent can update the current story's specs and acceptance criteria based on actual implementation of prior stories
- **FR7:** The agent can create and checkout a git branch following the `story/{epic}-{story}` naming convention

### Development Session

- **FR8:** The daemon can instantiate a streaming rig agent session with the BMAD dev agent persona, activated via Zed-style XML context (agent file sent as first user message, not as system preamble)
- **FR9:** The daemon can expose surgical development tools to the agent via rig tool calling: `read_file` (partial reading & outline mode), `edit_file` (search-replace surgical editing), `grep` (regex codebase search), `find_path` (glob-based file discovery), `list_directory` (directory listing), `git` (version control operations), `terminal` (shell command execution), `ask_supervisor` (supervision escalation), and `think` (rig's built-in ThinkTool, derived from Anthropic's Claude Think Tool pattern, for structured reasoning without consuming real tool calls). Tools follow the Claude Code / Zed agent-mode pattern for optimal token efficiency and code safety
- **FR10:** The agent can execute the full BMAD `dev-story` workflow autonomously
- **FR11:** The daemon can inject a session language override (English) via a minimal system preamble without modifying repo files

### Post-Development Propagation

- **FR43:** The agent can analyze downstream dependent stories after completing a story by reading `sprint-status.yaml` to identify stories whose `depends-on` references the completed story, and update their "Previous Story Intelligence" sections when actual implementation deviates from planned assumptions. Optionally updates `architecture.md` if new modules or interfaces were introduced (checks existence first). Changes committed with `docs(stories):` prefix. Best-effort and non-blocking — failures do not block story completion or PR creation

### Supervision

- **FR12:** The supervisor can intercept agent questions during a development session
- **FR13:** The supervisor can answer predictable questions via a deterministic rule engine (confirmations, step-by-step detection, story selection)
- **FR14:** The supervisor can answer substantive questions via LLM fallback using full project documentation as context
- **FR15:** The supervisor can escalate to human when neither rules nor LLM can answer confidently
- **FR16:** The supervisor can log every decision with the question, chosen answer, reasoning, and alternatives considered
- **FR17:** The supervisor can commit a decisions file at `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md`

### Code Review

- **FR18:** The daemon can optionally launch a code review using a separate LLM after the development session (configurable: enabled/disabled)
- **FR19:** When enabled, the review agent can commit fixes in a separate commit (distinct from dev commits)
- **FR20:** When enabled, the review agent can post its review as a comment on the PR

### Pull Request Management

- **FR21:** The daemon can create a Pull Request on GitHub with an agent-written description
- **FR22:** The PR description includes a dedicated "Supervisor Decisions" section listing all decisions made during the session
- **FR23:** The daemon can create a PR for blocked/failed stories with partial code and a description of the failure
- **FR24:** When code review is disabled, the daemon proceeds directly to PR creation after the development session

### Notifications

- **FR25:** The daemon can send Telegram notifications with run summaries (stories completed, blocked, errored)
- **FR26:** Notifications include story ID, status, and a link to the PR

### CLI & Configuration

- **FR27:** The user can run `bmad-bot init` to interactively generate a project configuration file
- **FR28:** The user can run `bmad-bot start` to launch the daemon
- **FR29:** The user can run `bmad-bot status` to view current daemon state
- **FR30:** The user can run `bmad-bot logs` to view structured daemon logs
- **FR31:** The daemon can load configuration from a YAML file with secrets separated in a gitignored file
- **FR32:** The daemon can auto-discover BMAD version and installed modules from the project repo
- **FR39:** The user can authenticate with GitHub Copilot via OAuth Device Flow during `bmad-bot init` to automatically obtain an LLM access token, and the daemon can transparently exchange it for short-lived Copilot session tokens at runtime. The Copilot provider is a proxy to multiple backends — API format is hardcoded per model: known OpenAI model families (`gpt-*`, `o1-*`, `o3-*`, `codex`) use the Responses API, all other models fallback to the Completions API (safe default for non-OpenAI backends). Required IDE-specific headers are included in all Copilot requests
- **FR40:** The daemon logs all LLM requests and responses via a dedicated `llm_logging` module for debugging and operational visibility
- **FR42:** The daemon centralizes all LLM provider construction via an `AgentFactory` that returns a `BuiltAgent` with unified `stream_chat()` dispatch. API format selection is hardcoded per provider and model — not configurable. GitHub Copilot API format is determined by model name heuristic with Completions API as the safe fallback

### Error Handling & Resilience

- **FR33:** The daemon can handle LLM provider rate limits with retry and exponential backoff
- **FR34:** The daemon can handle cooperative shutdown on SIGTERM/SIGINT via a shared `ShutdownFlag` (Arc<AtomicBool>) propagated across pipeline → session → streaming chat layers. The flag can interrupt mid-streaming chunks and mid-tool-call loops, not just between steps. On shutdown: saves WAL state, commits partial work, notifies
- **FR35:** The daemon can notify the human of any blocking error (session crash, git failure, LLM provider down)
- **FR36:** The daemon can validate configuration at startup and report missing or invalid settings
- **FR41:** The daemon can validate git CLI availability at startup (by running `git --version`) and fail fast with a clear error message if git is missing

## Non-Functional Requirements

### Security

- API keys and tokens never stored in committed config — secrets loaded from gitignored `.env` or secrets file
- Secrets never logged by `tracing` — structured logging filters sensitive fields
- Git credentials inherited from user's git configuration (SSH agent, credential manager, osxkeychain) — daemon does not manage git auth directly

### Integration

- LLM provider connection failures and unexpected responses handled without crashing
- GitHub API rate limiting (5000 req/hour authenticated) handled with retry
- Telegram API failures do not block the pipeline — logged but do not stop story processing

### Reliability

- Transient LLM errors (timeouts, 500s, rate limits) recovered with exponential backoff, max 3 retries per call
- No work lost on unexpected shutdown — SIGTERM triggers graceful completion, commit, notification
- Crash recovery produces clean state — no corrupted branches, no half-committed files. Watcher re-reads `sprint-status.yaml` and resumes
- All errors logged via `tracing::error!()` with full context (story_id, step, error details)

### Scalability (Future — v2/v3)

- MVP: single daemon per project, sequential execution. No scaling requirements.
- Future: master daemon orchestrating workers, story parallelization, Kubernetes deployment. MVP architecture decisions must not preclude this evolution.

## Risk Mitigation

| Risk | Impact | Mitigation |
|---|---|---|
| **LLM quality variable** | PRs not mergeable, trust loss | Code review LLM + mandatory human review. Iterate on prompt/persona |
| **Supervisor decisions wrong** | Out-of-scope code, wasted runs | Full decision logging with alternatives. Human reviews and corrects. Rule engine refined iteratively |
| **Crate `rig` immature** | Technical blocker | Evaluate early. Fallback: direct LLM provider API calls |
| **Response parsing fragile** | Sessions derail silently | Start with simple patterns, enrich iteratively. Log unparsed responses |
| **Dogfooding bootstrap** | No real-world validation early | Build MVP manually with BMAD, dogfood as soon as pipeline works |