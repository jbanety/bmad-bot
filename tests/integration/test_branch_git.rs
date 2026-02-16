//! Integration tests for branch management (`session::branch`, `session::cleanup`)
//! and `GitTool` (`tools::git`) operating on real temp git repositories.
//!
//! Story 7.8 — verifies cross-module git state consistency.

use bmad_bot::session::branch::{ensure_story_branch, determine_base_branch, BranchAction, BranchError};
use bmad_bot::session::cleanup::preserve_partial_work;
use bmad_bot::tools::git::{GitTool, GitToolArgs};
use rig::tool::Tool; // REQUIRED — GitTool::call() comes from this trait
use std::path::Path;
use tempfile::TempDir;

use crate::helpers::fixtures::{create_test_repo, make_test_story};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build `GitToolArgs` with only `action` set — all others `None`.
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

/// Create a branch with a commit on a temp repo (CLI-only, no git2).
fn create_branch_with_commit(repo_path: &Path, branch_name: &str) {
    let run = |args: &[&str]| {
        let output = std::process::Command::new("git")
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
    run(&["commit", "--allow-empty", "-m", &format!("work on {branch_name}")]);
}

/// Return the current HEAD branch name.
fn current_branch(repo_path: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_path)
        .output()
        .expect("git branch --show-current failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Return the short SHA of HEAD.
fn head_sha(repo_path: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Return the full SHA of HEAD.
fn head_sha_full(repo_path: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Write a file in the repo directory.
fn write_file(repo_path: &Path, name: &str, content: &str) {
    std::fs::write(repo_path.join(name), content).expect("Failed to write file");
}

// ===========================================================================
// Task 2: ensure_story_branch integration tests (AC: #1, #2)
// ===========================================================================

#[test]
fn test_ensure_story_branch_creates_new_branch_from_main() {
    // AC #1: create new branch from main → BranchAction::Created, HEAD on new branch
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    let result = ensure_story_branch(dir, "story/1-2-cli", "main")
        .expect("ensure_story_branch should succeed");

    match &result {
        BranchAction::Created { branch_name, base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli", "branch_name mismatch");
            assert_eq!(base_branch, "main", "base_branch mismatch");
        }
        BranchAction::Reused { .. } => panic!("Expected Created, got Reused"),
    }
    assert_eq!(current_branch(dir), "story/1-2-cli", "HEAD should be on the new branch");
}

#[test]
fn test_ensure_story_branch_reuses_existing_branch() {
    // AC #2: calling again returns Reused, no error
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // First call — creates the branch
    let _ = ensure_story_branch(dir, "story/1-2-cli", "main")
        .expect("first call should succeed");

    // Switch back to main so reuse actually switches
    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(dir)
        .output()
        .expect("checkout main failed");

    // Second call — reuses the branch
    let result = ensure_story_branch(dir, "story/1-2-cli", "main")
        .expect("second call should succeed");

    match &result {
        BranchAction::Reused { branch_name } => {
            assert_eq!(branch_name, "story/1-2-cli", "branch_name mismatch");
        }
        BranchAction::Created { .. } => panic!("Expected Reused, got Created"),
    }
    assert_eq!(current_branch(dir), "story/1-2-cli", "HEAD should be on reused branch");
}

#[test]
fn test_ensure_story_branch_creates_from_non_main_parent() {
    // Task 2.3: create branch from a non-main parent → verify correct base commit
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // Create parent branch with a unique commit
    create_branch_with_commit(dir, "story/1-1-scaffolding");
    let parent_head = head_sha_full(dir);

    // Create child branch from parent
    let result = ensure_story_branch(dir, "story/1-2-cli", "story/1-1-scaffolding")
        .expect("should succeed");

    match &result {
        BranchAction::Created { branch_name, base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli");
            assert_eq!(base_branch, "story/1-1-scaffolding");
        }
        BranchAction::Reused { .. } => panic!("Expected Created"),
    }

    // The new branch HEAD should be the same as parent's HEAD (branched from it)
    let child_head = head_sha_full(dir);
    assert_eq!(child_head, parent_head, "child branch should start from parent branch's HEAD");
}

#[test]
fn test_ensure_story_branch_base_not_found_returns_error() {
    // Task 2.4: base branch does not exist → BranchError::BaseBranchNotFound
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    let result = ensure_story_branch(dir, "story/1-2-cli", "nonexistent-base");

    match result {
        Err(BranchError::BaseBranchNotFound { branch }) => {
            assert_eq!(branch, "nonexistent-base", "error should name the missing base");
        }
        Err(other) => panic!("Expected BaseBranchNotFound, got: {other:?}"),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

#[test]
fn test_ensure_story_branch_non_git_directory_returns_error() {
    // Task 2.5: call on non-git directory → returns an error
    // Note: BranchError::RepoOpenFailed does not exist in the enum; the actual
    // behavior is BaseBranchNotFound because branch_exists returns false for
    // both the target and base branches on a non-git directory.
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    // Intentionally do NOT call create_test_repo — this is just a plain directory.

    let result = ensure_story_branch(dir, "story/1-2-cli", "main");
    assert!(result.is_err(), "should fail on non-git directory");
}

// ===========================================================================
// Task 3: determine_base_branch integration tests (AC: #3, #4)
// ===========================================================================

#[test]
fn test_determine_base_branch_no_deps_returns_main() {
    // AC #4: no dependencies → returns "main"
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    let story = make_test_story("1-2-cli", "cli", vec![]);
    let base = determine_base_branch(&story, dir, "main");
    assert_eq!(base, "main", "no deps should return default branch");
}

#[test]
fn test_determine_base_branch_dep_branch_exists_returns_parent() {
    // AC #3: dependency branch exists → returns "story/{dep_key}"
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // Create the dependency branch
    create_branch_with_commit(dir, "story/1-1-scaffolding");
    // Switch back to main
    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(dir)
        .output()
        .expect("checkout main");

    let story = make_test_story(
        "1-2-cli",
        "cli",
        vec!["1-1-scaffolding".to_string()],
    );
    let base = determine_base_branch(&story, dir, "main");
    assert_eq!(base, "story/1-1-scaffolding", "should chain from dependency branch");
}

#[test]
fn test_determine_base_branch_dep_branch_missing_returns_main() {
    // Task 3.3: dependency branch doesn't exist → fallback to "main"
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    let story = make_test_story(
        "1-2-cli",
        "cli",
        vec!["1-1-scaffolding".to_string()],
    );
    let base = determine_base_branch(&story, dir, "main");
    assert_eq!(base, "main", "missing dep branch should fallback to main");
}

#[test]
fn test_determine_base_branch_multiple_deps_uses_last() {
    // Task 3.4: multiple dependencies → uses LAST dependency's branch
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // Create both dependency branches
    create_branch_with_commit(dir, "story/1-1-scaffolding");
    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(dir)
        .output()
        .expect("checkout main");
    create_branch_with_commit(dir, "story/1-2-cli");
    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(dir)
        .output()
        .expect("checkout main");

    let story = make_test_story(
        "1-3-init",
        "init",
        vec!["1-1-scaffolding".to_string(), "1-2-cli".to_string()],
    );
    let base = determine_base_branch(&story, dir, "main");
    assert_eq!(base, "story/1-2-cli", "should use the LAST dependency branch");
}

// ===========================================================================
// Task 4: End-to-end branch flow integration tests (AC: #1, #3, #4)
// ===========================================================================

#[test]
fn test_e2e_determine_then_ensure_creates_chained_branch() {
    // Task 4.1: determine_base_branch → ensure_story_branch → verify chained state
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // Create dependency branch
    create_branch_with_commit(dir, "story/1-1-scaffolding");
    let dep_head = head_sha_full(dir);
    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(dir)
        .output()
        .expect("checkout main");

    // Determine base
    let story = make_test_story(
        "1-2-cli",
        "cli",
        vec!["1-1-scaffolding".to_string()],
    );
    let base = determine_base_branch(&story, dir, "main");
    assert_eq!(base, "story/1-1-scaffolding");

    // Ensure story branch from that base
    let result = ensure_story_branch(dir, "story/1-2-cli", &base)
        .expect("should succeed");
    match &result {
        BranchAction::Created { branch_name, base_branch } => {
            assert_eq!(branch_name, "story/1-2-cli");
            assert_eq!(base_branch, "story/1-1-scaffolding");
        }
        _ => panic!("Expected Created"),
    }

    // Verify HEAD commit is the dep's HEAD (branched from it)
    let new_head = head_sha_full(dir);
    assert_eq!(new_head, dep_head, "new branch should start at dependency's HEAD");
    assert_eq!(current_branch(dir), "story/1-2-cli");
}

#[test]
fn test_e2e_multi_story_chain() {
    // Task 4.2: multi-story chain — 1-1 → 1-2 → 1-3, each from previous
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // Create story/1-1
    let _ = ensure_story_branch(dir, "story/1-1-scaffolding", "main")
        .expect("1-1 create");
    // Add a unique commit on 1-1
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "1-1 work"])
        .current_dir(dir)
        .output()
        .expect("commit on 1-1");
    let sha_1_1 = head_sha_full(dir);

    // Create story/1-2 from 1-1
    let _ = ensure_story_branch(dir, "story/1-2-cli", "story/1-1-scaffolding")
        .expect("1-2 create");
    // Parent of 1-2 HEAD should be the initial commit; HEAD should be 1-1's HEAD
    assert_eq!(head_sha_full(dir), sha_1_1, "1-2 should start at 1-1's HEAD");
    // Add a unique commit on 1-2
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "1-2 work"])
        .current_dir(dir)
        .output()
        .expect("commit on 1-2");
    let sha_1_2 = head_sha_full(dir);

    // Create story/1-3 from 1-2
    let _ = ensure_story_branch(dir, "story/1-3-init", "story/1-2-cli")
        .expect("1-3 create");
    assert_eq!(head_sha_full(dir), sha_1_2, "1-3 should start at 1-2's HEAD");
    assert_eq!(current_branch(dir), "story/1-3-init");
}

#[test]
fn test_e2e_dep_branch_missing_falls_back_to_main() {
    // Task 4.3: dependency branch missing → falls back to main → create from main
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);
    let main_head = head_sha_full(dir);

    let story = make_test_story(
        "1-2-cli",
        "cli",
        vec!["1-1-scaffolding".to_string()],
    );
    let base = determine_base_branch(&story, dir, "main");
    assert_eq!(base, "main", "missing dep should fallback");

    let result = ensure_story_branch(dir, "story/1-2-cli", &base)
        .expect("should succeed");
    match &result {
        BranchAction::Created { base_branch, .. } => {
            assert_eq!(base_branch, "main");
        }
        _ => panic!("Expected Created"),
    }

    assert_eq!(head_sha_full(dir), main_head, "branch should start at main's HEAD");
}

// ===========================================================================
// Task 5: GitTool integration tests (AC: #1, #5) — LOCAL ACTIONS ONLY
// ===========================================================================

#[tokio::test]
async fn test_git_tool_branch_create_and_checkout() {
    // Task 5.1: branch_create + checkout → verify branch exists, HEAD on it
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    let tool = GitTool::new(dir.to_path_buf());

    // branch_create
    let mut args = git_args("branch_create");
    args.branch = Some("feature/test".to_string());
    let result = tool.call(args).await.expect("branch_create should succeed");
    assert!(result.contains("feature/test"), "output should mention branch: {result}");

    assert_eq!(current_branch(dir), "feature/test", "HEAD should be on new branch");

    // checkout back to main
    let mut args = git_args("checkout");
    args.branch = Some("main".to_string());
    let _ = tool.call(args).await.expect("checkout main should succeed");
    assert_eq!(current_branch(dir), "main");

    // checkout back to feature branch
    let mut args = git_args("checkout");
    args.branch = Some("feature/test".to_string());
    let _ = tool.call(args).await.expect("checkout feature should succeed");
    assert_eq!(current_branch(dir), "feature/test");
}

#[tokio::test]
async fn test_git_tool_add_commit_log() {
    // Task 5.2: add + commit → verify commit in log output
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    write_file(dir, "hello.txt", "hello world");

    let tool = GitTool::new(dir.to_path_buf());

    // add
    let mut args = git_args("add");
    args.paths = Some(vec!["hello.txt".to_string()]);
    let _ = tool.call(args).await.expect("add should succeed");

    // commit
    let mut args = git_args("commit");
    args.message = Some("add hello.txt".to_string());
    let result = tool.call(args).await.expect("commit should succeed");
    assert!(result.contains("add hello.txt"), "commit output should mention message: {result}");

    // log
    let mut args = git_args("log");
    args.max_count = Some(5);
    let log = tool.call(args).await.expect("log should succeed");
    assert!(log.contains("add hello.txt"), "log should contain commit message: {log}");
}

#[tokio::test]
async fn test_git_tool_status_dirty_and_clean() {
    // Task 5.3: status on dirty tree → shows files; clean tree → "Clean working directory"
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    let tool = GitTool::new(dir.to_path_buf());

    // Clean tree status
    let status = tool.call(git_args("status")).await.expect("status should succeed");
    assert!(
        status.contains("Clean working directory") || status.contains("nothing to commit"),
        "clean status unexpected: {status}"
    );

    // Make tree dirty
    write_file(dir, "dirty.txt", "dirty");

    let status = tool.call(git_args("status")).await.expect("status should succeed");
    assert!(status.contains("dirty.txt"), "dirty status should mention file: {status}");
}

#[tokio::test]
async fn test_git_tool_diff_shows_changes() {
    // Task 5.4: diff shows uncommitted changes
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // Create and commit a file first
    write_file(dir, "file.txt", "original");
    let tool = GitTool::new(dir.to_path_buf());

    let mut args = git_args("add");
    args.paths = Some(vec!["file.txt".to_string()]);
    let _ = tool.call(args).await.expect("add");
    let mut args = git_args("commit");
    args.message = Some("add file".to_string());
    let _ = tool.call(args).await.expect("commit");

    // Modify the file (tracked change)
    write_file(dir, "file.txt", "modified content");

    let diff = tool.call(git_args("diff")).await.expect("diff should succeed");
    assert!(diff.contains("modified content") || diff.contains("+modified"),
        "diff should show changes: {diff}");
}

#[tokio::test]
async fn test_git_tool_full_roundtrip() {
    // Task 5.5: branch_create → write files → add → commit → log → verify
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    let tool = GitTool::new(dir.to_path_buf());

    // branch_create
    let mut args = git_args("branch_create");
    args.branch = Some("story/7-8-test".to_string());
    let _ = tool.call(args).await.expect("branch_create");
    assert_eq!(current_branch(dir), "story/7-8-test");

    // Write files
    write_file(dir, "feature.rs", "// feature code");
    write_file(dir, "README.md", "# Test");

    // add
    let mut args = git_args("add");
    args.paths = Some(vec!["feature.rs".to_string(), "README.md".to_string()]);
    let _ = tool.call(args).await.expect("add");

    // commit
    let commit_msg = "feat: initial implementation for story 7-8";
    let mut args = git_args("commit");
    args.message = Some(commit_msg.to_string());
    let commit_result = tool.call(args).await.expect("commit");
    assert!(commit_result.contains("7-8"), "commit output should reference story: {commit_result}");

    // log
    let mut args = git_args("log");
    args.max_count = Some(3);
    let log = tool.call(args).await.expect("log");
    assert!(log.contains(commit_msg), "log should contain commit message: {log}");

    // Verify SHA is present in log
    let sha = head_sha(dir);
    assert!(log.contains(&sha), "log should contain HEAD SHA {sha}: {log}");
}

// ===========================================================================
// Task 6: preserve_partial_work integration tests (AC: #5)
// ===========================================================================

#[tokio::test]
async fn test_preserve_partial_work_dirty_tree_creates_wip_commit() {
    // Task 6.1: dirty tree → WIP commit created, summary contains "WIP commit: yes"
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // Make the tree dirty
    write_file(dir, "wip.txt", "work in progress");

    let summary = preserve_partial_work(dir, "1-2-cli", "What should I do?").await;

    assert!(summary.contains("WIP commit: yes"), "summary should say WIP commit: yes — got: {summary}");
    assert!(summary.contains("wip.txt"), "summary should list the dirty file — got: {summary}");

    // Verify commit actually exists in log
    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-n", "1"])
        .current_dir(dir)
        .output()
        .expect("git log failed");
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(log.contains("WIP"), "most recent commit should be a WIP commit: {log}");
}

#[tokio::test]
async fn test_preserve_partial_work_clean_tree_no_commit() {
    // Task 6.2: clean tree → no commit, summary contains "no (clean tree)"
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    let summary = preserve_partial_work(dir, "1-2-cli", "Any changes?").await;

    assert!(summary.contains("no (clean tree)"), "summary should say no commit — got: {summary}");
}

#[tokio::test]
async fn test_preserve_partial_work_on_story_branch() {
    // Task 6.3: preserve on a branch created by ensure_story_branch → WIP commit on story branch
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // Create story branch
    let _ = ensure_story_branch(dir, "story/1-2-cli", "main")
        .expect("ensure_story_branch");

    assert_eq!(current_branch(dir), "story/1-2-cli");

    // Dirty the tree
    write_file(dir, "partial.rs", "// partial work");

    let summary = preserve_partial_work(dir, "1-2-cli", "Needs review").await;

    assert!(summary.contains("WIP commit: yes"), "should have WIP commit — got: {summary}");
    assert!(summary.contains("story/1-2-cli"), "summary should reference the branch — got: {summary}");

    // Verify commit is on the story branch
    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-n", "1"])
        .current_dir(dir)
        .output()
        .expect("git log");
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(log.contains("WIP"), "most recent commit should be WIP: {log}");

    // Confirm we're still on the story branch
    assert_eq!(current_branch(dir), "story/1-2-cli");
}

// ===========================================================================
// Task 7: Cross-module integration tests (AC: ALL)
// ===========================================================================

#[tokio::test]
async fn test_cross_module_full_lifecycle() {
    // Task 7.1: StoryInfo → determine_base_branch → ensure_story_branch
    // → GitTool write+add+commit → preserve_partial_work → verify both commits
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // 1. Create StoryInfo with dependency
    let story = make_test_story(
        "1-2-cli",
        "cli",
        vec!["1-1-scaffolding".to_string()],
    );

    // 2. determine_base_branch (no dep branch → fallback to main)
    let base = determine_base_branch(&story, dir, "main");
    assert_eq!(base, "main");

    // 3. ensure_story_branch
    let result = ensure_story_branch(dir, &story.branch_name, &base)
        .expect("ensure_story_branch");
    match &result {
        BranchAction::Created { branch_name, .. } => {
            assert_eq!(branch_name, "story/1-2-cli");
        }
        _ => panic!("Expected Created"),
    }

    // 4. Use GitTool to write + add + commit
    let tool = GitTool::new(dir.to_path_buf());

    write_file(dir, "feature.rs", "// feature code");

    let mut args = git_args("add");
    args.paths = Some(vec!["feature.rs".to_string()]);
    let _ = tool.call(args).await.expect("add");

    let mut args = git_args("commit");
    args.message = Some("feat: implement feature for 1-2-cli".to_string());
    let _ = tool.call(args).await.expect("commit");

    let sha_after_feature = head_sha_full(dir);

    // 5. Create more dirty changes, then preserve_partial_work
    write_file(dir, "wip.rs", "// work in progress");

    let summary = preserve_partial_work(dir, "1-2-cli", "Needs input").await;
    assert!(summary.contains("WIP commit: yes"), "should have WIP commit — got: {summary}");

    let sha_after_wip = head_sha_full(dir);
    assert_ne!(sha_after_feature, sha_after_wip, "WIP commit should be a new commit");

    // 6. Verify both commits exist on the story branch
    let mut args = git_args("log");
    args.max_count = Some(5);
    let log = tool.call(args).await.expect("log");
    assert!(log.contains("feat: implement feature for 1-2-cli"), "log should contain feature commit: {log}");
    assert!(log.contains("WIP"), "log should contain WIP commit: {log}");

    assert_eq!(current_branch(dir), "story/1-2-cli");
}

#[tokio::test]
async fn test_cross_module_branch_switching_consistency() {
    // Task 7.2: ensure_story_branch → checkout main via GitTool → checkout back → verify consistency
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    create_test_repo(dir);

    // Create story branch and add a commit
    let _ = ensure_story_branch(dir, "story/x", "main")
        .expect("ensure_story_branch");
    write_file(dir, "story_file.txt", "story content");

    let tool = GitTool::new(dir.to_path_buf());

    let mut args = git_args("add");
    args.paths = Some(vec!["story_file.txt".to_string()]);
    let _ = tool.call(args).await.expect("add");

    let mut args = git_args("commit");
    args.message = Some("story work".to_string());
    let _ = tool.call(args).await.expect("commit");

    let story_sha = head_sha_full(dir);

    // Switch to main via GitTool
    let mut args = git_args("checkout");
    args.branch = Some("main".to_string());
    let _ = tool.call(args).await.expect("checkout main");
    assert_eq!(current_branch(dir), "main");
    // story_file.txt should NOT exist on main
    assert!(!dir.join("story_file.txt").exists(), "story file should not exist on main");

    // Switch back to story/x via GitTool
    let mut args = git_args("checkout");
    args.branch = Some("story/x".to_string());
    let _ = tool.call(args).await.expect("checkout story/x");
    assert_eq!(current_branch(dir), "story/x");
    // story_file.txt should be back
    assert!(dir.join("story_file.txt").exists(), "story file should exist on story branch");
    assert_eq!(head_sha_full(dir), story_sha, "HEAD should be the same as before switching");
}
