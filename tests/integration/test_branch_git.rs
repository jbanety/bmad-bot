//! Integration tests for branch management and git tool operations.
//!
//! Tests the interaction between three modules on real (temp) git repos:
//! - `session::branch` — `determine_base_branch`, `ensure_story_branch`
//! - `tools::git` — `GitTool::call()` (local actions only)
//! - `session::cleanup` — `preserve_partial_work`

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

/// Build GitToolArgs with only the action — all other fields default to None.
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

/// Create a branch with an empty commit on it (using git CLI).
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

/// Get HEAD branch name.
fn head_branch(repo_path: &Path) -> String {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_path)
        .output()
        .expect("git branch --show-current failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Get HEAD commit SHA (short).
fn head_sha(repo_path: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Get full HEAD commit SHA.
fn head_sha_full(repo_path: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Build a StoryInfo with custom dependencies (delegates to fixture helper).
fn make_story(key: &str, deps: Vec<&str>) -> StoryInfo {
    let parts: Vec<&str> = key.splitn(3, '-').collect();
    let label = parts.get(2).unwrap_or(&"test").to_string();
    make_test_story(key, &label, deps.into_iter().map(String::from).collect())
}

// ===========================================================================
// Task 2: ensure_story_branch integration tests (AC #1, #2)
// ===========================================================================

#[test]
fn test_ensure_story_branch_creates_new_from_main() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", "main")
        .expect("ensure_story_branch should succeed");

    match &result {
        BranchAction::Created { branch_name, base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli", "branch_name mismatch");
            assert_eq!(base_branch, "main", "base_branch mismatch");
        }
        BranchAction::Reused { .. } => panic!("Expected Created, got Reused"),
    }

    assert_eq!(head_branch(tmp.path()), "story/1-2-cli", "HEAD should be on new branch");
}

#[test]
fn test_ensure_story_branch_reuses_existing() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // First call — creates
    let first = ensure_story_branch(tmp.path(), "story/1-2-cli", "main")
        .expect("first call should succeed");
    assert!(matches!(first, BranchAction::Created { .. }), "first call should create");

    // Go back to main so we can verify checkout happens
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(tmp.path())
        .output()
        .expect("checkout main failed");

    // Second call — reuses
    let second = ensure_story_branch(tmp.path(), "story/1-2-cli", "main")
        .expect("second call should succeed");
    match &second {
        BranchAction::Reused { branch_name } => {
            assert_eq!(branch_name, "story/1-2-cli", "reused branch_name mismatch");
        }
        BranchAction::Created { .. } => panic!("Expected Reused, got Created"),
    }

    assert_eq!(head_branch(tmp.path()), "story/1-2-cli", "HEAD should be on reused branch");
}

#[test]
fn test_ensure_story_branch_creates_from_non_main_parent() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Create parent branch with a distinguishing commit
    create_branch_with_commit(tmp.path(), "story/1-1-scaffolding");

    // Get the parent branch tip commit
    Command::new("git")
        .args(["checkout", "story/1-1-scaffolding"])
        .current_dir(tmp.path())
        .output()
        .expect("checkout parent failed");
    let parent_sha = head_sha_full(tmp.path());
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(tmp.path())
        .output()
        .expect("checkout main failed");

    // Create story branch from the parent (not main)
    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", "story/1-1-scaffolding")
        .expect("ensure_story_branch should succeed");

    match &result {
        BranchAction::Created { branch_name, base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli");
            assert_eq!(base_branch, "story/1-1-scaffolding");
        }
        BranchAction::Reused { .. } => panic!("Expected Created, got Reused"),
    }

    // Verify the new branch's HEAD matches the parent's tip
    assert_eq!(head_sha_full(tmp.path()), parent_sha, "new branch should fork from parent branch tip");
}

#[test]
fn test_ensure_story_branch_base_not_found() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", "nonexistent-base");

    match result {
        Err(BranchError::BaseBranchNotFound { branch }) => {
            assert_eq!(branch, "nonexistent-base", "error should name the missing base");
        }
        Err(other) => panic!("Expected BaseBranchNotFound, got: {other}"),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

#[test]
fn test_ensure_story_branch_non_git_directory() {
    let tmp = TempDir::new().expect("tempdir failed");
    // Do NOT init a git repo — tmp is a plain directory

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", "main");

    // On a non-git directory, branch_exists returns false for both story branch and
    // base branch, so we get BaseBranchNotFound (no RepoOpenFailed variant exists).
    assert!(result.is_err(), "should error on non-git directory");
    match result {
        Err(BranchError::BaseBranchNotFound { .. }) => { /* expected */ }
        Err(other) => panic!("Expected BaseBranchNotFound on non-git dir, got: {other}"),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

// ===========================================================================
// Task 3: determine_base_branch integration tests (AC #3, #4)
// ===========================================================================

#[test]
fn test_determine_base_branch_no_deps_returns_main() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    let story = make_story("1-2-cli", vec![]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "main", "no deps → should return default branch");
}

#[test]
fn test_determine_base_branch_dep_branch_exists() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    create_branch_with_commit(tmp.path(), "story/1-1-scaffolding");

    let story = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "story/1-1-scaffolding", "dep branch exists → should return it");
}

#[test]
fn test_determine_base_branch_dep_branch_missing_falls_back() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Do NOT create the dependency branch
    let story = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "main", "dep branch missing → should fall back to default");
}

#[test]
fn test_determine_base_branch_multiple_deps_uses_last() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Create both dependency branches
    create_branch_with_commit(tmp.path(), "story/1-1-scaffolding");
    create_branch_with_commit(tmp.path(), "story/1-2-cli");

    let story = make_story("1-3-init", vec!["1-1-scaffolding", "1-2-cli"]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "story/1-2-cli", "multiple deps → should use LAST dep branch");
}

// ===========================================================================
// Task 4: End-to-end branch flow integration tests (AC #1, #3, #4)
// ===========================================================================

#[test]
fn test_e2e_determine_then_ensure_from_dependency() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Setup: create dep branch
    create_branch_with_commit(tmp.path(), "story/1-1-scaffolding");

    // Determine base
    let story = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "story/1-1-scaffolding");

    // Ensure story branch from that base
    let result = ensure_story_branch(tmp.path(), &story.branch_name, &base)
        .expect("ensure_story_branch should succeed");
    assert!(matches!(result, BranchAction::Created { .. }));
    assert_eq!(head_branch(tmp.path()), "story/1-2-cli");
}

#[test]
fn test_e2e_multi_story_chain() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Story 1-1: no deps → branch from main
    let story1 = make_story("1-1-scaffolding", vec![]);
    let base1 = determine_base_branch(&story1, tmp.path(), "main");
    assert_eq!(base1, "main");
    ensure_story_branch(tmp.path(), "story/1-1-scaffolding", &base1)
        .expect("create story/1-1 failed");

    // Add a distinguishing commit on story/1-1
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "work on 1-1"])
        .current_dir(tmp.path())
        .output()
        .expect("commit on 1-1 failed");
    let sha_1_1 = head_sha_full(tmp.path());

    // Story 1-2: depends on 1-1
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(tmp.path())
        .output()
        .expect("checkout main failed");

    let story2 = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base2 = determine_base_branch(&story2, tmp.path(), "main");
    assert_eq!(base2, "story/1-1-scaffolding");
    ensure_story_branch(tmp.path(), "story/1-2-cli", &base2)
        .expect("create story/1-2 failed");

    // 1-2's parent commit should be 1-1's tip
    assert_eq!(head_sha_full(tmp.path()), sha_1_1, "1-2 should fork from 1-1 tip");

    // Add a commit on 1-2
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "work on 1-2"])
        .current_dir(tmp.path())
        .output()
        .expect("commit on 1-2 failed");
    let sha_1_2 = head_sha_full(tmp.path());

    // Story 1-3: depends on 1-2
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(tmp.path())
        .output()
        .expect("checkout main failed");

    let story3 = make_story("1-3-init", vec!["1-2-cli"]);
    let base3 = determine_base_branch(&story3, tmp.path(), "main");
    assert_eq!(base3, "story/1-2-cli");
    ensure_story_branch(tmp.path(), "story/1-3-init", &base3)
        .expect("create story/1-3 failed");

    // 1-3's parent commit should be 1-2's tip
    assert_eq!(head_sha_full(tmp.path()), sha_1_2, "1-3 should fork from 1-2 tip");
}

#[test]
fn test_e2e_dependency_branch_missing_falls_back_to_main() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Dep branch does NOT exist (simulates already merged to main)
    let story = make_story("1-2-cli", vec!["1-1-scaffolding"]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "main", "missing dep → fallback to main");

    let result = ensure_story_branch(tmp.path(), "story/1-2-cli", &base)
        .expect("ensure_story_branch should succeed");
    assert!(matches!(result, BranchAction::Created { .. }));
    assert_eq!(head_branch(tmp.path()), "story/1-2-cli");
}

// ===========================================================================
// Task 5: GitTool integration tests (AC #1, #5) — LOCAL ACTIONS ONLY
// ===========================================================================

#[tokio::test]
async fn test_git_tool_branch_create_and_checkout() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    let tool = GitTool::new(tmp.path().to_path_buf());

    // branch_create
    let mut args = git_args("branch_create");
    args.branch = Some("story/1-2-cli".to_string());
    let result = tool.call(args).await.expect("branch_create should succeed");
    assert!(result.contains("story/1-2-cli"), "output should mention branch name: {result}");

    // Verify HEAD is on the new branch
    assert_eq!(head_branch(tmp.path()), "story/1-2-cli");

    // checkout back to main
    let mut args = git_args("checkout");
    args.branch = Some("main".to_string());
    tool.call(args).await.expect("checkout main should succeed");
    assert_eq!(head_branch(tmp.path()), "main");

    // checkout story branch again
    let mut args = git_args("checkout");
    args.branch = Some("story/1-2-cli".to_string());
    tool.call(args).await.expect("checkout story branch should succeed");
    assert_eq!(head_branch(tmp.path()), "story/1-2-cli");
}

#[tokio::test]
async fn test_git_tool_add_and_commit() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Write a file
    std::fs::write(tmp.path().join("hello.txt"), "world").expect("write failed");

    let tool = GitTool::new(tmp.path().to_path_buf());

    // add
    let mut args = git_args("add");
    args.paths = Some(vec!["hello.txt".to_string()]);
    tool.call(args).await.expect("add should succeed");

    // commit
    let mut args = git_args("commit");
    args.message = Some("add hello".to_string());
    tool.call(args).await.expect("commit should succeed");

    // log — verify commit appears
    let log_args = git_args("log");
    let log_output = tool.call(log_args).await.expect("log should succeed");
    assert!(log_output.contains("add hello"), "log should contain commit message: {log_output}");
}

#[tokio::test]
async fn test_git_tool_status_dirty_and_clean() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    let tool = GitTool::new(tmp.path().to_path_buf());

    // Clean tree
    let status = tool.call(git_args("status")).await.expect("status should succeed");
    assert!(
        status.contains("Clean working directory") || status.contains("nothing to commit"),
        "clean tree should report clean: {status}"
    );

    // Dirty tree
    std::fs::write(tmp.path().join("dirty.txt"), "changes").expect("write failed");
    let status = tool.call(git_args("status")).await.expect("status should succeed");
    assert!(status.contains("dirty.txt"), "dirty status should show file: {status}");
}

#[tokio::test]
async fn test_git_tool_diff_shows_uncommitted_changes() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Create and commit a file first
    std::fs::write(tmp.path().join("file.txt"), "original").expect("write failed");
    let tool = GitTool::new(tmp.path().to_path_buf());

    let mut add_args = git_args("add");
    add_args.paths = Some(vec!["file.txt".to_string()]);
    tool.call(add_args).await.expect("add should succeed");

    let mut commit_args = git_args("commit");
    commit_args.message = Some("add file".to_string());
    tool.call(commit_args).await.expect("commit should succeed");

    // Modify the file
    std::fs::write(tmp.path().join("file.txt"), "modified").expect("write failed");

    // diff should show the change
    let diff = tool.call(git_args("diff")).await.expect("diff should succeed");
    assert!(diff.contains("modified") || diff.contains("file.txt"),
        "diff should show changes: {diff}");
}

#[tokio::test]
async fn test_git_tool_full_roundtrip() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    let tool = GitTool::new(tmp.path().to_path_buf());

    // branch_create
    let mut args = git_args("branch_create");
    args.branch = Some("story/roundtrip".to_string());
    tool.call(args).await.expect("branch_create should succeed");
    assert_eq!(head_branch(tmp.path()), "story/roundtrip");

    // Write files
    std::fs::write(tmp.path().join("src.rs"), "fn main() {}").expect("write failed");
    std::fs::write(tmp.path().join("test.rs"), "fn test() {}").expect("write failed");

    // add all
    let mut args = git_args("add");
    args.paths = Some(vec!["*".to_string()]);
    tool.call(args).await.expect("add should succeed");

    // commit
    let mut args = git_args("commit");
    args.message = Some("feat: roundtrip test commit".to_string());
    tool.call(args).await.expect("commit should succeed");

    // log
    let log_output = tool.call(git_args("log")).await.expect("log should succeed");
    assert!(log_output.contains("feat: roundtrip test commit"),
        "log should contain commit message: {log_output}");
    // Verify SHA is present (7+ hex chars)
    let has_sha = log_output.lines().any(|line| {
        line.split_whitespace().any(|word| {
            word.len() >= 7 && word.chars().all(|c| c.is_ascii_hexdigit())
        })
    });
    assert!(has_sha, "log should contain a commit SHA: {log_output}");
}

// ===========================================================================
// Task 6: preserve_partial_work integration tests (AC #5)
// ===========================================================================

#[tokio::test]
async fn test_preserve_partial_work_dirty_tree() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Create dirty files
    std::fs::write(tmp.path().join("wip.txt"), "work in progress").expect("write failed");
    std::fs::write(tmp.path().join("another.txt"), "more wip").expect("write failed");

    let summary = preserve_partial_work(tmp.path(), "1-2-cli", "Need clarification").await;

    assert!(summary.contains("WIP commit: yes"), "summary should indicate commit: {summary}");
    assert!(summary.contains("wip.txt"), "summary should list wip.txt: {summary}");
    assert!(summary.contains("another.txt"), "summary should list another.txt: {summary}");
}

#[tokio::test]
async fn test_preserve_partial_work_clean_tree() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    let summary = preserve_partial_work(tmp.path(), "1-2-cli", "Any question").await;

    assert!(summary.contains("no (clean tree)"), "clean tree should report no commit: {summary}");
}

#[tokio::test]
async fn test_preserve_partial_work_on_story_branch() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Create and switch to story branch using ensure_story_branch
    ensure_story_branch(tmp.path(), "story/1-2-cli", "main")
        .expect("ensure_story_branch should succeed");
    assert_eq!(head_branch(tmp.path()), "story/1-2-cli");

    // Create dirty files
    std::fs::write(tmp.path().join("feature.rs"), "fn feature() {}").expect("write failed");

    let summary = preserve_partial_work(tmp.path(), "1-2-cli", "Blocked on API spec").await;

    assert!(summary.contains("WIP commit: yes"), "should create WIP commit: {summary}");
    assert!(summary.contains("story/1-2-cli"), "summary should mention story branch: {summary}");

    // Verify the commit is on the story branch
    let log_output = Command::new("git")
        .args(["log", "--oneline", "-3"])
        .current_dir(tmp.path())
        .output()
        .expect("git log failed");
    let log = String::from_utf8_lossy(&log_output.stdout);
    assert!(log.contains("WIP") || log.contains("escalated"),
        "commit message should contain WIP or escalated marker: {log}");

    // Verify we're still on story branch
    assert_eq!(head_branch(tmp.path()), "story/1-2-cli");
}

// ===========================================================================
// Task 7: Cross-module integration tests (AC ALL)
// ===========================================================================

#[tokio::test]
async fn test_cross_module_full_lifecycle() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // 1. Create StoryInfo + determine base + ensure branch
    let story = make_story("1-2-cli", vec![]);
    let base = determine_base_branch(&story, tmp.path(), "main");
    assert_eq!(base, "main");

    ensure_story_branch(tmp.path(), &story.branch_name, &base)
        .expect("ensure_story_branch should succeed");
    assert_eq!(head_branch(tmp.path()), "story/1-2-cli");

    // 2. Use GitTool to write+add+commit on that branch
    let tool = GitTool::new(tmp.path().to_path_buf());
    std::fs::write(tmp.path().join("impl.rs"), "fn impl_code() {}").expect("write failed");

    let mut add_args = git_args("add");
    add_args.paths = Some(vec!["impl.rs".to_string()]);
    tool.call(add_args).await.expect("add should succeed");

    let mut commit_args = git_args("commit");
    commit_args.message = Some("feat: implement story 1-2".to_string());
    tool.call(commit_args).await.expect("commit should succeed");

    // 3. Create more dirty changes + preserve_partial_work
    std::fs::write(tmp.path().join("wip.rs"), "fn wip() {}").expect("write failed");

    let summary = preserve_partial_work(tmp.path(), "1-2-cli", "Need API clarification").await;
    assert!(summary.contains("WIP commit: yes"), "should create WIP commit: {summary}");

    // 4. Verify both commits exist on the story branch
    let log_output = Command::new("git")
        .args(["log", "--oneline", "-5"])
        .current_dir(tmp.path())
        .output()
        .expect("git log failed");
    let log = String::from_utf8_lossy(&log_output.stdout);
    assert!(log.contains("feat: implement story 1-2"), "should have implementation commit: {log}");
    assert!(log.contains("WIP") || log.contains("escalated"), "should have WIP commit: {log}");
    assert_eq!(head_branch(tmp.path()), "story/1-2-cli", "should still be on story branch");
}

#[tokio::test]
async fn test_cross_module_branch_consistency() {
    let tmp = TempDir::new().expect("tempdir failed");
    create_test_repo(tmp.path());

    // Create branch via ensure_story_branch
    ensure_story_branch(tmp.path(), "story/x", "main")
        .expect("ensure_story_branch should succeed");
    assert_eq!(head_branch(tmp.path()), "story/x");

    // Write and commit a file on story/x so we can verify state later
    std::fs::write(tmp.path().join("story_x_file.txt"), "content").expect("write failed");
    let tool = GitTool::new(tmp.path().to_path_buf());

    let mut add_args = git_args("add");
    add_args.paths = Some(vec!["story_x_file.txt".to_string()]);
    tool.call(add_args).await.expect("add should succeed");

    let mut commit_args = git_args("commit");
    commit_args.message = Some("file on story/x".to_string());
    tool.call(commit_args).await.expect("commit should succeed");

    // Switch to main via GitTool
    let mut checkout_main = git_args("checkout");
    checkout_main.branch = Some("main".to_string());
    tool.call(checkout_main).await.expect("checkout main should succeed");
    assert_eq!(head_branch(tmp.path()), "main");

    // File should NOT exist on main
    assert!(!tmp.path().join("story_x_file.txt").exists(),
        "story file should not be on main branch");

    // Switch back to story/x via GitTool
    let mut checkout_story = git_args("checkout");
    checkout_story.branch = Some("story/x".to_string());
    tool.call(checkout_story).await.expect("checkout story/x should succeed");
    assert_eq!(head_branch(tmp.path()), "story/x");

    // File should exist again
    assert!(tmp.path().join("story_x_file.txt").exists(),
        "story file should be back after checking out story/x");
    let content = std::fs::read_to_string(tmp.path().join("story_x_file.txt"))
        .expect("read failed");
    assert_eq!(content, "content", "file content should be preserved");
}
