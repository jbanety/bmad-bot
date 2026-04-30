//! Codex provider integration for SDK runtime sessions.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::config::LlmRoleConfig;
use crate::session::SessionOutcome;
use crate::session::state::{PHASE_CREATE, PHASE_DEV, PHASE_REVIEW};
use crate::watcher::StoryInfo;

use super::SdkRuntime;
use super::sdk::{SdkOutputEvent, SdkSessionConfig, SdkSessionResult};

// ---------------------------------------------------------------------------
// Codex NDJSON deserialization types (private)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CodexEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    thread_id: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    usage: Option<CodexUsage>,
    #[serde(default)]
    error: Option<CodexError>,
    #[serde(default)]
    item: Option<CodexItem>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    delta: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexUsage {
    #[allow(dead_code)]
    #[serde(default)]
    input_tokens: u64,
    #[allow(dead_code)]
    #[serde(default)]
    cached_input_tokens: u64,
    #[allow(dead_code)]
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct CodexError {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct CodexItem {
    #[allow(dead_code)]
    #[serde(default)]
    id: String,
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

// ---------------------------------------------------------------------------
// Line parser — public, passed to execute_session()
// ---------------------------------------------------------------------------

pub fn parse_codex_line(line: &str) -> Option<SdkOutputEvent> {
    let event: CodexEvent = serde_json::from_str(line).ok()?;
    match event.event_type.as_str() {
        "thread.started" => {
            let thread_id = event.thread_id?;
            Some(SdkOutputEvent::SessionStarted {
                session_id: thread_id,
            })
        }
        "turn.started" => Some(SdkOutputEvent::Progress {
            message: "Turn started".to_string(),
        }),
        "turn.completed" => None,
        "turn.failed" => {
            let error_msg = event
                .error
                .map(|e| e.message)
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "Turn failed".to_string());
            if error_msg.to_lowercase().contains("rate limit") {
                Some(SdkOutputEvent::RateLimited { resets_at: None })
            } else {
                Some(SdkOutputEvent::Error { message: error_msg })
            }
        }
        "item.started" => {
            let item = event.item?;
            match item.item_type.as_str() {
                "command_execution" => Some(SdkOutputEvent::ToolCall {
                    tool_name: "command_execution".to_string(),
                    detail: item.command.unwrap_or_default(),
                }),
                "file_change" => Some(SdkOutputEvent::ToolCall {
                    tool_name: "file_change".to_string(),
                    detail: String::new(),
                }),
                "mcp_tool_call" => Some(SdkOutputEvent::ToolCall {
                    tool_name: format!(
                        "mcp:{}:{}",
                        item.server.as_deref().unwrap_or("unknown"),
                        item.tool.as_deref().unwrap_or("unknown"),
                    ),
                    detail: String::new(),
                }),
                "web_search" => Some(SdkOutputEvent::ToolCall {
                    tool_name: "web_search".to_string(),
                    detail: item.query.unwrap_or_default(),
                }),
                "reasoning" => Some(SdkOutputEvent::Progress {
                    message: "Reasoning...".to_string(),
                }),
                _ => None,
            }
        }
        "item.completed" => {
            let item = event.item?;
            match item.item_type.as_str() {
                "agent_message" => {
                    let text = item.text.unwrap_or_default();
                    if text.is_empty() {
                        return None;
                    }
                    Some(SdkOutputEvent::Completion {
                        result: text,
                        is_error: false,
                    })
                }
                "command_execution" | "file_change" | "mcp_tool_call" | "web_search" => {
                    Some(SdkOutputEvent::ToolResult {
                        tool_name: item.item_type.clone(),
                        detail: String::new(),
                    })
                }
                "error" => {
                    let msg = item
                        .text
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| "Unknown item error".to_string());
                    Some(SdkOutputEvent::Error { message: msg })
                }
                _ => None,
            }
        }
        "item.updated" => None,
        "item/agentMessage/delta" => {
            let text = event.delta.unwrap_or_default();
            if text.is_empty() {
                None
            } else {
                let truncated = if text.chars().count() > 200 {
                    format!("{}...", text.chars().take(200).collect::<String>())
                } else {
                    text
                };
                Some(SdkOutputEvent::Progress { message: truncated })
            }
        }
        "item/commandExecution/outputDelta" => {
            let text = event.delta.unwrap_or_default();
            if text.is_empty() {
                None
            } else {
                Some(SdkOutputEvent::ToolResult {
                    tool_name: "command_execution".to_string(),
                    detail: if text.len() > 120 { format!("{}...", &text[..117]) } else { text },
                })
            }
        }
        "error" => {
            let msg = event
                .message
                .filter(|m| !m.is_empty())
                .or_else(|| event.error.map(|e| e.message).filter(|m| !m.is_empty()))
                .unwrap_or_else(|| "Unknown error".to_string());
            Some(SdkOutputEvent::Error { message: msg })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Session config builder
// ---------------------------------------------------------------------------

const VALID_CODEX_REASONING: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];

pub fn build_codex_config(
    role_config: &LlmRoleConfig,
    project_root: &Path,
    prompt: &str,
) -> SdkSessionConfig {
    let command = role_config
        .cli_path
        .clone()
        .unwrap_or_else(|| "codex".to_string());

    let mut args = vec![
        "exec".to_string(),
        "--json".to_string(),
        "--dangerously-bypass-approvals-and-sandbox".to_string(),
        "--model".to_string(),
        role_config.model.clone(),
        "--cd".to_string(),
        project_root.to_string_lossy().to_string(),
    ];

    if let Some(ref effort) = role_config.reasoning_effort {
        if VALID_CODEX_REASONING.contains(&effort.as_str()) {
            args.push("--config".to_string());
            args.push(format!("model_reasoning_effort={effort}"));
        } else {
            tracing::warn!(
                effort = %effort,
                "Unrecognized reasoning_effort for Codex, ignoring. Valid: minimal, low, medium, high, xhigh"
            );
        }
    }

    args.push("--".to_string());
    args.push(prompt.to_string());

    SdkSessionConfig {
        command,
        args,
        env: Vec::new(),
        working_directory: project_root.to_path_buf(),
        timeout: Duration::from_secs(30 * 60),
        sigterm_grace: Duration::from_secs(10),
        stdin_data: None,
    }
}

// ---------------------------------------------------------------------------
// Prompt construction per phase
// ---------------------------------------------------------------------------

pub fn build_codex_prompt(phase: &str, story: &StoryInfo) -> String {
    let skill_cmd = match phase {
        PHASE_CREATE => format!("$bmad-create-story {}", story.story_key),
        PHASE_REVIEW => format!("$bmad-code-review {}", story.specs_path.to_string_lossy()),
        _ => {
            if phase != PHASE_DEV {
                tracing::warn!(phase = %phase, "Unknown phase for Codex prompt, defaulting to dev-story");
            }
            format!("$bmad-dev-story {}", story.specs_path.to_string_lossy())
        }
    };
    format!("{skill_cmd}\n\nIMPORTANT: ALL communication and output MUST be in English regardless of any config file settings.")
}

// ---------------------------------------------------------------------------
// MCP config via project-scoped .codex/config.toml
// ---------------------------------------------------------------------------

struct CodexMcpBackup {
    project_root: PathBuf,
    original_content: Option<String>,
}

fn write_codex_mcp_config(
    project_root: &Path,
    mcp_json: &serde_json::Value,
) -> Result<CodexMcpBackup, std::io::Error> {
    let codex_dir = project_root.join(".codex");
    let config_path = codex_dir.join("config.toml");

    let original_content = if config_path.exists() {
        Some(std::fs::read_to_string(&config_path)?)
    } else {
        None
    };

    std::fs::create_dir_all(&codex_dir)?;

    let merged = merge_mcp_into_config(original_content.as_deref(), mcp_json);
    std::fs::write(&config_path, &merged)?;

    Ok(CodexMcpBackup {
        project_root: project_root.to_path_buf(),
        original_content,
    })
}

fn merge_mcp_into_config(existing: Option<&str>, mcp_json: &serde_json::Value) -> String {
    let mut toml = String::new();

    if let Some(content) = existing {
        let cleaned = remove_toml_section(content, "mcp_servers.bmad-supervisor");
        toml.push_str(cleaned.trim_end());
        toml.push_str("\n\n");
    }

    toml.push_str(&generate_mcp_toml_section(mcp_json));
    toml
}

fn remove_toml_section(content: &str, section_prefix: &str) -> String {
    let mut result = String::new();
    let mut skip = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            let header = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
            skip = header == section_prefix || header.starts_with(&format!("{section_prefix}."));
        }
        if !skip {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

fn generate_mcp_toml_section(mcp_json: &serde_json::Value) -> String {
    let mut toml = String::new();
    if let Some(servers) = mcp_json.get("mcpServers").and_then(|s| s.as_object()) {
        for (name, config) in servers {
            toml.push_str(&format!("[mcp_servers.{name}]\n"));
            if let Some(cmd) = config.get("command").and_then(|c| c.as_str()) {
                toml.push_str(&format!("command = {}\n", escape_toml_string(cmd)));
            }
            if let Some(args) = config.get("args").and_then(|a| a.as_array()) {
                let args_str: Vec<String> = args
                    .iter()
                    .filter_map(|a| a.as_str())
                    .map(escape_toml_string)
                    .collect();
                toml.push_str(&format!("args = [{}]\n", args_str.join(", ")));
            }
            toml.push_str("required = true\n");
            if let Some(env) = config.get("env").and_then(|e| e.as_object()) {
                toml.push_str(&format!("\n[mcp_servers.{name}.env]\n"));
                for (key, val) in env {
                    if let Some(v) = val.as_str() {
                        toml.push_str(&format!("{key} = {}\n", escape_toml_string(v)));
                    }
                }
            }
        }
    }
    toml
}

fn escape_toml_string(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn restore_codex_mcp_config(backup: CodexMcpBackup) {
    let config_path = backup.project_root.join(".codex/config.toml");
    match backup.original_content {
        Some(original) => {
            if let Err(e) = std::fs::write(&config_path, original) {
                tracing::warn!(error = %e, "Failed to restore .codex/config.toml");
            }
        }
        None => {
            let _ = std::fs::remove_file(&config_path);
            let _ = std::fs::remove_dir(backup.project_root.join(".codex"));
        }
    }
}

// ---------------------------------------------------------------------------
// Resume config builder
// ---------------------------------------------------------------------------

pub fn build_codex_resume_config(
    role_config: &LlmRoleConfig,
    project_root: &Path,
    session_id: &str,
    prompt: &str,
) -> SdkSessionConfig {
    let command = role_config
        .cli_path
        .clone()
        .unwrap_or_else(|| "codex".to_string());

    let mut args = vec![
        "exec".to_string(),
        "--dangerously-bypass-approvals-and-sandbox".to_string(),
        "resume".to_string(),
        "--json".to_string(),
        session_id.to_string(),
    ];

    args.push("--".to_string());
    args.push(prompt.to_string());

    SdkSessionConfig {
        command,
        args,
        env: Vec::new(),
        working_directory: project_root.to_path_buf(),
        timeout: Duration::from_secs(30 * 60),
        sigterm_grace: Duration::from_secs(10),
        stdin_data: None,
    }
}

// ---------------------------------------------------------------------------
// Orchestration — resume_codex_session
// ---------------------------------------------------------------------------

pub async fn resume_codex_session(
    runtime: &SdkRuntime,
    session_id: &str,
    prompt: &str,
    story: &StoryInfo,
    role: &crate::llm::agent_factory::LlmRole,
) -> (SessionOutcome, Option<SdkSessionResult>) {
    let role_config = runtime.config_for_role(role);

    let project_root = match std::fs::canonicalize(&runtime.config().bmad_paths.project_root) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                error = %e,
                raw = %runtime.config().bmad_paths.project_root,
                "Failed to canonicalize project_root for resume, using raw value"
            );
            PathBuf::from(&runtime.config().bmad_paths.project_root)
        }
    };

    let mcp_json = crate::mcp_server::generate_mcp_config(
        &story.story_key,
        runtime.config_path(),
        runtime.secrets(),
        &runtime.config().mcp_servers,
    );
    let mcp_backup = match write_codex_mcp_config(&project_root, &mcp_json) {
        Ok(backup) => Some(backup),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to write Codex MCP config for resume, proceeding without supervisor");
            None
        }
    };

    let session_config = build_codex_resume_config(role_config, &project_root, session_id, prompt);

    let result = match runtime
        .execute_session(session_config, parse_codex_line)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            if let Some(backup) = mcp_backup {
                restore_codex_mcp_config(backup);
            }
            return (
                SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("SDK resume failed: {e}"),
                    decisions: vec![],
                },
                None,
            );
        }
    };

    if let Some(backup) = mcp_backup {
        restore_codex_mcp_config(backup);
    }

    let impl_artifacts_path = PathBuf::from(&runtime.config().bmad_paths.implementation_artifacts);
    let outcome =
        super::sdk_claude::map_sdk_result_to_outcome(&result, story, &impl_artifacts_path).await;
    (outcome, Some(result))
}

// ---------------------------------------------------------------------------
// Orchestration — run_codex_session
// ---------------------------------------------------------------------------

pub async fn run_codex_session(
    runtime: &SdkRuntime,
    context: super::SessionContext<'_>,
) -> SessionOutcome {
    let role_config = runtime.config_for_role(&context.role);
    let prompt = build_codex_prompt(context.initial_phase, context.story);
    runtime.ui().sdk_session_info(&role_config.provider, &role_config.model);
    runtime.ui().chat_turn(0, &prompt);
    runtime.ui().activation_start();

    let project_root = match std::fs::canonicalize(&runtime.config().bmad_paths.project_root) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                error = %e,
                raw = %runtime.config().bmad_paths.project_root,
                "Failed to canonicalize project_root, using raw value"
            );
            PathBuf::from(&runtime.config().bmad_paths.project_root)
        }
    };

    let needs_supervisor = matches!(context.initial_phase, PHASE_CREATE | PHASE_DEV);
    let mcp_backup: Option<CodexMcpBackup> = if needs_supervisor {
        let mcp_json = crate::mcp_server::generate_mcp_config(
            &context.story.story_key,
            runtime.config_path(),
            runtime.secrets(),
            &runtime.config().mcp_servers,
        );
        match write_codex_mcp_config(&project_root, &mcp_json) {
            Ok(backup) => {
                tracing::info!(
                    "Wrote Codex MCP config — ensure project is trusted (`codex trust .`) for project-scoped config"
                );
                Some(backup)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to write Codex MCP config, proceeding without supervisor");
                None
            }
        }
    } else {
        None
    };

    let session_config = build_codex_config(role_config, &project_root, &prompt);

    let mut result = match runtime
        .execute_session(session_config, parse_codex_line)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            if let Some(backup) = mcp_backup {
                restore_codex_mcp_config(backup);
            }
            return SessionOutcome::Failed {
                story_key: context.story.story_key.clone(),
                error: e.to_string(),
                decisions: vec![],
            };
        }
    };

    // Auto-confirm loop (same logic as claude code)
    let mut confirm_attempts = 0;
    while confirm_attempts < 15 {
        if result.exit_code != Some(0) {
            break;
        }
        let completion = result.completion_text.as_deref().unwrap_or("");
        let response = super::sdk_claude::auto_response_for_prompt(completion);
        let Some(response) = response else {
            break;
        };
        let Some(ref session_id) = result.session_id else {
            break;
        };
        confirm_attempts += 1;
        tracing::info!(
            action = "auto_confirm",
            attempt = confirm_attempts,
            response = %response,
            story_key = %context.story.story_key,
            "Detected interactive prompt — auto-resuming"
        );
        runtime
            .ui()
            .sdk_text(&format!("Auto-responding: {response}"));
        let (_, resume_result) = runtime
            .resume_sdk_session(
                "codex",
                session_id,
                &response,
                context.story,
                &context.role,
            )
            .await;
        match resume_result {
            Some(r) => result = r,
            None => break,
        }
    }

    if let Some(backup) = mcp_backup {
        restore_codex_mcp_config(backup);
    }

    let impl_artifacts_path = PathBuf::from(&runtime.config().bmad_paths.implementation_artifacts);
    let outcome =
        super::sdk_claude::map_sdk_result_to_outcome(&result, context.story, &impl_artifacts_path)
            .await;

    if !context.consultations.is_empty() {
        if let SessionOutcome::Completed { .. } = &outcome {
            let agent_factory = std::sync::Arc::new(crate::llm::AgentFactory::new(
                runtime.config_arc(),
                std::sync::Arc::new(crate::config::BotSecrets {
                    anthropic_api_key: None,
                    openai_api_key: None,
                    github_token: None,
                    gitlab_token: None,
                    telegram_bot_token: None,
                }),
            ));
            let mut consultation_runner = super::sdk_consultation::SdkConsultationRunner::new(
                runtime,
                context.consultations,
            );
            return consultation_runner
                .run_with_consultations(
                    context.story,
                    context.initial_phase,
                    outcome,
                    &result,
                    &context.role,
                    Some(&agent_factory),
                )
                .await;
        }
    }

    outcome
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_test_story() -> StoryInfo {
        StoryInfo {
            story_id: "15.6".to_string(),
            story_key: "15-6-codex-provider".to_string(),
            epic_num: 15,
            story_num: 6,
            label: "codex-provider".to_string(),
            branch_name: "story/15-6-codex-provider".to_string(),
            specs_path: PathBuf::from("/tmp/impl-artifacts/15-6-codex-provider.md"),
            dependencies: vec![],
            status: "in-progress".to_string(),
        }
    }

    fn make_test_role_config() -> LlmRoleConfig {
        LlmRoleConfig {
            provider: "codex".to_string(),
            model: "o4-mini".to_string(),
            reasoning_effort: None,
            base_url: None,
            cli_path: None,
        }
    }

    // -- 9.1: thread.started --
    #[test]
    fn test_parse_codex_thread_started() {
        let line = r#"{"type":"thread.started","thread_id":"abc-123"}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::SessionStarted { session_id } => {
                assert_eq!(session_id, "abc-123");
            }
            _ => panic!("expected SessionStarted"),
        }
    }

    // -- 9.2: thread.started without thread_id --
    #[test]
    fn test_parse_codex_thread_started_no_thread_id() {
        let line = r#"{"type":"thread.started"}"#;
        assert!(parse_codex_line(line).is_none());
    }

    // -- 9.3: turn.started --
    #[test]
    fn test_parse_codex_turn_started() {
        let line = r#"{"type":"turn.started"}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::Progress { message } => {
                assert_eq!(message, "Turn started");
            }
            _ => panic!("expected Progress"),
        }
    }

    // -- 9.4: turn.completed ignored --
    #[test]
    fn test_parse_codex_turn_completed_ignored() {
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":50,"output_tokens":20}}"#;
        assert!(parse_codex_line(line).is_none());
    }

    // -- 9.5: turn.failed --
    #[test]
    fn test_parse_codex_turn_failed_rate_limit() {
        let line = r#"{"type":"turn.failed","error":{"message":"Rate limit exceeded"}}"#;
        let event = parse_codex_line(line).unwrap();
        assert!(matches!(event, SdkOutputEvent::RateLimited { .. }));
    }

    #[test]
    fn test_parse_codex_turn_failed() {
        let line = r#"{"type":"turn.failed","error":{"message":"Internal server error"}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::Error { message } => {
                assert_eq!(message, "Internal server error");
            }
            _ => panic!("expected Error"),
        }
    }

    // -- 9.6: turn.failed no message --
    #[test]
    fn test_parse_codex_turn_failed_no_message() {
        let line = r#"{"type":"turn.failed"}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::Error { message } => {
                assert_eq!(message, "Turn failed");
            }
            _ => panic!("expected Error"),
        }
    }

    // -- 9.7: item.started command_execution --
    #[test]
    fn test_parse_codex_item_started_command_execution() {
        let line = r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"cargo test","status":"in_progress"}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::ToolCall { tool_name, detail } => {
                assert_eq!(tool_name, "command_execution");
                assert_eq!(detail, "cargo test");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    // -- 9.8: item.started file_change --
    #[test]
    fn test_parse_codex_item_started_file_change() {
        let line = r#"{"type":"item.started","item":{"id":"item_2","type":"file_change","status":"in_progress"}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::ToolCall { tool_name, .. } => {
                assert_eq!(tool_name, "file_change");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    // -- 9.9: item.started mcp_tool_call --
    #[test]
    fn test_parse_codex_item_started_mcp_tool_call() {
        let line = r#"{"type":"item.started","item":{"id":"item_3","type":"mcp_tool_call","server":"bmad-supervisor","tool":"ask_supervisor","status":"in_progress"}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::ToolCall { tool_name, .. } => {
                assert_eq!(tool_name, "mcp:bmad-supervisor:ask_supervisor");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    // -- 9.10: item.started web_search --
    #[test]
    fn test_parse_codex_item_started_web_search() {
        let line = r#"{"type":"item.started","item":{"id":"item_4","type":"web_search","query":"rust tokio tutorial","status":"in_progress"}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::ToolCall { tool_name, detail } => {
                assert_eq!(tool_name, "web_search");
                assert_eq!(detail, "rust tokio tutorial");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    // -- 9.11: item.started reasoning --
    #[test]
    fn test_parse_codex_item_started_reasoning() {
        let line = r#"{"type":"item.started","item":{"id":"item_5","type":"reasoning","status":"in_progress"}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::Progress { message } => {
                assert_eq!(message, "Reasoning...");
            }
            _ => panic!("expected Progress"),
        }
    }

    // -- 9.12: item.completed agent_message --
    #[test]
    fn test_parse_codex_item_completed_agent_message() {
        let line = r#"{"type":"item.completed","item":{"id":"item_6","type":"agent_message","text":"All tasks completed successfully."}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::Completion { result, is_error } => {
                assert_eq!(result, "All tasks completed successfully.");
                assert!(!is_error);
            }
            _ => panic!("expected Completion"),
        }
    }

    // -- 9.13: item.completed agent_message empty --
    #[test]
    fn test_parse_codex_item_completed_agent_message_empty() {
        let line =
            r#"{"type":"item.completed","item":{"id":"item_6b","type":"agent_message","text":""}}"#;
        assert!(parse_codex_line(line).is_none());
    }

    // -- 9.14: item.completed command_execution --
    #[test]
    fn test_parse_codex_item_completed_command_execution() {
        let line = r#"{"type":"item.completed","item":{"id":"item_7","type":"command_execution","command":"cargo test","status":"completed"}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::ToolResult { tool_name, .. } => {
                assert_eq!(tool_name, "command_execution");
            }
            _ => panic!("expected ToolResult"),
        }
    }

    // -- 9.15: item.completed error --
    #[test]
    fn test_parse_codex_item_completed_error() {
        let line = r#"{"type":"item.completed","item":{"id":"item_8","type":"error","text":"Permission denied"}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::Error { message } => {
                assert_eq!(message, "Permission denied");
            }
            _ => panic!("expected Error"),
        }
    }

    // -- 9.16: item.updated ignored --
    #[test]
    fn test_parse_codex_item_updated_ignored() {
        let line = r#"{"type":"item.updated","item":{"id":"item_9","type":"agent_message","text":"partial"}}"#;
        assert!(parse_codex_line(line).is_none());
    }

    // -- 9.17: top-level error with message --
    #[test]
    fn test_parse_codex_top_level_error_with_message() {
        let line = r#"{"type":"error","message":"API key invalid"}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::Error { message } => {
                assert_eq!(message, "API key invalid");
            }
            _ => panic!("expected Error"),
        }
    }

    // -- 9.18: top-level error with error field --
    #[test]
    fn test_parse_codex_top_level_error_with_error_field() {
        let line = r#"{"type":"error","error":{"message":"Connection refused"}}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::Error { message } => {
                assert_eq!(message, "Connection refused");
            }
            _ => panic!("expected Error"),
        }
    }

    // -- 9.19: top-level error no details --
    #[test]
    fn test_parse_codex_top_level_error_no_details() {
        let line = r#"{"type":"error"}"#;
        let event = parse_codex_line(line).unwrap();
        match event {
            SdkOutputEvent::Error { message } => {
                assert_eq!(message, "Unknown error");
            }
            _ => panic!("expected Error"),
        }
    }

    // -- 9.20: invalid JSON --
    #[test]
    fn test_parse_codex_invalid_json() {
        assert!(parse_codex_line("not json").is_none());
    }

    // -- 9.21: unknown event type --
    #[test]
    fn test_parse_codex_unknown_event_type() {
        let line = r#"{"type":"unknown.event"}"#;
        assert!(parse_codex_line(line).is_none());
    }

    // -- 9.22: basic config builder --
    #[test]
    fn test_build_codex_config_basic() {
        let role = make_test_role_config();
        let config = build_codex_config(&role, Path::new("/repo"), "test prompt");
        assert_eq!(config.command, "codex");
        assert!(config.args.contains(&"exec".to_string()));
        assert!(config.args.contains(&"--json".to_string()));
        assert!(config
            .args
            .contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
        assert!(config.args.contains(&"--model".to_string()));
        assert!(config.args.contains(&"o4-mini".to_string()));
        assert!(config.args.contains(&"--cd".to_string()));
        assert!(config.args.contains(&"/repo".to_string()));
        assert!(config.args.contains(&"--".to_string()));
        assert!(config.args.contains(&"test prompt".to_string()));
    }

    // -- 9.23: cli_path override --
    #[test]
    fn test_build_codex_config_with_cli_path() {
        let mut role = make_test_role_config();
        role.cli_path = Some("/custom/codex".to_string());
        let config = build_codex_config(&role, Path::new("/repo"), "prompt");
        assert_eq!(config.command, "/custom/codex");
    }

    // -- 9.24: reasoning_effort valid --
    #[test]
    fn test_build_codex_config_with_reasoning_effort() {
        let mut role = make_test_role_config();
        role.reasoning_effort = Some("high".to_string());
        let config = build_codex_config(&role, Path::new("/repo"), "prompt");
        assert!(config.args.contains(&"--config".to_string()));
        assert!(
            config
                .args
                .contains(&"model_reasoning_effort=high".to_string())
        );
    }

    // -- 9.25: reasoning_effort invalid --
    #[test]
    fn test_build_codex_config_invalid_reasoning_effort() {
        let mut role = make_test_role_config();
        role.reasoning_effort = Some("turbo".to_string());
        let config = build_codex_config(&role, Path::new("/repo"), "prompt");
        assert!(!config.args.contains(&"--config".to_string()));
    }

    // -- 9.26: no --mcp-config in args --
    #[test]
    fn test_build_codex_config_no_mcp_in_args() {
        let role = make_test_role_config();
        let config = build_codex_config(&role, Path::new("/repo"), "prompt");
        assert!(!config.args.contains(&"--mcp-config".to_string()));
    }

    // -- 9.27: prompt after separator --
    #[test]
    fn test_build_codex_config_prompt_after_separator() {
        let role = make_test_role_config();
        let config = build_codex_config(&role, Path::new("/repo"), "my prompt");
        let sep_idx = config.args.iter().position(|a| a == "--").unwrap();
        assert_eq!(config.args[sep_idx + 1], "my prompt");
        assert_eq!(sep_idx + 1, config.args.len() - 1);
    }

    // -- 9.28: prompt create phase --
    #[test]
    fn test_build_codex_prompt_create() {
        let story = make_test_story();
        let prompt = build_codex_prompt("create", &story);
        assert!(prompt.starts_with("$bmad-create-story 15-6-codex-provider"), "prompt should start with $skill command: {prompt}");
        assert!(prompt.contains("English"), "prompt should contain English override");
    }

    // -- 9.29: prompt dev phase --
    #[test]
    fn test_build_codex_prompt_dev() {
        let story = make_test_story();
        let prompt = build_codex_prompt("dev", &story);
        assert!(prompt.starts_with("$bmad-dev-story /tmp/impl-artifacts/15-6-codex-provider.md"), "prompt should start with $skill command: {prompt}");
        assert!(prompt.contains("English"), "prompt should contain English override");
    }

    // -- 9.30: prompt review phase --
    #[test]
    fn test_build_codex_prompt_review() {
        let story = make_test_story();
        let prompt = build_codex_prompt("review", &story);
        assert!(prompt.starts_with("$bmad-code-review /tmp/impl-artifacts/15-6-codex-provider.md"), "prompt should start with $skill command: {prompt}");
        assert!(prompt.contains("English"), "prompt should contain English override");
    }

    // -- 9.31: generate_mcp_toml_section --
    #[test]
    fn test_generate_mcp_toml_section() {
        let json = serde_json::json!({
            "mcpServers": {
                "bmad-supervisor": {
                    "command": "bmad-bot",
                    "args": ["mcp-supervisor", "--config", "/path/to/config.yaml", "--story", "15-6"],
                    "env": {
                        "ANTHROPIC_API_KEY": "sk-ant-test",
                        "OPENAI_API_KEY": "sk-oai-test"
                    }
                }
            }
        });
        let toml = generate_mcp_toml_section(&json);
        assert!(toml.contains("[mcp_servers.bmad-supervisor]"));
        assert!(toml.contains("command = \"bmad-bot\""));
        assert!(toml.contains("\"mcp-supervisor\""));
        assert!(toml.contains("required = true"));
        assert!(toml.contains("[mcp_servers.bmad-supervisor.env]"));
        assert!(toml.contains("ANTHROPIC_API_KEY = \"sk-ant-test\""));
        assert!(toml.contains("OPENAI_API_KEY = \"sk-oai-test\""));
    }

    // -- 9.32: special char escaping in TOML --
    #[test]
    fn test_generate_mcp_toml_section_escapes_special_chars() {
        let json = serde_json::json!({
            "mcpServers": {
                "test-server": {
                    "command": "C:\\Program Files\\bmad-bot.exe",
                    "args": ["--name", "say \"hello\""],
                    "env": {}
                }
            }
        });
        let toml = generate_mcp_toml_section(&json);
        assert!(toml.contains(r#"command = "C:\\Program Files\\bmad-bot.exe""#));
        assert!(toml.contains(r#""say \"hello\"""#));
    }

    // -- 9.33: escape_toml_string --
    #[test]
    fn test_escape_toml_string() {
        assert_eq!(escape_toml_string(r"C:\path"), r#""C:\\path""#);
        assert_eq!(escape_toml_string(r#"say "hello""#), r#""say \"hello\"""#);
        assert_eq!(escape_toml_string("simple"), r#""simple""#);
    }

    // -- 9.34: merge with no existing config --
    #[test]
    fn test_merge_mcp_into_config_no_existing() {
        let json = serde_json::json!({
            "mcpServers": {
                "bmad-supervisor": {
                    "command": "bmad-bot",
                    "args": ["mcp-supervisor"],
                    "env": {}
                }
            }
        });
        let result = merge_mcp_into_config(None, &json);
        assert!(result.contains("[mcp_servers.bmad-supervisor]"));
        assert!(result.contains("command = \"bmad-bot\""));
    }

    // -- 9.35: merge preserves user settings --
    #[test]
    fn test_merge_mcp_into_config_preserves_user_settings() {
        let existing = "model = \"o4-mini\"\nsandbox_mode = \"workspace-write\"\n";
        let json = serde_json::json!({
            "mcpServers": {
                "bmad-supervisor": {
                    "command": "bmad-bot",
                    "args": ["mcp-supervisor"],
                    "env": {}
                }
            }
        });
        let result = merge_mcp_into_config(Some(existing), &json);
        assert!(result.contains("model = \"o4-mini\""));
        assert!(result.contains("sandbox_mode = \"workspace-write\""));
        assert!(result.contains("[mcp_servers.bmad-supervisor]"));
    }

    // -- 9.36: merge replaces stale supervisor --
    #[test]
    fn test_merge_mcp_into_config_replaces_stale_supervisor() {
        let existing = "model = \"o4-mini\"\n\n[mcp_servers.bmad-supervisor]\ncommand = \"old-binary\"\nargs = [\"old-arg\"]\nrequired = true\n\n[mcp_servers.bmad-supervisor.env]\nOLD_KEY = \"old-val\"\n";
        let json = serde_json::json!({
            "mcpServers": {
                "bmad-supervisor": {
                    "command": "new-binary",
                    "args": ["new-arg"],
                    "env": {}
                }
            }
        });
        let result = merge_mcp_into_config(Some(existing), &json);
        assert!(result.contains("model = \"o4-mini\""));
        assert!(!result.contains("old-binary"));
        assert!(!result.contains("OLD_KEY"));
        assert!(result.contains("command = \"new-binary\""));
        let count = result.matches("[mcp_servers.bmad-supervisor]").count();
        assert_eq!(count, 1, "should have exactly one supervisor section");
    }

    // -- 9.37: remove_toml_section --
    #[test]
    fn test_remove_toml_section() {
        let content = "[other_section]\nkey = \"val\"\n\n[mcp_servers.bmad-supervisor]\ncommand = \"x\"\n\n[mcp_servers.bmad-supervisor.env]\nFOO = \"bar\"\n\n[another_section]\nfoo = \"baz\"\n";
        let result = remove_toml_section(content, "mcp_servers.bmad-supervisor");
        assert!(result.contains("[other_section]"));
        assert!(result.contains("[another_section]"));
        assert!(!result.contains("[mcp_servers.bmad-supervisor]"));
        assert!(!result.contains("command = \"x\""));
        assert!(!result.contains("FOO = \"bar\""));
    }

    // -- 9.38: write/restore with no existing config --
    #[test]
    fn test_write_restore_codex_mcp_config_no_existing() {
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "mcpServers": {
                "bmad-supervisor": {
                    "command": "bmad-bot",
                    "args": ["mcp-supervisor"],
                    "env": {}
                }
            }
        });
        let backup = write_codex_mcp_config(dir.path(), &json).unwrap();
        let config_path = dir.path().join(".codex/config.toml");
        assert!(config_path.exists());
        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("[mcp_servers.bmad-supervisor]"));

        restore_codex_mcp_config(backup);
        assert!(!config_path.exists());
        assert!(!dir.path().join(".codex").exists());
    }

    // -- 9.39: write/restore with existing config --
    #[test]
    fn test_write_restore_codex_mcp_config_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let original = "model = \"o4-mini\"\nsandbox_mode = \"workspace-write\"\n";
        std::fs::write(codex_dir.join("config.toml"), original).unwrap();

        let json = serde_json::json!({
            "mcpServers": {
                "bmad-supervisor": {
                    "command": "bmad-bot",
                    "args": ["mcp-supervisor"],
                    "env": {}
                }
            }
        });
        let backup = write_codex_mcp_config(dir.path(), &json).unwrap();
        let config_path = codex_dir.join("config.toml");
        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("model = \"o4-mini\""));
        assert!(content.contains("[mcp_servers.bmad-supervisor]"));

        restore_codex_mcp_config(backup);
        let restored = std::fs::read_to_string(&config_path).unwrap();
        assert_eq!(restored, original);
    }

    // -- 9.40: integration test — Codex dispatch --
    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_session_codex_dispatches() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        use crate::config::{BotConfig, BotSecrets};
        use crate::llm::agent_factory::LlmRole;
        use crate::ui::UiHandle;

        let dir = tempfile::tempdir().unwrap();

        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("README.md"), "init").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let script_path = dir.path().join("fake-codex.sh");
        std::fs::write(
            &script_path,
            "#!/bin/sh\necho '{\"type\":\"thread.started\",\"thread_id\":\"test-codex-session-456\"}'\necho '{\"type\":\"item.completed\",\"item\":{\"id\":\"item_1\",\"type\":\"agent_message\",\"text\":\"All done\"}}'\n",
        )
        .unwrap();
        std::fs::set_permissions(&script_path, PermissionsExt::from_mode(0o755)).unwrap();

        let mut config = BotConfig::_test_minimal("pretty", "info");
        config.llm.dev.provider = "codex".to_string();
        config.llm.dev.cli_path = Some(script_path.to_string_lossy().to_string());
        config.bmad_paths.project_root = dir.path().to_string_lossy().to_string();
        config.bmad_paths.implementation_artifacts = dir.path().to_string_lossy().to_string();

        let runtime = SdkRuntime::new(
            Arc::new(config),
            Arc::new(BotSecrets {
                anthropic_api_key: None,
                openai_api_key: Some("sk-test".to_string()),
                github_token: None,
                gitlab_token: None,
                telegram_bot_token: None,
            }),
            PathBuf::from("test-config.yaml"),
            Arc::new(AtomicBool::new(false)),
            UiHandle::null(),
        );

        let story = StoryInfo {
            story_id: "15.6".to_string(),
            story_key: "15-6-codex".to_string(),
            epic_num: 15,
            story_num: 6,
            label: "codex".to_string(),
            branch_name: "story/15-6-codex".to_string(),
            specs_path: dir.path().join("15-6-codex.md"),
            dependencies: vec![],
            status: "in-progress".to_string(),
        };
        let context = super::super::SessionContext {
            story: &story,
            base_branch_override: None,
            consultations: vec![],
            role: LlmRole::Dev,
            initial_phase: "dev",
        };
        let outcome = runtime.run_session(context).await;
        match outcome {
            SessionOutcome::Completed {
                story_key,
                branch,
                pr_context,
                ..
            } => {
                assert_eq!(story_key, "15-6-codex");
                assert_eq!(branch, "story/15-6-codex");
                assert!(pr_context.is_some());
                assert_eq!(pr_context.unwrap(), "All done");
            }
            other => panic!("expected SessionOutcome::Completed, got {:?}", other),
        }
    }

    // -- Story 15.7: resume config builder tests --

    #[test]
    fn test_build_codex_resume_config_basic() {
        let role = make_test_role_config();
        let config = build_codex_resume_config(&role, Path::new("/repo"), "thread-abc", "Continue");
        assert_eq!(config.command, "codex");
        assert!(config.args.contains(&"exec".to_string()));
        assert!(config.args.contains(&"resume".to_string()));
        assert!(config.args.contains(&"thread-abc".to_string()));
        assert!(config.args.contains(&"--json".to_string()));
        assert!(config.args.contains(&"--".to_string()));
        assert!(config.args.contains(&"Continue".to_string()));
        assert!(!config.args.contains(&"--cd".to_string()), "resume should not use --cd");
    }

    #[test]
    fn test_build_codex_resume_config_custom_cli_path() {
        let mut role = make_test_role_config();
        role.cli_path = Some("/custom/codex".to_string());
        let config = build_codex_resume_config(&role, Path::new("/repo"), "thread-abc", "prompt");
        assert_eq!(config.command, "/custom/codex");
    }

    #[test]
    fn test_build_codex_resume_config_args_order() {
        let role = make_test_role_config();
        let config = build_codex_resume_config(&role, Path::new("/repo"), "thread-abc", "Continue");
        let exec_idx = config.args.iter().position(|a| a == "exec").unwrap();
        let resume_idx = config.args.iter().position(|a| a == "resume").unwrap();
        let session_idx = config.args.iter().position(|a| a == "thread-abc").unwrap();
        let separator_idx = config.args.iter().position(|a| a == "--").unwrap();
        assert!(exec_idx < resume_idx);
        assert!(resume_idx < session_idx);
        assert!(session_idx < separator_idx);
    }
}
