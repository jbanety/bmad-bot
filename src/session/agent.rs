//! Shared agent activation — preamble, tool construction, activation, and streaming chat.
//!
//! This module contains the common logic used by [`SessionRunner`](super::runner::SessionRunner),
//! [`ReviewRunner`](crate::review::ReviewRunner), and
//! [`ArchitectSession`](crate::supervisor::architect::ArchitectSession) to set up
//! and run BMAD agents. The activation flow is identical for all roles:
//!
//! 1. Build a generic preamble with tool usage rules and English override
//! 2. Create the standard tool set via [`create_base_tools()`] or [`create_tools_with_supervisor()`]
//! 3. Send the agent file as a user message (Zed-style XML context) to trigger BMAD activation
//! 4. The agent processes activation steps: loads `config.yaml`, greets user, shows menu
//! 5. Caller sends a menu command (`DS` for dev, `CR` for review, `CH` for supervisor)

use crate::config::BotConfig;
use crate::llm::agent_factory::AgentFactory;
use crate::llm::context::ContextBuilder;
use crate::llm::logging::{log_llm_error, log_llm_request, log_llm_response};
use crate::mcp::McpManager;
use crate::session::state::ChatMessage;
use crate::supervisor::decisions::DecisionLog;
use crate::supervisor::{AskSupervisor, EscalationSlot};
use crate::tools::{
    EditFileTool, FindPathTool, GitTool, GrepTool, ListDirectoryTool, ReadFileTool, TerminalTool,
};

use futures::StreamExt;
use rig::agent::{MultiTurnStreamItem, StreamingPromptHook};
use rig::completion::{Chat, CompletionModel, GetTokenUsage, Message};
use rig::message::Text;
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat};

use crate::ui::UiHandle;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Terminal tool timeout in seconds — shared by all agent roles.
pub const TERMINAL_TIMEOUT_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// Centralized tool construction
// ---------------------------------------------------------------------------

/// The 7 base tools returned by [`create_base_tools()`].
pub type BaseToolSet = (
    GitTool,
    ReadFileTool,
    EditFileTool,
    GrepTool,
    FindPathTool,
    ListDirectoryTool,
    TerminalTool,
);

/// The 8 tools (7 base + supervisor) returned by [`create_tools_with_supervisor()`].
pub type FullToolSet = (
    GitTool,
    ReadFileTool,
    EditFileTool,
    GrepTool,
    FindPathTool,
    ListDirectoryTool,
    TerminalTool,
    AskSupervisor,
);

/// Standard tool set WITHOUT ask_supervisor — used by the supervisor/architect agent.
///
/// Returns 7 base tools. Caller adds `ThinkTool` via `configure_agent_tools!`.
pub fn create_base_tools(project_root: &Path) -> BaseToolSet {
    let git = GitTool::new(project_root.to_path_buf());
    let read_file = ReadFileTool::new(project_root.to_path_buf());
    let edit_file = EditFileTool::new(project_root.to_path_buf());
    let grep = GrepTool::new(project_root.to_path_buf());
    let find_path = FindPathTool::new(project_root.to_path_buf());
    let list_dir = ListDirectoryTool::new(project_root.to_path_buf());
    let terminal = TerminalTool::new(project_root.to_path_buf(), TERMINAL_TIMEOUT_SECS);
    (
        git, read_file, edit_file, grep, find_path, list_dir, terminal,
    )
}

/// Standard tool set WITH ask_supervisor — used by dev session and code review.
///
/// Returns 8 custom tools (7 base + supervisor). Caller adds `ThinkTool` via
/// `configure_agent_tools!`.
pub fn create_tools_with_supervisor(
    project_root: &Path,
    config: &BotConfig,
    agent_factory: &Arc<AgentFactory>,
    escalation_slot: EscalationSlot,
    decision_log: DecisionLog,
    mcp_manager: &Arc<McpManager>,
) -> Result<FullToolSet, Box<dyn std::error::Error>> {
    let (git, read_file, edit_file, grep, find_path, list_dir, terminal) =
        create_base_tools(project_root);
    let supervisor = AskSupervisor::with_architect_from_config(
        config,
        Some(Arc::clone(agent_factory)),
        escalation_slot,
        decision_log,
        Arc::clone(mcp_manager),
    )?;
    Ok((
        git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor,
    ))
}

/// Shared shutdown flag — set to `true` when Ctrl+C or SIGTERM is received.
///
/// Checked cooperatively between streaming chunks and chat turns so that
/// long-running tool-calling loops can be interrupted cleanly.
pub type ShutdownFlag = Arc<AtomicBool>;

/// Maximum tool-call rounds allowed per single prompt in the streaming loop.
const STREAMING_MAX_TURNS: usize = 300;

// ---------------------------------------------------------------------------
// ChatHistoryHook — captures full conversation history (including tool calls)
// ---------------------------------------------------------------------------

/// Hook that captures the full conversation history during streaming multi-turn.
///
/// Rig's `on_completion_call` hook is invoked before each LLM call with the
/// current prompt and the accumulated history (including tool calls and tool
/// results from previous turns). By storing the latest snapshot, we can
/// reconstruct the complete conversation after the stream finishes.
///
/// ## Reconstruction formula
///
/// ```text
/// full_history = last_captured_history + [last_captured_prompt, Message::assistant(text)]
/// ```
/// Snapshot captured by [`ChatHistoryHook::on_completion_call`]: `(history, prompt)`.
type HistorySnapshot = Option<(Vec<Message>, Message)>;

#[derive(Clone)]
pub struct ChatHistoryHook {
    /// Latest `(history, prompt)` snapshot from `on_completion_call`.
    /// `None` until the first hook invocation.
    inner: Arc<Mutex<HistorySnapshot>>,
}

impl ChatHistoryHook {
    /// Create a new hook with no captured state.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Extract the full conversation history after the stream completes.
    ///
    /// Combines the last captured `(history, prompt)` with the final assistant
    /// text response to produce the complete message sequence including all
    /// tool calls and tool results.
    ///
    /// Returns `None` if the hook was never invoked (e.g. stream errored
    /// before the first LLM call).
    pub fn take_full_history(&self, final_text: &str) -> Option<Vec<Message>> {
        let guard = self.inner.lock().expect("ChatHistoryHook lock");
        let (history, prompt) = guard.as_ref()?;
        let mut full = history.clone();
        full.push(prompt.clone());
        full.push(Message::assistant(final_text));
        Some(full)
    }
}

impl<M: CompletionModel> StreamingPromptHook<M> for ChatHistoryHook {
    fn on_completion_call(
        &self,
        prompt: &Message,
        history: &[Message],
    ) -> impl std::future::Future<Output = rig::agent::HookAction> + Send {
        // Capture the latest snapshot synchronously (lock is brief).
        let mut guard = self.inner.lock().expect("ChatHistoryHook lock");
        *guard = Some((history.to_vec(), prompt.clone()));
        async { rig::agent::HookAction::cont() }
    }
}

/// Build the generic agent preamble with tool usage rules and English override.
///
/// This preamble is used as the system prompt for both dev sessions and code
/// review sessions. It does NOT contain the agent persona — that is sent as
/// a user message via [`activate_agent`].
///
/// When `mcp_tool_names` is non-empty, the preamble's tool section includes
/// the MCP tool names so the agent knows they are available. When empty, the
/// preamble output is identical to the pre-MCP version.
///
/// When `model` contains `"preview"`, an extra rule is injected to force
/// sequential tool calls (one at a time). This works around a known issue
/// where preview models (e.g. `gemini-3.1-pro-preview`) concatenate multiple
/// tool call arguments into a single malformed JSON blob (`{...}{...}`),
/// which poisons the conversation history and causes unrecoverable 400 errors.
/// TODO: Remove this workaround once the model exits preview.
pub fn build_preamble(mcp_tool_names: &[String], model: &str) -> String {
    let mcp_line = if mcp_tool_names.is_empty() {
        String::new()
    } else {
        format!(
            "\nYou also have access to MCP tools: {}. Use them like any other tool.",
            mcp_tool_names.join(", ")
        )
    };

    // Workaround: preview models (e.g. gemini-3.1-pro-preview) sometimes
    // concatenate parallel tool call args into invalid JSON. Force sequential.
    let sequential_tool_rule = if model.contains("preview") {
        "- **CRITICAL: Call tools ONE AT A TIME, sequentially.** Never attempt parallel tool calls or combine arguments from multiple calls into one."
    } else {
        ""
    };

    format!(
        r#"You are an AI agent operating autonomously in a BMAD workflow environment.

## Tools
You have access to these tools: edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor, plus a built-in think tool for reasoning.{mcp_line}

## Tool Usage Rules
- **ALWAYS use `edit_file` with mode="edit"** to modify existing files. NEVER rewrite entire files unless creating a new file (mode="create") or a complete rewrite is truly necessary (mode="overwrite").
- **Use `read_file` with line ranges** for large files. Read the outline first, then target specific sections with start_line/end_line.
- **Use `grep` to find symbols** before editing — never assume file paths or line numbers.
- **Use `find_path`** to discover files by name pattern when you don't know the full path.
- **Use `list_directory`** to explore directory structure.
- **Use `terminal`** for build commands, tests, mkdir, rm, and other shell operations.
- **Use `ask_supervisor`** when you need clarification on requirements, architecture decisions, or are uncertain about the correct approach.
- When `edit_file` fails (ambiguous match), use `read_file` with a line range to get more context, then retry with a larger `old_text` fragment.
- When making multiple related changes in one file, batch them in a single `edit_file` call with multiple edit operations.
{sequential_tool_rule}

## Session Completion Protocol
When you have fully completed your workflow (all tasks done, all tests passing, story file updated, all changes committed), your **final message** MUST end with exactly:

<<BMAD_JOB_DONE>>

Rules:
- `<<BMAD_JOB_DONE>>` MUST appear on its own line as the very last thing in your final message.
- Do NOT paraphrase, omit, or embed the sentinel mid-sentence. Emit it exactly as shown.

## Communication
OVERRIDE: communication_language = English

## Rules
- When the user provides an agent file in <context><files> tags, you MUST fully embody that agent's persona and follow ALL activation instructions exactly as specified.
- NEVER break character until given an exit command.
- Execute activation steps in order — load configuration files via tools, then greet and display the menu.
- Wait for user input after displaying the menu."#,
        mcp_line = mcp_line,
        sequential_tool_rule = sequential_tool_rule
    )
}

/// Send a prompt via streaming and collect the complete text response.
///
/// This is a drop-in replacement for `agent.chat(prompt, history)` that uses
/// rig's streaming API instead. All providers (Anthropic, OpenAI, GitHub Copilot)
/// support streaming — and Copilot **requires** it (`stream: false` is rejected).
///
/// Tool calls are handled automatically by rig within the stream.
///
/// When `shutdown` is `Some` and the flag is set to `true`, the stream is
/// abandoned and a `ShutdownRequested` error is returned. This allows Ctrl+C
/// to interrupt even deep multi-turn tool-calling loops.
/// Send a prompt via streaming, collect the text response, and return the
/// **full conversation history** including all tool calls and tool results.
///
/// Returns `(accumulated_text, full_history)` where `full_history` contains
/// every message exchanged during the multi-turn loop — user prompts,
/// assistant tool calls, user tool results, and the final assistant text.
///
/// If the history hook fails to capture (e.g. stream errors before the first
/// LLM call), `full_history` falls back to the input `history` plus the
/// prompt and accumulated text as plain messages.
pub async fn streaming_chat<A, M>(
    agent: &A,
    prompt: impl Into<Message> + Send,
    history: Vec<Message>,
    shutdown: Option<&ShutdownFlag>,
    ui: Option<&UiHandle>,
) -> Result<(String, Vec<Message>), rig::completion::PromptError>
where
    A: StreamingChat<M, M::StreamingResponse>,
    M: CompletionModel + 'static,
    M::StreamingResponse: Clone + Unpin + GetTokenUsage,
{
    let hook = ChatHistoryHook::new();
    let prompt_msg: Message = prompt.into();

    let mut stream = agent
        .stream_chat(prompt_msg.clone(), history.clone())
        .with_hook(hook.clone())
        .multi_turn(STREAMING_MAX_TURNS)
        .await;

    let mut acc = String::new();
    // Track tool call IDs → tool names so we can label tool results.
    let mut tool_call_names: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    loop {
        // Cooperative shutdown check — between every chunk/tool-call round
        if let Some(flag) = shutdown
            && flag.load(Ordering::Relaxed)
        {
            tracing::info!(
                action = "shutdown_requested",
                "Shutdown flag detected in streaming loop"
            );
            return Err(rig::completion::PromptError::CompletionError(
                rig::completion::CompletionError::ResponseError(
                    "Shutdown requested (Ctrl+C)".to_string(),
                ),
            ));
        }

        let Some(chunk) = stream.next().await else {
            break;
        };

        match chunk {
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(
                Text { text },
            ))) => {
                acc.push_str(&text);
            }
            // ── Tool call visibility (AC #8) ────────────────────────
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
                tool_call,
                internal_call_id,
            })) => {
                let tool_name = &tool_call.function.name;
                let detail = extract_tool_call_detail(tool_name, &tool_call.function.arguments);
                tool_call_names.insert(internal_call_id, tool_name.clone());
                if let Some(ui) = ui {
                    ui.tool_call(tool_name, &detail);
                }
            }
            // ── Tool result visibility (AC #9) ──────────────────────
            Ok(MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult {
                tool_result,
                internal_call_id,
            })) => {
                let tool_name = tool_call_names
                    .get(&internal_call_id)
                    .map(|s| s.as_str())
                    .unwrap_or("unknown");
                let brief = extract_tool_result_brief(&tool_result.content);
                if let Some(ui) = ui {
                    ui.tool_result(tool_name, &brief);
                }
            }
            Ok(MultiTurnStreamItem::FinalResponse(_)) => {
                // FinalResponse signals the end of the stream.
                // acc already contains the full accumulated text.
                break;
            }
            Err(e) => {
                return Err(rig::completion::PromptError::CompletionError(
                    rig::completion::CompletionError::ResponseError(e.to_string()),
                ));
            }
            _ => continue, // tool call deltas, reasoning, etc. — handled by rig
        }
    }

    // Reconstruct full history from the hook capture.
    // Fallback: if hook never fired, build a text-only history.
    let full_history = hook.take_full_history(&acc).unwrap_or_else(|| {
        let mut fallback = history;
        fallback.push(prompt_msg);
        fallback.push(Message::assistant(&acc));
        fallback
    });

    Ok((acc, full_history))
}

/// Extract a human-readable detail string from a tool call's arguments.
///
/// Each tool type has a specific detail format (see story Dev Notes):
/// - `edit_file` → `"{path} ({mode})"`
/// - `read_file` → `"{path}"` or `"{path} L{start}-{end}"`
/// - `grep` → `"/{pattern}/"`
/// - `find_path` → `"{glob}"`
/// - `list_directory` → `"{path}"`
/// - `git` → `"{sub_action} {key_arg}"`
/// - `terminal` → first 80 chars of the command
/// - `ask_supervisor` → first 80 chars of the question
/// - `think` → `"(reasoning)"`
fn extract_tool_call_detail(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "edit_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("edit");
            format!("{path} ({mode})")
        }
        "read_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let start = args.get("start_line").and_then(|v| v.as_u64());
            let end = args.get("end_line").and_then(|v| v.as_u64());
            match (start, end) {
                (Some(s), Some(e)) => format!("{path} L{s}-{e}"),
                (Some(s), None) => format!("{path} L{s}-"),
                _ => path.to_string(),
            }
        }
        "grep" => {
            let pattern = args.get("regex").and_then(|v| v.as_str()).unwrap_or("?");
            format!("/{pattern}/")
        }
        "find_path" => {
            let glob = args.get("glob").and_then(|v| v.as_str()).unwrap_or("?");
            glob.to_string()
        }
        "list_directory" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            path.to_string()
        }
        "git" => {
            let sub = args
                .get("sub_action")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            // Pick the most descriptive argument for the sub-action
            let key_arg = args
                .get("message")
                .or_else(|| args.get("branch"))
                .or_else(|| args.get("args"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if key_arg.is_empty() {
                sub.to_string()
            } else {
                format!("{sub} \"{key_arg}\"")
            }
        }
        "terminal" => {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("?");
            truncate_str(cmd, 80)
        }
        "ask_supervisor" => {
            let q = args.get("question").and_then(|v| v.as_str()).unwrap_or("?");
            truncate_str(q, 80)
        }
        "think" => "(reasoning)".to_string(),
        _other => {
            // MCP or unknown tool — show first 80 chars of serialized args
            let s = args.to_string();
            truncate_str(&s, 80)
        }
    }
}

/// Extract a brief summary string from a tool result's content.
///
/// Returns a short string suitable for `ui.tool_result()`: file size, line
/// count, match count, exit code, or a truncated text snippet.
fn extract_tool_result_brief(
    content: &rig::one_or_many::OneOrMany<rig::message::ToolResultContent>,
) -> String {
    // Get the first text content item
    let text = content.iter().find_map(|c| {
        if let rig::message::ToolResultContent::Text(t) = c {
            Some(t.text.as_str())
        } else {
            None
        }
    });

    let Some(text) = text else {
        return "(no text)".to_string();
    };

    // Heuristic: detect common result patterns and produce brief summaries
    if text.starts_with("Error") || text.starts_with("error") {
        return truncate_str(text, 80);
    }

    // Line-count heuristic: count newlines for multi-line output
    let line_count = text.chars().filter(|&c| c == '\n').count();
    if line_count > 3 {
        let byte_len = text.len();
        return format!("{line_count} lines, {byte_len} bytes");
    }

    // Short result — return truncated
    truncate_str(text, 120)
}

/// Truncate a string to `max` characters, appending `…` if truncated.
///
/// Uses `char_indices` to find a safe Unicode boundary — never slices by byte
/// index directly.
fn truncate_str(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((byte_idx, _)) => {
            let mut t = s[..byte_idx].to_string();
            t.push('…');
            t
        }
        None => s.to_string(),
    }
}

/// Activate a BMAD agent by sending the agent file as the first user message.
///
/// Returns `(rig_history, chat_history)` — the rig `Message` vec for subsequent
/// `streaming_chat` calls and the `ChatMessage` vec for WAL state persistence.
///
/// The LLM receives the full agent file content as a Zed-style XML context user
/// message, processes the activation steps (loads `config.yaml` via tools, reads
/// the story file, shows the greeting and menu), and is then ready to accept
/// commands like `"DS"`, `"CR"`, or `"CH"`.
///
/// # Arguments
/// - `agent` — the built rig agent (with preamble and tools already attached)
/// - `project_root` — path to the project root
/// - `agent_relative_path` — relative path from project root to the agent file
///   (e.g. `"_bmad/bmm/agents/dev.md"` or `"_bmad/bmm/agents/architect.md"`)
/// - `label` — logging label (e.g. `"dev-session"`, `"code-review"`, `"supervisor"`)
/// - `shutdown` — optional shutdown flag for cooperative cancellation
/// - `ui` — optional UI handle for emitting tool call events during activation
pub async fn activate_agent<A, M>(
    agent: &A,
    project_root: &str,
    agent_relative_path: &str,
    label: &str,
    shutdown: Option<&ShutdownFlag>,
    ui: Option<&UiHandle>,
) -> Result<(Vec<Message>, Vec<ChatMessage>), String>
where
    A: Chat + StreamingChat<M, M::StreamingResponse>,
    M: CompletionModel + 'static,
    M::StreamingResponse: Clone + Unpin + GetTokenUsage,
{
    let agent_path = Path::new(project_root).join(agent_relative_path);

    // Build Zed-style XML context message via ContextBuilder helper.
    // This reads the file, resolves to absolute path, and wraps in
    // <context><files>...</files></context> — same format Zed uses
    // for @file inclusions (thread.rs:206-409).
    let activation_msg = ContextBuilder::new()
        .add_file_from_disk(&agent_path)
        .map_err(|e| format!("Failed to build agent activation context: {e}"))?
        .build();

    let mut rig_history: Vec<Message> = vec![];
    let mut chat_history: Vec<ChatMessage> = vec![];

    // Send agent file wrapped in XML context tags — triggers BMAD activation flow
    log_llm_request(
        label,
        0,
        &format!("[agent activation: {agent_relative_path} in context tags]"),
        rig_history.len(),
    );
    let (response, new_history) =
        streaming_chat(agent, activation_msg.as_str(), rig_history, shutdown, ui)
            .await
            .map_err(|e| {
                log_llm_error(label, 0, &e);
                format!("Agent activation failed: {e}")
            })?;
    log_llm_response(label, 0, &response);

    rig_history = new_history;
    chat_history.push(ChatMessage {
        role: "user".to_string(),
        content: activation_msg,
    });
    chat_history.push(ChatMessage {
        role: "assistant".to_string(),
        content: response,
    });

    tracing::info!(
        action = "agent_activation_complete",
        label = %label,
        "BMAD agent activated via user message"
    );

    Ok((rig_history, chat_history))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_preamble_contains_tool_rules() {
        let preamble = build_preamble(&[], "claude-sonnet-4-20250514");
        assert!(preamble.contains("edit_file"));
        assert!(preamble.contains("read_file"));
        assert!(preamble.contains("grep"));
        assert!(preamble.contains("find_path"));
        assert!(preamble.contains("list_directory"));
        assert!(preamble.contains("terminal"));
        assert!(preamble.contains("ask_supervisor"));
    }

    #[test]
    fn test_build_preamble_contains_english_override() {
        let preamble = build_preamble(&[], "claude-sonnet-4-20250514");
        assert!(
            preamble.contains("communication_language = English"),
            "Preamble must contain English override"
        );
    }

    #[test]
    fn test_build_preamble_contains_activation_rules() {
        let preamble = build_preamble(&[], "claude-sonnet-4-20250514");
        assert!(preamble.contains("<context><files>"));
        assert!(preamble.contains("activation instructions"));
    }

    #[test]
    fn test_build_preamble_does_not_contain_agent_content() {
        let preamble = build_preamble(&[], "claude-sonnet-4-20250514");
        // The preamble should NOT contain the agent file content — that goes
        // in the user message via activate_agent()
        assert!(
            !preamble.contains("dev.agent.yaml"),
            "Preamble should not contain dev.md agent content"
        );
        assert!(
            !preamble.contains("Amelia"),
            "Preamble should not contain agent persona name"
        );
    }

    #[test]
    fn test_build_preamble_contains_job_done_sentinel() {
        let preamble = build_preamble(&[], "claude-sonnet-4-20250514");
        assert!(
            preamble.contains("<<BMAD_JOB_DONE>>"),
            "Preamble must contain the deterministic completion sentinel"
        );
        assert!(
            preamble.contains("Session Completion Protocol"),
            "Preamble must contain the sentinel instruction section"
        );
    }

    #[test]
    fn test_build_preamble_mentions_tool_usage_best_practices() {
        let preamble = build_preamble(&[], "claude-sonnet-4-20250514");
        assert!(
            preamble.contains("mode=\"edit\""),
            "Should mention edit mode for existing files"
        );
        assert!(
            preamble.contains("line range"),
            "Should mention line ranges for large files"
        );
    }

    #[test]
    fn test_shutdown_flag_type_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ShutdownFlag>();
    }

    // -- Story 9.2: build_preamble MCP integration tests --

    #[test]
    fn test_build_preamble_empty_mcp_is_identical() {
        // When mcp_tool_names is empty, output must be byte-identical to
        // the pre-MCP hardcoded preamble (no extra lines, no trailing space).
        let preamble = build_preamble(&[], "claude-sonnet-4-20250514");
        assert!(
            !preamble.contains("MCP tools"),
            "Empty MCP names must not inject any MCP line into preamble"
        );
    }

    #[test]
    fn test_build_preamble_with_mcp_tools_includes_names() {
        let names = vec![
            "browser_navigate".to_string(),
            "browser_screenshot".to_string(),
        ];
        let preamble = build_preamble(&names, "gpt-4o");
        assert!(
            preamble.contains("browser_navigate"),
            "Preamble must mention browser_navigate when MCP tools provided"
        );
        assert!(
            preamble.contains("browser_screenshot"),
            "Preamble must mention browser_screenshot when MCP tools provided"
        );
        assert!(
            preamble.contains("MCP tools"),
            "Preamble must contain the MCP tools label"
        );
        assert!(
            preamble.contains("Use them like any other tool"),
            "Preamble must instruct the agent to use MCP tools normally"
        );
    }

    #[test]
    fn test_build_preamble_with_mcp_still_contains_native_tools() {
        // Even with MCP tools, all existing native tool references must be present.
        let names = vec!["browser_navigate".to_string(), "browser_click".to_string()];
        let preamble = build_preamble(&names, "gpt-4o");
        assert!(preamble.contains("edit_file"));
        assert!(preamble.contains("read_file"));
        assert!(preamble.contains("grep"));
        assert!(preamble.contains("find_path"));
        assert!(preamble.contains("list_directory"));
        assert!(preamble.contains("terminal"));
        assert!(preamble.contains("ask_supervisor"));
        assert!(preamble.contains("<<BMAD_JOB_DONE>>"));
        assert!(preamble.contains("communication_language = English"));
    }

    // -----------------------------------------------------------------------
    // ChatHistoryHook tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_chat_history_hook_new_returns_none() {
        let hook = ChatHistoryHook::new();
        assert!(
            hook.take_full_history("hello").is_none(),
            "Hook should return None when never invoked"
        );
    }

    #[test]
    fn test_chat_history_hook_captures_snapshot() {
        let hook = ChatHistoryHook::new();
        // Simulate on_completion_call by writing directly to inner
        {
            let mut guard = hook.inner.lock().unwrap();
            *guard = Some((
                vec![Message::user("msg1"), Message::assistant("resp1")],
                Message::user("msg2"),
            ));
        }
        let full = hook.take_full_history("final response").unwrap();
        assert_eq!(full.len(), 4); // msg1, resp1, msg2, assistant("final response")
    }

    #[test]
    fn test_chat_history_hook_keeps_latest_snapshot() {
        let hook = ChatHistoryHook::new();
        // First snapshot
        {
            let mut guard = hook.inner.lock().unwrap();
            *guard = Some((vec![Message::user("old")], Message::user("old_prompt")));
        }
        // Second snapshot overwrites
        {
            let mut guard = hook.inner.lock().unwrap();
            *guard = Some((
                vec![
                    Message::user("old"),
                    Message::assistant("old_resp"),
                    Message::user("old_prompt"),
                    Message::assistant("tool_call_resp"),
                ],
                Message::user("tool_result"),
            ));
        }
        let full = hook.take_full_history("done").unwrap();
        // Should be: old, old_resp, old_prompt, tool_call_resp, tool_result, assistant("done")
        assert_eq!(full.len(), 6);
    }

    #[test]
    fn test_chat_history_hook_reconstructs_complete_history() {
        let hook = ChatHistoryHook::new();
        // Simulate a multi-turn: initial prompt → tool call → tool result → text response
        // on_completion_call for turn 2 gives us:
        //   history = [user_prompt, assistant_tool_call]
        //   prompt  = user_tool_result
        {
            let mut guard = hook.inner.lock().unwrap();
            *guard = Some((
                vec![
                    Message::user("implement feature X"),
                    Message::assistant("I'll read the file first"),
                ],
                Message::user("tool result: file contents here"),
            ));
        }
        let full = hook
            .take_full_history("Feature X implemented. All tests pass.")
            .unwrap();

        // Full history should be:
        // [0] user: "implement feature X"
        // [1] assistant: "I'll read the file first"
        // [2] user: "tool result: file contents here"
        // [3] assistant: "Feature X implemented. All tests pass."
        assert_eq!(full.len(), 4);
    }

    #[test]
    fn test_chat_history_hook_is_clone_send_sync() {
        fn assert_clone_send_sync<T: Clone + Send + Sync>() {}
        assert_clone_send_sync::<ChatHistoryHook>();
    }

    #[test]
    fn test_chat_history_hook_empty_history_with_prompt() {
        let hook = ChatHistoryHook::new();
        // First turn: no prior history, just the initial prompt
        {
            let mut guard = hook.inner.lock().unwrap();
            *guard = Some((vec![], Message::user("hello")));
        }
        let full = hook.take_full_history("hi there").unwrap();
        assert_eq!(full.len(), 2); // user("hello"), assistant("hi there")
    }

    // -- Preview model sequential tool call workaround tests --

    #[test]
    fn test_build_preamble_preview_model_injects_sequential_rule() {
        let preamble = build_preamble(&[], "gemini-3.1-pro-preview");
        assert!(
            preamble.contains("Call tools ONE AT A TIME"),
            "Preview model preamble must contain sequential tool call rule"
        );
    }

    #[test]
    fn test_build_preamble_stable_model_no_sequential_rule() {
        let preamble = build_preamble(&[], "claude-opus-4.6");
        assert!(
            !preamble.contains("Call tools ONE AT A TIME"),
            "Stable model preamble must NOT contain sequential tool call rule"
        );
    }

    #[test]
    fn test_build_preamble_preview_detection_is_substring() {
        // Any model with "preview" anywhere in the name triggers the rule
        let preamble = build_preamble(&[], "some-model-preview-v2");
        assert!(preamble.contains("Call tools ONE AT A TIME"));
    }

    // -----------------------------------------------------------------------
    // create_base_tools tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_base_tools_returns_seven_tools() {
        use rig::tool::Tool;
        let tmp = std::env::temp_dir();
        let (git, read_file, edit_file, grep, find_path, list_dir, terminal) =
            create_base_tools(&tmp);
        // Type system guarantees tuple correctness — verify construction didn't panic
        // and that each tool reports the expected NAME constant.
        assert_eq!(<crate::tools::GitTool as Tool>::NAME, "git");
        assert_eq!(<crate::tools::ReadFileTool as Tool>::NAME, "read_file");
        assert_eq!(<crate::tools::EditFileTool as Tool>::NAME, "edit_file");
        assert_eq!(<crate::tools::GrepTool as Tool>::NAME, "grep");
        assert_eq!(<crate::tools::FindPathTool as Tool>::NAME, "find_path");
        assert_eq!(
            <crate::tools::ListDirectoryTool as Tool>::NAME,
            "list_directory"
        );
        assert_eq!(<crate::tools::TerminalTool as Tool>::NAME, "terminal");
        // Suppress unused-variable warnings — the bindings prove the tuple destructures correctly.
        let _ = (
            git, read_file, edit_file, grep, find_path, list_dir, terminal,
        );
    }

    #[test]
    fn test_create_base_tools_accepts_any_path() {
        // Ensure construction works with an arbitrary path (no filesystem validation at build time).
        let fake_root = Path::new("/nonexistent/project/root");
        let tools = create_base_tools(fake_root);
        let _ = tools; // 7-tuple constructed without panic
    }
}
