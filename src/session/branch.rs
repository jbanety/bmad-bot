//! Branch management — dependency-aware branch chaining for story sessions.
//!
//! Before launching a rig agent session, the daemon must ensure the repository
//! is on the correct branch. This module provides:
//!
//! - [`determine_base_branch`] — resolves which branch to create the story branch from
//!   (chains from dependency branch if it exists locally, otherwise falls back to default)
//! - [`ensure_story_branch`] — creates or reuses the story branch and checks it out
//!
//! These functions are **synchronous** (for use with `spawn_blocking`). The caller
//! in `SessionRunner::run()` MUST wrap them in `tokio::task::spawn_blocking()`.

use crate::watcher::StoryInfo;
use std::path::Path;

/// Errors originating from branch operations.
///
/// Implements `std::error::Error + Send + Sync` via `thiserror`.
#[derive(Debug, thiserror::Error)]
pub enum BranchError {
    /// Failed to create a new branch.
    #[error("Failed to create branch {branch}: {reason}")]
    CreationFailed {
        /// The branch that could not be created.
        branch: String,
        /// Description of the creation failure.
        reason: String,
    },

    /// Failed to checkout a branch.
    #[error("Failed to checkout branch {branch}: {reason}")]
    CheckoutFailed {
        /// The branch that could not be checked out.
        branch: String,
        /// Description of the checkout failure.
        reason: String,
    },

    /// The specified base branch does not exist locally.
    #[error("Base branch not found: {branch}")]
    BaseBranchNotFound {
        /// The base branch that was not found.
        branch: String,
    },

    /// A git CLI command failed.
    #[error("Git command failed: {command}: {stderr}")]
    CommandFailed {
        /// The command that failed.
        command: String,
        /// The stderr output from the command.
        stderr: String,
    },
}

/// Result of a branch setup operation.
///
/// Returned by [`ensure_story_branch`] to indicate whether a new branch was
/// created or an existing one was reused.
#[derive(Debug)]
pub enum BranchAction {
    /// A new branch was created from the specified base.
    Created {
        /// Name of the newly created branch.
        branch_name: String,
        /// Name of the base branch it was created from.
        base_branch: String,
    },
    /// An existing branch was checked out.
    Reused {
        /// Name of the reused branch.
        branch_name: String,
    },
}

/// Check if a local branch exists using `git branch --list`.
///
/// Returns `true` if the branch exists locally, `false` otherwise.
/// On any CLI error, returns `false` (safe fallback).
fn branch_exists(repo_path: &Path, branch_name: &str) -> bool {
    match std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["branch", "--list", branch_name])
        .output()
    {
        Ok(output) => output.status.success() && !output.stdout.is_empty(),
        Err(_) => false,
    }
}

/// Determine which base branch to create the story branch from.
///
/// Resolution logic:
/// 1. If the story has no dependencies → `default_branch`
/// 2. Take the **last** dependency key (most recent predecessor)
/// 3. Parse the epic number from that dependency key
/// 4. If the dependency belongs to a **different epic** (inter-epic dep) → `default_branch`
///    always. An inter-epic dependency means the predecessor epic is fully merged into
///    `default_branch` by the time the gate is cleared — forking from a story branch
///    would inherit a stale sprint-status (e.g. `epic-N-retrospective: review`) and
///    block all subsequent epics on the next poll cycle.
/// 5. If the dependency belongs to the **same epic** (intra-epic dep) → check if
///    `story/{last_dep_key}` exists locally and chain from it; otherwise `default_branch`.
///
/// # Arguments
/// * `story` — The story being developed (contains `dependencies` from watcher)
/// * `repo_path` — Path to the git repository root
/// * `default_branch` — Fallback branch name (typically `"main"`)
pub fn determine_base_branch(story: &StoryInfo, repo_path: &Path, default_branch: &str) -> String {
    if story.dependencies.is_empty() {
        tracing::info!(
            action = "base_branch_resolved",
            base = %default_branch,
            story = %story.story_key,
            reason = "no dependencies",
            "Using default branch as base"
        );
        return default_branch.to_string();
    }

    // Take the last dependency — the most recent predecessor in the chain
    let last_dep = &story.dependencies[story.dependencies.len() - 1];

    // Parse the epic number from the dependency key (format: "{epic_num}-{story_num}-{slug}")
    let dep_epic_num: Option<u32> = last_dep.splitn(2, '-').next().and_then(|s| s.parse().ok());

    // Inter-epic dependency: the predecessor epic is fully merged into default_branch.
    // Always fork from default_branch — never from a story branch of another epic.
    if dep_epic_num.map_or(false, |dep_epic| dep_epic != story.epic_num) {
        tracing::info!(
            action = "base_branch_resolved",
            base = %default_branch,
            story = %story.story_key,
            dependency = %last_dep,
            reason = "inter-epic dependency — predecessor epic merged into default branch",
            "Using default branch as base (inter-epic dep)"
        );
        return default_branch.to_string();
    }

    // Intra-epic dependency: chain from the predecessor story branch if it exists locally.
    let candidate = format!("story/{last_dep}");
    let exists = branch_exists(repo_path, &candidate);

    if exists {
        tracing::info!(
            action = "base_branch_resolved",
            base = %candidate,
            story = %story.story_key,
            dependency = %last_dep,
            reason = "intra-epic dependency branch exists locally",
            "Chaining from dependency branch"
        );
        candidate
    } else {
        tracing::info!(
            action = "base_branch_resolved",
            base = %default_branch,
            story = %story.story_key,
            dependency = %last_dep,
            reason = "intra-epic dependency branch not found locally (likely merged)",
            "Falling back to default branch"
        );
        default_branch.to_string()
    }
}

/// Ensure the story branch exists and is checked out.
///
/// - If the branch already exists, checks it out and returns [`BranchAction::Reused`].
/// - If the branch does not exist, creates it from `base_branch` and returns [`BranchAction::Created`].
///
/// This function is **synchronous**. The caller MUST wrap it in
/// `tokio::task::spawn_blocking()` to avoid blocking the async runtime.
///
/// # Arguments
/// * `repo_path` — Path to the git repository root
/// * `branch_name` — The story branch name (e.g., `"story/4-3-branch-mgmt"`)
/// * `base_branch` — The branch to create from if `branch_name` doesn't exist
///
/// # Errors
/// Returns [`BranchError`] if the base branch is missing, or branch creation/checkout fails.
pub fn ensure_story_branch(
    repo_path: &Path,
    branch_name: &str,
    base_branch: &str,
) -> Result<BranchAction, BranchError> {
    // Check if the branch already exists
    if branch_exists(repo_path, branch_name) {
        // Branch exists — check it out
        checkout_branch(repo_path, branch_name)?;

        tracing::info!(
            action = "branch_reuse",
            branch = %branch_name,
            "Reusing existing story branch"
        );

        return Ok(BranchAction::Reused {
            branch_name: branch_name.to_string(),
        });
    }

    // Verify the base branch exists before trying to create from it
    if !branch_exists(repo_path, base_branch) {
        return Err(BranchError::BaseBranchNotFound {
            branch: base_branch.to_string(),
        });
    }

    // Branch does not exist — create from base and checkout
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["checkout", "-b", branch_name, base_branch])
        .output()
        .map_err(|e| BranchError::CreationFailed {
            branch: branch_name.to_string(),
            reason: format!("Failed to execute git: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BranchError::CreationFailed {
            branch: branch_name.to_string(),
            reason: stderr.to_string(),
        });
    }

    tracing::info!(
        action = "branch_created",
        branch = %branch_name,
        base = %base_branch,
        "Created new story branch"
    );

    Ok(BranchAction::Created {
        branch_name: branch_name.to_string(),
        base_branch: base_branch.to_string(),
    })
}

/// Set HEAD to the given branch and update the working directory.
fn checkout_branch(repo_path: &Path, branch_name: &str) -> Result<(), BranchError> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["checkout", branch_name])
        .output()
        .map_err(|e| BranchError::CheckoutFailed {
            branch: branch_name.to_string(),
            reason: format!("Failed to execute git: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BranchError::CheckoutFailed {
            branch: branch_name.to_string(),
            reason: stderr.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Create a temporary git repo with an initial commit on `main` (CLI-based).
    fn init_test_repo() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();

        // Initialize repo
        let output = std::process::Command::new("git")
            .args(["init", path.to_str().unwrap()])
            .output()
            .expect("git init");
        assert!(output.status.success(), "git init failed");

        // Set identity for commits (required in CI/test environments)
        std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .expect("git config email");
        std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["config", "user.name", "Test"])
            .output()
            .expect("git config name");

        // Rename default branch to main
        std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["branch", "-M", "main"])
            .output()
            .expect("git branch rename");

        // Create initial empty commit (branch operations need at least one commit)
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["commit", "--allow-empty", "-m", "initial commit"])
            .output()
            .expect("git commit");
        assert!(output.status.success(), "git commit failed");

        (dir, path)
    }

    /// Create a minimal `StoryInfo` for tests.
    fn make_story(key: &str, deps: Vec<&str>) -> StoryInfo {
        let parts: Vec<&str> = key.splitn(3, '-').collect();
        let epic_num: u32 = parts.first().and_then(|p| p.parse().ok()).unwrap_or(1);
        let story_num: u32 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(1);
        let label = parts.get(2).unwrap_or(&"test").to_string();

        StoryInfo {
            story_id: format!("{epic_num}.{story_num}"),
            story_key: key.to_string(),
            epic_num,
            story_num,
            label,
            branch_name: format!("story/{key}"),
            specs_path: PathBuf::from(format!("_bmad-output/implementation-artifacts/{key}.md")),
            dependencies: deps.into_iter().map(String::from).collect(),
            status: "in-progress".to_string(),
        }
    }

    /// Helper: create a branch with a commit in a test repo (CLI-based).
    fn create_branch_with_commit(repo_path: &Path, branch_name: &str) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["branch", branch_name])
            .output()
            .expect("git branch create");
        assert!(
            output.status.success(),
            "git branch create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // -----------------------------------------------------------------------
    // determine_base_branch tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_determine_base_branch_no_deps_returns_default() {
        let (_dir, path) = init_test_repo();
        let story = make_story("1-1-scaffolding", vec![]);

        let base = determine_base_branch(&story, &path, "main");
        assert_eq!(base, "main");
    }

    #[test]
    fn test_determine_base_branch_dep_branch_exists_returns_parent() {
        let (_dir, path) = init_test_repo();

        // Create the dependency branch
        create_branch_with_commit(&path, "story/4-1-rig-tools");

        let story = make_story("4-2-session-setup", vec!["4-1-rig-tools"]);

        let base = determine_base_branch(&story, &path, "main");
        assert_eq!(base, "story/4-1-rig-tools");
    }

    #[test]
    fn test_determine_base_branch_dep_branch_missing_returns_default() {
        let (_dir, path) = init_test_repo();

        // Dependency exists in StoryInfo but branch is NOT in repo
        let story = make_story("4-2-session-setup", vec!["4-1-rig-tools"]);

        let base = determine_base_branch(&story, &path, "main");
        assert_eq!(base, "main");
    }

    #[test]
    fn test_determine_base_branch_inter_epic_dep_returns_default_even_if_branch_exists() {
        let (_dir, path) = init_test_repo();

        // Story 2-1 depends on 1-5 (different epic) — branch exists locally
        create_branch_with_commit(&path, "story/1-5-structured-logging-bootstrap");

        let story = make_story(
            "2-1-bingx-rest-client",
            vec!["1-5-structured-logging-bootstrap"],
        );

        let base = determine_base_branch(&story, &path, "main");
        // Inter-epic dep → always default_branch, even though story/1-5-... exists
        assert_eq!(base, "main");
    }

    #[test]
    fn test_determine_base_branch_inter_epic_dep_returns_default_when_branch_missing() {
        let (_dir, path) = init_test_repo();

        // Story 2-1 depends on 1-5 (different epic) — branch does NOT exist locally
        let story = make_story(
            "2-1-bingx-rest-client",
            vec!["1-5-structured-logging-bootstrap"],
        );

        let base = determine_base_branch(&story, &path, "main");
        assert_eq!(base, "main");
    }

    #[test]
    fn test_determine_base_branch_inter_epic_range_dep_returns_default() {
        let (_dir, path) = init_test_repo();

        // Story 5-1 depends on last story of epic 2 and epic 4 (both different epics)
        create_branch_with_commit(&path, "story/2-5-live-tick-data-recorder");
        create_branch_with_commit(&path, "story/4-5-state-reconciliation-on-restart");

        let story = make_story(
            "5-1-backtest-executor",
            vec![
                "2-5-live-tick-data-recorder",
                "4-5-state-reconciliation-on-restart",
            ],
        );

        let base = determine_base_branch(&story, &path, "main");
        // Last dep is epic 4, story is epic 5 — inter-epic → default_branch
        assert_eq!(base, "main");
    }

    #[test]
    fn test_determine_base_branch_intra_epic_dep_still_chains_from_story_branch() {
        let (_dir, path) = init_test_repo();

        // Story 4-3 depends on 4-2 (same epic) — branch exists locally
        create_branch_with_commit(&path, "story/4-2-session-setup");

        let story = make_story("4-3-branch-mgmt", vec!["4-2-session-setup"]);

        let base = determine_base_branch(&story, &path, "main");
        // Intra-epic dep → chain from story branch as before
        assert_eq!(base, "story/4-2-session-setup");
    }

    #[test]
    fn test_determine_base_branch_uses_last_dependency() {
        let (_dir, path) = init_test_repo();

        // Create only the second dep branch
        create_branch_with_commit(&path, "story/4-2-session-setup");

        let story = make_story(
            "4-3-branch-mgmt",
            vec!["4-1-rig-tools", "4-2-session-setup"],
        );

        let base = determine_base_branch(&story, &path, "main");
        // Should check the LAST dep (4-2), which exists
        assert_eq!(base, "story/4-2-session-setup");
    }

    // -----------------------------------------------------------------------
    // ensure_story_branch tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ensure_story_branch_creates_new_from_main() {
        let (_dir, path) = init_test_repo();

        let result =
            ensure_story_branch(&path, "story/1-1-scaffolding", "main").expect("should succeed");

        match result {
            BranchAction::Created {
                branch_name,
                base_branch,
            } => {
                assert_eq!(branch_name, "story/1-1-scaffolding");
                assert_eq!(base_branch, "main");
            }
            BranchAction::Reused { .. } => panic!("Expected Created, got Reused"),
        }

        // Verify HEAD is on the new branch using git CLI
        let head_output = std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .expect("git rev-parse");
        let current_branch = String::from_utf8_lossy(&head_output.stdout)
            .trim()
            .to_string();
        assert_eq!(current_branch, "story/1-1-scaffolding");
    }

    #[test]
    fn test_ensure_story_branch_creates_from_parent_branch() {
        let (_dir, path) = init_test_repo();

        // Create parent branch
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["checkout", "-b", "story/4-1-rig-tools"])
            .output()
            .expect("git checkout -b");
        assert!(output.status.success());

        // Add a commit on the parent branch
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["commit", "--allow-empty", "-m", "parent branch commit"])
            .output()
            .expect("git commit");
        assert!(output.status.success());

        // Get parent commit SHA for later comparison
        let parent_sha_output = std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse");
        let parent_sha = String::from_utf8_lossy(&parent_sha_output.stdout)
            .trim()
            .to_string();

        let result = ensure_story_branch(&path, "story/4-2-session-setup", "story/4-1-rig-tools")
            .expect("should succeed");

        match result {
            BranchAction::Created {
                branch_name,
                base_branch,
            } => {
                assert_eq!(branch_name, "story/4-2-session-setup");
                assert_eq!(base_branch, "story/4-1-rig-tools");
            }
            BranchAction::Reused { .. } => panic!("Expected Created, got Reused"),
        }

        // Verify child branch has the parent's commit
        let child_sha_output = std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse");
        let child_sha = String::from_utf8_lossy(&child_sha_output.stdout)
            .trim()
            .to_string();
        assert_eq!(child_sha, parent_sha);
    }

    #[test]
    fn test_ensure_story_branch_reuses_existing() {
        let (_dir, path) = init_test_repo();

        // First call — creates the branch
        let first = ensure_story_branch(&path, "story/2-1-polling", "main")
            .expect("first call should succeed");
        assert!(matches!(first, BranchAction::Created { .. }));

        // Second call — should reuse the existing branch
        let second = ensure_story_branch(&path, "story/2-1-polling", "main")
            .expect("second call should succeed");
        match second {
            BranchAction::Reused { branch_name } => {
                assert_eq!(branch_name, "story/2-1-polling");
            }
            BranchAction::Created { .. } => panic!("Expected Reused, got Created"),
        }
    }

    #[test]
    fn test_ensure_story_branch_base_not_found_returns_error() {
        let (_dir, path) = init_test_repo();

        let result = ensure_story_branch(&path, "story/1-1-scaffolding", "nonexistent-branch");

        assert!(result.is_err());
        match result.unwrap_err() {
            BranchError::BaseBranchNotFound { branch } => {
                assert_eq!(branch, "nonexistent-branch");
            }
            other => panic!("Expected BaseBranchNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_ensure_story_branch_invalid_repo_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        // dir.path() exists but is NOT a git repo

        let result = ensure_story_branch(dir.path(), "story/1-1-scaffolding", "main");

        assert!(result.is_err());
        // With CLI-based approach, this will fail at branch_exists (returns false for base)
        // leading to BaseBranchNotFound since git commands fail on non-repo dirs
        match result.unwrap_err() {
            BranchError::BaseBranchNotFound { .. } => {
                // Expected — branch_exists returns false for non-repo dir
            }
            other => panic!("Expected BaseBranchNotFound, got: {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // BranchError type tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_branch_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BranchError>();
    }

    #[test]
    fn test_branch_error_display_messages() {
        let err = BranchError::CreationFailed {
            branch: "story/test".to_string(),
            reason: "conflict".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("story/test"));
        assert!(display.contains("conflict"));

        let err = BranchError::CheckoutFailed {
            branch: "story/test".to_string(),
            reason: "dirty tree".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("story/test"));
        assert!(display.contains("dirty tree"));

        let err = BranchError::BaseBranchNotFound {
            branch: "develop".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("develop"));

        let err = BranchError::CommandFailed {
            command: "git checkout".to_string(),
            stderr: "error: pathspec 'x' did not match".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("git checkout"));
        assert!(display.contains("pathspec"));
    }
}
