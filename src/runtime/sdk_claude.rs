//! Claude Code provider integration for SDK runtime sessions.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::config::LlmRoleConfig;
use crate::session::SessionOutcome;
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
        "bypassPermissions".to_string(),
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
// Dumb executors — Phase B: no auto-response, no consultation, no outcome mapping
// ---------------------------------------------------------------------------

/// Execute a fresh Claude Code session. Returns raw subprocess result.
pub async fn execute_claude_start(
    runtime: &SdkRuntime,
    role: &crate::llm::agent_factory::LlmRole,
    phase: &str,
    story_key: &str,
    prompt: &str,
    _skill_path: Option<&str>,
    _preamble: Option<&str>,
    needs_supervisor: bool,
) -> SdkSessionResult {
    let role_config = runtime.config_for_role(role);

    let project_root = match std::fs::canonicalize(&runtime.config().bmad_paths.project_root) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to canonicalize project_root, using raw value");
            PathBuf::from(&runtime.config().bmad_paths.project_root)
        }
    };

    let mcp_temp_file = if needs_supervisor {
        let mcp_json = crate::mcp_server::generate_mcp_config(
            story_key,
            runtime.config_path(),
            runtime.secrets(),
            &runtime.config().mcp_servers,
        );
        match write_mcp_config_temp_file(&mcp_json) {
            Ok(f) => Some(f),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to write MCP config temp file");
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
        prompt,
        mcp_config_path.as_deref(),
    );

    let result = match runtime
        .execute_session(session_config, parse_claude_code_line)
        .await
    {
        Ok(r) => r,
        Err(e) => SdkSessionResult {
            session_id: None,
            exit_code: Some(1),
            stderr: e.to_string(),
            timed_out: false,
            shutdown_requested: false,
            completion_text: None,
            stream_error: Some(e.to_string()),
            rate_limit_resets_at: None,
        },
    };

    tracing::info!(
        action = "execute_claude_start",
        phase = %phase,
        story_key = %story_key,
        exit_code = ?result.exit_code,
        "Claude Code session completed"
    );
    drop(mcp_temp_file);
    result
}

/// Resume a Claude Code session. Returns raw subprocess result.
pub async fn execute_claude_resume(
    runtime: &SdkRuntime,
    role: &crate::llm::agent_factory::LlmRole,
    session_id: &str,
    prompt: &str,
    story_key: &str,
) -> SdkSessionResult {
    let role_config = runtime.config_for_role(role);

    let project_root = match std::fs::canonicalize(&runtime.config().bmad_paths.project_root) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to canonicalize project_root for resume");
            PathBuf::from(&runtime.config().bmad_paths.project_root)
        }
    };

    let mcp_json = crate::mcp_server::generate_mcp_config(
        story_key,
        runtime.config_path(),
        runtime.secrets(),
        &runtime.config().mcp_servers,
    );
    let mcp_temp_file = match write_mcp_config_temp_file(&mcp_json) {
        Ok(f) => Some(f),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to write MCP config temp file for resume");
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
        Err(e) => SdkSessionResult {
            session_id: None,
            exit_code: Some(1),
            stderr: e.to_string(),
            timed_out: false,
            shutdown_requested: false,
            completion_text: None,
            stream_error: Some(e.to_string()),
            rate_limit_resets_at: None,
        },
    };

    drop(mcp_temp_file);
    result
}

// ---------------------------------------------------------------------------
// Orchestration — resume_claude_code_session (legacy, Phase C removal)
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
    let outcome =
        crate::pipeline::outcome::map_sdk_result_to_outcome(&result, story, &impl_artifacts_path)
            .await;
    (outcome, Some(result))
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
        assert!(config.args.contains(&"bypassPermissions".to_string()));
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
