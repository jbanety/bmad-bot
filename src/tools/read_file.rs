//! ReadFileTool — read files with optional line ranges and automatic outline mode for large files.
//!
//! This tool provides three reading modes:
//!
//! - **Full read** — files ≤ 300 lines return complete content with line numbers
//! - **Outline mode** — files > 300 lines return a structural outline (symbol names + line numbers)
//! - **Partial read** — specify `start_line`/`end_line` to read a specific range (any file size)
//!
//! Security: all paths are validated against the project root boundary via `canonicalize()` +
//! `starts_with()`. Paths resolving outside the project root are rejected.

use regex::Regex;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Threshold: files with more than this many lines trigger outline mode (when no range is given).
const OUTLINE_THRESHOLD: usize = 300;

// ---------------------------------------------------------------------------
// Static regex patterns — compiled once per process via LazyLock
// ---------------------------------------------------------------------------

// Rust patterns
static RE_RUST_FN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(pub(\([^)]*\))?\s+)?(async\s+)?fn\s+\w+").unwrap());
static RE_RUST_STRUCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(pub(\([^)]*\))?\s+)?struct\s+\w+").unwrap());
static RE_RUST_ENUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(pub(\([^)]*\))?\s+)?enum\s+\w+").unwrap());
static RE_RUST_IMPL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*impl(<[^>]*>)?\s+\w+").unwrap());
static RE_RUST_MOD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(pub(\([^)]*\))?\s+)?mod\s+\w+").unwrap());
static RE_RUST_TRAIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(pub(\([^)]*\))?\s+)?trait\s+\w+").unwrap());
static RE_RUST_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(pub(\([^)]*\))?\s+)?type\s+\w+").unwrap());
static RE_RUST_CONST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(pub(\([^)]*\))?\s+)?const\s+\w+").unwrap());
static RE_RUST_STATIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(pub(\([^)]*\))?\s+)?static\s+\w+").unwrap());
static RE_RUST_CFG_TEST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*#\[cfg\(test\)\]").unwrap());

// Markdown patterns
static RE_MD_HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#{1,6}\s+.+").unwrap());

// Generic fallback patterns
static RE_GENERIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(pub(lic)?|private|protected)?\s*(static\s+)?(async\s+)?(fn|func|function|def|class|interface|struct|enum|mod|module|trait|type|const|let|var)\s+\w+",
    )
    .unwrap()
});

/// ReadFileTool — read files with optional line ranges and automatic outline mode for large files.
#[derive(Debug, Serialize, Deserialize)]
pub struct ReadFileTool {
    /// Absolute path to the project root — all reads are bounded to this directory.
    project_root: PathBuf,
}

/// Arguments passed by the LLM agent when calling the `read_file` tool.
#[derive(Debug, Deserialize)]
pub struct ReadFileToolArgs {
    /// Relative path from the project root to the file to read.
    pub path: String,
    /// Optional 1-indexed inclusive start line. If provided, enables partial read mode.
    pub start_line: Option<u32>,
    /// Optional 1-indexed inclusive end line. If provided, enables partial read mode.
    pub end_line: Option<u32>,
}

/// Errors from the `read_file` tool.
#[derive(Debug, thiserror::Error)]
pub enum ReadFileToolError {
    /// The requested file does not exist.
    #[error("File not found: {path}")]
    NotFound {
        /// The path that was requested.
        path: String,
    },

    /// The requested path resolves outside the project root boundary.
    #[error("Access denied for '{path}': {reason}")]
    PathDenied {
        /// The path that was requested.
        path: String,
        /// Reason the path was denied.
        reason: String,
    },

    /// An I/O or encoding error occurred while reading the file.
    #[error("Read failed for '{path}': {reason}")]
    ReadFailed {
        /// The path that was requested.
        path: String,
        /// Description of the error.
        reason: String,
    },

    /// The requested path points to a directory, not a file.
    #[error("Path is a directory, not a file: {path}")]
    IsDirectory {
        /// The path that was requested.
        path: String,
    },
}

impl ReadFileTool {
    /// Create a new `ReadFileTool` bounded to the given project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Validate the requested path is within the project root and exists.
    ///
    /// Resolves the path via `canonicalize()` and checks that it starts with
    /// `self.project_root`. This prevents directory traversal attacks.
    fn validate_path(&self, requested: &str) -> Result<PathBuf, ReadFileToolError> {
        let full_path = self.project_root.join(requested);

        // Canonicalize resolves symlinks and `..` components.
        // If the file doesn't exist, canonicalize fails — treat as NotFound.
        let canonical = full_path
            .canonicalize()
            .map_err(|_| ReadFileToolError::NotFound {
                path: requested.to_string(),
            })?;

        // Canonicalize the project root too (in case it contains symlinks).
        let canonical_root =
            self.project_root
                .canonicalize()
                .map_err(|_| ReadFileToolError::PathDenied {
                    path: requested.to_string(),
                    reason: "Cannot resolve project root".to_string(),
                })?;

        if !canonical.starts_with(&canonical_root) {
            return Err(ReadFileToolError::PathDenied {
                path: requested.to_string(),
                reason: "Path is outside project root".to_string(),
            });
        }

        Ok(canonical)
    }

    /// Format lines with right-aligned line numbers.
    ///
    /// `start_offset` is the 0-based index into the original file's lines
    /// (used so partial reads show correct line numbers).
    /// `total_lines` is the total number of lines in the file (for padding width).
    fn format_with_line_numbers(lines: &[&str], start_offset: usize, total_lines: usize) -> String {
        let width = total_lines.max(1).to_string().len();
        let mut result = String::new();
        for (i, line) in lines.iter().enumerate() {
            let line_num = start_offset + i + 1;
            if i > 0 {
                result.push('\n');
            }
            result.push_str(&format!("{line_num:>width$} | {line}"));
        }
        result
    }

    /// Read a file fully, partially, or in outline mode depending on args and file size.
    async fn read_full_or_range(
        &self,
        path: &Path,
        args: &ReadFileToolArgs,
    ) -> Result<String, ReadFileToolError> {
        let content = match tokio::fs::read(path).await {
            Ok(bytes) => String::from_utf8(bytes).map_err(|_| ReadFileToolError::ReadFailed {
                path: args.path.clone(),
                reason: "File appears to be binary or non-UTF-8 encoded".to_string(),
            })?,
            Err(e) => {
                return Err(ReadFileToolError::ReadFailed {
                    path: args.path.clone(),
                    reason: e.to_string(),
                });
            }
        };

        let all_lines: Vec<&str> = content.split('\n').collect();
        let total_lines = all_lines.len();
        let has_range = args.start_line.is_some() || args.end_line.is_some();

        if has_range {
            // Partial read mode — extract the requested range
            let mut start = args.start_line.unwrap_or(1);
            let mut end = args.end_line.unwrap_or(total_lines as u32);

            // Clamp start_line of 0 to 1
            if start == 0 {
                start = 1;
            }

            // Clamp end to total line count
            if end > total_lines as u32 {
                end = total_lines as u32;
            }

            // If start > end after clamping, return empty
            if start > end {
                return Ok(String::new());
            }

            let start_idx = (start as usize) - 1;
            let end_idx = end as usize;
            let slice = &all_lines[start_idx..end_idx];
            Ok(Self::format_with_line_numbers(
                slice,
                start_idx,
                total_lines,
            ))
        } else if total_lines <= OUTLINE_THRESHOLD {
            // Full read — small file
            Ok(Self::format_with_line_numbers(&all_lines, 0, total_lines))
        } else {
            // Outline mode — large file
            Ok(Self::extract_outline(&content, &args.path))
        }
    }

    /// Extract a structural outline from file content using regex-based symbol detection.
    ///
    /// Uses the file extension to select the appropriate pattern set:
    /// - `.rs` → Rust patterns (functions, structs, enums, impls, mods, traits, etc.)
    /// - `.md` → Markdown patterns (headings)
    /// - Other → generic multi-language fallback
    fn extract_outline(content: &str, file_path: &str) -> String {
        let lines: Vec<&str> = content.split('\n').collect();
        let total_lines = lines.len();
        let extension = Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let mut symbols: Vec<String> = Vec::new();

        match extension {
            "rs" => {
                // Track indentation for nesting detection
                let mut last_toplevel_indent: Option<usize> = None;

                for (i, line) in lines.iter().enumerate() {
                    let line_num = i + 1;
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    let leading_spaces = line.len() - line.trim_start().len();

                    // Check top-level constructs first
                    let is_toplevel = RE_RUST_IMPL.is_match(line)
                        || RE_RUST_STRUCT.is_match(line)
                        || RE_RUST_ENUM.is_match(line)
                        || RE_RUST_MOD.is_match(line)
                        || RE_RUST_TRAIT.is_match(line)
                        || RE_RUST_CFG_TEST.is_match(line);

                    let is_member = RE_RUST_FN.is_match(line)
                        || RE_RUST_TYPE.is_match(line)
                        || RE_RUST_CONST.is_match(line)
                        || RE_RUST_STATIC.is_match(line);

                    if is_toplevel {
                        last_toplevel_indent = Some(leading_spaces);
                        // Extract the symbol signature (trimmed, up to opening brace or end)
                        let sig = extract_signature(trimmed);
                        symbols.push(format!("{sig} [L{line_num}]"));
                    } else if is_member {
                        let nested = if let Some(tl_indent) = last_toplevel_indent {
                            leading_spaces > tl_indent
                        } else {
                            false
                        };

                        let sig = extract_signature(trimmed);
                        if nested {
                            symbols.push(format!("  {sig} [L{line_num}]"));
                        } else {
                            symbols.push(format!("{sig} [L{line_num}]"));
                        }
                    }
                }
            }
            "md" => {
                for (i, line) in lines.iter().enumerate() {
                    let line_num = i + 1;
                    if RE_MD_HEADING.is_match(line) {
                        symbols.push(format!("{} [L{line_num}]", line.trim()));
                    }
                }
            }
            _ => {
                for (i, line) in lines.iter().enumerate() {
                    let line_num = i + 1;
                    if RE_GENERIC.is_match(line) {
                        let sig = extract_signature(line.trim());
                        symbols.push(format!("{sig} [L{line_num}]"));
                    }
                }
            }
        }

        if symbols.is_empty() {
            format!(
                "No structural symbols found in {file_path} ({total_lines} lines).\n\
                 Use start_line and end_line to read specific sections."
            )
        } else {
            let mut output = format!("File outline for {file_path} ({total_lines} lines):\n\n");
            for sym in &symbols {
                output.push_str(sym);
                output.push('\n');
            }
            output.push_str("\nUse start_line and end_line to read specific sections.");
            output
        }
    }
}

/// Extract a clean signature from a source line — strip trailing `{`, `,`, etc.
fn extract_signature(line: &str) -> &str {
    let sig = line.trim_end();
    // Strip trailing opening brace and whitespace
    let sig = sig.strip_suffix('{').unwrap_or(sig).trim_end();
    // Strip trailing comma
    let sig = sig.strip_suffix(',').unwrap_or(sig).trim_end();
    sig
}

impl Tool for ReadFileTool {
    const NAME: &'static str = "read_file";
    type Error = ReadFileToolError;
    type Args = ReadFileToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file from the project. Returns file content with line numbers.\n\n\
                **Modes:**\n\
                - **Full read:** Files ≤ 300 lines return complete content with line numbers.\n\
                - **Outline mode:** Files > 300 lines return a structural outline \
                (function/struct/enum/impl/mod declarations with line numbers) instead of full content. \
                Use the line numbers from the outline to read specific sections with start_line/end_line.\n\
                - **Partial read:** Specify start_line and/or end_line (1-indexed, inclusive) to read \
                a specific range. This works on files of any size and always returns content (never outline).\n\n\
                **Workflow for large files:** Call without line range → get outline → identify the section \
                you need → call again with start_line/end_line.\n\n\
                Line numbers are always prepended to output lines. Out-of-range values are clamped \
                to file boundaries without error."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path from the project root to the file to read"
                    },
                    "start_line": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Optional 1-indexed inclusive start line for partial read. When provided, returns content (not outline) regardless of file size."
                    },
                    "end_line": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Optional 1-indexed inclusive end line for partial read. When provided, returns content (not outline) regardless of file size."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::info!(
            action = "read_file",
            path = %args.path,
            start_line = ?args.start_line,
            end_line = ?args.end_line,
            "Reading file"
        );

        let validated_path = self.validate_path(&args.path)?;

        // Check it's a file, not a directory
        if validated_path.is_dir() {
            return Err(ReadFileToolError::IsDirectory {
                path: args.path.clone(),
            });
        }

        let result = self.read_full_or_range(&validated_path, &args).await?;

        tracing::info!(
            action = "read_file",
            path = %args.path,
            bytes = result.len(),
            mode = if args.start_line.is_some() || args.end_line.is_some() {
                "partial"
            } else {
                "auto"
            },
            "File read complete"
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: create a file with numbered lines like "Line 1\nLine 2\n...".
    fn create_test_file(dir: &Path, name: &str, lines: usize) -> PathBuf {
        let path = dir.join(name);
        let content: String = (1..=lines)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, &content).unwrap();
        path
    }

    /// Helper: create a Rust-like source file with known symbols.
    fn create_rust_file(dir: &Path, name: &str, lines: usize) -> PathBuf {
        let path = dir.join(name);
        let mut content = String::new();
        // Write known symbols at specific positions, pad the rest with comments
        content.push_str("//! Module doc\n");
        content.push_str("\n");
        content.push_str("pub struct MyStruct {\n");
        content.push_str("    field: u32,\n");
        content.push_str("}\n");
        content.push_str("\n");
        content.push_str("pub enum MyEnum {\n");
        content.push_str("    A,\n");
        content.push_str("    B,\n");
        content.push_str("}\n");
        content.push_str("\n");
        content.push_str("impl MyStruct {\n");
        content.push_str("    pub fn new() -> Self {\n");
        content.push_str("        Self { field: 0 }\n");
        content.push_str("    }\n");
        content.push_str("\n");
        content.push_str("    pub async fn do_something(&self) -> u32 {\n");
        content.push_str("        self.field\n");
        content.push_str("    }\n");
        content.push_str("}\n");
        content.push_str("\n");
        content.push_str("pub trait MyTrait {\n");
        content.push_str("    fn required(&self);\n");
        content.push_str("}\n");
        content.push_str("\n");
        content.push_str("pub(crate) mod inner {\n");
        content.push_str("    pub fn inner_fn() {}\n");
        content.push_str("}\n");
        content.push_str("\n");
        content.push_str("#[cfg(test)]\n");
        content.push_str("mod tests {\n");
        content.push_str("    fn test_helper() {}\n");
        content.push_str("}\n");

        // Pad to requested line count
        let current_count = content.matches('\n').count() + 1;
        if lines > current_count {
            for i in current_count..lines {
                content.push_str(&format!("// filler line {i}\n"));
            }
        }

        fs::write(&path, &content).unwrap();
        path
    }

    // -----------------------------------------------------------------------
    // Tool definition tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_read_file_tool_definition_name() {
        assert_eq!(ReadFileTool::NAME, "read_file");
        let dir = TempDir::new().unwrap();
        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "read_file");
    }

    #[tokio::test]
    async fn test_read_file_tool_definition_has_detailed_description() {
        let dir = TempDir::new().unwrap();
        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        assert!(!def.description.is_empty());
        assert!(def.description.contains("Outline mode"));
        assert!(def.description.contains("Partial read"));
        assert!(def.description.contains("Full read"));
        assert!(def.description.contains("300 lines"));
        assert!(def.description.contains("start_line"));
        assert!(def.description.contains("end_line"));
    }

    #[tokio::test]
    async fn test_read_file_tool_definition_parameters() {
        let dir = TempDir::new().unwrap();
        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let def = tool.definition("test".to_string()).await;
        let params = &def.parameters;
        // path is required
        let required = params["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("path")));
        // start_line and end_line are NOT required
        assert!(!required.iter().any(|v| v.as_str() == Some("start_line")));
        assert!(!required.iter().any(|v| v.as_str() == Some("end_line")));
        // But they exist as properties
        assert!(params["properties"]["start_line"].is_object());
        assert!(params["properties"]["end_line"].is_object());
        // start_line and end_line are integers
        assert_eq!(params["properties"]["start_line"]["type"], "integer");
        assert_eq!(params["properties"]["end_line"]["type"], "integer");
    }

    // -----------------------------------------------------------------------
    // Args deserialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_file_tool_args_deserialize_minimal() {
        let json = r#"{"path": "src/main.rs"}"#;
        let args: ReadFileToolArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.path, "src/main.rs");
        assert!(args.start_line.is_none());
        assert!(args.end_line.is_none());
    }

    #[test]
    fn test_read_file_tool_args_deserialize_full() {
        let json = r#"{"path": "src/main.rs", "start_line": 10, "end_line": 50}"#;
        let args: ReadFileToolArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.path, "src/main.rs");
        assert_eq!(args.start_line, Some(10));
        assert_eq!(args.end_line, Some(50));
    }

    // -----------------------------------------------------------------------
    // Error type tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_file_tool_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ReadFileToolError>();
    }

    #[test]
    fn test_read_file_tool_error_display() {
        let err = ReadFileToolError::NotFound {
            path: "missing.rs".to_string(),
        };
        assert!(err.to_string().contains("missing.rs"));

        let err = ReadFileToolError::PathDenied {
            path: "../etc/passwd".to_string(),
            reason: "Path is outside project root".to_string(),
        };
        assert!(err.to_string().contains("../etc/passwd"));
        assert!(err.to_string().contains("outside project root"));

        let err = ReadFileToolError::ReadFailed {
            path: "bad.bin".to_string(),
            reason: "binary or non-UTF-8".to_string(),
        };
        assert!(err.to_string().contains("bad.bin"));
        assert!(err.to_string().contains("binary or non-UTF-8"));

        let err = ReadFileToolError::IsDirectory {
            path: "src".to_string(),
        };
        assert!(err.to_string().contains("src"));
        assert!(err.to_string().contains("directory"));
    }

    // -----------------------------------------------------------------------
    // Serializable test
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_file_tool_serializable() {
        let tool = ReadFileTool::new(PathBuf::from("/tmp/test"));
        let json = serde_json::to_string(&tool).expect("Should serialize");
        let _deserialized: ReadFileTool = serde_json::from_str(&json).expect("Should deserialize");
    }

    // -----------------------------------------------------------------------
    // Full read tests (small files)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_read_file_full_small_file() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "small.txt", 10);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "small.txt".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        // Should contain all 10 lines with line numbers
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 10"));
        // Line numbers are present
        assert!(result.contains(" 1 | Line 1"));
        assert!(result.contains("10 | Line 10"));
    }

    #[tokio::test]
    async fn test_read_file_line_numbers_format() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "numbered.txt", 5);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "numbered.txt".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        let lines: Vec<&str> = result.split('\n').collect();
        // 1-indexed, right-aligned
        assert_eq!(lines[0], "1 | Line 1");
        assert_eq!(lines[4], "5 | Line 5");
    }

    // -----------------------------------------------------------------------
    // Partial read tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_read_file_partial_start_and_end() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "partial.txt", 20);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "partial.txt".to_string(),
            start_line: Some(5),
            end_line: Some(10),
        };
        let result = tool.call(args).await.unwrap();
        let lines: Vec<&str> = result.split('\n').collect();
        assert_eq!(lines.len(), 6);
        assert!(lines[0].contains("Line 5"));
        assert!(lines[5].contains("Line 10"));
    }

    #[tokio::test]
    async fn test_read_file_partial_start_only() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "start_only.txt", 20);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "start_only.txt".to_string(),
            start_line: Some(15),
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Line 15"));
        assert!(result.contains("Line 20"));
        assert!(!result.contains("Line 14"));
    }

    #[tokio::test]
    async fn test_read_file_partial_end_only() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "end_only.txt", 20);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "end_only.txt".to_string(),
            start_line: None,
            end_line: Some(10),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 10"));
        assert!(!result.contains("Line 11"));
    }

    #[tokio::test]
    async fn test_read_file_partial_clamp_overflow() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "clamp.txt", 20);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "clamp.txt".to_string(),
            start_line: Some(15),
            end_line: Some(9999),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Line 15"));
        assert!(result.contains("Line 20"));
    }

    #[tokio::test]
    async fn test_read_file_partial_start_beyond_file() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "beyond.txt", 20);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "beyond.txt".to_string(),
            start_line: Some(9999),
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        // start_line (9999) > end_line (clamped to 20) → empty
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_read_file_partial_single_line() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "single.txt", 20);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "single.txt".to_string(),
            start_line: Some(5),
            end_line: Some(5),
        };
        let result = tool.call(args).await.unwrap();
        let lines: Vec<&str> = result.split('\n').collect();
        assert_eq!(lines.len(), 1);
        assert!(result.contains("Line 5"));
    }

    #[tokio::test]
    async fn test_read_file_partial_start_zero_clamps_to_one() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "zero.txt", 20);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "zero.txt".to_string(),
            start_line: Some(0),
            end_line: Some(3),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 3"));
        let lines: Vec<&str> = result.split('\n').collect();
        assert_eq!(lines.len(), 3);
    }

    #[tokio::test]
    async fn test_read_file_partial_start_after_end() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "inverted.txt", 20);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "inverted.txt".to_string(),
            start_line: Some(10),
            end_line: Some(5),
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // Outline mode tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_read_file_outline_large_file() {
        let dir = TempDir::new().unwrap();
        create_rust_file(dir.path(), "large.rs", 350);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "large.rs".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        // Should be outline, not full content
        assert!(result.contains("File outline for"));
        assert!(result.contains("Use start_line and end_line"));
        // Should NOT contain raw "Line X" content from filler
        assert!(!result.starts_with("1 |"));
    }

    #[tokio::test]
    async fn test_read_file_outline_contains_functions() {
        let dir = TempDir::new().unwrap();
        create_rust_file(dir.path(), "funcs.rs", 350);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "funcs.rs".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("pub fn new"));
        assert!(result.contains("pub async fn do_something"));
    }

    #[tokio::test]
    async fn test_read_file_outline_contains_structs_enums() {
        let dir = TempDir::new().unwrap();
        create_rust_file(dir.path(), "types.rs", 350);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "types.rs".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("pub struct MyStruct"));
        assert!(result.contains("pub enum MyEnum"));
    }

    #[tokio::test]
    async fn test_read_file_outline_contains_impl_blocks() {
        let dir = TempDir::new().unwrap();
        create_rust_file(dir.path(), "impls.rs", 350);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "impls.rs".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("impl MyStruct"));
    }

    #[tokio::test]
    async fn test_read_file_outline_contains_mods() {
        let dir = TempDir::new().unwrap();
        create_rust_file(dir.path(), "mods.rs", 350);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "mods.rs".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("pub(crate) mod inner"));
        assert!(result.contains("mod tests"));
    }

    #[tokio::test]
    async fn test_read_file_large_file_with_range_returns_content() {
        let dir = TempDir::new().unwrap();
        create_rust_file(dir.path(), "large_range.rs", 350);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "large_range.rs".to_string(),
            start_line: Some(1),
            end_line: Some(5),
        };
        let result = tool.call(args).await.unwrap();
        // Even though file is > 300 lines, a range was specified → content, not outline
        assert!(!result.contains("File outline for"));
        assert!(result.contains("| "));
    }

    #[tokio::test]
    async fn test_read_file_exactly_300_lines() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "exact300.txt", 300);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "exact300.txt".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        // ≤ 300 → full content
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 300"));
        assert!(!result.contains("File outline for"));
    }

    #[tokio::test]
    async fn test_read_file_301_lines_triggers_outline() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "just301.txt", 301);

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "just301.txt".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        // > 300 → outline mode (though a plain .txt file may have no symbols)
        // Should not be a full content dump with "1 | Line 1"
        assert!(!result.starts_with("  1 |"));
    }

    // -----------------------------------------------------------------------
    // Error tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_read_file_not_found() {
        let dir = TempDir::new().unwrap();
        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "nonexistent.rs".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            ReadFileToolError::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_read_file_path_denied_outside_root() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: format!("../../{}", outside_file.display()),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await;
        // Should be denied or not found (path traversal blocked)
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_empty_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("empty.txt");
        fs::write(&path, "").unwrap();

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "empty.txt".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        // Empty file has 1 line (the empty string from split), but content is empty-ish
        // Should not error
        assert!(result.contains("1 | "));
    }

    #[tokio::test]
    async fn test_read_file_binary_file_returns_clear_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("binary.bin");
        fs::write(&path, &[0xFF, 0xFE, 0x00, 0x80, 0x81]).unwrap();

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "binary.bin".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await;
        let err = result.unwrap_err();
        match &err {
            ReadFileToolError::ReadFailed { reason, .. } => {
                assert!(reason.contains("binary") || reason.contains("non-UTF-8"));
            }
            _ => panic!("Expected ReadFailed, got: {err:?}"),
        }
    }

    #[tokio::test]
    async fn test_read_file_is_directory() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("subdir");
        fs::create_dir(&sub).unwrap();

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "subdir".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await;
        assert!(matches!(
            result.unwrap_err(),
            ReadFileToolError::IsDirectory { .. }
        ));
    }

    #[tokio::test]
    async fn test_read_file_nested_path() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("src/tools");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("mod.rs"), "pub mod fs;").unwrap();

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "src/tools/mod.rs".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("pub mod fs;"));
    }

    #[tokio::test]
    async fn test_read_file_outline_markdown_headings() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("big.md");
        let mut content = String::new();
        content.push_str("# Top Heading\n");
        content.push_str("\n");
        content.push_str("Some text\n");
        content.push_str("\n");
        content.push_str("## Second Heading\n");
        content.push_str("\n");
        content.push_str("More text\n");
        content.push_str("\n");
        content.push_str("### Third Heading\n");
        // Pad to > 300 lines
        for i in 10..320 {
            content.push_str(&format!("Filler line {i}\n"));
        }
        fs::write(&path, &content).unwrap();

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "big.md".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("# Top Heading"));
        assert!(result.contains("## Second Heading"));
        assert!(result.contains("### Third Heading"));
        assert!(result.contains("[L1]"));
        assert!(result.contains("[L5]"));
        assert!(result.contains("[L9]"));
    }

    #[tokio::test]
    async fn test_read_file_outline_no_symbols_fallback() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("plain.txt");
        let content: String = (1..=310)
            .map(|i| format!("Just a plain text line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, &content).unwrap();

        let tool = ReadFileTool::new(dir.path().to_path_buf());
        let args = ReadFileToolArgs {
            path: "plain.txt".to_string(),
            start_line: None,
            end_line: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No structural symbols found"));
        assert!(result.contains("plain.txt"));
        assert!(result.contains("Use start_line and end_line"));
    }
}
