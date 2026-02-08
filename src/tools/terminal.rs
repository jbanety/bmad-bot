//! Terminal command execution tool — sandboxed shell access for the LLM agent.
//!
//! Implements the rig `Tool` trait for shell command execution via `tokio::process`.
//! Commands are executed through `sh -c` with configurable timeout protection.
//! Non-zero exit codes are returned as `Ok(output)` — only spawn failures,
//! timeouts, and invalid working directories produce `Err`.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

/// Maximum combined output size in bytes (~50KB).
const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Terminal command execution tool for the rig agent.
///
/// Executes shell commands via `sh -c` with timeout protection.
/// Non-zero exit codes are NOT errors — they are returned as `Ok(output)`
/// with the exit code included in the output string. Only process spawn
/// failures, timeouts, and invalid working directories produce `Err`.
///
/// The struct holds only configuration — no cached process handles.
#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalTool {
    /// Default working directory for commands.
    working_dir: PathBuf,
    /// Maximum execution time per command in seconds.
    timeout_secs: u64,
}

/// Arguments passed by the LLM agent when calling the `terminal` tool.
#[derive(Debug, Deserialize)]
pub struct TerminalToolArgs {
    /// Shell command to execute.
    pub command: String,
    /// Override working directory (relative to project root or absolute).
    pub working_dir: Option<String>,
    /// Override default timeout for this command (in seconds).
    pub timeout_secs: Option<u64>,
}

/// Errors from the `terminal` tool.
///
/// Note: There is intentionally NO `NonZeroExit` variant. Non-zero exit codes
/// are returned as `Ok(output)` so the LLM agent can reason about the full output.
#[derive(Debug, thiserror::Error)]
pub enum TerminalToolError {
    /// Process spawn or I/O error.
    #[error("Command execution failed for '{command}': {reason}")]
    ExecutionFailed {
        /// The command that failed.
        command: String,
        /// Description of the failure.
        reason: String,
    },

    /// Command exceeded timeout.
    #[error("Command timed out after {timeout_secs}s: '{command}'")]
    Timeout {
        /// The command that timed out.
        command: String,
        /// The timeout that was exceeded.
        timeout_secs: u64,
    },

    /// Specified working directory doesn't exist.
    #[error("Invalid working directory '{path}': {reason}")]
    InvalidWorkingDir {
        /// The path that was invalid.
        path: String,
        /// Reason the path was invalid.
        reason: String,
    },
}

impl TerminalTool {
    /// Create a new `TerminalTool` with the given working directory and timeout.
    pub fn new(working_dir: PathBuf, timeout_secs: u64) -> Self {
        Self {
            working_dir,
            timeout_secs,
        }
    }

    /// Truncate output to MAX_OUTPUT_BYTES, appending a truncation notice if needed.
    fn truncate_output(output: &str, total_bytes: usize) -> String {
        if output.len() <= MAX_OUTPUT_BYTES {
            return output.to_string();
        }
        // Find a valid UTF-8 boundary near the limit
        let mut end = MAX_OUTPUT_BYTES;
        while end > 0 && !output.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}\n[... truncated, total {} bytes]",
            &output[..end],
            total_bytes
        )
    }
}

impl Tool for TerminalTool {
    const NAME: &'static str = "terminal";
    type Error = TerminalToolError;
    type Args = TerminalToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "terminal".to_string(),
            description: "Execute a shell command and return its output. The command is run via \
                'sh -c' with timeout protection. Returns combined stdout and stderr with the exit code. \
                IMPORTANT: Non-zero exit codes are NOT errors — many valid commands return non-zero \
                (e.g. grep with no matches returns 1, cargo test on failure returns 1). \
                The full output is always returned so you can reason about the result. \
                Only process spawn failures, timeouts, and invalid working directories produce errors. \
                Use this for: running tests (cargo test), checking compilation (cargo check), \
                formatting (cargo fmt), linting (cargo clippy), or any other shell command needed."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute (passed to 'sh -c')"
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Override working directory for this command (optional)"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Override timeout in seconds for this command (optional, default: 30)"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Determine working directory
        let work_dir = if let Some(ref dir) = args.working_dir {
            let path = PathBuf::from(dir);
            if !path.exists() {
                return Err(TerminalToolError::InvalidWorkingDir {
                    path: dir.clone(),
                    reason: "Directory does not exist".to_string(),
                });
            }
            if !path.is_dir() {
                return Err(TerminalToolError::InvalidWorkingDir {
                    path: dir.clone(),
                    reason: "Path is not a directory".to_string(),
                });
            }
            path
        } else {
            self.working_dir.clone()
        };

        let timeout = args.timeout_secs.unwrap_or(self.timeout_secs);

        tracing::info!(
            action = "terminal_exec",
            command = %args.command,
            working_dir = %work_dir.display(),
            timeout_secs = timeout,
            "Executing command"
        );

        // Spawn the command
        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&args.command)
            .current_dir(&work_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| TerminalToolError::ExecutionFailed {
                command: args.command.clone(),
                reason: e.to_string(),
            })?;

        // Wait with timeout
        let timeout_duration = std::time::Duration::from_secs(timeout);
        let output_result = tokio::time::timeout(timeout_duration, child.wait_with_output()).await;

        match output_result {
            Ok(Ok(output)) => {
                let exit_code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let stdout_len = stdout.len();
                let stderr_len = stderr.len();

                tracing::info!(
                    action = "terminal_result",
                    exit_code = exit_code,
                    stdout_len = stdout_len,
                    stderr_len = stderr_len,
                    "Command completed"
                );

                // Format the output
                let mut combined = format!("Exit code: {}\n--- stdout ---\n{}", exit_code, stdout);
                if !stderr.is_empty() {
                    combined.push_str(&format!("\n--- stderr ---\n{}", stderr));
                }

                let total_bytes = combined.len();
                Ok(Self::truncate_output(&combined, total_bytes))
            }
            Ok(Err(e)) => {
                tracing::error!(
                    action = "terminal_exec_failed",
                    command = %args.command,
                    reason = %e,
                    "Command execution failed"
                );
                Err(TerminalToolError::ExecutionFailed {
                    command: args.command,
                    reason: e.to_string(),
                })
            }
            Err(_) => {
                tracing::warn!(
                    action = "terminal_timeout",
                    command = %args.command,
                    timeout_secs = timeout,
                    "Command timed out"
                );
                Err(TerminalToolError::Timeout {
                    command: args.command,
                    timeout_secs: timeout,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_terminal_tool_definition_name() {
        let dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "terminal");
        assert_eq!(TerminalTool::NAME, "terminal");
    }

    #[tokio::test]
    async fn test_terminal_tool_definition_has_detailed_description() {
        let dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let def = tool.definition("test".to_string()).await;
        assert!(def.description.contains("shell command"));
        assert!(def.description.contains("timeout"));
        assert!(
            def.description
                .contains("Non-zero exit codes are NOT errors")
        );
        assert!(def.description.contains("stdout"));
        assert!(def.description.contains("stderr"));
    }

    #[test]
    fn test_terminal_tool_args_deserialize_minimal() {
        let json = r#"{"command": "echo hello"}"#;
        let args: TerminalToolArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.command, "echo hello");
        assert!(args.working_dir.is_none());
        assert!(args.timeout_secs.is_none());
    }

    #[test]
    fn test_terminal_tool_args_deserialize_full() {
        let json = r#"{
            "command": "cargo test",
            "working_dir": "/tmp",
            "timeout_secs": 60
        }"#;
        let args: TerminalToolArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.command, "cargo test");
        assert_eq!(args.working_dir.unwrap(), "/tmp");
        assert_eq!(args.timeout_secs.unwrap(), 60);
    }

    #[test]
    fn test_terminal_tool_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TerminalToolError>();
    }

    #[test]
    fn test_terminal_tool_error_display() {
        let err = TerminalToolError::ExecutionFailed {
            command: "bad_cmd".to_string(),
            reason: "not found".to_string(),
        };
        assert!(err.to_string().contains("bad_cmd"));
        assert!(err.to_string().contains("not found"));

        let err = TerminalToolError::Timeout {
            command: "sleep 999".to_string(),
            timeout_secs: 5,
        };
        assert!(err.to_string().contains("sleep 999"));
        assert!(err.to_string().contains("5s"));

        let err = TerminalToolError::InvalidWorkingDir {
            path: "/nonexistent".to_string(),
            reason: "Directory does not exist".to_string(),
        };
        assert!(err.to_string().contains("/nonexistent"));
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn test_terminal_tool_serializable() {
        let tool = TerminalTool::new(PathBuf::from("/tmp/project"), 30);
        let json = serde_json::to_string(&tool).expect("Should serialize");
        let deserialized: TerminalTool = serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(deserialized.working_dir, PathBuf::from("/tmp/project"));
        assert_eq!(deserialized.timeout_secs, 30);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_terminal_tool_echo_command() {
        let dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let args = TerminalToolArgs {
            command: "echo hello".to_string(),
            working_dir: None,
            timeout_secs: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("hello"));
        assert!(result.contains("Exit code: 0"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_terminal_tool_exit_code_zero() {
        let dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let args = TerminalToolArgs {
            command: "true".to_string(),
            working_dir: None,
            timeout_secs: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Exit code: 0"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_terminal_tool_nonzero_exit_returns_ok() {
        let dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let args = TerminalToolArgs {
            command: "false".to_string(),
            working_dir: None,
            timeout_secs: None,
        };
        // Non-zero exit should still be Ok
        let result = tool.call(args).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Exit code: 1"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_terminal_tool_captures_stderr() {
        let dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let args = TerminalToolArgs {
            command: "echo error_msg >&2".to_string(),
            working_dir: None,
            timeout_secs: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("error_msg"));
        assert!(result.contains("--- stderr ---"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_terminal_tool_working_dir_override() {
        let dir = TempDir::new().unwrap();
        let override_dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let args = TerminalToolArgs {
            command: "pwd".to_string(),
            working_dir: Some(override_dir.path().to_string_lossy().to_string()),
            timeout_secs: None,
        };
        let result = tool.call(args).await.unwrap();
        // The output should contain the override directory path
        // Use canonicalize to handle symlinks (e.g., /tmp -> /private/tmp on macOS)
        let canonical_override = override_dir
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            result.contains(&canonical_override),
            "Expected output to contain '{}', got: {}",
            canonical_override,
            result
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_terminal_tool_timeout_kills_process() {
        let dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let args = TerminalToolArgs {
            command: "sleep 60".to_string(),
            working_dir: None,
            timeout_secs: Some(1),
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            TerminalToolError::Timeout { .. }
        ));
    }

    #[tokio::test]
    async fn test_terminal_tool_invalid_working_dir() {
        let dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let args = TerminalToolArgs {
            command: "echo test".to_string(),
            working_dir: Some("/this/path/does/not/exist/at/all".to_string()),
            timeout_secs: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            TerminalToolError::InvalidWorkingDir { .. }
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_terminal_tool_multiline_output() {
        let dir = TempDir::new().unwrap();
        let tool = TerminalTool::new(dir.path().to_path_buf(), 30);
        let args = TerminalToolArgs {
            command: "printf 'line1\nline2\nline3\n'".to_string(),
            working_dir: None,
            timeout_secs: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
    }

    #[test]
    fn test_truncate_output_within_limit() {
        let short = "Hello, World!";
        let result = TerminalTool::truncate_output(short, short.len());
        assert_eq!(result, short);
    }

    #[test]
    fn test_truncate_output_exceeds_limit() {
        // Create a string larger than MAX_OUTPUT_BYTES
        let large = "x".repeat(MAX_OUTPUT_BYTES + 1000);
        let result = TerminalTool::truncate_output(&large, large.len());
        assert!(result.len() < large.len());
        assert!(result.contains("[... truncated, total"));
        assert!(result.contains(&format!("{} bytes", large.len())));
    }
}
