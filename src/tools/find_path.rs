//! FindPathTool — find files in the project by glob pattern.
//!
//! This tool provides glob-based file path discovery across project files with:
//!
//! - **.gitignore-aware** file traversal via the `ignore` crate
//! - **Glob pattern matching** via the `globset` crate
//! - **Pagination** with offset-based navigation (50 results per page)
//! - **Alphabetically sorted** results
//!
//! Security: searches are bounded to the project root. The `.git` directory is always excluded.

use globset::Glob;
use ignore::WalkBuilder;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

/// Maximum number of results returned per page.
const PAGE_SIZE: usize = 50;

/// FindPathTool — find files in the project by glob pattern.
#[derive(Debug, Serialize, Deserialize)]
pub struct FindPathTool {
    /// Absolute path to the project root — all searches are bounded to this directory.
    project_root: PathBuf,
}

/// Arguments passed by the LLM agent when calling the `find_path` tool.
#[derive(Debug, Deserialize)]
pub struct FindPathToolArgs {
    /// Glob pattern to match file paths against (e.g., `"**/*.rs"`, `"src/**/mod.rs"`).
    pub glob: String,
    /// Pagination offset — number of results to skip before collecting. Default: 0.
    pub offset: Option<u32>,
}

/// Errors from the `find_path` tool.
#[derive(Debug, thiserror::Error)]
pub enum FindPathToolError {
    /// The glob pattern is invalid.
    #[error("Invalid glob pattern '{pattern}': {reason}")]
    InvalidGlob {
        /// The glob pattern that failed to compile.
        pattern: String,
        /// Description of the error.
        reason: String,
    },

    /// The project root path could not be resolved.
    #[error("Access denied for '{path}': {reason}")]
    PathDenied {
        /// The path that was requested.
        path: String,
        /// Reason the path was denied.
        reason: String,
    },

    /// A directory traversal error occurred.
    #[error("Walk error: {reason}")]
    WalkError {
        /// Description of the error.
        reason: String,
    },
}

impl FindPathTool {
    /// Create a new `FindPathTool` bounded to the given project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Validate and canonicalize the project root.
    fn validate_project_root(&self) -> Result<PathBuf, FindPathToolError> {
        self.project_root
            .canonicalize()
            .map_err(|_| FindPathToolError::PathDenied {
                path: self.project_root.display().to_string(),
                reason: "Cannot resolve project root".to_string(),
            })
    }

    /// Execute the find_path search synchronously (intended for use inside `spawn_blocking`).
    fn find_sync(
        project_root: PathBuf,
        glob_pattern: &str,
        offset: u32,
    ) -> Result<String, FindPathToolError> {
        // Compile glob
        let matcher = Glob::new(glob_pattern)
            .map_err(|e| FindPathToolError::InvalidGlob {
                pattern: glob_pattern.to_string(),
                reason: e.to_string(),
            })?
            .compile_matcher();

        // Build directory walker
        let walker = WalkBuilder::new(&project_root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .filter_entry(|entry| {
                // Skip .git directory
                !(entry.file_type().is_some_and(|ft| ft.is_dir()) && entry.file_name() == ".git")
            })
            .build();

        let mut matching_paths: Vec<String> = Vec::new();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Only process files, not directories
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let abs_path = entry.path();

            // Compute relative path
            let rel_path = match abs_path.strip_prefix(&project_root) {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            // Test against glob pattern
            if matcher.is_match(&rel_path) {
                matching_paths.push(rel_path);
            }
        }

        // Sort alphabetically
        matching_paths.sort();

        let total_matches = matching_paths.len();

        if total_matches == 0 {
            let mut msg = format!("No files found matching pattern '{glob_pattern}'.");
            // Add hint if pattern doesn't contain **/ or /
            if !glob_pattern.contains("**/") && !glob_pattern.contains('/') {
                msg.push_str(&format!(
                    "\nHint: use **/{glob_pattern} to search recursively."
                ));
            }
            return Ok(msg);
        }

        // Apply pagination
        let offset = offset as usize;
        let page_end = (offset + PAGE_SIZE).min(total_matches);

        if offset >= total_matches {
            return Ok(format!(
                "Found {} total matches. Offset {} is beyond the last result.",
                total_matches, offset
            ));
        }

        // Build output
        let mut output = String::new();

        if total_matches <= PAGE_SIZE && offset == 0 {
            output.push_str(&format!(
                "Found {} total matches. Showing results 1-{}.\n",
                total_matches, total_matches
            ));
        } else {
            let remaining = total_matches.saturating_sub(page_end);
            if remaining > 0 {
                output.push_str(&format!(
                    "Found {} total matches. Showing results {}-{} ({} more available, use offset: {} to see next page).\n",
                    total_matches,
                    offset + 1,
                    page_end,
                    remaining,
                    page_end
                ));
            } else {
                output.push_str(&format!(
                    "Found {} total matches. Showing results {}-{}.\n",
                    total_matches,
                    offset + 1,
                    page_end
                ));
            }
        }

        output.push('\n');
        for path in &matching_paths[offset..page_end] {
            output.push_str(path);
            output.push('\n');
        }

        Ok(output)
    }
}

impl Tool for FindPathTool {
    const NAME: &'static str = "find_path";
    type Error = FindPathToolError;
    type Args = FindPathToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "find_path".to_string(),
            description: "Find files in the project by glob pattern. Returns matching file paths \
                sorted alphabetically.\n\n\
                **Usage:** Provide a `glob` pattern using standard glob syntax:\n\
                - `**/*.rs` — all Rust files recursively\n\
                - `src/**/mod.rs` — all mod.rs files under src/\n\
                - `Cargo.*` — files starting with \"Cargo\" in the root\n\
                - `src/tools/*.rs` — Rust files directly in src/tools/\n\n\
                **Results:** One path per line, sorted alphabetically. Results are paginated \
                (50 per page). Use `offset` to get subsequent pages.\n\n\
                **Prefer `find_path` over `grep` when** you need to discover files by name \
                or extension.\n\
                **Prefer `grep` over `find_path` when** you need to find files containing \
                specific code or text."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "glob": {
                        "type": "string",
                        "description": "Glob pattern to match file paths. Use **/*.rs for recursive matching — *.rs only matches the root directory. Examples: \"**/*.rs\", \"src/**/mod.rs\", \"Cargo.*\""
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 0,
                        "description": "Pagination offset — number of results to skip. Use to get subsequent pages of results."
                    }
                },
                "required": ["glob"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::info!(
            action = "find_path",
            glob = %args.glob,
            offset = ?args.offset,
            "Finding file paths"
        );

        let canonical_root = self.validate_project_root()?;
        let glob_str = args.glob.clone();
        let offset = args.offset.unwrap_or(0);

        let result =
            tokio::task::spawn_blocking(move || Self::find_sync(canonical_root, &glob_str, offset))
                .await
                .map_err(|e| FindPathToolError::WalkError {
                    reason: format!("Task join error: {e}"),
                })??;

        tracing::info!(
            action = "find_path",
            glob = %args.glob,
            result_bytes = result.len(),
            "Path search complete"
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: create a test project directory with known files.
    fn create_test_project(dir: &std::path::Path) {
        // Create .git directory so ignore crate recognizes .gitignore
        fs::create_dir_all(dir.join(".git")).unwrap();

        // Create source files
        fs::create_dir_all(dir.join("src/tools")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(dir.join("src/lib.rs"), "pub mod tools;\n").unwrap();
        fs::write(dir.join("src/tools/mod.rs"), "pub mod grep;\n").unwrap();
        fs::write(dir.join("src/tools/grep.rs"), "pub struct GrepTool;\n").unwrap();
        fs::write(
            dir.join("src/tools/find_path.rs"),
            "pub struct FindPathTool;\n",
        )
        .unwrap();

        // Non-Rust files
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
        fs::write(dir.join("Cargo.lock"), "# lock file\n").unwrap();
        fs::write(dir.join("README.md"), "# Test Project\n").unwrap();

        // .gitignore and ignored files
        fs::write(dir.join(".gitignore"), "target/\n*.log\n").unwrap();
        fs::create_dir_all(dir.join("target/debug")).unwrap();
        fs::write(dir.join("target/debug/output"), "binary stuff").unwrap();
        fs::write(dir.join("build.log"), "build log").unwrap();
    }

    // -----------------------------------------------------------------------
    // Tool definition tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_find_path_definition_name() {
        assert_eq!(FindPathTool::NAME, "find_path");
        let dir = TempDir::new().unwrap();
        let tool = FindPathTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "find_path");
    }

    #[tokio::test]
    async fn test_find_path_definition_has_detailed_description() {
        let dir = TempDir::new().unwrap();
        let tool = FindPathTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert!(!def.description.is_empty());
        assert!(def.description.contains("glob"));
        assert!(def.description.contains("**/*.rs"));
        assert!(def.description.contains("find_path"));
        assert!(def.description.contains("grep"));
    }

    #[test]
    fn test_find_path_serializable() {
        let tool = FindPathTool::new(PathBuf::from("/tmp/test"));
        let json = serde_json::to_string(&tool).expect("Should serialize");
        let _deserialized: FindPathTool = serde_json::from_str(&json).expect("Should deserialize");
    }

    #[test]
    fn test_find_path_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FindPathToolError>();
    }

    // -----------------------------------------------------------------------
    // Basic glob tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_find_path_basic_glob() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.rs".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("src/lib.rs"));
        assert!(result.contains("src/tools/grep.rs"));
        assert!(result.contains("src/tools/find_path.rs"));
        assert!(result.contains("src/tools/mod.rs"));
    }

    #[tokio::test]
    async fn test_find_path_specific_pattern() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "src/**/mod.rs".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("src/tools/mod.rs"));
        assert!(!result.contains("src/main.rs"));
    }

    #[tokio::test]
    async fn test_find_path_exact_filename() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "Cargo.toml".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Cargo.toml"));
        assert!(!result.contains("Cargo.lock"));
    }

    #[tokio::test]
    async fn test_find_path_wildcard_extension() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.md".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("README.md"));
        assert!(!result.contains(".rs"));
    }

    #[tokio::test]
    async fn test_find_path_no_matches() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.py".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No files found matching pattern"));
    }

    #[tokio::test]
    async fn test_find_path_results_sorted_alphabetically() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.rs".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        let paths: Vec<&str> = result.lines().filter(|l| l.contains(".rs")).collect();
        // Verify sorted
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted);
    }

    // -----------------------------------------------------------------------
    // Gitignore tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_find_path_respects_gitignore() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        // build.log matches *.log in .gitignore
        let args = FindPathToolArgs {
            glob: "**/*".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(!result.contains("build.log"));
        assert!(!result.contains("target/"));
    }

    #[tokio::test]
    async fn test_find_path_respects_nested_gitignore() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        // Create a nested gitignore
        fs::create_dir_all(dir.path().join("docs")).unwrap();
        fs::write(dir.path().join("docs/.gitignore"), "*.draft\n").unwrap();
        fs::write(dir.path().join("docs/notes.draft"), "draft content").unwrap();
        fs::write(dir.path().join("docs/public.md"), "public content").unwrap();

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "docs/**".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("docs/public.md"));
        assert!(!result.contains("notes.draft"));
    }

    // -----------------------------------------------------------------------
    // Pagination tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_find_path_pagination_default_limit() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        // Create 60 files
        for i in 1..=60 {
            fs::write(
                dir.path().join(format!("file_{:03}.txt", i)),
                format!("content {i}"),
            )
            .unwrap();
        }

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.txt".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Found 60 total matches"));
        assert!(result.contains("Showing results 1-50"));
        assert!(result.contains("10 more available"));
        // Count returned paths
        let path_lines: Vec<&str> = result.lines().filter(|l| l.ends_with(".txt")).collect();
        assert_eq!(path_lines.len(), 50);
    }

    #[tokio::test]
    async fn test_find_path_pagination_with_offset() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        for i in 1..=60 {
            fs::write(
                dir.path().join(format!("file_{:03}.txt", i)),
                format!("content {i}"),
            )
            .unwrap();
        }

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.txt".to_string(),
            offset: Some(50),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Showing results 51-60"));
        let path_lines: Vec<&str> = result.lines().filter(|l| l.ends_with(".txt")).collect();
        assert_eq!(path_lines.len(), 10);
    }

    #[tokio::test]
    async fn test_find_path_pagination_offset_beyond_results() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("one.txt"), "content").unwrap();

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.txt".to_string(),
            offset: Some(999),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("beyond the last result"));
    }

    // -----------------------------------------------------------------------
    // Error and edge case tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_find_path_invalid_glob() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "[invalid".to_string(),
            offset: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            FindPathToolError::InvalidGlob { .. }
        ));
    }

    #[tokio::test]
    async fn test_find_path_skips_git_directory() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
        fs::write(dir.path().join(".git/objects/abc123"), "git object").unwrap();
        fs::write(dir.path().join("visible.rs"), "fn visible()\n").unwrap();

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("visible.rs"));
        assert!(!result.contains(".git"));
    }

    #[tokio::test]
    async fn test_find_path_empty_project() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.rs".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No files found matching pattern"));
    }

    #[tokio::test]
    async fn test_find_path_relative_paths() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.rs".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // All paths should be relative (not start with /)
        for line in result.lines() {
            if line.ends_with(".rs") {
                assert!(!line.starts_with('/'), "Path should be relative: {}", line);
            }
        }
    }

    #[tokio::test]
    async fn test_find_path_includes_total_count() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "**/*.rs".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Found"));
        assert!(result.contains("total matches"));
    }

    #[tokio::test]
    async fn test_find_path_no_match_hint_for_non_recursive() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        // Use *.rs which only matches root — no .rs files in root
        let args = FindPathToolArgs {
            glob: "*.py".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No files found"));
        assert!(result.contains("Hint"));
        assert!(result.contains("**/"));
    }

    #[tokio::test]
    async fn test_find_path_cargo_wildcard() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = FindPathTool::new(dir.path().to_path_buf());
        let args = FindPathToolArgs {
            glob: "Cargo.*".to_string(),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Cargo.toml"));
        assert!(result.contains("Cargo.lock"));
    }
}
