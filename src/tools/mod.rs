//! Tool modules for the rig agent — 7 focused tools for autonomous development.
//!
//! - **[`EditFileTool`]** — Surgical search-replace edits, create new files, overwrite
//! - **[`ReadFileTool`]** — Partial reading (line ranges) + automatic outline mode for large files
//! - **[`GrepTool`]** — Regex search across project file contents with glob filtering
//! - **[`FindPathTool`]** — Glob-based file path discovery
//! - **[`ListDirectoryTool`]** — List directory contents with types and sizes
//! - **[`GitTool`]** — Git operations via git2
//! - **[`TerminalTool`]** — Shell command execution with timeout

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
