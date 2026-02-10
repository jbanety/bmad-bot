//! Tool modules for the rig agent — git, terminal, read-file, edit-file, grep, find-path, and list-directory.
//!
//! This module exposes tools that the LLM agent uses during autonomous
//! development sessions:
//!
//! - **[`GitTool`]** — Git operations (clone, checkout, branch, add, commit, push, diff, status, log) via `git2`
//! - **[`TerminalTool`]** — Shell command execution via `tokio::process` with timeout protection
//! - **[`ReadFileTool`]** — Read files with optional line ranges and automatic outline mode for large files
//! - **[`EditFileTool`]** — Surgical search-replace edits, create new files, overwrite when justified
//! - **[`GrepTool`]** — Regex-based content search across project files with .gitignore respect and pagination
//! - **[`FindPathTool`]** — Glob-based file path discovery with .gitignore respect and pagination
//! - **[`ListDirectoryTool`]** — List directory contents with entry types and file sizes, directories first

pub mod edit_file;
pub mod find_path;
pub mod git;
pub mod grep;
pub mod list_directory;
pub mod read_file;
pub mod terminal;

pub use edit_file::EditFileTool;
pub use find_path::FindPathTool;
pub use git::GitTool;
pub use grep::GrepTool;
pub use list_directory::ListDirectoryTool;
pub use read_file::ReadFileTool;
pub use terminal::TerminalTool;
