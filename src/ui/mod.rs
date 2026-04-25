//! UI module — decoupled terminal output for the daemon.
//!
//! The [`UiHandle`] struct wraps an `Arc<dyn UiRenderer>` and provides
//! convenience methods that delegate to the inner trait. It is `Clone`,
//! `Send`, and `Sync`, designed to be propagated through the pipeline
//! like `ShutdownFlag` and `Arc<McpManager>`.

pub(crate) mod console;
pub(crate) mod null;
pub(crate) mod renderer;

use std::sync::Arc;
use std::time::Duration;

use renderer::UiRenderer;

/// Cheaply-cloneable handle to the active UI renderer.
///
/// Wraps `Arc<dyn UiRenderer>` so it can be shared across async tasks
/// without additional synchronization. All methods delegate directly to
/// the inner [`UiRenderer`] implementation.
#[derive(Clone)]
pub(crate) struct UiHandle(Arc<dyn UiRenderer>);

impl std::fmt::Debug for UiHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiHandle").finish_non_exhaustive()
    }
}

impl UiHandle {
    /// Creates a `UiHandle` backed by [`null::NullRenderer`] (zero I/O).
    ///
    /// Suitable for unit tests and CI environments.
    pub(crate) fn null() -> Self {
        Self(Arc::new(null::NullRenderer))
    }

    /// Creates a `UiHandle` backed by [`console::ConsoleRenderer`].
    ///
    /// Outputs styled text and spinners to stderr via `indicatif` and
    /// `console`. When `plain` is `true`, Unicode glyphs are replaced
    /// with ASCII equivalents and spinner animation is disabled.
    pub(crate) fn console(plain: bool, verbose: bool) -> Self {
        Self(Arc::new(console::ConsoleRenderer::new(plain, verbose)))
    }

    // ── Pipeline events ─────────────────────────────────────────────

    /// A story has started processing.
    pub(crate) fn story_start(&self, key: &str, title: &str) {
        self.0.story_start(key, title);
    }

    /// A story completed successfully.
    pub(crate) fn story_complete(&self, key: &str, pr_url: Option<&str>) {
        self.0.story_complete(key, pr_url);
    }

    /// A story encountered an error.
    pub(crate) fn story_error(&self, key: &str, error: &str) {
        self.0.story_error(key, error);
    }

    /// A story was escalated to a human.
    pub(crate) fn story_escalated(&self, key: &str, reason: &str) {
        self.0.story_escalated(key, reason);
    }

    /// A batch of stories has started processing.
    pub(crate) fn batch_start(&self, count: usize) {
        self.0.batch_start(count);
    }

    /// A batch of stories has completed.
    pub(crate) fn batch_complete(&self, summary: &str) {
        self.0.batch_complete(summary);
    }

    // ── Phase events ────────────────────────────────────────────────

    /// A named phase has started.
    pub(crate) fn phase_start(&self, phase_name: &str) {
        self.0.phase_start(phase_name);
    }

    /// A named phase completed successfully.
    pub(crate) fn phase_complete(&self, phase_name: &str, duration: Duration) {
        self.0.phase_complete(phase_name, duration);
    }

    /// A named phase encountered an error.
    pub(crate) fn phase_error(&self, phase_name: &str, error: &str) {
        self.0.phase_error(phase_name, error);
    }

    // ── Session events ──────────────────────────────────────────────

    /// A chat turn was completed within a session.
    pub(crate) fn chat_turn(&self, turn: u32, summary: &str) {
        self.0.chat_turn(turn, summary);
    }

    /// Agent activation has started.
    pub(crate) fn activation_start(&self) {
        self.0.activation_start();
    }

    /// Agent activation completed.
    pub(crate) fn activation_complete(&self) {
        self.0.activation_complete();
    }

    /// The agent detected story completion.
    pub(crate) fn completion_detected(&self, story_key: &str) {
        self.0.completion_detected(story_key);
    }

    // ── Tool events ─────────────────────────────────────────────────

    /// A tool call was initiated.
    pub(crate) fn tool_call(&self, tool_name: &str, detail: &str) {
        self.0.tool_call(tool_name, detail);
    }

    /// A tool call returned a result.
    pub(crate) fn tool_result(&self, tool_name: &str, detail: &str) {
        self.0.tool_result(tool_name, detail);
    }

    // ── LLM events ──────────────────────────────────────────────────

    /// An LLM request was sent.
    pub(crate) fn llm_request(&self, label: &str, turn: u32) {
        self.0.llm_request(label, turn);
    }

    /// An LLM response was received.
    pub(crate) fn llm_response(&self, label: &str, turn: u32, response_len: usize) {
        self.0.llm_response(label, turn, response_len);
    }

    /// An LLM request failed.
    pub(crate) fn llm_error(&self, label: &str, turn: u32, error: &str) {
        self.0.llm_error(label, turn, error);
    }

    /// An LLM request is being retried.
    pub(crate) fn llm_retry(&self, label: &str, turn: u32, retry_count: u32, delay_secs: f64) {
        self.0.llm_retry(label, turn, retry_count, delay_secs);
    }

    /// Preview of the prompt/message being sent to the LLM (verbose mode only).
    pub(crate) fn llm_request_content(&self, label: &str, turn: u32, preview: &str) {
        self.0.llm_request_content(label, turn, preview);
    }

    /// Preview of the LLM response content (verbose mode only).
    pub(crate) fn llm_response_content(&self, label: &str, turn: u32, preview: &str) {
        self.0.llm_response_content(label, turn, preview);
    }

    // ── Consultation events ───────────────────────────────────────────

    /// A consultation agent has started (sub-phase of an active session).
    pub(crate) fn consultation_start(
        &self,
        consultation_type: &str,
        story_key: &str,
        detail: Option<&str>,
    ) {
        self.0
            .consultation_start(consultation_type, story_key, detail);
    }

    /// A consultation agent completed and produced findings.
    pub(crate) fn consultation_complete(
        &self,
        consultation_type: &str,
        findings_count: usize,
        duration: Duration,
    ) {
        self.0
            .consultation_complete(consultation_type, findings_count, duration);
    }

    /// A consultation agent failed.
    pub(crate) fn consultation_error(&self, consultation_type: &str, error: &str) {
        self.0.consultation_error(consultation_type, error);
    }

    /// The Story Critic updated its persistent memory file.
    pub(crate) fn critic_memory_update(&self, story_key: &str) {
        self.0.critic_memory_update(story_key);
    }

    // ── System events ───────────────────────────────────────────────

    /// The daemon has started.
    pub(crate) fn daemon_start(&self, config_summary: &str) {
        self.0.daemon_start(config_summary);
    }

    /// A poll cycle was executed.
    pub(crate) fn poll_cycle(&self, cycle_num: u32) {
        self.0.poll_cycle(cycle_num);
    }

    /// Stories were found during a poll cycle.
    pub(crate) fn stories_found(&self, count: usize) {
        self.0.stories_found(count);
    }

    /// Crash recovery has started.
    pub(crate) fn crash_recovery_start(&self) {
        self.0.crash_recovery_start();
    }

    /// Crash recovery completed for a story.
    pub(crate) fn crash_recovery_complete(&self, story_key: &str) {
        self.0.crash_recovery_complete(story_key);
    }

    /// A graceful shutdown was requested.
    pub(crate) fn shutdown_requested(&self) {
        self.0.shutdown_requested();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time proof that `UiHandle` is `Send + Sync + Clone`.
    fn assert_send_sync_clone<T: Send + Sync + Clone>() {}

    #[test]
    fn test_ui_handle_is_send_sync_clone() {
        assert_send_sync_clone::<UiHandle>();
    }

    #[test]
    fn test_ui_handle_null_creation_succeeds() {
        let _ui = UiHandle::null();
    }

    #[test]
    fn test_ui_handle_console_creation_does_not_panic() {
        let _ui = UiHandle::console(false, false);
    }

    #[test]
    fn test_null_renderer_all_methods_compile_and_succeed() {
        let ui = UiHandle::null();

        // Pipeline events
        ui.story_start("10-1", "Foundation Story");
        ui.story_complete("10-1", Some("https://example.com/pr/1"));
        ui.story_complete("10-1", None);
        ui.story_error("10-1", "compile error");
        ui.story_escalated("10-1", "ambiguous AC");
        ui.batch_start(3);
        ui.batch_complete("3/3 done");

        // Phase events
        ui.phase_start("Dev Session");
        ui.phase_complete("Dev Session", Duration::from_secs(47));
        ui.phase_error("Code Review", "LLM timeout");

        // Session events
        ui.chat_turn(1, "implemented task 1");
        ui.activation_start();
        ui.activation_complete();
        ui.completion_detected("10-1");

        // Tool events
        ui.tool_call("edit_file", "src/ui/mod.rs");
        ui.tool_result("edit_file", "ok");

        // LLM events
        ui.llm_request("dev", 1);
        ui.llm_response("dev", 1, 4096);
        ui.llm_error("dev", 2, "rate limited");
        ui.llm_retry("dev", 2, 1, 2.0);
        ui.llm_request_content("dev", 1, "preview of request");
        ui.llm_response_content("dev", 1, "preview of response");

        // Consultation events
        ui.consultation_start("adversarial", "13-11", None);
        ui.consultation_complete("adversarial", 5, Duration::from_secs(30));
        ui.consultation_error("critic", "LLM timeout");
        ui.critic_memory_update("13-11");

        // System events
        ui.daemon_start("poll=5m, provider=copilot");
        ui.poll_cycle(1);
        ui.stories_found(2);
        ui.crash_recovery_start();
        ui.crash_recovery_complete("10-1");
        ui.shutdown_requested();
    }

    #[test]
    fn test_null_renderer_implements_ui_renderer_trait_object() {
        // Object safety: can be wrapped in Arc<dyn UiRenderer>
        let _obj: Arc<dyn UiRenderer> = Arc::new(null::NullRenderer);
    }

    #[test]
    fn test_console_renderer_implements_ui_renderer_trait_object() {
        // Object safety: can be wrapped in Arc<dyn UiRenderer>
        let _obj: Arc<dyn UiRenderer> = Arc::new(console::ConsoleRenderer::new(false, false));
    }

    #[test]
    fn test_ui_handle_consultation_events_compile() {
        let ui = UiHandle::null();
        ui.consultation_start("adversarial", "13-11", None);
        ui.consultation_start("review-critic", "13-11", Some("2 decision-needed findings"));
        ui.consultation_complete("adversarial", 5, Duration::from_secs(30));
        ui.consultation_complete("critic", 0, Duration::from_secs(0));
        ui.consultation_error("critic", "LLM timeout");
        ui.critic_memory_update("13-11");
    }

    #[test]
    fn test_ui_handle_clone_shares_inner() {
        let ui1 = UiHandle::null();
        let ui2 = ui1.clone();
        // Both handles work independently
        ui1.daemon_start("a");
        ui2.daemon_start("b");
    }
}
