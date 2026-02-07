# Story 3.1: Supervisor Tool Skeleton & Rule Engine

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want agent questions to be automatically intercepted and answered by a deterministic rule engine,
So that predictable questions are resolved instantly without LLM cost.

## Acceptance Criteria

1. **Given** the supervisor module is initialized **When** the `ask_supervisor` rig tool is built **Then** it follows the standard rig Tool pattern (serializable struct + `AskSupervisorArgs` + `SupervisorError` thiserror enum + Tool trait impl) **And** the tool NAME is `ask_supervisor` (snake_case) **And** the tool definition description is detailed enough for the LLM agent to know when and how to call it

2. **Given** the agent calls `ask_supervisor` with a question matching a known pattern **When** the rule engine in `rules.rs` evaluates the question **Then** the rule engine matches against deterministic patterns: confirmations ("Should I proceed?"), step-by-step detection, story selection prompts, and other predictable BMAD workflow interactions **And** the matched rule returns an answer immediately without any LLM call

3. **Given** the agent calls `ask_supervisor` with a question that does not match any rule **When** the rule engine evaluates the question **Then** the rule engine returns a `NoMatch` result indicating LLM fallback is needed **And** the question and attempted match are logged via `tracing::info!()` with `action = "rule_engine_miss"`

4. **Given** the rule engine is deployed **When** new patterns are identified from decision file analysis **Then** rules can be added to `rules.rs` without modifying the tool interface or supervisor module structure

## Tasks / Subtasks

- [ ] Task 0: Verify prerequisites from Epic 1 (AC: #1, #2, #3, #4)
  - [ ] 0.1 Verify `src/supervisor/mod.rs` stub exists (created in Story 1.1)
  - [ ] 0.2 Verify `src/supervisor/rules.rs` stub exists (or create alongside `decisions.rs`)
  - [ ] 0.3 Verify `rig-core` is in Cargo.toml dependencies (added in Story 1.1)
  - [ ] 0.4 Verify `serde_json` is in Cargo.toml dependencies (needed for ToolDefinition parameters)
  - [ ] 0.5 Run `cargo check` to confirm clean baseline

- [ ] Task 1: Define `SupervisorError` thiserror enum in `src/supervisor/mod.rs` (AC: #1, #3)
  - [ ] 1.1 Create `SupervisorError` with variants: `RuleEngineError { reason: String }`, `EscalationRequired { question: String, reason: String }` (placeholder for Story 3.3), `LlmFallbackNotImplemented` (placeholder for Story 3.2)
  - [ ] 1.2 Implement `Display` via thiserror derive
  - [ ] 1.3 Add `/// doc comments` on every variant explaining when it occurs
  - [ ] 1.4 Note: `SupervisorError` must implement `std::error::Error + Send + Sync` for rig Tool compatibility

- [ ] Task 2: Define `AskSupervisorArgs` in `src/supervisor/mod.rs` (AC: #1)
  - [ ] 2.1 Create `#[derive(Deserialize)] pub struct AskSupervisorArgs` with field: `question: String`
  - [ ] 2.2 Add `/// doc comment` explaining this is the input from the LLM agent when it calls the tool
  - [ ] 2.3 Optionally add `context: Option<String>` for the agent to provide additional context with the question

- [ ] Task 3: Implement `AskSupervisor` tool struct in `src/supervisor/mod.rs` (AC: #1)
  - [ ] 3.1 Create `#[derive(Deserialize, Serialize)] pub struct AskSupervisor` with field: `rule_engine: RuleEngine` (from rules.rs)
  - [ ] 3.2 Implement constructor `AskSupervisor::new() -> Self` that initializes with a default `RuleEngine`
  - [ ] 3.3 Implement `rig::tool::Tool` trait with: `NAME = "ask_supervisor"`, `Error = SupervisorError`, `Args = AskSupervisorArgs`, `Output = String`
  - [ ] 3.4 Implement `definition()` with a detailed description explaining to the LLM WHEN to call this tool (doubts, questions, blockers) and HOW (provide clear question text)
  - [ ] 3.5 Implement `call()`: try rule engine first → on match, return answer → on no match, return `LlmFallbackNotImplemented` error (Story 3.2 replaces this with actual LLM call)
  - [ ] 3.6 Log every call via `tracing::info!(action = "ask_supervisor", question = %args.question, "Supervisor tool invoked")`
  - [ ] 3.7 Log rule engine results: match at info level, miss at info level with `action = "rule_engine_miss"`

- [ ] Task 4: Implement `RuleEngine` in `src/supervisor/rules.rs` (AC: #2, #3, #4)
  - [ ] 4.1 Create `#[derive(Debug, Clone, Serialize, Deserialize)] pub struct RuleEngine` holding a `Vec<Rule>`
  - [ ] 4.2 Create `#[derive(Debug, Clone, Serialize, Deserialize)] pub struct Rule` with fields: `name: String`, `pattern: RulePattern`, `response: String`, `description: String`
  - [ ] 4.3 Create `#[derive(Debug, Clone, Serialize, Deserialize)] pub enum RulePattern` with variants: `Contains(String)`, `StartsWithAny(Vec<String>)`, `AnyOf(Vec<RulePattern>)` for composite matching. Note: NO `Regex` variant — simple string matching is sufficient for known BMAD patterns and avoids a `regex` crate dependency
  - [ ] 4.4 Implement `RuleEngine::new() -> Self` loading default built-in rules
  - [ ] 4.5 Implement `RuleEngine::evaluate(&self, question: &str) -> RuleResult` — iterates rules in order, returns first match
  - [ ] 4.6 Implement `RuleEngine::add_rule(&mut self, rule: Rule)` for extensibility (AC #4)

- [ ] Task 5: Implement `RuleResult` enum in `src/supervisor/rules.rs` (AC: #2, #3)
  - [ ] 5.1 Create `pub enum RuleResult { Matched { rule_name: String, answer: String }, NoMatch }`
  - [ ] 5.2 Implement `Display` for `RuleResult`

- [ ] Task 6: Implement built-in rules in `src/supervisor/rules.rs` (AC: #2)
  - [ ] 6.1 Confirmation patterns: "Should I proceed?", "Shall I continue?", "Do you want me to", "Ready to proceed?", "Can I go ahead?" → Response: "Yes, proceed."
  - [ ] 6.2 Step-by-step detection: "I'll do this step by step", "Let me break this down", "Here's my plan:" → Response: "Skip the step-by-step breakdown. Execute directly using yolo mode."
  - [ ] 6.3 Story selection: "Which story should I work on?", "What's the next story?" → Response: "The story file has been provided in context. Follow the tasks and acceptance criteria in the story file."
  - [ ] 6.4 Progress confirmation: "I've completed", "I'm done with", "Task complete" → Response: "Acknowledged. Continue to the next task."
  - [ ] 6.5 Permission requests: "Should I create", "Should I modify", "Should I delete", "Can I update" → Response: "Yes, proceed with the action as described."
  - [ ] 6.6 All patterns case-insensitive matching

- [ ] Task 7: Create `DecisionRecord` stub in `src/supervisor/decisions.rs` (AC: #2)
  - [ ] 7.1 Create `#[derive(Debug, Clone, Serialize, Deserialize)] pub struct DecisionRecord` with fields: `question: String`, `answer: String`, `source: DecisionSource`, `reasoning: String`, `alternatives: Vec<String>`, `timestamp: String`
  - [ ] 7.2 Create `#[derive(Debug, Clone, Serialize, Deserialize)] pub enum DecisionSource { RuleEngine { rule_name: String }, LlmFallback, HumanEscalation }`
  - [ ] 7.3 Add `// TODO: Story 3.4 — Decision file writing and session accumulation` comment
  - [ ] 7.4 Do NOT implement file writing or session accumulation — that's Story 3.4

- [ ] Task 8: Write unit tests (AC: #1, #2, #3, #4)
  - [ ] 8.1 Test rule engine matches confirmation patterns (multiple phrasings)
  - [ ] 8.2 Test rule engine matches step-by-step detection patterns
  - [ ] 8.3 Test rule engine matches story selection patterns
  - [ ] 8.4 Test rule engine matches progress confirmation patterns
  - [ ] 8.5 Test rule engine matches permission request patterns
  - [ ] 8.6 Test rule engine returns NoMatch for unknown questions
  - [ ] 8.7 Test rule engine is case-insensitive
  - [ ] 8.8 Test rule engine returns first matching rule (priority order)
  - [ ] 8.9 Test RuleEngine::add_rule adds new rules that can match
  - [ ] 8.10 Test AskSupervisor::call returns answer for matching questions
  - [ ] 8.11 Test AskSupervisor::call returns LlmFallbackNotImplemented for non-matching questions
  - [ ] 8.12 Test AskSupervisor tool definition has correct name and non-empty description
  - [ ] 8.13 Test DecisionRecord can be constructed and serialized

- [ ] Task 9: Final quality checks
  - [ ] 9.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [ ] 9.2 Run `cargo clippy` and fix any warnings
  - [ ] 9.3 Run `cargo test` and verify all tests pass (including Epic 1 and Epic 2 tests)
  - [ ] 9.4 Verify all public items have `///` doc comments
  - [ ] 9.5 Verify `SupervisorError` implements `std::error::Error + Send + Sync` (required by rig Tool trait)

## Dev Notes

### Previous Story Intelligence

**Story 1.1** established:
- `BotConfig` with `llm: LlmConfig` containing `supervisor: LlmRoleConfig { provider, model }` — the supervisor's LLM provider/model for Story 3.2's fallback. NOT used in this story (rule engine only), but the config structure is ready.
- All module stubs created including `src/supervisor/mod.rs`, plus module declarations `pub mod rules;` and `pub mod decisions;` should be added
- `ConfigError` thiserror enum as reference pattern for `SupervisorError`
- Cargo.toml includes: `rig-core`, `serde`, `serde_json`, `serde_yaml`, `thiserror`, `tracing`, `tokio`
- `build_http_client()` — shared reqwest client with retry middleware. Available for Story 3.2's LLM fallback calls.
- `Arc<BotConfig>` sharing pattern across modules
- rig-core Tool trait pattern documented as reference (struct + args + error + Tool impl)

**Story 1.2** established:
- `run_polling_loop()` with `tokio::select!` for graceful shutdown
- `Arc<BotConfig>` shared to all modules
- Tracing with structured fields, never `println!`

**Story 1.3** established:
- `Serialize` derive on all config structs — same pattern needed for `AskSupervisor` (rig requires `Serialize + Deserialize`)

**Stories 1.4, 2.1–2.3** established:
- Per-module thiserror enum pattern — apply same to `SupervisorError`
- Tracing structured fields with `action` field pattern
- Test patterns: `make_test_*` helpers, inline `#[cfg(test)] mod tests`

### Supervisor Architecture — Decision 1 Context

**Architecture Decision 1: Hybrid Chat Loop + Supervisor Tool**

The supervisor operates at two levels:
1. **Chat loop (external, daemon-controlled)** — handles workflow-level interaction (confirmations, "should I proceed?"). The daemon analyzes agent text output between turns and responds automatically. This is NOT this story — it's Epic 4.
2. **`ask_supervisor` tool (internal, agent-called)** — registered as a rig tool. When the agent has a substantive question or doubt DURING tool-calling work, it calls `ask_supervisor`. THIS is what Story 3.1 builds.

**Inside `ask_supervisor.call()`:**
1. Rule engine (deterministic, free) — matches known patterns → **THIS STORY**
2. LLM fallback (context-aware) — loads project docs to answer → **Story 3.2**
3. Human escalation — returns error, stops rig loop → **Story 3.3**

**⚠️ Important distinction:** The chat loop (Epic 4) also handles confirmations and step-by-step patterns at the session level. The rule engine in `ask_supervisor` handles the SAME patterns when the agent explicitly calls the tool. Both layers should produce consistent responses. The rule patterns defined here will also inform the chat loop's pattern matching in Epic 4.

### rig-core Tool Trait — Verified API (Latest Stable)

From rig-core research, the Tool trait API:

```rust
use rig::tool::Tool;
use rig::completion::ToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::json;

// Tool struct must be Serialize + Deserialize
#[derive(Deserialize, Serialize)]
pub struct AskSupervisor {
    rule_engine: RuleEngine,
}

// Args must be Deserialize (rig deserializes from LLM JSON output)
#[derive(Deserialize)]
pub struct AskSupervisorArgs {
    pub question: String,
}

// Error must implement std::error::Error + Send + Sync
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    // ...
}

impl Tool for AskSupervisor {
    const NAME: &'static str = "ask_supervisor";
    type Error = SupervisorError;
    type Args = AskSupervisorArgs;
    type Output = String;  // String for rig compatibility

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "ask_supervisor".to_string(),
            description: "...detailed description...".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question or doubt to ask the supervisor"
                    }
                },
                "required": ["question"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // 1. Try rule engine
        // 2. On match → return answer
        // 3. On no match → Story 3.2 adds LLM fallback here
        // For now → return LlmFallbackNotImplemented error
    }
}
```

**Tool registration** (Epic 4, for reference — NOT implemented here):
```rust
let agent = provider
    .agent(model)
    .preamble(&preamble)
    .tool(ask_supervisor)  // registered alongside git, fs, terminal
    .build();
```

### `SupervisorError` Implementation — `src/supervisor/mod.rs`

```rust
/// Errors originating from the supervisor module.
///
/// This error type must implement `std::error::Error + Send + Sync`
/// as required by the rig Tool trait. When the `ask_supervisor` tool
/// returns an error, rig stops the agent's tool-calling loop and
/// returns control to the daemon's chat loop.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    /// Internal rule engine failure (should not occur in normal operation).
    #[error("Rule engine error: {reason}")]
    RuleEngineError { reason: String },

    /// The supervisor cannot answer the question — escalate to human.
    /// When returned from the tool, rig stops the agent loop and the
    /// daemon marks the story as `needs-clarification`.
    /// Implemented fully in Story 3.3.
    #[error("Escalation required for question '{question}': {reason}")]
    EscalationRequired { question: String, reason: String },

    /// LLM fallback is not yet implemented.
    /// Placeholder for Story 3.2 — replaced by actual LLM call.
    /// For now, returned when no rule matches the question.
    #[error("LLM fallback not implemented — no rule matched the question")]
    LlmFallbackNotImplemented,
}
```

### `AskSupervisorArgs` — `src/supervisor/mod.rs`

```rust
/// Arguments passed by the LLM agent when calling the `ask_supervisor` tool.
///
/// The agent provides a question when it encounters a doubt, blocker,
/// or decision point during its dev-story workflow execution.
#[derive(Debug, Deserialize)]
pub struct AskSupervisorArgs {
    /// The question or doubt the agent wants the supervisor to answer.
    pub question: String,
    /// Optional additional context to help the supervisor answer.
    /// The agent may include relevant code snippets, error messages,
    /// or workflow state here.
    #[serde(default)]
    pub context: Option<String>,
}
```

### `AskSupervisor` Tool Implementation — `src/supervisor/mod.rs`

```rust
use rig::tool::Tool;
use rig::completion::ToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub mod rules;
pub mod decisions;

use rules::{RuleEngine, RuleResult};

/// The `ask_supervisor` rig tool — intercepts agent questions during dev sessions.
///
/// This tool is registered with the rig agent alongside git, filesystem, and
/// terminal tools. The LLM agent calls it autonomously when it encounters
/// questions, doubts, or decision points during the dev-story workflow.
///
/// **Processing pipeline:**
/// 1. Rule engine (deterministic, free) — matches known patterns
/// 2. LLM fallback (Story 3.2) — context-aware answer from project docs
/// 3. Human escalation (Story 3.3) — stops agent, notifies human
///
/// **Architecture Decision 1:** The supervisor is an internal rig tool, not an
/// external interceptor. The daemon's chat loop (Epic 4) handles workflow-level
/// interaction separately.
#[derive(Debug, Serialize, Deserialize)]
pub struct AskSupervisor {
    rule_engine: RuleEngine,
}

impl AskSupervisor {
    /// Create a new AskSupervisor with the default rule engine.
    pub fn new() -> Self {
        Self {
            rule_engine: RuleEngine::new(),
        }
    }
}

impl Default for AskSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for AskSupervisor {
    const NAME: &'static str = "ask_supervisor";
    type Error = SupervisorError;
    type Args = AskSupervisorArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "ask_supervisor".to_string(),
            description: "Ask the supervisor a question when you encounter a doubt, \
                blocker, decision point, or need clarification during your work. \
                Use this tool when: (1) you are unsure about an implementation \
                approach, (2) you need to make a decision that isn't covered by \
                the story specs, (3) you encounter an unexpected situation, \
                (4) you need confirmation on a technical choice, or \
                (5) you want to verify your understanding of a requirement. \
                Provide a clear, specific question. The supervisor will answer \
                using project documentation and established patterns."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The specific question or doubt you need answered. Be clear and provide enough context for a useful response."
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional additional context: code snippets, error messages, or relevant workflow state that helps answer the question."
                    }
                },
                "required": ["question"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::info!(
            action = "ask_supervisor",
            question = %args.question,
            has_context = args.context.is_some(),
            "Supervisor tool invoked"
        );

        // Step 1: Try rule engine (deterministic, free, fast)
        let result = self.rule_engine.evaluate(&args.question);

        match result {
            RuleResult::Matched { ref rule_name, ref answer } => {
                tracing::info!(
                    action = "rule_engine_match",
                    rule = %rule_name,
                    question = %args.question,
                    "Rule engine matched — returning deterministic answer"
                );
                Ok(answer.clone())
            }
            RuleResult::NoMatch => {
                tracing::info!(
                    action = "rule_engine_miss",
                    question = %args.question,
                    "Rule engine miss — no matching pattern found"
                );
                // TODO: Story 3.2 — Replace with LLM fallback call
                // TODO: Story 3.3 — If LLM also fails, escalate to human
                Err(SupervisorError::LlmFallbackNotImplemented)
            }
        }
    }
}
```

### `RuleEngine` Implementation — `src/supervisor/rules.rs`

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

/// Result of evaluating a question against the rule engine.
#[derive(Debug, Clone)]
pub enum RuleResult {
    /// A rule matched the question — use this answer.
    Matched {
        /// Name of the matched rule (for logging and decision records).
        rule_name: String,
        /// The deterministic answer to return.
        answer: String,
    },
    /// No rule matched — LLM fallback is needed.
    NoMatch,
}

impl fmt::Display for RuleResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleResult::Matched { rule_name, answer } => {
                write!(f, "Matched rule '{}': {}", rule_name, answer)
            }
            RuleResult::NoMatch => write!(f, "NoMatch"),
        }
    }
}

/// Pattern matching strategy for a rule.
///
/// Patterns are evaluated case-insensitively against the question text.
/// Multiple pattern types allow flexible matching from simple substring
/// checks to composite patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RulePattern {
    /// Match if the question contains this substring (case-insensitive).
    Contains(String),
    /// Match if the question starts with any of these prefixes (case-insensitive).
    StartsWithAny(Vec<String>),
    /// Match if any of the sub-patterns match (logical OR).
    AnyOf(Vec<RulePattern>),
}

impl RulePattern {
    /// Evaluate this pattern against a question (case-insensitive).
    pub fn matches(&self, question: &str) -> bool {
        let q_lower = question.to_lowercase();
        match self {
            RulePattern::Contains(substring) => {
                q_lower.contains(&substring.to_lowercase())
            }
            RulePattern::StartsWithAny(prefixes) => {
                let q_trimmed = q_lower.trim_start();
                prefixes.iter().any(|p| q_trimmed.starts_with(&p.to_lowercase()))
            }
            RulePattern::AnyOf(patterns) => {
                patterns.iter().any(|p| p.matches(question))
            }
        }
    }
}

/// A single deterministic rule in the rule engine.
///
/// Rules are evaluated in order — the first match wins. This enables
/// priority ordering: more specific rules should come before general ones.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Human-readable name for logging and decision records.
    pub name: String,
    /// The pattern to match against the agent's question.
    pub pattern: RulePattern,
    /// The deterministic response to return when matched.
    pub response: String,
    /// Description of what this rule handles (for documentation).
    pub description: String,
}

/// Deterministic rule engine for the supervisor.
///
/// Evaluates agent questions against a list of pattern-based rules.
/// Rules are checked in order — first match wins. This is the "fast, free,
/// deterministic" first layer of the supervisor's three-tier architecture
/// (rule engine → LLM fallback → human escalation).
///
/// **Extensibility (AC #4):** New rules can be added via `add_rule()` without
/// modifying the tool interface or module structure. Rules are also
/// serializable for potential future config-driven rule loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEngine {
    rules: Vec<Rule>,
}

impl RuleEngine {
    /// Create a new RuleEngine loaded with all built-in rules.
    pub fn new() -> Self {
        Self {
            rules: Self::default_rules(),
        }
    }

    /// Evaluate a question against all rules in priority order.
    /// Returns the first matching rule's answer, or `NoMatch`.
    pub fn evaluate(&self, question: &str) -> RuleResult {
        for rule in &self.rules {
            if rule.pattern.matches(question) {
                return RuleResult::Matched {
                    rule_name: rule.name.clone(),
                    answer: rule.response.clone(),
                };
            }
        }
        RuleResult::NoMatch
    }

    /// Add a rule to the engine. New rules are appended at the end
    /// (lowest priority). Use `insert_rule` for priority placement.
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Insert a rule at a specific position (0-indexed).
    /// Rules before this position have higher priority.
    pub fn insert_rule(&mut self, index: usize, rule: Rule) {
        let idx = index.min(self.rules.len());
        self.rules.insert(idx, rule);
    }

    /// Returns the number of rules in the engine.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Built-in rules covering common BMAD workflow patterns.
    ///
    /// These patterns are derived from the BMAD methodology's interactive
    /// conversation flow. The agent (running dev-story workflow) frequently
    /// asks for confirmation or announces step-by-step plans that should
    /// be short-circuited for autonomous operation.
    ///
    /// **Pattern source:** [project-context.md § Supervisor Hybrid Pattern]
    fn default_rules() -> Vec<Rule> {
        vec![
            // --- Confirmation patterns ---
            // The agent asks for permission to proceed. In autonomous mode,
            // the answer is always "yes" — the story file IS the approval.
            Rule {
                name: "confirmation_proceed".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("should i proceed".to_string()),
                    RulePattern::Contains("shall i continue".to_string()),
                    RulePattern::Contains("do you want me to".to_string()),
                    RulePattern::Contains("ready to proceed".to_string()),
                    RulePattern::Contains("can i go ahead".to_string()),
                    RulePattern::Contains("may i proceed".to_string()),
                    RulePattern::Contains("want me to continue".to_string()),
                    RulePattern::Contains("should i go ahead".to_string()),
                    RulePattern::Contains("shall i proceed".to_string()),
                    RulePattern::Contains("ok to proceed".to_string()),
                ]),
                response: "Yes, proceed.".to_string(),
                description: "Matches agent requests for confirmation to proceed with work.".to_string(),
            },

            // --- Permission request patterns ---
            // The agent asks if it should perform a specific action.
            Rule {
                name: "permission_action".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::StartsWithAny(vec![
                        "should i create".to_string(),
                        "should i modify".to_string(),
                        "should i delete".to_string(),
                        "should i update".to_string(),
                        "should i add".to_string(),
                        "should i remove".to_string(),
                        "should i refactor".to_string(),
                        "should i implement".to_string(),
                        "can i create".to_string(),
                        "can i modify".to_string(),
                        "can i delete".to_string(),
                        "can i update".to_string(),
                    ]),
                ]),
                response: "Yes, proceed with the action as described.".to_string(),
                description: "Matches agent requests for permission to perform specific actions.".to_string(),
            },

            // --- Step-by-step detection ---
            // The agent announces it will explain its plan step-by-step.
            // In autonomous mode, we want execution not explanation.
            Rule {
                name: "step_by_step_detection".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("step by step".to_string()),
                    RulePattern::Contains("step-by-step".to_string()),
                    RulePattern::Contains("let me break this down".to_string()),
                    RulePattern::Contains("here's my plan".to_string()),
                    RulePattern::Contains("here is my plan".to_string()),
                    RulePattern::Contains("i'll outline".to_string()),
                    RulePattern::Contains("let me outline".to_string()),
                    RulePattern::Contains("my approach will be".to_string()),
                ]),
                response: "Skip the step-by-step breakdown. Execute directly using yolo mode.".to_string(),
                description: "Detects agent announcing a step-by-step plan and redirects to direct execution.".to_string(),
            },

            // --- Story selection ---
            // The agent asks which story to work on.
            Rule {
                name: "story_selection".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("which story".to_string()),
                    RulePattern::Contains("what story".to_string()),
                    RulePattern::Contains("next story".to_string()),
                    RulePattern::Contains("what should i work on".to_string()),
                    RulePattern::Contains("which task".to_string()),
                ]),
                response: "The story file has been provided in context. Follow the tasks and acceptance criteria in the story file.".to_string(),
                description: "Matches agent questions about which story or task to work on.".to_string(),
            },

            // --- Progress confirmation ---
            // The agent reports completion of a task or subtask.
            Rule {
                name: "progress_confirmation".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("i've completed".to_string()),
                    RulePattern::Contains("i have completed".to_string()),
                    RulePattern::Contains("i'm done with".to_string()),
                    RulePattern::Contains("i am done with".to_string()),
                    RulePattern::Contains("task complete".to_string()),
                    RulePattern::Contains("finished implementing".to_string()),
                    RulePattern::Contains("implementation complete".to_string()),
                ]),
                response: "Acknowledged. Continue to the next task.".to_string(),
                description: "Matches agent progress reports and acknowledges completion.".to_string(),
            },

            // --- Error/blocker reporting ---
            // The agent reports it's stuck but isn't asking a specific question.
            Rule {
                name: "stuck_general".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("i'm stuck".to_string()),
                    RulePattern::Contains("i am stuck".to_string()),
                    RulePattern::Contains("i can't figure out".to_string()),
                    RulePattern::Contains("i cannot figure out".to_string()),
                    RulePattern::Contains("i'm blocked".to_string()),
                    RulePattern::Contains("i am blocked".to_string()),
                ]),
                response: "Describe the specific problem including error messages and what you've tried, then proceed with your best judgment based on the story specs and project-context.md.".to_string(),
                description: "Matches vague 'I'm stuck' reports and asks for specifics.".to_string(),
            },
        ]
    }
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}
```

### `DecisionRecord` Stub — `src/supervisor/decisions.rs`

```rust
//! Decision logging and traceability for the supervisor.
//!
//! Every supervisor decision (rule engine match, LLM fallback answer,
//! or human escalation) is recorded as a `DecisionRecord`. The full
//! implementation (session accumulation, file writing, PR section
//! generation) is in Story 3.4.

use serde::{Deserialize, Serialize};

/// Source that provided the answer for a supervisor decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionSource {
    /// Answer came from the deterministic rule engine.
    RuleEngine {
        /// Name of the matched rule.
        rule_name: String,
    },
    /// Answer came from the LLM fallback with project context.
    LlmFallback,
    /// Question was escalated to a human.
    HumanEscalation,
}

/// A single supervisor decision record.
///
/// Created every time the supervisor answers a question (or escalates).
/// Accumulated during a session and written to a decisions file at
/// `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md`.
///
/// **Forward-compatibility:** This struct is used by:
/// - Story 3.4: Decision file writing and session accumulation
/// - Epic 5 Story 5.1: PR description "Supervisor Decisions" section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// The agent's original question.
    pub question: String,
    /// The answer provided (or escalation reason).
    pub answer: String,
    /// How the answer was determined.
    pub source: DecisionSource,
    /// Reasoning for why this answer was given.
    pub reasoning: String,
    /// Alternative answers that were considered.
    pub alternatives: Vec<String>,
    /// ISO 8601 timestamp of when the decision was made.
    pub timestamp: String,
}

// TODO: Story 3.4 — Add DecisionLog struct for session accumulation
// TODO: Story 3.4 — Add write_decisions_file() for markdown output
// TODO: Story 3.4 — Add to_pr_section() for PR description inclusion
```

### Tool Definition Description — Design Rationale

The `definition()` description is deliberately detailed because:
1. The LLM agent uses it to decide WHEN to call `ask_supervisor` vs trying to figure things out alone
2. A vague description → agent never calls the tool → questions go unanswered → hallucinated decisions
3. A too-broad description → agent calls it for everything → unnecessary overhead

The description covers 5 explicit use cases to guide the agent's tool selection:
1. Unsure about implementation approach
2. Decision not covered by specs
3. Unexpected situation encountered
4. Need confirmation on technical choice
5. Verify understanding of a requirement

### Integration with Future Stories

**Story 3.2 (LLM Fallback)** will modify `AskSupervisor::call()`:
- Replace `Err(SupervisorError::LlmFallbackNotImplemented)` with actual LLM call
- Add `llm_client` field to `AskSupervisor` struct (or pass via Arc)
- Load project docs (architecture, PRD, project-context) for LLM context
- `AskSupervisor::new()` signature will change to accept LLM config
- **⚠️ Serde note:** The LLM client (reqwest/rig provider) is NOT serializable. The new field must be marked `#[serde(skip)]` since `AskSupervisor` derives `Serialize + Deserialize` for the rig Tool trait. Initialize it separately via a constructor, not via deserialization.

**Story 3.3 (Human Escalation)** will modify `AskSupervisor::call()`:
- After LLM fallback failure, return `Err(SupervisorError::EscalationRequired { .. })`
- Session module (Epic 4) catches this error and marks story `needs-clarification`

**Story 3.4 (Decision Logging)** will:
- Add `DecisionLog` accumulator to `AskSupervisor` (or passed via Arc<Mutex>)
- Record a `DecisionRecord` for every `call()` — both matches and misses
- Write decisions file on session completion or interruption

**Epic 4 (Session)** will:
- Construct `AskSupervisor::new()` and register via `.tool(ask_supervisor)`
- The chat loop ALSO matches confirmation/step-by-step patterns at the session level — ensure consistency with rule engine patterns defined here

### Imports Required in `src/supervisor/mod.rs`

```rust
use rig::tool::Tool;
use rig::completion::ToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub mod rules;
pub mod decisions;

use rules::{RuleEngine, RuleResult};
```

### Imports Required in `src/supervisor/rules.rs`

```rust
use serde::{Deserialize, Serialize};
use std::fmt;
```

### Files Modified/Created in This Story

| File | Change |
|------|--------|
| `src/supervisor/mod.rs` | **REPLACE STUB** — Full implementation: `SupervisorError`, `AskSupervisorArgs`, `AskSupervisor` tool with Tool trait impl, unit tests |
| `src/supervisor/rules.rs` | **CREATE** — Full implementation: `RulePattern`, `Rule`, `RuleEngine`, `RuleResult`, built-in rules, unit tests |
| `src/supervisor/decisions.rs` | **CREATE** — Stub: `DecisionSource`, `DecisionRecord` structs with TODO comments for Story 3.4 |

### Anti-Patterns to Avoid

- ❌ **NO** LLM calls in this story — rule engine is purely deterministic. LLM fallback is Story 3.2
- ❌ **NO** decision file writing — that's Story 3.4. Only define the `DecisionRecord` struct
- ❌ **NO** human escalation logic beyond the `EscalationRequired` error variant — that's Story 3.3
- ❌ **NO** `unwrap()` or `expect()` in production code — use `?` with `SupervisorError`
- ❌ **NO** `anyhow::Result` in supervisor module — typed `SupervisorError` only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** real LLM API calls in tests — mock everything, deterministic only
- ❌ **NO** regex crate dependency in this story — use simple string matching (`Contains`, `StartsWithAny`). Regex support is in `RulePattern` enum for future extensibility but compile-time regex is not needed yet
- ❌ **NO** modifying modules other than `supervisor/mod.rs`, `supervisor/rules.rs`, `supervisor/decisions.rs`
- ❌ **NO** registering the tool with an agent — that's Epic 4
- ❌ **NO** inventing answers for unmatched questions — supervisor must never hallucinate. Return `NoMatch` and let higher layers handle it

### Scope Boundaries

**IN SCOPE for this story:**
- `src/supervisor/mod.rs` — `SupervisorError`, `AskSupervisorArgs`, `AskSupervisor` with Tool trait
- `src/supervisor/rules.rs` — `RulePattern`, `Rule`, `RuleEngine`, `RuleResult`, built-in rules
- `src/supervisor/decisions.rs` — `DecisionSource`, `DecisionRecord` struct stubs

**OUT OF SCOPE — do NOT implement:**
- LLM fallback call with project docs context (Story 3.2)
- Human escalation and story status marking (Story 3.3)
- Decision file writing, session accumulation, PR section (Story 3.4)
- Tool registration with rig agent (Epic 4, Story 4.2)
- Chat loop pattern matching at session level (Epic 4, Story 4.2)
- Notification of escalation to human (Epic 6, Story 6.1)
- Regex compilation or regex crate dependency (future enhancement if needed)

### Testing Requirements

Tests are split between `src/supervisor/mod.rs` and `src/supervisor/rules.rs`:

**In `src/supervisor/rules.rs` — `#[cfg(test)] mod tests`:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // --- RulePattern tests ---

    #[test]
    fn test_pattern_contains_matches_substring() {
        let pattern = RulePattern::Contains("proceed".to_string());
        assert!(pattern.matches("Should I proceed with the task?"));
        assert!(pattern.matches("SHOULD I PROCEED?"));
        assert!(!pattern.matches("What should I do next?"));
    }

    #[test]
    fn test_pattern_contains_case_insensitive() {
        let pattern = RulePattern::Contains("should i proceed".to_string());
        assert!(pattern.matches("Should I Proceed?"));
        assert!(pattern.matches("SHOULD I PROCEED?"));
        assert!(pattern.matches("should i proceed"));
    }

    #[test]
    fn test_pattern_starts_with_any() {
        let pattern = RulePattern::StartsWithAny(vec![
            "should i create".to_string(),
            "should i modify".to_string(),
        ]);
        assert!(pattern.matches("Should I create a new file?"));
        assert!(pattern.matches("should i modify the config?"));
        assert!(!pattern.matches("Can I create something?"));
    }

    #[test]
    fn test_pattern_starts_with_any_trims_leading_whitespace() {
        let pattern = RulePattern::StartsWithAny(vec![
            "should i".to_string(),
        ]);
        assert!(pattern.matches("  Should I proceed?"));
    }

    #[test]
    fn test_pattern_any_of_matches_first_hit() {
        let pattern = RulePattern::AnyOf(vec![
            RulePattern::Contains("alpha".to_string()),
            RulePattern::Contains("beta".to_string()),
        ]);
        assert!(pattern.matches("this has alpha in it"));
        assert!(pattern.matches("this has beta in it"));
        assert!(!pattern.matches("this has gamma in it"));
    }

    // --- RuleEngine tests ---

    #[test]
    fn test_rule_engine_matches_confirmation() {
        let engine = RuleEngine::new();
        let result = engine.evaluate("Should I proceed with the implementation?");
        match result {
            RuleResult::Matched { rule_name, answer } => {
                assert_eq!(rule_name, "confirmation_proceed");
                assert_eq!(answer, "Yes, proceed.");
            }
            RuleResult::NoMatch => panic!("Expected match for confirmation"),
        }
    }

    #[test]
    fn test_rule_engine_matches_confirmation_variants() {
        let engine = RuleEngine::new();
        let confirmations = vec![
            "Shall I continue with the next task?",
            "Do you want me to implement this?",
            "Ready to proceed?",
            "Can I go ahead with the changes?",
            "May I proceed?",
        ];
        for q in confirmations {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "confirmation_proceed", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_matches_step_by_step() {
        let engine = RuleEngine::new();
        let questions = vec![
            "I'll do this step by step",
            "Let me break this down into parts",
            "Here's my plan for implementing this:",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "step_by_step_detection", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_matches_story_selection() {
        let engine = RuleEngine::new();
        let questions = vec![
            "Which story should I work on?",
            "What's the next story?",
            "What should I work on next?",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "story_selection", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_matches_progress_confirmation() {
        let engine = RuleEngine::new();
        let questions = vec![
            "I've completed the unit tests",
            "I'm done with task 3",
            "Task complete — all tests passing",
            "Finished implementing the error handler",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "progress_confirmation", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_matches_permission_requests() {
        let engine = RuleEngine::new();
        let questions = vec![
            "Should I create a new module for this?",
            "Should I modify the existing struct?",
            "Can I delete the unused test file?",
            "Should I update the Cargo.toml?",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "permission_action", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_returns_no_match_for_unknown() {
        let engine = RuleEngine::new();
        let questions = vec![
            "What is the correct database schema for this table?",
            "How should I handle authentication in this endpoint?",
            "The test is failing with error code 42, what does it mean?",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::NoMatch => {} // expected
                RuleResult::Matched { rule_name, .. } => {
                    panic!("Expected NoMatch for '{q}', got match: {rule_name}");
                }
            }
        }
    }

    #[test]
    fn test_rule_engine_case_insensitive() {
        let engine = RuleEngine::new();
        assert!(matches!(
            engine.evaluate("SHOULD I PROCEED?"),
            RuleResult::Matched { .. }
        ));
        assert!(matches!(
            engine.evaluate("should i proceed?"),
            RuleResult::Matched { .. }
        ));
        assert!(matches!(
            engine.evaluate("Should I Proceed?"),
            RuleResult::Matched { .. }
        ));
    }

    #[test]
    fn test_rule_engine_first_match_wins() {
        // Confirmation pattern should match before permission pattern
        // for "Should I proceed?" (contains "should i proceed" AND starts with "should i")
        let engine = RuleEngine::new();
        match engine.evaluate("Should I proceed with creating the file?") {
            RuleResult::Matched { rule_name, .. } => {
                // "should i proceed" is in confirmation_proceed which comes first
                assert_eq!(rule_name, "confirmation_proceed");
            }
            RuleResult::NoMatch => panic!("Expected a match"),
        }
    }

    #[test]
    fn test_rule_engine_add_rule() {
        let mut engine = RuleEngine::new();
        let initial_count = engine.rule_count();

        engine.add_rule(Rule {
            name: "custom_rule".to_string(),
            pattern: RulePattern::Contains("custom pattern".to_string()),
            response: "Custom response".to_string(),
            description: "Test rule".to_string(),
        });

        assert_eq!(engine.rule_count(), initial_count + 1);

        match engine.evaluate("This has a custom pattern in it") {
            RuleResult::Matched { rule_name, answer } => {
                assert_eq!(rule_name, "custom_rule");
                assert_eq!(answer, "Custom response");
            }
            RuleResult::NoMatch => panic!("Expected custom rule to match"),
        }
    }

    #[test]
    fn test_rule_engine_serializable() {
        let engine = RuleEngine::new();
        let json = serde_json::to_string(&engine).expect("Should serialize");
        let deserialized: RuleEngine = serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(deserialized.rule_count(), engine.rule_count());
    }

    #[test]
    fn test_rule_engine_matches_stuck_general() {
        let engine = RuleEngine::new();
        match engine.evaluate("I'm stuck on this implementation") {
            RuleResult::Matched { rule_name, answer } => {
                assert_eq!(rule_name, "stuck_general");
                assert!(answer.contains("specific problem"));
            }
            RuleResult::NoMatch => panic!("Expected match for stuck pattern"),
        }
    }

    #[test]
    fn test_rule_result_display() {
        let matched = RuleResult::Matched {
            rule_name: "test".to_string(),
            answer: "yes".to_string(),
        };
        assert!(matched.to_string().contains("test"));

        let no_match = RuleResult::NoMatch;
        assert_eq!(no_match.to_string(), "NoMatch");
    }
}
```

**In `src/supervisor/mod.rs` — `#[cfg(test)] mod tests`:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ask_supervisor_returns_answer_for_matching_question() {
        let supervisor = AskSupervisor::new();
        let args = AskSupervisorArgs {
            question: "Should I proceed with the implementation?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Yes, proceed.");
    }

    #[tokio::test]
    async fn test_ask_supervisor_returns_error_for_no_match() {
        let supervisor = AskSupervisor::new();
        let args = AskSupervisorArgs {
            question: "What database schema should I use for the users table?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SupervisorError::LlmFallbackNotImplemented => {} // expected
            other => panic!("Expected LlmFallbackNotImplemented, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_ask_supervisor_with_context() {
        let supervisor = AskSupervisor::new();
        let args = AskSupervisorArgs {
            question: "Should I proceed?".to_string(),
            context: Some("Working on task 3 of story 1.2".to_string()),
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ask_supervisor_tool_definition_correct_name() {
        let supervisor = AskSupervisor::new();
        let def = supervisor.definition("test prompt".to_string()).await;
        assert_eq!(def.name, "ask_supervisor");
        assert!(!def.description.is_empty());
        // Verify parameters include "question" as required
        let params = &def.parameters;
        assert!(params["required"].as_array().unwrap()
            .iter().any(|v| v.as_str() == Some("question")));
    }

    #[test]
    fn test_decision_record_serializable() {
        let record = decisions::DecisionRecord {
            question: "Should I proceed?".to_string(),
            answer: "Yes, proceed.".to_string(),
            source: decisions::DecisionSource::RuleEngine {
                rule_name: "confirmation_proceed".to_string(),
            },
            reasoning: "Matched confirmation pattern".to_string(),
            alternatives: vec!["Wait for explicit approval".to_string()],
            timestamp: "2026-02-07T12:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&record).expect("Should serialize");
        let deserialized: decisions::DecisionRecord =
            serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(deserialized.question, "Should I proceed?");
    }

    #[test]
    fn test_supervisor_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SupervisorError>();
    }
}
```

### Project Structure Notes

After this story, the supervisor module structure is:

```
src/supervisor/
├── mod.rs        # AskSupervisor tool (Tool trait), SupervisorError, AskSupervisorArgs
├── rules.rs      # RuleEngine, RulePattern, Rule, RuleResult, built-in rules
└── decisions.rs  # DecisionRecord, DecisionSource (stubs for Story 3.4)
```

Epic 3 progress after this story:
- **Story 3.1:** Supervisor Tool Skeleton & Rule Engine ✓ (this story)
- **Story 3.2:** LLM Fallback with Project Context (next)
- **Story 3.3:** Human Escalation
- **Story 3.4:** Decision Logging & Traceability

The supervisor → session interface is partially defined:
- `AskSupervisor` can be constructed and registered as a rig tool (Epic 4 does registration)
- `SupervisorError::EscalationRequired` stops the rig loop → session module handles it (Epic 4)
- `DecisionRecord` is ready for accumulation and file writing (Story 3.4)

### References

- [Source: epics.md § Story 3.1: Supervisor Tool Skeleton & Rule Engine] — User story, acceptance criteria
- [Source: epics.md § Epic 3: Intelligent Supervision] — Epic context, FR12-FR17
- [Source: prd.md § FR12] — Intercept agent questions during development session
- [Source: prd.md § FR13] — Answer predictable questions via deterministic rule engine
- [Source: architecture.md § Decision 1: Supervisor Interception Model] — Hybrid Chat Loop + Supervisor Tool
- [Source: architecture.md § Rig Tool Implementation Pattern] — Standard structure for all rig tools
- [Source: architecture.md § Test Mock Pattern] — Deterministic LLM responses, Arrange-Act-Assert
- [Source: architecture.md § Error Type Pattern] — Per-module thiserror enums
- [Source: architecture.md § Architectural Boundaries] — session → supervisor via rig tool autonomously
- [Source: project-context.md § Supervisor Hybrid Pattern] — Rule engine patterns: confirmations, step-by-step, story selection
- [Source: project-context.md § Critical Don't-Miss Rules] — Supervisor must never invent answers
- [Source: project-context.md § Multi-Provider LLM Config] — Three LLM roles: dev, review, supervisor
- [Source: project-context.md § Testing Rules] — test_supervisor_handles_confirmation_pattern naming example
- [Source: Story 1.1] — BotConfig with LlmConfig.supervisor, module stubs, rig-core Tool trait reference, Cargo.toml deps
- [Source: rig-core docs] — Tool trait API: NAME, Error, Args, Output, definition(), call(), ToolDefinition, agent.tool() builder

## Dev Agent Record

<!-- This section is filled automatically by the dev agent post-implementation. Do not edit manually. -->

### Agent Model Used

_(filled post-implementation)_

### Debug Log References

### Completion Notes List

### File List