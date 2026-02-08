//! Branch management — dependency-aware branch chaining for story sessions.
//!
//! Before launching a rig agent session, the daemon must ensure the repository
//! is on the correct branch. This module provides:
//!
//! - [`determine_base_branch`] — resolves which branch to create the story branch from
//!   (chains from dependency branch if it exists locally, otherwise falls back to default)
//! - [`ensure_story_branch`] — creates or reuses the story branch and checks it out
//!
//! These functions are **synchronous** (git2 is a blocking C library). The caller
//! in `SessionRunner::run()` MUST wrap them in `tokio::task::spawn_blocking()`.

use crate::watcher::StoryInfo;
use git2::{BranchType, Repository, build::CheckoutBuilder};
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

    /// Failed to open the git repository.
    #[error("Failed to open repo at {path}: {reason}")]
    RepoOpenFailed {
        /// Path to the repository that could not be opened.
        path: String,
        /// Description of the open failure.
        reason: String,
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

/// Determine which base branch to create the story branch from.
///
/// Resolution logic:
/// 1. If the story has dependencies, take the **last** dependency key (most recent predecessor)
/// 2. Check if `story/{last_dep_key}` exists as a local branch
/// 3. If it exists → return that branch (parent not yet merged, chain from it)
/// 4. If not → return `default_branch` (parent already merged to main, or no deps)
///
/// # Arguments
/// * `story` — The story being developed (contains `dependencies` from watcher)
/// * `repo` — An open git2 `Repository` reference
/// * `default_branch` — Fallback branch name (typically `"main"`)
pub fn determine_base_branch(story: &StoryInfo, repo: &Repository, default_branch: &str) -> String {
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
    let candidate = format!("story/{last_dep}");

    let exists = repo.find_branch(&candidate, BranchType::Local).is_ok();

    if exists {
        tracing::info!(
            action = "base_branch_resolved",
            base = %candidate,
            story = %story.story_key,
            dependency = %last_dep,
            reason = "dependency branch exists locally",
            "Chaining from dependency branch"
        );
        candidate
    } else {
        tracing::info!(
            action = "base_branch_resolved",
            base = %default_branch,
            story = %story.story_key,
            dependency = %last_dep,
            reason = "dependency branch not found locally (likely merged)",
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
/// This function is **synchronous** because git2 is blocking. The caller MUST wrap
/// it in `tokio::task::spawn_blocking()` to avoid blocking the async runtime.
///
/// # Arguments
/// * `repo_path` — Path to the git repository root
/// * `branch_name` — The story branch name (e.g., `"story/4-3-branch-mgmt"`)
/// * `base_branch` — The branch to create from if `branch_name` doesn't exist
///
/// # Errors
/// Returns [`BranchError`] if the repo can't be opened, the base branch is missing,
/// or branch creation/checkout fails.
pub fn ensure_story_branch(
    repo_path: &Path,
    branch_name: &str,
    base_branch: &str,
) -> Result<BranchAction, BranchError> {
    let repo = Repository::open(repo_path).map_err(|e| BranchError::RepoOpenFailed {
        path: repo_path.display().to_string(),
        reason: e.to_string(),
    })?;

    // Check if the branch already exists
    if repo.find_branch(branch_name, BranchType::Local).is_ok() {
        // Branch exists — check it out
        checkout_branch(&repo, branch_name)?;

        tracing::info!(
            action = "branch_reuse",
            branch = %branch_name,
            "Reusing existing story branch"
        );

        return Ok(BranchAction::Reused {
            branch_name: branch_name.to_string(),
        });
    }

    // Branch does not exist — create from base
    let base = repo
        .find_branch(base_branch, BranchType::Local)
        .map_err(|_| BranchError::BaseBranchNotFound {
            branch: base_branch.to_string(),
        })?;

    let commit = base
        .get()
        .peel_to_commit()
        .map_err(|e| BranchError::CreationFailed {
            branch: branch_name.to_string(),
            reason: format!("Failed to get tip commit of base branch: {e}"),
        })?;

    repo.branch(branch_name, &commit, false)
        .map_err(|e| BranchError::CreationFailed {
            branch: branch_name.to_string(),
            reason: e.to_string(),
        })?;

    // Checkout the new branch
    checkout_branch(&repo, branch_name)?;

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
fn checkout_branch(repo: &Repository, branch_name: &str) -> Result<(), BranchError> {
    let refname = format!("refs/heads/{branch_name}");

    repo.set_head(&refname)
        .map_err(|e| BranchError::CheckoutFailed {
            branch: branch_name.to_string(),
            reason: format!("set_head failed: {e}"),
        })?;

    repo.checkout_head(Some(CheckoutBuilder::default().force()))
        .map_err(|e| BranchError::CheckoutFailed {
            branch: branch_name.to_string(),
            reason: format!("checkout_head failed: {e}"),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Signature;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Create a temporary git repo with an initial commit on `main`.
    ///
    /// git2 requires at least one commit for branch operations. The default
    /// branch created by `Repository::init()` may not be named "main", so
    /// we explicitly create a "main" branch from the initial commit.
    fn init_test_repo() -> (TempDir, Repository) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = Repository::init(dir.path()).expect("init");

        // Create an initial commit (scoped to drop `tree` before moving `repo`)
        {
            let sig = Signature::now("test", "test@test.com").expect("sig");
            let tree_id = repo.index().expect("index").write_tree().expect("tree");
            let tree = repo.find_tree(tree_id).expect("find tree");
            repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
                .expect("commit");
        }

        // Ensure "main" branch exists — the default branch might be "master"
        {
            let head_commit = repo.head().expect("head").peel_to_commit().expect("commit");
            if repo.find_branch("main", BranchType::Local).is_err() {
                repo.branch("main", &head_commit, false)
                    .expect("create main");
            }
        }
        // Set HEAD to main
        repo.set_head("refs/heads/main").expect("set head to main");
        repo.checkout_head(Some(CheckoutBuilder::default().force()))
            .expect("checkout main");

        (dir, repo)
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

    /// Helper: create a branch with a commit in a test repo.
    fn create_branch_with_commit(repo: &Repository, branch_name: &str) {
        let head_commit = repo.head().expect("head").peel_to_commit().expect("commit");
        repo.branch(branch_name, &head_commit, false)
            .expect("create branch");
    }

    // -----------------------------------------------------------------------
    // determine_base_branch tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_determine_base_branch_no_deps_returns_default() {
        let (_dir, repo) = init_test_repo();
        let story = make_story("1-1-scaffolding", vec![]);

        let base = determine_base_branch(&story, &repo, "main");
        assert_eq!(base, "main");
    }

    #[test]
    fn test_determine_base_branch_dep_branch_exists_returns_parent() {
        let (_dir, repo) = init_test_repo();

        // Create the dependency branch
        create_branch_with_commit(&repo, "story/4-1-rig-tools");

        let story = make_story("4-2-session-setup", vec!["4-1-rig-tools"]);

        let base = determine_base_branch(&story, &repo, "main");
        assert_eq!(base, "story/4-1-rig-tools");
    }

    #[test]
    fn test_determine_base_branch_dep_branch_missing_returns_default() {
        let (_dir, repo) = init_test_repo();

        // Dependency exists in StoryInfo but branch is NOT in repo
        let story = make_story("4-2-session-setup", vec!["4-1-rig-tools"]);

        let base = determine_base_branch(&story, &repo, "main");
        assert_eq!(base, "main");
    }

    #[test]
    fn test_determine_base_branch_uses_last_dependency() {
        let (_dir, repo) = init_test_repo();

        // Create only the second dep branch
        create_branch_with_commit(&repo, "story/4-2-session-setup");

        let story = make_story(
            "4-3-branch-mgmt",
            vec!["4-1-rig-tools", "4-2-session-setup"],
        );

        let base = determine_base_branch(&story, &repo, "main");
        // Should check the LAST dep (4-2), which exists
        assert_eq!(base, "story/4-2-session-setup");
    }

    // -----------------------------------------------------------------------
    // ensure_story_branch tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ensure_story_branch_creates_new_from_main() {
        let (dir, repo) = init_test_repo();

        let result = ensure_story_branch(dir.path(), "story/1-1-scaffolding", "main")
            .expect("should succeed");

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

        // Verify HEAD is on the new branch
        let head = repo.head().expect("head");
        assert_eq!(
            head.shorthand().expect("shorthand"),
            "story/1-1-scaffolding"
        );
    }

    #[test]
    fn test_ensure_story_branch_creates_from_parent_branch() {
        let (dir, repo) = init_test_repo();

        // Create parent branch with a unique commit
        let head_commit = repo.head().expect("head").peel_to_commit().expect("commit");
        repo.branch("story/4-1-rig-tools", &head_commit, false)
            .expect("create parent");
        repo.set_head("refs/heads/story/4-1-rig-tools")
            .expect("set head");
        repo.checkout_head(Some(CheckoutBuilder::default().force()))
            .expect("checkout");

        // Add a commit on the parent branch
        let sig = Signature::now("test", "test@test.com").expect("sig");
        let parent_commit = repo.head().expect("head").peel_to_commit().expect("commit");
        let tree_id = repo.index().expect("index").write_tree().expect("tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let parent_oid = repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                "parent branch commit",
                &tree,
                &[&parent_commit],
            )
            .expect("commit on parent");

        let result =
            ensure_story_branch(dir.path(), "story/4-2-session-setup", "story/4-1-rig-tools")
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
        let child_head = repo.head().expect("head").peel_to_commit().expect("commit");
        assert_eq!(child_head.id(), parent_oid);
    }

    #[test]
    fn test_ensure_story_branch_reuses_existing() {
        let (dir, _repo) = init_test_repo();

        // First call — creates the branch
        let first = ensure_story_branch(dir.path(), "story/2-1-polling", "main")
            .expect("first call should succeed");
        assert!(matches!(first, BranchAction::Created { .. }));

        // Second call — should reuse the existing branch
        let second = ensure_story_branch(dir.path(), "story/2-1-polling", "main")
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
        let (dir, _repo) = init_test_repo();

        let result = ensure_story_branch(dir.path(), "story/1-1-scaffolding", "nonexistent-branch");

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
        match result.unwrap_err() {
            BranchError::RepoOpenFailed { path, reason } => {
                assert!(path.contains(dir.path().to_str().unwrap()));
                assert!(!reason.is_empty());
            }
            other => panic!("Expected RepoOpenFailed, got: {other:?}"),
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

        let err = BranchError::RepoOpenFailed {
            path: "/tmp/bad".to_string(),
            reason: "not a repo".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("/tmp/bad"));
        assert!(display.contains("not a repo"));
    }
}
