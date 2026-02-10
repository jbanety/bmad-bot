//! GrepTool — search file contents in the project using regex patterns.
//!
//! This tool provides regex-based content search across project files with:
//!
//! - **.gitignore-aware** file traversal via the `ignore` crate
//! - **Glob filtering** via `include_pattern` to limit search to specific file types
//! - **Context lines** around each match (configurable, default 2)
//! - **Pagination** with offset-based navigation (20 matches per page)
//!
//! Security: searches are bounded to the project root. The `.git` directory is always excluded.

use globset::Glob;
use ignore::WalkBuilder;
use regex::Regex;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

/// Default number of context lines before/after each match.
const DEFAULT_CONTEXT_LINES: u32 = 2;

/// Maximum number of matches returned per page.
const PAGE_SIZE: usize = 20;

/// GrepTool — search file contents in the project using regex patterns.
#[derive(Debug, Serialize, Deserialize)]
pub struct GrepTool {
    /// Absolute path to the project root — all searches are bounded to this directory.
    project_root: PathBuf,
}

/// Arguments passed by the LLM agent when calling the `grep` tool.
#[derive(Debug, Deserialize)]
pub struct GrepToolArgs {
    /// Regex pattern to search for (Rust `regex` crate syntax).
    pub regex: String,
    /// Optional glob filter to limit which files are searched (e.g., `"**/*.rs"`).
    /// Use `**/*.ext` for recursive matching — `*.ext` only matches the root directory.
    pub include_pattern: Option<String>,
    /// Number of context lines before/after each match. Default: 2. Set to 0 for no context.
    pub context_lines: Option<u32>,
    /// Pagination offset — number of matches to skip before collecting results. Default: 0.
    pub offset: Option<u32>,
}

/// Errors from the `grep` tool.
#[derive(Debug, thiserror::Error)]
pub enum GrepToolError {
    /// The regex pattern is invalid.
    #[error("Invalid regex pattern '{pattern}': {reason}")]
    InvalidRegex {
        /// The pattern that failed to compile.
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

    /// An I/O error occurred while reading a file.
    #[error("I/O error for '{path}': {reason}")]
    IoError {
        /// The path that caused the error.
        path: String,
        /// Description of the error.
        reason: String,
    },

    /// The include_pattern glob is invalid.
    #[error("Invalid glob pattern '{pattern}': {reason}")]
    InvalidGlob {
        /// The glob pattern that failed to compile.
        pattern: String,
        /// Description of the error.
        reason: String,
    },
}

/// A single grep match with file path, line number, and content.
#[derive(Debug)]
struct GrepMatch {
    /// Relative file path from project root.
    path: String,
    /// 1-indexed line number of the match.
    line_number: usize,
}

/// A block of context lines around one or more matches in the same file.
#[derive(Debug)]
struct ContextBlock {
    /// Relative file path from project root.
    path: String,
    /// Start line (1-indexed, inclusive) of the context block.
    start_line: usize,
    /// End line (1-indexed, inclusive) of the context block.
    end_line: usize,
    /// Lines in this block (tuples of 1-indexed line number and content).
    lines: Vec<(usize, String)>,
    /// Which line numbers in this block are actual matches (not just context).
    match_lines: Vec<usize>,
}

impl GrepTool {
    /// Create a new `GrepTool` bounded to the given project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Validate and canonicalize the project root.
    fn validate_project_root(&self) -> Result<PathBuf, GrepToolError> {
        self.project_root
            .canonicalize()
            .map_err(|_| GrepToolError::PathDenied {
                path: self.project_root.display().to_string(),
                reason: "Cannot resolve project root".to_string(),
            })
    }

    /// Execute the grep search synchronously (intended for use inside `spawn_blocking`).
    fn search_sync(
        project_root: PathBuf,
        regex_pattern: &str,
        include_pattern: Option<&str>,
        context_lines: u32,
        offset: u32,
    ) -> Result<String, GrepToolError> {
        // Compile regex
        let re = Regex::new(regex_pattern).map_err(|e| GrepToolError::InvalidRegex {
            pattern: regex_pattern.to_string(),
            reason: e.to_string(),
        })?;

        // Compile include glob if provided
        let glob_matcher = if let Some(pattern) = include_pattern {
            Some(
                Glob::new(pattern)
                    .map_err(|e| GrepToolError::InvalidGlob {
                        pattern: pattern.to_string(),
                        reason: e.to_string(),
                    })?
                    .compile_matcher(),
            )
        } else {
            None
        };

        // Build directory walker
        let walker = WalkBuilder::new(&project_root)
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .filter_entry(|entry| {
                // Skip .git directory
                !(entry.file_type().is_some_and(|ft| ft.is_dir()) && entry.file_name() == ".git")
            })
            .build();

        let mut all_matches: Vec<GrepMatch> = Vec::new();
        let mut files_searched: usize = 0;

        // Collect all file paths and their matches
        // We need file content for context, so we store per-file match info
        struct FileMatches {
            path: String,
            lines: Vec<String>,
            match_line_numbers: Vec<usize>,
        }

        let mut file_matches_list: Vec<FileMatches> = Vec::new();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Only process files
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let abs_path = entry.path();

            // Compute relative path
            let rel_path = match abs_path.strip_prefix(&project_root) {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            // Apply include_pattern filter
            if let Some(ref matcher) = glob_matcher {
                if !matcher.is_match(&rel_path) {
                    continue;
                }
            }

            files_searched += 1;

            // Read file content — skip binary/non-UTF-8 silently
            let content = match std::fs::read_to_string(abs_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let file_lines: Vec<String> = content.split('\n').map(|s| s.to_string()).collect();
            let mut match_line_numbers: Vec<usize> = Vec::new();

            for (idx, line) in file_lines.iter().enumerate() {
                if re.is_match(line) {
                    let line_num = idx + 1;
                    match_line_numbers.push(line_num);
                    all_matches.push(GrepMatch {
                        path: rel_path.clone(),
                        line_number: line_num,
                    });
                }
            }

            if !match_line_numbers.is_empty() {
                file_matches_list.push(FileMatches {
                    path: rel_path,
                    lines: file_lines,
                    match_line_numbers,
                });
            }
        }

        let total_matches = all_matches.len();

        if total_matches == 0 {
            return Ok(format!(
                "No matches found for pattern '{}' (searched {} files).",
                regex_pattern, files_searched
            ));
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
                "Found {} total matches. Showing matches 1-{}.\n",
                total_matches, total_matches
            ));
        } else {
            let remaining = total_matches.saturating_sub(page_end);
            if remaining > 0 {
                output.push_str(&format!(
                    "Found {} total matches. Showing matches {}-{} ({} more available, use offset: {} to see next page).\n",
                    total_matches,
                    offset + 1,
                    page_end,
                    remaining,
                    page_end
                ));
            } else {
                output.push_str(&format!(
                    "Found {} total matches. Showing matches {}-{}.\n",
                    total_matches,
                    offset + 1,
                    page_end
                ));
            }
        }

        // Get the paginated slice of matches
        let paginated = &all_matches[offset..page_end];

        if context_lines == 0 {
            // No context — simple format: path:line_number:content
            output.push('\n');
            for m in paginated {
                // Find the file and line content
                if let Some(fm) = file_matches_list.iter().find(|f| f.path == m.path) {
                    let line_idx = m.line_number - 1;
                    if line_idx < fm.lines.len() {
                        output.push_str(&format!(
                            "{}:{}:{}\n",
                            m.path, m.line_number, fm.lines[line_idx]
                        ));
                    }
                }
            }
        } else {
            // With context — group by file, merge overlapping ranges
            output.push('\n');

            let ctx = context_lines as usize;
            let mut blocks: Vec<ContextBlock> = Vec::new();

            for m in paginated {
                if let Some(fm) = file_matches_list.iter().find(|f| f.path == m.path) {
                    let total_file_lines = fm.lines.len();
                    let start = if m.line_number > ctx {
                        m.line_number - ctx
                    } else {
                        1
                    };
                    let end = (m.line_number + ctx).min(total_file_lines);

                    // Try to merge with the last block if same file and overlapping
                    let merged = if let Some(last) = blocks.last_mut() {
                        if last.path == m.path && start <= last.end_line + 1 {
                            // Extend the block
                            if end > last.end_line {
                                // Add new lines
                                for ln in (last.end_line + 1)..=end {
                                    let idx = ln - 1;
                                    if idx < fm.lines.len() {
                                        last.lines.push((ln, fm.lines[idx].clone()));
                                    }
                                }
                                last.end_line = end;
                            }
                            last.match_lines.push(m.line_number);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if !merged {
                        let mut lines = Vec::new();
                        for ln in start..=end {
                            let idx = ln - 1;
                            if idx < fm.lines.len() {
                                lines.push((ln, fm.lines[idx].clone()));
                            }
                        }
                        blocks.push(ContextBlock {
                            path: m.path.clone(),
                            start_line: start,
                            end_line: end,
                            lines,
                            match_lines: vec![m.line_number],
                        });
                    }
                }
            }

            // Format context blocks
            for (i, block) in blocks.iter().enumerate() {
                if i > 0 {
                    output.push_str("--\n");
                }
                output.push_str(&format!("{}:\n", block.path));

                // Determine line number width for formatting
                let max_ln = block.end_line;
                let width = max_ln.to_string().len();

                for (ln, content) in &block.lines {
                    let separator = if block.match_lines.contains(ln) {
                        ':'
                    } else {
                        '|'
                    };
                    output.push_str(&format!(
                        "{:>width$}{} {}\n",
                        ln,
                        separator,
                        content,
                        width = width
                    ));
                }
            }
        }

        Ok(output)
    }
}

impl Tool for GrepTool {
    const NAME: &'static str = "grep";
    type Error = GrepToolError;
    type Args = GrepToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "grep".to_string(),
            description: "Search file contents in the project using a regex pattern. \
                Returns matching lines with file paths and line numbers.\n\n\
                **Usage:** Provide a `regex` pattern (Rust regex syntax). Use `include_pattern` \
                to limit search to specific file types (e.g., `\"**/*.rs\"` for Rust files only).\n\n\
                **Results:** Each match shows `file_path:line_number:content`. Results are \
                paginated (20 per page). Use `offset` to get subsequent pages.\n\n\
                **Tips:**\n\
                - Use `\\b` for word boundaries: `\\bfn\\b` matches \"fn\" but not \"pfn\"\n\
                - Use `(?i)` prefix for case-insensitive: `(?i)error`\n\
                - Combine with `read_file` to see surrounding context after finding a match\n\
                - Files matching `.gitignore` patterns are automatically excluded\n\n\
                **Prefer `grep` over `find_path` when** you need to find where a symbol, \
                string, or pattern is used in code.\n\
                **Prefer `find_path` over `grep` when** you need to find files by name or extension."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "regex": {
                        "type": "string",
                        "description": "Regex pattern to search for (Rust regex crate syntax)"
                    },
                    "include_pattern": {
                        "type": "string",
                        "description": "Glob filter to limit which files are searched. Use **/*.rs to match all Rust files recursively — *.rs only matches the root directory."
                    },
                    "context_lines": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 2,
                        "description": "Number of context lines before/after each match. Default: 2. Set to 0 for compact output."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 0,
                        "description": "Pagination offset — number of matches to skip. Use to get subsequent pages of results."
                    }
                },
                "required": ["regex"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::info!(
            action = "grep",
            pattern = %args.regex,
            include_pattern = ?args.include_pattern,
            context_lines = ?args.context_lines,
            offset = ?args.offset,
            "Searching file contents"
        );

        let canonical_root = self.validate_project_root()?;
        let regex_str = args.regex.clone();
        let include = args.include_pattern.clone();
        let ctx = args.context_lines.unwrap_or(DEFAULT_CONTEXT_LINES);
        let offset = args.offset.unwrap_or(0);

        let result = tokio::task::spawn_blocking(move || {
            Self::search_sync(canonical_root, &regex_str, include.as_deref(), ctx, offset)
        })
        .await
        .map_err(|e| GrepToolError::IoError {
            path: String::new(),
            reason: format!("Task join error: {e}"),
        })??;

        tracing::info!(
            action = "grep",
            pattern = %args.regex,
            result_bytes = result.len(),
            "Search complete"
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
        fs::write(
            dir.join("src/main.rs"),
            "fn main() {\n    println!(\"hello\");\n    let x = 42;\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            "pub mod tools;\npub mod config;\n\nfn helper() {\n    // helper function\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/tools/mod.rs"),
            "pub mod grep;\npub mod find_path;\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/tools/grep.rs"),
            "pub struct GrepTool {\n    project_root: PathBuf,\n}\n\nimpl GrepTool {\n    pub fn new() -> Self {\n        todo!()\n    }\n}\n",
        )
        .unwrap();

        // Non-Rust files
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
        fs::write(
            dir.join("README.md"),
            "# Test Project\nSome content.\nfn not_real_code() {}\n",
        )
        .unwrap();

        // .gitignore and ignored files
        fs::write(dir.join(".gitignore"), "target/\n*.log\n").unwrap();
        fs::create_dir_all(dir.join("target/debug")).unwrap();
        fs::write(dir.join("target/debug/output.log"), "debug output fn match").unwrap();
        fs::write(dir.join("build.log"), "build log fn match").unwrap();
    }

    // -----------------------------------------------------------------------
    // Tool definition tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_grep_definition_name() {
        assert_eq!(GrepTool::NAME, "grep");
        let dir = TempDir::new().unwrap();
        let tool = GrepTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "grep");
    }

    #[tokio::test]
    async fn test_grep_definition_has_detailed_description() {
        let dir = TempDir::new().unwrap();
        let tool = GrepTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert!(!def.description.is_empty());
        assert!(def.description.contains("regex"));
        assert!(def.description.contains("include_pattern"));
        assert!(def.description.contains("gitignore"));
        assert!(def.description.contains("find_path"));
    }

    #[test]
    fn test_grep_serializable() {
        let tool = GrepTool::new(PathBuf::from("/tmp/test"));
        let json = serde_json::to_string(&tool).expect("Should serialize");
        let _deserialized: GrepTool = serde_json::from_str(&json).expect("Should deserialize");
    }

    #[test]
    fn test_grep_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<GrepToolError>();
    }

    // -----------------------------------------------------------------------
    // Basic search tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_grep_basic_match() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "fn main".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("fn main"));
    }

    #[tokio::test]
    async fn test_grep_regex_match() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: r"fn\s+\w+".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // Should find fn main, fn helper, fn new, fn not_real_code
        assert!(result.contains("fn main"));
        assert!(result.contains("fn helper"));
    }

    #[tokio::test]
    async fn test_grep_case_sensitive_default() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        // Search for "FN" (uppercase) — should NOT match "fn" (lowercase)
        let args = GrepToolArgs {
            regex: "FN".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No matches found"));
    }

    #[tokio::test]
    async fn test_grep_case_insensitive_regex() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "(?i)FN MAIN".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("fn main"));
    }

    #[tokio::test]
    async fn test_grep_multiple_matches_same_file() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "pub mod".to_string(),
            include_pattern: Some("**/lib.rs".to_string()),
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // lib.rs has "pub mod tools;" and "pub mod config;"
        assert!(result.contains("pub mod tools"));
        assert!(result.contains("pub mod config"));
    }

    #[tokio::test]
    async fn test_grep_matches_across_files() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "pub mod".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // Should find pub mod in lib.rs and tools/mod.rs
        assert!(result.contains("lib.rs"));
        assert!(result.contains("tools/mod.rs"));
    }

    #[tokio::test]
    async fn test_grep_no_matches() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "nonexistent_function_xyz".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No matches found"));
        assert!(result.contains("searched"));
    }

    // -----------------------------------------------------------------------
    // Include pattern tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_grep_include_pattern_filters_files() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        // Search for "fn" only in .rs files — should not find it in README.md
        let args = GrepToolArgs {
            regex: "fn ".to_string(),
            include_pattern: Some("**/*.rs".to_string()),
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains(".rs"));
        assert!(!result.contains("README.md"));
    }

    #[tokio::test]
    async fn test_grep_include_pattern_no_matches() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "fn main".to_string(),
            include_pattern: Some("**/*.py".to_string()),
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No matches found"));
    }

    #[tokio::test]
    async fn test_grep_invalid_include_pattern() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "fn".to_string(),
            include_pattern: Some("[invalid".to_string()),
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            GrepToolError::InvalidGlob { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // Gitignore tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_grep_respects_gitignore() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        // "build log fn match" is in build.log which matches *.log in .gitignore
        let args = GrepToolArgs {
            regex: "build log".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No matches found") || !result.contains("build.log"));
    }

    #[tokio::test]
    async fn test_grep_respects_nested_gitignore() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        // Create a nested gitignore
        fs::create_dir_all(dir.path().join("docs")).unwrap();
        fs::write(dir.path().join("docs/.gitignore"), "*.draft\n").unwrap();
        fs::write(dir.path().join("docs/notes.draft"), "fn secret_draft").unwrap();
        fs::write(dir.path().join("docs/public.md"), "fn public_stuff").unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "fn ".to_string(),
            include_pattern: Some("docs/**".to_string()),
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // Should find public.md but not notes.draft
        assert!(!result.contains("notes.draft"));
    }

    // -----------------------------------------------------------------------
    // Context lines tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_grep_context_lines_default() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "println".to_string(),
            include_pattern: Some("**/main.rs".to_string()),
            context_lines: None, // default: 2
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // println is on line 2, context should show surrounding lines
        assert!(result.contains("fn main"));
        assert!(result.contains("println"));
    }

    #[tokio::test]
    async fn test_grep_context_lines_custom() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        let content: String = (1..=20)
            .map(|i| {
                if i == 10 {
                    "MATCH_LINE".to_string()
                } else {
                    format!("line {i}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(dir.path().join("test.txt"), &content).unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "MATCH_LINE".to_string(),
            include_pattern: None,
            context_lines: Some(3),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // Match on line 10, context 3 → lines 7-13
        assert!(result.contains("line 7"));
        assert!(result.contains("MATCH_LINE"));
        assert!(result.contains("line 13"));
    }

    #[tokio::test]
    async fn test_grep_context_lines_zero() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "fn main".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // Format should be path:line:content — no context block separators
        assert!(result.contains("src/main.rs:1:fn main"));
        assert!(!result.contains("--"));
    }

    #[tokio::test]
    async fn test_grep_context_lines_at_file_boundary() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("short.txt"), "MATCH\nline2\nline3\n").unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "MATCH".to_string(),
            include_pattern: None,
            context_lines: Some(5),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // MATCH is on line 1, context clamped to file boundaries
        assert!(result.contains("MATCH"));
        // Should not error or include nonexistent lines
    }

    // -----------------------------------------------------------------------
    // Pagination tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_grep_pagination_default_limit() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        // Create a file with 30 matching lines
        let content: String = (1..=30)
            .map(|i| format!("match_target line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(dir.path().join("many.txt"), &content).unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "match_target".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Found 30 total matches"));
        assert!(result.contains("Showing matches 1-20"));
        assert!(result.contains("10 more available"));
        // Count actual match lines in output (lines matching "many.txt:")
        let match_lines: Vec<&str> = result
            .lines()
            .filter(|l| l.starts_with("many.txt:"))
            .collect();
        assert_eq!(match_lines.len(), 20);
    }

    #[tokio::test]
    async fn test_grep_pagination_with_offset() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        let content: String = (1..=30)
            .map(|i| format!("match_target line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(dir.path().join("many.txt"), &content).unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "match_target".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: Some(20),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Showing matches 21-30"));
        // Should have 10 results (lines 21-30)
        let match_lines: Vec<&str> = result
            .lines()
            .filter(|l| l.starts_with("many.txt:"))
            .collect();
        assert_eq!(match_lines.len(), 10);
    }

    #[tokio::test]
    async fn test_grep_pagination_offset_beyond_results() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("one.txt"), "match_me\n").unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "match_me".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: Some(999),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("beyond the last result"));
    }

    // -----------------------------------------------------------------------
    // Error and edge case tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_grep_invalid_regex() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "[invalid".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            GrepToolError::InvalidRegex { .. }
        ));
    }

    #[tokio::test]
    async fn test_grep_skips_binary_files() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        // Write a binary file
        fs::write(dir.path().join("binary.bin"), &[0xFF, 0xFE, 0x00, 0x80]).unwrap();
        // Write a text file with a match
        fs::write(dir.path().join("text.txt"), "search_target\n").unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "search_target".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // Should find text.txt match, not error on binary
        assert!(result.contains("text.txt"));
        assert!(!result.contains("binary.bin"));
    }

    #[tokio::test]
    async fn test_grep_empty_project() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "anything".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No matches found"));
    }

    #[tokio::test]
    async fn test_grep_skips_git_directory() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
        fs::write(
            dir.path().join(".git/objects/abc123"),
            "fn secret_git_object",
        )
        .unwrap();
        fs::write(dir.path().join("visible.rs"), "fn visible_function\n").unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "fn ".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("visible.rs"));
        assert!(!result.contains(".git"));
    }

    #[tokio::test]
    async fn test_grep_line_numbers_are_one_indexed() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("nums.txt"), "first\nsecond\nthird\n").unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "second".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("nums.txt:2:second"));
    }

    #[tokio::test]
    async fn test_grep_output_format() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("fmt.rs"), "line1\nfn target()\nline3\n").unwrap();

        let tool = GrepTool::new(dir.path().to_path_buf());
        let args = GrepToolArgs {
            regex: "fn target".to_string(),
            include_pattern: None,
            context_lines: Some(0),
            offset: None,
        };
        let result = tool.call(args).await.unwrap();
        // Format: path:line_number:content
        assert!(result.contains("fmt.rs:2:fn target()"));
    }
}
