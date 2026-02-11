//! Git operations tool — exposes git (branch, checkout, commit, push) to the rig agent via Git CLI.
//!
//! Implements the rig `Tool` trait with 9 git actions: clone, checkout, branch_create,
//! add, commit, push, diff, status, log. All operations use `tokio::process::Command`
//! to invoke the `git` CLI as a subprocess.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

/// Git operations tool for the rig agent.
///
/// Exposes 9 git actions via Git CLI subprocess: clone, checkout, branch_create, add, commit,
/// push, diff, status, log. The struct holds only configuration — git is invoked fresh
/// on each `call()` invocation via `tokio::process::Command`.
#[derive(Debug, Serialize, Deserialize)]
pub struct GitTool {
    /// Absolute path to the git repository root.
    repo_path: PathBuf,
}

/// Arguments passed by the LLM agent when calling the `git` tool.
#[derive(Debug, Deserialize)]
pub struct GitToolArgs {
    /// One of: clone, checkout, branch_create, add, commit, push, diff, status, log.
    pub action: String,
    /// Branch name for checkout/branch_create actions.
    pub branch: Option<String>,
    /// Commit message for commit action.
    pub message: Option<String>,
    /// File paths for add action (glob patterns like `["*"]` to stage all).
    pub paths: Option<Vec<String>>,
    /// Remote URL for clone action.
    pub url: Option<String>,
    /// Remote name for push (default: "origin").
    pub remote: Option<String>,
    /// Max entries for log (default: 10).
    pub max_count: Option<usize>,
    /// Base branch when creating a new branch (default: HEAD).
    pub from_branch: Option<String>,
}

/// Errors from the `git` tool.
#[derive(Debug, thiserror::Error)]
pub enum GitToolError {
    /// Unknown action string.
    #[error(
        "Invalid git action '{action}'. Valid actions: clone, checkout, branch_create, add, commit, push, diff, status, log"
    )]
    InvalidAction {
        /// The action that was not recognized.
        action: String,
    },

    /// Git CLI command failed with non-zero exit code.
    #[error("Git {action} failed (exit code {exit_code}): {stderr}")]
    CommandFailed {
        /// The git action that failed.
        action: String,
        /// The stderr output from the git command.
        stderr: String,
        /// The exit code from the git process.
        exit_code: i32,
    },

    /// Required argument not provided.
    #[error("Missing required argument '{argument}' for git {action}")]
    MissingArgument {
        /// The git action that needed the argument.
        action: String,
        /// The argument that was missing.
        argument: String,
    },

    /// Repository path issues.
    #[error("Path error: {reason}")]
    PathError {
        /// Description of the path problem.
        reason: String,
    },

    /// I/O error when spawning git subprocess.
    #[error("Failed to execute git: {reason}")]
    IoError {
        /// Description of the I/O error.
        reason: String,
    },
}

impl GitTool {
    /// Create a new `GitTool` operating on the repository at `repo_path`.
    pub fn new(repo_path: PathBuf) -> Self {
        Self { repo_path }
    }

    /// Run a git command with `-C <repo_path>` and the given arguments.
    ///
    /// Returns `(stdout, stderr)` on success (zero exit code).
    /// Returns `GitToolError::CommandFailed` on non-zero exit.
    async fn run_git(&self, action: &str, args: &[&str]) -> Result<(String, String), GitToolError> {
        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&self.repo_path)
            .args(args)
            .output()
            .await
            .map_err(|e| GitToolError::IoError {
                reason: e.to_string(),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(GitToolError::CommandFailed {
                action: action.to_string(),
                stderr,
                exit_code: output.status.code().unwrap_or(-1),
            });
        }

        Ok((stdout, stderr))
    }

    /// Clone a remote repository to `self.repo_path`.
    async fn handle_clone(&self, url: &str) -> Result<String, GitToolError> {
        let path_str = self
            .repo_path
            .to_str()
            .ok_or_else(|| GitToolError::PathError {
                reason: format!("Invalid repo path: {}", self.repo_path.display()),
            })?;

        let output = tokio::process::Command::new("git")
            .args(["clone", url, path_str])
            .output()
            .await
            .map_err(|e| GitToolError::IoError {
                reason: e.to_string(),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitToolError::CommandFailed {
                action: "clone".to_string(),
                stderr: stderr.to_string(),
                exit_code: output.status.code().unwrap_or(-1),
            });
        }

        Ok(format!("Cloned {} to {}", url, self.repo_path.display()))
    }

    /// Checkout an existing branch.
    async fn handle_checkout(&self, branch: &str) -> Result<String, GitToolError> {
        self.run_git("checkout", &["checkout", branch]).await?;
        Ok(format!("Checked out branch '{}'", branch))
    }

    /// Create a new branch from HEAD or a specified base, then checkout.
    async fn handle_branch_create(
        &self,
        branch: &str,
        from_branch: Option<&str>,
    ) -> Result<String, GitToolError> {
        let base_name = from_branch.unwrap_or("HEAD");
        let mut args = vec!["checkout", "-b", branch];
        // Only pass from_branch if explicitly provided (not HEAD, which is the default)
        if let Some(fb) = from_branch {
            args.push(fb);
        }
        self.run_git("branch_create", &args).await?;
        Ok(format!(
            "Created and checked out branch '{}' from {}",
            branch, base_name
        ))
    }

    /// Stage files.
    async fn handle_add(&self, paths: &[String]) -> Result<String, GitToolError> {
        // If paths contains "*", use "." to stage all
        let effective_paths: Vec<&str> = if paths.iter().any(|p| p == "*") {
            vec!["."]
        } else {
            paths.iter().map(|s| s.as_str()).collect()
        };

        let mut args: Vec<&str> = vec!["add"];
        args.extend(&effective_paths);

        self.run_git("add", &args).await?;
        Ok(format!("Staged {} path pattern(s)", paths.len()))
    }

    /// Create a commit on the current branch with staged changes.
    async fn handle_commit(&self, message: &str) -> Result<String, GitToolError> {
        let (stdout, _stderr) = self.run_git("commit", &["commit", "-m", message]).await?;

        // Extract short SHA from the commit output
        // git commit output first line is like: "[branch abc1234] commit message"
        let short_sha = stdout
            .lines()
            .next()
            .and_then(|line| {
                // Find content between [ and ]
                let start = line.find('[')? + 1;
                let end = line.find(']')?;
                let bracket_content = &line[start..end];
                // The SHA is after the space: "branch abc1234"
                bracket_content.split_whitespace().last().map(String::from)
            })
            .unwrap_or_else(|| "unknown".to_string());

        Ok(format!("Committed {}: {}", short_sha, message))
    }

    /// Push a branch to a remote.
    async fn handle_push(&self, remote: &str, branch: &str) -> Result<String, GitToolError> {
        self.run_git("push", &["push", remote, branch]).await?;
        Ok(format!("Pushed branch '{}' to remote '{}'", branch, remote))
    }

    /// Diff working directory against HEAD (unstaged changes).
    async fn handle_diff(&self) -> Result<String, GitToolError> {
        let (stdout, _stderr) = self.run_git("diff", &["diff"]).await?;

        if stdout.trim().is_empty() {
            Ok("No changes detected".to_string())
        } else {
            Ok(stdout)
        }
    }

    /// Return file statuses using `--porcelain` for stable, parseable output.
    async fn handle_status(&self) -> Result<String, GitToolError> {
        let (stdout, _stderr) = self.run_git("status", &["status", "--porcelain"]).await?;

        if stdout.trim().is_empty() {
            Ok("Clean working directory".to_string())
        } else {
            // Porcelain output: "XY path" — convert to simpler "X path" format
            let mut output = String::new();
            for line in stdout.lines() {
                if line.len() < 3 {
                    continue;
                }
                let status_chars = &line[..2];
                let path = line[3..].trim();

                let label = if status_chars.contains('?') || status_chars.contains('A') {
                    "A"
                } else if status_chars.contains('M') {
                    "M"
                } else if status_chars.contains('D') {
                    "D"
                } else if status_chars.contains('R') {
                    "R"
                } else {
                    "?"
                };

                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&format!("{} {}", label, path));
            }

            if output.is_empty() {
                Ok("Clean working directory".to_string())
            } else {
                Ok(output)
            }
        }
    }

    /// Return last N commit messages in oneline format.
    async fn handle_log(&self, max_count: usize) -> Result<String, GitToolError> {
        let count_arg = format!("-{}", max_count);
        let (stdout, _stderr) = self
            .run_git("log", &["log", "--oneline", &count_arg])
            .await?;

        if stdout.trim().is_empty() {
            Ok("No commits found".to_string())
        } else {
            Ok(stdout.trim_end().to_string())
        }
    }
}

impl Tool for GitTool {
    const NAME: &'static str = "git";
    type Error = GitToolError;
    type Args = GitToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "git".to_string(),
            description: "Perform git operations on the repository. Supports 9 actions: \
                'clone' (clone a remote repo), 'checkout' (switch to existing branch), \
                'branch_create' (create and checkout new branch), 'add' (stage files), \
                'commit' (commit staged changes), 'push' (push branch to remote), \
                'diff' (show unstaged changes), 'status' (show working directory status), \
                'log' (show recent commit history). \
                Use 'status' to see what files changed, 'add' then 'commit' to save changes, \
                'branch_create' for new feature branches, 'diff' to review changes before committing."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["clone", "checkout", "branch_create", "add", "commit", "push", "diff", "status", "log"],
                        "description": "The git action to perform"
                    },
                    "branch": {
                        "type": "string",
                        "description": "Branch name for checkout or branch_create actions"
                    },
                    "message": {
                        "type": "string",
                        "description": "Commit message for the commit action"
                    },
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "File paths or glob patterns for the add action (e.g. ['*'] to stage all)"
                    },
                    "url": {
                        "type": "string",
                        "description": "Remote repository URL for clone action"
                    },
                    "remote": {
                        "type": "string",
                        "description": "Remote name for push action (default: 'origin')"
                    },
                    "max_count": {
                        "type": "integer",
                        "description": "Maximum number of log entries to return (default: 10)"
                    },
                    "from_branch": {
                        "type": "string",
                        "description": "Base branch for branch_create (default: HEAD)"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::info!(action = "git", sub_action = %args.action, repo = %self.repo_path.display(), "Git tool called");

        let result = match args.action.as_str() {
            "clone" => {
                let url = args
                    .url
                    .as_deref()
                    .ok_or_else(|| GitToolError::MissingArgument {
                        action: "clone".to_string(),
                        argument: "url".to_string(),
                    })?;
                tracing::info!(action = "git_clone", url = %url, "Cloning repository");
                self.handle_clone(url).await?
            }
            "checkout" => {
                let branch =
                    args.branch
                        .as_deref()
                        .ok_or_else(|| GitToolError::MissingArgument {
                            action: "checkout".to_string(),
                            argument: "branch".to_string(),
                        })?;
                tracing::info!(action = "git_checkout", branch = %branch, "Checking out branch");
                self.handle_checkout(branch).await?
            }
            "branch_create" => {
                let branch =
                    args.branch
                        .as_deref()
                        .ok_or_else(|| GitToolError::MissingArgument {
                            action: "branch_create".to_string(),
                            argument: "branch".to_string(),
                        })?;
                tracing::info!(action = "git_branch_create", branch = %branch, "Creating branch");
                self.handle_branch_create(branch, args.from_branch.as_deref())
                    .await?
            }
            "add" => {
                let paths = args
                    .paths
                    .as_ref()
                    .ok_or_else(|| GitToolError::MissingArgument {
                        action: "add".to_string(),
                        argument: "paths".to_string(),
                    })?;
                tracing::info!(
                    action = "git_add",
                    path_count = paths.len(),
                    "Staging files"
                );
                self.handle_add(paths).await?
            }
            "commit" => {
                let message =
                    args.message
                        .as_deref()
                        .ok_or_else(|| GitToolError::MissingArgument {
                            action: "commit".to_string(),
                            argument: "message".to_string(),
                        })?;
                tracing::info!(action = "git_commit", "Creating commit");
                self.handle_commit(message).await?
            }
            "push" => {
                let branch =
                    args.branch
                        .as_deref()
                        .ok_or_else(|| GitToolError::MissingArgument {
                            action: "push".to_string(),
                            argument: "branch".to_string(),
                        })?;
                let remote = args.remote.as_deref().unwrap_or("origin");
                tracing::info!(action = "git_push", remote = %remote, branch = %branch, "Pushing to remote");
                self.handle_push(remote, branch).await?
            }
            "diff" => {
                tracing::info!(action = "git_diff", "Getting diff");
                self.handle_diff().await?
            }
            "status" => {
                tracing::info!(action = "git_status", "Getting status");
                self.handle_status().await?
            }
            "log" => {
                let max_count = args.max_count.unwrap_or(10);
                tracing::info!(action = "git_log", max_count = max_count, "Getting log");
                self.handle_log(max_count).await?
            }
            other => {
                return Err(GitToolError::InvalidAction {
                    action: other.to_string(),
                });
            }
        };

        tracing::info!(action = "git", sub_action = %args.action, "Git operation completed");
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: create a temp dir with an initialized git repo and an initial commit (CLI-based).
    fn init_repo_with_commit(dir: &std::path::Path) {
        // Initialize repo
        let output = std::process::Command::new("git")
            .args(["init", dir.to_str().unwrap()])
            .output()
            .expect("git init");
        assert!(output.status.success(), "git init failed");

        // Set identity for commits (required in CI/test environments)
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .expect("git config email");
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["config", "user.name", "Test User"])
            .output()
            .expect("git config name");

        // Rename default branch to main
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["branch", "-M", "main"])
            .output()
            .expect("git branch rename");

        // Create an initial file and commit
        let file_path = dir.join("README.md");
        fs::write(&file_path, "# Test Repo\n").unwrap();

        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["add", "."])
            .output()
            .expect("git add");

        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .expect("git commit");
        assert!(output.status.success(), "git commit failed");
    }

    #[tokio::test]
    async fn test_git_tool_definition_name() {
        let dir = TempDir::new().unwrap();
        let tool = GitTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "git");
        assert_eq!(GitTool::NAME, "git");
    }

    #[tokio::test]
    async fn test_git_tool_definition_has_detailed_description() {
        let dir = TempDir::new().unwrap();
        let tool = GitTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert!(def.description.contains("clone"));
        assert!(def.description.contains("checkout"));
        assert!(def.description.contains("commit"));
        assert!(def.description.contains("push"));
        assert!(def.description.contains("diff"));
        assert!(def.description.contains("status"));
        assert!(def.description.contains("log"));
        assert!(def.description.contains("branch_create"));
        assert!(def.description.contains("add"));
    }

    #[tokio::test]
    async fn test_git_tool_definition_action_enum() {
        let dir = TempDir::new().unwrap();
        let tool = GitTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;

        let action_prop = &def.parameters["properties"]["action"];
        let enum_values = action_prop["enum"]
            .as_array()
            .expect("action should have enum");
        assert_eq!(enum_values.len(), 9);

        let expected = [
            "clone",
            "checkout",
            "branch_create",
            "add",
            "commit",
            "push",
            "diff",
            "status",
            "log",
        ];
        for action in &expected {
            assert!(
                enum_values.iter().any(|v| v.as_str() == Some(action)),
                "Missing action '{}' in enum",
                action
            );
        }
    }

    #[test]
    fn test_git_tool_args_deserialize_minimal() {
        let json = r#"{"action": "status"}"#;
        let args: GitToolArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.action, "status");
        assert!(args.branch.is_none());
        assert!(args.message.is_none());
        assert!(args.paths.is_none());
        assert!(args.url.is_none());
        assert!(args.remote.is_none());
        assert!(args.max_count.is_none());
        assert!(args.from_branch.is_none());
    }

    #[test]
    fn test_git_tool_args_deserialize_full() {
        let json = r#"{
            "action": "commit",
            "branch": "feature/test",
            "message": "test commit",
            "paths": ["*.rs", "Cargo.toml"],
            "url": "https://github.com/test/repo.git",
            "remote": "upstream",
            "max_count": 5,
            "from_branch": "main"
        }"#;
        let args: GitToolArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.action, "commit");
        assert_eq!(args.branch.unwrap(), "feature/test");
        assert_eq!(args.message.unwrap(), "test commit");
        assert_eq!(args.paths.unwrap().len(), 2);
        assert_eq!(args.url.unwrap(), "https://github.com/test/repo.git");
        assert_eq!(args.remote.unwrap(), "upstream");
        assert_eq!(args.max_count.unwrap(), 5);
        assert_eq!(args.from_branch.unwrap(), "main");
    }

    #[test]
    fn test_git_tool_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<GitToolError>();
    }

    #[test]
    fn test_git_tool_error_display() {
        let err = GitToolError::InvalidAction {
            action: "bogus".to_string(),
        };
        assert!(err.to_string().contains("bogus"));
        assert!(err.to_string().contains("Invalid git action"));

        let err = GitToolError::CommandFailed {
            action: "commit".to_string(),
            stderr: "nothing to commit".to_string(),
            exit_code: 1,
        };
        assert!(err.to_string().contains("commit"));
        assert!(err.to_string().contains("nothing to commit"));
        assert!(err.to_string().contains("exit code 1"));

        let err = GitToolError::MissingArgument {
            action: "checkout".to_string(),
            argument: "branch".to_string(),
        };
        assert!(err.to_string().contains("checkout"));
        assert!(err.to_string().contains("branch"));

        let err = GitToolError::PathError {
            reason: "not a directory".to_string(),
        };
        assert!(err.to_string().contains("not a directory"));

        let err = GitToolError::IoError {
            reason: "command not found".to_string(),
        };
        assert!(err.to_string().contains("command not found"));
    }

    #[test]
    fn test_git_tool_serializable() {
        let tool = GitTool::new(PathBuf::from("/tmp/test-repo"));
        let json = serde_json::to_string(&tool).expect("Should serialize");
        let deserialized: GitTool = serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(deserialized.repo_path, PathBuf::from("/tmp/test-repo"));
    }

    #[tokio::test]
    async fn test_git_tool_invalid_action_returns_error() {
        let dir = TempDir::new().unwrap();
        init_repo_with_commit(dir.path());

        let tool = GitTool::new(dir.path().to_path_buf());
        let args = GitToolArgs {
            action: "rebase".to_string(),
            branch: None,
            message: None,
            paths: None,
            url: None,
            remote: None,
            max_count: None,
            from_branch: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            GitToolError::InvalidAction { .. }
        ));
    }

    #[tokio::test]
    async fn test_git_tool_missing_branch_for_checkout() {
        let dir = TempDir::new().unwrap();
        init_repo_with_commit(dir.path());

        let tool = GitTool::new(dir.path().to_path_buf());
        let args = GitToolArgs {
            action: "checkout".to_string(),
            branch: None,
            message: None,
            paths: None,
            url: None,
            remote: None,
            max_count: None,
            from_branch: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            GitToolError::MissingArgument { .. }
        ));
    }

    #[tokio::test]
    async fn test_git_tool_missing_message_for_commit() {
        let dir = TempDir::new().unwrap();
        init_repo_with_commit(dir.path());

        let tool = GitTool::new(dir.path().to_path_buf());
        let args = GitToolArgs {
            action: "commit".to_string(),
            branch: None,
            message: None,
            paths: None,
            url: None,
            remote: None,
            max_count: None,
            from_branch: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            GitToolError::MissingArgument { .. }
        ));
    }

    #[tokio::test]
    async fn test_git_tool_init_status_on_new_repo() {
        let dir = TempDir::new().unwrap();
        init_repo_with_commit(dir.path());

        let tool = GitTool::new(dir.path().to_path_buf());
        let args = GitToolArgs {
            action: "status".to_string(),
            branch: None,
            message: None,
            paths: None,
            url: None,
            remote: None,
            max_count: None,
            from_branch: None,
        };
        let result = tool.call(args).await.unwrap();
        assert_eq!(result, "Clean working directory");
    }

    #[tokio::test]
    async fn test_git_tool_add_commit_log_roundtrip() {
        let dir = TempDir::new().unwrap();
        init_repo_with_commit(dir.path());

        // Create a new file
        fs::write(dir.path().join("new_file.txt"), "hello world").unwrap();

        let tool = GitTool::new(dir.path().to_path_buf());

        // Add
        let add_args = GitToolArgs {
            action: "add".to_string(),
            branch: None,
            message: None,
            paths: Some(vec!["*".to_string()]),
            url: None,
            remote: None,
            max_count: None,
            from_branch: None,
        };
        let add_result = tool.call(add_args).await.unwrap();
        assert!(add_result.contains("Staged"));

        // Commit
        let commit_args = GitToolArgs {
            action: "commit".to_string(),
            branch: None,
            message: Some("feat: add new file".to_string()),
            paths: None,
            url: None,
            remote: None,
            max_count: None,
            from_branch: None,
        };
        let commit_result = tool.call(commit_args).await.unwrap();
        assert!(commit_result.contains("Committed"));
        assert!(commit_result.contains("feat: add new file"));

        // Log
        let log_args = GitToolArgs {
            action: "log".to_string(),
            branch: None,
            message: None,
            paths: None,
            url: None,
            remote: None,
            max_count: Some(5),
            from_branch: None,
        };
        let log_result = tool.call(log_args).await.unwrap();
        assert!(log_result.contains("feat: add new file"));
        assert!(log_result.contains("Initial commit"));
        // Should have 2 lines (2 commits)
        assert_eq!(log_result.lines().count(), 2);
    }

    #[tokio::test]
    async fn test_git_tool_branch_create_and_checkout() {
        let dir = TempDir::new().unwrap();
        init_repo_with_commit(dir.path());

        let tool = GitTool::new(dir.path().to_path_buf());

        // Create branch
        let create_args = GitToolArgs {
            action: "branch_create".to_string(),
            branch: Some("feature/test-branch".to_string()),
            message: None,
            paths: None,
            url: None,
            remote: None,
            max_count: None,
            from_branch: None,
        };
        let result = tool.call(create_args).await.unwrap();
        assert!(result.contains("Created and checked out branch 'feature/test-branch'"));

        // Verify HEAD points to new branch using git CLI
        let head_output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .expect("git rev-parse");
        let current_branch = String::from_utf8_lossy(&head_output.stdout)
            .trim()
            .to_string();
        assert_eq!(current_branch, "feature/test-branch");
    }

    #[tokio::test]
    async fn test_git_tool_diff_shows_changes() {
        let dir = TempDir::new().unwrap();
        init_repo_with_commit(dir.path());

        // Modify the existing file
        fs::write(dir.path().join("README.md"), "# Modified\nNew content\n").unwrap();

        let tool = GitTool::new(dir.path().to_path_buf());
        let args = GitToolArgs {
            action: "diff".to_string(),
            branch: None,
            message: None,
            paths: None,
            url: None,
            remote: None,
            max_count: None,
            from_branch: None,
        };
        let result = tool.call(args).await.unwrap();
        assert_ne!(result, "No changes detected");
        // The diff should show changes related to the README
        assert!(result.contains("Modified") || result.contains("New content"));
    }
}
