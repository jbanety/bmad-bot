# Story 6.4: Context Window Limit Recovery

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want the daemon to recover from context window limit errors without losing session progress,
So that long or complex stories can still be completed autonomously.

## Acceptance Criteria

1. **Given** the agent session is active and the chat history has grown large **When** the LLM API returns a context limit error **Then** the error is detected from the provider response in the chat loop **And** the recovery process is initiated (not a crash — a controlled recovery)

2. **Given** a context limit error has been detected **When** the recovery process starts **Then** the full chat_history is read from the in-memory `SessionState` (which mirrors the WAL, already persisted after each turn) **And** the last N exchanges are extracted verbatim as immediate context **And** a separate, fresh LLM call is made (new context, not the exhausted one) to summarize the full chat_history into a compact session summary

3. **Given** the summary has been generated **When** the new session is bootstrapped **Then** a fresh agent is constructed with the same provider/model config and the same persona + tools **And** the new session preamble includes: agent persona + tool registrations, project context (project-context.md), the generated session summary, the last N verbatim exchanges, and the current story file reference **And** the session enters direct chat mode (not re-entering the full dev-story workflow pipeline, since checkboxes and Dev Agent Record are already up to date on disk)

4. **Given** the new session is bootstrapped **When** the chat loop resumes **Then** the agent picks up the current task with full awareness of prior work **And** the recovery event is logged via `tracing::info!()` with `action = "context_limit_recovery"`, original history length, and summary length **And** the WAL file is updated with the new (compressed) session state

## Functional Requirements Covered

- **NFR-REL1:** Transient LLM errors recovered with exponential backoff, max 3 retries per call
- **NFR-REL3:** Crash recovery produces clean state — no corrupted branches, no half-committed files
- **Architecture Decision 3:** Session State Persistence — WAL File for Crash & Context Limit Recovery (Recovery Case B)

## Dependencies

- **Story 6.3 (Crash Recovery via Session WAL) MUST be completed first.** This story builds on the `run_session()` refactor from 6.3 (the `Option<SessionState>` parameter) and reuses `RecoveryInfo`, `story_info_from_wal()`, and the WAL infrastructure. Story 6.3 also introduces the pattern of rebuilding agents from WAL state that this story extends.
- **Story 6.2 (HTTP Retry & Error Resilience) MUST be completed first.** This story uses `StoryPipeline` created by 6.2.

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [ ] 🚨 **BLOCKING**: Verify `run_session()` in `src/session/runner.rs` accepts `recovered_state: Option<SessionState>` as its last parameter (added by Story 6.3). **If this parameter does NOT exist, STOP — Story 6.3 MUST be implemented first.** This story's recovery design calls `run_session()` recursively with `Some(compressed_state)` for the inner loop, which is impossible without this parameter.
- [ ] Verify `SessionState` in `src/session/state.rs` with `save()`, `load()`, `to_rig_messages()`, `add_user_message()`, `add_assistant_message()`, `chat_history: Vec<ChatMessage>`
- [ ] Verify `ChatMessage` struct: `role: String`, `content: String` (derives `Clone`)
- [ ] Verify `SessionRunner` has `build_anthropic_agent()`, `build_openai_agent()`, `build_preamble()`, `create_tools()`, `state_file_path`
- [ ] Verify `resolve_api_key()` in `src/session/provider.rs`
- [ ] Verify `EscalationSlot` type alias in `src/supervisor/mod.rs`
- [ ] Verify `DecisionLog` in `src/supervisor/decisions.rs`
- [ ] Verify `ResponseAnalyzer` in `src/session/analyzer.rs`
- [ ] Verify `rig-core` version 0.30 — `agent.chat()` returns `Result<String, PromptError>`, errors surface as `CompletionError::ProviderError(String)` or `CompletionError::ResponseError(String)`
- [ ] Verify project-context.md exists at `_bmad-output/project-context.md`

### Task 1: Add Context Limit Error Detection (`src/session/runner.rs`)

- [ ] Add helper function `fn is_context_limit_error(error: &str) -> bool`:
  ```rust
  /// Detect context window / token limit errors from LLM provider error strings.
  ///
  /// Each provider returns different error messages. This function checks for
  /// known patterns across Anthropic, OpenAI, and GitHub Models.
  fn is_context_limit_error(error: &str) -> bool {
      let lower = error.to_lowercase();
      // Anthropic patterns
      lower.contains("context_length_exceeded")
          || lower.contains("prompt is too long")
          || lower.contains("maximum context length")
          // OpenAI patterns
          || lower.contains("maximum context length")
          || lower.contains("context_length_exceeded")
          || lower.contains("max_tokens")
          || lower.contains("token limit")
          || lower.contains("context window")
          // Generic patterns
          || lower.contains("too many tokens")
          || lower.contains("input too long")
          || lower.contains("exceeds the model")
  }
  ```
- [ ] This is a pure function — easy to test, no dependencies

### Task 2: Modify Chat Loop Error Handling in `run_session()` (`src/session/runner.rs`)

- [ ] In the `Err(e)` branch of `agent.chat(&reply, history).await` (around line 626-649):
- [ ] BEFORE the existing retry logic, add context limit detection:
  ```rust
  Err(e) => {
      let error_str = e.to_string();

      // Check for context limit error BEFORE retry logic
      if is_context_limit_error(&error_str) {
          tracing::warn!(
              action = "context_limit_detected",
              turn = %turn,
              history_len = %state.chat_history.len(),
              error = %error_str,
              "Context window limit hit — initiating recovery"
          );

          // Remove the user message we just added (it failed)
          state.chat_history.pop();

          // Recovery runs its own inner chat loop to completion via run_session().
          // It returns a terminal SessionOutcome — the current loop exits.
          let outcome = self.context_limit_recovery(
              state, story, provider, model, base_branch,
              escalation_slot.clone(), decision_log.clone(),
              0, // recovery_depth: first recovery attempt
          ).await;

          // Write decisions regardless of outcome
          self.write_decisions(story, &decision_log).await;
          return outcome;  // Exit the current run_session()
      }

      // Existing retry logic for non-context-limit errors continues below...
      retries += 1;
      // ... (rest of existing retry code unchanged)
  }
  ```
- [ ] The context limit check MUST come BEFORE the `retries += 1` line — retrying a context limit error is pointless (same history = same error)
- [ ] The failed user message MUST be popped from `state.chat_history` before recovery (it was added by `state.add_user_message(&reply)` before the `agent.chat()` call)
- [ ] Recovery is a **terminal action** for the current chat loop — the method returns `SessionOutcome` directly and the caller does `return outcome`. The fresh agent runs its own inner loop via `run_session(Some(compressed_state))`.
- [ ] Pass `recovery_depth: 0` on first recovery. Inside `context_limit_recovery()`, if another context limit is hit, the inner `run_session()` will call recovery again with `recovery_depth + 1`, up to `MAX_RECOVERY_DEPTH = 3`.

### Task 3: Implement `context_limit_recovery()` Method (`src/session/runner.rs`)

This is the unified recovery method. It summarizes history, builds a fresh agent with an enhanced preamble, then delegates to `run_session(Some(compressed_state))` for the inner chat loop. It returns `SessionOutcome` directly — the caller treats recovery as a terminal action.

- [ ] Add constants:
  ```rust
  const RECOVERY_KEEP_LAST_EXCHANGES: usize = 10; // 10 exchanges = 20 messages
  const MAX_RECOVERY_DEPTH: usize = 3;
  ```

- [ ] Add method signature to `SessionRunner`:
  ```rust
  /// Recover from a context window limit error by summarizing history and
  /// bootstrapping a fresh session via `run_session(Some(compressed_state))`.
  ///
  /// Architecture Decision 3, Recovery Case B. The method:
  /// 1. Extracts last N exchanges from in-memory state as immediate context
  /// 2. Makes a fresh LLM call to summarize the full history
  /// 3. Builds an enhanced preamble with summary + last N exchanges + project context
  /// 4. Builds a fresh agent (provider-matched) with enhanced preamble + all tools
  /// 5. Calls `run_session()` with `Some(compressed_state)` — reuses the existing
  ///    chat loop instead of duplicating it
  /// 6. Returns the `SessionOutcome` from the inner loop directly
  ///
  /// If `recovery_depth >= MAX_RECOVERY_DEPTH`, returns `SessionOutcome::Failed`
  /// to prevent infinite recursion.
  async fn context_limit_recovery(
      &self,
      state: &SessionState,
      story: &StoryInfo,
      provider: &str,
      model: &str,
      base_branch: &str,
      escalation_slot: EscalationSlot,
      decision_log: DecisionLog,
      recovery_depth: usize,
  ) -> SessionOutcome
  ```

- [ ] **Step 0 — Check recovery depth:**
  - [ ] If `recovery_depth >= MAX_RECOVERY_DEPTH`:
    ```rust
    tracing::error!(
        action = "context_limit_max_depth",
        depth = %recovery_depth,
        "Max recovery depth reached — aborting"
    );
    return SessionOutcome::Failed {
        story_key: story.story_key.clone(),
        error: format!("Context limit recovery exceeded max depth ({MAX_RECOVERY_DEPTH})"),
        decisions: decision_log.records(),
    };
    ```

- [ ] **Step 1 — Extract last N exchanges:**
  - [ ] Call `extract_last_exchanges(&state.chat_history, RECOVERY_KEEP_LAST_EXCHANGES)` (see Task 4 helpers)
  - [ ] Call `format_exchanges_for_preamble(&last_exchanges)` for preamble injection

- [ ] **Step 2 — Summarize full history via fresh LLM call:**
  - [ ] Call `self.summarize_history(state, story, provider, model).await`
  - [ ] If summarization fails → return `SessionOutcome::Failed` with error detail
  - [ ] Log: `tracing::info!(action = "context_limit_summary_generated", original_len = %state.chat_history.len(), summary_len = %summary.len())`

- [ ] **Step 3 — Build enhanced preamble:**
  - [ ] Call `self.build_recovery_preamble(story, &summary, &formatted_exchanges)`
  - [ ] If preamble build fails → return `SessionOutcome::Failed`
  - [ ] Enhanced preamble format:
    ```
    {original_preamble}

    === SESSION RECOVERY — Context Window Limit Reached ===
    Your previous session hit the context window limit. Below is your session context:

    === Project Context ===
    {project_context}

    === Session Summary ===
    {summary}

    {last_n_exchanges_formatted}

    === Current Story ===
    The story file is at: {specs_path}
    Read this file to see current task checkboxes and progress.
    Continue working directly on the current task. Do NOT restart the workflow from the beginning.
    ```

- [ ] **Step 4 — Build compressed `SessionState` and fresh agent, then delegate to `run_session()`:**
  - [ ] Build compressed state (see Task 5 for details)
  - [ ] Resolve API key via `resolve_api_key(provider, &self.secrets)`
  - [ ] Match on provider string (same pattern as `run()` and `resume_session()`):
    - [ ] `"anthropic"` → build Anthropic client, create agent with **enhanced preamble** + all 4 tools
    - [ ] `"openai"` → build OpenAI client, same pattern
    - [ ] `"github-models"` → build OpenAI client with base URL override, same pattern
  - [ ] **Within each match arm**, call `run_session()` with `recovered_state: Some(compressed_state)`:
    ```rust
    // Inside the "anthropic" match arm (openai/github-models are identical pattern):
    let agent = client.agent(model)
        .preamble(&enhanced_preamble)
        .tool(git).tool(fs).tool(terminal).tool(supervisor)
        .build();

    // Delegate to run_session() — reuses the existing chat loop
    // The inner loop handles completion/escalation/failure normally
    let outcome = self.run_session(
        &agent, story, provider, model, base_branch,
        escalation_slot.clone(), decision_log.clone(),
        Some(compressed_state), // ← This is the key: recovery state passed in
    ).await;
    ```
  - [ ] The `Chat` trait is NOT object-safe — `agent` must be built and used within the same match arm. `run_session()` is generic over `<A: Chat>`, so each concrete agent type works.
  - [ ] IMPORTANT: `EscalationSlot` and `DecisionLog` are the SAME instances from the parent call — decision continuity is preserved across recovery boundary

- [ ] **Step 5 — Log and return:**
  - [ ] `tracing::info!(action = "context_limit_recovery", depth = %recovery_depth, original_history_len = %state.chat_history.len(), "Context limit recovery delegated to inner run_session()")`
  - [ ] Return the `SessionOutcome` from the inner `run_session()` call directly
  - [ ] If another context limit is hit inside the inner loop, `run_session()` will call `context_limit_recovery()` again with `recovery_depth + 1` — the depth check in Step 0 prevents infinite recursion

**Design rationale:** By delegating to `run_session(Some(compressed_state))`, we reuse ALL existing chat loop logic (completion detection, escalation handling, failure handling, WAL management, turn counting). No code duplication. The fresh agent has a clean context (enhanced preamble only), and the compressed state's `chat_history` contains only the last N exchanges.

### Task 4: Implement Helper Functions (`src/session/runner.rs`)

- [ ] `fn extract_last_exchanges(history: &[ChatMessage], n: usize) -> Vec<ChatMessage>`:
  - [ ] Extract the last `n * 2` messages (n exchanges = n user + n assistant messages)
  - [ ] If history has fewer messages, return all of them
  - [ ] **Odd message count handling:** If history length is odd (e.g., 21 messages — unpaired trailing user message), round DOWN to the nearest even number before slicing. The orphan message is excluded to keep clean user/assistant pairs. Example: history of 21, N=10 → take last 20 messages (10 pairs), the orphan 1st message is dropped.
  - [ ] Return cloned messages (`ChatMessage` derives `Clone`)

- [ ] `fn format_exchanges_for_preamble(exchanges: &[ChatMessage]) -> String`:
  - [ ] Format as readable text:
    ```
    === Recent Conversation (last N exchanges) ===
    User: {content}
    Assistant: {content}
    ...
    ```
  - [ ] Truncate individual messages if extremely long (> 2000 chars) with `"... [truncated]"` to keep preamble within reasonable bounds

- [ ] `async fn summarize_history(&self, state: &SessionState, story: &StoryInfo, provider: &str, model: &str) -> Result<String, String>`:
  - [ ] Format the full `state.chat_history` as text (`"User: {content}\nAssistant: {content}\n..."`)
  - [ ] Resolve API key via `resolve_api_key(provider, &self.secrets)`
  - [ ] Build a minimal agent (no tools) with summarization preamble — match on provider string (same pattern as elsewhere):
    ```rust
    // Anthropic example (OpenAI/GitHub-Models follow same pattern):
    let client = anthropic::Client::builder().api_key(&api_key).build()
        .map_err(|e| format!("Summarization client failed: {e}"))?;
    let agent = client.agent(model)
        .preamble("You are a technical session summarizer. Be concise but comprehensive.")
        .build();
    ```
  - [ ] Build the summarization prompt (sent as the user message with empty history):
    ```
    You are summarizing a development session for continuity. The session is implementing
    story {story_key} in a software project. Provide a concise but comprehensive summary
    covering:
    - What tasks have been completed (with file paths and key decisions)
    - What task is currently in progress
    - Any issues encountered and how they were resolved
    - Key code patterns and conventions established
    - Current state of the implementation

    Here is the full conversation history:
    {formatted_full_history}
    ```
  - [ ] Call `agent.chat(&summarization_prompt, vec![]).await` — empty history, prompt-only
  - [ ] **Fallback on failure:** If the summarization call fails with a context limit error (the history itself is too large for a fresh context), retry with truncated history (last 50% of messages). If that also fails → return `Err("Summarization failed even with truncated history: {e}")`
  - [ ] Return the summary string

- [ ] `fn build_recovery_preamble(&self, story: &StoryInfo, summary: &str, formatted_exchanges: &str) -> Result<String, String>`:
  - [ ] Load original preamble via `self.build_preamble(story)`
  - [ ] Load project-context.md from `config.bmad_paths.output_folder + "/project-context.md"`
  - [ ] Assemble the enhanced preamble (see format in Task 3, Step 3)
  - [ ] Include the current task name in the story reference section for immediate context. Instead of just "Continue working on the current task", identify the last unchecked task from the story file path if possible:
    ```
    === Current Story ===
    The story file is at: {specs_path}
    Read this file to see current task checkboxes and progress.
    Continue working directly on the current task. Do NOT restart the workflow from the beginning.
    ```
  - [ ] Return the full preamble string

### Task 5: Build New Compressed `SessionState` (`src/session/runner.rs`)

- [ ] When building the new state after recovery:
  ```rust
  let mut new_state = SessionState {
      story_id: state.story_id.clone(),
      story_key: state.story_key.clone(),
      branch: state.branch.clone(),
      started_at: state.started_at.clone(),  // preserve original start time
      last_activity: chrono::Utc::now().to_rfc3339(),
      provider: state.provider.clone(),
      model: state.model.clone(),
      branch_name: state.branch_name.clone(),
      base_branch: state.base_branch.clone(),
      chat_history: last_exchanges.clone(),   // ONLY the last N exchanges, not full history
  };
  ```
- [ ] The compressed state's `chat_history` starts with ONLY the last N exchanges
- [ ] New messages from the recovered session are appended normally after this
- [ ] The WAL is overwritten with the compressed state — the full history is gone from disk (the summary captures it in the preamble)
- [ ] This ensures the WAL file stays small even across multiple recovery events

### Task 6: Unit Tests

- [ ] `test_is_context_limit_error_anthropic_pattern` — verify detection of `"context_length_exceeded"`
- [ ] `test_is_context_limit_error_openai_pattern` — verify detection of `"maximum context length"`
- [ ] `test_is_context_limit_error_token_limit` — verify detection of `"token limit"`
- [ ] `test_is_context_limit_error_too_many_tokens` — verify detection of `"too many tokens"`
- [ ] `test_is_context_limit_error_case_insensitive` — verify `"CONTEXT_LENGTH_EXCEEDED"` matches
- [ ] `test_is_context_limit_error_false_for_network_error` — verify `"connection refused"` returns false
- [ ] `test_is_context_limit_error_false_for_auth_error` — verify `"invalid api key"` returns false
- [ ] `test_is_context_limit_error_false_for_rate_limit` — verify `"rate limit exceeded"` returns false (rate limits should retry, not recover)
- [ ] `test_extract_last_exchanges_normal` — verify 10 exchanges from 40-message history returns last 20 messages
- [ ] `test_extract_last_exchanges_fewer_than_n` — verify 3-exchange history returns all 6 messages when N=10
- [ ] `test_extract_last_exchanges_empty_history` — verify empty history returns empty vec
- [ ] `test_extract_last_exchanges_odd_message_count` — verify odd-length history (e.g., 21 messages) rounds DOWN to nearest even count before slicing, dropping the orphan message to keep clean user/assistant pairs
- [ ] `test_format_exchanges_for_preamble_basic` — verify correct "User: / Assistant:" formatting
- [ ] `test_format_exchanges_for_preamble_truncates_long_messages` — verify messages > 2000 chars are truncated
- [ ] `test_format_exchanges_for_preamble_empty` — verify empty exchanges produce reasonable output
- [ ] `test_build_recovery_preamble_contains_all_sections` — verify output contains "SESSION RECOVERY", "Project Context", "Session Summary", "Recent Conversation", "Current Story"
- [ ] `test_build_recovery_preamble_includes_story_path` — verify `specs_path` is in the output
- [ ] `test_compressed_state_preserves_metadata` — verify `story_id`, `story_key`, `branch`, `provider`, `model` are preserved from original state
- [ ] `test_compressed_state_has_only_last_exchanges` — verify `chat_history` length is `min(N*2, original_len)`
- [ ] `test_compressed_state_updates_last_activity` — verify `last_activity` is refreshed
- [ ] All tests use mocked data — NO real LLM calls, NO real file I/O (except tempdir for WAL)

### Task 7: Integration Verification

- [ ] `cargo check` — 0 errors
- [ ] `cargo test` — all existing + new tests pass, 0 regressions
- [ ] `cargo clippy` — 0 new warnings
- [ ] `cargo fmt` — clean
- [ ] All public items have `///` doc comments

## Dev Notes

### Previous Story Intelligence

**From Story 6.3 (Crash Recovery via Session WAL) — immediate predecessor, MUST be done first:**
- `run_session()` now accepts `recovered_state: Option<SessionState>` as last parameter
- `RecoveryInfo` struct, `story_info_from_wal()` helper
- `resume_session()` method on `SessionRunner` for crash recovery
- `recover_and_process()` on `StoryPipeline` — single entry point
- WAL is ALWAYS deleted after recovery attempt (prevents infinite loops)
- `SessionState` does NOT implement `Clone` — consume by ownership. But `ChatMessage` DOES implement `Clone`
- Turn counter initialized to `state.chat_history.len() / 2` in recovery mode
- Three edge cases handled: last-msg-assistant, last-msg-user, empty-history

**From Story 6.2 (HTTP Retry & Error Resilience) — MUST be done first:**
- `StoryPipeline` with `process_story()`, `process_eligible_stories()`
- Pipeline handles post-session flow: review → PR → notify
- Layer 3 error handling: session failures create failure PRs, daemon never stops

**From Story 6.1 (Telegram Notifications):**
- `Notifier` trait, `TelegramNotifier`, `NoopNotifier`
- All notification failures are non-blocking

**From Story 4.2 (Agent Session Setup & Chat Loop) — chat loop owner:**
- `run_session()` is a **private** async method with generic `<A: Chat>` — different concrete return types per provider
- `Chat` trait is NOT object-safe — cannot use `Box<dyn Chat>`. Must match on provider string and build concrete type
- Error from `agent.chat()` is `PromptError` (rig 0.30) — `.to_string()` produces the error message containing provider-specific text
- Chat loop retry pattern: on `Err(e)`, increment retries, pop last user message, continue. After MAX_RETRIES=3 → `SessionOutcome::Failed`
- WAL saved after each successful turn via `state.save(&self.state_file_path).await`
- The `state.chat_history.pop()` pattern is already used for retry (remove the failed user message)

**From Story 4.3 (Pre-Development Preparation & Branch Management):**
- `SessionState` fields `branch_name` and `base_branch` with `serde(default)` for backward compat

**From Story 5.2 (Automated Code Review):**
- `ReviewRunner` also uses the `<A: Chat>` generic pattern with provider matching
- Confirms that the "build agent inside match arm" pattern is the established approach

### Git Intelligence (Last 5 Commits)

1. `cd93cc6` docs(stories): create story 6-3 crash recovery via session WAL and update sprint status
2. `a57a125` docs(stories): create story 6-2 HTTP retry and error resilience and update sprint status
3. `97b7c80` docs(stories): create story 6-1 telegram notifications and update sprint status
4. `cdc25c3` feat(git-provider): implement GitLabProvider with full GitProvider trait support
5. `dea1232` feat(review): implement automated code review session runner

### Core Design — Context Window Limit Recovery Flow

Architecture Decision 3, Recovery Case B. This is a controlled, mid-session recovery — NOT a crash. Recovery is a **terminal action** for the current chat loop — it builds a fresh agent, delegates to a NEW `run_session()` call, and returns the outcome.

```
chat loop running
└── agent.chat(&reply, history).await → Err(e)
    └── is_context_limit_error(&e.to_string())?
        ├── NO → existing retry logic (retries += 1, pop user msg, continue)
        └── YES → context_limit_recovery(state, ..., recovery_depth=0)
            ├── check recovery_depth < MAX_RECOVERY_DEPTH (3)
            │   └── depth exceeded → return SessionOutcome::Failed
            ├── extract last N exchanges from state.chat_history (clone)
            ├── format exchanges for preamble injection
            ├── summarize_history() — fresh LLM call with full history
            │   ├── build minimal agent (no tools, just summarization prompt)
            │   ├── call agent.chat(summarization_prompt, [])
            │   └── return summary string
            ├── build_recovery_preamble()
            │   ├── original preamble (build_preamble)
            │   ├── + project-context.md content
            │   ├── + session summary
            │   ├── + last N verbatim exchanges
            │   └── + story file reference with "continue" instruction
            ├── build compressed SessionState (last N exchanges only)
            ├── match on provider → build fresh agent with enhanced preamble + 4 tools
            ├── call run_session(&agent, ..., Some(compressed_state))
            │   └── inner loop runs to completion with fresh context
            │       ├── if context limit hit again → recursive recovery(depth+1)
            │       ├── Completed → WAL deleted, return Completed
            │       ├── Escalated → WAL deleted, return Escalated
            │       └── Failed → WAL preserved, return Failed
            └── return SessionOutcome from inner run_session()
                └── caller does `return outcome` — exits the outer loop
```

**Key difference from Story 6.3 (crash recovery):**
- Crash recovery happens at daemon startup — context limit recovery happens mid-session
- Crash recovery rebuilds from WAL on disk — context limit recovery uses in-memory state (WAL is backup)
- Crash recovery re-enters `run_session()` — context limit recovery ALSO re-enters `run_session()` but with a fresh agent and compressed state
- Crash recovery always deletes WAL after — context limit overwrites WAL with compressed state (handled by inner `run_session()`)
- Context limit recovery has a depth limit (`MAX_RECOVERY_DEPTH = 3`) to prevent infinite recursion

### How `agent.chat()` Errors Surface

In rig 0.30, `agent.chat()` returns `Result<String, PromptError>`. The error chain:

1. Provider API returns HTTP 400 with body like `{"error": {"type": "invalid_request_error", "message": "prompt is too long: 204835 tokens > 200000 maximum"}}`
2. rig parses this into `CompletionError::ProviderError(String)` containing the error message
3. This surfaces to `agent.chat()` as a `PromptError` wrapping the `CompletionError`
4. Calling `.to_string()` on the error produces a string containing the provider's error message

The `is_context_limit_error()` function matches against `.to_string()` output — it must cover patterns from all three supported providers (Anthropic, OpenAI, GitHub Models/Azure).

### Why NOT Retry Context Limit Errors

The existing retry logic retries with the SAME history. For context limit errors, the history is too long — retrying will fail with the exact same error. Context limit recovery MUST intercept BEFORE the retry counter to avoid wasting 3 failed retries.

### Summarization Call — Design Details

The summarization call uses a FRESH LLM context (empty history). Key constraints:

1. **Provider:** Use the SAME provider/model as the dev session — ensures consistent quality
2. **Tools:** NO tools needed — summarization is text-only
3. **Preamble:** Simple instruction: "You are summarizing a development session"
4. **Input:** The full `chat_history` formatted as text (user/assistant turns)
5. **Risk:** If the full history is SO large it exceeds context even for summarization → fallback to truncated history (last 50%)
6. **Output:** A structured summary suitable for injection into a new preamble

### Enhanced Preamble Size Considerations

The enhanced preamble must fit within the context window alongside new conversation. Budget:

| Component | Estimated tokens |
|---|---|
| Original preamble (dev.md + override) | ~2,000-5,000 |
| Project context (project-context.md) | ~1,000-3,000 |
| Session summary | ~500-2,000 |
| Last 10 exchanges (possibly truncated) | ~2,000-10,000 |
| Story file reference | ~100 |
| **Total preamble** | **~5,600-20,100** |

This leaves 180K-195K tokens for new conversation (Anthropic 200K window). OpenAI models with smaller windows may need fewer kept exchanges — but N=10 is a reasonable default for MVP.

### Design Decision: Chat Trait Not Object-Safe — Implications for Recovery

The `Chat` trait in rig 0.30 is NOT object-safe (`fn chat()` returns `impl Future`). This drives the recovery design:

- Cannot use `Box<dyn Chat>` — must match on provider string and build concrete agent types
- The old agent's context is exhausted after a context limit error — cannot reuse it
- **Solution:** `context_limit_recovery()` builds a fresh agent inside a provider match arm, then calls `run_session(Some(compressed_state))` within the same arm. This reuses the existing chat loop and all completion/escalation/failure handling. The method returns `SessionOutcome` directly — the outer loop exits.
- Recursive recovery (if inner loop also hits context limit) is bounded by `MAX_RECOVERY_DEPTH = 3`

### Error Handling in Context Limit Recovery

| Error | Response | WAL State |
|---|---|---|
| `recovery_depth >= MAX_RECOVERY_DEPTH` | Return `SessionOutcome::Failed` | WAL preserved (original state) |
| API key resolution fails | Return `SessionOutcome::Failed` | WAL preserved (original state) |
| Summarization LLM call fails | Try with truncated history (50%); if still fails → `Failed` | WAL preserved |
| Agent build fails | Return `SessionOutcome::Failed` | WAL preserved |
| Recovery succeeds → inner `run_session()` completes | `Completed` | WAL deleted by inner loop |
| Recovery succeeds → inner `run_session()` escalates | `Escalated` | WAL deleted by inner loop |
| Recovery succeeds → inner `run_session()` fails | `Failed` | WAL preserved by inner loop (with compressed state) |
| Recovery succeeds → inner loop hits context limit again | Recursive `context_limit_recovery(depth+1)` | Handled by inner recursion |

### Architecture Compliance

| Constraint | Implementation |
|---|---|
| No new modules | All code in `src/session/runner.rs` — helper functions + recovery method |
| Error handling | Returns `SessionOutcome` variants — integrates with existing pipeline |
| Error field pattern | `{ reason: String }` for any new error types |
| Sequential execution | Recovery is synchronous within the session — no parallel sessions |
| WAL update | Compressed state overwrites WAL — keeps file small |
| Logging | `tracing` only — structured fields with `action = "context_limit_recovery"` |
| Doc comments | `///` on all new public/private functions |
| Tests | Inline `#[cfg(test)] mod tests` — mock data only |
| No unsafe | No unsafe code |
| BMAD sacred | No modifications to `_bmad/` |

### Existing Code to Reuse (DO NOT Reinvent)

| Component | Location | What to use |
|---|---|---|
| `SessionState` | `src/session/state.rs` | WAL state, `save()`, `to_rig_messages()`, `add_user_message()`, `add_assistant_message()` |
| `ChatMessage` | `src/session/state.rs` | Message format, derives `Clone` |
| `SessionRunner` | `src/session/runner.rs` | `build_anthropic_agent()`, `build_openai_agent()`, `build_preamble()`, `create_tools()`, `run_session()` |
| `resolve_api_key()` | `src/session/provider.rs` | API key resolution |
| `ResponseAnalyzer` | `src/session/analyzer.rs` | Response pattern matching (used by inner loop) |
| `EscalationSlot` | `src/supervisor/mod.rs` | Shared escalation slot (passed through to inner loop) |
| `DecisionLog` | `src/supervisor/decisions.rs` | Decision tracking (shared across recovery boundary) |
| `SessionOutcome` | `src/session/mod.rs` | `Completed`, `Escalated`, `Failed` |
| `run_session()` | `src/session/runner.rs` | Reuse for inner loop after recovery (with `Some(compressed_state)`) |

⚠️ **Do NOT reimplement any of these.** In particular:
- Do NOT create a new chat loop — reuse `run_session()` with `Some(compressed_state)` for the inner loop
- Do NOT create new standalone agent builder methods — build recovery agents **inline** within `context_limit_recovery()` using the same rig client/agent builder pattern as `build_anthropic_agent()`/`build_openai_agent()`, but with the **enhanced preamble** instead of `self.build_preamble()`. The existing builders hardcode the standard preamble and cannot be reused directly for recovery.
- Do NOT create a new WAL format — overwrite existing WAL with compressed `SessionState`
- Do NOT create a summarization-specific LLM infrastructure — build a minimal temp agent inline (no tools, summarization preamble only)

### Library & Framework Requirements

| Dependency | Version | Purpose | Already in Cargo.toml |
|---|---|---|---|
| `rig-core` | 0.30 | Chat trait, agent building, providers | ✅ Yes |
| `tokio` | latest | Async runtime | ✅ Yes |
| `tracing` | 0.1 | Structured logging | ✅ Yes |
| `serde` / `serde_yml` | latest | WAL serialization | ✅ Yes |
| `chrono` | latest | Timestamps | ✅ Yes |
| `thiserror` | 2 | Error types | ✅ Yes |

**No new dependencies needed.**

### File Structure Requirements

**Files to modify:**
- `src/session/runner.rs` — **MODIFY** — Add `is_context_limit_error()`, `context_limit_recovery()`, `summarize_history()`, `build_recovery_preamble()`, `extract_last_exchanges()`, `format_exchanges_for_preamble()`. Modify `run_session()` error handling to intercept context limit errors before retry logic.

**Files NOT to touch:**
- `src/session/state.rs` — WAL infrastructure is complete
- `src/session/mod.rs` — No new public types to export
- `src/session/branch.rs` — Branch operations are complete
- `src/session/analyzer.rs` — Response analysis is complete
- `src/session/provider.rs` — Provider factory is complete
- `src/session/cleanup.rs` — Cleanup operations are complete
- `src/pipeline.rs` — Pipeline unchanged (recovery is invisible to pipeline)
- `src/cli/mod.rs` — CLI unchanged (recovery is invisible to caller)
- `src/config/` — Config is complete
- `src/watcher/` — Watcher is complete
- `src/notifier/` — Notifier is complete
- `src/git_provider/` — Git provider is complete
- `src/review/` — Review runner is complete
- `src/supervisor/` — Supervisor is complete
- `src/tools/` — Agent tools are complete
- `Cargo.toml` — No new dependencies
- Anything under `_bmad/` — Read-only, sacred

### Testing Requirements

All tests inline in `#[cfg(test)] mod tests` at the bottom of `src/session/runner.rs`:
- Use `#[test]` for synchronous tests (`is_context_limit_error`, `extract_last_exchanges`, `format_exchanges_for_preamble`)
- Use `#[tokio::test]` for async tests if needed
- Naming convention: `test_{function}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- Mock all external dependencies — NO real LLM calls
- `is_context_limit_error()` and extraction/formatting functions are pure — no mocking needed
- Do NOT test `context_limit_recovery()` end-to-end (requires live LLM) — test the helper functions and error detection thoroughly

### Anti-Patterns to Avoid

- ❌ Do NOT retry context limit errors — they will fail with the same error. Intercept BEFORE retry logic
- ❌ Do NOT use `Box<dyn Chat>` — `Chat` is not object-safe in rig 0.30. Match on provider and build concrete types
- ❌ Do NOT try to swap the agent variable in the outer loop — the original agent's context is exhausted. Delegate to a NEW `run_session()` call with `Some(compressed_state)` and return its outcome
- ❌ Do NOT return `(SessionState, String)` from recovery and `continue` in the loop — the old agent variable is still bound to the exhausted context. Recovery MUST be a terminal action that returns `SessionOutcome`
- ❌ Do NOT keep full history in the compressed WAL — only last N exchanges. The summary is in the preamble, not the WAL
- ❌ Do NOT use a different LLM provider/model for summarization — use the same one for consistency
- ❌ Do NOT send "DS" as the continuation prompt — that restarts the entire workflow. The inner `run_session(Some(state))` handles this via Story 6.3's recovery path (last message analysis)
- ❌ Do NOT duplicate the chat loop — reuse `run_session()` with `Some(compressed_state)`
- ❌ Do NOT allow infinite recursive recoveries — cap at `MAX_RECOVERY_DEPTH = 3` via `recovery_depth` parameter
- ❌ Do NOT omit `recovery_depth` from the `context_limit_recovery()` signature — it is required for recursion safety
- ❌ Do NOT use `unwrap()` or `expect()` in production code
- ❌ Do NOT use `println!` or `eprintln!` — use `tracing` only
- ❌ Do NOT use `anyhow` in session modules — `thiserror` only
- ❌ Do NOT add new dependencies

### Scope Boundaries

**In scope:**
- Context limit error detection from provider error strings (`is_context_limit_error()`)
- Interception in `run_session()` error handler — before retry logic
- Chat history summarization via fresh LLM call
- Enhanced preamble construction with summary + last N exchanges + project context + story reference
- Fresh agent construction with enhanced preamble and same tools
- Compressed `SessionState` with only last N exchanges
- WAL overwrite with compressed state (handled by inner `run_session()`)
- Inner chat loop via `run_session()` with `Some(compressed_state)` — returns `SessionOutcome`
- Recovery depth limit (`MAX_RECOVERY_DEPTH = 3`) to prevent infinite recursion
- Summarization fallback (truncated history if full history too large)
- Unit tests for detection, extraction, formatting helpers

**Out of scope:**
- Changes to WAL format or `SessionState` struct (already complete)
- Changes to `run_session()` signature beyond what Story 6.3 already did
- Changes to pipeline or CLI (recovery is invisible — contained within `run_session()`)
- Streaming API support (future — rig 0.30 uses request/response)
- Dynamic N calculation based on model context window size (MVP uses fixed N=10; for small-context models like 8K, N=10 may need to be reduced — future enhancement)
- Persistent summary storage (summary lives only in the preamble of the recovered session)
- Changes to `ResponseAnalyzer` patterns
- New dependencies

### Project Structure Notes

After this story, modified files in the project:
```
src/
├── session/
│   ├── runner.rs       # MODIFIED — add context limit detection, recovery method, helpers
│   ├── mod.rs          # UNCHANGED
│   ├── state.rs        # UNCHANGED
│   ├── analyzer.rs     # UNCHANGED
│   ├── branch.rs       # UNCHANGED
│   ├── cleanup.rs      # UNCHANGED
│   ├── escalation.rs   # UNCHANGED
│   └── provider.rs     # UNCHANGED
├── pipeline.rs         # UNCHANGED
├── cli/
│   └── mod.rs          # UNCHANGED
├── main.rs             # UNCHANGED
├── config/             # UNCHANGED
├── watcher/            # UNCHANGED
├── notifier/           # UNCHANGED
├── git_provider/       # UNCHANGED
├── review/             # UNCHANGED
├── supervisor/         # UNCHANGED
└── tools/              # UNCHANGED
```

### References

- [Source: planning-artifacts/architecture.md — Decision 3: Session State Persistence — WAL File for Crash & Context Limit Recovery (Recovery Case B)]
- [Source: planning-artifacts/architecture.md — Decision 4: Error Propagation — Layered with Bubble-Up]
- [Source: planning-artifacts/architecture.md — Decision 5: Agent Prompt Composition — Load BMAD Agent File Directly]
- [Source: planning-artifacts/prd.md — NFR-REL1: Transient LLM errors recovered with exponential backoff]
- [Source: planning-artifacts/prd.md — NFR-REL3: Crash recovery produces clean state]
- [Source: planning-artifacts/epics.md — Epic 6, Story 6.4: Context Window Limit Recovery]
- [Source: project-context.md — Resilience Rules, Daemon Lifecycle]
- [Source: project-context.md — Session Language Override, Multi-Provider LLM Config]
- [Source: src/session/runner.rs — run_session() chat loop, build_*_agent(), build_preamble(), create_tools()]
- [Source: src/session/runner.rs — Chat error handling at L618-649, retry logic, state.chat_history.pop()]
- [Source: src/session/state.rs — SessionState, ChatMessage (derives Clone), to_rig_messages()]
- [Source: src/session/mod.rs — SessionOutcome, SessionError]
- [Source: src/session/provider.rs — resolve_api_key()]
- [Source: src/session/analyzer.rs — ResponseAnalyzer, ResponseAction]
- [Source: src/supervisor/mod.rs — EscalationSlot type alias]
- [Source: src/supervisor/decisions.rs — DecisionLog]
- [Source: src/config/mod.rs — BotConfig, bmad_paths.output_folder]
- [Source: rig-core 0.30 — CompletionError::ProviderError(String), Chat trait (not object-safe)]
- [Source: implementation-artifacts/6-3-crash-recovery-via-session-wal.md — run_session() refactor, Option<SessionState>, RecoveryInfo]
- [Source: implementation-artifacts/6-2-http-retry-error-resilience.md — StoryPipeline, pipeline flow]

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### Change Log

### File List

- `src/session/runner.rs` (MODIFIED — add `is_context_limit_error()`, `context_limit_recovery()`, `summarize_history()`, `build_recovery_preamble()`, `extract_last_exchanges()`, `format_exchanges_for_preamble()`, modify `run_session()` error handling)