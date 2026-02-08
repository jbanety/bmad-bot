//! Supervisor module — hybrid rule-engine + LLM fallback for agent question handling.
//!
//! The supervisor operates as a rig tool (`ask_supervisor`) that the LLM agent
//! calls autonomously when it encounters questions, doubts, or decision points
//! during dev-story workflow execution.
//!
//! **Processing pipeline (three-tier architecture):**
//! 1. Rule engine (deterministic, free, fast) — matches known patterns (this story)
//! 2. LLM fallback (context-aware) — loads project docs to answer (Story 3.2)
//! 3. Human escalation — stops agent, notifies human (Story 3.3)

/// Decision logging and traceability types for supervisor decisions.
pub mod decisions;
/// Deterministic rule engine for pattern-based question matching.
pub mod rules;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use rules::{RuleEngine, RuleResult};
use serde::{Deserialize, Serialize};
use serde_json::json;

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
    /// Placeholder for Story 3.2 — replaced by actual LLM call.
    /// For now, returned when no rule matches the question.
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
            RuleResult::Matched {
                ref rule_name,
                ref answer,
            } => {
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
            decisions::DecisionSource::HumanEscalation,
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
        // Default should produce a working supervisor with rules
        assert!(supervisor.rule_engine.rule_count() > 0);
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

    #[test]
    fn test_ask_supervisor_serializable() {
        let supervisor = AskSupervisor::new();
        let json = serde_json::to_string(&supervisor).expect("Should serialize");
        let deserialized: AskSupervisor = serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(
            deserialized.rule_engine.rule_count(),
            supervisor.rule_engine.rule_count()
        );
    }
}
