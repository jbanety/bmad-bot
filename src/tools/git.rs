//! Git operations tool — exposes git (branch, checkout, commit, push) to the rig agent via `git2`.
//!
//! Implements the rig `Tool` trait with 9 git actions: clone, checkout, branch_create,
//! add, commit, push, diff, status, log. All operations use `git2` (libgit2 bindings).
//! Network operations (clone, push) are wrapped in `tokio::task::spawn_blocking`.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

/// Git operations tool for the rig agent.
///
/// Exposes 9 git actions via `git2`: clone, checkout, branch_create, add, commit,
/// push, diff, status, log. The struct holds only configuration — the repository
/// is opened fresh on each `call()` invocation for `Serialize`/`Deserialize` and
/// `Send + Sync` safety.
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

    /// Wraps git2 errors with action context.
    #[error("Git {action} failed: {reason}")]
    GitError {
        /// The git action that failed.
        action: String,
        /// Description of the git2 error.
        reason: String,
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

    /// `spawn_blocking` join failure.
    #[error("Task join error: {reason}")]
    TaskJoinError {
        /// Description of the join error.
        reason: String,
    },
}

impl GitTool {
    /// Create a new `GitTool` operating on the repository at `repo_path`.
    pub fn new(repo_path: PathBuf) -> Self {
        Self { repo_path }
    }

    /// Open the git repository at `self.repo_path`.
    fn open_repo(&self) -> Result<git2::Repository, GitToolError> {
        git2::Repository::open(&self.repo_path).map_err(|e| GitToolError::GitError {
            action: "open".to_string(),
            reason: e.to_string(),
        })
    }

    /// Clone a remote repository to `self.repo_path`.
    fn handle_clone(&self, url: &str) -> Result<String, GitToolError> {
        let url = url.to_string();
        let path = self.repo_path.clone();
        git2::Repository::clone(&url, &path).map_err(|e| GitToolError::GitError {
            action: "clone".to_string(),
            reason: e.to_string(),
        })?;
        Ok(format!("Cloned {} to {}", url, path.display()))
    }

    /// Checkout an existing branch.
    fn handle_checkout(&self, branch: &str) -> Result<String, GitToolError> {
        let repo = self.open_repo()?;

        // Resolve the branch reference
        let (object, reference) =
            repo.revparse_ext(branch)
                .map_err(|e| GitToolError::GitError {
                    action: "checkout".to_string(),
                    reason: format!("Cannot resolve '{}': {}", branch, e),
                })?;

        repo.checkout_tree(&object, None)
            .map_err(|e| GitToolError::GitError {
                action: "checkout".to_string(),
                reason: e.to_string(),
            })?;

        match reference {
            Some(r) => {
                if let Some(name) = r.name() {
                    repo.set_head(name).map_err(|e| GitToolError::GitError {
                        action: "checkout".to_string(),
                        reason: e.to_string(),
                    })?;
                }
            }
            None => {
                // Detached HEAD
                repo.set_head_detached(object.id())
                    .map_err(|e| GitToolError::GitError {
                        action: "checkout".to_string(),
                        reason: e.to_string(),
                    })?;
            }
        }

        Ok(format!("Checked out branch '{}'", branch))
    }

    /// Create a new branch from HEAD or a specified base, then checkout.
    fn handle_branch_create(
        &self,
        branch: &str,
        from_branch: Option<&str>,
    ) -> Result<String, GitToolError> {
        let repo = self.open_repo()?;

        // Find the base commit
        let base_name = from_branch.unwrap_or("HEAD");
        let base_obj = repo
            .revparse_single(base_name)
            .map_err(|e| GitToolError::GitError {
                action: "branch_create".to_string(),
                reason: format!("Cannot resolve '{}': {}", base_name, e),
            })?;

        let base_commit = base_obj
            .peel_to_commit()
            .map_err(|e| GitToolError::GitError {
                action: "branch_create".to_string(),
                reason: format!("Cannot peel to commit: {}", e),
            })?;

        // Create the branch
        repo.branch(branch, &base_commit, false)
            .map_err(|e| GitToolError::GitError {
                action: "branch_create".to_string(),
                reason: e.to_string(),
            })?;

        // Checkout the new branch
        let refname = format!("refs/heads/{}", branch);
        let obj = repo
            .revparse_single(&refname)
            .map_err(|e| GitToolError::GitError {
                action: "branch_create".to_string(),
                reason: format!("Cannot resolve new branch: {}", e),
            })?;

        repo.checkout_tree(&obj, None)
            .map_err(|e| GitToolError::GitError {
                action: "branch_create".to_string(),
                reason: e.to_string(),
            })?;

        repo.set_head(&refname)
            .map_err(|e| GitToolError::GitError {
                action: "branch_create".to_string(),
                reason: e.to_string(),
            })?;

        Ok(format!(
            "Created and checked out branch '{}' from {}",
            branch, base_name
        ))
    }

    /// Stage files via index.
    fn handle_add(&self, paths: &[String]) -> Result<String, GitToolError> {
        let repo = self.open_repo()?;
        let mut index = repo.index().map_err(|e| GitToolError::GitError {
            action: "add".to_string(),
            reason: e.to_string(),
        })?;

        index
            .add_all(paths.iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| GitToolError::GitError {
                action: "add".to_string(),
                reason: e.to_string(),
            })?;

        index.write().map_err(|e| GitToolError::GitError {
            action: "add".to_string(),
            reason: e.to_string(),
        })?;

        Ok(format!("Staged {} path pattern(s)", paths.len()))
    }

    /// Create a commit on the current branch with staged changes.
    fn handle_commit(&self, message: &str) -> Result<String, GitToolError> {
        let repo = self.open_repo()?;

        let sig = repo
            .signature()
            .or_else(|_| git2::Signature::now("bmad-bot", "bmad-bot@localhost"))
            .map_err(|e| GitToolError::GitError {
                action: "commit".to_string(),
                reason: format!("Cannot create signature: {}", e),
            })?;

        let mut index = repo.index().map_err(|e| GitToolError::GitError {
            action: "commit".to_string(),
            reason: e.to_string(),
        })?;

        let tree_oid = index.write_tree().map_err(|e| GitToolError::GitError {
            action: "commit".to_string(),
            reason: e.to_string(),
        })?;

        let tree = repo
            .find_tree(tree_oid)
            .map_err(|e| GitToolError::GitError {
                action: "commit".to_string(),
                reason: e.to_string(),
            })?;

        // Get parent commit (if any — first commit has no parent)
        let parent = match repo.head() {
            Ok(head) => {
                let target = head.target().ok_or_else(|| GitToolError::GitError {
                    action: "commit".to_string(),
                    reason: "HEAD has no target".to_string(),
                })?;
                Some(
                    repo.find_commit(target)
                        .map_err(|e| GitToolError::GitError {
                            action: "commit".to_string(),
                            reason: e.to_string(),
                        })?,
                )
            }
            Err(_) => None,
        };

        let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();

        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .map_err(|e| GitToolError::GitError {
                action: "commit".to_string(),
                reason: e.to_string(),
            })?;

        let short_sha = &oid.to_string()[..7.min(oid.to_string().len())];
        Ok(format!("Committed {}: {}", short_sha, message))
    }

    /// Push a branch to a remote.
    fn handle_push(&self, remote: &str, branch: &str) -> Result<String, GitToolError> {
        let repo = self.open_repo()?;

        let mut remote_obj = repo
            .find_remote(remote)
            .map_err(|e| GitToolError::GitError {
                action: "push".to_string(),
                reason: format!("Cannot find remote '{}': {}", remote, e),
            })?;

        let refspec = format!("refs/heads/{}:refs/heads/{}", branch, branch);

        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(|_url, username, allowed_types| {
            if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                return git2::Cred::ssh_key_from_agent(username.unwrap_or("git"));
            }
            if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
                let config = git2::Config::open_default()?;
                return git2::Cred::credential_helper(&config, _url, username);
            }
            Err(git2::Error::from_str("no suitable credentials found"))
        });

        let mut push_options = git2::PushOptions::new();
        push_options.remote_callbacks(callbacks);

        remote_obj
            .push(&[&refspec], Some(&mut push_options))
            .map_err(|e| GitToolError::GitError {
                action: "push".to_string(),
                reason: e.to_string(),
            })?;

        Ok(format!("Pushed branch '{}' to remote '{}'", branch, remote))
    }

    /// Diff working directory against HEAD (unstaged changes).
    fn handle_diff(&self) -> Result<String, GitToolError> {
        let repo = self.open_repo()?;

        let diff = repo
            .diff_index_to_workdir(None, None)
            .map_err(|e| GitToolError::GitError {
                action: "diff".to_string(),
                reason: e.to_string(),
            })?;

        let mut diff_output = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let origin = line.origin();
            if origin == '+' || origin == '-' || origin == ' ' {
                diff_output.push(origin);
            }
            if let Ok(content) = std::str::from_utf8(line.content()) {
                diff_output.push_str(content);
            }
            true
        })
        .map_err(|e| GitToolError::GitError {
            action: "diff".to_string(),
            reason: e.to_string(),
        })?;

        if diff_output.is_empty() {
            Ok("No changes detected".to_string())
        } else {
            Ok(diff_output)
        }
    }

    /// Return file statuses as formatted text.
    fn handle_status(&self) -> Result<String, GitToolError> {
        let repo = self.open_repo()?;

        let statuses = repo.statuses(None).map_err(|e| GitToolError::GitError {
            action: "status".to_string(),
            reason: e.to_string(),
        })?;

        if statuses.is_empty() {
            return Ok("Clean working directory".to_string());
        }

        let mut output = String::new();
        for entry in statuses.iter() {
            let status = entry.status();
            let path = entry.path().unwrap_or("<invalid utf-8>");

            let label = if status.contains(git2::Status::WT_NEW)
                || status.contains(git2::Status::INDEX_NEW)
            {
                "A"
            } else if status.contains(git2::Status::WT_MODIFIED)
                || status.contains(git2::Status::INDEX_MODIFIED)
            {
                "M"
            } else if status.contains(git2::Status::WT_DELETED)
                || status.contains(git2::Status::INDEX_DELETED)
            {
                "D"
            } else if status.contains(git2::Status::WT_RENAMED)
                || status.contains(git2::Status::INDEX_RENAMED)
            {
                "R"
            } else {
                "?"
            };

            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&format!("{} {}", label, path));
        }

        Ok(output)
    }

    /// Return last N commit messages with short SHA and author.
    fn handle_log(&self, max_count: usize) -> Result<String, GitToolError> {
        let repo = self.open_repo()?;

        let mut revwalk = repo.revwalk().map_err(|e| GitToolError::GitError {
            action: "log".to_string(),
            reason: e.to_string(),
        })?;

        revwalk.push_head().map_err(|e| GitToolError::GitError {
            action: "log".to_string(),
            reason: e.to_string(),
        })?;

        revwalk
            .set_sorting(git2::Sort::TIME)
            .map_err(|e| GitToolError::GitError {
                action: "log".to_string(),
                reason: e.to_string(),
            })?;

        let mut output = String::new();

        for (count, oid_result) in revwalk.enumerate() {
            if count >= max_count {
                break;
            }
            let oid = oid_result.map_err(|e| GitToolError::GitError {
                action: "log".to_string(),
                reason: e.to_string(),
            })?;

            let commit = repo.find_commit(oid).map_err(|e| GitToolError::GitError {
                action: "log".to_string(),
                reason: e.to_string(),
            })?;

            let short_sha = &oid.to_string()[..7.min(oid.to_string().len())];
            let author = commit.author();
            let author_name = author.name().unwrap_or("<unknown>");
            let message = commit.summary().unwrap_or("<no message>");
            let time = commit.time();
            let timestamp = chrono::DateTime::from_timestamp(time.seconds(), 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "unknown-date".to_string());

            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&format!(
                "{} | {} | {} | {}",
                short_sha, author_name, timestamp, message
            ));
        }

        if output.is_empty() {
            Ok("No commits found".to_string())
        } else {
            Ok(output)
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
                let repo_path = self.repo_path.clone();
                let url_owned = url.to_string();
                let tool = GitTool::new(repo_path);
                tokio::task::spawn_blocking(move || tool.handle_clone(&url_owned))
                    .await
                    .map_err(|e| GitToolError::TaskJoinError {
                        reason: e.to_string(),
                    })??
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
                self.handle_checkout(branch)?
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
                self.handle_branch_create(branch, args.from_branch.as_deref())?
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
                self.handle_add(paths)?
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
                self.handle_commit(message)?
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
                let repo_path = self.repo_path.clone();
                let remote_owned = remote.to_string();
                let branch_owned = branch.to_string();
                let tool = GitTool::new(repo_path);
                tokio::task::spawn_blocking(move || tool.handle_push(&remote_owned, &branch_owned))
                    .await
                    .map_err(|e| GitToolError::TaskJoinError {
                        reason: e.to_string(),
                    })??
            }
            "diff" => {
                tracing::info!(action = "git_diff", "Getting diff");
                self.handle_diff()?
            }
            "status" => {
                tracing::info!(action = "git_status", "Getting status");
                self.handle_status()?
            }
            "log" => {
                let max_count = args.max_count.unwrap_or(10);
                tracing::info!(action = "git_log", max_count = max_count, "Getting log");
                self.handle_log(max_count)?
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

    /// Helper: create a temp dir with an initialized git repo and an initial commit.
    fn init_repo_with_commit(dir: &std::path::Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();

        // Configure user for commits
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();

        // Create an initial file and commit
        let file_path = dir.join("README.md");
        fs::write(&file_path, "# Test Repo\n").unwrap();

        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();

        let tree_oid = index.write_tree().unwrap();
        {
            let tree = repo.find_tree(tree_oid).unwrap();
            let sig = repo.signature().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        repo
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

        let err = GitToolError::GitError {
            action: "commit".to_string(),
            reason: "nothing to commit".to_string(),
        };
        assert!(err.to_string().contains("commit"));
        assert!(err.to_string().contains("nothing to commit"));

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

        let err = GitToolError::TaskJoinError {
            reason: "task panicked".to_string(),
        };
        assert!(err.to_string().contains("task panicked"));
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

        // Verify HEAD points to new branch
        let repo = git2::Repository::open(dir.path()).unwrap();
        let head = repo.head().unwrap();
        assert!(head.is_branch());
        assert_eq!(head.shorthand().unwrap(), "feature/test-branch");
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
