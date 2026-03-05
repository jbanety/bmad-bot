/// No-op renderer that performs zero I/O.
///
/// Used in unit tests and CI environments to avoid terminal output pollution.
/// Every `UiRenderer` method is an empty body returning `()`.
pub(crate) struct NullRenderer;

impl super::renderer::UiRenderer for NullRenderer {
    // ── Pipeline events ─────────────────────────────────────────────

    fn story_start(&self, _key: &str, _title: &str) {}
    fn story_complete(&self, _key: &str, _pr_url: Option<&str>) {}
    fn story_error(&self, _key: &str, _error: &str) {}
    fn story_escalated(&self, _key: &str, _reason: &str) {}
    fn batch_start(&self, _count: usize) {}
    fn batch_complete(&self, _summary: &str) {}

    // ── Phase events ────────────────────────────────────────────────

    fn phase_start(&self, _phase_name: &str) {}
    fn phase_complete(&self, _phase_name: &str, _duration: std::time::Duration) {}
    fn phase_error(&self, _phase_name: &str, _error: &str) {}

    // ── Session events ──────────────────────────────────────────────

    fn chat_turn(&self, _turn: u32, _summary: &str) {}
    fn activation_start(&self) {}
    fn activation_complete(&self) {}
    fn completion_detected(&self, _story_key: &str) {}

    // ── Tool events ─────────────────────────────────────────────────

    fn tool_call(&self, _tool_name: &str, _detail: &str) {}
    fn tool_result(&self, _tool_name: &str, _detail: &str) {}

    // ── LLM events ──────────────────────────────────────────────────

    fn llm_request(&self, _label: &str, _turn: u32) {}
    fn llm_response(&self, _label: &str, _turn: u32, _response_len: usize) {}
    fn llm_error(&self, _label: &str, _turn: u32, _error: &str) {}
    fn llm_retry(&self, _label: &str, _turn: u32, _retry_count: u32, _delay_secs: f64) {}

    // ── System events ───────────────────────────────────────────────

    fn daemon_start(&self, _config_summary: &str) {}
    fn poll_cycle(&self, _cycle_num: u32) {}
    fn stories_found(&self, _count: usize) {}
    fn crash_recovery_start(&self) {}
    fn crash_recovery_complete(&self, _story_key: &str) {}
    fn shutdown_requested(&self) {}
}

#[cfg(test)]
mod tests {
    use super::super::renderer::UiRenderer;
    use super::NullRenderer;
    use std::sync::Arc;
    use std::time::Duration;

    /// Compile-time proof that `NullRenderer` can be used as a trait object.
    #[test]
    fn test_null_renderer_is_object_safe() {
        let _obj: Arc<dyn UiRenderer> = Arc::new(NullRenderer);
    }

    /// Compile-time proof that `NullRenderer` is `Send + Sync`.
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_null_renderer_is_send_sync() {
        assert_send_sync::<NullRenderer>();
    }

    /// Verify all methods can be called via an immutable shared reference,
    /// enforcing the `&self` contract directly on `NullRenderer`.
    #[test]
    fn test_null_renderer_all_methods_via_shared_ref() {
        let r: &dyn UiRenderer = &NullRenderer;

        // Pipeline
        r.story_start("10-1", "Foundation");
        r.story_complete("10-1", Some("https://example.com/pr/1"));
        r.story_complete("10-1", None);
        r.story_error("10-1", "err");
        r.story_escalated("10-1", "reason");
        r.batch_start(2);
        r.batch_complete("2/2 done");

        // Phase
        r.phase_start("Dev Session");
        r.phase_complete("Dev Session", Duration::from_secs(10));
        r.phase_error("Code Review", "timeout");

        // Session
        r.chat_turn(1, "summary");
        r.activation_start();
        r.activation_complete();
        r.completion_detected("10-1");

        // Tool
        r.tool_call("edit_file", "src/lib.rs");
        r.tool_result("edit_file", "ok");

        // LLM
        r.llm_request("dev", 1);
        r.llm_response("dev", 1, 1024);
        r.llm_error("dev", 2, "rate limited");
        r.llm_retry("dev", 2, 1, 1.5);

        // System
        r.daemon_start("poll=5m");
        r.poll_cycle(1);
        r.stories_found(3);
        r.crash_recovery_start();
        r.crash_recovery_complete("10-1");
        r.shutdown_requested();
    }
}
