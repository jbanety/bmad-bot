//! Tool modules for the rig agent — git, filesystem, and terminal.
//!
//! This module exposes three tools that the LLM agent uses during autonomous
//! development sessions:
//!
//! - **[`GitTool`]** — Git operations (clone, checkout, branch, add, commit, push, diff, status, log) via `git2`
//! - **[`FsTool`]** — Filesystem operations (read, write, list, mkdir, delete, exists) with project-root security boundary
//! - **[`TerminalTool`]** — Shell command execution via `tokio::process` with timeout protection

pub mod fs;
pub mod git;
pub mod terminal;

pub use fs::FsTool;
pub use git::GitTool;
pub use terminal::TerminalTool;
