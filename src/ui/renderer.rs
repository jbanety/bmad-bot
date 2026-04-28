/// Backend-agnostic trait for rendering user-facing terminal output.
///
/// All methods take `&self` (never `&mut self`) and return `()` — UI rendering
/// is fire-and-forget. Implementations must handle their own errors internally
/// (e.g., log via `tracing::debug!`) and never propagate failures to callers.
///
/// The trait is `Send + Sync` and object-safe so it can be wrapped in
/// `Arc<dyn UiRenderer>` and shared across async tasks.
///
/// No `indicatif` or `console` types appear in method signatures — the trait
/// is rendering-backend agnostic. Only primitive types are used: `&str`,
/// `usize`, `u32`, `f64`, `std::time::Duration`, `Option<&str>`.
pub(crate) trait UiRenderer: Send + Sync {
    // ── Pipeline events ─────────────────────────────────────────────

    /// A story has started processing.
    fn story_start(&self, key: &str, title: &str);

    /// A story completed successfully.
    fn story_complete(&self, key: &str, pr_url: Option<&str>);

    /// A story encountered an error.
    fn story_error(&self, key: &str, error: &str);

    /// A story was escalated to a human.
    fn story_escalated(&self, key: &str, reason: &str);

    /// A batch of stories has started processing.
    fn batch_start(&self, count: usize);

    /// A batch of stories has completed.
    fn batch_complete(&self, summary: &str);

    // ── Phase events ────────────────────────────────────────────────

    /// A named phase (e.g., "Dev Session", "Code Review") has started.
    fn phase_start(&self, phase_name: &str);

    /// A named phase completed successfully.
    fn phase_complete(&self, phase_name: &str, duration: std::time::Duration);

    /// A named phase encountered an error.
    fn phase_error(&self, phase_name: &str, error: &str);

    // ── Session events ──────────────────────────────────────────────

    /// A chat turn was completed within a session.
    fn chat_turn(&self, turn: u32, summary: &str);

    /// Agent activation has started.
    fn activation_start(&self);

    /// Agent activation completed.
    fn activation_complete(&self);

    /// The agent detected story completion.
    fn completion_detected(&self, story_key: &str);

    // ── Tool events ─────────────────────────────────────────────────

    /// A tool call was initiated.
    fn tool_call(&self, tool_name: &str, detail: &str);

    /// A tool call returned a result.
    fn tool_result(&self, tool_name: &str, detail: &str);

    // ── LLM events ──────────────────────────────────────────────────

    /// An LLM request was sent.
    fn llm_request(&self, label: &str, turn: u32);

    /// An LLM response was received.
    fn llm_response(&self, label: &str, turn: u32, response_len: usize);

    /// An LLM request failed.
    fn llm_error(&self, label: &str, turn: u32, error: &str);

    /// An LLM request is being retried.
    fn llm_retry(&self, label: &str, turn: u32, retry_count: u32, delay_secs: f64);

    /// Preview of the prompt/message being sent to the LLM (verbose mode only).
    fn llm_request_content(&self, _label: &str, _turn: u32, _preview: &str) {}

    /// Preview of the LLM response content (verbose mode only).
    fn llm_response_content(&self, _label: &str, _turn: u32, _preview: &str) {}

    // ── Consultation events ───────────────────────────────────────────

    /// A consultation agent has started (sub-phase of an active session).
    fn consultation_start(&self, consultation_type: &str, story_key: &str, detail: Option<&str>);

    /// A consultation agent completed and produced findings.
    fn consultation_complete(
        &self,
        consultation_type: &str,
        findings_count: usize,
        duration: std::time::Duration,
    );

    /// A consultation agent failed.
    fn consultation_error(&self, consultation_type: &str, error: &str);

    /// The Story Critic updated its persistent memory file.
    fn critic_memory_update(&self, story_key: &str);

    // ── System events ───────────────────────────────────────────────

    /// The daemon has started.
    fn daemon_start(&self, config_summary: &str);

    /// A poll cycle was executed.
    fn poll_cycle(&self, cycle_num: u32);

    /// Stories were found during a poll cycle.
    fn stories_found(&self, count: usize);

    /// Crash recovery has started.
    fn crash_recovery_start(&self);

    /// Crash recovery completed for a story.
    fn crash_recovery_complete(&self, story_key: &str);

    /// Rate limit status update from SDK provider.
    fn rate_limit_status(&self, _resets_at: Option<u64>, _limit_type: &str, _percent_used: Option<f64>) {}

    /// A graceful shutdown was requested.
    fn shutdown_requested(&self);
}

#[cfg(test)]
mod tests {
    use super::UiRenderer;
    use std::sync::Arc;

    /// Minimal no-op renderer used only within this module's tests.
    struct TestRenderer;

    impl UiRenderer for TestRenderer {
        fn story_start(&self, _key: &str, _title: &str) {}
        fn story_complete(&self, _key: &str, _pr_url: Option<&str>) {}
        fn story_error(&self, _key: &str, _error: &str) {}
        fn story_escalated(&self, _key: &str, _reason: &str) {}
        fn batch_start(&self, _count: usize) {}
        fn batch_complete(&self, _summary: &str) {}
        fn phase_start(&self, _phase_name: &str) {}
        fn phase_complete(&self, _phase_name: &str, _duration: std::time::Duration) {}
        fn phase_error(&self, _phase_name: &str, _error: &str) {}
        fn chat_turn(&self, _turn: u32, _summary: &str) {}
        fn activation_start(&self) {}
        fn activation_complete(&self) {}
        fn completion_detected(&self, _story_key: &str) {}
        fn tool_call(&self, _tool_name: &str, _detail: &str) {}
        fn tool_result(&self, _tool_name: &str, _detail: &str) {}
        fn llm_request(&self, _label: &str, _turn: u32) {}
        fn llm_response(&self, _label: &str, _turn: u32, _response_len: usize) {}
        fn llm_error(&self, _label: &str, _turn: u32, _error: &str) {}
        fn llm_retry(&self, _label: &str, _turn: u32, _retry_count: u32, _delay_secs: f64) {}
        fn consultation_start(
            &self,
            _consultation_type: &str,
            _story_key: &str,
            _detail: Option<&str>,
        ) {
        }
        fn consultation_complete(
            &self,
            _consultation_type: &str,
            _findings_count: usize,
            _duration: std::time::Duration,
        ) {
        }
        fn consultation_error(&self, _consultation_type: &str, _error: &str) {}
        fn critic_memory_update(&self, _story_key: &str) {}
        fn daemon_start(&self, _config_summary: &str) {}
        fn poll_cycle(&self, _cycle_num: u32) {}
        fn stories_found(&self, _count: usize) {}
        fn crash_recovery_start(&self) {}
        fn crash_recovery_complete(&self, _story_key: &str) {}
        fn shutdown_requested(&self) {}
    }

    /// Compile-time proof that `UiRenderer` is object-safe.
    #[test]
    fn test_ui_renderer_is_object_safe() {
        let _obj: Arc<dyn UiRenderer> = Arc::new(TestRenderer);
    }

    /// Compile-time proof that `Arc<dyn UiRenderer>` is `Send + Sync`.
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_ui_renderer_arc_is_send_sync() {
        assert_send_sync::<Arc<dyn UiRenderer>>();
    }

    /// Verify the trait can be implemented with all `&self` methods (never `&mut self`).
    #[test]
    fn test_ui_renderer_all_methods_take_shared_ref() {
        // If any method required &mut self, the immutable binding below would
        // fail to compile — this test enforces the &self contract at the trait level.
        let r: &dyn UiRenderer = &TestRenderer;
        r.story_start("k", "t");
        r.story_complete("k", None);
        r.story_error("k", "e");
        r.story_escalated("k", "r");
        r.batch_start(1);
        r.batch_complete("s");
        r.phase_start("p");
        r.phase_complete("p", std::time::Duration::from_secs(1));
        r.phase_error("p", "e");
        r.chat_turn(1, "s");
        r.activation_start();
        r.activation_complete();
        r.completion_detected("k");
        r.tool_call("t", "d");
        r.tool_result("t", "d");
        r.llm_request("l", 1);
        r.llm_response("l", 1, 100);
        r.llm_error("l", 1, "e");
        r.llm_retry("l", 1, 1, 1.0);
        r.llm_request_content("l", 1, "preview");
        r.llm_response_content("l", 1, "preview");
        r.consultation_start("adversarial", "13-11", None);
        r.consultation_complete("adversarial", 5, std::time::Duration::from_secs(30));
        r.consultation_error("critic", "LLM timeout");
        r.critic_memory_update("13-11");
        r.daemon_start("c");
        r.poll_cycle(1);
        r.stories_found(1);
        r.crash_recovery_start();
        r.crash_recovery_complete("k");
        r.shutdown_requested();
    }
}
