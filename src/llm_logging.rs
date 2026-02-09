//! LLM payload debug logging — structured tracing for request/response payloads.
//!
//! All functions use the dedicated target `bmad_bot::llm` so payloads can be
//! filtered independently:
//!
//! ```sh
//! RUST_LOG=bmad_bot::llm=debug cargo run -- start
//! ```
//!
//! - **`debug`** — logs the user message sent, full response text, and errors.
//! - **`trace`** — logs the complete chat history (each message role + content).
//!
//! **Cheap when disabled:** Every function has an early `tracing::enabled!`
//! guard. When the log level is not active, the cost is a function call plus
//! a single atomic load (~1ns) — no formatting, no allocation, no real work.

use crate::session::state::ChatMessage;

/// Target used for all LLM payload log events.
///
/// Filter with `RUST_LOG=bmad_bot::llm=debug` or `=trace`.
const LLM_TARGET: &str = "bmad_bot::llm";

/// Log an outgoing LLM request at `DEBUG` level.
///
/// Call this **before** `agent.chat()`.
///
/// # Arguments
/// - `label` — context identifier (e.g. `"dev-session"`, `"supervisor"`, `"code-review"`)
/// - `turn` — current conversation turn number
/// - `message` — the user message being sent
/// - `history_len` — number of messages in the chat history
pub fn log_llm_request(label: &str, turn: usize, message: &str, history_len: usize) {
    if !tracing::enabled!(target: LLM_TARGET, tracing::Level::DEBUG) {
        return;
    }

    tracing::debug!(
        target: LLM_TARGET,
        action = "llm_request",
        label = %label,
        turn = %turn,
        history_len = %history_len,
        payload = %message,
        "→ LLM request"
    );
}

/// Log a successful LLM response at `DEBUG` level.
///
/// Call this **after** a successful `agent.chat()`.
///
/// # Arguments
/// - `label` — context identifier
/// - `turn` — current conversation turn number
/// - `response` — the full response text from the LLM
pub fn log_llm_response(label: &str, turn: usize, response: &str) {
    if !tracing::enabled!(target: LLM_TARGET, tracing::Level::DEBUG) {
        return;
    }

    tracing::debug!(
        target: LLM_TARGET,
        action = "llm_response",
        label = %label,
        turn = %turn,
        response_len = %response.len(),
        payload = %response,
        "← LLM response"
    );
}

/// Log an LLM error at `DEBUG` level.
///
/// Call this when `agent.chat()` returns `Err`.
///
/// # Arguments
/// - `label` — context identifier
/// - `turn` — current conversation turn number
/// - `error` — the error (anything implementing `Display`)
pub fn log_llm_error(label: &str, turn: usize, error: &dyn std::fmt::Display) {
    if !tracing::enabled!(target: LLM_TARGET, tracing::Level::DEBUG) {
        return;
    }

    tracing::debug!(
        target: LLM_TARGET,
        action = "llm_error",
        label = %label,
        turn = %turn,
        error = %error,
        "← LLM error"
    );
}

/// Log the full chat history at `TRACE` level.
///
/// This is expensive and verbose — only emitted when `RUST_LOG=bmad_bot::llm=trace`.
/// Call this before `agent.chat()` when you need to inspect the complete history
/// being sent to the provider.
///
/// # Arguments
/// - `label` — context identifier
/// - `turn` — current conversation turn number
/// - `history` — the chat history as `&[ChatMessage]`
pub fn log_llm_history(label: &str, turn: usize, history: &[ChatMessage]) {
    if !tracing::enabled!(target: LLM_TARGET, tracing::Level::TRACE) {
        return;
    }

    for (i, msg) in history.iter().enumerate() {
        tracing::trace!(
            target: LLM_TARGET,
            action = "llm_history",
            label = %label,
            turn = %turn,
            index = %i,
            role = %msg.role,
            content_len = %msg.content.len(),
            content = %msg.content,
            "  history[{i}]"
        );
    }
}

/// Log a compact summary of chat history at `TRACE` level.
///
/// Unlike [`log_llm_history`], this logs a single event with just
/// roles and content lengths — useful for a quick overview without
/// flooding the log with full message contents.
///
/// # Arguments
/// - `label` — context identifier
/// - `turn` — current conversation turn number
/// - `history` — the chat history as `&[ChatMessage]`
pub fn log_llm_history_summary(label: &str, turn: usize, history: &[ChatMessage]) {
    if !tracing::enabled!(target: LLM_TARGET, tracing::Level::TRACE) {
        return;
    }

    let summary: Vec<String> = history
        .iter()
        .enumerate()
        .map(|(i, msg)| format!("[{i}] {}: {} chars", msg.role, msg.content.len()))
        .collect();

    tracing::trace!(
        target: LLM_TARGET,
        action = "llm_history_summary",
        label = %label,
        turn = %turn,
        total_messages = %history.len(),
        messages = %summary.join(" | "),
        "Chat history summary"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the LLM_TARGET constant matches the module convention.
    #[test]
    fn test_llm_target_is_bmad_bot_llm() {
        assert_eq!(LLM_TARGET, "bmad_bot::llm");
    }

    /// Ensure log functions don't panic with empty inputs.
    #[test]
    fn test_log_llm_request_empty_message() {
        log_llm_request("test", 0, "", 0);
    }

    #[test]
    fn test_log_llm_response_empty_response() {
        log_llm_response("test", 0, "");
    }

    #[test]
    fn test_log_llm_error_with_string_error() {
        let err = "something went wrong";
        log_llm_error("test", 0, &err);
    }

    #[test]
    fn test_log_llm_error_with_io_error() {
        let err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout");
        log_llm_error("test", 1, &err);
    }

    #[test]
    fn test_log_llm_history_empty() {
        log_llm_history("test", 0, &[]);
    }

    #[test]
    fn test_log_llm_history_with_messages() {
        let history = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "DS".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Starting dev story workflow...".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Continue.".to_string(),
            },
        ];
        log_llm_history("test", 1, &history);
    }

    #[test]
    fn test_log_llm_history_summary_empty() {
        log_llm_history_summary("test", 0, &[]);
    }

    #[test]
    fn test_log_llm_history_summary_with_messages() {
        let history = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "DS".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "I'll start implementing story 1-1.".to_string(),
            },
        ];
        log_llm_history_summary("test", 0, &history);
    }

    /// Ensure log functions don't panic with large payloads.
    #[test]
    fn test_log_llm_request_large_payload() {
        let big = "x".repeat(100_000);
        log_llm_request("stress", 999, &big, 500);
    }

    #[test]
    fn test_log_llm_response_large_payload() {
        let big = "y".repeat(100_000);
        log_llm_response("stress", 999, &big);
    }

    /// Verify unicode content doesn't cause issues.
    #[test]
    fn test_log_llm_request_unicode() {
        log_llm_request("unicode", 0, "こんにちは 🚀 émoji", 1);
    }

    #[test]
    fn test_log_llm_response_unicode() {
        log_llm_response("unicode", 0, "Réponse avec des accents é è ê ë");
    }

    #[test]
    fn test_log_llm_history_unicode() {
        let history = vec![ChatMessage {
            role: "user".to_string(),
            content: "日本語テスト".to_string(),
        }];
        log_llm_history("unicode", 0, &history);
    }

    /// Verify multiline content is handled.
    #[test]
    fn test_log_llm_request_multiline() {
        let msg = "line1\nline2\nline3\n";
        log_llm_request("multiline", 0, msg, 0);
    }

    #[test]
    fn test_log_llm_response_multiline() {
        let resp = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
        log_llm_response("multiline", 0, resp);
    }

    /// Verify various label values work.
    #[test]
    fn test_log_functions_with_all_labels() {
        let labels = [
            "dev-session",
            "dev-recovery",
            "dev-summarize",
            "supervisor",
            "code-review",
        ];
        for label in labels {
            log_llm_request(label, 0, "test", 0);
            log_llm_response(label, 0, "ok");
            log_llm_error(label, 0, &"err");
            log_llm_history(label, 0, &[]);
            log_llm_history_summary(label, 0, &[]);
        }
    }
}
