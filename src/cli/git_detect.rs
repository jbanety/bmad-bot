//! Git remote auto-detection for the `init` command.
//!
//! Provides utilities to detect git provider, repository owner, repository name,
//! and default branch from the local `.git` configuration. Used by the interactive
//! init wizard to pre-fill git settings and reduce manual input.
//!
//! All git operations use `std::process::Command` to invoke the `git` CLI.

use std::path::Path;

/// Git remote information extracted from the local repository.
///
/// Contains all fields needed to populate [`bmad_bot::config::GitProviderConfig`]
/// from auto-detected values.
#[derive(Debug, Clone, PartialEq)]
pub struct GitRemoteInfo {
    /// Git hosting provider if recognized (`"github"` or `"gitlab"`), `None` for unknown hosts.
    pub provider: Option<String>,
    /// The hostname extracted from the remote URL (e.g., `"github.com"`).
    pub host: String,
    /// Repository owner (organization or user).
    pub owner: String,
    /// Repository name (without `.git` suffix).
    pub repo_name: String,
    /// Default branch name detected from HEAD.
    pub default_branch: String,
    /// Name of the remote used for detection (typically `"origin"`).
    pub remote_name: String,
}

/// Result of attempting to auto-detect git remote information.
///
/// Encodes the three possible outcomes: successful detection, multiple remotes
/// requiring user selection, or no information available (silent fallback).
#[derive(Debug, Clone, PartialEq)]
pub enum GitDetectionResult {
    /// Successfully detected remote info from a single remote.
    Detected(GitRemoteInfo),
    /// Multiple remotes found but no `origin` — user must choose.
    MultipleRemotes(Vec<String>),
    /// No git repo or no remotes found — silent fallback.
    NotAvailable,
}

/// Parses a git remote URL into `(host, owner, repo_name)`.
///
/// Handles the following URL formats:
/// - SSH SCP-like: `git@github.com:owner/repo.git`
/// - HTTPS: `https://github.com/owner/repo.git`
/// - SSH with scheme: `ssh://git@github.com/owner/repo.git`
/// - SSH with port: `ssh://git@github.com:22/owner/repo.git`
///
/// The `.git` suffix and trailing slashes are stripped from the repo name.
/// Returns `None` for malformed or unparseable URLs.
pub fn parse_git_remote_url(url: &str) -> Option<(String, String, String)> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    // Try SSH with scheme: ssh://git@<host>[:<port>]/<owner>/<repo>.git
    if let Some(rest) = url.strip_prefix("ssh://") {
        return parse_ssh_scheme_url(rest);
    }

    // Try HTTPS: https://<host>/<owner>/<repo>.git
    if url.starts_with("https://") || url.starts_with("http://") {
        return parse_https_url(url);
    }

    // Try SSH SCP-like: git@<host>:<owner>/<repo>.git
    // Must contain ':' but not '://' (already handled above)
    if let Some(colon_pos) = url.find(':') {
        // Ensure it's SCP-like format (user@host:path), not a scheme
        let before_colon = &url[..colon_pos];
        if before_colon.contains('@') && !url[..colon_pos].contains('/') {
            let host_part = if let Some(at_pos) = before_colon.find('@') {
                &before_colon[at_pos + 1..]
            } else {
                before_colon
            };
            let path = &url[colon_pos + 1..];
            return parse_owner_repo_from_path(host_part, path);
        }
    }

    None
}

/// Maps a hostname to a known git provider identifier.
///
/// Returns `Some("github")` for `github.com`, `Some("gitlab")` for `gitlab.com`,
/// and `None` for any other host (including self-hosted instances).
pub fn map_host_to_provider(host: &str) -> Option<String> {
    match host.to_lowercase().as_str() {
        "github.com" => Some("github".to_string()),
        "gitlab.com" => Some("gitlab".to_string()),
        _ => None,
    }
}

/// Detects git remote information from the repository at the given path.
///
/// Discovery logic:
/// 1. Verifies a git repo exists via `git rev-parse --git-dir`
/// 2. Lists remotes via `git remote`
/// 3. Looks for `origin` remote first
/// 4. If no `origin` and exactly one remote → uses it automatically (AC #10)
/// 5. If no `origin` and multiple remotes → returns `MultipleRemotes` for user selection
/// 6. Parses the remote URL and detects default branch from HEAD
///
/// Returns `NotAvailable` silently on any failure (no `.git`, no remotes, bad URL).
pub fn detect_git_remote(project_path: &Path) -> GitDetectionResult {
    // Verify this is a git repo
    if !is_git_repo(project_path) {
        return GitDetectionResult::NotAvailable;
    }

    detect_from_repo(project_path, None)
}

/// Detects git remote information using a specific remote name.
///
/// Used when the caller already knows which remote to inspect (e.g., after
/// user selects from a multi-remote prompt).
pub fn detect_git_remote_with_name(project_path: &Path, remote_name: &str) -> GitDetectionResult {
    // Verify this is a git repo
    if !is_git_repo(project_path) {
        return GitDetectionResult::NotAvailable;
    }

    detect_from_repo(project_path, Some(remote_name))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check if the given path is inside a git repository.
fn is_git_repo(project_path: &Path) -> bool {
    match std::process::Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["rev-parse", "--git-dir"])
        .output()
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Core detection logic operating on a verified git repository path.
///
/// If `preferred_remote` is `Some`, uses that remote directly. Otherwise,
/// applies the origin → single-remote → multiple-remotes discovery logic.
fn detect_from_repo(project_path: &Path, preferred_remote: Option<&str>) -> GitDetectionResult {
    if let Some(name) = preferred_remote {
        return detect_single_remote(project_path, name);
    }

    // List all remote names via `git remote`
    let remote_output = match std::process::Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("remote")
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return GitDetectionResult::NotAvailable,
    };

    let stdout = String::from_utf8_lossy(&remote_output.stdout);
    let remote_names: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();

    if remote_names.is_empty() {
        return GitDetectionResult::NotAvailable;
    }

    // Prefer origin
    if remote_names.iter().any(|n| n == "origin") {
        return detect_single_remote(project_path, "origin");
    }

    // Exactly one remote (not origin) → use it automatically (AC #10)
    if remote_names.len() == 1 {
        return detect_single_remote(project_path, &remote_names[0]);
    }

    // Multiple remotes, no origin → user must choose
    GitDetectionResult::MultipleRemotes(remote_names)
}

/// Detects remote info from a single named remote.
fn detect_single_remote(project_path: &Path, remote_name: &str) -> GitDetectionResult {
    // Get remote URL via `git remote get-url <name>`
    let url_output = match std::process::Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["remote", "get-url", remote_name])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return GitDetectionResult::NotAvailable,
    };

    let url = String::from_utf8_lossy(&url_output.stdout)
        .trim()
        .to_string();
    if url.is_empty() {
        return GitDetectionResult::NotAvailable;
    }

    let (host, owner, repo_name) = match parse_git_remote_url(&url) {
        Some(parsed) => parsed,
        None => return GitDetectionResult::NotAvailable,
    };

    let provider = map_host_to_provider(&host);
    let default_branch = detect_default_branch(project_path);

    GitDetectionResult::Detected(GitRemoteInfo {
        provider,
        host,
        owner,
        repo_name,
        default_branch,
        remote_name: remote_name.to_string(),
    })
}

/// Detects the default branch name from HEAD.
///
/// Uses `git rev-parse --abbrev-ref HEAD` to get the current branch.
/// Falls back to `"main"` if HEAD is detached, unborn, or unreadable.
fn detect_default_branch(project_path: &Path) -> String {
    match std::process::Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if branch.is_empty() || branch == "HEAD" {
                "main".to_string()
            } else {
                branch
            }
        }
        _ => "main".to_string(),
    }
}

/// Parses `ssh://git@<host>[:<port>]/<owner>/<repo>.git` format.
fn parse_ssh_scheme_url(rest: &str) -> Option<(String, String, String)> {
    // rest = "git@<host>[:<port>]/<owner>/<repo>.git"
    // Strip user@ prefix
    let after_at = if let Some(at_pos) = rest.find('@') {
        &rest[at_pos + 1..]
    } else {
        rest
    };

    // Find first '/' to separate host[:port] from path
    let slash_pos = after_at.find('/')?;
    let host_port = &after_at[..slash_pos];
    let path = &after_at[slash_pos + 1..];

    // Strip port if present (host:port → host)
    let host = if let Some(colon_pos) = host_port.find(':') {
        &host_port[..colon_pos]
    } else {
        host_port
    };

    parse_owner_repo_from_path(host, path)
}

/// Parses `https://<host>/<owner>/<repo>.git` format.
fn parse_https_url(url: &str) -> Option<(String, String, String)> {
    // Strip scheme
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;

    // Find first '/' to separate host from path
    let slash_pos = rest.find('/')?;
    let host = &rest[..slash_pos];
    let path = &rest[slash_pos + 1..];

    // Strip port from host if present
    let host = if let Some(colon_pos) = host.find(':') {
        &host[..colon_pos]
    } else {
        host
    };

    parse_owner_repo_from_path(host, path)
}

/// Extracts `(host, owner, repo_name)` from a host string and a `owner/repo[.git]` path.
///
/// Strips `.git` suffix and trailing slashes from repo name.
fn parse_owner_repo_from_path(host: &str, path: &str) -> Option<(String, String, String)> {
    let path = path.trim_end_matches('/');

    if path.is_empty() || host.is_empty() {
        return None;
    }

    // Split path into segments
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if segments.len() < 2 {
        return None;
    }

    let owner = segments[0];
    let repo_raw = segments[1];

    // Strip .git suffix
    let repo_name = repo_raw
        .strip_suffix(".git")
        .unwrap_or(repo_raw)
        .trim_end_matches('/');

    if owner.is_empty() || repo_name.is_empty() {
        return None;
    }

    Some((host.to_string(), owner.to_string(), repo_name.to_string()))
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- CLI-based test fixtures ---

    /// Initialize a git repo with an initial commit in a tempdir.
    fn init_test_repo(dir: &Path) {
        let output = std::process::Command::new("git")
            .args(["init", dir.to_str().unwrap()])
            .output()
            .expect("git init");
        assert!(output.status.success(), "git init failed");

        // Set identity for commits (required in CI/test environments)
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .expect("git config email");
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["config", "user.name", "Test"])
            .output()
            .expect("git config name");
    }

    /// Add a remote to a test repo.
    fn add_remote(dir: &Path, name: &str, url: &str) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["remote", "add", name, url])
            .output()
            .expect("git remote add");
        assert!(output.status.success(), "git remote add failed");
    }

    /// Create an initial empty commit so HEAD exists.
    fn create_initial_commit(dir: &Path) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["commit", "--allow-empty", "-m", "init"])
            .output()
            .expect("git commit");
        assert!(output.status.success(), "git commit failed");
    }

    // --- parse_git_remote_url tests ---

    #[test]
    fn test_parse_ssh_scp_format_github() {
        let result = parse_git_remote_url("git@github.com:owner/repo.git");
        assert_eq!(
            result,
            Some((
                "github.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_https_format_github() {
        let result = parse_git_remote_url("https://github.com/owner/repo.git");
        assert_eq!(
            result,
            Some((
                "github.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_ssh_scheme_format() {
        let result = parse_git_remote_url("ssh://git@github.com/owner/repo.git");
        assert_eq!(
            result,
            Some((
                "github.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_gitlab_url() {
        let result = parse_git_remote_url("git@gitlab.com:owner/repo.git");
        assert_eq!(
            result,
            Some((
                "gitlab.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_self_hosted_url() {
        let result = parse_git_remote_url("git@git.company.com:owner/repo.git");
        assert_eq!(
            result,
            Some((
                "git.company.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_strips_git_suffix() {
        let result = parse_git_remote_url("https://github.com/owner/my-project.git");
        assert_eq!(
            result,
            Some((
                "github.com".to_string(),
                "owner".to_string(),
                "my-project".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_works_without_git_suffix() {
        let result = parse_git_remote_url("https://github.com/owner/repo");
        assert_eq!(
            result,
            Some((
                "github.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_with_trailing_slash() {
        let result = parse_git_remote_url("https://github.com/owner/repo/");
        assert_eq!(
            result,
            Some((
                "github.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_ssh_with_port() {
        let result = parse_git_remote_url("ssh://git@github.com:22/owner/repo.git");
        assert_eq!(
            result,
            Some((
                "github.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_returns_none_for_malformed() {
        assert_eq!(parse_git_remote_url(""), None);
        assert_eq!(parse_git_remote_url("not-a-url"), None);
        assert_eq!(parse_git_remote_url("https://"), None);
        assert_eq!(parse_git_remote_url("https://github.com/"), None);
        assert_eq!(parse_git_remote_url("https://github.com/owner-only"), None);
    }

    // --- map_host_to_provider tests ---

    #[test]
    fn test_map_github_provider() {
        assert_eq!(
            map_host_to_provider("github.com"),
            Some("github".to_string())
        );
    }

    #[test]
    fn test_map_gitlab_provider() {
        assert_eq!(
            map_host_to_provider("gitlab.com"),
            Some("gitlab".to_string())
        );
    }

    #[test]
    fn test_map_unknown_provider() {
        assert_eq!(map_host_to_provider("git.company.com"), None);
    }

    #[test]
    fn test_map_provider_case_insensitive() {
        assert_eq!(
            map_host_to_provider("GitHub.com"),
            Some("github".to_string())
        );
        assert_eq!(
            map_host_to_provider("GITLAB.COM"),
            Some("gitlab".to_string())
        );
    }

    // --- detect_git_remote tests (CLI-based fixtures) ---

    #[test]
    fn test_detect_returns_not_available_for_non_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = detect_git_remote(tmp.path());
        assert_eq!(result, GitDetectionResult::NotAvailable);
    }

    #[test]
    fn test_detect_with_configured_origin_remote() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        // Configure origin remote
        add_remote(
            tmp.path(),
            "origin",
            "git@github.com:test-owner/test-repo.git",
        );

        // Create an initial commit so HEAD exists
        create_initial_commit(tmp.path());

        let result = detect_git_remote(tmp.path());
        match result {
            GitDetectionResult::Detected(info) => {
                assert_eq!(info.provider, Some("github".to_string()));
                assert_eq!(info.host, "github.com");
                assert_eq!(info.owner, "test-owner");
                assert_eq!(info.repo_name, "test-repo");
                assert_eq!(info.remote_name, "origin");
            }
            other => panic!("Expected Detected, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_single_non_origin_remote_auto_selects() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        // Configure a single remote named "upstream" (not origin)
        add_remote(
            tmp.path(),
            "upstream",
            "git@github.com:upstream-owner/upstream-repo.git",
        );

        let result = detect_git_remote(tmp.path());
        match result {
            GitDetectionResult::Detected(info) => {
                assert_eq!(info.owner, "upstream-owner");
                assert_eq!(info.repo_name, "upstream-repo");
                assert_eq!(info.remote_name, "upstream");
            }
            other => panic!("Expected Detected, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_multiple_remotes_no_origin_returns_list() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        add_remote(tmp.path(), "upstream", "git@github.com:up/repo.git");
        add_remote(tmp.path(), "fork", "git@github.com:fork/repo.git");

        let result = detect_git_remote(tmp.path());
        match result {
            GitDetectionResult::MultipleRemotes(names) => {
                assert_eq!(names.len(), 2);
                assert!(names.contains(&"upstream".to_string()));
                assert!(names.contains(&"fork".to_string()));
            }
            other => panic!("Expected MultipleRemotes, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_returns_not_available_for_malformed_url() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        add_remote(tmp.path(), "origin", "not-a-valid-url");

        let result = detect_git_remote(tmp.path());
        assert_eq!(result, GitDetectionResult::NotAvailable);
    }

    #[test]
    fn test_detect_no_remotes_returns_not_available() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let result = detect_git_remote(tmp.path());
        assert_eq!(result, GitDetectionResult::NotAvailable);
    }

    #[test]
    fn test_detect_default_branch_from_head() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        add_remote(tmp.path(), "origin", "https://github.com/owner/repo.git");

        // Create initial commit on default branch
        create_initial_commit(tmp.path());

        let result = detect_git_remote(tmp.path());
        match result {
            GitDetectionResult::Detected(info) => {
                // git init creates a branch depending on config — the branch name should be non-empty and valid
                assert!(!info.default_branch.is_empty());
            }
            other => panic!("Expected Detected, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_default_branch_fallback_when_no_head() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        // No commits → HEAD is unborn
        add_remote(tmp.path(), "origin", "https://github.com/owner/repo.git");

        let result = detect_git_remote(tmp.path());
        match result {
            GitDetectionResult::Detected(info) => {
                assert_eq!(info.default_branch, "main");
            }
            other => panic!("Expected Detected, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_with_specific_remote_name() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        add_remote(
            tmp.path(),
            "origin",
            "git@github.com:origin-owner/origin-repo.git",
        );
        add_remote(
            tmp.path(),
            "upstream",
            "git@gitlab.com:upstream-owner/upstream-repo.git",
        );

        let result = detect_git_remote_with_name(tmp.path(), "upstream");
        match result {
            GitDetectionResult::Detected(info) => {
                assert_eq!(info.provider, Some("gitlab".to_string()));
                assert_eq!(info.owner, "upstream-owner");
                assert_eq!(info.repo_name, "upstream-repo");
                assert_eq!(info.remote_name, "upstream");
            }
            other => panic!("Expected Detected, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_https_gitlab() {
        let result = parse_git_remote_url("https://gitlab.com/my-group/my-project.git");
        assert_eq!(
            result,
            Some((
                "gitlab.com".to_string(),
                "my-group".to_string(),
                "my-project".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_ssh_scp_without_git_suffix() {
        let result = parse_git_remote_url("git@github.com:owner/repo");
        assert_eq!(
            result,
            Some((
                "github.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_http_url() {
        let result = parse_git_remote_url("http://github.com/owner/repo.git");
        assert_eq!(
            result,
            Some((
                "github.com".to_string(),
                "owner".to_string(),
                "repo".to_string()
            ))
        );
    }
}
