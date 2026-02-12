//! Zed-style XML context formatting for LLM messages.
//!
//! When Zed sends file content to an LLM agent, it wraps files in structured
//! XML tags with markdown code blocks (`thread.rs:206-409` in Zed's source).
//! This module provides helpers to reproduce that exact format so that LLMs
//! trained on Zed-style context (Claude, GPT, etc.) correctly interpret
//! attached files as actionable context.
//!
//! # Format
//!
//! ```text
//! <context>
//! The following items were attached by the user. They are up-to-date and don't need to be re-read.
//!
//! <files>
//! ````md /absolute/path/to/file.md
//! {file content}
//! ````
//! </files>
//! </context>
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use crate::llm::context::ContextBuilder;
//!
//! let msg = ContextBuilder::new()
//!     .add_file("/path/to/agent.md", &agent_content)
//!     .add_file("/path/to/config.yaml", &config_content)
//!     .build();
//! ```

use std::path::Path;

/// Minimum backtick fence length for code blocks.
const MIN_FENCE_LEN: usize = 3;

/// Builder for Zed-style `<context>` XML messages containing one or more files.
///
/// Files are accumulated via [`add_file`](Self::add_file) /
/// [`add_file_with_lines`](Self::add_file_with_lines) and rendered into a
/// single XML string by [`build`](Self::build).
#[derive(Debug, Clone)]
pub struct ContextBuilder {
    /// Accumulated file entries (extension, display path, content, optional line range).
    files: Vec<FileEntry>,
    /// Optional description text inside the `<context>` tag.
    /// Defaults to Zed's standard phrasing.
    description: String,
}

/// A single file entry in the context.
#[derive(Debug, Clone)]
struct FileEntry {
    /// File extension for syntax highlighting (e.g. `md`, `rs`, `yaml`).
    extension: String,
    /// Display path — typically the absolute path.
    path: String,
    /// Raw file content.
    content: String,
    /// Optional line range suffix (e.g. `#L10-25`).
    line_range: Option<String>,
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextBuilder {
    /// Create a new empty context builder with the default Zed description.
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            description: "The following items were attached by the user. They are up-to-date and don't need to be re-read.".to_string(),
        }
    }

    /// Override the description text inside the `<context>` tag.
    pub fn description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Add a file to the context.
    ///
    /// The extension is inferred from the path. The path is used as-is for
    /// display — call [`std::fs::canonicalize`] before passing if you want an
    /// absolute path.
    pub fn add_file(self, path: impl AsRef<Path>, content: &str) -> Self {
        self.add_file_with_lines(path, content, None)
    }

    /// Add a file with an optional line range (e.g. `10..=25` → `#L10-25`).
    ///
    /// When `lines` is `Some((start, end))`, the range suffix is appended to
    /// the path in the code block tag.
    pub fn add_file_with_lines(
        mut self,
        path: impl AsRef<Path>,
        content: &str,
        lines: Option<(usize, usize)>,
    ) -> Self {
        let p = path.as_ref();
        let extension = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        let display_path = p.display().to_string();
        let line_range = lines.map(|(start, end)| format!("#L{start}-{end}"));

        self.files.push(FileEntry {
            extension,
            path: display_path,
            content: content.to_string(),
            line_range,
        });
        self
    }

    /// Read a file from disk and add it to the context.
    ///
    /// Resolves the path to an absolute path via [`std::fs::canonicalize`].
    /// Returns an error string if the file cannot be read.
    pub fn add_file_from_disk(self, path: impl AsRef<Path>) -> Result<Self, String> {
        let p = path.as_ref();
        let abs_path = std::fs::canonicalize(p)
            .map_err(|e| format!("Failed to resolve path '{}': {e}", p.display()))?;
        let content = std::fs::read_to_string(&abs_path)
            .map_err(|e| format!("Failed to read file '{}': {e}", abs_path.display()))?;
        Ok(self.add_file(abs_path, &content))
    }

    /// Build the final XML context string.
    ///
    /// Returns the complete `<context>...</context>` block ready to be sent
    /// as a user message to the LLM.
    pub fn build(&self) -> String {
        let mut out = String::new();
        out.push_str("<context>\n");
        out.push_str(&self.description);
        out.push_str("\n\n");

        if !self.files.is_empty() {
            out.push_str("<files>\n");
            for entry in &self.files {
                let fence = build_fence(&entry.content);
                let line_suffix = entry.line_range.as_deref().unwrap_or("");
                out.push_str(&format!(
                    "{fence}{ext} {path}{line_suffix}\n{content}\n{fence}\n",
                    ext = entry.extension,
                    path = entry.path,
                    content = entry.content,
                    fence = fence,
                ));
            }
            out.push_str("</files>\n");
        }

        out.push_str("</context>");
        out
    }
}

/// Build a backtick fence that doesn't collide with the content.
///
/// Zed's `MarkdownCodeBlock` (`markdown.rs:151-158`) uses the minimum number
/// of backticks that avoids ambiguity. We scan the content for the longest
/// run of consecutive backticks and return a fence one longer (minimum 3).
fn build_fence(content: &str) -> String {
    let mut max_run: usize = 0;
    let mut current_run: usize = 0;

    for ch in content.chars() {
        if ch == '`' {
            current_run += 1;
            if current_run > max_run {
                max_run = current_run;
            }
        } else {
            current_run = 0;
        }
    }

    let fence_len = if max_run >= MIN_FENCE_LEN {
        max_run + 1
    } else {
        MIN_FENCE_LEN
    };

    "`".repeat(fence_len)
}

/// Convenience: format a single file into a complete `<context>` block.
///
/// Shorthand for `ContextBuilder::new().add_file(path, content).build()`.
pub fn format_file_context(path: impl AsRef<Path>, content: &str) -> String {
    ContextBuilder::new().add_file(path, content).build()
}

/// Convenience: read a file from disk and format it into a `<context>` block.
///
/// Resolves the path to absolute via [`std::fs::canonicalize`].
pub fn format_file_context_from_disk(path: impl AsRef<Path>) -> Result<String, String> {
    Ok(ContextBuilder::new().add_file_from_disk(path)?.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // build_fence tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_fence_no_backticks() {
        assert_eq!(build_fence("hello world"), "```");
    }

    #[test]
    fn test_build_fence_single_backtick() {
        assert_eq!(build_fence("use `foo` here"), "```");
    }

    #[test]
    fn test_build_fence_double_backtick() {
        assert_eq!(build_fence("use ``foo`` here"), "```");
    }

    #[test]
    fn test_build_fence_triple_backtick() {
        assert_eq!(build_fence("```rust\nfn main() {}\n```"), "````");
    }

    #[test]
    fn test_build_fence_quadruple_backtick() {
        assert_eq!(build_fence("````md\nsome content\n````"), "`````");
    }

    #[test]
    fn test_build_fence_mixed_runs() {
        assert_eq!(build_fence("` `` ``` ```` ````` normal"), "``````");
    }

    #[test]
    fn test_build_fence_empty_content() {
        assert_eq!(build_fence(""), "```");
    }

    // -----------------------------------------------------------------------
    // ContextBuilder tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_builder_single_file() {
        let ctx = ContextBuilder::new()
            .add_file("/project/src/main.rs", "fn main() {}")
            .build();

        assert!(ctx.starts_with("<context>"));
        assert!(ctx.ends_with("</context>"));
        assert!(ctx.contains("<files>"));
        assert!(ctx.contains("</files>"));
        assert!(ctx.contains("```rs /project/src/main.rs"));
        assert!(ctx.contains("fn main() {}"));
        assert!(ctx.contains("They are up-to-date"));
    }

    #[test]
    fn test_builder_multiple_files() {
        let ctx = ContextBuilder::new()
            .add_file("/project/src/main.rs", "fn main() {}")
            .add_file("/project/config.yaml", "key: value")
            .build();

        assert!(ctx.contains("```rs /project/src/main.rs"));
        assert!(ctx.contains("fn main() {}"));
        assert!(ctx.contains("```yaml /project/config.yaml"));
        assert!(ctx.contains("key: value"));
    }

    #[test]
    fn test_builder_file_with_triple_backticks_uses_four() {
        let content = "```rust\nfn hello() {}\n```";
        let ctx = ContextBuilder::new()
            .add_file("/project/readme.md", content)
            .build();

        assert!(ctx.contains("````md /project/readme.md"));
        assert!(ctx.contains(content));
    }

    #[test]
    fn test_builder_with_line_range() {
        let ctx = ContextBuilder::new()
            .add_file_with_lines("/project/src/lib.rs", "pub fn foo() {}", Some((10, 25)))
            .build();

        assert!(ctx.contains("```rs /project/src/lib.rs#L10-25"));
    }

    #[test]
    fn test_builder_custom_description() {
        let ctx = ContextBuilder::new()
            .description("Custom context description.")
            .add_file("/test.txt", "hello")
            .build();

        assert!(ctx.contains("Custom context description."));
        assert!(!ctx.contains("They are up-to-date"));
    }

    #[test]
    fn test_builder_no_files() {
        let ctx = ContextBuilder::new().build();

        assert!(ctx.starts_with("<context>"));
        assert!(ctx.ends_with("</context>"));
        assert!(!ctx.contains("<files>"));
    }

    #[test]
    fn test_builder_no_extension() {
        let ctx = ContextBuilder::new()
            .add_file("/project/Makefile", "all: build")
            .build();

        // No extension → empty ext before the path
        assert!(ctx.contains("``` /project/Makefile"));
    }

    #[test]
    fn test_builder_preserves_content_exactly() {
        let content = "line1\n  indented\n\ttabbed\n\nempty above";
        let ctx = ContextBuilder::new().add_file("/file.txt", content).build();

        assert!(ctx.contains(content));
    }

    // -----------------------------------------------------------------------
    // Convenience function tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_file_context() {
        let ctx = format_file_context("/project/file.rs", "use std::io;");

        assert!(ctx.starts_with("<context>"));
        assert!(ctx.ends_with("</context>"));
        assert!(ctx.contains("```rs /project/file.rs"));
        assert!(ctx.contains("use std::io;"));
    }

    #[test]
    fn test_format_file_context_from_disk_missing_file() {
        let result = format_file_context_from_disk("/nonexistent/path/to/file.rs");
        assert!(result.is_err());
    }

    #[test]
    fn test_format_file_context_from_disk_reads_real_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file_path = dir.path().join("test_agent.md");
        std::fs::write(&file_path, "# Agent\nHello!").expect("write test file");

        let ctx = format_file_context_from_disk(&file_path).expect("format should succeed");

        assert!(ctx.contains("<context>"));
        assert!(ctx.contains("# Agent\nHello!"));
        assert!(ctx.contains("```md "));
    }

    // -----------------------------------------------------------------------
    // add_file_from_disk tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_file_from_disk_uses_absolute_path() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file_path = dir.path().join("relative.yaml");
        std::fs::write(&file_path, "key: val").expect("write test file");

        let ctx = ContextBuilder::new()
            .add_file_from_disk(&file_path)
            .expect("add_file_from_disk")
            .build();

        // Path in the output should be absolute (canonicalized)
        let canonical = std::fs::canonicalize(&file_path).expect("canonicalize");
        assert!(ctx.contains(&canonical.display().to_string()));
    }

    #[test]
    fn test_add_file_from_disk_error_on_missing() {
        let result = ContextBuilder::new().add_file_from_disk("/no/such/file.rs");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to resolve path"));
    }

    // -----------------------------------------------------------------------
    // Integration: builder with content containing nested backtick fences
    // -----------------------------------------------------------------------

    #[test]
    fn test_deeply_nested_backticks() {
        // Simulate a markdown file that itself contains 4-backtick code blocks
        let content = "# Doc\n\n````rust\nfn foo() {\n    println!(\"```\");\n}\n````\n";
        let ctx = ContextBuilder::new()
            .add_file("/project/doc.md", content)
            .build();

        // Should use 5 backticks since content has 4-backtick runs
        assert!(ctx.contains("`````md /project/doc.md"));
    }

    #[test]
    fn test_builder_default_is_new() {
        let a = ContextBuilder::new().build();
        let b = ContextBuilder::default().build();
        assert_eq!(a, b);
    }

    #[test]
    fn test_builder_is_clone() {
        let builder = ContextBuilder::new().add_file("/a.rs", "code");
        let cloned = builder.clone();
        assert_eq!(builder.build(), cloned.build());
    }

    #[test]
    fn test_builder_is_debug() {
        let builder = ContextBuilder::new().add_file("/a.rs", "code");
        let debug = format!("{builder:?}");
        assert!(debug.contains("ContextBuilder"));
    }

    // -----------------------------------------------------------------------
    // PathBuf compatibility
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_file_accepts_pathbuf() {
        let path = PathBuf::from("/project/file.rs");
        let ctx = ContextBuilder::new().add_file(&path, "content").build();
        assert!(ctx.contains("/project/file.rs"));
    }

    #[test]
    fn test_add_file_accepts_str() {
        let ctx = ContextBuilder::new()
            .add_file("/project/file.rs", "content")
            .build();
        assert!(ctx.contains("/project/file.rs"));
    }
}
