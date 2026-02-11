//! Terminal command execution tool — sandboxed shell access for the LLM agent.
//!
//! Implements the rig `Tool` trait for shell command execution via `tokio::process`.
//! Commands are executed through `sh -c` with configurable timeout protection.
//! Non-zero exit codes are returned as `Ok(output)` — only spawn failures,
//! timeouts, invalid working directories, and blocked commands produce `Err`.
//!
//! ## Sandboxing
//!
//! The tool enforces that all operations stay within the project root:
//! - `working_dir` overrides are resolved and checked against `project_root`
//! - `cd` targets inside commands are extracted and validated
//! - A minimal blocklist prevents clearly dangerous commands (`sudo`, `mkfs`, etc.)

use regex::Regex;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Component, Path, PathBuf};

/// Maximum combined output size in bytes (~50KB).
const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Dangerous command patterns — blocked unconditionally.
/// Each entry is a regex pattern matched against the full command string.
const BLOCKED_COMMAND_PATTERNS: &[&str] = &[
    r"\bsudo\b",
    r"\bmkfs\b",
    r"\bdd\s+if=",
    r"\bshutdown\b",
    r"\breboot\b",
    r"\binit\s+[0-6]\b",
    r"\bsystemctl\s+(start|stop|restart|disable|enable)\b",
    r"\blaunchctl\s+(load|unload|bootout|bootstrap)\b",
    r"\brm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+|--force\s+)*/\s*$", // rm -rf /
    r"\brm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+|--force\s+)*/[^a-zA-Z]", // rm -rf /etc etc
];

/// Paths that are allowed even though they're outside project_root.
/// These are common tool/device paths that commands legitimately reference.
const ALLOWED_EXTERNAL_PATHS: &[&str] = &[
    "/dev/null",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/stdin",
    "/dev/zero",
    "/dev/urandom",
    "/dev/random",
    "/tmp",
    "/var/tmp",
];

/// Terminal command execution tool for the rig agent.
///
/// Executes shell commands via `sh -c` with timeout protection.
/// Non-zero exit codes are NOT errors — they are returned as `Ok(output)`
/// with the exit code included in the output string. Only process spawn
/// failures, timeouts, invalid working directories, and blocked commands
/// produce `Err`.
///
/// The struct holds only configuration — no cached process handles.
#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalTool {
    /// Default working directory for commands (project root).
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

    /// Specified working directory doesn't exist or is outside project root.
    #[error("Invalid working directory '{path}': {reason}")]
    InvalidWorkingDir {
        /// The path that was invalid.
        path: String,
        /// Reason the path was invalid.
        reason: String,
    },

    /// Command was blocked by sandboxing rules.
    #[error("Command blocked: {reason}")]
    CommandBlocked {
        /// The command that was blocked.
        command: String,
        /// Why it was blocked.
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

    /// Validate that a working directory is under the project root.
    ///
    /// Resolves the path (canonicalize if it exists, otherwise normalize manually)
    /// and checks it starts with the project root.
    fn validate_working_dir(&self, dir: &str) -> Result<PathBuf, TerminalToolError> {
        let path = PathBuf::from(dir);

        if !path.exists() {
            return Err(TerminalToolError::InvalidWorkingDir {
                path: dir.to_string(),
                reason: "Directory does not exist".to_string(),
            });
        }
        if !path.is_dir() {
            return Err(TerminalToolError::InvalidWorkingDir {
                path: dir.to_string(),
                reason: "Path is not a directory".to_string(),
            });
        }

        // Canonicalize both paths to resolve symlinks and ..
        let canonical =
            std::fs::canonicalize(&path).map_err(|e| TerminalToolError::InvalidWorkingDir {
                path: dir.to_string(),
                reason: format!("Failed to resolve path: {e}"),
            })?;

        let project_root =
            std::fs::canonicalize(&self.working_dir).unwrap_or_else(|_| self.working_dir.clone());

        if !canonical.starts_with(&project_root) {
            return Err(TerminalToolError::InvalidWorkingDir {
                path: dir.to_string(),
                reason: format!(
                    "Working directory is outside project root ({})",
                    project_root.display()
                ),
            });
        }

        Ok(canonical)
    }

    /// Check command against the blocked patterns blocklist.
    fn check_blocked_patterns(command: &str) -> Option<String> {
        for pattern in BLOCKED_COMMAND_PATTERNS {
            if let Ok(re) = Regex::new(pattern)
                && re.is_match(command)
            {
                return Some(format!("Matches blocked pattern: {pattern}"));
            }
        }
        None
    }

    /// Extract `cd` targets from a shell command and validate they resolve
    /// under the project root.
    ///
    /// Handles common shell patterns:
    /// - `cd /some/path && ...`
    /// - `cd ../sibling && ...`
    /// - `cd ~/somewhere`
    /// - `(cd /path; ...)`
    fn validate_cd_targets(
        &self,
        command: &str,
        effective_working_dir: &Path,
    ) -> Result<(), TerminalToolError> {
        // Match cd followed by a path (stop at shell metacharacters)
        let re = Regex::new(r"\bcd\s+([^\s;&|)]+)").expect("valid regex");

        let project_root =
            std::fs::canonicalize(&self.working_dir).unwrap_or_else(|_| self.working_dir.clone());

        // Canonicalize the effective working dir so relative cd targets resolve
        // against the same canonical base as project_root (handles symlinks like
        // /tmp → /private/tmp on macOS).
        let canonical_working_dir = std::fs::canonicalize(effective_working_dir)
            .unwrap_or_else(|_| effective_working_dir.to_path_buf());

        for cap in re.captures_iter(command) {
            let target = &cap[1];

            // Skip cd with flags like cd -P, cd -L (the actual path would be the next arg)
            if target.starts_with('-') {
                continue;
            }

            // ~ expansion — always suspect outside project root
            if target.starts_with('~') {
                // Allow ~/... only if it resolves under project root
                if let Some(home) = dirs_or_home() {
                    let expanded = if target == "~" {
                        home.clone()
                    } else {
                        home.join(&target[2..]) // skip ~/
                    };
                    if !expanded.starts_with(&project_root) {
                        return Err(TerminalToolError::CommandBlocked {
                            command: command.to_string(),
                            reason: format!(
                                "cd target '{}' resolves outside project root ({})",
                                target,
                                project_root.display()
                            ),
                        });
                    }
                } else {
                    // Can't resolve ~, block to be safe
                    return Err(TerminalToolError::CommandBlocked {
                        command: command.to_string(),
                        reason: format!("cd target '{}' uses ~ which cannot be resolved", target),
                    });
                }
                continue;
            }

            // Resolve the target path
            let resolved = if Path::new(target).is_absolute() {
                PathBuf::from(target)
            } else {
                canonical_working_dir.join(target)
            };

            // Try to canonicalize (handles symlinks like /tmp → /private/tmp).
            // Fall back to normalize_path if the path doesn't exist yet.
            let normalized =
                std::fs::canonicalize(&resolved).unwrap_or_else(|_| normalize_path(&resolved));

            // Check if it's under project root
            if !normalized.starts_with(&project_root) {
                return Err(TerminalToolError::CommandBlocked {
                    command: command.to_string(),
                    reason: format!(
                        "cd target '{}' resolves to '{}' which is outside project root ({})",
                        target,
                        normalized.display(),
                        project_root.display()
                    ),
                });
            }
        }

        Ok(())
    }

    /// Validate absolute paths referenced in the command stay within project root
    /// or are in the allowed external paths list.
    ///
    /// This is a heuristic — it extracts path-like tokens starting with `/`
    /// and warns (via tracing) if they're outside project root. Paths in the
    /// allowed list (`/dev/null`, `/tmp`, etc.) are silently permitted.
    fn check_absolute_paths(&self, command: &str) {
        // Match absolute paths (starting with /) — stop at shell metacharacters and quotes
        let re = Regex::new(r#"(?:^|\s)(/[^\s;&|)"']+)"#).expect("valid regex");

        let project_root =
            std::fs::canonicalize(&self.working_dir).unwrap_or_else(|_| self.working_dir.clone());

        for cap in re.captures_iter(command) {
            let path_str = &cap[1];
            let path = Path::new(path_str);

            // Skip allowed external paths
            if ALLOWED_EXTERNAL_PATHS
                .iter()
                .any(|allowed| path_str.starts_with(allowed))
            {
                continue;
            }

            // Skip paths that are under project root
            let normalized = normalize_path(&PathBuf::from(path_str));
            if normalized.starts_with(&project_root) {
                continue;
            }

            // Warn about external paths (don't block — too many false positives
            // with tool paths like /usr/bin/env, build output paths, etc.)
            tracing::warn!(
                action = "terminal_external_path",
                path = %path.display(),
                project_root = %project_root.display(),
                "Command references absolute path outside project root"
            );
        }
    }

    /// Run all sandboxing validations on a command before execution.
    fn validate_command(
        &self,
        command: &str,
        effective_working_dir: &Path,
    ) -> Result<(), TerminalToolError> {
        // 1. Blocked command patterns
        if let Some(reason) = Self::check_blocked_patterns(command) {
            tracing::error!(
                action = "terminal_command_blocked",
                command = %command,
                reason = %reason,
                "Command blocked by sandbox"
            );
            return Err(TerminalToolError::CommandBlocked {
                command: command.to_string(),
                reason,
            });
        }

        // 2. Validate cd targets
        self.validate_cd_targets(command, effective_working_dir)?;

        // 3. Warn about absolute paths outside project root (non-blocking)
        self.check_absolute_paths(command);

        Ok(())
    }
}

/// Normalize a path by resolving `.` and `..` components without touching the filesystem.
///
/// This allows us to check path containment even for paths that don't exist yet.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop the last component if possible (don't go above root)
                if !components.is_empty() && !matches!(components.last(), Some(Component::RootDir))
                {
                    components.pop();
                }
            }
            Component::CurDir => {
                // Skip .
            }
            other => {
                components.push(other);
            }
        }
    }
    components.iter().collect()
}

/// Best-effort home directory resolution.
fn dirs_or_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
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
                formatting (cargo fmt), linting (cargo clippy), or any other shell command needed. \
                NOTE: Commands are sandboxed to the project root directory. Working directory \
                overrides and cd commands must stay within the project."
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
                        "description": "Override working directory for this command. Must be within the project root. (optional)"
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
        // Determine and validate working directory
        let work_dir = if let Some(ref dir) = args.working_dir {
            self.validate_working_dir(dir)?
        } else {
            self.working_dir.clone()
        };

        let timeout = args.timeout_secs.unwrap_or(self.timeout_secs);

        // Validate command against sandbox rules
        self.validate_command(&args.command, &work_dir)?;

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
    use std::sync::LazyLock;

    /// Shared project root for tests — a real temp dir that exists on disk.
    static TEST_PROJECT_ROOT: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
        let dir = tempfile::tempdir().expect("create temp dir");
        // Create a subdirectory to test cd validation
        std::fs::create_dir_all(dir.path().join("src/tools")).expect("create subdirs");
        std::fs::create_dir_all(dir.path().join("tests")).expect("create tests dir");
        dir
    });

    fn make_tool() -> TerminalTool {
        TerminalTool::new(TEST_PROJECT_ROOT.path().to_path_buf(), 30)
    }

    // -----------------------------------------------------------------------
    // Tool definition tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_terminal_tool_definition_name() {
        let tool = make_tool();
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "terminal");
    }

    #[tokio::test]
    async fn test_terminal_tool_definition_has_detailed_description() {
        let tool = make_tool();
        let def = tool.definition("test".to_string()).await;
        assert!(
            def.description.contains("sh -c"),
            "Should mention sh -c: {}",
            def.description
        );
        assert!(
            def.description.contains("cargo test"),
            "Should mention cargo test: {}",
            def.description
        );
        assert!(
            def.description.contains("sandbox"),
            "Should mention sandboxing: {}",
            def.description
        );
    }

    // -----------------------------------------------------------------------
    // Args deserialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_terminal_tool_args_deserialize_minimal() {
        let json = r#"{"command": "echo hello"}"#;
        let args: TerminalToolArgs = serde_json::from_str(json).expect("parse");
        assert_eq!(args.command, "echo hello");
        assert!(args.working_dir.is_none());
        assert!(args.timeout_secs.is_none());
    }

    #[test]
    fn test_terminal_tool_args_deserialize_full() {
        let json = r#"{"command": "ls", "working_dir": "/tmp", "timeout_secs": 60}"#;
        let args: TerminalToolArgs = serde_json::from_str(json).expect("parse");
        assert_eq!(args.command, "ls");
        assert_eq!(args.working_dir.unwrap(), "/tmp");
        assert_eq!(args.timeout_secs.unwrap(), 60);
    }

    // -----------------------------------------------------------------------
    // Error type tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_terminal_tool_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TerminalToolError>();
    }

    #[test]
    fn test_terminal_tool_error_display() {
        let exec_err = TerminalToolError::ExecutionFailed {
            command: "bad-cmd".to_string(),
            reason: "not found".to_string(),
        };
        assert!(exec_err.to_string().contains("bad-cmd"));
        assert!(exec_err.to_string().contains("not found"));

        let timeout_err = TerminalToolError::Timeout {
            command: "sleep 999".to_string(),
            timeout_secs: 30,
        };
        assert!(timeout_err.to_string().contains("sleep 999"));
        assert!(timeout_err.to_string().contains("30"));

        let dir_err = TerminalToolError::InvalidWorkingDir {
            path: "/fake".to_string(),
            reason: "doesn't exist".to_string(),
        };
        assert!(dir_err.to_string().contains("/fake"));

        let blocked_err = TerminalToolError::CommandBlocked {
            command: "sudo rm -rf /".to_string(),
            reason: "Matches blocked pattern".to_string(),
        };
        assert!(blocked_err.to_string().contains("blocked"));
        assert!(blocked_err.to_string().contains("Matches blocked pattern"));
    }

    #[test]
    fn test_terminal_tool_serializable() {
        let tool = make_tool();
        let json = serde_json::to_string(&tool).expect("serialize");
        assert!(json.contains("working_dir"));
        assert!(json.contains("timeout_secs"));
    }

    // -----------------------------------------------------------------------
    // Command execution tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_terminal_tool_echo_command() {
        let tool = make_tool();
        let args = TerminalToolArgs {
            command: "echo hello world".to_string(),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("hello world"));
        assert!(output.contains("Exit code: 0"));
    }

    #[tokio::test]
    async fn test_terminal_tool_exit_code_zero() {
        let tool = make_tool();
        let args = TerminalToolArgs {
            command: "true".to_string(),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.call(args).await.expect("should succeed");
        assert!(result.contains("Exit code: 0"));
    }

    #[tokio::test]
    async fn test_terminal_tool_nonzero_exit_returns_ok() {
        let tool = make_tool();
        let args = TerminalToolArgs {
            command: "false".to_string(),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_ok(), "Non-zero exit should be Ok");
        let output = result.unwrap();
        assert!(
            output.contains("Exit code: 1"),
            "Should contain non-zero exit code"
        );
    }

    #[tokio::test]
    async fn test_terminal_tool_captures_stderr() {
        let tool = make_tool();
        let args = TerminalToolArgs {
            command: "echo error_output >&2".to_string(),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.call(args).await.expect("should succeed");
        assert!(result.contains("error_output"));
        assert!(result.contains("stderr"));
    }

    #[tokio::test]
    async fn test_terminal_tool_working_dir_override_within_project() {
        let tool = make_tool();
        let src_dir = TEST_PROJECT_ROOT.path().join("src");

        let args = TerminalToolArgs {
            command: "pwd".to_string(),
            working_dir: Some(src_dir.display().to_string()),
            timeout_secs: None,
        };

        let result = tool.call(args).await.expect("should succeed");
        assert!(result.contains("src"));
    }

    #[tokio::test]
    async fn test_terminal_tool_timeout_kills_process() {
        let tool = TerminalTool::new(TEST_PROJECT_ROOT.path().to_path_buf(), 1);
        let args = TerminalToolArgs {
            command: "sleep 60".to_string(),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_terminal_tool_invalid_working_dir() {
        let tool = make_tool();
        let args = TerminalToolArgs {
            command: "echo test".to_string(),
            working_dir: Some("/nonexistent/path/12345".to_string()),
            timeout_secs: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn test_terminal_tool_multiline_output() {
        let tool = make_tool();
        let args = TerminalToolArgs {
            command: "echo 'line1'; echo 'line2'; echo 'line3'".to_string(),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.call(args).await.expect("should succeed");
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
    }

    // -----------------------------------------------------------------------
    // Truncation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_truncate_output_within_limit() {
        let output = "short output";
        let result = TerminalTool::truncate_output(output, output.len());
        assert_eq!(result, output);
    }

    #[test]
    fn test_truncate_output_exceeds_limit() {
        let output = "x".repeat(MAX_OUTPUT_BYTES + 100);
        let result = TerminalTool::truncate_output(&output, output.len());
        assert!(result.len() < output.len());
        assert!(result.contains("truncated"));
    }

    // -----------------------------------------------------------------------
    // normalize_path tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_normalize_path_no_dotdot() {
        let p = normalize_path(Path::new("/a/b/c"));
        assert_eq!(p, PathBuf::from("/a/b/c"));
    }

    #[test]
    fn test_normalize_path_with_dotdot() {
        let p = normalize_path(Path::new("/a/b/../c"));
        assert_eq!(p, PathBuf::from("/a/c"));
    }

    #[test]
    fn test_normalize_path_with_dot() {
        let p = normalize_path(Path::new("/a/./b/c"));
        assert_eq!(p, PathBuf::from("/a/b/c"));
    }

    #[test]
    fn test_normalize_path_multiple_dotdots() {
        let p = normalize_path(Path::new("/a/b/c/../../d"));
        assert_eq!(p, PathBuf::from("/a/d"));
    }

    #[test]
    fn test_normalize_path_dotdot_at_root() {
        let p = normalize_path(Path::new("/a/../.."));
        assert_eq!(p, PathBuf::from("/"));
    }

    // -----------------------------------------------------------------------
    // Blocked command pattern tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_blocks_sudo() {
        let result = TerminalTool::check_blocked_patterns("sudo apt-get install foo");
        assert!(result.is_some());
        assert!(result.unwrap().contains("sudo"));
    }

    #[test]
    fn test_blocks_sudo_mid_command() {
        let result = TerminalTool::check_blocked_patterns("echo hello && sudo rm -rf /");
        assert!(result.is_some());
    }

    #[test]
    fn test_blocks_mkfs() {
        let result = TerminalTool::check_blocked_patterns("mkfs.ext4 /dev/sda1");
        assert!(result.is_some());
    }

    #[test]
    fn test_blocks_dd() {
        let result = TerminalTool::check_blocked_patterns("dd if=/dev/zero of=/dev/sda");
        assert!(result.is_some());
    }

    #[test]
    fn test_blocks_shutdown() {
        let result = TerminalTool::check_blocked_patterns("shutdown -h now");
        assert!(result.is_some());
    }

    #[test]
    fn test_blocks_reboot() {
        let result = TerminalTool::check_blocked_patterns("reboot");
        assert!(result.is_some());
    }

    #[test]
    fn test_blocks_systemctl() {
        let result = TerminalTool::check_blocked_patterns("systemctl stop nginx");
        assert!(result.is_some());
    }

    #[test]
    fn test_blocks_launchctl() {
        let result = TerminalTool::check_blocked_patterns("launchctl unload com.apple.foo");
        assert!(result.is_some());
    }

    #[test]
    fn test_allows_normal_commands() {
        assert!(TerminalTool::check_blocked_patterns("cargo test").is_none());
        assert!(TerminalTool::check_blocked_patterns("cargo build --release").is_none());
        assert!(TerminalTool::check_blocked_patterns("echo hello").is_none());
        assert!(TerminalTool::check_blocked_patterns("cat src/main.rs").is_none());
        assert!(TerminalTool::check_blocked_patterns("grep -r 'pattern' .").is_none());
        assert!(TerminalTool::check_blocked_patterns("ls -la").is_none());
        assert!(TerminalTool::check_blocked_patterns("git status").is_none());
        assert!(TerminalTool::check_blocked_patterns("rm -rf target/").is_none());
    }

    #[test]
    fn test_does_not_false_positive_pseudo_sudo() {
        // "pseudo" contains "sudo" as substring but \b should prevent matching
        assert!(TerminalTool::check_blocked_patterns("pseudocode test").is_none());
    }

    // -----------------------------------------------------------------------
    // cd validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_cd_within_project_allowed() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let result = tool.validate_cd_targets("cd src && ls", work_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cd_subdirectory_allowed() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let result = tool.validate_cd_targets("cd src/tools && cargo test", work_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cd_absolute_within_project_allowed() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let abs_src = work_dir.join("src");
        let cmd = format!("cd {} && ls", abs_src.display());
        let result = tool.validate_cd_targets(&cmd, work_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cd_dotdot_escaping_project_blocked() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let result = tool.validate_cd_targets("cd ../../../ etc && cat passwd", work_dir);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("blocked") || err.contains("outside project root"));
    }

    #[test]
    fn test_cd_absolute_outside_project_blocked() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let result = tool.validate_cd_targets("cd /etc && ls", work_dir);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("outside project root"));
    }

    #[test]
    fn test_cd_tilde_blocked() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let result = tool.validate_cd_targets("cd ~/other-project && ls", work_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_cd_dotdot_within_project_allowed() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path().join("src/tools");
        let result = tool.validate_cd_targets("cd ../.. && ls", &work_dir);
        // ../.. from src/tools goes back to project root — should be allowed
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_cd_in_command_allowed() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let result = tool.validate_cd_targets("cargo test 2>&1 | grep 'test result'", work_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_cd_commands_all_validated() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        // First cd is fine, second escapes
        let result = tool.validate_cd_targets("cd src && echo ok; cd /etc && cat passwd", work_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_cd_with_flags_skipped() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        // cd -P is a flag, should be skipped
        let result = tool.validate_cd_targets("cd -P src", work_dir);
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // working_dir sandbox tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_working_dir_within_project() {
        let tool = make_tool();
        let src_dir = TEST_PROJECT_ROOT.path().join("src");
        let result = tool.validate_working_dir(&src_dir.display().to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_working_dir_outside_project_blocked() {
        let tool = make_tool();
        // /tmp exists but is outside project root
        let result = tool.validate_working_dir("/tmp");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("outside project root"));
    }

    #[test]
    fn test_validate_working_dir_nonexistent_blocked() {
        let tool = make_tool();
        let result = tool.validate_working_dir("/nonexistent/path/xyz");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not exist"));
    }

    // -----------------------------------------------------------------------
    // Full validate_command integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_command_normal_allowed() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        assert!(tool.validate_command("cargo test", work_dir).is_ok());
        assert!(
            tool.validate_command("cargo build --release", work_dir)
                .is_ok()
        );
        assert!(tool.validate_command("echo hello", work_dir).is_ok());
        assert!(
            tool.validate_command("grep -r 'pattern' src/", work_dir)
                .is_ok()
        );
    }

    #[test]
    fn test_validate_command_sudo_blocked() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let result = tool.validate_command("sudo cargo test", work_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_command_cd_escape_blocked() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let result = tool.validate_command("cd /etc && cat passwd", work_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_command_cd_within_project_allowed() {
        let tool = make_tool();
        let work_dir = TEST_PROJECT_ROOT.path();
        let result = tool.validate_command("cd src && ls", work_dir);
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Full integration: blocked command in call()
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_terminal_tool_blocks_sudo_in_call() {
        let tool = make_tool();
        let args = TerminalToolArgs {
            command: "sudo echo hello".to_string(),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("blocked"));
    }

    #[tokio::test]
    async fn test_terminal_tool_blocks_working_dir_escape() {
        let tool = make_tool();
        let args = TerminalToolArgs {
            command: "ls".to_string(),
            working_dir: Some("/tmp".to_string()),
            timeout_secs: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("outside project root"));
    }

    #[tokio::test]
    async fn test_terminal_tool_blocks_cd_escape_in_call() {
        let tool = make_tool();
        let args = TerminalToolArgs {
            command: "cd /etc && cat passwd".to_string(),
            working_dir: None,
            timeout_secs: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("outside project root"));
    }
}
