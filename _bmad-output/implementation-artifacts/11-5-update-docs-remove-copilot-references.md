# Story 11.5: Update Documentation — Remove Copilot References

Status: done

## Story

As a developer reading the documentation,
I want all references to GitHub Copilot removed and the OpenAI provider with `base_url` documented,
So that the docs accurately reflect the current two-provider model (Anthropic + OpenAI with optional `base_url`).

## Acceptance Criteria

1. **Given** `_bmad-output/project-context.md`
   **When** this story is implemented
   **Then** the "Multi-Provider LLM Config" section lists only `anthropic` and `openai`
   **And** all four LLM roles are documented: `dev`, `review`, `supervisor`, and optional `epic_review`
   **And** the `base_url` option is documented for both providers, with examples (Ollama, LM Studio, Groq)
   **And** all references to `github-copilot`, `CopilotTokenCache`, `copilot_requires_responses_api()`, `BuiltAgent::OpenAiCompletions`, IDE-specific headers, and Copilot streaming compat are removed
   **And** the HTTP Client line no longer mentions "GitHub Copilot adapter"
   **And** the `copilot-login` CLI command reference is removed
   **And** the `agent_factory.rs` code comment no longer mentions "Copilot API format detection"
   **And** the Technology Stack section says `serde_yml` (not `serde_yaml`) and `rig-core 0.35`
   **And** the "All crates: latest stable versions, no pinned versions" statement is removed or corrected

2. **Given** `bmad-bot.yaml.example`
   **When** this story is implemented
   **Then** the supported providers comment says `"anthropic", "openai"`
   **And** commented `base_url` examples are added inside the `dev:` role block
   **And** no `github-copilot` provider or `Copilot` reference appears anywhere
   **And** `reasoning_effort` comment references only `openai` (not Copilot)

3. **Given** `README.md`
   **When** this story is implemented
   **Then** the "Key Features" section lists Anthropic and OpenAI (with `base_url`) — no Copilot
   **And** the "Prerequisites" section removes the "GitHub Copilot access" bullet
   **And** the Configuration section's LLM comment shows `"anthropic", "openai"` as supported providers
   **And** `reasoning_effort` comment says "OpenAI only" (not "OpenAI/Copilot only")
   **And** the `base_url` option is shown as a commented example in the LLM config block
   **And** the Key Dependencies table removes the `git2` / libgit2 entry (git migrated to CLI subprocess in Story 4.4)

## Not in Scope

- Source code changes (all Copilot code already removed in 11.1–11.3)
- Cargo.toml changes (migration done in 11.4)
- Updating the planning artifacts (`epics.md`, `architecture.md`, `prd.md`) — historical records
- Adding new features or functionality
- `.env.example` — already clean (grep confirmed no Copilot references)
- `docs/mcp-servers.md` — already clean (grep confirmed no Copilot references)
- `README.md` Project Structure section — `src/auth/` already absent (removed alongside source code)

## Tasks / Subtasks

> ⚠️ **LINE NUMBER DRIFT WARNING:** Apply edits in file order (top to bottom). After each deletion or insertion, subsequent line numbers in the original file no longer match. Use the search strings in each task as the primary target anchor — not the line numbers (which are provided as orientation only).

- [x] **Task 1: Update `_bmad-output/project-context.md`** (AC: #1)

  - [x] 1.1 — Technology Stack (~L17–28): fix three stale lines:
    - `serde + serde_yaml` → `serde + serde_yml`
    - `rig-core (latest stable)` → `rig-core (0.35, crates.io)`
    - Delete the line `All crates: latest stable versions, no pinned versions` — this is no longer true (`rig-core`, `serde_yml`, `rmcp` are all pinned)

  - [x] 1.2 — HTTP Client line (~L24): change
    - FROM: `reqwest (Telegram API, GitHub Copilot adapter)`
    - TO: `reqwest (Telegram API, GitHub/GitLab API)`

  - [x] 1.3 — "Multi-Provider LLM Config" section (~L99–112): perform a complete rewrite. Search anchor: `#### Multi-Provider LLM Config — AgentFactory + BuiltAgent`. Replace the entire content of this subsection (up to the next `###` heading) with:

    ```
    #### Multi-Provider LLM Config — AgentFactory + BuiltAgent
    - Four LLM roles: **dev** (Amelia session), **review** (code review), **supervisor** (question answering), **epic_review** (autonomous post-epic retrospective — optional, defaults to `review` config when provider is empty)
    - Supported providers: `anthropic`, `openai`
    - **All provider construction is centralized in `AgentFactory`** (`src/llm/agent_factory.rs`). Since rig's `Chat` trait is not object-safe, `BuiltAgent` uses enum dispatch to wrap concrete agent types with a unified `stream_chat()` method.
    - **API format per provider:**
      - **Anthropic** → Messages API (always)
      - **OpenAI** → Responses API (always, rig default)
    - **Optional `base_url` per role:** Both providers accept an optional `base_url` field. For OpenAI, this enables any OpenAI-compatible endpoint (Ollama: `http://localhost:11434/v1`, LM Studio: `http://localhost:1234/v1`, Groq: `https://api.groq.com/openai/v1`, vLLM, etc.). For Anthropic, `base_url` can point to a compatible proxy. When absent, defaults to the provider's standard endpoint. Validated as a valid `http://` or `https://` URL at config load time.
    - **Optional `reasoning_effort` per role:** `LlmRoleConfig` supports an optional `reasoning_effort` field (`"low"`, `"medium"`, `"high"`, `"xhigh"`). When set, `AgentFactory` injects it as `additional_params({"reasoning": {"effort": "<value>"}})` on the rig `AgentBuilder`. Only effective for OpenAI providers. Ignored with a tracing warning for Anthropic. Validated at config load time.
    - API keys stored in environment variables, never in config files
    ```

  - [x] 1.4 — `agent_factory.rs` code comment in the directory tree (~L134): search for `Copilot API format detection`, change the comment to:
    - FROM: `agent_factory.rs # AgentFactory + BuiltAgent enum dispatch — centralized provider construction, Copilot API format detection`
    - TO: `agent_factory.rs # AgentFactory + BuiltAgent enum dispatch — centralized provider construction`

  - [x] 1.5 — CLI exception list (~L196): remove `copilot-login` from the `println!` exception note. Search for `copilot-login`. Change:
    - FROM: `cli/mod.rs` interactive commands (`init`, `status`, `copilot-login`, `logs`) may use `println!` directly
    - TO: `cli/mod.rs` interactive commands (`init`, `status`, `logs`) may use `println!` directly

  - [x] 1.6 — Update "Last Updated" note at the bottom: set to `2026-04-16` and describe the change (Copilot removed, two-provider model documented, `base_url` added, four LLM roles corrected).

- [x] **Task 2: Update `bmad-bot.yaml.example`** (AC: #2)

  - [x] 2.1 — Supported providers comment (search: `"anthropic", "openai", "github-copilot"`):
    - FROM: `# Supported providers: "anthropic", "openai", "github-copilot"`
    - TO: `# Supported providers: "anthropic", "openai"`

  - [x] 2.2 — `reasoning_effort` comment block (search: `Only effective for OpenAI and GitHub Copilot providers`):
    - FROM (lines ~33–34):
      ```
      #   Only effective for OpenAI and GitHub Copilot providers using the Responses API.
      #   Ignored for Anthropic and Copilot models routed through Chat Completions API.
      ```
    - TO:
      ```
      #   Only effective for OpenAI providers using the Responses API.
      #   Ignored for Anthropic.
      ```

  - [x] 2.3 — `reasoning_effort` inline comment on `dev:` block (search: `# uncomment for OpenAI/Copilot models`):
    - FROM: `# reasoning_effort: high  # uncomment for OpenAI/Copilot models`
    - TO: `# reasoning_effort: high  # uncomment for OpenAI models`

  - [x] 2.4 — Add `base_url` examples inside the `dev:` role block, as commented lines **after** the `model:` line and **before** `# reasoning_effort`. Placement (search anchor: `model: claude-sonnet-4-20250514` inside the `dev:` block):
    ```yaml
    dev:
      provider: anthropic
      model: claude-sonnet-4-20250514
      # base_url: "http://localhost:11434/v1"   # optional — Ollama
      # base_url: "http://localhost:1234/v1"    # optional — LM Studio
      # base_url: "https://api.groq.com/openai/v1"  # optional — Groq (use with provider: openai)
      # reasoning_effort: high  # uncomment for OpenAI models
    ```
    Note: only the `base_url` comment lines are new — do not duplicate or remove the `reasoning_effort` comment.

- [x] **Task 3: Update `README.md`** (AC: #3)

  - [x] 3.1 — Key Features (search: `Multi-Provider LLM Support`):
    - FROM: `- **Multi-Provider LLM Support** — Anthropic (Claude), OpenAI (GPT), and GitHub Copilot — configure different providers per role (dev, review, supervisor)`
    - TO: `- **Multi-Provider LLM Support** — Anthropic (Claude) and OpenAI-compatible (GPT, Ollama, LM Studio, Groq via optional `base_url`) — configure different providers per role (dev, review, supervisor)`

  - [x] 3.2 — Prerequisites LLM API Key (search: `[GitHub Copilot](https://github.com/marketplace/models) access`):
    - Delete this bullet point entirely. The remaining bullets (`Anthropic API Key`, `OpenAI API Key`) are correct.

  - [x] 3.3 — Configuration YAML comment (search: `# Supported: "anthropic", "openai", "github-copilot"`):
    - FROM: `# Supported: "anthropic", "openai", "github-copilot"`
    - TO: `# Supported: "anthropic", "openai"`

  - [x] 3.4 — `reasoning_effort` inline comment (search: `OpenAI/Copilot only`):
    - FROM: `reasoning_effort: high    # optional: "low", "medium", "high", "xhigh" (OpenAI/Copilot only)`
    - TO: `reasoning_effort: high    # optional: "low", "medium", "high", "xhigh" (OpenAI only)`

  - [x] 3.5 — Add `base_url` as a commented example in the configuration YAML block. Add it **inside the `supervisor:` role block** as a new commented line after the `model:` line. Do **NOT** change the `provider: anthropic` value — all three roles remain `anthropic` in the example. The comment shows the option exists for users who want to use OpenAI:
    ```yaml
    supervisor:
      provider: anthropic
      model: claude-sonnet-4-20250514
      # base_url: "http://localhost:11434/v1"  # optional: use any OpenAI-compatible endpoint
    ```
    Placement search anchor: `model: claude-sonnet-4-20250514` inside the `supervisor:` block (the third occurrence of this model string in the config example).

  - [x] 3.6 — Key Dependencies table (search: `git2`): remove the stale `git2` row entirely:
    - DELETE: `| [git2](https://crates.io/crates/git2) | Native git operations (libgit2 bindings) |`
    - This crate was removed in Story 4.4 when all git operations migrated to Git CLI subprocess. The `tools/git.rs` tool now uses `tokio::process::Command`.

  - [x] 3.7 — Verify the `.env (Secrets)` section — confirm it only shows `ANTHROPIC_API_KEY` and `OPENAI_API_KEY`. Current content is correct; no change required.

- [x] **Task 4: Final Verification** (AC: #1, #2, #3)
  - [ ] 4.1 Run from project root: `grep -rni "copilot" _bmad-output/project-context.md bmad-bot.yaml.example README.md` — must return **zero results**
  - [ ] 4.2 Run: `grep -rni "github-copilot" _bmad-output/project-context.md bmad-bot.yaml.example README.md` — must return **zero results**
  - [ ] 4.3 Run: `grep -rni "CopilotTokenCache" _bmad-output/project-context.md bmad-bot.yaml.example README.md` — must return **zero results**
  - [ ] 4.4 Run: `grep -rni "copilot-login" _bmad-output/project-context.md bmad-bot.yaml.example README.md` — must return **zero results**
  - [ ] 4.5 Run: `grep -rni "serde_yaml" _bmad-output/project-context.md` — must return **zero results** (crate is `serde_yml`)
  - [ ] 4.6 Run: `grep -rni "git2" README.md` — must return **zero results**
  - [ ] 4.7 Verify `base_url` appears in all three modified files: `grep -c "base_url" _bmad-output/project-context.md bmad-bot.yaml.example README.md` — each count must be > 0
  - [ ] 4.8 Verify only `"anthropic"` and `"openai"` appear as provider values in the three files: `grep -n "provider:" _bmad-output/project-context.md bmad-bot.yaml.example README.md` — no `github-copilot` in output
  - [ ] 4.9 Confirm docs/ is clean (pre-check, no action expected): `grep -rni "copilot" docs/` — must return **zero results**

## Dev Notes

### Epic 11 Context

Epic 11 is a linear chain: **11.1 → 11.2 → 11.3 → 11.4 → 11.5**. Stories 11.1–11.4 are all done. This is the final story — documentation-only polish. No source code changes required.

- Story 11.1 (done): Removed the entire `src/auth/` directory and `copilot-login` CLI subcommand (~1,950 lines deleted)
- Story 11.2 (done): Restructured `AgentFactory` to two variants: `Anthropic` + `OpenAiCompatible`, added `base_url` support to both providers
- Story 11.3 (done): Cleaned all remaining Copilot references from config, secrets, provider routing, CLI
- Story 11.4 (done): Migrated from rig fork (`jbanety/rig`) to official `rig-core` 0.35 from crates.io, rmcp 0.13 → 1.0

### ⚠️ CRITICAL: Provider Name Is `"openai"` — NOT `"openai-compatible"`

The epics file says `"openai-compatible"` but this was **reverted during 11.2 code review**. The actual provider name in the source code is `"openai"`. Verified in `src/config/mod.rs` L304: `VALID_LLM_PROVIDERS: &[&str] = &["anthropic", "openai"]`. All documentation MUST use `"openai"` as the provider name.

### Current Source of Truth — What the Code Actually Does

The following is the accurate state of the codebase after stories 11.1–11.4:

- **`LlmRoleConfig` fields:** `provider: String`, `model: String`, `reasoning_effort: Option<String>`, `base_url: Option<String>`
- **Valid providers:** `["anthropic", "openai"]` — validated in `src/config/mod.rs`
- **`LlmConfig` struct — four roles:** `dev`, `review`, `supervisor`, `epic_review` (optional, defaults to `review` config when provider is empty — see `AgentFactory::config_for_role`)
- **`BuiltAgent` enum:** Two variants — `Anthropic(Agent<anthropic::completion::CompletionModel>)` and `OpenAiCompatible(Agent<openai::responses_api::ResponsesCompletionModel>)`
- **`base_url`:** Supported on BOTH Anthropic and OpenAI builders. Validated as valid URL starting with `http://` or `https://`. Optional — when absent, defaults to the provider's standard endpoint.
- **CLI `init` flow:** Prompts for `base_url` for every role (both providers). Lists only `["anthropic", "openai"]` as provider choices.
- **`reasoning_effort`:** Only effective for OpenAI providers. Ignored for Anthropic.
- **No `copilot-login` subcommand** — removed in 11.1
- **No `CopilotTokenCache`** — removed in 11.1
- **No `src/auth/` directory** — removed in 11.1
- **No `BotSecrets.github_copilot_oauth_token`** — removed in 11.3
- **rig-core:** Official crate `0.35` from crates.io (not fork). rmcp `1.0`.
- **Serialization crate:** `serde_yml` (not `serde_yaml`) — `serde_yml = "0.0.12"` in Cargo.toml
- **git2 / libgit2:** NOT in Cargo.toml — all git operations use Git CLI subprocess since Story 4.4
- **`.env` generation:** Only generates `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` lines (no Copilot token)

### Exact Stale References to Fix (Pre-Located)

**`_bmad-output/project-context.md`** (7 locations):

| Line(s) | Current Content | Action |
|---------|----------------|--------|
| L21 | `rig-core (latest stable)` | Change to `rig-core (0.35, crates.io)` |
| L22 | `serde + serde_yaml` | Change to `serde + serde_yml` |
| L24 | `reqwest (Telegram API, GitHub Copilot adapter)` | Change to `reqwest (Telegram API, GitHub/GitLab API)` |
| L27 | `All crates: latest stable versions, no pinned versions` | Delete this line |
| L99–112 | Full "Multi-Provider LLM Config" subsection | Rewrite (see Task 1.3 for exact replacement text) |
| L134 | `Copilot API format detection` in agent_factory.rs comment | Remove "Copilot API format detection" from comment |
| L196 | `copilot-login` in CLI exception list | Remove `copilot-login,` from the list |

**`bmad-bot.yaml.example`** (3 locations + 1 addition):

| Line(s) | Current Content | Action |
|---------|----------------|--------|
| L28 | `"anthropic", "openai", "github-copilot"` | Change to `"anthropic", "openai"` |
| L33–34 | Copilot Responses/Completions API references | Simplify to "OpenAI only" (see Task 2.2) |
| L39 | `OpenAI/Copilot models` | Change to `OpenAI models` |
| After `dev.model:` | *(missing)* | Add `base_url` commented examples (see Task 2.4) |

**`README.md`** (5 locations + 1 addition):

| Line(s) | Current Content | Action |
|---------|----------------|--------|
| L84 | `Anthropic (Claude), OpenAI (GPT), and GitHub Copilot` | Remove Copilot, add `base_url` mention (see Task 3.1) |
| L126 | `[GitHub Copilot](https://github.com/marketplace/models) access` | Delete bullet |
| L257 | `"anthropic", "openai", "github-copilot"` | Change to `"anthropic", "openai"` |
| L265 | `OpenAI/Copilot only` | Change to `OpenAI only` |
| L668 | `git2` / libgit2 row | Delete entire table row |
| After `supervisor.model:` | *(missing)* | Add `base_url` commented example (see Task 3.5) |

### Anti-Patterns to Avoid

- **DO NOT use `"openai-compatible"` as the provider name** — the code uses `"openai"`. The epics file is outdated on this point.
- **DO NOT change any role's `provider:` from `anthropic` to `openai` in the README/yaml examples** — the default recommended provider remains Anthropic. The `base_url` feature is added as *commented* examples, not as a changed default.
- **DO NOT write `serde_yaml`** — the crate is `serde_yml`. These are different crates.
- **DO NOT update planning artifacts** (`epics.md`, `architecture.md`, `prd.md`) — historical records.
- **DO NOT change any source code** — this is a documentation-only story.
- **DO NOT mention the rig fork** — `jbanety/rig` is gone. Just reference `rig-core 0.35` from crates.io.

### Previous Story Intelligence (11.4)

Key learnings from Story 11.4:
- The `project-context.md` was explicitly flagged as stale — this story is the fix
- rig-core is now pinned at `0.35` from crates.io, rmcp at `1.0`
- The provider name is `"openai"` — confirmed in source code, NOT `"openai-compatible"`
- Codebase has 1131 passing tests, 1 pre-existing failure (unchanged)
- `git2` is NOT in Cargo.toml — confirmed by grep

### Git Intelligence

Recent commits (most recent first):
- `1941956` — feat(epic-11): migrate rig-core from git fork to crates.io 0.35, rmcp 0.13 → 1 (Story 11.4)
- `5746a62` — feat(epic-11): remove Copilot provider, add base_url init prompt (Story 11.3)
- `43c1a5a` — feat(epic-11): add base_url support to AgentFactory for both providers (Story 11.2)
- `07a3b0f` — feat(epic-11): remove GitHub Copilot auth module (Story 11.1)

### Project Structure Notes

- No new files created
- No files deleted
- Three files modified: `_bmad-output/project-context.md`, `bmad-bot.yaml.example`, `README.md`
- All changes are text edits — no structural changes
- `docs/mcp-servers.md` and `.env.example` confirmed clean (no Copilot references) — no action needed

### References

- [Source: _bmad-output/planning-artifacts/epics.md § Story 11.5 (L2903–2928)]
- [Source: _bmad-output/planning-artifacts/epics.md § Epic 11 Summary (L2928–2949)]
- [Source: _bmad-output/implementation-artifacts/11-4-migrate-rig-fork-to-official-crate.md § Dev Notes — "project-context.md is stale"]
- [Source: _bmad-output/project-context.md § Multi-Provider LLM Config (L99–112) — STALE, this story fixes it]
- [Source: src/config/mod.rs § VALID_LLM_PROVIDERS (L304) — source of truth for provider names]
- [Source: src/config/mod.rs § LlmConfig (L165–179) — source of truth: four roles including epic_review]
- [Source: src/config/mod.rs § LlmRoleConfig (L182–203) — source of truth for config fields including base_url]
- [Source: src/cli/mod.rs § LLM_PROVIDERS (L127) — confirms `["anthropic", "openai"]`]
- [Source: src/cli/mod.rs § generate_env_file (L763–771) — confirms only anthropic/openai keys generated]
- [Source: Cargo.toml § dependencies — confirms rig-core 0.35, serde_yml 0.0.12, no git2]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### Change Log

### File List


### Review Findings

- [x] [Review][Patch] **Fichier .bak commite** — project-context.md.bak est un backup manuel qui n a pas sa place en VCS. Supprimer du tracking et ajouter *.bak a .gitignore.
- [-] [Review][Patch] **Commit message ok non descriptif** — Le message ok ne convey aucune intention. Devrait suivre le format conventional commits.
- [x] [Review][Patch] **Mismatch de statut story vs sprint-status** — Le fichier story dit Status: in-progress mais sprint-status.yaml dit ready-for-dev. Synchroniser a in-progress.
- [x] [Review][Patch] **Reformatage (reverted as side effect) whitespace gratuit dans sprint-status.yaml** — 226 lignes de bruit pour un changement d indentation. Pollue git blame et masque le vrai changement.
- [x] [Review][Patch] **AC #1 incomplet : project-context.md** — Tasks 1.3, 1.4, 1.5, 1.6 non faites. Au moins 5 references Copilot restent.
- [x] [Review][Patch] **AC #2 non commence : bmad-bot.yaml.example jamais touche** — Les 4 sous-taches (2.1-2.4) non implementees.
- [x] [Review][Patch] **AC #3 non commence : README.md jamais touche** — Les 7 sous-taches (3.1-3.7) non implementees.
- [x] [Review][Defer] **Pas de CI gate / peer review visible** — Probleme de process pre-existant. — deferred, pre-existing
