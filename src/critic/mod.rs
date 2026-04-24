use std::io::Write;
use std::path::{Path, PathBuf};

/// Errors that can occur during critic memory file operations.
#[derive(Debug, thiserror::Error)]
pub enum CriticMemoryError {
    #[error("Critic memory I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Persistent memory file for the Story Critic.
///
/// Manages a `critic-memory.md` file in the implementation artifacts directory.
/// The Critic agent appends observations after each review; the daemon only
/// creates the file, provides it as context, and checks its size.
pub struct CriticMemory {
    file_path: PathBuf,
    size_threshold_bytes: u64,
}

impl CriticMemory {
    /// Creates a new `CriticMemory` pointing at `{project_root}/{impl_artifacts_dir}/critic-memory.md`.
    pub fn new(impl_artifacts_dir: &str, project_root: &str, threshold_kb: u64) -> Self {
        let file_path = Path::new(project_root)
            .join(impl_artifacts_dir)
            .join("critic-memory.md");
        Self {
            file_path,
            size_threshold_bytes: threshold_kb.saturating_mul(1024),
        }
    }

    /// Atomically creates the memory file with an initial header if it does not exist.
    pub fn ensure_exists(&self) -> Result<(), CriticMemoryError> {
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.file_path)
        {
            Ok(mut file) => {
                let date = chrono::Local::now().format("%Y-%m-%d");
                let header = format!("# Story Critic Memory\n\nInitialized: {date}\n");
                file.write_all(header.as_bytes())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    /// Returns the resolved absolute path to the memory file.
    pub fn path(&self) -> &Path {
        &self.file_path
    }

    /// Emits a `tracing::warn!` if the memory file exceeds the configured size threshold.
    ///
    /// Silently returns if the file does not exist. Logs non-NotFound errors.
    pub fn check_size_threshold(&self) {
        let meta = match std::fs::metadata(&self.file_path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    path = %self.file_path.display(),
                    "Failed to read critic memory file metadata"
                );
                return;
            }
        };
        let actual_size = meta.len();
        if actual_size > self.size_threshold_bytes {
            let threshold_kb = self.size_threshold_bytes / 1024;
            tracing::warn!(
                threshold_kb = threshold_kb,
                actual_bytes = actual_size,
                path = %self.file_path.display(),
                "Critic memory file exceeds size threshold, consider manual review or summarization"
            );
        }
    }

    /// Ensures the memory file exists and returns its path as a string.
    ///
    /// Returns `None` (degraded mode) if the file cannot be created, logging
    /// a warning. Callers should proceed without memory context in that case.
    pub fn prepare_context_path(&self) -> Option<String> {
        match self.ensure_exists() {
            Ok(()) => Some(self.file_path.to_string_lossy().to_string()),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    path = %self.file_path.display(),
                    "Critic memory unavailable — proceeding without memory (degraded mode)"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_critic_memory_creates_file_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let cm = CriticMemory::new("artifacts", root, 50);

        cm.ensure_exists().unwrap();

        let content = std::fs::read_to_string(cm.path()).unwrap();
        assert!(content.contains("# Story Critic Memory"));
        assert!(content.contains("Initialized:"));
    }

    #[test]
    fn test_critic_memory_idempotent_ensure_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let cm = CriticMemory::new("artifacts", root, 50);

        cm.ensure_exists().unwrap();
        let content_first = std::fs::read_to_string(cm.path()).unwrap();

        cm.ensure_exists().unwrap();
        let content_second = std::fs::read_to_string(cm.path()).unwrap();

        assert_eq!(content_first, content_second);
    }

    #[test]
    fn test_critic_memory_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let cm = CriticMemory::new("deep/nested/artifacts", root, 50);

        cm.ensure_exists().unwrap();
        assert!(cm.path().exists());
    }

    #[test]
    fn test_critic_memory_path_returns_correct_path() {
        let cm = CriticMemory::new("impl-artifacts", "/project/root", 50);
        let expected = Path::new("/project/root/impl-artifacts/critic-memory.md");
        assert_eq!(cm.path(), expected);
    }

    #[test]
    fn test_critic_memory_check_size_no_warning_under_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let cm = CriticMemory::new("artifacts", root, 50);

        cm.ensure_exists().unwrap();
        cm.check_size_threshold();
    }

    #[test]
    fn test_critic_memory_check_size_warns_over_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let cm = CriticMemory::new("artifacts", root, 1);

        cm.ensure_exists().unwrap();
        let big_content = "x".repeat(2048);
        std::fs::write(cm.path(), big_content).unwrap();

        cm.check_size_threshold();
    }

    #[test]
    fn test_critic_memory_missing_file_no_error() {
        let cm = CriticMemory::new("nonexistent", "/nonexistent/path", 50);
        cm.check_size_threshold();
    }

    #[test]
    fn test_critic_memory_config_default_threshold() {
        let cm = CriticMemory::new("artifacts", "/root", 50);
        assert_eq!(cm.size_threshold_bytes, 50 * 1024);
    }

    #[test]
    fn test_critic_memory_config_custom_threshold() {
        let cm = CriticMemory::new("artifacts", "/root", 100);
        assert_eq!(cm.size_threshold_bytes, 100 * 1024);
    }

    #[test]
    fn test_critic_memory_prepare_context_path_returns_some() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let cm = CriticMemory::new("artifacts", root, 50);

        let result = cm.prepare_context_path();
        assert!(result.is_some());
        let path_str = result.unwrap();
        assert!(path_str.contains("critic-memory.md"));
        assert!(cm.path().exists());
    }

    #[test]
    fn test_critic_memory_prepare_context_path_returns_none_on_failure() {
        let cm = CriticMemory::new("artifacts", "/proc/nonexistent", 50);
        let result = cm.prepare_context_path();
        assert!(result.is_none());
    }

    #[test]
    fn test_critic_memory_saturating_mul_no_overflow() {
        let cm = CriticMemory::new("artifacts", "/root", u64::MAX);
        assert_eq!(cm.size_threshold_bytes, u64::MAX);
    }
}
