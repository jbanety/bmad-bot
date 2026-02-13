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
use rig::agent::MultiTurnStreamItem;
use rig::completion::{Chat, CompletionModel, GetTokenUsage, Message};
use rig::message::Text;
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Shared shutdown flag — set to `true` when Ctrl+C or SIGTERM is received.
///
/// Checked cooperatively between streaming chunks and chat turns so that
/// long-running tool-calling loops can be interrupted cleanly.
pub type ShutdownFlag = Arc<AtomicBool>;

/// Maximum tool-call rounds allowed per single prompt in the streaming loop.
const STREAMING_MAX_TURNS: usize = 300;

/// Build the generic agent preamble with tool usage rules and English override.
///
/// This preamble is used as the system prompt for both dev sessions and code
/// review sessions. It does NOT contain the agent persona — that is sent as
/// a user message via [`activate_agent`].
pub fn build_preamble() -> String {
    r#"You are an AI agent operating autonomously in a BMAD workflow environment.

## Tools
You have access to these tools: edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor, plus a built-in think tool for reasoning.

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

## Session Completion Protocol
When you have fully completed your workflow (all tasks done, all tests passing, story file updated, all changes committed), your **final message** MUST end with exactly this structure:

<pr-summary>
<context>
(Summarize what was built and why, referencing the story requirements. Be specific about modules, functions, and patterns used. Use `###` headers)
</context>
<how-to-test>
(Provide concrete commands and steps: specific test names to run, manual verification steps if applicable. Use `###` headers)
</how-to-test>
<additional-info>
(Note design decisions made, dependencies added or removed, tech debt created, migration notes, caveats, or concerns. Use `###` headers)
</additional-info>
</pr-summary>

<<BMAD_JOB_DONE>>

Rules:
- Each `<pr-summary>` section must contain meaningful content — do not leave any section empty.
- `<<BMAD_JOB_DONE>>` MUST appear on its own line, AFTER the `</pr-summary>` closing tag, as the very last thing in the message.
- Do NOT paraphrase, omit, or embed the sentinel mid-sentence. Emit it exactly as shown.
- Do NOT wait to be asked for a PR summary — include it proactively in your final completion message.

## Communication
OVERRIDE: communication_language = English

## Rules
- When the user provides an agent file in <context><files> tags, you MUST fully embody that agent's persona and follow ALL activation instructions exactly as specified.
- NEVER break character until given an exit command.
- Execute activation steps in order — load configuration files via tools, then greet and display the menu.
- Wait for user input after displaying the menu."#
        .to_string()
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
pub async fn streaming_chat<A, M>(
    agent: &A,
    prompt: impl Into<Message> + Send,
    history: Vec<Message>,
    shutdown: Option<&ShutdownFlag>,
) -> Result<String, rig::completion::PromptError>
where
    A: StreamingChat<M, M::StreamingResponse>,
    M: CompletionModel + 'static,
    M::StreamingResponse: Clone + Unpin + GetTokenUsage,
{
    let mut stream = agent
        .stream_chat(prompt, history)
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

    Ok(acc)
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
    let response = streaming_chat(
        agent,
        activation_msg.as_str(),
        rig_history.clone(),
        shutdown,
    )
    .await
    .map_err(|e| {
        log_llm_error(label, 0, &e);
        format!("Agent activation failed: {e}")
    })?;
    log_llm_response(label, 0, &response);

    rig_history.push(Message::user(&activation_msg));
    rig_history.push(Message::assistant(&response));
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
        let preamble = build_preamble();
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
        let preamble = build_preamble();
        assert!(
            preamble.contains("communication_language = English"),
            "Preamble must contain English override"
        );
    }

    #[test]
    fn test_build_preamble_contains_activation_rules() {
        let preamble = build_preamble();
        assert!(preamble.contains("<context><files>"));
        assert!(preamble.contains("activation instructions"));
    }

    #[test]
    fn test_build_preamble_does_not_contain_agent_content() {
        let preamble = build_preamble();
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
        let preamble = build_preamble();
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
        let preamble = build_preamble();
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
}
