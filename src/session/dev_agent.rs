//! Shared BMAD dev agent activation — preamble, activation, and streaming chat.
//!
//! This module contains the common logic used by both [`SessionRunner`](super::runner::SessionRunner)
//! and [`ReviewRunner`](crate::review::ReviewRunner) to set up and activate the
//! BMAD dev agent (Amelia). The activation flow is identical for both:
//!
//! 1. Build a generic preamble with tool usage rules and English override
//! 2. Send `dev.md` as a user message (Zed-style XML context) to trigger BMAD activation
//! 3. The agent processes activation steps: loads `config.yaml`, greets user, shows menu
//! 4. Caller sends a menu command (`DS` for dev, `CR` for review)
//!
//! ## Why a shared module?
//!
//! The dev session and code review session both need the same agent persona with the
//! same activation flow. The only difference is the menu command sent after activation.
//! Extracting the common logic avoids duplication and ensures both flows stay in sync.

use crate::llm::context::ContextBuilder;
use crate::llm::logging::{log_llm_error, log_llm_request, log_llm_response};
use crate::session::state::ChatMessage;

use futures::StreamExt;
use rig::agent::{MultiTurnStreamItem, StreamingPromptHook};
use rig::completion::{Chat, CompletionModel, GetTokenUsage, Message};
use rig::message::Text;
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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

impl Default for ChatHistoryHook {
    fn default() -> Self {
        Self::new()
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
pub async fn activate_agent<A, M>(
    agent: &A,
    project_root: &str,
    agent_relative_path: &str,
    label: &str,
    shutdown: Option<&ShutdownFlag>,
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
        streaming_chat(agent, activation_msg.as_str(), rig_history, shutdown)
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
}
