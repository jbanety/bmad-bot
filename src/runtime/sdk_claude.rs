//! Claude Code provider integration for SDK runtime sessions.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::config::LlmRoleConfig;
use crate::session::SessionOutcome;
use crate::session::escalation::EscalationReport;
use crate::session::state::{PHASE_CREATE, PHASE_DEV, PHASE_REVIEW};
use crate::supervisor::decisions::{DecisionRecord, DecisionSource};
use crate::watcher::StoryInfo;

use super::SdkRuntime;
use super::sdk::{SdkOutputEvent, SdkSessionConfig, SdkSessionResult};

// ---------------------------------------------------------------------------
// Claude Code streaming JSON deserialization types (private)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ClaudeCodeEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    message: Option<ClaudeCodeMessage>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: Option<bool>,
    #[serde(default)]
    errors: Option<Vec<String>>,
    #[serde(default)]
    tool_use_result: Option<String>,
    #[serde(default)]
    rate_limit_info: Option<ClaudeCodeRateLimitInfo>,
    #[allow(dead_code)]
    #[serde(default)]
    duration_ms: Option<u64>,
    #[allow(dead_code)]
    #[serde(default)]
    total_cost_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ClaudeCodeRateLimitInfo {
    #[serde(default)]
    status: String,
    #[serde(default, rename = "resetsAt")]
    resets_at: Option<u64>,
    #[serde(default, rename = "rateLimitType")]
    rate_limit_type: Option<String>,
    #[serde(default)]
    utilization: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ClaudeCodeMessage {
    #[serde(default)]
    content: Vec<ClaudeCodeContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClaudeCodeContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        #[allow(dead_code)]
        #[serde(default)]
        id: String,
        #[serde(default)]
        input: Option<serde_json::Value>,
    },
    #[serde(other)]
    Other,
}

fn extract_tool_detail(tool_name: &str, input: Option<&serde_json::Value>) -> String {
    let Some(obj) = input.and_then(|v| v.as_object()) else {
        return String::new();
    };
    match tool_name {
        "Read" | "read_file" => obj
            .get("file_path")
            .or_else(|| obj.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "Edit" | "Write" | "edit_file" => obj
            .get("file_path")
            .or_else(|| obj.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "Bash" => obj
            .get("command")
            .and_then(|v| v.as_str())
            .map(|cmd| {
                if cmd.len() > 80 {
                    format!("{}...", &cmd[..77])
                } else {
                    cmd.to_string()
                }
            })
            .unwrap_or_default(),
        "Grep" | "grep" => obj
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "Glob" | "find_path" => obj
            .get("pattern")
            .or_else(|| obj.get("glob"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "Agent" => obj
            .get("description")
            .or_else(|| obj.get("prompt"))
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.len() > 80 {
                    format!("{}...", &s[..77])
                } else {
                    s.to_string()
                }
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Line parser — public, passed to execute_session()
// ---------------------------------------------------------------------------

pub fn parse_claude_code_line(line: &str) -> Option<SdkOutputEvent> {
    let event: ClaudeCodeEvent = serde_json::from_str(line).ok()?;
    match event.event_type.as_str() {
        "system" => match event.subtype.as_deref() {
            Some("init") => {
                let session_id = event.session_id?;
                Some(SdkOutputEvent::SessionStarted { session_id })
            }
            Some("api_retry") => Some(SdkOutputEvent::Progress {
                message: "API retry in progress".to_string(),
            }),
            _ => None,
        },
        "assistant" => {
            let message = event.message?;
            for block in &message.content {
                if let ClaudeCodeContentBlock::ToolUse { name, input, .. } = block {
                    let detail = extract_tool_detail(name, input.as_ref());
                    return Some(SdkOutputEvent::ToolCall {
                        tool_name: name.clone(),
                        detail,
                    });
                }
            }
            for block in &message.content {
                if let ClaudeCodeContentBlock::Text { text } = block
                    && !text.is_empty()
                {
                    let truncated = if text.chars().count() > 200 {
                        let end: String = text.chars().take(200).collect();
                        format!("{end}...")
                    } else {
                        text.clone()
                    };
                    return Some(SdkOutputEvent::Progress { message: truncated });
                }
            }
            None
        }
        "user" => {
            let detail = event
                .tool_use_result
                .map(|s| if s.len() > 120 { format!("{}...", &s[..117]) } else { s })
                .unwrap_or_default();
            Some(SdkOutputEvent::ToolResult {
                tool_name: String::new(),
                detail,
            })
        }
        "result" => {
            let is_error = event.is_error.unwrap_or(false);
            if is_error {
                let error_msg = event
                    .errors
                    .and_then(|e| e.into_iter().next())
                    .unwrap_or_else(|| "Unknown error".to_string());
                Some(SdkOutputEvent::Error { message: error_msg })
            } else {
                Some(SdkOutputEvent::Completion {
                    result: event.result.unwrap_or_default(),
                    is_error: false,
                })
            }
        }
        "rate_limit_event" => {
            let info = event.rate_limit_info?;
            let pct = info.utilization.map(|u| u * 100.0);
            if info.status == "rejected" {
                Some(SdkOutputEvent::RateLimited {
                    resets_at: info.resets_at,
                })
            } else {
                Some(SdkOutputEvent::RateLimitStatus {
                    resets_at: info.resets_at,
                    limit_type: info.rate_limit_type.unwrap_or_default(),
                    percent_used: pct,
                })
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Session config builder
// ---------------------------------------------------------------------------

pub fn build_claude_code_config(
    role_config: &LlmRoleConfig,
    project_root: &Path,
    prompt: &str,
    mcp_config_path: Option<&Path>,
) -> SdkSessionConfig {
    let command = role_config
        .cli_path
        .clone()
        .unwrap_or_else(|| "claude".to_string());

    let mut args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--verbose".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--model".to_string(),
        role_config.model.clone(),
        "--permission-mode".to_string(),
        "acceptEdits".to_string(),
        "--allowedTools".to_string(),
        "Read,Edit,Write,Bash,Grep,Glob,WebSearch,Agent,Skill,Monitor,ToolSearch".to_string(),
        "--max-turns".to_string(),
        "200".to_string(),
        "--append-system-prompt".to_string(),
        "OVERRIDE: communication_language = English".to_string(),
    ];

    if let Some(mcp_path) = mcp_config_path {
        args.push("--mcp-config".to_string());
        args.push(mcp_path.to_string_lossy().to_string());
    }

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

pub fn build_claude_code_prompt(phase: &str, story: &StoryInfo) -> String {
    match phase {
        PHASE_CREATE => format!("/bmad-create-story {}", story.story_key),
        PHASE_REVIEW => format!("/bmad-code-review {}", story.specs_path.to_string_lossy()),
        _ => {
            if phase != PHASE_DEV {
                tracing::warn!(phase = %phase, "Unknown phase for Claude Code prompt, defaulting to dev-story");
            }
            format!("/bmad-dev-story {}", story.specs_path.to_string_lossy())
        }
    }
}

// ---------------------------------------------------------------------------
// Resume config builder
// ---------------------------------------------------------------------------

pub fn build_claude_code_resume_config(
    role_config: &LlmRoleConfig,
    project_root: &Path,
    session_id: &str,
    prompt: &str,
    mcp_config_path: Option<&Path>,
) -> SdkSessionConfig {
    let command = role_config
        .cli_path
        .clone()
        .unwrap_or_else(|| "claude".to_string());

    let mut args = vec![
        "--resume".to_string(),
        session_id.to_string(),
        "-p".to_string(),
        prompt.to_string(),
        "--verbose".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--model".to_string(),
        role_config.model.clone(),
    ];

    if let Some(mcp_path) = mcp_config_path {
        args.push("--mcp-config".to_string());
        args.push(mcp_path.to_string_lossy().to_string());
    }

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
// Orchestration — resume_claude_code_session
// ---------------------------------------------------------------------------

pub async fn resume_claude_code_session(
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
    let mcp_temp_file = match write_mcp_config_temp_file(&mcp_json) {
        Ok(f) => Some(f),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to write MCP config temp file for resume, proceeding without supervisor");
            None
        }
    };
    let mcp_config_path = mcp_temp_file.as_ref().map(|f| f.path().to_path_buf());

    let session_config = build_claude_code_resume_config(
        role_config,
        &project_root,
        session_id,
        prompt,
        mcp_config_path.as_deref(),
    );

    let result = match runtime
        .execute_session(session_config, parse_claude_code_line)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            drop(mcp_temp_file);
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

    drop(mcp_temp_file);

    let impl_artifacts_path = PathBuf::from(&runtime.config().bmad_paths.implementation_artifacts);
    let outcome = map_sdk_result_to_outcome(&result, story, &impl_artifacts_path).await;
    (outcome, Some(result))
}

// ---------------------------------------------------------------------------
// Orchestration — run_claude_code_session
// ---------------------------------------------------------------------------

pub async fn run_claude_code_session(
    runtime: &SdkRuntime,
    context: super::SessionContext<'_>,
) -> SessionOutcome {
    let role_config = runtime.config_for_role(&context.role);
    let prompt = build_claude_code_prompt(context.initial_phase, context.story);
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
    let mcp_temp_file = if needs_supervisor {
        let mcp_json = crate::mcp_server::generate_mcp_config(
            &context.story.story_key,
            runtime.config_path(),
            runtime.secrets(),
            &runtime.config().mcp_servers,
        );
        match write_mcp_config_temp_file(&mcp_json) {
            Ok(f) => Some(f),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to write MCP config temp file, proceeding without supervisor");
                None
            }
        }
    } else {
        None
    };

    let mcp_config_path = mcp_temp_file.as_ref().map(|f| f.path().to_path_buf());

    let session_config = build_claude_code_config(
        role_config,
        &project_root,
        &prompt,
        mcp_config_path.as_deref(),
    );

    let mut result = match runtime
        .execute_session(session_config, parse_claude_code_line)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return SessionOutcome::Failed {
                story_key: context.story.story_key.clone(),
                error: e.to_string(),
                decisions: vec![],
            };
        }
    };

    // Auto-confirm loop: if the session ended with a [Y]/[N] confirmation prompt,
    // resume with "Y" and repeat until the session completes without a prompt.
    let mut confirm_attempts = 0;
    const MAX_CONFIRM_ATTEMPTS: usize = 5;
    while confirm_attempts < MAX_CONFIRM_ATTEMPTS {
        if result.exit_code != Some(0) {
            break;
        }
        let needs_confirm = result
            .completion_text
            .as_deref()
            .map(|t| is_confirmation_prompt(t))
            .unwrap_or(false);
        if !needs_confirm {
            break;
        }
        let Some(ref session_id) = result.session_id else {
            break;
        };
        confirm_attempts += 1;
        tracing::info!(
            action = "auto_confirm",
            attempt = confirm_attempts,
            story_key = %context.story.story_key,
            "Detected [Y]/[N] confirmation prompt — auto-resuming with Y"
        );
        runtime.ui().sdk_text("Auto-confirming [Y] prompt");
        let (_, resume_result) = runtime
            .resume_sdk_session(
                &role_config.provider,
                session_id,
                "Y",
                context.story,
                &context.role,
            )
            .await;
        match resume_result {
            Some(r) => result = r,
            None => break,
        }
    }

    // Drop temp file after session completes (best-effort cleanup)
    drop(mcp_temp_file);

    let impl_artifacts_path = PathBuf::from(&runtime.config().bmad_paths.implementation_artifacts);
    let outcome = map_sdk_result_to_outcome(&result, context.story, &impl_artifacts_path).await;

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

fn write_mcp_config_temp_file(
    mcp_json: &serde_json::Value,
) -> Result<tempfile::NamedTempFile, std::io::Error> {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new()?;
    serde_json::to_writer(&mut tmp, mcp_json).map_err(std::io::Error::other)?;
    tmp.flush()?;
    Ok(tmp)
}

/// Detect if the completion text ends with a BMAD skill confirmation prompt.
///
/// Matches patterns like `[Y]/[N]`, `[Y] pour confirmer`, `[Y] Yes`, etc.
/// Only considers the last ~500 chars to avoid false positives in large outputs.
pub(crate) fn is_confirmation_prompt(text: &str) -> bool {
    let tail = if text.len() > 500 {
        &text[text.len() - 500..]
    } else {
        text
    };
    let lower = tail.to_lowercase();
    // [Y]/[N] or [Y] / [N] patterns
    if lower.contains("[y]/[n]") || lower.contains("[y] / [n]") {
        return true;
    }
    // [Y] followed by confirming text (but not inside a checklist like "- [x]")
    if let Some(pos) = lower.rfind("[y]") {
        let after = &lower[pos + 3..];
        let trimmed = after.trim_start();
        if trimmed.starts_with("pour")
            || trimmed.starts_with("yes")
            || trimmed.starts_with("oui")
            || trimmed.starts_with("to confirm")
            || trimmed.is_empty()
            || trimmed.starts_with('\n')
        {
            return true;
        }
    }
    false
}

pub(crate) async fn map_sdk_result_to_outcome(
    result: &super::sdk::SdkSessionResult,
    story: &StoryInfo,
    impl_artifacts_path: &Path,
) -> SessionOutcome {
    if let Some(ref text) = result.completion_text {
        tracing::info!(
            action = "sdk_completion",
            story_key = %story.story_key,
            exit_code = ?result.exit_code,
            len = text.len(),
            text = %text,
            "SDK session final completion"
        );
    }

    let decisions = read_decisions_json_sidecar(impl_artifacts_path, &story.story_key).await;

    if let Some(resets_at) = result.rate_limit_resets_at {
        return SessionOutcome::Failed {
            story_key: story.story_key.clone(),
            error: format!("RATE_LIMITED:{resets_at}"),
            decisions,
        };
    }

    if let Some((question, reason)) = detect_escalation(&decisions) {
        return SessionOutcome::Escalated {
            report: EscalationReport::new(
                story.story_key.clone(),
                question,
                reason,
                story.branch_name.clone(),
                "SDK session completed with escalation".to_string(),
            ),
            decisions,
        };
    }

    if result.exit_code == Some(0) {
        let pr_context = result
            .completion_text
            .as_ref()
            .filter(|text| !text.is_empty())
            .map(|text| {
                if text.chars().count() > 2000 {
                    text.chars().take(2000).collect()
                } else {
                    text.clone()
                }
            });

        SessionOutcome::Completed {
            story_key: story.story_key.clone(),
            branch: story.branch_name.clone(),
            decisions,
            pr_context,
            pr_how_to_test: None,
            pr_additional_info: None,
        }
    } else {
        let mut error = format!("SDK session failed (exit code {:?})", result.exit_code);
        if let Some(ref stream_err) = result.stream_error {
            error.push_str(": ");
            error.push_str(stream_err);
        } else if !result.stderr.is_empty() {
            error.push_str(": ");
            error.push_str(&result.stderr);
        }

        if error.to_lowercase().contains("rate limit") && result.rate_limit_resets_at.is_none() {
            return SessionOutcome::Failed {
                story_key: story.story_key.clone(),
                error: "RATE_LIMITED:0".to_string(),
                decisions,
            };
        }

        SessionOutcome::Failed {
            story_key: story.story_key.clone(),
            error,
            decisions,
        }
    }
}

// ---------------------------------------------------------------------------
// JSON sidecar: read decisions written by MCP supervisor process
// ---------------------------------------------------------------------------

pub async fn read_decisions_json_sidecar(
    impl_artifacts_dir: &Path,
    story_key: &str,
) -> Vec<DecisionRecord> {
    let path = impl_artifacts_dir.join(format!("{story_key}-SUPERVISOR-DECISIONS.json"));
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_str(&content) {
        Ok(decisions) => decisions,
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "Failed to parse decisions JSON sidecar, treating as empty"
            );
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Escalation detection from structured decisions
// ---------------------------------------------------------------------------

pub fn detect_escalation(decisions: &[DecisionRecord]) -> Option<(String, String)> {
    for record in decisions {
        if record.source == DecisionSource::Escalation {
            return Some((record.question.clone(), record.reasoning.clone()));
        }
    }
    None
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
            story_id: "15.5".to_string(),
            story_key: "15-5-claude-code".to_string(),
            epic_num: 15,
            story_num: 5,
            label: "claude-code".to_string(),
            branch_name: "story/15-5-claude-code".to_string(),
            specs_path: PathBuf::from("/tmp/impl-artifacts/15-5-claude-code.md"),
            dependencies: vec![],
            status: "in-progress".to_string(),
        }
    }

    fn make_test_role_config() -> LlmRoleConfig {
        LlmRoleConfig {
            provider: "claude-code".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            reasoning_effort: None,
            base_url: None,
            cli_path: None,
        }
    }

    // -- Task 9.1: system/init --
    #[test]
    fn test_parse_claude_code_system_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc-123"}"#;
        let event = parse_claude_code_line(line).unwrap();
        match event {
            SdkOutputEvent::SessionStarted { session_id } => {
                assert_eq!(session_id, "abc-123");
            }
            _ => panic!("expected SessionStarted"),
        }
    }

    // -- Task 9.2: system/init without session_id --
    #[test]
    fn test_parse_claude_code_system_init_no_session_id() {
        let line = r#"{"type":"system","subtype":"init"}"#;
        assert!(parse_claude_code_line(line).is_none());
    }

    // -- Task 9.3: assistant with tool_use --
    #[test]
    fn test_parse_claude_code_assistant_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","id":"tu_01"}]}}"#;
        let event = parse_claude_code_line(line).unwrap();
        match event {
            SdkOutputEvent::ToolCall { tool_name, .. } => {
                assert_eq!(tool_name, "Read");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    // -- Task 9.4: assistant with text only --
    #[test]
    fn test_parse_claude_code_assistant_text_only() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Working on it"}]}}"#;
        let event = parse_claude_code_line(line).unwrap();
        match event {
            SdkOutputEvent::Progress { message } => {
                assert_eq!(message, "Working on it");
            }
            _ => panic!("expected Progress"),
        }
    }

    // -- Task 9.5: user tool result --
    #[test]
    fn test_parse_claude_code_user_tool_result() {
        let line = r#"{"type":"user"}"#;
        let event = parse_claude_code_line(line).unwrap();
        match event {
            SdkOutputEvent::ToolResult { .. } => {}
            _ => panic!("expected ToolResult"),
        }
    }

    // -- Task 9.6: result success --
    #[test]
    fn test_parse_claude_code_result_success() {
        let line = r#"{"type":"result","subtype":"success","result":"Done","is_error":false}"#;
        let event = parse_claude_code_line(line).unwrap();
        match event {
            SdkOutputEvent::Completion { result, is_error } => {
                assert_eq!(result, "Done");
                assert!(!is_error);
            }
            _ => panic!("expected Completion"),
        }
    }

    // -- Task 9.7: result error --
    #[test]
    fn test_parse_claude_code_result_error() {
        let line = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["Max turns"]}"#;
        let event = parse_claude_code_line(line).unwrap();
        match event {
            SdkOutputEvent::Error { message } => {
                assert_eq!(message, "Max turns");
            }
            _ => panic!("expected Error"),
        }
    }

    // -- Task 9.8: api_retry --
    #[test]
    fn test_parse_claude_code_api_retry() {
        let line = r#"{"type":"system","subtype":"api_retry"}"#;
        let event = parse_claude_code_line(line).unwrap();
        match event {
            SdkOutputEvent::Progress { message } => {
                assert!(message.contains("retry"));
            }
            _ => panic!("expected Progress"),
        }
    }

    // -- Task 9.9: stream_event ignored --
    #[test]
    fn test_parse_claude_code_stream_event_ignored() {
        let line = r#"{"type":"stream_event"}"#;
        assert!(parse_claude_code_line(line).is_none());
    }

    // -- Task 9.10: invalid JSON --
    #[test]
    fn test_parse_claude_code_invalid_json() {
        assert!(parse_claude_code_line("not json").is_none());
    }

    // -- Task 9.11: unknown type --
    #[test]
    fn test_parse_claude_code_unknown_type() {
        let line = r#"{"type":"unknown_event"}"#;
        assert!(parse_claude_code_line(line).is_none());
    }

    // -- Task 9.12: basic config builder --
    #[test]
    fn test_build_claude_code_config_basic() {
        let role = make_test_role_config();
        let config = build_claude_code_config(&role, Path::new("/repo"), "test prompt", None);
        assert_eq!(config.command, "claude");
        assert!(config.args.contains(&"-p".to_string()));
        assert!(config.args.contains(&"test prompt".to_string()));
        assert!(config.args.contains(&"--output-format".to_string()));
        assert!(config.args.contains(&"stream-json".to_string()));
        assert!(config.args.contains(&"--model".to_string()));
        assert!(
            config
                .args
                .contains(&"claude-sonnet-4-20250514".to_string())
        );
        assert!(config.args.contains(&"--permission-mode".to_string()));
        assert!(config.args.contains(&"acceptEdits".to_string()));
        assert!(config.args.contains(&"--allowedTools".to_string()));
        assert!(config.args.contains(&"--max-turns".to_string()));
        assert!(config.args.contains(&"200".to_string()));
        assert_eq!(config.working_directory, Path::new("/repo"));
    }

    // -- Task 9.13: cli_path override --
    #[test]
    fn test_build_claude_code_config_with_cli_path() {
        let mut role = make_test_role_config();
        role.cli_path = Some("/custom/claude".to_string());
        let config = build_claude_code_config(&role, Path::new("/repo"), "prompt", None);
        assert_eq!(config.command, "/custom/claude");
    }

    // -- Task 9.14: with MCP config --
    #[test]
    fn test_build_claude_code_config_with_mcp() {
        let role = make_test_role_config();
        let mcp_path = Path::new("/tmp/mcp.json");
        let config = build_claude_code_config(&role, Path::new("/repo"), "prompt", Some(mcp_path));
        assert!(config.args.contains(&"--mcp-config".to_string()));
        assert!(config.args.contains(&"/tmp/mcp.json".to_string()));
    }

    // -- Task 9.15: without MCP config --
    #[test]
    fn test_build_claude_code_config_without_mcp() {
        let role = make_test_role_config();
        let config = build_claude_code_config(&role, Path::new("/repo"), "prompt", None);
        assert!(!config.args.contains(&"--mcp-config".to_string()));
    }

    // -- Task 9.15b: language override --
    #[test]
    fn test_build_claude_code_config_has_language_override() {
        let role = make_test_role_config();
        let config = build_claude_code_config(&role, Path::new("/repo"), "prompt", None);
        assert!(config.args.contains(&"--append-system-prompt".to_string()));
        assert!(
            config
                .args
                .contains(&"OVERRIDE: communication_language = English".to_string())
        );
    }

    // -- Task 9.16: prompt create phase --
    #[test]
    fn test_build_claude_code_prompt_create() {
        let story = make_test_story();
        let prompt = build_claude_code_prompt("create", &story);
        assert_eq!(prompt, "/bmad-create-story 15-5-claude-code");
    }

    // -- Task 9.17: prompt dev phase --
    #[test]
    fn test_build_claude_code_prompt_dev() {
        let story = make_test_story();
        let prompt = build_claude_code_prompt("dev", &story);
        assert_eq!(
            prompt,
            "/bmad-dev-story /tmp/impl-artifacts/15-5-claude-code.md"
        );
    }

    // -- Task 9.18: prompt review phase --
    #[test]
    fn test_build_claude_code_prompt_review() {
        let story = make_test_story();
        let prompt = build_claude_code_prompt("review", &story);
        assert_eq!(
            prompt,
            "/bmad-code-review /tmp/impl-artifacts/15-5-claude-code.md"
        );
    }

    // -- Task 9.22: detect escalation found --
    #[test]
    fn test_detect_escalation_found() {
        let decisions = vec![
            DecisionRecord::new(
                "Proceed?".to_string(),
                None,
                "Yes".to_string(),
                DecisionSource::RuleEngine {
                    rule_name: "confirm".to_string(),
                },
                "matched".to_string(),
                vec![],
            ),
            DecisionRecord::new(
                "What DB schema?".to_string(),
                None,
                String::new(),
                DecisionSource::Escalation,
                "Cannot determine".to_string(),
                vec![],
            ),
        ];
        let result = detect_escalation(&decisions);
        assert!(result.is_some());
        let (question, reason) = result.unwrap();
        assert_eq!(question, "What DB schema?");
        assert_eq!(reason, "Cannot determine");
    }

    // -- Task 9.23: detect escalation not found --
    #[test]
    fn test_detect_escalation_not_found() {
        let decisions = vec![
            DecisionRecord::new(
                "Proceed?".to_string(),
                None,
                "Yes".to_string(),
                DecisionSource::RuleEngine {
                    rule_name: "confirm".to_string(),
                },
                "matched".to_string(),
                vec![],
            ),
            DecisionRecord::new(
                "What approach?".to_string(),
                None,
                "Use X".to_string(),
                DecisionSource::LlmFallback,
                "analysis".to_string(),
                vec![],
            ),
        ];
        assert!(detect_escalation(&decisions).is_none());
    }

    // -- Task 9.24: detect escalation empty decisions --
    #[test]
    fn test_detect_escalation_empty_decisions() {
        assert!(detect_escalation(&[]).is_none());
    }

    // -- Task 9.25: read sidecar missing file --
    #[tokio::test]
    async fn test_read_decisions_json_sidecar_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_decisions_json_sidecar(dir.path(), "nonexistent").await;
        assert!(result.is_empty());
    }

    // -- Task 9.26: read sidecar valid JSON --
    #[tokio::test]
    async fn test_read_decisions_json_sidecar_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let decisions = vec![DecisionRecord::new(
            "question".to_string(),
            None,
            "answer".to_string(),
            DecisionSource::LlmFallback,
            "reasoning".to_string(),
            vec![],
        )];
        let path = dir.path().join("test-story-SUPERVISOR-DECISIONS.json");
        let json = serde_json::to_string(&decisions).unwrap();
        tokio::fs::write(&path, &json).await.unwrap();

        let result = read_decisions_json_sidecar(dir.path(), "test-story").await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].question, "question");
        assert_eq!(result[0].answer, "answer");
    }

    // -- Story 15.7: resume config builder tests --

    #[test]
    fn test_build_claude_code_resume_config_basic() {
        let role = make_test_role_config();
        let config = build_claude_code_resume_config(
            &role,
            Path::new("/repo"),
            "sess-abc",
            "Continue from where you left off",
            None,
        );
        assert_eq!(config.command, "claude");
        assert!(config.args.contains(&"--resume".to_string()));
        assert!(config.args.contains(&"sess-abc".to_string()));
        assert!(config.args.contains(&"-p".to_string()));
        assert!(
            config
                .args
                .contains(&"Continue from where you left off".to_string())
        );
        assert!(config.args.contains(&"--output-format".to_string()));
        assert!(config.args.contains(&"stream-json".to_string()));
        assert_eq!(config.working_directory, Path::new("/repo"));
    }

    #[test]
    fn test_build_claude_code_resume_config_with_mcp() {
        let role = make_test_role_config();
        let mcp_path = Path::new("/tmp/mcp.json");
        let config = build_claude_code_resume_config(
            &role,
            Path::new("/repo"),
            "sess-abc",
            "prompt",
            Some(mcp_path),
        );
        assert!(config.args.contains(&"--mcp-config".to_string()));
        assert!(config.args.contains(&"/tmp/mcp.json".to_string()));
    }

    #[test]
    fn test_build_claude_code_resume_config_without_mcp() {
        let role = make_test_role_config();
        let config =
            build_claude_code_resume_config(&role, Path::new("/repo"), "sess-abc", "prompt", None);
        assert!(!config.args.contains(&"--mcp-config".to_string()));
    }

    #[test]
    fn test_build_claude_code_resume_config_custom_cli_path() {
        let mut role = make_test_role_config();
        role.cli_path = Some("/custom/claude".to_string());
        let config =
            build_claude_code_resume_config(&role, Path::new("/repo"), "sess-abc", "prompt", None);
        assert_eq!(config.command, "/custom/claude");
    }
}
