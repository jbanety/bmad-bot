//! MCP Server module — exposes `ask_supervisor` as a stdio-transport MCP server.
//!
//! This module implements the server side of the MCP protocol for SDK-mode sessions
//! (Claude Code, Codex). The supervisor logic is identical to the rig tool
//! (`AskSupervisor`): rule engine → LLM fallback → escalation. Only the transport
//! changes (stdio JSON-RPC instead of in-process rig tool calls).
//!
//! **Architecture:** `src/mcp/` = MCP CLIENT, `src/mcp_server/` = MCP SERVER.

use std::path::Path;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};

use crate::config::BotSecrets;
use crate::supervisor::architect::AnswerProvider;
use crate::supervisor::decisions::{DecisionLog, DecisionRecord, DecisionSource};
use crate::supervisor::rules::{RuleEngine, RuleResult};

/// Parameters for the `ask_supervisor` MCP tool.
#[derive(Debug, serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct AskSupervisorParams {
    #[schemars(description = "The specific question or doubt you need answered.")]
    pub question: String,
    #[schemars(
        description = "Optional additional context: code snippets, error messages, or relevant workflow state."
    )]
    pub context: Option<String>,
}

/// MCP server handler exposing the supervisor's `ask_supervisor` tool via stdio transport.
///
/// Implements the same 3-tier cascade as the rig-based `AskSupervisor` tool:
/// 1. Rule engine (deterministic, free)
/// 2. LLM fallback (`AnswerProvider` / `ArchitectSession`)
/// 3. Escalation (`CallToolResult::error()` with structured JSON)
///
/// `Clone` is required by rmcp (handler shared across requests).
/// `Debug` is required by the `ServerHandler` trait bounds.
#[derive(Clone, Debug)]
pub struct SupervisorMcpServer {
    rule_engine: Arc<RuleEngine>,
    answer_provider: Option<Arc<dyn AnswerProvider>>,
    decision_log: DecisionLog,
    // Read by rmcp proc-macro generated code — invisible to rustc dead_code analysis
    #[allow(dead_code)]
    tool_router: ToolRouter<SupervisorMcpServer>,
}

#[tool_router]
impl SupervisorMcpServer {
    /// Construct a new supervisor MCP server.
    ///
    /// `#[tool_router]` generates `Self::tool_router()` which initializes the
    /// `tool_router` field.
    pub fn create(
        answer_provider: Option<Box<dyn AnswerProvider>>,
        decision_log: DecisionLog,
    ) -> Self {
        Self {
            rule_engine: Arc::new(RuleEngine::new()),
            answer_provider: answer_provider.map(Arc::from),
            decision_log,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Ask the supervisor a question when you encounter a doubt, blocker, decision point, or need clarification during your work."
    )]
    async fn ask_supervisor(
        &self,
        Parameters(params): Parameters<AskSupervisorParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.handle_question(params.question, params.context).await
    }
}

#[tool_handler]
impl ServerHandler for SupervisorMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "bmad-supervisor",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "BMAD Supervisor — answers developer questions using project documentation. \
                 Use the ask_supervisor tool when you encounter doubts, blockers, or decision points."
                    .to_string(),
            )
    }
}

/// Private methods — separate impl block from the `#[tool_router]` block.
impl SupervisorMcpServer {
    async fn handle_question(
        &self,
        question: String,
        context: Option<String>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tracing::info!(action = "mcp_ask_supervisor", question = %question, "MCP supervisor tool invoked");

        // Step 1: Rule engine (deterministic, free)
        let result = self.rule_engine.evaluate(&question);
        match result {
            RuleResult::Matched { rule_name, answer } => {
                self.decision_log.record(DecisionRecord::new(
                    question,
                    context,
                    answer.clone(),
                    DecisionSource::RuleEngine { rule_name },
                    "Matched deterministic rule pattern".to_string(),
                    vec![],
                ));
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::json!({ "answer": answer, "source": "rule_engine" }).to_string(),
                )]))
            }
            RuleResult::NoMatch => {
                // Step 2: LLM fallback
                match &self.answer_provider {
                    Some(provider) => {
                        match provider.ask(&question, context.as_deref()).await {
                            Ok(response) => {
                                self.decision_log.record(DecisionRecord::new(
                                    question,
                                    context,
                                    response.clone(),
                                    DecisionSource::LlmFallback,
                                    "Answered by BMAD Architect agent session".to_string(),
                                    vec!["Rule engine had no matching pattern".to_string()],
                                ));
                                Ok(CallToolResult::success(vec![Content::text(
                                    serde_json::json!({ "answer": response, "source": "llm_fallback" }).to_string(),
                                )]))
                            }
                            Err(e) => {
                                // Step 3: Escalation (LLM failed)
                                let reason = format!("Architect session failed: {e}");
                                self.build_escalation_response(question, context, reason)
                            }
                        }
                    }
                    None => self.build_escalation_response(
                        question,
                        context,
                        "No LLM fallback configured and no rule matched".to_string(),
                    ),
                }
            }
        }
    }

    fn build_escalation_response(
        &self,
        question: String,
        context: Option<String>,
        reason: String,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.decision_log.record(DecisionRecord::new(
            question.clone(),
            context,
            String::new(),
            DecisionSource::Escalation,
            format!("Escalated to human: {reason}"),
            vec!["Rule engine had no matching pattern".to_string()],
        ));
        Ok(CallToolResult::error(vec![Content::text(
            serde_json::json!({
                "answer": "",
                "source": "escalation",
                "escalation": true,
                "question": question,
                "reason": reason,
            })
            .to_string(),
        )]))
    }
}

// ---------------------------------------------------------------------------
// serve_stdio — MCP server entry point
// ---------------------------------------------------------------------------

/// Errors from the MCP server module.
#[derive(Debug, thiserror::Error)]
pub enum McpServerError {
    /// MCP server failed to start on stdio transport.
    #[error("MCP server failed to start: {reason}")]
    ServeFailed {
        /// Description of the serve failure.
        reason: String,
    },
    /// MCP server connection closed unexpectedly.
    #[error("MCP server connection closed: {reason}")]
    ConnectionClosed {
        /// Description of the close reason.
        reason: String,
    },
    /// Configuration error.
    #[error("Config error: {0}")]
    Config(#[from] crate::config::ConfigError),
}

/// Start the MCP server on stdio transport and block until the client disconnects.
pub async fn serve_stdio(server: SupervisorMcpServer) -> Result<(), McpServerError> {
    use rmcp::ServiceExt;
    let service =
        server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| McpServerError::ServeFailed {
                reason: e.to_string(),
            })?;
    service
        .waiting()
        .await
        .map_err(|e| McpServerError::ConnectionClosed {
            reason: e.to_string(),
        })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// MCP config generation for SDK sessions
// ---------------------------------------------------------------------------

/// Generate the MCP server config JSON for SDK sessions.
///
/// This JSON is passed to the SDK CLI (Claude Code / Codex) so it can spawn
/// the `bmad-bot mcp-supervisor` child process with the correct arguments
/// and API keys.
pub fn generate_mcp_supervisor_config(
    story_key: &str,
    config_path: &Path,
    secrets: &BotSecrets,
) -> serde_json::Value {
    generate_mcp_config(story_key, config_path, secrets, &[])
}

/// Generate MCP config JSON for SDK sessions, including both the supervisor
/// and any user-configured MCP servers from `bmad-bot.yaml`.
pub fn generate_mcp_config(
    story_key: &str,
    config_path: &Path,
    secrets: &BotSecrets,
    user_mcp_servers: &[crate::config::McpServerConfig],
) -> serde_json::Value {
    let mut env = serde_json::Map::new();
    if let Some(key) = &secrets.anthropic_api_key {
        env.insert(
            "ANTHROPIC_API_KEY".to_string(),
            serde_json::Value::String(key.clone()),
        );
    }
    if let Some(key) = &secrets.openai_api_key {
        env.insert(
            "OPENAI_API_KEY".to_string(),
            serde_json::Value::String(key.clone()),
        );
    }

    let mut servers = serde_json::Map::new();

    servers.insert(
        "bmad-supervisor".to_string(),
        serde_json::json!({
            "command": std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "bmad-bot".to_string()),
            "args": [
                "mcp-supervisor",
                "--story", story_key,
                "--config", config_path.to_string_lossy()
            ],
            "env": serde_json::Value::Object(env)
        }),
    );

    for mcp in user_mcp_servers {
        if !mcp.enabled {
            continue;
        }
        servers.insert(
            mcp.name.clone(),
            serde_json::json!({
                "command": mcp.command,
                "args": mcp.args,
            }),
        );
    }

    serde_json::json!({ "mcpServers": serde_json::Value::Object(servers) })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::architect::MockAnswerProvider;
    use crate::supervisor::decisions::DecisionSource;

    // --- Task 6.1: Construction without AnswerProvider ---
    #[test]
    fn test_supervisor_mcp_server_construction() {
        let log = DecisionLog::new();
        let server = SupervisorMcpServer::create(None, log);
        assert!(server.answer_provider.is_none());
    }

    // --- Task 6.2: Construction with MockAnswerProvider ---
    #[test]
    fn test_supervisor_mcp_server_with_mock_provider() {
        let log = DecisionLog::new();
        let mock = MockAnswerProvider {
            response: "test".to_string(),
            should_fail: false,
        };
        let server = SupervisorMcpServer::create(Some(Box::new(mock)), log);
        assert!(server.answer_provider.is_some());
    }

    // --- Task 6.3: Rule engine match ---
    #[tokio::test]
    async fn test_handle_question_rule_match() {
        let log = DecisionLog::new();
        let server = SupervisorMcpServer::create(None, log.clone());
        let result = server
            .handle_question("Should I proceed?".to_string(), None)
            .await
            .expect("should succeed");
        assert_ne!(result.is_error, Some(true));
        let text = get_result_text(&result);
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(json["source"], "rule_engine");
        assert!(!json["answer"].as_str().unwrap().is_empty());
    }

    // --- Task 6.4: LLM fallback success ---
    #[tokio::test]
    async fn test_handle_question_llm_fallback_success() {
        let log = DecisionLog::new();
        let mock = MockAnswerProvider {
            response: "Use the existing pattern".to_string(),
            should_fail: false,
        };
        let server = SupervisorMcpServer::create(Some(Box::new(mock)), log.clone());
        let result = server
            .handle_question(
                "What database schema should I use?".to_string(),
                Some("context here".to_string()),
            )
            .await
            .expect("should succeed");
        assert_ne!(result.is_error, Some(true));
        let text = get_result_text(&result);
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(json["source"], "llm_fallback");
        assert!(
            json["answer"]
                .as_str()
                .unwrap()
                .contains("existing pattern")
        );
    }

    // --- Task 6.5: LLM fallback failure escalates ---
    #[tokio::test]
    async fn test_handle_question_llm_fallback_failure_escalates() {
        let log = DecisionLog::new();
        let mock = MockAnswerProvider {
            response: "unused".to_string(),
            should_fail: true,
        };
        let server = SupervisorMcpServer::create(Some(Box::new(mock)), log.clone());
        let result = server
            .handle_question("What database schema should I use?".to_string(), None)
            .await
            .expect("should return Ok with error content");
        assert_eq!(result.is_error, Some(true));
        let text = get_result_text(&result);
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(json["escalation"], true);
        assert_eq!(json["answer"], "");
        assert_eq!(json["source"], "escalation");
    }

    // --- Task 6.6: No provider escalates ---
    #[tokio::test]
    async fn test_handle_question_no_provider_escalates() {
        let log = DecisionLog::new();
        let server = SupervisorMcpServer::create(None, log.clone());
        let result = server
            .handle_question("What database schema should I use?".to_string(), None)
            .await
            .expect("should return Ok with error content");
        assert_eq!(result.is_error, Some(true));
        let text = get_result_text(&result);
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(json["escalation"], true);
        assert_eq!(json["answer"], "");
        assert_eq!(json["source"], "escalation");
    }

    // --- Task 6.7: Decision logging on rule match ---
    #[tokio::test]
    async fn test_decision_logging_on_rule_match() {
        let log = DecisionLog::new();
        let server = SupervisorMcpServer::create(None, log.clone());
        let _ = server
            .handle_question("Should I proceed?".to_string(), None)
            .await;
        let records = log.records();
        assert_eq!(records.len(), 1);
        match &records[0].source {
            DecisionSource::RuleEngine { .. } => {}
            other => panic!("Expected RuleEngine, got: {other}"),
        }
    }

    // --- Task 6.8: Decision logging on LLM fallback ---
    #[tokio::test]
    async fn test_decision_logging_on_llm_fallback() {
        let log = DecisionLog::new();
        let mock = MockAnswerProvider {
            response: "answer".to_string(),
            should_fail: false,
        };
        let server = SupervisorMcpServer::create(Some(Box::new(mock)), log.clone());
        let _ = server
            .handle_question("What database?".to_string(), None)
            .await;
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source, DecisionSource::LlmFallback);
    }

    // --- Task 6.9: Decision logging on escalation ---
    #[tokio::test]
    async fn test_decision_logging_on_escalation() {
        let log = DecisionLog::new();
        let server = SupervisorMcpServer::create(None, log.clone());
        let _ = server
            .handle_question("What database?".to_string(), None)
            .await;
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source, DecisionSource::Escalation);
    }

    // --- Task 6.10: All response paths include "answer" field ---
    #[tokio::test]
    async fn test_escalation_response_includes_answer_field() {
        let log = DecisionLog::new();

        // Rule match
        let server = SupervisorMcpServer::create(None, log.clone());
        let result = server
            .handle_question("Should I proceed?".to_string(), None)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&get_result_text(&result)).unwrap();
        assert!(json.get("answer").is_some(), "rule match missing 'answer'");

        // LLM fallback
        let log2 = DecisionLog::new();
        let mock = MockAnswerProvider {
            response: "yes".to_string(),
            should_fail: false,
        };
        let server2 = SupervisorMcpServer::create(Some(Box::new(mock)), log2);
        let result2 = server2
            .handle_question("What database?".to_string(), None)
            .await
            .unwrap();
        let json2: serde_json::Value = serde_json::from_str(&get_result_text(&result2)).unwrap();
        assert!(
            json2.get("answer").is_some(),
            "LLM fallback missing 'answer'"
        );

        // Escalation
        let log3 = DecisionLog::new();
        let server3 = SupervisorMcpServer::create(None, log3);
        let result3 = server3
            .handle_question("What database?".to_string(), None)
            .await
            .unwrap();
        let json3: serde_json::Value = serde_json::from_str(&get_result_text(&result3)).unwrap();
        assert!(json3.get("answer").is_some(), "escalation missing 'answer'");
    }

    // --- Task 6.11: Config generation structure ---
    #[test]
    fn test_generate_mcp_supervisor_config_structure() {
        let secrets = BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        };
        let config =
            generate_mcp_supervisor_config("15-4-test", Path::new("/tmp/config.yaml"), &secrets);
        assert!(
            config["mcpServers"]["bmad-supervisor"]["command"]
                .as_str()
                .is_some()
        );
        let args = config["mcpServers"]["bmad-supervisor"]["args"]
            .as_array()
            .unwrap();
        assert!(args.iter().any(|a| a == "mcp-supervisor"));
        assert!(args.iter().any(|a| a == "--story"));
        assert!(args.iter().any(|a| a == "15-4-test"));
        assert!(args.iter().any(|a| a == "--config"));
    }

    // --- Task 6.12: Config includes env vars ---
    #[test]
    fn test_generate_mcp_supervisor_config_includes_env_vars() {
        let secrets = BotSecrets {
            anthropic_api_key: Some("sk-anthropic".to_string()),
            openai_api_key: Some("sk-openai".to_string()),
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        };
        let config = generate_mcp_supervisor_config("test", Path::new("config.yaml"), &secrets);
        let env = &config["mcpServers"]["bmad-supervisor"]["env"];
        assert_eq!(env["ANTHROPIC_API_KEY"], "sk-anthropic");
        assert_eq!(env["OPENAI_API_KEY"], "sk-openai");
    }

    // --- Task 6.13: Config with no keys ---
    #[test]
    fn test_generate_mcp_supervisor_config_no_keys() {
        let secrets = BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        };
        let config = generate_mcp_supervisor_config("test", Path::new("config.yaml"), &secrets);
        let env = config["mcpServers"]["bmad-supervisor"]["env"]
            .as_object()
            .unwrap();
        assert!(env.is_empty());
    }

    // --- Task 6.14: ServerInfo ---
    #[test]
    fn test_server_info() {
        let log = DecisionLog::new();
        let server = SupervisorMcpServer::create(None, log);
        let info = server.get_info();
        assert!(info.capabilities.tools.is_some());
        assert_eq!(info.server_info.name, "bmad-supervisor");
    }

    // --- Task 6.15: Params deserialization ---
    #[test]
    fn test_ask_supervisor_params_deserialization() {
        let json_q_only = r#"{"question": "Should I proceed?"}"#;
        let params: AskSupervisorParams =
            serde_json::from_str(json_q_only).expect("Should deserialize");
        assert_eq!(params.question, "Should I proceed?");
        assert!(params.context.is_none());

        let json_with_ctx = r#"{"question": "What db?", "context": "Using PostgreSQL"}"#;
        let params2: AskSupervisorParams =
            serde_json::from_str(json_with_ctx).expect("Should deserialize");
        assert_eq!(params2.question, "What db?");
        assert_eq!(params2.context.unwrap(), "Using PostgreSQL");
    }

    // --- Task 6.16: McpServerError display ---
    #[test]
    fn test_mcp_server_error_display() {
        let err = McpServerError::ServeFailed {
            reason: "bind failed".to_string(),
        };
        assert!(err.to_string().contains("bind failed"));

        let err = McpServerError::ConnectionClosed {
            reason: "EOF".to_string(),
        };
        assert!(err.to_string().contains("EOF"));

        let err = McpServerError::Config(crate::config::ConfigError::MissingField {
            field: "test".to_string(),
        });
        assert!(err.to_string().contains("Config error"));
    }

    // --- Test helper ---
    fn get_result_text(result: &CallToolResult) -> String {
        result
            .content
            .first()
            .and_then(|c| c.as_text())
            .map(|t| t.text.clone())
            .unwrap_or_default()
    }
}
