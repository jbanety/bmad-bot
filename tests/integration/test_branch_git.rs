//! Integration tests for branch management and git tool operations (Story 7.8).
//!
//! Tests verify that `session::branch`, `tools::git`, and `session::cleanup` modules
//! produce consistent git state when operating on real (temp) repositories.

use bmad_bot::session::branch::{ensure_story_branch, determine_base_branch, BranchAction, BranchError};
use bmad_bot::session::cleanup::preserve_partial_work;
use bmad_bot::tools::git::{GitTool, GitToolArgs};
use bmad_bot::watcher::StoryInfo;
use rig::tool::Tool; // REQUIRED — GitTool::call() comes from this trait
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

use crate::helpers::fixtures::{create_test_repo, make_test_story};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build `GitToolArgs` with only `action` set — all optional fields default to `None`.
fn git_args(action: &str) -> GitToolArgs {
    GitToolArgs {
        action: action.to_string(),
        branch: None,
        message: None,
        paths: None,
        url: None,
        remote: None,
        max_count: None,
        from_branch: None,
    }
}

/// Shorthand for constructing a `StoryInfo` with dependency support (delegates to fixture helper).
fn make_story(key: &str, deps: Vec<&str>) -> StoryInfo {
    make_test_story(key, "test", deps.into_iter().map(String::from).collect())
}

/// Create a branch with a commit on a test repo (for setting up dependency branches).
fn create_branch_with_commit(repo_path: &Path, branch_name: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["checkout", "-b", branch_name]);
    run(&["commit", "--allow-empty", "-m", &format!("commit on {branch_name}")]);
    run(&["checkout", "main"]);
}

/// Return the current HEAD branch name.
fn current_branch(repo_path: &Path) -> String {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_path)
        .output()
        .expect("git branch --show-current failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Return the short SHA of HEAD.
fn head_sha(repo_path: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Return the full SHA of HEAD.
fn head_sha_full(repo_path: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Return the full SHA of a given branch tip.
fn branch_sha(repo_path: &Path, branch: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", branch])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

// ===========================================================================
// Task 2: ensure_story_branch integration tests (AC #1, #2)
// ===========================================================================

/// AC #1 — Create a new branch from `main` on a temp repo.
#[test]
fn test_ensure_story_branch_creates_new_from_main() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", "main")
        .expect("ensure_story_branch failed");

    match result {
        BranchAction::Created { ref branch_name, ref base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli");
            assert_eq!(base_branch, "main");
        }
        BranchAction::Reused { .. } => panic!("Expected Created, got Reused"),
    }

    assert_eq!(current_branch(tmp.path()), "story/1-2-cli", "HEAD should be on story branch");
}

/// AC #2 — Calling ensure_story_branch twice → second call returns Reused.
#[test]
fn test_ensure_story_branch_reuses_existing() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // First call — creates
    let first = ensure_story_branch(tmp.path(), "story/1-2-cli", "main")
        .expect("first call failed");
    assert!(matches!(first, BranchAction::Created { .. }), "Expected Created on first call");

    // Second call — reuses
    let second = ensure_story_branch(tmp.path(), "story/1-2-cli", "main")
        .expect("second call failed");
    match second {
        BranchAction::Reused { ref branch_name } => {
            assert_eq!(branch_name, "story/1-2-cli");
        }
        BranchAction::Created { .. } => panic!("Expected Reused on second call, got Created"),
    }
}

/// Task 2.3 — Create branch from a non-main parent branch.
#[test]
fn test_ensure_story_branch_creates_from_parent_branch() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // Create a parent branch with a distinguishing commit
    create_branch_with_commit(tmp.path(), "story/1-1-scaffolding");

    let parent_sha = branch_sha(tmp.path(), "story/1-1-scaffolding");

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", "story/1-1-scaffolding")
        .expect("ensure_story_branch from parent failed");

    match result {
        BranchAction::Created { ref branch_name, ref base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli");
            assert_eq!(base_branch, "story/1-1-scaffolding");
        }
        BranchAction::Reused { .. } => panic!("Expected Created"),
    }

    // New branch should share the same commit as the parent
    let new_sha = head_sha_full(tmp.path());
    assert_eq!(new_sha, parent_sha, "New branch should be created from parent's tip commit");
}

/// Task 2.4 — Base branch does not exist → BaseBranchNotFound.
#[test]
fn test_ensure_story_branch_base_not_found() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", "nonexistent-branch");

    match result {
        Err(BranchError::BaseBranchNotFound { ref branch }) => {
            assert_eq!(branch, "nonexistent-branch");
        }
        other => panic!("Expected BaseBranchNotFound, got: {other:?}"),
    }
}

/// Task 2.5 — Call on non-git directory → error returned.
/// Note: BranchError::RepoOpenFailed does not exist in the actual API.
/// The API returns BaseBranchNotFound because branch_exists() returns false
/// when git commands fail on a non-repo directory.
#[test]
fn test_ensure_story_branch_non_git_directory() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    // Do NOT init a git repo — tmp.path() is just an empty directory

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", "main");

    assert!(result.is_err(), "Expected error on non-git directory");
    match result {
        Err(BranchError::BaseBranchNotFound { ref branch }) => {
            assert_eq!(branch, "main", "Should report base branch not found");
        }
        other => panic!("Expected BaseBranchNotFound error, got: {other:?}"),
    }
}

// ===========================================================================
// Task 3: determine_base_branch integration tests (AC #3, #4)
// ===========================================================================

/// AC #4 — Story with no dependencies → returns "main".
#[test]
fn test_determine_base_branch_no_deps_returns_main() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    let story = make_story("1-2-cli", vec![]);
    let base = determine_base_branch(&story, tmp.path(), "main");

    assert_eq!(base, "main");
}

/// AC #3 — Story with one dependency whose branch exists locally.
#[test]
fn test_determine_base_branch_dep_branch_exists() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());
    create_branch_with_commit(tmp.path(), "story/1-1-scaffolding");

    let story = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base = determine_base_branch(&story, tmp.path(), "main");

    assert_eq!(base, "story/1-1-scaffolding");
}

/// Task 3.3 — Dependency branch does NOT exist → fallback to "main".
#[test]
fn test_determine_base_branch_dep_branch_missing_fallback() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());
    // Do NOT create the dependency branch

    let story = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base = determine_base_branch(&story, tmp.path(), "main");

    assert_eq!(base, "main");
}

/// Task 3.4 — Multiple dependencies → uses LAST dependency.
#[test]
fn test_determine_base_branch_multiple_deps_uses_last() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());
    create_branch_with_commit(tmp.path(), "story/1-1-scaffolding");
    create_branch_with_commit(tmp.path(), "story/1-2-cli");

    let story = make_story("1-3-feature", vec!["1-1-scaffolding", "1-2-cli"]);
    let base = determine_base_branch(&story, tmp.path(), "main");

    assert_eq!(base, "story/1-2-cli", "Should use last dependency branch");
}

// ===========================================================================
// Task 4: End-to-end branch flow integration tests (AC #1, #3, #4)
// ===========================================================================

/// Task 4.1 — Full flow: determine_base_branch → ensure_story_branch → verify.
#[test]
fn test_e2e_determine_then_ensure_chained_from_dependency() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());
    create_branch_with_commit(tmp.path(), "story/1-1-scaffolding");

    let story = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "story/1-1-scaffolding");

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", &base)
        .expect("ensure_story_branch failed");

    match result {
        BranchAction::Created { ref branch_name, ref base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli");
            assert_eq!(base_branch, "story/1-1-scaffolding");
        }
        BranchAction::Reused { .. } => panic!("Expected Created"),
    }

    assert_eq!(current_branch(tmp.path()), "story/1-2-cli");
    // Verify the commit parent chain — new branch tip should match parent tip
    let parent_sha = branch_sha(tmp.path(), "story/1-1-scaffolding");
    let new_sha = head_sha_full(tmp.path());
    assert_eq!(new_sha, parent_sha, "Story branch should fork from dependency tip");
}

/// Task 4.2 — Multi-story chain: story/1-1 → story/1-2 → story/1-3.
#[test]
fn test_e2e_multi_story_chain() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // Story 1-1: no deps → base is main
    let story_1 = make_story("1-1-scaffolding", vec![]);
    let base_1 = determine_base_branch(&story_1, tmp.path(), "main");
    assert_eq!(base_1, "main");
    ensure_story_branch(tmp.path(), "story/1-1-scaffolding", &base_1)
        .expect("ensure 1-1 failed");
    // Add a commit to distinguish from main
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(tmp.path())
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {} failed", args.join(" "));
    };
    run(&["commit", "--allow-empty", "-m", "work on 1-1"]);
    let sha_1 = head_sha_full(tmp.path());
    run(&["checkout", "main"]);

    // Story 1-2: depends on 1-1
    let story_2 = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base_2 = determine_base_branch(&story_2, tmp.path(), "main");
    assert_eq!(base_2, "story/1-1-scaffolding");
    ensure_story_branch(tmp.path(), "story/1-2-cli", &base_2)
        .expect("ensure 1-2 failed");
    let sha_2_parent = head_sha_full(tmp.path());
    assert_eq!(sha_2_parent, sha_1, "1-2 should fork from 1-1's tip");
    run(&["commit", "--allow-empty", "-m", "work on 1-2"]);
    let sha_2 = head_sha_full(tmp.path());
    run(&["checkout", "main"]);

    // Story 1-3: depends on 1-2
    let story_3 = make_story("1-3-feature", vec!["1-2-cli"]);
    let base_3 = determine_base_branch(&story_3, tmp.path(), "main");
    assert_eq!(base_3, "story/1-2-cli");
    ensure_story_branch(tmp.path(), "story/1-3-feature", &base_3)
        .expect("ensure 1-3 failed");
    let sha_3_parent = head_sha_full(tmp.path());
    assert_eq!(sha_3_parent, sha_2, "1-3 should fork from 1-2's tip");
}

/// Task 4.3 — Dependency branch missing (merged to main) → fallback to main.
#[test]
fn test_e2e_dependency_missing_falls_back_to_main() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // Dependency branch doesn't exist (simulating already-merged scenario)
    let story = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "main", "Should fall back to main when dep branch missing");

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", &base)
        .expect("ensure_story_branch from main failed");

    match result {
        BranchAction::Created { ref base_branch, .. } => {
            assert_eq!(base_branch, "main");
        }
        BranchAction::Reused { .. } => panic!("Expected Created"),
    }

    assert_eq!(current_branch(tmp.path()), "story/1-2-cli");
}

// ===========================================================================
// Task 5: GitTool integration tests (AC #1, #5) — LOCAL ACTIONS ONLY
// ===========================================================================

/// Task 5.1 — branch_create + checkout → verify branch exists and HEAD is on it.
#[tokio::test]
async fn test_git_tool_branch_create_and_checkout() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    let tool = GitTool::new(tmp.path().to_path_buf());

    // Create a branch
    let mut args = git_args("branch_create");
    args.branch = Some("feature/test-branch".to_string());
    let output = tool.call(args).await.expect("branch_create failed");
    assert!(
        output.contains("feature/test-branch"),
        "Output should mention new branch name. Got: {output}"
    );

    // Verify HEAD is on the new branch
    assert_eq!(current_branch(tmp.path()), "feature/test-branch");

    // Checkout main
    let mut args = git_args("checkout");
    args.branch = Some("main".to_string());
    tool.call(args).await.expect("checkout main failed");
    assert_eq!(current_branch(tmp.path()), "main");

    // Checkout back to feature branch
    let mut args = git_args("checkout");
    args.branch = Some("feature/test-branch".to_string());
    tool.call(args).await.expect("checkout feature failed");
    assert_eq!(current_branch(tmp.path()), "feature/test-branch");
}

/// Task 5.2 — add + commit → verify commit exists in log output.
#[tokio::test]
async fn test_git_tool_add_and_commit_in_log() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // Write a file
    std::fs::write(tmp.path().join("test.txt"), "hello world").expect("write failed");

    let tool = GitTool::new(tmp.path().to_path_buf());

    // Stage it
    let mut args = git_args("add");
    args.paths = Some(vec!["test.txt".to_string()]);
    tool.call(args).await.expect("add failed");

    // Commit
    let mut args = git_args("commit");
    args.message = Some("Add test.txt".to_string());
    tool.call(args).await.expect("commit failed");

    // Verify in log
    let log_output = tool.call(git_args("log")).await.expect("log failed");
    assert!(
        log_output.contains("Add test.txt"),
        "Log should contain commit message. Got: {log_output}"
    );
}

/// Task 5.3 — status on dirty tree shows changes; status on clean tree shows clean.
#[tokio::test]
async fn test_git_tool_status_dirty_and_clean() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    let tool = GitTool::new(tmp.path().to_path_buf());

    // Clean tree first
    let status_clean = tool.call(git_args("status")).await.expect("status failed");
    assert!(
        status_clean.contains("Clean working directory") || status_clean.contains("nothing to commit"),
        "Expected clean status. Got: {status_clean}"
    );

    // Create a file to dirty the tree
    std::fs::write(tmp.path().join("dirty.txt"), "changes").expect("write failed");

    let status_dirty = tool.call(git_args("status")).await.expect("status failed");
    assert!(
        status_dirty.contains("dirty.txt"),
        "Status should show dirty file. Got: {status_dirty}"
    );
}

/// Task 5.4 — diff shows uncommitted changes.
#[tokio::test]
async fn test_git_tool_diff_shows_changes() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // Create and commit a file first
    std::fs::write(tmp.path().join("existing.txt"), "original").expect("write failed");
    let tool = GitTool::new(tmp.path().to_path_buf());

    let mut args = git_args("add");
    args.paths = Some(vec!["existing.txt".to_string()]);
    tool.call(args).await.expect("add failed");

    let mut args = git_args("commit");
    args.message = Some("add existing.txt".to_string());
    tool.call(args).await.expect("commit failed");

    // Modify the file
    std::fs::write(tmp.path().join("existing.txt"), "modified content").expect("write failed");

    let diff_output = tool.call(git_args("diff")).await.expect("diff failed");
    assert!(
        diff_output.contains("modified content") || diff_output.contains("existing.txt"),
        "Diff should show changes. Got: {diff_output}"
    );
}

/// Task 5.5 — Full roundtrip: branch_create → write files → add → commit → log → verify.
#[tokio::test]
async fn test_git_tool_full_roundtrip() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    let tool = GitTool::new(tmp.path().to_path_buf());

    // Create branch
    let mut args = git_args("branch_create");
    args.branch = Some("feature/roundtrip".to_string());
    tool.call(args).await.expect("branch_create failed");
    assert_eq!(current_branch(tmp.path()), "feature/roundtrip");

    // Write files
    std::fs::write(tmp.path().join("file_a.rs"), "fn main() {}").expect("write failed");
    std::fs::write(tmp.path().join("file_b.rs"), "fn test() {}").expect("write failed");

    // Stage all
    let mut args = git_args("add");
    args.paths = Some(vec!["*".to_string()]);
    tool.call(args).await.expect("add failed");

    // Commit
    let mut args = git_args("commit");
    args.message = Some("feat: add roundtrip files".to_string());
    tool.call(args).await.expect("commit failed");

    // Verify in log
    let log_output = tool.call(git_args("log")).await.expect("log failed");
    assert!(
        log_output.contains("feat: add roundtrip files"),
        "Log should contain commit message. Got: {log_output}"
    );
    // Verify SHA is present in log
    let sha = head_sha(tmp.path());
    assert!(
        log_output.contains(&sha),
        "Log should contain HEAD SHA {sha}. Got: {log_output}"
    );
}

// ===========================================================================
// Task 6: preserve_partial_work integration tests (AC #5)
// ===========================================================================

/// Task 6.1 — Dirty tree → WIP commit created.
#[tokio::test]
async fn test_preserve_partial_work_dirty_tree_creates_wip_commit() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // Create dirty files
    std::fs::write(tmp.path().join("wip_file.rs"), "// work in progress").expect("write failed");

    let summary = preserve_partial_work(tmp.path(), "1-2-cli", "Need clarification").await;

    assert!(
        summary.contains("WIP commit: yes"),
        "Summary should indicate WIP commit was created. Got: {summary}"
    );
    assert!(
        summary.contains("wip_file.rs"),
        "Summary should list changed file. Got: {summary}"
    );
}

/// Task 6.2 — Clean tree → no commit created.
#[tokio::test]
async fn test_preserve_partial_work_clean_tree_no_commit() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    let summary = preserve_partial_work(tmp.path(), "1-2-cli", "Question?").await;

    assert!(
        summary.contains("no (clean tree)"),
        "Summary should indicate no WIP commit on clean tree. Got: {summary}"
    );
}

/// Task 6.3 — preserve_partial_work on a branch created by ensure_story_branch.
#[tokio::test]
async fn test_preserve_partial_work_on_story_branch() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // Create story branch
    ensure_story_branch(tmp.path(), "story/1-2-cli", "main")
        .expect("ensure_story_branch failed");
    assert_eq!(current_branch(tmp.path()), "story/1-2-cli");

    // Create dirty files on the story branch
    std::fs::write(tmp.path().join("feature.rs"), "fn feature() {}").expect("write failed");

    let summary = preserve_partial_work(tmp.path(), "1-2-cli", "Need help").await;

    assert!(
        summary.contains("WIP commit: yes"),
        "Should create WIP commit. Got: {summary}"
    );
    assert!(
        summary.contains("story/1-2-cli"),
        "Summary should mention story branch. Got: {summary}"
    );

    // Verify the commit exists on the story branch via git log
    let log_output = Command::new("git")
        .args(["log", "--oneline", "-5"])
        .current_dir(tmp.path())
        .output()
        .expect("git log failed");
    let log_text = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        log_text.contains("WIP") || log_text.contains("escalated"),
        "Git log should contain WIP commit. Got: {log_text}"
    );

    // Verify we're still on the story branch
    assert_eq!(current_branch(tmp.path()), "story/1-2-cli");
}

// ===========================================================================
// Task 7: Cross-module integration tests (AC ALL)
// ===========================================================================

/// Task 7.1 — Full lifecycle: StoryInfo → determine_base_branch → ensure_story_branch
///   → GitTool write+add+commit → preserve_partial_work on additional dirty changes
///   → verify both commits exist on the story branch.
#[tokio::test]
async fn test_cross_module_full_lifecycle() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // Step 1: Determine base (no deps → main)
    let story = make_story("2-1-feature", vec![]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "main");

    // Step 2: Create story branch
    ensure_story_branch(tmp.path(), "story/2-1-feature", &base)
        .expect("ensure_story_branch failed");
    assert_eq!(current_branch(tmp.path()), "story/2-1-feature");

    // Step 3: Use GitTool to write, add, commit
    let tool = GitTool::new(tmp.path().to_path_buf());
    std::fs::write(tmp.path().join("impl.rs"), "fn implementation() {}").expect("write failed");

    let mut args = git_args("add");
    args.paths = Some(vec!["impl.rs".to_string()]);
    tool.call(args).await.expect("GitTool add failed");

    let mut args = git_args("commit");
    args.message = Some("feat: add implementation".to_string());
    tool.call(args).await.expect("GitTool commit failed");

    // Step 4: Create additional dirty changes
    std::fs::write(tmp.path().join("wip.rs"), "// not done yet").expect("write failed");

    // Step 5: Preserve partial work
    let summary = preserve_partial_work(tmp.path(), "2-1-feature", "Stuck on edge case").await;
    assert!(
        summary.contains("WIP commit: yes"),
        "Should create WIP commit for dirty changes. Got: {summary}"
    );

    // Step 6: Verify both commits exist on the story branch
    let log_output = Command::new("git")
        .args(["log", "--oneline", "-10"])
        .current_dir(tmp.path())
        .output()
        .expect("git log failed");
    let log_text = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        log_text.contains("feat: add implementation"),
        "Log should contain GitTool commit. Got: {log_text}"
    );
    assert!(
        log_text.contains("WIP") || log_text.contains("escalated"),
        "Log should contain WIP commit. Got: {log_text}"
    );

    assert_eq!(current_branch(tmp.path()), "story/2-1-feature");
}

/// Task 7.2 — Cross-module consistency: create branch via ensure_story_branch,
///   switch to main via GitTool, switch back via GitTool, verify consistent state.
#[tokio::test]
async fn test_cross_module_branch_switching_consistency() {
    let tmp = TempDir::new().expect("failed to create tempdir");
    create_test_repo(tmp.path());

    // Create story branch via ensure_story_branch
    ensure_story_branch(tmp.path(), "story/3-1-test", "main")
        .expect("ensure_story_branch failed");
    assert_eq!(current_branch(tmp.path()), "story/3-1-test");

    // Add a commit so the branch has unique content
    std::fs::write(tmp.path().join("story_file.txt"), "story content").expect("write failed");
    let tool = GitTool::new(tmp.path().to_path_buf());

    let mut args = git_args("add");
    args.paths = Some(vec!["story_file.txt".to_string()]);
    tool.call(args).await.expect("add failed");

    let mut args = git_args("commit");
    args.message = Some("story commit".to_string());
    tool.call(args).await.expect("commit failed");

    let story_sha = head_sha_full(tmp.path());

    // Switch to main via GitTool
    let mut args = git_args("checkout");
    args.branch = Some("main".to_string());
    tool.call(args).await.expect("checkout main failed");
    assert_eq!(current_branch(tmp.path()), "main");
    assert!(
        !tmp.path().join("story_file.txt").exists(),
        "story_file.txt should NOT exist on main"
    );

    // Switch back to story branch via GitTool
    let mut args = git_args("checkout");
    args.branch = Some("story/3-1-test".to_string());
    tool.call(args).await.expect("checkout story branch failed");
    assert_eq!(current_branch(tmp.path()), "story/3-1-test");
    assert_eq!(head_sha_full(tmp.path()), story_sha, "HEAD should be at story commit");
    assert!(
        tmp.path().join("story_file.txt").exists(),
        "story_file.txt should exist on story branch"
    );
}
