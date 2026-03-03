//! Supervisor module — hybrid rule-engine + LLM fallback for agent question handling.
//!
//! The supervisor operates as a rig tool (`ask_supervisor`) that the LLM agent
//! calls autonomously when it encounters questions, doubts, or decision points
//! during dev-story workflow execution.
//!
//! **Processing pipeline (three-tier architecture):**
//! 1. Rule engine (deterministic, free, fast) — matches known patterns
//! 2. LLM fallback (context-aware) — BMAD Architect session with project docs
//! 3. Human escalation — stops agent, notifies human (Story 3.3)

/// BMAD Architect session for supervisor LLM fallback.
pub mod architect;
/// Decision logging and traceability types for supervisor decisions.
pub mod decisions;
/// Deterministic rule engine for pattern-based question matching.
pub mod rules;

use std::sync::{Arc, Mutex};

use architect::{AnswerProvider, ArchitectSession, ArchitectSessionError};
use decisions::{DecisionLog, DecisionRecord, DecisionSource};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use rules::{RuleEngine, RuleResult};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::BotConfig;
use crate::llm::agent_factory::AgentFactory;
use crate::session::escalation::EscalationInfo;

/// Shared escalation slot between the `AskSupervisor` tool and the session chat loop.
///
/// When `AskSupervisor::call()` returns `EscalationRequired`, it writes
/// `Some(EscalationInfo)` into this slot BEFORE returning the error. The session
/// chat loop checks the slot after each `agent.chat()` turn to detect escalation
/// and extract the question/reason context.
pub type EscalationSlot = Arc<Mutex<Option<EscalationInfo>>>;

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
    RuleEngineError {
        /// Description of what went wrong in the rule engine.
        reason: String,
    },

    /// The supervisor cannot answer the question — escalate to human.
    /// When returned from the tool, rig stops the agent loop and the
    /// daemon marks the story as `needs-clarification`.
    /// Implemented fully in Story 3.3.
    #[error("Escalation required for question '{question}': {reason}")]
    EscalationRequired {
        /// The original question that could not be answered.
        question: String,
        /// Why the question requires human intervention.
        reason: String,
    },

    /// LLM fallback is not yet implemented.
    /// Returned when no Architect session is configured and no rule matches.
    #[error("LLM fallback not implemented — no rule matched the question")]
    LlmFallbackNotImplemented,
}

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

/// The `ask_supervisor` rig tool — intercepts agent questions during dev sessions.
///
/// This tool is registered with the rig agent alongside git, filesystem, and
/// terminal tools. The LLM agent calls it autonomously when it encounters
/// questions, doubts, or decision points during the dev-story workflow.
///
/// **Processing pipeline:**
/// 1. Rule engine (deterministic, free) — matches known patterns
/// 2. LLM fallback — BMAD Architect session with project docs
/// 3. Human escalation (Story 3.3) — stops agent, notifies human
///
/// **Architecture Decision 1:** The supervisor is an internal rig tool, not an
/// external interceptor. The daemon's chat loop (Epic 4) handles workflow-level
/// interaction separately.
#[derive(Debug, Serialize, Deserialize)]
pub struct AskSupervisor {
    rule_engine: RuleEngine,
    /// Optional LLM fallback provider (Architect session or mock).
    ///
    /// Skipped during serialization because the provider holds an API key
    /// and non-serializable state. When deserialized, this field is `None`.
    #[serde(skip)]
    answer_provider: Option<Box<dyn AnswerProvider>>,
    /// Shared escalation slot — written by `call()` before returning `EscalationRequired`.
    ///
    /// The session chat loop reads this slot after each `agent.chat()` turn to
    /// detect escalation and extract the question/reason context. Skipped during
    /// serialization because `Arc<Mutex<_>>` is runtime state.
    #[serde(skip)]
    escalation_slot: EscalationSlot,
    /// Thread-safe in-memory log of all supervisor decisions for the current session.
    ///
    /// Every `call()` invocation records a [`DecisionRecord`] before returning.
    /// Skipped during serialization — runtime state only.
    #[serde(skip)]
    decision_log: DecisionLog,
}

impl AskSupervisor {
    /// Create a new AskSupervisor with the default rule engine and **no** Architect session.
    ///
    /// Used in tests and when LLM fallback is not configured. On `NoMatch`,
    /// returns `SupervisorError::LlmFallbackNotImplemented`.
    pub fn new() -> Self {
        Self {
            rule_engine: RuleEngine::new(),
            answer_provider: None,
            escalation_slot: Arc::new(Mutex::new(None)),
            decision_log: DecisionLog::new(),
        }
    }

    /// Create an AskSupervisor with an explicit [`AnswerProvider`].
    ///
    /// Used in tests (with `MockAnswerProvider`) and when you have a
    /// pre-built provider instance.
    pub fn with_answer_provider(provider: Box<dyn AnswerProvider>) -> Self {
        Self {
            rule_engine: RuleEngine::new(),
            answer_provider: Some(provider),
            escalation_slot: Arc::new(Mutex::new(None)),
            decision_log: DecisionLog::new(),
        }
    }

    /// Create an AskSupervisor with an explicit [`AnswerProvider`] and a shared
    /// [`EscalationSlot`].
    ///
    /// The session module passes a clone of its escalation slot so it can detect
    /// when the supervisor signals escalation during a chat turn.
    pub fn with_answer_provider_and_slot(
        provider: Box<dyn AnswerProvider>,
        escalation_slot: EscalationSlot,
    ) -> Self {
        Self {
            rule_engine: RuleEngine::new(),
            answer_provider: Some(provider),
            escalation_slot,
            decision_log: DecisionLog::new(),
        }
    }

    /// Create an AskSupervisor with an explicit [`AnswerProvider`], a shared
    /// [`EscalationSlot`], and a shared [`DecisionLog`].
    ///
    /// This is the full constructor used in production by the session module.
    pub fn with_all(
        provider: Box<dyn AnswerProvider>,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
    ) -> Self {
        Self {
            rule_engine: RuleEngine::new(),
            answer_provider: Some(provider),
            escalation_slot,
            decision_log,
        }
    }

    /// Create an AskSupervisor with an [`ArchitectSession`] built from the daemon config
    /// and a shared [`EscalationSlot`].
    ///
    /// This reads the `architect.md` agent file, resolves the supervisor LLM
    /// provider/model/key, and stores everything for on-demand session creation.
    ///
    /// When `factory` is provided, the [`ArchitectSession`] delegates all agent
    /// construction to the shared [`AgentFactory`] instead of resolving keys
    /// from the environment (legacy path).
    pub fn with_architect_from_config(
        config: &BotConfig,
        factory: Option<Arc<AgentFactory>>,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
        mcp_manager: Arc<crate::mcp::McpManager>,
    ) -> Result<Self, ArchitectSessionError> {
        let session = ArchitectSession::new_with_factory(config, factory, mcp_manager)?;
        Ok(Self {
            rule_engine: RuleEngine::new(),
            answer_provider: Some(Box::new(session)),
            escalation_slot,
            decision_log,
        })
    }

    /// Returns a clone of the escalation slot for sharing with the session chat loop.
    pub fn escalation_slot(&self) -> EscalationSlot {
        Arc::clone(&self.escalation_slot)
    }

    /// Returns a clone of the decision log for sharing with the session module.
    pub fn decision_log(&self) -> DecisionLog {
        self.decision_log.clone()
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
            RuleResult::Matched {
                ref rule_name,
                ref answer,
            } => {
                // Record decision BEFORE returning
                self.decision_log.record(DecisionRecord::new(
                    args.question.clone(),
                    args.context.clone(),
                    answer.clone(),
                    DecisionSource::RuleEngine {
                        rule_name: rule_name.clone(),
                    },
                    format!("Matched deterministic rule pattern: {rule_name}"),
                    vec![],
                ));
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

                // Step 2: Try Architect session (BMAD agent, costs tokens)
                match &self.answer_provider {
                    Some(provider) => {
                        tracing::warn!(
                            action = "supervisor_fallback",
                            question = %args.question,
                            "Launching Architect session for question"
                        );
                        match provider.ask(&args.question, args.context.as_deref()).await {
                            Ok(response) => {
                                // Record decision BEFORE returning
                                self.decision_log.record(DecisionRecord::new(
                                    args.question.clone(),
                                    args.context.clone(),
                                    response.clone(),
                                    DecisionSource::LlmFallback,
                                    "Answered by BMAD Architect agent session with project context"
                                        .to_string(),
                                    vec!["Rule engine had no matching pattern".to_string()],
                                ));
                                tracing::info!(
                                    action = "supervisor_fallback_response",
                                    question = %args.question,
                                    response_len = response.len(),
                                    "Architect session answered successfully"
                                );
                                Ok(response)
                            }
                            Err(e) => {
                                tracing::error!(
                                    action = "supervisor_fallback_failed",
                                    question = %args.question,
                                    error = %e,
                                    "Architect session failed — escalating"
                                );
                                // Step 3: Escalate to human
                                let question = args.question;
                                let reason = format!("Architect session failed: {e}");
                                // Record escalation decision BEFORE setting slot and returning error
                                self.decision_log.record(DecisionRecord::new(
                                    question.clone(),
                                    args.context.clone(),
                                    String::new(),
                                    DecisionSource::Escalation,
                                    format!("Escalated to human: {reason}"),
                                    vec![
                                        "Rule engine had no matching pattern".to_string(),
                                        "Architect session failed or could not answer".to_string(),
                                    ],
                                ));
                                // Write escalation info to shared slot before returning error
                                if let Ok(mut slot) = self.escalation_slot.lock() {
                                    *slot = Some(EscalationInfo {
                                        question: question.clone(),
                                        reason: reason.clone(),
                                    });
                                }
                                Err(SupervisorError::EscalationRequired { question, reason })
                            }
                        }
                    }
                    None => {
                        // No Architect session configured — record escalation and return old error
                        self.decision_log.record(DecisionRecord::new(
                            args.question.clone(),
                            args.context.clone(),
                            String::new(),
                            DecisionSource::Escalation,
                            "No LLM fallback configured — escalated".to_string(),
                            vec!["Rule engine had no matching pattern".to_string()],
                        ));
                        Err(SupervisorError::LlmFallbackNotImplemented)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use architect::MockAnswerProvider;

    // === Story 3.1 tests (backward compatibility) ===

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
    async fn test_ask_supervisor_returns_error_for_no_match_without_architect() {
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
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some("question"))
        );
    }

    #[tokio::test]
    async fn test_ask_supervisor_tool_definition_has_detailed_description() {
        let supervisor = AskSupervisor::new();
        let def = supervisor.definition("test".to_string()).await;
        // Description should be detailed enough for LLM to know when to call it
        assert!(def.description.contains("doubt"));
        assert!(def.description.contains("blocker"));
        assert!(def.description.contains("decision point"));
    }

    #[tokio::test]
    async fn test_ask_supervisor_tool_definition_has_question_and_context_properties() {
        let supervisor = AskSupervisor::new();
        let def = supervisor.definition("test".to_string()).await;
        let props = &def.parameters["properties"];
        assert!(props.get("question").is_some());
        assert!(props.get("context").is_some());
    }

    #[test]
    fn test_decision_record_serializable() {
        let record = decisions::DecisionRecord {
            question: "Should I proceed?".to_string(),
            context: None,
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
        assert_eq!(deserialized.answer, "Yes, proceed.");
    }

    #[test]
    fn test_decision_source_variants_serializable() {
        let sources = vec![
            decisions::DecisionSource::RuleEngine {
                rule_name: "test_rule".to_string(),
            },
            decisions::DecisionSource::LlmFallback,
            decisions::DecisionSource::Escalation,
        ];
        for source in sources {
            let json = serde_json::to_string(&source).expect("Should serialize");
            let _deserialized: decisions::DecisionSource =
                serde_json::from_str(&json).expect("Should deserialize");
        }
    }

    #[test]
    fn test_supervisor_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SupervisorError>();
    }

    #[test]
    fn test_supervisor_error_display() {
        let err = SupervisorError::RuleEngineError {
            reason: "test failure".to_string(),
        };
        assert_eq!(err.to_string(), "Rule engine error: test failure");

        let err = SupervisorError::EscalationRequired {
            question: "How?".to_string(),
            reason: "too complex".to_string(),
        };
        assert!(err.to_string().contains("How?"));
        assert!(err.to_string().contains("too complex"));

        let err = SupervisorError::LlmFallbackNotImplemented;
        assert!(err.to_string().contains("LLM fallback not implemented"));
    }

    #[test]
    fn test_ask_supervisor_default_trait() {
        let supervisor = AskSupervisor::default();
        // Default should produce a working supervisor with rules but no architect
        assert!(supervisor.rule_engine.rule_count() > 0);
        assert!(supervisor.answer_provider.is_none());
    }

    #[test]
    fn test_ask_supervisor_args_deserialize_without_context() {
        let json = r#"{"question": "Should I proceed?"}"#;
        let args: AskSupervisorArgs = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(args.question, "Should I proceed?");
        assert!(args.context.is_none());
    }

    #[test]
    fn test_ask_supervisor_args_deserialize_with_context() {
        let json = r#"{"question": "Should I proceed?", "context": "Working on task 3"}"#;
        let args: AskSupervisorArgs = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(args.question, "Should I proceed?");
        assert_eq!(args.context.unwrap(), "Working on task 3");
    }

    // === Story 3.2 tests — serialization with serde(skip) ===

    #[test]
    fn test_ask_supervisor_serialization_skips_answer_provider() {
        let supervisor = AskSupervisor::new();
        let json = serde_json::to_string(&supervisor).expect("Should serialize");
        assert!(!json.contains("answer_provider"));
        let deserialized: AskSupervisor = serde_json::from_str(&json).expect("Should deserialize");
        // Deserialized supervisor has no answer provider
        assert!(deserialized.answer_provider.is_none());
    }

    #[test]
    fn test_ask_supervisor_with_mock_serialization_skips_provider() {
        let mock = MockAnswerProvider {
            response: "test".to_string(),
            should_fail: false,
        };
        let supervisor = AskSupervisor::with_answer_provider(Box::new(mock));
        let json = serde_json::to_string(&supervisor).expect("Should serialize");
        assert!(!json.contains("answer_provider"));
        assert!(!json.contains("mock"));
        // Deserialized version loses the provider
        let deserialized: AskSupervisor = serde_json::from_str(&json).expect("Should deserialize");
        assert!(deserialized.answer_provider.is_none());
    }

    #[tokio::test]
    async fn test_ask_supervisor_rule_match_bypasses_architect() {
        // Even with an Architect session configured, rule engine match
        // should return immediately without launching a session.
        let mock = MockAnswerProvider {
            response: "Should not be called".to_string(),
            should_fail: false,
        };
        let supervisor = AskSupervisor::with_answer_provider(Box::new(mock));
        let args = AskSupervisorArgs {
            question: "Should I proceed?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Yes, proceed.");
    }

    // === Story 3.2 tests — mock Architect fallback ===

    #[tokio::test]
    async fn test_ask_supervisor_fallback_to_architect_on_no_match() {
        let mock = MockAnswerProvider {
            response: "Use JWT with refresh tokens for authentication.".to_string(),
            should_fail: false,
        };
        let supervisor = AskSupervisor::with_answer_provider(Box::new(mock));
        let args = AskSupervisorArgs {
            question: "What authentication approach should I use?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "Use JWT with refresh tokens for authentication."
        );
    }

    #[tokio::test]
    async fn test_ask_supervisor_fallback_with_context() {
        let mock = MockAnswerProvider {
            response: "Given the JWT context, use refresh token rotation.".to_string(),
            should_fail: false,
        };
        let supervisor = AskSupervisor::with_answer_provider(Box::new(mock));
        let args = AskSupervisorArgs {
            question: "What authentication approach should I use?".to_string(),
            context: Some("Currently using JWT tokens".to_string()),
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("refresh token rotation"));
    }

    #[tokio::test]
    async fn test_ask_supervisor_fallback_failure_escalates() {
        let mock = MockAnswerProvider {
            response: "unused".to_string(),
            should_fail: true,
        };
        let supervisor = AskSupervisor::with_answer_provider(Box::new(mock));
        let args = AskSupervisorArgs {
            question: "What database schema should I use?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SupervisorError::EscalationRequired { question, reason } => {
                assert_eq!(question, "What database schema should I use?");
                assert!(reason.contains("Architect session failed"));
            }
            other => panic!("Expected EscalationRequired, got: {other}"),
        }
    }

    #[test]
    fn test_ask_supervisor_with_answer_provider_has_provider() {
        let mock = MockAnswerProvider {
            response: "test".to_string(),
            should_fail: false,
        };
        let supervisor = AskSupervisor::with_answer_provider(Box::new(mock));
        assert!(supervisor.answer_provider.is_some());
    }

    #[test]
    fn test_ask_supervisor_new_has_no_provider() {
        let supervisor = AskSupervisor::new();
        assert!(supervisor.answer_provider.is_none());
    }

    // === Story 3.3 tests — escalation slot ===

    #[tokio::test]
    async fn test_ask_supervisor_escalation_writes_to_slot() {
        let slot: EscalationSlot = Arc::new(Mutex::new(None));
        let mock = MockAnswerProvider {
            response: "unused".to_string(),
            should_fail: true,
        };
        let supervisor =
            AskSupervisor::with_answer_provider_and_slot(Box::new(mock), Arc::clone(&slot));
        let args = AskSupervisorArgs {
            question: "What database schema should I use?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_err());

        // Verify the slot was written before the error was returned
        let guard = slot.lock().expect("lock");
        let info = guard.as_ref().expect("slot should contain EscalationInfo");
        assert_eq!(info.question, "What database schema should I use?");
        assert!(info.reason.contains("Architect session failed"));
    }

    #[tokio::test]
    async fn test_ask_supervisor_no_escalation_slot_stays_none_on_rule_match() {
        let slot: EscalationSlot = Arc::new(Mutex::new(None));
        let mock = MockAnswerProvider {
            response: "unused".to_string(),
            should_fail: false,
        };
        let supervisor =
            AskSupervisor::with_answer_provider_and_slot(Box::new(mock), Arc::clone(&slot));
        let args = AskSupervisorArgs {
            question: "Should I proceed?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());

        // Slot should remain None — no escalation occurred
        let guard = slot.lock().expect("lock");
        assert!(guard.is_none());
    }

    #[tokio::test]
    async fn test_ask_supervisor_no_escalation_slot_stays_none_on_architect_success() {
        let slot: EscalationSlot = Arc::new(Mutex::new(None));
        let mock = MockAnswerProvider {
            response: "Use PostgreSQL.".to_string(),
            should_fail: false,
        };
        let supervisor =
            AskSupervisor::with_answer_provider_and_slot(Box::new(mock), Arc::clone(&slot));
        let args = AskSupervisorArgs {
            question: "What database should I use?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());

        // Slot should remain None — Architect answered successfully
        let guard = slot.lock().expect("lock");
        assert!(guard.is_none());
    }

    #[test]
    fn test_ask_supervisor_escalation_slot_accessor() {
        let supervisor = AskSupervisor::new();
        let slot = supervisor.escalation_slot();
        // Should be None initially
        let guard = slot.lock().expect("lock");
        assert!(guard.is_none());
    }

    #[test]
    fn test_ask_supervisor_serialization_skips_escalation_slot() {
        let slot: EscalationSlot = Arc::new(Mutex::new(None));
        let mock = MockAnswerProvider {
            response: "test".to_string(),
            should_fail: false,
        };
        let supervisor =
            AskSupervisor::with_answer_provider_and_slot(Box::new(mock), Arc::clone(&slot));
        let json = serde_json::to_string(&supervisor).expect("Should serialize");
        assert!(!json.contains("escalation_slot"));
        // Deserialized version gets a fresh empty slot
        let deserialized: AskSupervisor = serde_json::from_str(&json).expect("Should deserialize");
        let guard = deserialized.escalation_slot.lock().expect("lock");
        assert!(guard.is_none());
    }

    // === Story 3.4 tests — decision logging in call() ===

    #[tokio::test]
    async fn test_call_rule_match_records_rule_engine_decision() {
        let supervisor = AskSupervisor::new();
        let log = supervisor.decision_log();
        assert!(log.is_empty());

        let args = AskSupervisorArgs {
            question: "Should I proceed with the implementation?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());

        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].question,
            "Should I proceed with the implementation?"
        );
        assert_eq!(records[0].answer, "Yes, proceed.");
        assert!(records[0].context.is_none());
        match &records[0].source {
            DecisionSource::RuleEngine { rule_name } => {
                assert!(!rule_name.is_empty());
            }
            other => panic!("Expected RuleEngine source, got: {other}"),
        }
        assert!(
            records[0]
                .reasoning
                .contains("Matched deterministic rule pattern")
        );
        assert!(records[0].alternatives.is_empty());
    }

    #[tokio::test]
    async fn test_call_rule_match_with_context_records_context() {
        let supervisor = AskSupervisor::new();
        let log = supervisor.decision_log();

        let args = AskSupervisorArgs {
            question: "Should I proceed?".to_string(),
            context: Some("Working on task 3".to_string()),
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());

        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].context.as_deref(), Some("Working on task 3"));
    }

    #[tokio::test]
    async fn test_call_llm_success_records_llm_fallback_decision() {
        let mock = MockAnswerProvider {
            response: "Use JWT with refresh tokens.".to_string(),
            should_fail: false,
        };
        let supervisor = AskSupervisor::with_answer_provider(Box::new(mock));
        let log = supervisor.decision_log();

        let args = AskSupervisorArgs {
            question: "What auth approach?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());

        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].question, "What auth approach?");
        assert_eq!(records[0].answer, "Use JWT with refresh tokens.");
        assert_eq!(records[0].source, DecisionSource::LlmFallback);
        assert!(records[0].reasoning.contains("Architect"));
        assert_eq!(
            records[0].alternatives,
            vec!["Rule engine had no matching pattern"]
        );
    }

    #[tokio::test]
    async fn test_call_escalation_records_escalation_decision() {
        let mock = MockAnswerProvider {
            response: "unused".to_string(),
            should_fail: true,
        };
        let supervisor = AskSupervisor::with_answer_provider(Box::new(mock));
        let log = supervisor.decision_log();

        let args = AskSupervisorArgs {
            question: "What DB schema?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_err());

        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].question, "What DB schema?");
        assert!(
            records[0].answer.is_empty(),
            "Escalation should have empty answer"
        );
        assert_eq!(records[0].source, DecisionSource::Escalation);
        assert!(records[0].reasoning.contains("Escalated to human"));
        assert_eq!(records[0].alternatives.len(), 2);
    }

    #[tokio::test]
    async fn test_call_no_architect_records_escalation_decision() {
        let supervisor = AskSupervisor::new(); // no architect configured
        let log = supervisor.decision_log();

        let args = AskSupervisorArgs {
            question: "What database should I use?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_err());

        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].question, "What database should I use?");
        assert!(records[0].answer.is_empty());
        assert_eq!(records[0].source, DecisionSource::Escalation);
        assert!(records[0].reasoning.contains("No LLM fallback configured"));
        assert_eq!(
            records[0].alternatives,
            vec!["Rule engine had no matching pattern"]
        );
    }

    #[tokio::test]
    async fn test_call_multiple_calls_accumulate_decisions() {
        let supervisor = AskSupervisor::new();
        let log = supervisor.decision_log();

        // First call — rule match
        let args1 = AskSupervisorArgs {
            question: "Should I proceed?".to_string(),
            context: None,
        };
        let _ = supervisor.call(args1).await;

        // Second call — no match, escalation (no architect)
        let args2 = AskSupervisorArgs {
            question: "What database?".to_string(),
            context: None,
        };
        let _ = supervisor.call(args2).await;

        let records = log.records();
        assert_eq!(records.len(), 2);
        match &records[0].source {
            DecisionSource::RuleEngine { .. } => {}
            other => panic!("Expected RuleEngine for first call, got: {other}"),
        }
        assert_eq!(records[1].source, DecisionSource::Escalation);
    }

    #[test]
    fn test_decision_log_accessible_via_accessor() {
        let supervisor = AskSupervisor::new();
        let log = supervisor.decision_log();
        assert!(log.is_empty());

        // Record directly to verify it's the same Arc
        log.record(DecisionRecord::new(
            "test".to_string(),
            None,
            "answer".to_string(),
            DecisionSource::LlmFallback,
            "reason".to_string(),
            vec![],
        ));
        assert_eq!(supervisor.decision_log().len(), 1);
    }

    #[test]
    fn test_ask_supervisor_serialization_skips_decision_log() {
        let supervisor = AskSupervisor::new();
        // Add a decision to the log
        supervisor.decision_log.record(DecisionRecord::new(
            "q".to_string(),
            None,
            "a".to_string(),
            DecisionSource::LlmFallback,
            "r".to_string(),
            vec![],
        ));
        assert_eq!(supervisor.decision_log.len(), 1);

        let json = serde_json::to_string(&supervisor).expect("Should serialize");
        assert!(!json.contains("decision_log"));

        // Deserialized version gets a fresh empty log
        let deserialized: AskSupervisor = serde_json::from_str(&json).expect("Should deserialize");
        assert!(deserialized.decision_log.is_empty());
    }

    #[tokio::test]
    async fn test_call_with_all_constructor_records_to_shared_log() {
        let slot: EscalationSlot = Arc::new(Mutex::new(None));
        let log = DecisionLog::new();
        let mock = MockAnswerProvider {
            response: "Use PostgreSQL.".to_string(),
            should_fail: false,
        };
        let supervisor = AskSupervisor::with_all(Box::new(mock), Arc::clone(&slot), log.clone());

        let args = AskSupervisorArgs {
            question: "What database?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());

        // The shared log should have the record
        assert_eq!(log.len(), 1);
        assert_eq!(log.records()[0].answer, "Use PostgreSQL.");
    }
}
