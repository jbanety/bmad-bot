//! ListDirectoryTool — list directory contents with entry types and file sizes.
//!
//! This tool provides directory listing functionality for the rig agent.
//! It replaces the `list` action from the legacy `FsTool` with improvements:
//!
//! - **Directories first, then files** — grouped and sorted alphabetically within each group
//! - **Dedicated error variants** — `NotADirectory`, `NotFound`, `PathDenied`, `IoError`
//! - **Single responsibility** — only lists directories, no read/write/delete/mkdir
//!
//! Security: All paths are validated via `canonicalize()` + `starts_with()` to
//! prevent directory traversal outside the project root.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

/// ListDirectoryTool — list directory contents with entry types and file sizes.
///
/// Returns a formatted listing of a directory's contents, grouped by type:
/// directories first (alphabetically), then files (alphabetically), each with
/// size information.
#[derive(Debug, Serialize, Deserialize)]
pub struct ListDirectoryTool {
    /// Absolute path to the project root — all listings are bounded to this directory.
    project_root: PathBuf,
}

/// Arguments passed by the LLM agent when calling the `list_directory` tool.
#[derive(Debug, Deserialize)]
pub struct ListDirectoryToolArgs {
    /// Relative path from the project root to the directory to list.
    /// Use `"."` or `""` to list the project root itself.
    pub path: String,
}

/// Errors from the `list_directory` tool.
#[derive(Debug, thiserror::Error)]
pub enum ListDirectoryToolError {
    /// The requested path resolves outside the project root boundary.
    #[error("Access denied for '{path}': {reason}")]
    PathDenied {
        /// The path that was requested.
        path: String,
        /// Reason the path was denied.
        reason: String,
    },

    /// The requested directory does not exist.
    #[error("Directory not found: {path}")]
    NotFound {
        /// The path that was requested.
        path: String,
    },

    /// The requested path exists but is a file, not a directory.
    #[error("Path is a file, not a directory: {path}")]
    NotADirectory {
        /// The path that was requested.
        path: String,
    },

    /// An I/O error occurred during directory listing.
    #[error("I/O error listing '{path}': {reason}")]
    IoError {
        /// The path that was requested.
        path: String,
        /// Description of the I/O error.
        reason: String,
    },
}

impl ListDirectoryTool {
    /// Create a new `ListDirectoryTool` bounded to the given project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Validate the requested path is within the project root and exists.
    ///
    /// Resolves the path via `canonicalize()` and checks that it starts with
    /// `self.project_root`. This prevents directory traversal attacks.
    fn validate_path(&self, requested: &str) -> Result<PathBuf, ListDirectoryToolError> {
        let full_path = if requested.is_empty() || requested == "." {
            self.project_root.clone()
        } else {
            self.project_root.join(requested)
        };

        // Canonicalize resolves symlinks and `..` components.
        // If the path doesn't exist, canonicalize fails — treat as NotFound.
        let canonical = full_path
            .canonicalize()
            .map_err(|_| ListDirectoryToolError::NotFound {
                path: requested.to_string(),
            })?;

        // Canonicalize the project root too (in case it contains symlinks).
        let canonical_root =
            self.project_root
                .canonicalize()
                .map_err(|_| ListDirectoryToolError::PathDenied {
                    path: requested.to_string(),
                    reason: "Cannot resolve project root".to_string(),
                })?;

        if !canonical.starts_with(&canonical_root) {
            return Err(ListDirectoryToolError::PathDenied {
                path: requested.to_string(),
                reason: "Path is outside project root".to_string(),
            });
        }

        Ok(canonical)
    }
}

impl Tool for ListDirectoryTool {
    const NAME: &'static str = "list_directory";
    type Error = ListDirectoryToolError;
    type Args = ListDirectoryToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "list_directory".to_string(),
            description: "List the contents of a directory in the project. Returns files and \
                subdirectories with types and sizes.\n\n\
                **Output format:** Directories are listed first (alphabetically), then files \
                (alphabetically). Each entry shows:\n\
                - `[dir]  name/` — for directories\n\
                - `[file] name (N bytes)` — for files with their size\n\n\
                **Usage:** Provide a `path` relative to the project root. Use `\".\"` or `\"\"` \
                to list the project root.\n\n\
                **Prefer `list_directory` when** you need to explore directory structure or check \
                what files exist in a specific folder.\n\
                **Prefer `find_path` when** you need to find files matching a pattern across the \
                entire project.\n\
                **Prefer `grep` when** you need to find files containing specific code or text."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path from the project root to the directory to list. Use \".\" or \"\" to list the project root."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::info!(
            action = "list_directory",
            path = %args.path,
            "Listing directory"
        );

        let validated_path = self.validate_path(&args.path)?;

        // Verify the path is a directory, not a file
        if validated_path.is_file() {
            return Err(ListDirectoryToolError::NotADirectory { path: args.path });
        }

        let mut entries = tokio::fs::read_dir(&validated_path).await.map_err(|e| {
            ListDirectoryToolError::IoError {
                path: args.path.clone(),
                reason: e.to_string(),
            }
        })?;

        let mut dirs: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();

        while let Some(entry) =
            entries
                .next_entry()
                .await
                .map_err(|e| ListDirectoryToolError::IoError {
                    path: args.path.clone(),
                    reason: e.to_string(),
                })?
        {
            let metadata = entry
                .metadata()
                .await
                .map_err(|e| ListDirectoryToolError::IoError {
                    path: args.path.clone(),
                    reason: e.to_string(),
                })?;

            let name = entry.file_name().to_string_lossy().to_string();

            if metadata.is_dir() {
                dirs.push(format!("[dir]  {name}/"));
            } else {
                files.push(format!("[file] {name} ({} bytes)", metadata.len()));
            }
        }

        dirs.sort();
        files.sort();

        if dirs.is_empty() && files.is_empty() {
            tracing::info!(
                action = "list_directory_done",
                path = %args.path,
                entries = 0,
                "Directory listing complete (empty)"
            );
            return Ok("Empty directory".to_string());
        }

        let mut result = dirs;
        result.extend(files);

        tracing::info!(
            action = "list_directory_done",
            path = %args.path,
            entries = result.len(),
            "Directory listing complete"
        );

        Ok(result.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Create a test directory structure with a mix of files and subdirectories.
    ///
    /// Structure:
    /// ```text
    /// root/
    ///   src/
    ///     main.rs       → "fn main() {}" (13 bytes)
    ///     lib.rs        → "pub mod tools;" (15 bytes)
    ///   docs/
    ///     README.md     → "# Readme" (8 bytes)
    ///   Cargo.toml      → "[package]" (9 bytes)
    ///   .gitignore      → "target/" (7 bytes)
    /// ```
    fn create_test_directory(root: &std::path::Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("src/lib.rs"), "pub mod tools;\n").unwrap();
        fs::write(root.join("docs/README.md"), "# Readme").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]").unwrap();
        fs::write(root.join(".gitignore"), "target/").unwrap();
    }

    #[tokio::test]
    async fn test_list_directory_basic() {
        let dir = TempDir::new().unwrap();
        create_test_directory(dir.path());

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: ".".to_string(),
        };
        let result = tool.call(args).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        // Should contain both dirs and files
        assert!(output.contains("[dir]"));
        assert!(output.contains("[file]"));
        assert!(output.contains("src/"));
        assert!(output.contains("docs/"));
        assert!(output.contains("Cargo.toml"));
    }

    #[tokio::test]
    async fn test_list_directory_dirs_first_then_files() {
        let dir = TempDir::new().unwrap();
        create_test_directory(dir.path());

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: ".".to_string(),
        };
        let result = tool.call(args).await.unwrap();
        let lines: Vec<&str> = result.lines().collect();

        // Find where dirs end and files begin
        let first_file_idx = lines.iter().position(|l| l.starts_with("[file]")).unwrap();
        let last_dir_idx = lines.iter().rposition(|l| l.starts_with("[dir]")).unwrap();

        // All directories must appear before any file
        assert!(
            last_dir_idx < first_file_idx,
            "Directories must appear before files. Last dir at {last_dir_idx}, first file at {first_file_idx}"
        );
    }

    #[tokio::test]
    async fn test_list_directory_alphabetical_within_groups() {
        let dir = TempDir::new().unwrap();
        // Create dirs: zebra, alpha, middle
        fs::create_dir_all(dir.path().join("zebra")).unwrap();
        fs::create_dir_all(dir.path().join("alpha")).unwrap();
        fs::create_dir_all(dir.path().join("middle")).unwrap();
        // Create files: zoo.txt, apple.txt, banana.txt
        fs::write(dir.path().join("zoo.txt"), "z").unwrap();
        fs::write(dir.path().join("apple.txt"), "a").unwrap();
        fs::write(dir.path().join("banana.txt"), "b").unwrap();

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: ".".to_string(),
        };
        let result = tool.call(args).await.unwrap();
        let lines: Vec<&str> = result.lines().collect();

        // Dirs should be: alpha, middle, zebra (sorted)
        assert_eq!(lines[0], "[dir]  alpha/");
        assert_eq!(lines[1], "[dir]  middle/");
        assert_eq!(lines[2], "[dir]  zebra/");
        // Files should be: apple.txt, banana.txt, zoo.txt (sorted)
        assert!(lines[3].contains("apple.txt"));
        assert!(lines[4].contains("banana.txt"));
        assert!(lines[5].contains("zoo.txt"));
    }

    #[tokio::test]
    async fn test_list_directory_shows_file_sizes() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "Hello, World!").unwrap(); // 13 bytes

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: ".".to_string(),
        };
        let result = tool.call(args).await.unwrap();
        assert!(
            result.contains("(13 bytes)"),
            "Expected file size in output, got: {result}"
        );
    }

    #[tokio::test]
    async fn test_list_directory_dir_entries_have_trailing_slash() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("mydir")).unwrap();

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: ".".to_string(),
        };
        let result = tool.call(args).await.unwrap();
        assert!(
            result.contains("[dir]  mydir/"),
            "Expected '[dir]  mydir/' in output, got: {result}"
        );
    }

    #[tokio::test]
    async fn test_list_directory_empty_directory() {
        let dir = TempDir::new().unwrap();
        let empty = dir.path().join("empty");
        fs::create_dir_all(&empty).unwrap();

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: "empty".to_string(),
        };
        let result = tool.call(args).await.unwrap();
        assert_eq!(result, "Empty directory");
    }

    #[tokio::test]
    async fn test_list_directory_path_denied_outside_root() {
        let dir = TempDir::new().unwrap();
        // Create a file outside the project root to ensure the traversal target exists
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: format!("../../{}", outside.path().display()),
        };
        let result = tool.call(args).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Should be PathDenied or NotFound (depending on resolution)
        assert!(
            matches!(
                &err,
                ListDirectoryToolError::PathDenied { .. } | ListDirectoryToolError::NotFound { .. }
            ),
            "Expected PathDenied or NotFound, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_list_directory_not_found() {
        let dir = TempDir::new().unwrap();

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: "nonexistent".to_string(),
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            ListDirectoryToolError::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_list_directory_not_a_directory() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file.txt"), "content").unwrap();

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: "file.txt".to_string(),
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            ListDirectoryToolError::NotADirectory { .. }
        ));
    }

    #[tokio::test]
    async fn test_list_directory_nested_path() {
        let dir = TempDir::new().unwrap();
        create_test_directory(dir.path());

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: "src".to_string(),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("lib.rs"));
        assert!(result.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_list_directory_hidden_files_included() {
        let dir = TempDir::new().unwrap();
        create_test_directory(dir.path());

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let args = ListDirectoryToolArgs {
            path: ".".to_string(),
        };
        let result = tool.call(args).await.unwrap();
        assert!(
            result.contains(".gitignore"),
            "Expected hidden file .gitignore in output, got: {result}"
        );
    }

    #[tokio::test]
    async fn test_list_directory_definition_name() {
        let dir = TempDir::new().unwrap();
        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        assert_eq!(ListDirectoryTool::NAME, "list_directory");
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "list_directory");
    }

    #[tokio::test]
    async fn test_list_directory_definition_has_detailed_description() {
        let dir = TempDir::new().unwrap();
        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert!(!def.description.is_empty());
        assert!(
            def.description.contains("directory"),
            "Description should mention directory"
        );
        assert!(
            def.description.contains("list_directory"),
            "Description should mention tool name"
        );
        assert!(
            def.description.contains("find_path"),
            "Description should mention find_path for contrast"
        );
        assert!(
            def.description.contains("grep"),
            "Description should mention grep for contrast"
        );
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
    fn test_list_directory_serializable() {
        let dir = TempDir::new().unwrap();
        let tool = ListDirectoryTool::new(dir.path().to_path_buf());
        let json = serde_json::to_string(&tool).expect("Should serialize");
        let _deserialized: ListDirectoryTool =
            serde_json::from_str(&json).expect("Should deserialize");
    }

    #[test]
    fn test_list_directory_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ListDirectoryToolError>();
    }

    #[tokio::test]
    async fn test_list_directory_root_path() {
        let dir = TempDir::new().unwrap();
        create_test_directory(dir.path());

        let tool = ListDirectoryTool::new(dir.path().to_path_buf());

        // Test with empty string
        let args = ListDirectoryToolArgs {
            path: String::new(),
        };
        let result = tool.call(args).await;
        assert!(result.is_ok(), "Empty path should list project root");
        let output = result.unwrap();
        assert!(output.contains("src/"));
        assert!(output.contains("Cargo.toml"));

        // Test with "."
        let args = ListDirectoryToolArgs {
            path: ".".to_string(),
        };
        let result = tool.call(args).await;
        assert!(result.is_ok(), "Dot path should list project root");
    }
}
