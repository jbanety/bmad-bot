//! SDK runtime subprocess infrastructure for CLI-based LLM providers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::Instant;

use crate::config::{BotConfig, BotSecrets, LlmRoleConfig};
use crate::llm::agent_factory::LlmRole;
use crate::session::SessionOutcome;
use crate::session::agent::ShutdownFlag;
use crate::ui::UiHandle;

use super::SessionContext;

// ---------------------------------------------------------------------------
// SdkError
// ---------------------------------------------------------------------------

/// Errors that can occur during SDK subprocess management.
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("failed to spawn '{command}': {source}")]
    SpawnFailed {
        command: String,
        source: std::io::Error,
    },

    #[error("SDK session timed out after {elapsed:?}")]
    Timeout { elapsed: Duration },

    #[error("shutdown requested during SDK session")]
    ShutdownRequested,

    #[error("process failed (exit code {exit_code:?}): {stderr}")]
    ProcessFailed {
        exit_code: Option<i32>,
        stderr: String,
    },
}

// ---------------------------------------------------------------------------
// SdkSessionConfig
// ---------------------------------------------------------------------------

/// Configuration for spawning an SDK CLI subprocess.
pub struct SdkSessionConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub working_directory: PathBuf,
    pub timeout: Duration,
    pub sigterm_grace: Duration,
}

// ---------------------------------------------------------------------------
// SdkOutputEvent
// ---------------------------------------------------------------------------

/// Provider-agnostic events parsed from SDK CLI stdout.
#[derive(Debug)]
pub enum SdkOutputEvent {
    SessionStarted { session_id: String },
    Progress { message: String },
    ToolCall { tool_name: String, detail: String },
    ToolResult { tool_name: String, detail: String },
    Completion { result: String, is_error: bool },
    Error { message: String },
}

// ---------------------------------------------------------------------------
// SdkSessionResult
// ---------------------------------------------------------------------------

/// Result of an SDK subprocess session.
pub struct SdkSessionResult {
    pub session_id: Option<String>,
    pub exit_code: Option<i32>,
    pub stderr: String,
    pub timed_out: bool,
    pub shutdown_requested: bool,
    pub completion_text: Option<String>,
    /// Last error captured from the SDK stream (e.g. API errors, result errors).
    pub stream_error: Option<String>,
}

// ---------------------------------------------------------------------------
// SdkRuntime
// ---------------------------------------------------------------------------

/// Manages CLI-based LLM provider subprocesses (Claude Code, Codex).
pub struct SdkRuntime {
    config: Arc<BotConfig>,
    secrets: Arc<BotSecrets>,
    config_path: PathBuf,
    shutdown: ShutdownFlag,
    ui: UiHandle,
}

const STDERR_MAX_BYTES: usize = 1_024 * 1_024;

impl SdkRuntime {
    /// Creates a new SDK runtime with the given configuration and dependencies.
    pub fn new(
        config: Arc<BotConfig>,
        secrets: Arc<BotSecrets>,
        config_path: PathBuf,
        shutdown: ShutdownFlag,
        ui: UiHandle,
    ) -> Self {
        Self {
            config,
            secrets,
            config_path,
            shutdown,
            ui,
        }
    }

    pub(crate) fn config(&self) -> &BotConfig {
        &self.config
    }

    pub(crate) fn secrets(&self) -> &BotSecrets {
        &self.secrets
    }

    pub(crate) fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub(crate) fn shutdown_flag(&self) -> &ShutdownFlag {
        &self.shutdown
    }

    pub(crate) fn config_for_role(&self, role: &LlmRole) -> &LlmRoleConfig {
        let llm = &self.config.llm;
        match role {
            LlmRole::Dev => &llm.dev,
            LlmRole::Review => &llm.review,
            LlmRole::Supervisor => &llm.supervisor,
            LlmRole::EpicReview => {
                if llm.epic_review.provider.is_empty() {
                    &llm.review
                } else {
                    &llm.epic_review
                }
            }
            LlmRole::Critic => {
                if llm.critic.provider.is_empty() {
                    &llm.review
                } else {
                    &llm.critic
                }
            }
        }
    }

    fn merge_env_vars(&self, config_env: &[(String, String)]) -> Vec<(String, String)> {
        let mut map = HashMap::new();
        if let Some(key) = &self.secrets.anthropic_api_key {
            map.insert("ANTHROPIC_API_KEY".to_string(), key.clone());
        }
        if let Some(key) = &self.secrets.openai_api_key {
            map.insert("OPENAI_API_KEY".to_string(), key.clone());
        }
        for (k, v) in config_env {
            map.insert(k.clone(), v.clone());
        }
        map.into_iter().collect()
    }

    fn resolve_provider_for_role(&self, role: &LlmRole) -> String {
        self.config_for_role(role).provider.clone()
    }

    /// Dispatches an SDK session resume to the appropriate provider.
    pub async fn resume_sdk_session(
        &self,
        provider: &str,
        session_id: &str,
        prompt: &str,
        story: &crate::watcher::StoryInfo,
        role: &LlmRole,
    ) -> (SessionOutcome, Option<SdkSessionResult>) {
        match provider {
            "claude-code" => {
                super::sdk_claude::resume_claude_code_session(self, session_id, prompt, story, role)
                    .await
            }
            "codex" => {
                super::sdk_codex::resume_codex_session(self, session_id, prompt, story, role).await
            }
            other => (
                SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("SDK provider '{}' does not support resume.", other),
                    decisions: vec![],
                },
                None,
            ),
        }
    }

    /// Dispatches an SDK session to the appropriate provider.
    pub async fn run_session(&self, context: SessionContext<'_>) -> SessionOutcome {
        let provider = self.resolve_provider_for_role(&context.role);
        match provider.as_str() {
            "claude-code" => super::sdk_claude::run_claude_code_session(self, context).await,
            "codex" => super::sdk_codex::run_codex_session(self, context).await,
            other => SessionOutcome::Failed {
                story_key: context.story.story_key.clone(),
                error: format!("SDK provider '{}' not implemented.", other),
                decisions: vec![],
            },
        }
    }

    /// Spawns a CLI subprocess, streams its stdout through `parser`, and returns the result.
    pub async fn execute_session<F>(
        &self,
        session_config: SdkSessionConfig,
        parser: F,
    ) -> Result<SdkSessionResult, SdkError>
    where
        F: Fn(&str) -> Option<SdkOutputEvent> + Send,
    {
        let merged_env = self.merge_env_vars(&session_config.env);

        tracing::info!(
            command = %session_config.command,
            args = ?session_config.args,
            cwd = %session_config.working_directory.display(),
            "Spawning SDK subprocess"
        );

        let mut child = Command::new(&session_config.command)
            .args(&session_config.args)
            .envs(merged_env)
            .current_dir(&session_config.working_directory)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| SdkError::SpawnFailed {
                command: session_config.command.clone(),
                source: e,
            })?;

        let stdout = child.stdout.take().ok_or_else(|| SdkError::SpawnFailed {
            command: session_config.command.clone(),
            source: std::io::Error::other("stdout not piped"),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| SdkError::SpawnFailed {
            command: session_config.command.clone(),
            source: std::io::Error::other("stderr not piped"),
        })?;

        let stderr_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            let mut captured = String::new();
            let mut truncated = false;
            while let Some(line) = reader.next_line().await.unwrap_or(None) {
                tracing::warn!(sdk_stderr = %line);
                if !truncated {
                    if captured.len() + line.len() + 1 > STDERR_MAX_BYTES {
                        captured.push_str("\n...[stderr truncated at 1 MB]");
                        truncated = true;
                    } else {
                        if !captured.is_empty() {
                            captured.push('\n');
                        }
                        captured.push_str(&line);
                    }
                }
            }
            captured
        });

        let mut reader = BufReader::new(stdout).lines();
        let timeout_at = Instant::now() + session_config.timeout;
        let shutdown_poll = Duration::from_millis(200);
        let mut session_id: Option<String> = None;
        let mut timed_out = false;
        let mut shutdown_requested = false;
        let mut last_completion_text: Option<String> = None;
        let mut stream_error: Option<String> = None;

        loop {
            tokio::select! {
                biased;

                _ = tokio::time::sleep_until(timeout_at) => {
                    tracing::warn!("SDK session timeout exceeded");
                    if let Err(e) = graceful_kill(&mut child, session_config.sigterm_grace).await {
                        tracing::error!(error = %e, "graceful_kill failed during timeout");
                    }
                    timed_out = true;
                    break;
                }

                _ = tokio::time::sleep(shutdown_poll) => {
                    if self.shutdown.load(Ordering::Relaxed) {
                        tracing::info!("Shutdown requested during SDK session");
                        if let Err(e) = graceful_kill(&mut child, session_config.sigterm_grace).await {
                            tracing::error!(error = %e, "graceful_kill failed during shutdown");
                        }
                        shutdown_requested = true;
                        break;
                    }
                    continue;
                }

                line = reader.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            if let Some(event) = parser(&line) {
                                self.emit_ui_event(&event);
                                match &event {
                                    SdkOutputEvent::SessionStarted { session_id: id } if session_id.is_none() => {
                                        tracing::info!(session_id = %id, "SDK session ID captured");
                                        session_id = Some(id.clone());
                                    }
                                    SdkOutputEvent::Completion { result, .. } => {
                                        last_completion_text = Some(result.clone());
                                    }
                                    SdkOutputEvent::Error { message } => {
                                        if stream_error.is_none() || message != "Unknown error" {
                                            stream_error = Some(message.clone());
                                        }
                                    }
                                    SdkOutputEvent::Progress { message } if message.contains("API Error") || message.contains("Error:") => {
                                        if stream_error.is_none() {
                                            stream_error = Some(message.clone());
                                        }
                                    }
                                    _ => {}
                                }
                            } else {
                                tracing::debug!(sdk_stdout_unrecognized = %line);
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            tracing::warn!(error = %e, "stdout read error");
                            break;
                        }
                    }
                }
            }
        }

        let remaining = timeout_at.saturating_duration_since(Instant::now());
        let wait_timeout = remaining.max(Duration::from_secs(10));
        let status = match tokio::time::timeout(wait_timeout, child.wait()).await {
            Ok(result) => result.map_err(|e| SdkError::SpawnFailed {
                command: session_config.command.clone(),
                source: e,
            })?,
            Err(_) => {
                tracing::warn!("child.wait() timed out, sending SIGKILL");
                child.kill().await.ok();
                timed_out = true;
                child.wait().await.map_err(|e| SdkError::SpawnFailed {
                    command: session_config.command.clone(),
                    source: e,
                })?
            }
        };
        let stderr_output = stderr_handle.await.unwrap_or_default();

        Ok(SdkSessionResult {
            session_id,
            exit_code: status.code(),
            stderr: stderr_output,
            timed_out,
            shutdown_requested,
            completion_text: last_completion_text,
            stream_error,
        })
    }

    fn emit_ui_event(&self, event: &SdkOutputEvent) {
        match event {
            SdkOutputEvent::SessionStarted { .. } => self.ui.activation_start(),
            SdkOutputEvent::ToolCall {
                tool_name, detail, ..
            } => self.ui.tool_call(tool_name, detail),
            SdkOutputEvent::ToolResult {
                tool_name, detail, ..
            } => self.ui.tool_result(tool_name, detail),
            SdkOutputEvent::Progress { message } => {
                tracing::info!(sdk_progress = %message);
            }
            SdkOutputEvent::Completion { .. } => self.ui.activation_complete(),
            SdkOutputEvent::Error { message } => {
                tracing::error!(sdk_error = %message);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Graceful shutdown helpers
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn send_sigterm(child: &Child) -> std::io::Result<()> {
    match child.id() {
        Some(pid) => {
            // SAFETY: POSIX kill() to a known child PID. ESRCH handled below.
            let pid = libc::pid_t::try_from(pid).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "pid exceeds i32::MAX")
            })?;
            let ret = unsafe { libc::kill(pid, libc::SIGTERM) };
            if ret == 0 {
                Ok(())
            } else {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::ESRCH) {
                    Ok(())
                } else {
                    Err(err)
                }
            }
        }
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn send_sigterm(_child: &Child) -> std::io::Result<()> {
    Ok(())
}

async fn graceful_kill(child: &mut Child, sigterm_grace: Duration) -> Result<(), std::io::Error> {
    if let Err(e) = send_sigterm(child) {
        tracing::warn!(error = %e, "failed to send SIGTERM, falling back to SIGKILL");
    }

    match tokio::time::timeout(sigterm_grace, child.wait()).await {
        Ok(_) => Ok(()),
        Err(_) => {
            tracing::warn!("SIGTERM timeout expired, sending SIGKILL");
            child.kill().await
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::AtomicBool;

    fn make_test_secrets(anthropic: Option<&str>, openai: Option<&str>) -> Arc<BotSecrets> {
        Arc::new(BotSecrets {
            anthropic_api_key: anthropic.map(|s| s.to_string()),
            openai_api_key: openai.map(|s| s.to_string()),
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        })
    }

    fn make_test_config() -> Arc<BotConfig> {
        Arc::new(BotConfig::_test_minimal("pretty", "info"))
    }

    fn make_test_runtime(secrets: Arc<BotSecrets>) -> SdkRuntime {
        SdkRuntime::new(
            make_test_config(),
            secrets,
            PathBuf::from("test-config.yaml"),
            Arc::new(AtomicBool::new(false)),
            UiHandle::null(),
        )
    }

    fn make_session_config(command: &str, args: &[&str]) -> SdkSessionConfig {
        SdkSessionConfig {
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env: vec![],
            working_directory: std::env::temp_dir(),
            timeout: Duration::from_secs(10),
            sigterm_grace: Duration::from_secs(2),
        }
    }

    // -- Task 1 tests: types --

    #[test]
    fn test_sdk_session_config_default_values() {
        let config = SdkSessionConfig {
            command: "claude".to_string(),
            args: vec!["--json".to_string()],
            env: vec![("FOO".to_string(), "bar".to_string())],
            working_directory: PathBuf::from("/tmp"),
            timeout: Duration::from_secs(1800),
            sigterm_grace: Duration::from_secs(10),
        };
        assert_eq!(config.command, "claude");
        assert_eq!(config.args, vec!["--json"]);
        assert_eq!(config.env, vec![("FOO".to_string(), "bar".to_string())]);
        assert_eq!(config.working_directory, PathBuf::from("/tmp"));
        assert_eq!(config.timeout, Duration::from_secs(1800));
        assert_eq!(config.sigterm_grace, Duration::from_secs(10));
    }

    #[test]
    fn test_sdk_output_event_variants() {
        let events: Vec<SdkOutputEvent> = vec![
            SdkOutputEvent::SessionStarted {
                session_id: "abc".to_string(),
            },
            SdkOutputEvent::Progress {
                message: "working".to_string(),
            },
            SdkOutputEvent::ToolCall {
                tool_name: "read".to_string(),
                detail: "file.rs".to_string(),
            },
            SdkOutputEvent::ToolResult {
                tool_name: "read".to_string(),
                detail: "ok".to_string(),
            },
            SdkOutputEvent::Completion {
                result: "done".to_string(),
                is_error: false,
            },
            SdkOutputEvent::Error {
                message: "failed".to_string(),
            },
        ];
        for event in &events {
            let debug = format!("{:?}", event);
            assert!(!debug.is_empty());
        }
    }

    #[test]
    fn test_sdk_error_display() {
        let err = SdkError::Timeout {
            elapsed: Duration::from_secs(30),
        };
        assert!(err.to_string().contains("timed out"));

        let err = SdkError::ShutdownRequested;
        assert!(err.to_string().contains("shutdown"));

        let err = SdkError::ProcessFailed {
            exit_code: Some(1),
            stderr: "error output".to_string(),
        };
        assert!(err.to_string().contains("exit code"));

        let err = SdkError::SpawnFailed {
            command: "claude".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        assert!(err.to_string().contains("claude"));
    }

    // -- Task 2 tests: merge_env_vars --

    #[test]
    fn test_merge_env_vars_both_keys() {
        let runtime = make_test_runtime(make_test_secrets(Some("sk-ant"), Some("sk-oai")));
        let merged = runtime.merge_env_vars(&[]);
        let map: HashMap<String, String> = merged.into_iter().collect();
        assert_eq!(map.get("ANTHROPIC_API_KEY").unwrap(), "sk-ant");
        assert_eq!(map.get("OPENAI_API_KEY").unwrap(), "sk-oai");
    }

    #[test]
    fn test_merge_env_vars_anthropic_only() {
        let runtime = make_test_runtime(make_test_secrets(Some("sk-ant"), None));
        let merged = runtime.merge_env_vars(&[]);
        let map: HashMap<String, String> = merged.into_iter().collect();
        assert_eq!(map.get("ANTHROPIC_API_KEY").unwrap(), "sk-ant");
        assert!(!map.contains_key("OPENAI_API_KEY"));
    }

    #[test]
    fn test_merge_env_vars_no_keys() {
        let runtime = make_test_runtime(make_test_secrets(None, None));
        let merged = runtime.merge_env_vars(&[]);
        assert!(merged.is_empty());
    }

    #[test]
    fn test_merge_env_vars_merge_precedence() {
        let runtime = make_test_runtime(make_test_secrets(Some("sk-ant-secret"), None));
        let config_env = vec![("ANTHROPIC_API_KEY".to_string(), "sk-ant-config".to_string())];
        let merged = runtime.merge_env_vars(&config_env);
        let map: HashMap<String, String> = merged.into_iter().collect();
        assert_eq!(
            map.get("ANTHROPIC_API_KEY").unwrap(),
            "sk-ant-config",
            "config env should override secrets"
        );
    }

    // -- Task 3 tests: execute_session --

    #[tokio::test]
    async fn test_execute_session_simple_command() {
        let runtime = make_test_runtime(make_test_secrets(None, None));
        let config = make_session_config("echo", &["hello world"]);
        let result = runtime.execute_session(config, |_line| None).await.unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.timed_out);
        assert!(!result.shutdown_requested);
    }

    #[tokio::test]
    async fn test_execute_session_captures_stderr() {
        let runtime = make_test_runtime(make_test_secrets(None, None));
        let config = make_session_config("sh", &["-c", "echo 'error output' >&2"]);
        let result = runtime.execute_session(config, |_line| None).await.unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stderr.contains("error output"));
    }

    #[tokio::test]
    async fn test_execute_session_nonzero_exit() {
        let runtime = make_test_runtime(make_test_secrets(None, None));
        let config = make_session_config("false", &[]);
        let result = runtime.execute_session(config, |_line| None).await.unwrap();
        assert_eq!(result.exit_code, Some(1));
    }

    #[tokio::test]
    async fn test_execute_session_timeout() {
        let runtime = make_test_runtime(make_test_secrets(None, None));
        let mut config = make_session_config("sleep", &["60"]);
        config.timeout = Duration::from_secs(1);
        config.sigterm_grace = Duration::from_secs(1);
        let result = runtime.execute_session(config, |_line| None).await.unwrap();
        assert!(result.timed_out);
    }

    #[tokio::test]
    async fn test_execute_session_shutdown_flag() {
        let secrets = make_test_secrets(None, None);
        let shutdown = Arc::new(AtomicBool::new(false));
        let runtime = SdkRuntime::new(
            make_test_config(),
            secrets,
            PathBuf::from("test-config.yaml"),
            Arc::clone(&shutdown),
            UiHandle::null(),
        );
        let config = make_session_config("sleep", &["60"]);

        let shutdown_clone = Arc::clone(&shutdown);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            shutdown_clone.store(true, Ordering::Relaxed);
        });

        let result = runtime.execute_session(config, |_line| None).await.unwrap();
        assert!(result.shutdown_requested);
    }

    #[tokio::test]
    async fn test_execute_session_session_id_tracking() {
        let runtime = make_test_runtime(make_test_secrets(None, None));
        let config = make_session_config(
            "echo",
            &[r#"{"type":"session_started","session_id":"sess-123"}"#],
        );
        let result = runtime
            .execute_session(config, |line| {
                if line.contains("session_started") {
                    Some(SdkOutputEvent::SessionStarted {
                        session_id: "sess-123".to_string(),
                    })
                } else {
                    None
                }
            })
            .await
            .unwrap();
        assert_eq!(result.session_id.as_deref(), Some("sess-123"));
    }

    // -- Task 4 tests: graceful shutdown helpers --

    #[tokio::test]
    async fn test_send_sigterm_exited_process() {
        let mut child = Command::new("true").spawn().unwrap();
        child.wait().await.unwrap();
        let result = send_sigterm(&child);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_graceful_kill_immediate_exit() {
        let mut child = Command::new("true").spawn().unwrap();
        let result = graceful_kill(&mut child, Duration::from_secs(1)).await;
        assert!(result.is_ok());
    }

    // -- Task 9.20: unknown SDK provider fails --

    #[tokio::test]
    async fn test_run_session_unknown_sdk_provider_fails() {
        let runtime = make_test_runtime(make_test_secrets(None, None));
        let story = crate::watcher::StoryInfo {
            story_id: "15.5".to_string(),
            story_key: "15-5-test".to_string(),
            epic_num: 15,
            story_num: 5,
            label: "test".to_string(),
            branch_name: "story/15-5-test".to_string(),
            specs_path: PathBuf::from("/tmp/story.md"),
            dependencies: vec![],
            status: "ready-for-dev".to_string(),
        };
        let context = SessionContext {
            story: &story,
            base_branch_override: None,
            consultations: vec![],
            role: LlmRole::Dev,
            initial_phase: "dev",
        };
        let outcome = runtime.run_session(context).await;
        match outcome {
            SessionOutcome::Failed {
                error, story_key, ..
            } => {
                assert!(error.contains("not implemented"));
                assert_eq!(story_key, "15-5-test");
            }
            _ => panic!("expected SessionOutcome::Failed"),
        }
    }

    // -- Task 9.21: config_for_role returns correct config --

    #[test]
    fn test_config_for_role_returns_correct_config() {
        let runtime = make_test_runtime(make_test_secrets(None, None));
        let dev_config = runtime.config_for_role(&LlmRole::Dev);
        assert_eq!(dev_config.provider, "anthropic");
        let review_config = runtime.config_for_role(&LlmRole::Review);
        assert_eq!(review_config.provider, "anthropic");
        let supervisor_config = runtime.config_for_role(&LlmRole::Supervisor);
        assert_eq!(supervisor_config.provider, "anthropic");
    }

    // -- Task 9.19: claude-code dispatch integration test --

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_session_claude_code_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("fake-claude.sh");
        std::fs::write(
            &script_path,
            "#!/bin/sh\necho '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"test-session-123\"}'\necho '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"All done\",\"is_error\":false}'\n",
        )
        .unwrap();
        std::fs::set_permissions(
            &script_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        let mut config = BotConfig::_test_minimal("pretty", "info");
        config.llm.dev.provider = "claude-code".to_string();
        config.llm.dev.cli_path = Some(script_path.to_string_lossy().to_string());
        config.bmad_paths.project_root = dir.path().to_string_lossy().to_string();
        config.bmad_paths.implementation_artifacts = dir.path().to_string_lossy().to_string();

        let runtime = SdkRuntime::new(
            Arc::new(config),
            make_test_secrets(Some("sk-test"), None),
            PathBuf::from("test-config.yaml"),
            Arc::new(AtomicBool::new(false)),
            UiHandle::null(),
        );

        let story = crate::watcher::StoryInfo {
            story_id: "15.5".to_string(),
            story_key: "15-5-claude-code".to_string(),
            epic_num: 15,
            story_num: 5,
            label: "claude-code".to_string(),
            branch_name: "story/15-5-claude-code".to_string(),
            specs_path: dir.path().join("15-5-claude-code.md"),
            dependencies: vec![],
            status: "in-progress".to_string(),
        };
        let context = SessionContext {
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
                assert_eq!(story_key, "15-5-claude-code");
                assert_eq!(branch, "story/15-5-claude-code");
                assert!(pr_context.is_some());
                assert_eq!(pr_context.unwrap(), "All done");
            }
            other => panic!("expected SessionOutcome::Completed, got {:?}", other),
        }
    }

    // -- Story 15.7: resume dispatcher tests --

    #[tokio::test]
    async fn test_resume_sdk_session_unknown_provider_fails() {
        let runtime = make_test_runtime(make_test_secrets(None, None));
        let story = crate::watcher::StoryInfo {
            story_id: "15.7".to_string(),
            story_key: "15-7-test".to_string(),
            epic_num: 15,
            story_num: 7,
            label: "test".to_string(),
            branch_name: "story/15-7-test".to_string(),
            specs_path: PathBuf::from("/tmp/story.md"),
            dependencies: vec![],
            status: "in-progress".to_string(),
        };
        let (outcome, result) = runtime
            .resume_sdk_session(
                "unknown-provider",
                "sess-123",
                "Continue",
                &story,
                &LlmRole::Dev,
            )
            .await;
        assert!(
            result.is_none(),
            "unknown provider should not return a result"
        );
        match outcome {
            SessionOutcome::Failed { error, .. } => {
                assert!(error.contains("does not support resume"));
            }
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn test_shutdown_flag_accessor() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let runtime = SdkRuntime::new(
            make_test_config(),
            make_test_secrets(None, None),
            PathBuf::from("test.yaml"),
            Arc::clone(&shutdown),
            UiHandle::null(),
        );
        assert!(!runtime.shutdown_flag().load(Ordering::Relaxed));
        shutdown.store(true, Ordering::Relaxed);
        assert!(runtime.shutdown_flag().load(Ordering::Relaxed));
    }
}
