use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

use super::renderer::UiRenderer;

/// Console-based renderer using `indicatif` spinners and `console` colors.
///
/// Uses `MultiProgress` for managing concurrent spinners and `console::style()`
/// for colored and styled text output. All output is directed to stderr to keep
/// stdout clean and avoid interference with the file tracing layer.
///
/// # Visual Vocabulary
///
/// - `●` (green)  — completed action
/// - `◉` (cyan, animated) — in-progress action (spinner)
/// - `└` (dim)    — sub-detail / child event
/// - `►` (dim)    — outgoing call / request initiated
/// - `✗` (red)    — error
/// - `⚠` (yellow) — warning / escalation
///
/// # Interior Mutability
///
/// All `UiRenderer` methods take `&self`, so mutable state (the spinner map)
/// is protected by `std::sync::Mutex`. The `MultiProgress` instance itself
/// is already thread-safe.
pub(crate) struct ConsoleRenderer {
    /// Thread-safe multi-progress bar manager (renders to stderr).
    multi: MultiProgress,
    /// Active phase spinners indexed by phase name.
    spinners: Mutex<HashMap<String, ProgressBar>>,
}

impl ConsoleRenderer {
    /// Creates a new `ConsoleRenderer`.
    ///
    /// Config-based initialization (`ui_mode`, TTY detection) is deferred
    /// to Story 10.2 — this constructor takes no parameters.
    pub(crate) fn new() -> Self {
        let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr());
        Self {
            multi,
            spinners: Mutex::new(HashMap::new()),
        }
    }

    /// Creates a new spinner `ProgressBar`, adds it to `MultiProgress`, and
    /// returns it. Uses the standard cyan `◉` animated style.
    fn create_spinner(&self, message: String) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        let pb = self.multi.add(pb);
        let spinner_style = ProgressStyle::with_template("  {spinner} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner());
        pb.set_style(spinner_style);
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message(format!("{} {}", style("◉").cyan(), message));
        pb
    }

    /// Prints a styled line to the `MultiProgress` context so it does not
    /// collide with active spinners.
    fn println(&self, line: &str) {
        if let Err(e) = self.multi.println(line) {
            tracing::debug!("ConsoleRenderer: println failed: {e}");
        }
    }

    /// Finishes and removes a tracked phase spinner by name.
    /// Returns `Some(ProgressBar)` if one was active, `None` otherwise.
    fn take_spinner(&self, phase_name: &str) -> Option<ProgressBar> {
        match self.spinners.lock() {
            Ok(mut map) => map.remove(phase_name),
            Err(poisoned) => {
                tracing::debug!("ConsoleRenderer: spinner lock poisoned: {poisoned}");
                None
            }
        }
    }

    /// Stores a spinner for a given phase name.
    ///
    /// If a spinner already exists for `phase_name` (duplicate `phase_start`),
    /// the previous spinner is finished and cleared before the new one is stored,
    /// preventing orphaned spinners in `MultiProgress`.
    fn store_spinner(&self, phase_name: String, pb: ProgressBar) {
        match self.spinners.lock() {
            Ok(mut map) => {
                // H3 fix: finish any existing spinner for this phase before replacing it
                if let Some(existing) = map.remove(&phase_name) {
                    existing.finish_and_clear();
                    tracing::debug!(
                        "ConsoleRenderer: phase_start called twice for '{phase_name}', \
                         previous spinner cleared"
                    );
                }
                map.insert(phase_name, pb);
            }
            Err(poisoned) => {
                tracing::debug!("ConsoleRenderer: spinner lock poisoned: {poisoned}");
            }
        }
    }
}

impl UiRenderer for ConsoleRenderer {
    // ── Pipeline events ─────────────────────────────────────────────

    fn story_start(&self, key: &str, title: &str) {
        self.println(&format!(
            "{} Story {} — {}",
            style("◉").cyan(),
            style(key).bold(),
            title,
        ));
    }

    fn story_complete(&self, key: &str, pr_url: Option<&str>) {
        let suffix = match pr_url {
            Some(url) => format!(" → {url}"),
            None => String::new(),
        };
        self.println(&format!(
            "{} Story {} complete{}",
            style("●").green(),
            style(key).bold(),
            suffix,
        ));
    }

    fn story_error(&self, key: &str, error: &str) {
        self.println(&format!(
            "{} Story {} — {}",
            style("✗").red(),
            style(key).bold(),
            error,
        ));
    }

    fn story_escalated(&self, key: &str, reason: &str) {
        self.println(&format!(
            "{} Story {} escalated — {}",
            style("⚠").yellow(),
            style(key).bold(),
            reason,
        ));
    }

    fn batch_start(&self, count: usize) {
        self.println(&format!(
            "{} Batch started — {} stories",
            style("◉").cyan(),
            count,
        ));
    }

    fn batch_complete(&self, summary: &str) {
        self.println(&format!(
            "{} Batch complete — {}",
            style("●").green(),
            summary,
        ));
    }

    // ── Phase events ────────────────────────────────────────────────

    fn phase_start(&self, phase_name: &str) {
        let pb = self.create_spinner(phase_name.to_string());
        self.store_spinner(phase_name.to_string(), pb);
    }

    fn phase_complete(&self, phase_name: &str, duration: Duration) {
        if let Some(pb) = self.take_spinner(phase_name) {
            pb.finish_and_clear();
        }
        self.println(&format!(
            "  {} {} [{}s]",
            style("●").green(),
            phase_name,
            duration.as_secs(),
        ));
    }

    fn phase_error(&self, phase_name: &str, error: &str) {
        if let Some(pb) = self.take_spinner(phase_name) {
            pb.finish_and_clear();
        }
        self.println(&format!(
            "  {} {} — {}",
            style("✗").red(),
            phase_name,
            error,
        ));
    }

    // ── Session events ──────────────────────────────────────────────

    fn chat_turn(&self, turn: u32, summary: &str) {
        self.println(&format!(
            "    {} turn {} — {}",
            style("└").dim(),
            turn,
            summary,
        ));
    }

    fn activation_start(&self) {
        self.println(&format!("    {} Agent activation…", style("◉").cyan()));
    }

    fn activation_complete(&self) {
        self.println(&format!("    {} Agent activated", style("●").green()));
    }

    fn completion_detected(&self, story_key: &str) {
        self.println(&format!(
            "    {} Completion detected for {}",
            style("●").green(),
            style(story_key).bold(),
        ));
    }

    // ── Tool events ─────────────────────────────────────────────────

    /// A tool call was initiated — uses `►` (outgoing call) to distinguish
    /// from the returning result.
    fn tool_call(&self, tool_name: &str, detail: &str) {
        self.println(&format!(
            "      {} {} {}",
            style("►").dim(),
            style(tool_name).dim().bold(),
            style(detail).dim(),
        ));
    }

    /// A tool call returned a result — uses `●` (green, dim) to signal success
    /// and distinguish from the originating `tool_call`.
    fn tool_result(&self, tool_name: &str, detail: &str) {
        self.println(&format!(
            "      {} {} {}",
            style("●").green().dim(),
            style(tool_name).dim().bold(),
            style(detail).dim(),
        ));
    }

    // ── LLM events ──────────────────────────────────────────────────

    fn llm_request(&self, label: &str, turn: u32) {
        self.println(&format!("    {} {} turn {}", style("►").dim(), label, turn,));
    }

    fn llm_response(&self, label: &str, turn: u32, response_len: usize) {
        self.println(&format!(
            "    {} {} turn {} — {} bytes",
            style("●").green().dim(),
            label,
            turn,
            response_len,
        ));
    }

    fn llm_error(&self, label: &str, turn: u32, error: &str) {
        self.println(&format!(
            "    {} {} turn {} — {}",
            style("✗").red(),
            label,
            turn,
            error,
        ));
    }

    fn llm_retry(&self, label: &str, turn: u32, retry_count: u32, delay_secs: f64) {
        self.println(&format!(
            "    {} {} turn {} — retry {} in {:.1}s",
            style("⚠").yellow(),
            label,
            turn,
            retry_count,
            delay_secs,
        ));
    }

    // ── System events ───────────────────────────────────────────────

    fn daemon_start(&self, config_summary: &str) {
        self.println(&format!(
            "{} Daemon started — {}",
            style("●").green(),
            config_summary,
        ));
    }

    /// M3 fix: `poll_cycle` is a system-level event — uses `└` dim as a
    /// lightweight heartbeat marker (sub-detail of the running daemon), not
    /// `●` which signals a completed discrete action.
    fn poll_cycle(&self, cycle_num: u32) {
        self.println(&format!("{} Poll #{cycle_num}", style("└").dim(),));
    }

    fn stories_found(&self, count: usize) {
        self.println(&format!(
            "{} Found {} eligible stories",
            style("●").green(),
            count,
        ));
    }

    fn crash_recovery_start(&self) {
        self.println(&format!(
            "{} Crash recovery initiated…",
            style("⚠").yellow(),
        ));
    }

    fn crash_recovery_complete(&self, story_key: &str) {
        self.println(&format!(
            "{} Crash recovery complete for {}",
            style("●").green(),
            style(story_key).bold(),
        ));
    }

    fn shutdown_requested(&self) {
        self.println(&format!(
            "{} Shutdown requested — finishing current work…",
            style("⚠").yellow(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time proof that `ConsoleRenderer` is `Send + Sync` (M2).
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_console_renderer_is_send_sync() {
        assert_send_sync::<ConsoleRenderer>();
    }

    #[test]
    fn test_console_renderer_new_does_not_panic() {
        let _r = ConsoleRenderer::new();
    }

    /// H2: exercise every `UiRenderer` method on a real `ConsoleRenderer`
    /// to catch any internal panic (poisoned lock, bad ProgressStyle template, etc.).
    #[test]
    fn test_console_renderer_all_methods_do_not_panic() {
        let r = ConsoleRenderer::new();

        // Pipeline
        r.story_start("10-1", "Foundation");
        r.story_complete("10-1", Some("https://example.com/pr/1"));
        r.story_complete("10-1", None);
        r.story_error("10-1", "compile error");
        r.story_escalated("10-1", "ambiguous AC");
        r.batch_start(3);
        r.batch_complete("3/3 done");

        // Phase (normal lifecycle)
        r.phase_start("Dev Session");
        r.phase_complete("Dev Session", Duration::from_secs(47));

        r.phase_start("Code Review");
        r.phase_error("Code Review", "LLM timeout");

        // Session
        r.chat_turn(1, "implemented task 1");
        r.activation_start();
        r.activation_complete();
        r.completion_detected("10-1");

        // Tool — verify both are visually distinct (no panic)
        r.tool_call("edit_file", "src/ui/mod.rs");
        r.tool_result("edit_file", "ok");

        // LLM
        r.llm_request("dev", 1);
        r.llm_response("dev", 1, 4096);
        r.llm_error("dev", 2, "rate limited");
        r.llm_retry("dev", 2, 1, 2.0);

        // System
        r.daemon_start("poll=5m");
        r.poll_cycle(1);
        r.stories_found(2);
        r.crash_recovery_start();
        r.crash_recovery_complete("10-1");
        r.shutdown_requested();
    }

    /// H3: calling `phase_start` twice for the same name must not leak the
    /// first spinner — the previous one must be finished and removed.
    #[test]
    fn test_phase_start_duplicate_clears_previous_spinner() {
        let r = ConsoleRenderer::new();

        r.phase_start("Dev Session");
        // Verify one spinner is tracked
        {
            let map = r.spinners.lock().expect("lock not poisoned");
            assert_eq!(map.len(), 1, "one spinner after first phase_start");
        }

        // Second call for the same phase — previous spinner must be cleared
        r.phase_start("Dev Session");
        {
            let map = r.spinners.lock().expect("lock not poisoned");
            assert_eq!(
                map.len(),
                1,
                "still one spinner after duplicate phase_start (no leak)"
            );
        }

        // Normal teardown
        r.phase_complete("Dev Session", Duration::from_secs(1));
        {
            let map = r.spinners.lock().expect("lock not poisoned");
            assert!(map.is_empty(), "spinner removed after phase_complete");
        }
    }

    /// H3: `phase_complete` on a phase that was never started must not panic.
    #[test]
    fn test_phase_complete_without_start_does_not_panic() {
        let r = ConsoleRenderer::new();
        r.phase_complete("Ghost Phase", Duration::from_secs(0));
    }

    /// H3: `phase_error` on a phase that was never started must not panic.
    #[test]
    fn test_phase_error_without_start_does_not_panic() {
        let r = ConsoleRenderer::new();
        r.phase_error("Ghost Phase", "never started");
    }
}

