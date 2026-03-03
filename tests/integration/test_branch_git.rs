//! Integration tests for branch management (session::branch, session::cleanup)
//! and GitTool operations on real temp repositories.
//!
//! Story 7.8 — verifies cross-module git state consistency.

use bmad_bot::session::branch::{ensure_story_branch, determine_base_branch, BranchAction, BranchError};
use bmad_bot::session::cleanup::preserve_partial_work;
use bmad_bot::tools::git::{GitTool, GitToolArgs};
use rig::tool::Tool; // REQUIRED — GitTool::call() comes from this trait
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

use crate::helpers::fixtures::{create_test_repo, make_test_story};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build GitToolArgs with only the fields needed — all others default to None.
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

/// Get the current HEAD branch name via git CLI.
fn current_branch(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo)
        .output()
        .expect("git branch --show-current failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Get the commit SHA that a branch points to.
fn branch_commit(repo: &Path, branch: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", branch])
        .current_dir(repo)
        .output()
        .expect("git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Check if a branch exists locally.
fn branch_exists_local(repo: &Path, branch: &str) -> bool {
    let output = Command::new("git")
        .args(["branch", "--list", branch])
        .current_dir(repo)
        .output()
        .expect("git branch --list failed");
    output.status.success() && !output.stdout.is_empty()
}

/// Create a branch with at least one commit so it has a distinct tip.
fn create_branch_with_commit(repo: &Path, branch_name: &str) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
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
    // Go back to main so we don't leave HEAD on the new branch
    run(&["checkout", "main"]);
}

/// Write a file into the repo working tree.
fn write_file(repo: &Path, name: &str, content: &str) {
    std::fs::write(repo.join(name), content).expect("write file failed");
}

// ===========================================================================
// Task 2: ensure_story_branch integration tests (AC #1, #2)
// ===========================================================================

#[test]
fn test_ensure_branch_creates_new_from_main() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let result = ensure_story_branch(dir.path(), "story/1-2-cli", "main")
        .expect("ensure_story_branch should succeed");

    match &result {
        BranchAction::Created { branch_name, base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli");
            assert_eq!(base_branch, "main");
        }
        BranchAction::Reused { .. } => panic!("Expected Created, got Reused"),
    }

    assert_eq!(current_branch(dir.path()), "story/1-2-cli");
}

#[test]
fn test_ensure_branch_reuses_existing() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    // First call — creates
    let first = ensure_story_branch(dir.path(), "story/1-2-cli", "main")
        .expect("first call should succeed");
    assert!(matches!(first, BranchAction::Created { .. }));

    // Second call — reuses
    let second = ensure_story_branch(dir.path(), "story/1-2-cli", "main")
        .expect("second call should succeed");
    match &second {
        BranchAction::Reused { branch_name } => {
            assert_eq!(branch_name, "story/1-2-cli");
        }
        BranchAction::Created { .. } => panic!("Expected Reused, got Created"),
    }

    assert_eq!(current_branch(dir.path()), "story/1-2-cli");
}

#[test]
fn test_ensure_branch_creates_from_non_main_parent() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    // Create a parent branch with its own commit
    create_branch_with_commit(dir.path(), "story/1-1-scaffolding");

    let parent_tip = branch_commit(dir.path(), "story/1-1-scaffolding");

    // Create a new branch from the parent
    let result = ensure_story_branch(dir.path(), "story/1-2-cli", "story/1-1-scaffolding")
        .expect("ensure_story_branch should succeed");

    match &result {
        BranchAction::Created { branch_name, base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli");
            assert_eq!(base_branch, "story/1-1-scaffolding");
        }
        BranchAction::Reused { .. } => panic!("Expected Created, got Reused"),
    }

    // The new branch should start from the parent's tip commit
    let new_tip = branch_commit(dir.path(), "story/1-2-cli");
    // The new branch HEAD should be at the same commit as the parent tip
    // (since checkout -b creates at the tip of base)
    assert_eq!(new_tip, parent_tip, "New branch should start at parent tip");
}

#[test]
fn test_ensure_branch_base_not_found() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let result = ensure_story_branch(dir.path(), "story/1-2-cli", "nonexistent-branch");

    match result {
        Err(BranchError::BaseBranchNotFound { branch }) => {
            assert_eq!(branch, "nonexistent-branch");
        }
        other => panic!("Expected BaseBranchNotFound, got: {other:?}"),
    }
}

#[test]
fn test_ensure_branch_non_git_directory() {
    let dir = TempDir::new().expect("tempdir");
    // Don't init a git repo — just a plain directory

    let result = ensure_story_branch(dir.path(), "story/1-2-cli", "main");

    // Should get an error (BaseBranchNotFound or similar — the base doesn't exist
    // because git itself can't operate)
    assert!(result.is_err(), "Expected error on non-git directory, got: {result:?}");
}

// ===========================================================================
// Task 3: determine_base_branch integration tests (AC #3, #4)
// ===========================================================================

#[test]
fn test_determine_base_no_deps_returns_main() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let story = make_test_story("1-2-cli", "cli", vec![]);
    let base = determine_base_branch(&story, dir.path(), "main");
    assert_eq!(base, "main");
}

#[test]
fn test_determine_base_dep_branch_exists() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());
    create_branch_with_commit(dir.path(), "story/1-1-scaffolding");

    let story = make_test_story("1-2-cli", "cli", vec!["1-1-scaffolding".to_string()]);
    let base = determine_base_branch(&story, dir.path(), "main");
    assert_eq!(base, "story/1-1-scaffolding");
}

#[test]
fn test_determine_base_dep_branch_missing_falls_back() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    // Dependency branch does NOT exist locally
    let story = make_test_story("1-2-cli", "cli", vec!["1-1-scaffolding".to_string()]);
    let base = determine_base_branch(&story, dir.path(), "main");
    assert_eq!(base, "main");
}

#[test]
fn test_determine_base_multiple_deps_uses_last() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());
    create_branch_with_commit(dir.path(), "story/1-1-scaffolding");
    create_branch_with_commit(dir.path(), "story/1-2-cli");

    let story = make_test_story(
        "1-3-config",
        "config",
        vec!["1-1-scaffolding".to_string(), "1-2-cli".to_string()],
    );
    let base = determine_base_branch(&story, dir.path(), "main");
    assert_eq!(base, "story/1-2-cli", "Should use LAST dependency branch");
}

// ===========================================================================
// Task 4: End-to-end branch flow integration tests (AC #1, #3, #4)
// ===========================================================================

#[test]
fn test_e2e_determine_then_ensure_chained() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());
    create_branch_with_commit(dir.path(), "story/1-1-scaffolding");

    let story = make_test_story("1-2-cli", "cli", vec!["1-1-scaffolding".to_string()]);
    let base = determine_base_branch(&story, dir.path(), "main");
    assert_eq!(base, "story/1-1-scaffolding");

    let result = ensure_story_branch(dir.path(), &story.branch_name, &base)
        .expect("ensure should succeed");
    match &result {
        BranchAction::Created { branch_name, base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli");
            assert_eq!(base_branch, "story/1-1-scaffolding");
        }
        BranchAction::Reused { .. } => panic!("Expected Created"),
    }
    assert_eq!(current_branch(dir.path()), "story/1-2-cli");
}

#[test]
fn test_e2e_multi_story_chain() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let main_tip = branch_commit(dir.path(), "main");

    // Story 1-1: no deps → base = main
    let s1 = make_test_story("1-1-scaffolding", "scaffolding", vec![]);
    let base1 = determine_base_branch(&s1, dir.path(), "main");
    assert_eq!(base1, "main");
    ensure_story_branch(dir.path(), &s1.branch_name, &base1).expect("s1 branch");
    // HEAD is now on story/1-1-scaffolding. Its parent commit = main tip
    assert_eq!(
        branch_commit(dir.path(), "story/1-1-scaffolding"),
        main_tip,
        "story/1-1 should start at main tip"
    );

    // Go back to main
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("git failed");
    };
    // Add a commit on story/1-1 so it diverges from main
    run(&["commit", "--allow-empty", "-m", "work on 1-1"]);
    let s1_tip = branch_commit(dir.path(), "story/1-1-scaffolding");
    run(&["checkout", "main"]);

    // Story 1-2: deps on 1-1 → base = story/1-1-scaffolding
    let s2 = make_test_story("1-2-cli", "cli", vec!["1-1-scaffolding".to_string()]);
    let base2 = determine_base_branch(&s2, dir.path(), "main");
    assert_eq!(base2, "story/1-1-scaffolding");
    ensure_story_branch(dir.path(), &s2.branch_name, &base2).expect("s2 branch");
    assert_eq!(
        branch_commit(dir.path(), "story/1-2-cli"),
        s1_tip,
        "story/1-2 should start at story/1-1 tip"
    );

    // Add a commit on story/1-2
    run(&["commit", "--allow-empty", "-m", "work on 1-2"]);
    let s2_tip = branch_commit(dir.path(), "story/1-2-cli");
    run(&["checkout", "main"]);

    // Story 1-3: deps on 1-2 → base = story/1-2-cli
    let s3 = make_test_story("1-3-config", "config", vec!["1-2-cli".to_string()]);
    let base3 = determine_base_branch(&s3, dir.path(), "main");
    assert_eq!(base3, "story/1-2-cli");
    ensure_story_branch(dir.path(), &s3.branch_name, &base3).expect("s3 branch");
    assert_eq!(
        branch_commit(dir.path(), "story/1-3-config"),
        s2_tip,
        "story/1-3 should start at story/1-2 tip"
    );
}

#[test]
fn test_e2e_dep_branch_missing_falls_back_to_main() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let main_tip = branch_commit(dir.path(), "main");

    // Story with a dependency whose branch doesn't exist (merged to main already)
    let story = make_test_story("1-2-cli", "cli", vec!["1-1-scaffolding".to_string()]);
    let base = determine_base_branch(&story, dir.path(), "main");
    assert_eq!(base, "main", "Should fall back to main when dep branch missing");

    ensure_story_branch(dir.path(), &story.branch_name, &base).expect("ensure should succeed");
    assert_eq!(
        branch_commit(dir.path(), "story/1-2-cli"),
        main_tip,
        "Should branch from main"
    );
}

// ===========================================================================
// Task 5: GitTool integration tests (AC #1, #5) — LOCAL ACTIONS ONLY
// ===========================================================================

#[tokio::test]
async fn test_git_tool_branch_create_and_checkout() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let tool = GitTool::new(dir.path().to_path_buf());

    // branch_create
    let mut args = git_args("branch_create");
    args.branch = Some("story/1-2-cli".to_string());
    let result = tool.call(args).await.expect("branch_create should succeed");
    assert!(
        result.contains("story/1-2-cli"),
        "branch_create output should mention the branch: {result}"
    );

    // Verify branch exists
    assert!(branch_exists_local(dir.path(), "story/1-2-cli"));
    assert_eq!(current_branch(dir.path()), "story/1-2-cli");

    // checkout back to main
    let mut args = git_args("checkout");
    args.branch = Some("main".to_string());
    tool.call(args).await.expect("checkout main should succeed");
    assert_eq!(current_branch(dir.path()), "main");

    // checkout story branch again
    let mut args = git_args("checkout");
    args.branch = Some("story/1-2-cli".to_string());
    tool.call(args).await.expect("checkout story branch should succeed");
    assert_eq!(current_branch(dir.path()), "story/1-2-cli");
}

#[tokio::test]
async fn test_git_tool_add_and_commit() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let tool = GitTool::new(dir.path().to_path_buf());

    // Create a file
    write_file(dir.path(), "hello.txt", "hello world");

    // add
    let mut args = git_args("add");
    args.paths = Some(vec!["hello.txt".to_string()]);
    tool.call(args).await.expect("add should succeed");

    // commit
    let mut args = git_args("commit");
    args.message = Some("add hello.txt".to_string());
    tool.call(args).await.expect("commit should succeed");

    // log — verify commit exists
    let mut args = git_args("log");
    args.max_count = Some(5);
    let log_output = tool.call(args).await.expect("log should succeed");
    assert!(
        log_output.contains("add hello.txt"),
        "log should contain commit message: {log_output}"
    );
}

#[tokio::test]
async fn test_git_tool_status_dirty_and_clean() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let tool = GitTool::new(dir.path().to_path_buf());

    // Clean tree
    let args = git_args("status");
    let status_clean = tool.call(args).await.expect("status should succeed");
    assert!(
        status_clean.contains("Clean working directory") || status_clean.contains("nothing to commit"),
        "Clean repo status should indicate clean: {status_clean}"
    );

    // Dirty tree
    write_file(dir.path(), "dirty.txt", "some changes");
    let args = git_args("status");
    let status_dirty = tool.call(args).await.expect("status should succeed");
    assert!(
        status_dirty.contains("dirty.txt"),
        "Dirty status should show the new file: {status_dirty}"
    );
}

#[tokio::test]
async fn test_git_tool_diff_shows_uncommitted() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let tool = GitTool::new(dir.path().to_path_buf());

    // Create and add a file, commit it
    write_file(dir.path(), "tracked.txt", "original");
    let mut args = git_args("add");
    args.paths = Some(vec!["tracked.txt".to_string()]);
    tool.call(args).await.expect("add should succeed");
    let mut args = git_args("commit");
    args.message = Some("add tracked.txt".to_string());
    tool.call(args).await.expect("commit should succeed");

    // Modify the file
    write_file(dir.path(), "tracked.txt", "modified content");

    // diff should show changes
    let args = git_args("diff");
    let diff_output = tool.call(args).await.expect("diff should succeed");
    assert!(
        diff_output.contains("modified content") || diff_output.contains("tracked.txt"),
        "diff should show changes: {diff_output}"
    );
}

#[tokio::test]
async fn test_git_tool_full_roundtrip() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let tool = GitTool::new(dir.path().to_path_buf());

    // branch_create
    let mut args = git_args("branch_create");
    args.branch = Some("story/2-1-feature".to_string());
    tool.call(args).await.expect("branch_create should succeed");

    // write files
    write_file(dir.path(), "feature.rs", "fn main() {}");
    write_file(dir.path(), "test.rs", "fn test() {}");

    // add
    let mut args = git_args("add");
    args.paths = Some(vec!["feature.rs".to_string(), "test.rs".to_string()]);
    tool.call(args).await.expect("add should succeed");

    // commit
    let commit_msg = "feat: implement feature 2-1";
    let mut args = git_args("commit");
    args.message = Some(commit_msg.to_string());
    tool.call(args).await.expect("commit should succeed");

    // log — verify commit
    let mut args = git_args("log");
    args.max_count = Some(3);
    let log_output = tool.call(args).await.expect("log should succeed");
    assert!(
        log_output.contains(commit_msg),
        "log should contain commit message: {log_output}"
    );

    // Verify we're on the correct branch
    assert_eq!(current_branch(dir.path()), "story/2-1-feature");
}

// ===========================================================================
// Task 6: preserve_partial_work integration tests (AC #5)
// ===========================================================================

#[tokio::test]
async fn test_preserve_dirty_tree_creates_wip_commit() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    // Create dirty state
    write_file(dir.path(), "work.txt", "partial work");

    let summary = preserve_partial_work(dir.path(), "1-2-cli", "What should this do?").await;

    assert!(
        summary.contains("WIP commit: yes"),
        "Summary should contain 'WIP commit: yes': {summary}"
    );
    assert!(
        summary.contains("work.txt"),
        "Summary should contain file name: {summary}"
    );

    // Verify the commit actually exists in the log
    let output = Command::new("git")
        .args(["log", "--oneline", "-n", "1"])
        .current_dir(dir.path())
        .output()
        .expect("git log failed");
    let log_line = String::from_utf8_lossy(&output.stdout);
    assert!(
        log_line.contains("WIP"),
        "Latest commit should be a WIP commit: {log_line}"
    );
}

#[tokio::test]
async fn test_preserve_clean_tree_no_commit() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    let summary = preserve_partial_work(dir.path(), "1-2-cli", "Question?").await;

    assert!(
        summary.contains("no (clean tree)"),
        "Summary should contain 'no (clean tree)': {summary}"
    );
}

#[tokio::test]
async fn test_preserve_on_story_branch_contains_story_key() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    // Create story branch
    ensure_story_branch(dir.path(), "story/1-2-cli", "main")
        .expect("ensure branch should succeed");
    assert_eq!(current_branch(dir.path()), "story/1-2-cli");

    // Create dirty state
    write_file(dir.path(), "impl.rs", "fn new_feature() {}");

    let summary = preserve_partial_work(dir.path(), "1-2-cli", "Need clarification").await;

    assert!(
        summary.contains("WIP commit: yes"),
        "Should create WIP commit: {summary}"
    );
    assert!(
        summary.contains("story/1-2-cli"),
        "Summary should contain branch name: {summary}"
    );

    // Verify the WIP commit is on the story branch
    let output = Command::new("git")
        .args(["log", "--oneline", "-n", "1"])
        .current_dir(dir.path())
        .output()
        .expect("git log failed");
    let log_line = String::from_utf8_lossy(&output.stdout);
    assert!(
        log_line.contains("WIP"),
        "Latest commit on story branch should be WIP: {log_line}"
    );

    // Verify HEAD is still on the story branch
    assert_eq!(current_branch(dir.path()), "story/1-2-cli");
}

// ===========================================================================
// Task 7: Cross-module integration tests (AC: ALL)
// ===========================================================================

#[tokio::test]
async fn test_cross_module_full_lifecycle() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    // Step 1: determine_base_branch for a story with no deps
    let story = make_test_story("1-2-cli", "cli", vec![]);
    let base = determine_base_branch(&story, dir.path(), "main");
    assert_eq!(base, "main");

    // Step 2: ensure_story_branch
    ensure_story_branch(dir.path(), &story.branch_name, &base)
        .expect("ensure should succeed");
    assert_eq!(current_branch(dir.path()), "story/1-2-cli");

    // Step 3: Use GitTool to write+add+commit on that branch
    let tool = GitTool::new(dir.path().to_path_buf());

    write_file(dir.path(), "feature.rs", "fn feature() {}");
    let mut args = git_args("add");
    args.paths = Some(vec!["feature.rs".to_string()]);
    tool.call(args).await.expect("add should succeed");

    let mut args = git_args("commit");
    args.message = Some("feat: add feature".to_string());
    tool.call(args).await.expect("commit should succeed");

    // Step 4: Create additional dirty changes and preserve
    write_file(dir.path(), "more-work.txt", "additional changes");
    let summary = preserve_partial_work(dir.path(), "1-2-cli", "Need clarification").await;
    assert!(
        summary.contains("WIP commit: yes"),
        "Should preserve partial work: {summary}"
    );

    // Step 5: Verify both commits exist on the story branch
    let output = Command::new("git")
        .args(["log", "--oneline", "-n", "5"])
        .current_dir(dir.path())
        .output()
        .expect("git log failed");
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(
        log.contains("feat: add feature"),
        "GitTool commit should be in log: {log}"
    );
    assert!(
        log.contains("WIP"),
        "WIP commit should be in log: {log}"
    );

    // Verify we're still on the story branch
    assert_eq!(current_branch(dir.path()), "story/1-2-cli");
}

#[tokio::test]
async fn test_cross_module_branch_switching_consistency() {
    let dir = TempDir::new().expect("tempdir");
    create_test_repo(dir.path());

    // Create branch via ensure_story_branch
    ensure_story_branch(dir.path(), "story/x", "main")
        .expect("ensure should succeed");
    assert_eq!(current_branch(dir.path()), "story/x");

    let tool = GitTool::new(dir.path().to_path_buf());

    // Switch to main via GitTool
    let mut args = git_args("checkout");
    args.branch = Some("main".to_string());
    tool.call(args).await.expect("checkout main should succeed");
    assert_eq!(current_branch(dir.path()), "main");

    // Switch back to story branch via GitTool
    let mut args = git_args("checkout");
    args.branch = Some("story/x".to_string());
    tool.call(args).await.expect("checkout story/x should succeed");
    assert_eq!(current_branch(dir.path()), "story/x");
}
