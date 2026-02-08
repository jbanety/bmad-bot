//! Minimal read-only file tool for the supervisor Architect session.
//!
//! This tool allows the BMAD Architect agent to load project files
//! (configuration, documentation, source code) during a supervisor
//! fallback session. It is intentionally limited:
//!
//! - **Read-only** — no write, delete, or directory listing
//! - **Project-root bounded** — rejects paths outside `{project_root}`
//! - **Supervisor-only** — located in `src/supervisor/`, not `src/tools/`
//!
//! The full filesystem tool (read + write + directory ops) is in
//! Epic 4, Story 4.1 (`src/tools/fs.rs`). This tool is separate.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

/// Minimal read-only file tool for the supervisor Architect session.
///
/// Reads files relative to the project root. Rejects paths that
/// resolve outside the project root boundary (security).
#[derive(Debug, Serialize, Deserialize)]
pub struct ReadFile {
    /// Absolute path to the project root — all reads are bounded to this directory.
    project_root: PathBuf,
}

/// Arguments passed by the Architect agent when calling the `read_file` tool.
#[derive(Debug, Deserialize)]
pub struct ReadFileArgs {
    /// Relative path from the project root to the file to read.
    pub path: String,
}

/// Errors from the `read_file` supervisor tool.
#[derive(Debug, thiserror::Error)]
pub enum ReadFileError {
    /// The requested file does not exist.
    #[error("File not found: {path}")]
    NotFound {
        /// The path that was requested.
        path: String,
    },

    /// An I/O error occurred while reading the file.
    #[error("Read failed for '{path}': {reason}")]
    ReadFailed {
        /// The path that was requested.
        path: String,
        /// Description of the I/O error.
        reason: String,
    },

    /// The requested path resolves outside the project root boundary.
    #[error("Access denied for '{path}': {reason}")]
    PathDenied {
        /// The path that was requested.
        path: String,
        /// Reason the path was denied.
        reason: String,
    },
}

impl ReadFile {
    /// Create a new `ReadFile` tool bounded to the given project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Validate the requested path is within the project root.
    ///
    /// Resolves the path via `canonicalize()` and checks that it
    /// starts with `self.project_root`. This prevents directory
    /// traversal attacks (e.g. `../../etc/passwd`).
    fn validate_path(&self, requested: &str) -> Result<PathBuf, ReadFileError> {
        let full_path = self.project_root.join(requested);

        // Canonicalize resolves symlinks and `..` components.
        // If the file doesn't exist, canonicalize fails — treat as NotFound.
        let canonical = full_path
            .canonicalize()
            .map_err(|_| ReadFileError::NotFound {
                path: requested.to_string(),
            })?;

        // Canonicalize the project root too (in case it contains symlinks).
        let canonical_root =
            self.project_root
                .canonicalize()
                .map_err(|_| ReadFileError::PathDenied {
                    path: requested.to_string(),
                    reason: "Cannot resolve project root".to_string(),
                })?;

        if !canonical.starts_with(&canonical_root) {
            return Err(ReadFileError::PathDenied {
                path: requested.to_string(),
                reason: "Path is outside project root".to_string(),
            });
        }

        Ok(canonical)
    }
}

impl Tool for ReadFile {
    const NAME: &'static str = "read_file";
    type Error = ReadFileError;
    type Args = ReadFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file from the project. Provide the path relative \
                to the project root. Use this to load configuration files, \
                documentation, architecture docs, and source code as needed. \
                Examples: '_bmad/bmm/config.yaml', 'docs/architecture.md', \
                'src/main.rs'."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path from the project root to the file to read"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::debug!(
            action = "supervisor_read_file",
            path = %args.path,
            "Architect reading file"
        );

        let validated_path = self.validate_path(&args.path)?;

        tokio::fs::read_to_string(&validated_path)
            .await
            .map_err(|e| ReadFileError::ReadFailed {
                path: args.path,
                reason: e.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_file_existing_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.md");
        fs::write(&file_path, "# Test Content\nHello").unwrap();

        let tool = ReadFile::new(dir.path().to_path_buf());
        let args = ReadFileArgs {
            path: "test.md".to_string(),
        };
        let result = tool.call(args).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "# Test Content\nHello");
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let dir = TempDir::new().unwrap();
        let tool = ReadFile::new(dir.path().to_path_buf());
        let args = ReadFileArgs {
            path: "nonexistent.md".to_string(),
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            ReadFileError::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_read_file_path_denied_outside_root() {
        let dir = TempDir::new().unwrap();
        // Create a file outside the project root
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        let tool = ReadFile::new(dir.path().to_path_buf());
        let args = ReadFileArgs {
            path: format!("../../{}", outside_file.display()),
        };
        let result = tool.call(args).await;
        // Should be denied or not found (path traversal blocked)
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_nested_path() {
        let dir = TempDir::new().unwrap();
        let sub_dir = dir.path().join("_bmad/bmm/agents");
        fs::create_dir_all(&sub_dir).unwrap();
        fs::write(sub_dir.join("architect.md"), "# Architect").unwrap();

        let tool = ReadFile::new(dir.path().to_path_buf());
        let args = ReadFileArgs {
            path: "_bmad/bmm/agents/architect.md".to_string(),
        };
        let result = tool.call(args).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "# Architect");
    }

    #[tokio::test]
    async fn test_read_file_tool_definition() {
        let dir = TempDir::new().unwrap();
        let tool = ReadFile::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "read_file");
        assert!(!def.description.is_empty());
        assert!(def.description.contains("project"));
        // Verify parameters include "path" as required
        let params = &def.parameters;
        assert!(
            params["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some("path"))
        );
    }

    #[test]
    fn test_read_file_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ReadFileError>();
    }

    #[test]
    fn test_read_file_error_display() {
        let err = ReadFileError::NotFound {
            path: "missing.md".to_string(),
        };
        assert!(err.to_string().contains("missing.md"));

        let err = ReadFileError::ReadFailed {
            path: "bad.md".to_string(),
            reason: "permission denied".to_string(),
        };
        assert!(err.to_string().contains("bad.md"));
        assert!(err.to_string().contains("permission denied"));

        let err = ReadFileError::PathDenied {
            path: "../etc/passwd".to_string(),
            reason: "Path is outside project root".to_string(),
        };
        assert!(err.to_string().contains("../etc/passwd"));
        assert!(err.to_string().contains("outside project root"));
    }

    #[tokio::test]
    async fn test_read_file_empty_path() {
        let dir = TempDir::new().unwrap();
        let tool = ReadFile::new(dir.path().to_path_buf());
        let args = ReadFileArgs {
            path: String::new(),
        };
        // Empty path should error (canonicalize of dir itself would be a directory, not a file)
        let result = tool.call(args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_serializable() {
        let dir = TempDir::new().unwrap();
        let tool = ReadFile::new(dir.path().to_path_buf());
        let json = serde_json::to_string(&tool).expect("Should serialize");
        let _deserialized: ReadFile = serde_json::from_str(&json).expect("Should deserialize");
    }
}
