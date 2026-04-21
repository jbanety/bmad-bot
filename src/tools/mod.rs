//! Tool modules for the rig agent — focused tools for autonomous development.
//!
//! - **[`EditFileTool`]** — Surgical search-replace edits, create new files, overwrite
//! - **[`ReadFileTool`]** — Partial reading (line ranges) + automatic outline mode for large files
//! - **[`GrepTool`]** — Regex search across project file contents with glob filtering
//! - **[`FindPathTool`]** — Glob-based file path discovery
//! - **[`ListDirectoryTool`]** — List directory contents with types and sizes
//! - **[`GitTool`]** — Git operations via Git CLI
//! - **[`TerminalTool`]** — Shell command execution with timeout
//! - **[`SpawnAgentTool`]** — daemon-provided; spawn fresh sub-agents for delegated tasks
//!   with follow-up via `session_id`. Registered on dev + review sessions via
//!   [`session::agent::create_spawn_agent_tool`](crate::session::agent).
//! - **[`SubAgentState`]** — in-memory state of a live sub-agent session (opaque to
//!   callers; managed by `SpawnAgentTool`).

pub mod edit_file;
pub mod find_path;
pub mod git;
pub mod grep;
pub mod list_directory;
pub mod read_file;
pub mod spawn_agent;
pub mod terminal;

pub use edit_file::EditFileTool;
pub use find_path::FindPathTool;
pub use git::GitTool;
pub use grep::GrepTool;
pub use list_directory::ListDirectoryTool;
pub use read_file::ReadFileTool;
pub use spawn_agent::SpawnAgentTool;
pub use spawn_agent::SubAgentState;
pub use terminal::TerminalTool;
