//! Session cleanup: partial work preservation and sprint-status updates.
//!
//! This module contains key functions used during session cleanup:
//!
//! - [`preserve_partial_work`] — best-effort git operations to commit WIP changes
//!   and build a summary. Returns `String` (never errors) so the escalation flow
//!   is never blocked by a preservation failure.
//!
//! - [`mark_story_needs_clarification`] — updates a story's status in
//!   `sprint-status.yaml` using string-based replacement to preserve comments.
//!
//! - [`unstick_orphan_stories`] — detects `in-progress` stories with no WAL file
//!   and no other daemon running, resets them to `ready-for-dev`.
//!
//! All functions are designed for reuse in SIGTERM/SIGINT graceful shutdown (FR34).

use std::path::Path;

use crate::cli::state::DaemonState;
use crate::session::SessionError;

/// Preserves partial work on the story branch during escalation.
///
/// This function is **best-effort** — it returns a `String` summary, NEVER an error.
/// Every git CLI operation is individually guarded: failures are logged via `tracing::error!()`
/// and a fallback summary is returned. The escalation flow must NEVER be blocked by a
/// preservation failure.
///
/// # Operations (all best-effort)
/// 1. Check for uncommitted changes via `git status --porcelain`
/// 2. If dirty: stage all changes via `git add .` and commit with a WIP message
/// 3. Build a summary string with branch name, commit status, and file list
///
/// # Arguments
/// - `repo_path` — path to the git repository root
/// - `story_key` — the story being escalated (for logging)
/// - `question` — the escalation question (included in WIP commit message)
///
/// # Returns
/// A human-readable summary string describing the preserved partial work.
pub async fn preserve_partial_work(repo_path: &Path, story_key: &str, question: &str) -> String {
    // Check for dirty state via `git status --porcelain`
    let status_output = match tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["status", "--porcelain"])
        .output()
        .await
    {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!(
                    action = "preserve_partial_work",
                    story_id = %story_key,
                    error = %stderr,
                    "git status --porcelain failed — skipping preservation"
                );
                return format!("Preservation failed — git status failed: {stderr}");
            }
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        Err(e) => {
            tracing::error!(
                action = "preserve_partial_work",
                story_id = %story_key,
                error = %e,
                "Failed to execute git status — skipping preservation"
            );
            return format!("Preservation failed — could not execute git: {e}");
        }
    };

    // Parse changed files from porcelain output
    let changed_files: Vec<String> = status_output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            // Porcelain format: "XY path" — skip the 2-char status + space
            if line.len() > 3 {
                line[3..].trim().to_string()
            } else {
                line.trim().to_string()
            }
        })
        .collect();

    let has_changes = !changed_files.is_empty();

    if has_changes {
        // Stage all changes: git add .
        let add_result = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["add", "."])
            .output()
            .await;

        match &add_result {
            Err(e) => {
                tracing::error!(
                    action = "preserve_partial_work",
                    story_id = %story_key,
                    error = %e,
                    "Failed to stage changes — changes remain unstaged"
                );
            }
            Ok(output) if !output.status.success() => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!(
                    action = "preserve_partial_work",
                    story_id = %story_key,
                    error = %stderr,
                    "git add failed — changes remain unstaged"
                );
            }
            _ => {}
        }

        // Commit with WIP message — include story_key so the commit is traceable
        let message = format!(
            "chore: WIP [{story_key}] — escalated for human clarification\n\nQuestion: {question}"
        );
        let commit_result = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["commit", "-m", &message])
            .output()
            .await;

        match &commit_result {
            Err(e) => {
                tracing::error!(
                    action = "preserve_partial_work",
                    story_id = %story_key,
                    error = %e,
                    "Failed to create WIP commit — changes remain staged"
                );
            }
            Ok(output) if !output.status.success() => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!(
                    action = "preserve_partial_work",
                    story_id = %story_key,
                    error = %stderr,
                    "git commit failed — changes remain staged"
                );
            }
            _ => {}
        }
    }

    // Build summary — get branch name (individually guarded)
    let branch = match tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["branch", "--show-current"])
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let b = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if b.is_empty() {
                "unknown".to_string()
            } else {
                b
            }
        }
        _ => "unknown".to_string(),
    };

    let summary = format!(
        "Branch: {}\nWIP commit: {}\nFiles touched: {}",
        branch,
        if has_changes {
            "yes"
        } else {
            "no (clean tree)"
        },
        if changed_files.is_empty() {
            "none".to_string()
        } else {
            changed_files.join(", ")
        }
    );

    tracing::info!(
        action = "preserve_partial_work",
        story_id = %story_key,
        summary = %summary,
        "Partial work committed and preserved on branch"
    );

    summary
}

/// Updates a story's status to `needs-clarification` in sprint-status.yaml.
///
/// Uses string-based find-and-replace to preserve all comments and structure
/// (STATUS DEFINITIONS header, formatting). Does NOT use `serde_yml` for
/// serialization — only for reading if needed.
///
/// # Arguments
/// - `sprint_status_path` — path to `sprint-status.yaml`
/// - `story_key` — the story key to update (e.g., "3-3-human-escalation")
/// - `new_status` — the target status value (e.g., "done", "needs-clarification")
///
/// # Errors
/// Returns [`SessionError::StateFileFailed`] if:
/// - The file cannot be read
/// - The story key is not found in the file
/// - The file cannot be written back
pub async fn update_story_status(
    sprint_status_path: &Path,
    story_key: &str,
    new_status: &str,
) -> Result<(), SessionError> {
    let content = tokio::fs::read_to_string(sprint_status_path)
        .await
        .map_err(|e| SessionError::StateFileFailed {
            reason: format!("Failed to read sprint-status: {e}"),
        })?;

    // Pattern: "  story-key: current-status" → "  story-key: <new_status>"
    // Preserves trailing comments (e.g., "# depends-on: 1-1")
    let pattern = format!(r"(?m)(^\s*{}\s*:\s*)\S+", regex::escape(story_key));
    let re = regex::Regex::new(&pattern).map_err(|e| SessionError::StateFileFailed {
        reason: format!("Invalid regex for story key: {e}"),
    })?;

    if !re.is_match(&content) {
        return Err(SessionError::StateFileFailed {
            reason: format!("Story key '{story_key}' not found in sprint-status.yaml"),
        });
    }

    let replacement = format!("${{1}}{new_status}");
    let updated = re.replace(&content, replacement.as_str()).to_string();

    tokio::fs::write(sprint_status_path, &updated)
        .await
        .map_err(|e| SessionError::StateFileFailed {
            reason: format!("Failed to write sprint-status: {e}"),
        })?;

    tracing::info!(
        action = "story_status_update",
        story_id = %story_key,
        new_status = %new_status,
        "Story status updated in sprint-status.yaml"
    );

    Ok(())
}

/// Convenience wrapper: marks a story as `needs-clarification` in sprint-status.yaml.
///
/// See [`update_story_status`] for full documentation.
pub async fn mark_story_needs_clarification(
    sprint_status_path: &Path,
    story_key: &str,
) -> Result<(), SessionError> {
    update_story_status(sprint_status_path, story_key, "needs-clarification").await
}

/// Extract the short `"{epic}-{story}"` prefix from a full story key.
///
/// For `"7-1-integration-test-infrastructure-fixtures"` returns `"7-1"`.
/// For a key that is already short (e.g., `"7-1"`) returns it unchanged.
fn extract_short_key(full_key: &str) -> String {
    let mut parts = full_key.splitn(3, '-');
    match (parts.next(), parts.next()) {
        (Some(epic), Some(story))
            if epic.chars().all(|c| c.is_ascii_digit())
                && story.chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("{epic}-{story}")
        }
        _ => full_key.to_string(),
    }
}

/// Transitions `blocked` stories that depend on `completed_key` to `ready-for-dev`.
///
/// Scans sprint-status.yaml for lines matching the pattern:
/// ```text
///   some-story-key: blocked  # depends-on: <completed_key>
/// ```
/// and replaces `blocked` with `ready-for-dev`. Only exact single-dependency
/// matches are unblocked (multi-dep parsing is left to the watcher's resolver).
///
/// Returns the list of story keys that were unblocked (may be empty).
///
/// Best-effort: errors reading/writing the file are returned, but a file with
/// no matching dependents is not an error.
pub async fn unblock_dependents(
    sprint_status_path: &Path,
    completed_key: &str,
) -> Result<Vec<String>, SessionError> {
    let content = tokio::fs::read_to_string(sprint_status_path)
        .await
        .map_err(|e| SessionError::StateFileFailed {
            reason: format!("Failed to read sprint-status: {e}"),
        })?;

    // Match lines like: "  story-key: blocked  # depends-on: 7-1"
    // The depends-on comment may use a short key (e.g., "7-1") or the full key
    // (e.g., "7-1-integration-test-infrastructure-fixtures"). We extract the
    // short prefix "{epic}-{story}" from completed_key and match on either form.
    // Captures: (1) leading whitespace + story key + colon + whitespace, (2) story key alone
    let short_key = extract_short_key(completed_key);
    let escaped_full = regex::escape(completed_key);
    let escaped_short = regex::escape(&short_key);
    let dep_pattern = if escaped_short == escaped_full {
        escaped_full
    } else {
        format!("(?:{escaped_full}|{escaped_short})")
    };
    let pattern =
        format!(r"(?m)^(\s*(\S+)\s*:\s*)blocked(\s*#\s*depends-on:\s*{dep_pattern}(?:\s|$))");
    let re = regex::Regex::new(&pattern).map_err(|e| SessionError::StateFileFailed {
        reason: format!("Invalid regex for dependency pattern: {e}"),
    })?;

    // Collect unblocked keys before mutating
    let unblocked: Vec<String> = re
        .captures_iter(&content)
        .filter_map(|cap| cap.get(2).map(|m| m.as_str().to_string()))
        .collect();

    if unblocked.is_empty() {
        return Ok(unblocked);
    }

    let updated = re
        .replace_all(&content, "${1}ready-for-dev${3}")
        .to_string();

    tokio::fs::write(sprint_status_path, &updated)
        .await
        .map_err(|e| SessionError::StateFileFailed {
            reason: format!("Failed to write sprint-status: {e}"),
        })?;

    for key in &unblocked {
        tracing::info!(
            action = "story_unblocked",
            story_key = %key,
            dependency = %completed_key,
            "Dependent story unblocked: blocked → ready-for-dev"
        );
    }

    Ok(unblocked)
}

/// Detect `in-progress` stories that have no WAL file and no running daemon,
/// then reset them to `ready-for-dev` so the watcher can pick them up.
///
/// This handles the "stuck story" scenario: a daemon was killed (or a bug
/// deleted the WAL) while a story was `in-progress`. Without a WAL, crash
/// recovery cannot trigger, and the watcher ignores non-`ready-for-dev`
/// stories — leaving the story permanently stuck.
///
/// **Safety checks before resetting:**
/// 1. No WAL file exists at `wal_path` (otherwise crash recovery handles it)
/// 2. No other daemon instance is alive (`bmad-bot.state.json` PID check)
///
/// Returns the list of story keys that were unstuck (may be empty).
/// Best-effort: errors are logged but never block daemon startup.
pub async fn unstick_orphan_stories(
    sprint_status_path: &Path,
    wal_path: &Path,
    state_file_path: &Path,
) -> Vec<String> {
    // Guard 1 — if a WAL exists, crash recovery will handle it
    if wal_path.exists() {
        return vec![];
    }

    // Guard 2 — if another daemon is still running, the story may be legitimately in-progress.
    // Compare against our own PID: the current daemon writes the state file before calling
    // this function, so we must ignore our own PID to avoid blocking ourselves.
    let my_pid = std::process::id();
    if let Ok(Some(state)) = DaemonState::read(state_file_path)
        && state.status == "running"
        && state.pid != my_pid
        && DaemonState::is_process_alive(state.pid)
    {
        tracing::debug!(
            action = "unstick_skip_daemon_alive",
            pid = state.pid,
            "Another daemon is running — skipping orphan detection"
        );
        return vec![];
    }

    // Read sprint-status.yaml and find in-progress stories
    let content = match tokio::fs::read_to_string(sprint_status_path).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                action = "unstick_read_failed",
                error = %e,
                "Failed to read sprint-status.yaml for orphan detection"
            );
            return vec![];
        }
    };

    // Find all story keys with status "in-progress" (skip epic lines).
    // Pattern: "  <story-key>: in-progress" where story-key starts with digits.
    let pattern = r"(?m)^\s*(\d+\S+)\s*:\s*in-progress";
    let re = match regex::Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let orphans: Vec<String> = re
        .captures_iter(&content)
        .filter_map(|cap| {
            let key = cap.get(1)?.as_str().to_string();
            // Skip epic-level entries (e.g., "epic-1")
            if key.starts_with("epic-") {
                return None;
            }
            Some(key)
        })
        .collect();

    if orphans.is_empty() {
        return vec![];
    }

    tracing::warn!(
        action = "unstick_orphans_detected",
        count = orphans.len(),
        stories = %orphans.join(", "),
        "Detected in-progress stories with no WAL and no running daemon — resetting to ready-for-dev"
    );

    let mut unstuck = Vec::new();
    for key in &orphans {
        match update_story_status(sprint_status_path, key, "ready-for-dev").await {
            Ok(()) => {
                tracing::info!(
                    action = "unstick_story_reset",
                    story_key = %key,
                    "Orphan story reset: in-progress → ready-for-dev"
                );
                unstuck.push(key.clone());
            }
            Err(e) => {
                tracing::error!(
                    action = "unstick_story_failed",
                    story_key = %key,
                    error = %e,
                    "Failed to reset orphan story"
                );
            }
        }
    }

    unstuck
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // mark_story_needs_clarification tests
    // -----------------------------------------------------------------------

    fn sample_sprint_status() -> String {
        r#"# STATUS DEFINITIONS:
# ==================
# Story Status:
#   - backlog: Story only exists in epic file
#   - ready-for-dev: Story file created in stories folder
#   - in-progress: Developer actively working on implementation
#   - review: Ready for code review
#   - done: Story completed

generated: 2026-02-07
project: test-project

development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli-framework: review
  epic-2: in-progress
  2-1-polling: in-progress
  2-2-deps: ready-for-dev
  epic-3: in-progress
  3-3-human-escalation: in-progress
  3-4-decision-logging: ready-for-dev
"#
        .to_string()
    }

    #[tokio::test]
    async fn test_mark_story_needs_clarification_happy_path() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status())
            .await
            .expect("write");

        mark_story_needs_clarification(&path, "3-3-human-escalation")
            .await
            .expect("should succeed");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("3-3-human-escalation: needs-clarification"));
    }

    // -----------------------------------------------------------------------
    // update_story_status tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_update_story_status_to_done() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status())
            .await
            .expect("write");

        update_story_status(&path, "1-2-cli-framework", "done")
            .await
            .expect("should succeed");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("1-2-cli-framework: done"));
    }

    #[tokio::test]
    async fn test_update_story_status_preserves_other_statuses() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status())
            .await
            .expect("write");

        update_story_status(&path, "2-1-polling", "done")
            .await
            .expect("should succeed");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("2-1-polling: done"));
        assert!(content.contains("1-1-scaffolding: done"));
        assert!(content.contains("1-2-cli-framework: review"));
        assert!(content.contains("2-2-deps: ready-for-dev"));
        assert!(content.contains("3-3-human-escalation: in-progress"));
    }

    #[tokio::test]
    async fn test_update_story_status_preserves_comments() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status())
            .await
            .expect("write");

        update_story_status(&path, "3-3-human-escalation", "done")
            .await
            .expect("should succeed");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("# STATUS DEFINITIONS:"));
        assert!(content.contains("# Story Status:"));
    }

    #[tokio::test]
    async fn test_update_story_status_missing_key_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status())
            .await
            .expect("write");

        let result = update_story_status(&path, "99-99-nonexistent", "done").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let display = format!("{err}");
        assert!(display.contains("99-99-nonexistent"));
    }

    #[tokio::test]
    async fn test_update_story_status_missing_file_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nonexistent.yaml");

        let result = update_story_status(&path, "3-3-human-escalation", "done").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let display = format!("{err}");
        assert!(display.contains("Failed to read sprint-status"));
    }

    #[tokio::test]
    async fn test_update_story_status_preserves_trailing_comments() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        let content = r#"development_status:
  epic-1: in-progress
  1-1-scaffolding: review # depends-on: nothing
  1-2-cli: blocked # depends-on: 1-1
"#;
        tokio::fs::write(&path, content).await.expect("write");

        update_story_status(&path, "1-1-scaffolding", "done")
            .await
            .expect("should succeed");

        let updated = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(updated.contains("1-1-scaffolding: done # depends-on: nothing"));
        assert!(updated.contains("1-2-cli: blocked # depends-on: 1-1"));
    }

    // -----------------------------------------------------------------------
    // unblock_dependents tests
    // -----------------------------------------------------------------------

    fn sample_sprint_status_with_deps() -> String {
        r#"# STATUS DEFINITIONS:
# ==================
development_status:
  epic-7: in-progress
  7-1-infra: done
  7-2-config-tests: blocked # depends-on: 7-1-infra
  7-3-watcher-tests: blocked # depends-on: 7-1-infra
  7-4-pipeline-tests: blocked # depends-on: 7-1-infra
  7-5-wal-tests: blocked # depends-on: 7-1-infra
  epic-8: in-progress
  8-1-read-file: done
  8-2-edit-file: blocked # depends-on: 8-1-read-file
  8-3-grep: review
"#
        .to_string()
    }

    #[tokio::test]
    async fn test_unblock_dependents_happy_path() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status_with_deps())
            .await
            .expect("write");

        let unblocked = unblock_dependents(&path, "7-1-infra")
            .await
            .expect("should succeed");

        assert_eq!(unblocked.len(), 4);
        assert!(unblocked.contains(&"7-2-config-tests".to_string()));
        assert!(unblocked.contains(&"7-3-watcher-tests".to_string()));
        assert!(unblocked.contains(&"7-4-pipeline-tests".to_string()));
        assert!(unblocked.contains(&"7-5-wal-tests".to_string()));

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("7-2-config-tests: ready-for-dev # depends-on: 7-1-infra"));
        assert!(content.contains("7-3-watcher-tests: ready-for-dev # depends-on: 7-1-infra"));
        assert!(content.contains("7-4-pipeline-tests: ready-for-dev # depends-on: 7-1-infra"));
        assert!(content.contains("7-5-wal-tests: ready-for-dev # depends-on: 7-1-infra"));
    }

    #[tokio::test]
    async fn test_unblock_dependents_only_blocked_stories() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status_with_deps())
            .await
            .expect("write");

        let unblocked = unblock_dependents(&path, "8-1-read-file")
            .await
            .expect("should succeed");

        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0], "8-2-edit-file");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("8-2-edit-file: ready-for-dev # depends-on: 8-1-read-file"));
        // Non-blocked story with no depends-on for this key should be untouched
        assert!(content.contains("8-3-grep: review"));
    }

    #[tokio::test]
    async fn test_unblock_dependents_no_matches_returns_empty() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status_with_deps())
            .await
            .expect("write");

        let unblocked = unblock_dependents(&path, "99-99-nonexistent")
            .await
            .expect("should succeed even with no matches");

        assert!(unblocked.is_empty());
    }

    #[tokio::test]
    async fn test_unblock_dependents_preserves_other_statuses() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status_with_deps())
            .await
            .expect("write");

        unblock_dependents(&path, "7-1-infra")
            .await
            .expect("should succeed");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("7-1-infra: done"));
        assert!(content.contains("epic-7: in-progress"));
        assert!(content.contains("8-1-read-file: done"));
        assert!(content.contains("8-2-edit-file: blocked # depends-on: 8-1-read-file"));
        assert!(content.contains("8-3-grep: review"));
    }

    #[tokio::test]
    async fn test_unblock_dependents_preserves_comments() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status_with_deps())
            .await
            .expect("write");

        unblock_dependents(&path, "7-1-infra")
            .await
            .expect("should succeed");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("# STATUS DEFINITIONS:"));
    }

    #[tokio::test]
    async fn test_unblock_dependents_no_partial_key_match() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        let content = r#"development_status:
  7-1-infra: done
  7-10-extra: blocked # depends-on: 7-1-infra-extended
  7-2-config: blocked # depends-on: 7-1-infra
"#;
        tokio::fs::write(&path, content).await.expect("write");

        let unblocked = unblock_dependents(&path, "7-1-infra")
            .await
            .expect("should succeed");

        // Only 7-2-config matches exactly, not 7-10-extra (different dep key)
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0], "7-2-config");

        let updated = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(updated.contains("7-10-extra: blocked # depends-on: 7-1-infra-extended"));
        assert!(updated.contains("7-2-config: ready-for-dev # depends-on: 7-1-infra"));
    }

    #[tokio::test]
    async fn test_unblock_dependents_missing_file_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nonexistent.yaml");

        let result = unblock_dependents(&path, "7-1-infra").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mark_story_needs_clarification_preserves_other_statuses() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status())
            .await
            .expect("write");

        mark_story_needs_clarification(&path, "3-3-human-escalation")
            .await
            .expect("should succeed");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("1-1-scaffolding: done"));
        assert!(content.contains("1-2-cli-framework: review"));
        assert!(content.contains("2-1-polling: in-progress"));
        assert!(content.contains("2-2-deps: ready-for-dev"));
        assert!(content.contains("3-4-decision-logging: ready-for-dev"));
    }

    #[tokio::test]
    async fn test_mark_story_needs_clarification_preserves_comments() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status())
            .await
            .expect("write");

        mark_story_needs_clarification(&path, "3-3-human-escalation")
            .await
            .expect("should succeed");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("# STATUS DEFINITIONS:"));
        assert!(content.contains("# Story Status:"));
        assert!(content.contains("#   - backlog: Story only exists in epic file"));
    }

    #[tokio::test]
    async fn test_mark_story_needs_clarification_missing_key_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status())
            .await
            .expect("write");

        let result = mark_story_needs_clarification(&path, "99-99-nonexistent").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let display = format!("{err}");
        assert!(display.contains("99-99-nonexistent"));
    }

    #[tokio::test]
    async fn test_mark_story_needs_clarification_missing_file_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nonexistent.yaml");

        let result = mark_story_needs_clarification(&path, "3-3-human-escalation").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let display = format!("{err}");
        assert!(display.contains("Failed to read sprint-status"));
    }

    // -----------------------------------------------------------------------
    // preserve_partial_work tests (CLI-based fixtures)
    // -----------------------------------------------------------------------

    /// Helper: create a git repo with an initial commit in a tempdir (CLI-based).
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
            .args(["config", "user.name", "test"])
            .output()
            .expect("git config name");

        // Create initial empty commit so HEAD exists
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["commit", "--allow-empty", "-m", "Initial commit"])
            .output()
            .expect("git commit");
        assert!(output.status.success(), "git commit failed");
    }

    #[tokio::test]
    async fn test_preserve_partial_work_dirty_tree_creates_wip_commit() {
        let dir = TempDir::new().expect("tempdir");
        let repo_path = dir.path();
        init_test_repo(repo_path);

        // Create a dirty file
        std::fs::write(repo_path.join("new_file.rs"), "fn main() {}").expect("write file");

        let summary = preserve_partial_work(repo_path, "3-3-test", "What DB schema?").await;

        assert!(summary.contains("WIP commit: yes"), "summary: {summary}");
        assert!(
            summary.contains("new_file.rs"),
            "summary should list touched files: {summary}"
        );
    }

    #[tokio::test]
    async fn test_preserve_partial_work_clean_tree_no_commit() {
        let dir = TempDir::new().expect("tempdir");
        let repo_path = dir.path();
        init_test_repo(repo_path);

        let summary = preserve_partial_work(repo_path, "3-3-test", "What DB?").await;

        assert!(
            summary.contains("WIP commit: no (clean tree)"),
            "summary: {summary}"
        );
        assert!(
            summary.contains("Files touched: none"),
            "summary: {summary}"
        );
    }

    #[tokio::test]
    async fn test_preserve_partial_work_summary_includes_branch_name() {
        let dir = TempDir::new().expect("tempdir");
        let repo_path = dir.path();
        init_test_repo(repo_path);

        let summary = preserve_partial_work(repo_path, "3-3-test", "q").await;

        // Default branch from git init — could be "main", "master", or other
        assert!(
            summary.starts_with("Branch: "),
            "summary should start with branch name: {summary}"
        );
    }

    #[tokio::test]
    async fn test_preserve_partial_work_invalid_repo_returns_fallback() {
        let dir = TempDir::new().expect("tempdir");
        // Don't init — not a git repo
        let summary = preserve_partial_work(dir.path(), "3-3-test", "q").await;

        assert!(
            summary.contains("Preservation failed"),
            "should return fallback: {summary}"
        );
    }

    #[tokio::test]
    async fn test_preserve_partial_work_multiple_files() {
        let dir = TempDir::new().expect("tempdir");
        let repo_path = dir.path();
        init_test_repo(repo_path);

        // Create multiple dirty files
        std::fs::write(repo_path.join("a.rs"), "a").expect("write");
        std::fs::write(repo_path.join("b.rs"), "b").expect("write");

        let summary = preserve_partial_work(repo_path, "key", "q").await;

        assert!(summary.contains("WIP commit: yes"), "summary: {summary}");
        assert!(summary.contains("a.rs"), "summary: {summary}");
        assert!(summary.contains("b.rs"), "summary: {summary}");
    }

    // -----------------------------------------------------------------------
    // Integration: full escalation flow
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_full_escalation_flow_preserve_then_status_then_report() {
        use crate::session::escalation::{EscalationInfo, EscalationReport};

        // --- Arrange ---
        let dir = TempDir::new().expect("tempdir");

        // 1. Git repo with dirty working tree
        let repo_path = dir.path().join("repo");
        std::fs::create_dir_all(&repo_path).expect("mkdir");
        init_test_repo(&repo_path);
        std::fs::write(repo_path.join("wip.rs"), "fn wip() {}").expect("write dirty file");

        // 2. Sprint-status file
        let sprint_path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(
            &sprint_path,
            "# STATUS DEFINITIONS\n\
             development_status:\n  \
               epic-3: in-progress\n  \
               3-3-human-escalation: in-progress\n  \
               3-4-decision-logging: ready-for-dev\n",
        )
        .await
        .expect("write sprint-status");

        // 3. Simulate escalation info from supervisor
        let info = EscalationInfo {
            question: "What DB schema should I use?".to_string(),
            reason: "Architect session failed: timeout".to_string(),
        };

        let story_key = "3-3-human-escalation";
        let question = &info.question;
        let reason = &info.reason;

        // --- Act: escalation flow (same order as session orchestration) ---

        // Step 1: Preserve partial work (best-effort)
        let summary = preserve_partial_work(&repo_path, story_key, question).await;
        assert!(summary.contains("WIP commit: yes"), "summary: {summary}");
        assert!(summary.contains("wip.rs"), "summary: {summary}");

        // Step 2: Update sprint-status (returns Result, but failure must not block report)
        let status_result = mark_story_needs_clarification(&sprint_path, story_key).await;
        assert!(status_result.is_ok(), "status update should succeed");

        // Verify sprint-status updated
        let content = tokio::fs::read_to_string(&sprint_path).await.expect("read");
        assert!(content.contains("3-3-human-escalation: needs-clarification"));
        // Other statuses untouched
        assert!(content.contains("3-4-decision-logging: ready-for-dev"));
        // Comments preserved
        assert!(content.contains("# STATUS DEFINITIONS"));

        // Step 3: Build EscalationReport
        let branch = "story/3-3-human-escalation".to_string();
        let report = EscalationReport::new(
            story_key.to_string(),
            question.clone(),
            reason.clone(),
            branch.clone(),
            summary.clone(),
        );

        // --- Assert: report carries full context ---
        assert_eq!(report.story_key, "3-3-human-escalation");
        assert_eq!(report.question, "What DB schema should I use?");
        assert!(report.reason.contains("Architect session failed"));
        assert_eq!(report.branch_name, "story/3-3-human-escalation");
        assert!(report.partial_work_summary.contains("WIP commit: yes"));
        assert!(!report.escalated_at.is_empty());

        // Verify Display works for logging
        let display = format!("{report}");
        assert!(display.contains("Escalation Report"));
        assert!(display.contains("3-3-human-escalation"));

        // Verify serialization round-trip (for notification transport)
        let json = serde_json::to_string(&report).expect("serialize");
        let deser: EscalationReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, deser);
    }

    #[tokio::test]
    async fn test_escalation_flow_status_update_failure_does_not_block_report() {
        use crate::session::escalation::EscalationReport;

        // --- Arrange ---
        let dir = TempDir::new().expect("tempdir");

        // Git repo (clean — no partial work to preserve)
        let repo_path = dir.path().join("repo");
        std::fs::create_dir_all(&repo_path).expect("mkdir");
        init_test_repo(&repo_path);

        // Sprint-status path that does NOT exist — status update will fail
        let sprint_path = dir.path().join("nonexistent-sprint-status.yaml");

        let story_key = "3-3-human-escalation";
        let question = "What DB schema?";

        // --- Act ---

        // Step 1: Preserve partial work (clean tree — no-op)
        let summary = preserve_partial_work(&repo_path, story_key, question).await;
        assert!(summary.contains("no (clean tree)"), "summary: {summary}");

        // Step 2: Status update FAILS (file doesn't exist) — but we continue
        let status_result = mark_story_needs_clarification(&sprint_path, story_key).await;
        assert!(status_result.is_err(), "status update should fail");

        // Step 3: Build report ANYWAY — daemon must always receive the report
        let report = EscalationReport::new(
            story_key.to_string(),
            question.to_string(),
            "Architect failed".to_string(),
            "story/3-3-human-escalation".to_string(),
            summary,
        );

        // --- Assert: report is valid despite status update failure ---
        assert_eq!(report.story_key, "3-3-human-escalation");
        assert_eq!(report.question, "What DB schema?");
        assert!(!report.escalated_at.is_empty());
    }

    // -----------------------------------------------------------------------
    // extract_short_key tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_short_key_full_key() {
        assert_eq!(
            extract_short_key("7-1-integration-test-infrastructure-fixtures"),
            "7-1"
        );
    }

    #[test]
    fn test_extract_short_key_already_short() {
        assert_eq!(extract_short_key("7-1"), "7-1");
    }

    #[test]
    fn test_extract_short_key_two_digit_epic_and_story() {
        assert_eq!(extract_short_key("12-34-some-feature"), "12-34");
    }

    #[test]
    fn test_extract_short_key_non_numeric_prefix() {
        // Not a valid epic-story pattern — returns unchanged
        assert_eq!(extract_short_key("epic-1-something"), "epic-1-something");
    }

    // -----------------------------------------------------------------------
    // unblock_dependents with short dependency keys (real-world format)
    // -----------------------------------------------------------------------

    fn sample_sprint_status_with_short_deps() -> String {
        r#"# STATUS DEFINITIONS:
# ==================
development_status:
  epic-7: in-progress
  7-1-integration-test-infrastructure-fixtures: done
  7-2-config-startup-validation-integration-tests: blocked # depends-on: 7-1
  7-3-watcher-dependency-resolution-story-selection-integration-tests: blocked # depends-on: 7-1
  7-4-pipeline-orchestration-integration-tests: blocked # depends-on: 7-1
  7-5-session-wal-crash-recovery-integration-tests: blocked # depends-on: 7-1
  epic-8: in-progress
  8-1-read-file-tool: done
  8-2-edit-file-tool: blocked # depends-on: 8-1
"#
        .to_string()
    }

    #[tokio::test]
    async fn test_unblock_dependents_short_key_format() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        tokio::fs::write(&path, sample_sprint_status_with_short_deps())
            .await
            .expect("write");

        let unblocked = unblock_dependents(&path, "7-1-integration-test-infrastructure-fixtures")
            .await
            .expect("should succeed");

        assert_eq!(unblocked.len(), 4);
        assert!(unblocked.contains(&"7-2-config-startup-validation-integration-tests".to_string()));
        assert!(unblocked.contains(
            &"7-3-watcher-dependency-resolution-story-selection-integration-tests".to_string()
        ));
        assert!(unblocked.contains(&"7-4-pipeline-orchestration-integration-tests".to_string()));
        assert!(
            unblocked.contains(&"7-5-session-wal-crash-recovery-integration-tests".to_string())
        );

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains(
            "7-2-config-startup-validation-integration-tests: ready-for-dev # depends-on: 7-1"
        ));
        assert!(content.contains("7-3-watcher-dependency-resolution-story-selection-integration-tests: ready-for-dev # depends-on: 7-1"));
        // 8-2 should remain blocked (different dependency)
        assert!(content.contains("8-2-edit-file-tool: blocked # depends-on: 8-1"));
    }

    #[tokio::test]
    async fn test_unblock_dependents_short_key_no_false_positive() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("sprint-status.yaml");
        let content = r#"development_status:
  7-1-infra: done
  7-10-extra: blocked # depends-on: 7-10-other
  7-11-thing: blocked # depends-on: 7-1
  7-2-config: blocked # depends-on: 7-1
"#;
        tokio::fs::write(&path, content).await.expect("write");

        let unblocked = unblock_dependents(&path, "7-1-infra")
            .await
            .expect("should succeed");

        // 7-11 and 7-2 depend on short key 7-1, should be unblocked
        // 7-10-extra depends on 7-10-other, should NOT be unblocked
        assert_eq!(unblocked.len(), 2);
        assert!(unblocked.contains(&"7-11-thing".to_string()));
        assert!(unblocked.contains(&"7-2-config".to_string()));

        let updated = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(updated.contains("7-10-extra: blocked # depends-on: 7-10-other"));
    }

    // -----------------------------------------------------------------------
    // unstick_orphan_stories tests
    // -----------------------------------------------------------------------

    fn sprint_status_with_in_progress() -> String {
        r#"generated: 2026-02-08
project: test-project

development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: in-progress
  1-3-init: ready-for-dev
  epic-2: in-progress
  2-1-polling: in-progress
  2-2-deps: done
  2-3-cascade: blocked # depends-on: 2-2
"#
        .to_string()
    }

    fn sprint_status_no_in_progress() -> String {
        r#"generated: 2026-02-08
project: test-project

development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: ready-for-dev
  1-3-init: backlog
"#
        .to_string()
    }

    fn write_daemon_state(path: &std::path::Path, pid: u32, status: &str) {
        let state = serde_json::json!({
            "pid": pid,
            "started_at": "2026-02-07T10:00:00+01:00",
            "last_activity": "2026-02-07T10:05:00+01:00",
            "status": status,
            "log_file": "bmad-bot.log",
            "bmad_discovery": null,
            "stories_processed": 0
        });
        std::fs::write(path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn test_unstick_skips_when_wal_exists() {
        let dir = TempDir::new().unwrap();
        let sprint_path = dir.path().join("sprint-status.yaml");
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        let state_path = dir.path().join("bmad-bot.state.json");

        tokio::fs::write(&sprint_path, sprint_status_with_in_progress())
            .await
            .unwrap();
        // Create a WAL file — crash recovery should handle this, not unstick
        tokio::fs::write(&wal_path, "story_id: '1.2'\nstory_key: '1-2-cli'\n")
            .await
            .unwrap();

        let result = unstick_orphan_stories(&sprint_path, &wal_path, &state_path).await;
        assert!(result.is_empty(), "Should skip when WAL exists");

        // Verify sprint-status unchanged
        let content = tokio::fs::read_to_string(&sprint_path).await.unwrap();
        assert!(content.contains("1-2-cli: in-progress"));
        assert!(content.contains("2-1-polling: in-progress"));
    }

    #[tokio::test]
    async fn test_unstick_skips_when_other_daemon_alive() {
        let dir = TempDir::new().unwrap();
        let sprint_path = dir.path().join("sprint-status.yaml");
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        let state_path = dir.path().join("bmad-bot.state.json");

        tokio::fs::write(&sprint_path, sprint_status_with_in_progress())
            .await
            .unwrap();
        // Spawn a real subprocess so we have a guaranteed-alive PID that is not ours
        let child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn sleep");
        let other_pid = child.id();
        // Write state file with the subprocess PID
        write_daemon_state(&state_path, other_pid, "running");

        let result = unstick_orphan_stories(&sprint_path, &wal_path, &state_path).await;

        // Clean up the subprocess
        let _ = std::process::Command::new("kill")
            .arg(other_pid.to_string())
            .status();

        assert!(
            result.is_empty(),
            "Should skip when another daemon is running"
        );

        // Verify sprint-status unchanged
        let content = tokio::fs::read_to_string(&sprint_path).await.unwrap();
        assert!(content.contains("1-2-cli: in-progress"));
    }

    #[tokio::test]
    async fn test_unstick_ignores_own_pid_in_state_file() {
        let dir = TempDir::new().unwrap();
        let sprint_path = dir.path().join("sprint-status.yaml");
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        let state_path = dir.path().join("bmad-bot.state.json");

        tokio::fs::write(&sprint_path, sprint_status_with_in_progress())
            .await
            .unwrap();
        // State file with OUR OWN PID — must not block ourselves
        write_daemon_state(&state_path, std::process::id(), "running");

        let result = unstick_orphan_stories(&sprint_path, &wal_path, &state_path).await;
        assert_eq!(
            result.len(),
            2,
            "Should unstick orphans even when state file has our own PID"
        );
    }

    #[tokio::test]
    async fn test_unstick_resets_orphan_stories() {
        let dir = TempDir::new().unwrap();
        let sprint_path = dir.path().join("sprint-status.yaml");
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        let state_path = dir.path().join("bmad-bot.state.json");

        tokio::fs::write(&sprint_path, sprint_status_with_in_progress())
            .await
            .unwrap();
        // No WAL, no state file → orphans should be reset

        let result = unstick_orphan_stories(&sprint_path, &wal_path, &state_path).await;
        assert_eq!(result.len(), 2, "Should unstick 1-2-cli and 2-1-polling");
        assert!(result.contains(&"1-2-cli".to_string()));
        assert!(result.contains(&"2-1-polling".to_string()));

        // Verify sprint-status updated
        let content = tokio::fs::read_to_string(&sprint_path).await.unwrap();
        assert!(
            content.contains("1-2-cli: ready-for-dev"),
            "1-2-cli should be ready-for-dev, got: {content}"
        );
        assert!(
            content.contains("2-1-polling: ready-for-dev"),
            "2-1-polling should be ready-for-dev, got: {content}"
        );
        // Other statuses should be untouched
        assert!(content.contains("1-1-scaffolding: done"));
        assert!(content.contains("1-3-init: ready-for-dev"));
        assert!(content.contains("2-2-deps: done"));
        assert!(content.contains("2-3-cascade: blocked"));
    }

    #[tokio::test]
    async fn test_unstick_no_orphans_returns_empty() {
        let dir = TempDir::new().unwrap();
        let sprint_path = dir.path().join("sprint-status.yaml");
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        let state_path = dir.path().join("bmad-bot.state.json");

        tokio::fs::write(&sprint_path, sprint_status_no_in_progress())
            .await
            .unwrap();

        let result = unstick_orphan_stories(&sprint_path, &wal_path, &state_path).await;
        assert!(result.is_empty(), "No in-progress stories → nothing to do");
    }

    #[tokio::test]
    async fn test_unstick_dead_daemon_allows_reset() {
        let dir = TempDir::new().unwrap();
        let sprint_path = dir.path().join("sprint-status.yaml");
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        let state_path = dir.path().join("bmad-bot.state.json");

        tokio::fs::write(&sprint_path, sprint_status_with_in_progress())
            .await
            .unwrap();
        // State file with a dead PID (99999999 is almost certainly not alive)
        write_daemon_state(&state_path, 99999999, "running");

        let result = unstick_orphan_stories(&sprint_path, &wal_path, &state_path).await;
        assert_eq!(result.len(), 2, "Dead daemon PID → should unstick orphans");
    }

    #[tokio::test]
    async fn test_unstick_stopped_daemon_allows_reset() {
        let dir = TempDir::new().unwrap();
        let sprint_path = dir.path().join("sprint-status.yaml");
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        let state_path = dir.path().join("bmad-bot.state.json");

        tokio::fs::write(&sprint_path, sprint_status_with_in_progress())
            .await
            .unwrap();
        // State file with current PID but status "stopped"
        write_daemon_state(&state_path, std::process::id(), "stopped");

        let result = unstick_orphan_stories(&sprint_path, &wal_path, &state_path).await;
        assert_eq!(result.len(), 2, "Stopped daemon → should unstick orphans");
    }

    #[tokio::test]
    async fn test_unstick_missing_sprint_status_returns_empty() {
        let dir = TempDir::new().unwrap();
        let sprint_path = dir.path().join("nonexistent-sprint-status.yaml");
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        let state_path = dir.path().join("bmad-bot.state.json");

        let result = unstick_orphan_stories(&sprint_path, &wal_path, &state_path).await;
        assert!(
            result.is_empty(),
            "Missing sprint-status → graceful empty return"
        );
    }

    #[tokio::test]
    async fn test_unstick_preserves_epic_in_progress() {
        let dir = TempDir::new().unwrap();
        let sprint_path = dir.path().join("sprint-status.yaml");
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        let state_path = dir.path().join("bmad-bot.state.json");

        tokio::fs::write(&sprint_path, sprint_status_with_in_progress())
            .await
            .unwrap();

        let _result = unstick_orphan_stories(&sprint_path, &wal_path, &state_path).await;

        // Epic-level "in-progress" must NOT be touched
        let content = tokio::fs::read_to_string(&sprint_path).await.unwrap();
        assert!(
            content.contains("epic-1: in-progress"),
            "Epic status should be preserved, got: {content}"
        );
        assert!(
            content.contains("epic-2: in-progress"),
            "Epic status should be preserved, got: {content}"
        );
    }
}
