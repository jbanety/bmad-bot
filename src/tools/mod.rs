//! Tool modules for the rig agent — git, filesystem, terminal, read-file, and edit-file.
//!
//! This module exposes tools that the LLM agent uses during autonomous
//! development sessions:
//!
//! - **[`GitTool`]** — Git operations (clone, checkout, branch, add, commit, push, diff, status, log) via `git2`
//! - **[`FsTool`]** — Filesystem operations (read, write, list, mkdir, delete, exists) with project-root security boundary
//! - **[`TerminalTool`]** — Shell command execution via `tokio::process` with timeout protection
//! - **[`ReadFileTool`]** — Read files with optional line ranges and automatic outline mode for large files
//! - **[`EditFileTool`]** — Surgical search-replace edits, create new files, overwrite when justified

pub mod edit_file;
pub mod fs;
pub mod git;
pub mod read_file;
pub mod terminal;

pub use edit_file::EditFileTool;
pub use fs::FsTool;
pub use git::GitTool;
pub use read_file::ReadFileTool;
pub use terminal::TerminalTool;
