//! Filesystem tool — read/write files exposed to the rig agent.
//!
//! Implements the rig `Tool` trait with 6 filesystem actions: read, write, list,
//! mkdir, delete, exists. All operations use `tokio::fs` for async I/O.
//! A project root security boundary prevents path traversal attacks.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

/// Filesystem tool for the rig agent.
///
/// Exposes 6 file operations: read, write, list, mkdir, delete, exists.
/// All paths are validated against the `project_root` security boundary.
/// The struct holds only configuration — no cached file handles or iterators.
#[derive(Debug, Serialize, Deserialize)]
pub struct FsTool {
    /// Absolute path to project root — all operations are bounded to this directory.
    project_root: PathBuf,
}

/// Arguments passed by the LLM agent when calling the `filesystem` tool.
#[derive(Debug, Deserialize)]
pub struct FsToolArgs {
    /// One of: read, write, list, mkdir, delete, exists.
    pub action: String,
    /// Relative path from project root.
    pub path: String,
    /// File content for write action.
    pub content: Option<String>,
    /// For mkdir (create parent dirs) and delete (remove directories).
    pub recursive: Option<bool>,
}

/// Errors from the `filesystem` tool.
#[derive(Debug, thiserror::Error)]
pub enum FsToolError {
    /// Unknown action string.
    #[error(
        "Invalid filesystem action '{action}'. Valid actions: read, write, list, mkdir, delete, exists"
    )]
    InvalidAction {
        /// The action that was not recognized.
        action: String,
    },

    /// Path resolves outside project root.
    #[error("Access denied for '{path}': {reason}")]
    PathDenied {
        /// The path that was denied.
        path: String,
        /// Reason the path was denied.
        reason: String,
    },

    /// Requested file or directory not found.
    #[error("Not found: {path}")]
    NotFound {
        /// The path that was not found.
        path: String,
    },

    /// Wraps std::io::Error with action context.
    #[error("IO error during {action} on '{path}': {reason}")]
    IoError {
        /// The filesystem action that failed.
        action: String,
        /// The path involved.
        path: String,
        /// Description of the I/O error.
        reason: String,
    },

    /// Required argument not provided.
    #[error("Missing required argument '{argument}' for filesystem {action}")]
    MissingArgument {
        /// The filesystem action that needed the argument.
        action: String,
        /// The argument that was missing.
        argument: String,
    },
}

impl FsTool {
    /// Create a new `FsTool` bounded to the given project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Validate a path for an existing file/directory (must exist on disk).
    ///
    /// Resolves the path via `canonicalize()` and checks that it starts with
    /// the canonicalized `project_root`. Prevents directory traversal attacks.
    fn validate_path(&self, requested: &str) -> Result<PathBuf, FsToolError> {
        let full_path = self.project_root.join(requested);

        let canonical = full_path
            .canonicalize()
            .map_err(|_| FsToolError::NotFound {
                path: requested.to_string(),
            })?;

        let canonical_root =
            self.project_root
                .canonicalize()
                .map_err(|_| FsToolError::PathDenied {
                    path: requested.to_string(),
                    reason: "Cannot resolve project root".to_string(),
                })?;

        if !canonical.starts_with(&canonical_root) {
            return Err(FsToolError::PathDenied {
                path: requested.to_string(),
                reason: "Path is outside project root".to_string(),
            });
        }

        Ok(canonical)
    }

    /// Validate a path for a new file/directory (may not exist yet).
    ///
    /// Canonicalizes the **parent** directory (which must exist) and verifies
    /// it is within the project root, then joins the filename.
    fn validate_path_for_new(&self, requested: &str) -> Result<PathBuf, FsToolError> {
        let full_path = self.project_root.join(requested);

        if let Some(parent) = full_path.parent()
            && parent.exists()
        {
            let canonical_parent = parent.canonicalize().map_err(|_| FsToolError::PathDenied {
                path: requested.to_string(),
                reason: "Cannot resolve parent directory".to_string(),
            })?;

            let canonical_root =
                self.project_root
                    .canonicalize()
                    .map_err(|_| FsToolError::PathDenied {
                        path: requested.to_string(),
                        reason: "Cannot resolve project root".to_string(),
                    })?;

            if !canonical_parent.starts_with(&canonical_root) {
                return Err(FsToolError::PathDenied {
                    path: requested.to_string(),
                    reason: "Path is outside project root".to_string(),
                });
            }

            if let Some(file_name) = full_path.file_name() {
                return Ok(canonical_parent.join(file_name));
            }
        }

        // Fallback — will fail at IO if truly invalid
        Ok(full_path)
    }

    /// Read a file and return its content.
    async fn handle_read(&self, requested: &str) -> Result<String, FsToolError> {
        let path = self.validate_path(requested)?;

        tracing::info!(action = "fs_read", path = %path.display(), "Reading file");

        tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| FsToolError::IoError {
                action: "read".to_string(),
                path: requested.to_string(),
                reason: e.to_string(),
            })
    }

    /// Write content to a file, creating parent directories if needed.
    async fn handle_write(&self, requested: &str, content: &str) -> Result<String, FsToolError> {
        let full_path = self.project_root.join(requested);

        // Create parent directories if they don't exist (within project root)
        if let Some(parent) = full_path.parent()
            && !parent.exists()
        {
            // Validate that the eventual parent is within project root by
            // walking up to find the first existing ancestor.
            let mut ancestor = parent.to_path_buf();
            while !ancestor.exists() {
                if let Some(a) = ancestor.parent() {
                    ancestor = a.to_path_buf();
                } else {
                    break;
                }
            }

            if ancestor.exists() {
                let canonical_ancestor =
                    ancestor
                        .canonicalize()
                        .map_err(|_| FsToolError::PathDenied {
                            path: requested.to_string(),
                            reason: "Cannot resolve ancestor directory".to_string(),
                        })?;

                let canonical_root =
                    self.project_root
                        .canonicalize()
                        .map_err(|_| FsToolError::PathDenied {
                            path: requested.to_string(),
                            reason: "Cannot resolve project root".to_string(),
                        })?;

                if !canonical_ancestor.starts_with(&canonical_root) {
                    return Err(FsToolError::PathDenied {
                        path: requested.to_string(),
                        reason: "Path is outside project root".to_string(),
                    });
                }
            }

            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| FsToolError::IoError {
                    action: "write".to_string(),
                    path: requested.to_string(),
                    reason: format!("Failed to create parent directories: {}", e),
                })?;
        }

        // Now validate the path (parent exists at this point)
        let path = self.validate_path_for_new(requested)?;

        tracing::info!(action = "fs_write", path = %path.display(), bytes = content.len(), "Writing file");

        tokio::fs::write(&path, content)
            .await
            .map_err(|e| FsToolError::IoError {
                action: "write".to_string(),
                path: requested.to_string(),
                reason: e.to_string(),
            })?;

        Ok(format!("Written {} bytes to {}", content.len(), requested))
    }

    /// List directory contents with file types and sizes.
    async fn handle_list(&self, requested: &str) -> Result<String, FsToolError> {
        let path = self.validate_path(requested)?;

        tracing::info!(action = "fs_list", path = %path.display(), "Listing directory");

        let mut entries = tokio::fs::read_dir(&path)
            .await
            .map_err(|e| FsToolError::IoError {
                action: "list".to_string(),
                path: requested.to_string(),
                reason: e.to_string(),
            })?;

        let mut lines: Vec<String> = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| FsToolError::IoError {
                action: "list".to_string(),
                path: requested.to_string(),
                reason: e.to_string(),
            })?
        {
            let metadata = entry.metadata().await.map_err(|e| FsToolError::IoError {
                action: "list".to_string(),
                path: requested.to_string(),
                reason: e.to_string(),
            })?;

            let name = entry.file_name().to_string_lossy().to_string();

            if metadata.is_dir() {
                lines.push(format!("[dir] {}/", name));
            } else {
                lines.push(format!("[file] {} ({} bytes)", name, metadata.len()));
            }
        }

        lines.sort();

        if lines.is_empty() {
            Ok("Empty directory".to_string())
        } else {
            Ok(lines.join("\n"))
        }
    }

    /// Create a directory.
    async fn handle_mkdir(&self, requested: &str, recursive: bool) -> Result<String, FsToolError> {
        let path = self.validate_path_for_new(requested)?;

        tracing::info!(action = "fs_mkdir", path = %path.display(), recursive = recursive, "Creating directory");

        if recursive {
            tokio::fs::create_dir_all(&path)
                .await
                .map_err(|e| FsToolError::IoError {
                    action: "mkdir".to_string(),
                    path: requested.to_string(),
                    reason: e.to_string(),
                })?;
        } else {
            tokio::fs::create_dir(&path)
                .await
                .map_err(|e| FsToolError::IoError {
                    action: "mkdir".to_string(),
                    path: requested.to_string(),
                    reason: e.to_string(),
                })?;
        }

        Ok(format!("Created directory {}", requested))
    }

    /// Delete a file or directory.
    async fn handle_delete(&self, requested: &str, recursive: bool) -> Result<String, FsToolError> {
        let path = self.validate_path(requested)?;

        tracing::info!(action = "fs_delete", path = %path.display(), recursive = recursive, "Deleting");

        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|e| FsToolError::IoError {
                action: "delete".to_string(),
                path: requested.to_string(),
                reason: e.to_string(),
            })?;

        if metadata.is_dir() {
            if recursive {
                tokio::fs::remove_dir_all(&path)
                    .await
                    .map_err(|e| FsToolError::IoError {
                        action: "delete".to_string(),
                        path: requested.to_string(),
                        reason: e.to_string(),
                    })?;
            } else {
                tokio::fs::remove_dir(&path)
                    .await
                    .map_err(|e| FsToolError::IoError {
                        action: "delete".to_string(),
                        path: requested.to_string(),
                        reason: e.to_string(),
                    })?;
            }
        } else {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| FsToolError::IoError {
                    action: "delete".to_string(),
                    path: requested.to_string(),
                    reason: e.to_string(),
                })?;
        }

        Ok(format!("Deleted {}", requested))
    }

    /// Check if a path exists and report type.
    async fn handle_exists(&self, requested: &str) -> Result<String, FsToolError> {
        let full_path = self.project_root.join(requested);

        tracing::info!(action = "fs_exists", path = %full_path.display(), "Checking existence");

        // Validate the path is within project root if it exists
        if full_path.exists() {
            let canonical = full_path
                .canonicalize()
                .map_err(|_| FsToolError::PathDenied {
                    path: requested.to_string(),
                    reason: "Cannot resolve path".to_string(),
                })?;

            let canonical_root =
                self.project_root
                    .canonicalize()
                    .map_err(|_| FsToolError::PathDenied {
                        path: requested.to_string(),
                        reason: "Cannot resolve project root".to_string(),
                    })?;

            if !canonical.starts_with(&canonical_root) {
                return Err(FsToolError::PathDenied {
                    path: requested.to_string(),
                    reason: "Path is outside project root".to_string(),
                });
            }

            let metadata =
                tokio::fs::metadata(&canonical)
                    .await
                    .map_err(|e| FsToolError::IoError {
                        action: "exists".to_string(),
                        path: requested.to_string(),
                        reason: e.to_string(),
                    })?;

            if metadata.is_dir() {
                Ok("exists: true (directory)".to_string())
            } else {
                Ok("exists: true (file)".to_string())
            }
        } else {
            Ok("exists: false".to_string())
        }
    }
}

impl Tool for FsTool {
    const NAME: &'static str = "filesystem";
    type Error = FsToolError;
    type Args = FsToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "filesystem".to_string(),
            description: "Perform filesystem operations within the project directory. Supports 6 actions: \
                'read' (read file content), 'write' (write content to file, creates parent dirs if needed), \
                'list' (list directory contents with types and sizes), 'mkdir' (create directory), \
                'delete' (delete file or directory), 'exists' (check if path exists and its type). \
                All paths are relative to the project root. Path traversal outside the project root is blocked. \
                Use 'exists' to check before reading, 'list' to explore directory structure, \
                'write' to create or update files, 'read' to get file contents."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["read", "write", "list", "mkdir", "delete", "exists"],
                        "description": "The filesystem action to perform"
                    },
                    "path": {
                        "type": "string",
                        "description": "Relative path from the project root"
                    },
                    "content": {
                        "type": "string",
                        "description": "File content for the write action"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "For mkdir: create parent directories. For delete: remove directories recursively."
                    }
                },
                "required": ["action", "path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::info!(action = "filesystem", sub_action = %args.action, path = %args.path, "Filesystem tool called");

        let result = match args.action.as_str() {
            "read" => self.handle_read(&args.path).await?,
            "write" => {
                let content =
                    args.content
                        .as_deref()
                        .ok_or_else(|| FsToolError::MissingArgument {
                            action: "write".to_string(),
                            argument: "content".to_string(),
                        })?;
                self.handle_write(&args.path, content).await?
            }
            "list" => self.handle_list(&args.path).await?,
            "mkdir" => {
                let recursive = args.recursive.unwrap_or(false);
                self.handle_mkdir(&args.path, recursive).await?
            }
            "delete" => {
                let recursive = args.recursive.unwrap_or(false);
                self.handle_delete(&args.path, recursive).await?
            }
            "exists" => self.handle_exists(&args.path).await?,
            other => {
                return Err(FsToolError::InvalidAction {
                    action: other.to_string(),
                });
            }
        };

        tracing::info!(action = "filesystem", sub_action = %args.action, path = %args.path, "Filesystem operation completed");
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_fs_tool_definition_name() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "filesystem");
        assert_eq!(FsTool::NAME, "filesystem");
    }

    #[tokio::test]
    async fn test_fs_tool_definition_has_detailed_description() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert!(def.description.contains("read"));
        assert!(def.description.contains("write"));
        assert!(def.description.contains("list"));
        assert!(def.description.contains("mkdir"));
        assert!(def.description.contains("delete"));
        assert!(def.description.contains("exists"));
        assert!(def.description.contains("project root"));
    }

    #[tokio::test]
    async fn test_fs_tool_definition_action_enum() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;

        let action_prop = &def.parameters["properties"]["action"];
        let enum_values = action_prop["enum"]
            .as_array()
            .expect("action should have enum");
        assert_eq!(enum_values.len(), 6);

        let expected = ["read", "write", "list", "mkdir", "delete", "exists"];
        for action in &expected {
            assert!(
                enum_values.iter().any(|v| v.as_str() == Some(action)),
                "Missing action '{}' in enum",
                action
            );
        }
    }

    #[test]
    fn test_fs_tool_args_deserialize_minimal() {
        let json = r#"{"action": "read", "path": "src/main.rs"}"#;
        let args: FsToolArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.action, "read");
        assert_eq!(args.path, "src/main.rs");
        assert!(args.content.is_none());
        assert!(args.recursive.is_none());
    }

    #[test]
    fn test_fs_tool_args_deserialize_full() {
        let json = r#"{
            "action": "write",
            "path": "src/new_file.rs",
            "content": "fn main() {}",
            "recursive": true
        }"#;
        let args: FsToolArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.action, "write");
        assert_eq!(args.path, "src/new_file.rs");
        assert_eq!(args.content.unwrap(), "fn main() {}");
        assert!(args.recursive.unwrap());
    }

    #[test]
    fn test_fs_tool_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FsToolError>();
    }

    #[test]
    fn test_fs_tool_error_display() {
        let err = FsToolError::InvalidAction {
            action: "rename".to_string(),
        };
        assert!(err.to_string().contains("rename"));
        assert!(err.to_string().contains("Invalid filesystem action"));

        let err = FsToolError::PathDenied {
            path: "../etc/passwd".to_string(),
            reason: "Path is outside project root".to_string(),
        };
        assert!(err.to_string().contains("../etc/passwd"));
        assert!(err.to_string().contains("outside project root"));

        let err = FsToolError::NotFound {
            path: "nonexistent.txt".to_string(),
        };
        assert!(err.to_string().contains("nonexistent.txt"));

        let err = FsToolError::IoError {
            action: "read".to_string(),
            path: "locked.txt".to_string(),
            reason: "permission denied".to_string(),
        };
        assert!(err.to_string().contains("read"));
        assert!(err.to_string().contains("locked.txt"));
        assert!(err.to_string().contains("permission denied"));

        let err = FsToolError::MissingArgument {
            action: "write".to_string(),
            argument: "content".to_string(),
        };
        assert!(err.to_string().contains("write"));
        assert!(err.to_string().contains("content"));
    }

    #[test]
    fn test_fs_tool_serializable() {
        let tool = FsTool::new(PathBuf::from("/tmp/test-project"));
        let json = serde_json::to_string(&tool).expect("Should serialize");
        let deserialized: FsTool = serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(
            deserialized.project_root,
            PathBuf::from("/tmp/test-project")
        );
    }

    #[tokio::test]
    async fn test_fs_tool_invalid_action() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "rename".to_string(),
            path: "test.txt".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            FsToolError::InvalidAction { .. }
        ));
    }

    #[tokio::test]
    async fn test_fs_tool_path_denied_outside_root() {
        let dir = TempDir::new().unwrap();
        // Create a file outside the project root
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret data").unwrap();

        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "read".to_string(),
            path: format!("../../{}", outside_file.display()),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fs_tool_read_existing_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "Hello, World!").unwrap();

        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "read".to_string(),
            path: "hello.txt".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[tokio::test]
    async fn test_fs_tool_read_not_found() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "read".to_string(),
            path: "nonexistent.txt".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(result.unwrap_err(), FsToolError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_fs_tool_write_new_file() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "write".to_string(),
            path: "new_file.txt".to_string(),
            content: Some("new content".to_string()),
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Written"));
        assert!(result.contains("11 bytes"));

        // Verify file was actually written
        let content = fs::read_to_string(dir.path().join("new_file.txt")).unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn test_fs_tool_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("existing.txt"), "old content").unwrap();

        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "write".to_string(),
            path: "existing.txt".to_string(),
            content: Some("new content".to_string()),
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Written"));

        let content = fs::read_to_string(dir.path().join("existing.txt")).unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn test_fs_tool_write_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "write".to_string(),
            path: "deep/nested/dir/file.txt".to_string(),
            content: Some("deep content".to_string()),
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Written"));

        let content = fs::read_to_string(dir.path().join("deep/nested/dir/file.txt")).unwrap();
        assert_eq!(content, "deep content");
    }

    #[tokio::test]
    async fn test_fs_tool_list_directory() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file_a.txt"), "aaa").unwrap();
        fs::write(dir.path().join("file_b.txt"), "bbbbbb").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();

        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "list".to_string(),
            path: ".".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("[file] file_a.txt"));
        assert!(result.contains("[file] file_b.txt"));
        assert!(result.contains("[dir] subdir/"));
    }

    #[tokio::test]
    async fn test_fs_tool_list_empty_directory() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("empty")).unwrap();

        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "list".to_string(),
            path: "empty".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert_eq!(result, "Empty directory");
    }

    #[tokio::test]
    async fn test_fs_tool_mkdir_single() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "mkdir".to_string(),
            path: "new_dir".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Created directory"));
        assert!(dir.path().join("new_dir").is_dir());
    }

    #[tokio::test]
    async fn test_fs_tool_mkdir_recursive() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "mkdir".to_string(),
            path: "a/b/c".to_string(),
            content: None,
            recursive: Some(true),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Created directory"));
        assert!(dir.path().join("a/b/c").is_dir());
    }

    #[tokio::test]
    async fn test_fs_tool_delete_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("to_delete.txt"), "bye").unwrap();

        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "delete".to_string(),
            path: "to_delete.txt".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Deleted"));
        assert!(!dir.path().join("to_delete.txt").exists());
    }

    #[tokio::test]
    async fn test_fs_tool_delete_directory_recursive() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("dir_to_delete/nested");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("file.txt"), "content").unwrap();

        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "delete".to_string(),
            path: "dir_to_delete".to_string(),
            content: None,
            recursive: Some(true),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Deleted"));
        assert!(!dir.path().join("dir_to_delete").exists());
    }

    #[tokio::test]
    async fn test_fs_tool_exists_true_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("exists.txt"), "yes").unwrap();

        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "exists".to_string(),
            path: "exists.txt".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert_eq!(result, "exists: true (file)");
    }

    #[tokio::test]
    async fn test_fs_tool_exists_true_directory() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("a_dir")).unwrap();

        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "exists".to_string(),
            path: "a_dir".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert_eq!(result, "exists: true (directory)");
    }

    #[tokio::test]
    async fn test_fs_tool_exists_false() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "exists".to_string(),
            path: "nope.txt".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await.unwrap();
        assert_eq!(result, "exists: false");
    }

    #[tokio::test]
    async fn test_fs_tool_write_missing_content() {
        let dir = TempDir::new().unwrap();
        let tool = FsTool::new(dir.path().to_path_buf());
        let args = FsToolArgs {
            action: "write".to_string(),
            path: "file.txt".to_string(),
            content: None,
            recursive: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            FsToolError::MissingArgument { .. }
        ));
    }
}
