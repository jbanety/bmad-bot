use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

use super::renderer::UiRenderer;

/// Formats a `Duration` into a human-readable elapsed-time string.
///
/// - Under 60 s: `"47s"`
/// - 1–60 min:   `"3m 12s"`
/// - Over 60 min: `"1h 23m"`
fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    if total_secs < 60 {
        format!("{total_secs}s")
    } else if total_secs < 3600 {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{mins}m {secs}s")
    } else {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        format!("{hours}h {mins}m")
    }
}

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
    /// The `bool` indicates whether this is a sub-phase (nested inside another phase).
    spinners: Mutex<HashMap<String, (ProgressBar, bool)>>,
    /// When `true`, use ASCII fallback glyphs and disable spinner animation.
    plain_mode: bool,
    /// When `true`, display truncated content previews for LLM exchanges.
    verbose: bool,
    /// Current provider/model tag for spinner updates.
    session_info_suffix: Mutex<String>,
    /// Current rate-limit status suffix for spinner updates.
    rate_limit_suffix: Mutex<String>,
    /// Base phase name of the active spinner (unstyled, for rebuilding).
    active_phase_name: Mutex<String>,
}

impl ConsoleRenderer {
    /// Creates a new `ConsoleRenderer`.
    ///
    /// When `plain` is `true`, animated spinners are disabled and Unicode
    /// glyphs are replaced with ASCII equivalents. Color disabling is
    /// handled externally via `console::set_colors_enabled(false)`.
    pub(crate) fn new(plain: bool, verbose: bool) -> Self {
        let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr());
        Self {
            multi,
            spinners: Mutex::new(HashMap::new()),
            plain_mode: plain,
            verbose,
            session_info_suffix: Mutex::new(String::new()),
            rate_limit_suffix: Mutex::new(String::new()),
            active_phase_name: Mutex::new(String::new()),
        }
    }

    /// Creates a new spinner `ProgressBar`, adds it to `MultiProgress`, and
    /// returns it. In fancy mode uses the animated cyan `◉`; in plain mode
    /// uses a static `...` prefix with no tick animation.
    ///
    /// When `sub` is `true`, the spinner is indented by 4 spaces instead of 2
    /// to visually nest under a parent phase.
    fn create_spinner(&self, message: String, sub: bool) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        let pb = self.multi.add(pb);
        let indent = if sub { "    " } else { "  " };
        let template = format!("{indent}{{spinner}} {{msg}}");
        let spinner_style = ProgressStyle::with_template(&template)
            .unwrap_or_else(|_| ProgressStyle::default_spinner());
        pb.set_style(spinner_style);
        if self.plain_mode {
            pb.set_draw_target(ProgressDrawTarget::hidden());
            pb.set_message(format!("... {message}"));
        } else {
            pb.enable_steady_tick(Duration::from_millis(100));
            pb.set_message(format!("{} {message}", style("◉").cyan()));
        }
        pb
    }

    fn update_active_spinner_combined(&self) {
        let info = self
            .session_info_suffix
            .lock()
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.clone())
            .unwrap_or_default();
        let rate = self
            .rate_limit_suffix
            .lock()
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.clone())
            .unwrap_or_default();

        let suffix = match (info.is_empty(), rate.is_empty()) {
            (true, true) => return,
            (false, true) => info,
            (true, false) => rate,
            (false, false) => format!("{info} {rate}"),
        };

        let base = self
            .active_phase_name
            .lock()
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.clone());

        let Some(base) = base else { return };

        let spinners = self.spinners.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((pb, _)) = spinners.values().last() {
            let styled = if self.plain_mode {
                suffix
            } else {
                style(suffix).cyan().dim().to_string()
            };
            pb.set_message(format!("{} {base} {styled}", style("◉").cyan()));
        }
    }

    /// Returns the appropriate glyph for the current mode.
    ///
    /// In fancy mode, applies `console::style()` formatting. In plain mode,
    /// returns the ASCII equivalent without styling.
    fn glyph_ok(&self) -> String {
        if self.plain_mode {
            "[ok]".to_string()
        } else {
            style("●").green().to_string()
        }
    }

    fn glyph_err(&self) -> String {
        if self.plain_mode {
            "[ERR]".to_string()
        } else {
            style("✗").red().to_string()
        }
    }

    fn glyph_warn(&self) -> String {
        if self.plain_mode {
            "[WARN]".to_string()
        } else {
            style("⚠").yellow().to_string()
        }
    }

    fn glyph_progress(&self) -> String {
        if self.plain_mode {
            "...".to_string()
        } else {
            style("◉").cyan().to_string()
        }
    }

    fn glyph_sub(&self) -> String {
        if self.plain_mode {
            "  ".to_string()
        } else {
            style("└").dim().to_string()
        }
    }

    /// Returns `└` (dim) in fancy mode, `\-` in plain mode — used for
    /// tool result sub-lines beneath the tool call line.
    fn glyph_sub_tree(&self) -> String {
        if self.plain_mode {
            "\\-".to_string()
        } else {
            style("└").dim().to_string()
        }
    }

    fn glyph_arrow_out(&self) -> String {
        if self.plain_mode {
            "->".to_string()
        } else {
            style("→").dim().to_string()
        }
    }

    fn glyph_arrow_in(&self) -> String {
        if self.plain_mode {
            "<-".to_string()
        } else {
            style("←").dim().to_string()
        }
    }

    /// Returns a human-friendly capitalized name for a tool.
    ///
    /// Used by `tool_call` to render e.g. `● Read path` instead of
    /// `► read_file path`. Unknown/MCP tools are returned as-is.
    fn humanize_tool_name<'a>(&self, tool_name: &'a str) -> &'a str {
        match tool_name {
            "read_file" => "Read",
            "edit_file" => "Edit",
            "grep" => "Grep",
            "find_path" => "Find",
            "list_directory" => "List",
            "git" => "Git",
            "terminal" => "Terminal",
            "ask_supervisor" => "Ask supervisor",
            "think" => "Think",
            _ => tool_name,
        }
    }

    fn glyph_url_arrow(&self) -> &'static str {
        if self.plain_mode { "->" } else { "→" }
    }

    /// Returns the content pipe glyph: `│` in fancy mode, `|` in plain mode.
    fn glyph_content_pipe(&self) -> &'static str {
        if self.plain_mode { "|" } else { "│" }
    }

    /// Formats a content string as multi-line output with pipe prefixes.
    ///
    /// Splits on newlines and prefixes each line with the content pipe glyph at
    /// Level 2.5 indentation (5 spaces) to nest under the LLM event line.
    /// No truncation is applied — the full content is displayed.
    fn format_content_preview(&self, content: &str) -> Vec<String> {
        let pipe = self.glyph_content_pipe();
        content
            .lines()
            .map(|line| {
                if self.plain_mode {
                    format!("     {pipe} {line}")
                } else {
                    format!("     {} {}", style(pipe).dim(), style(line).dim())
                }
            })
            .collect()
    }

    /// Prints a styled line to the `MultiProgress` context so it does not
    /// collide with active spinners.
    fn println(&self, line: &str) {
        if let Err(e) = self.multi.println(line) {
            tracing::debug!("ConsoleRenderer: println failed: {e}");
        }
    }

    /// Finishes and removes a tracked phase spinner by name.
    /// Returns `Some((ProgressBar, is_sub))` if one was active, `None` otherwise.
    fn take_spinner(&self, phase_name: &str) -> Option<(ProgressBar, bool)> {
        match self.spinners.lock() {
            Ok(mut map) => map.remove(phase_name),
            Err(poisoned) => {
                tracing::debug!("ConsoleRenderer: spinner lock poisoned: {poisoned}");
                None
            }
        }
    }

    /// Finishes and clears **all** active spinners.
    ///
    /// Called before shutdown to prevent `indicatif` animation frames from
    /// being "baked" into the terminal when the process exits mid-tick.
    fn clear_all_spinners(&self) {
        match self.spinners.lock() {
            Ok(mut map) => {
                for (_, (pb, _)) in map.drain() {
                    pb.finish_and_clear();
                }
            }
            Err(poisoned) => {
                tracing::debug!("ConsoleRenderer: spinner lock poisoned: {poisoned}");
            }
        }
    }

    /// Stores a spinner for a given phase name.
    ///
    /// If a spinner already exists for `phase_name` (duplicate `phase_start`),
    /// the previous spinner is finished and cleared before the new one is stored,
    /// preventing orphaned spinners in `MultiProgress`.
    /// Returns `true` if there are active spinners (used to detect sub-phase nesting).
    fn has_active_spinners(&self) -> bool {
        match self.spinners.lock() {
            Ok(map) => !map.is_empty(),
            Err(_) => false,
        }
    }

    fn store_spinner(&self, phase_name: String, pb: ProgressBar, sub: bool) {
        match self.spinners.lock() {
            Ok(mut map) => {
                // H3 fix: finish any existing spinner for this phase before replacing it
                if let Some((existing, _)) = map.remove(&phase_name) {
                    existing.finish_and_clear();
                    tracing::debug!(
                        "ConsoleRenderer: phase_start called twice for '{phase_name}', \
                         previous spinner cleared"
                    );
                }
                map.insert(phase_name, (pb, sub));
            }
            Err(poisoned) => {
                tracing::debug!("ConsoleRenderer: spinner lock poisoned: {poisoned}");
            }
        }
    }
}

fn consultation_display_name(consultation_type: &str) -> (String, &'static str) {
    match consultation_type {
        "adversarial" => ("Adversarial review".to_string(), "finding"),
        "critic" => ("Story critic".to_string(), "observation"),
        "review-critic" => ("Review critic".to_string(), "observation"),
        other => {
            let mut chars = other.chars();
            let display = match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => other.to_string(),
            };
            (display, "finding")
        }
    }
}

impl UiRenderer for ConsoleRenderer {
    // ── Pipeline events ─────────────────────────────────────────────

    fn story_start(&self, key: &str, title: &str) {
        if let Ok(mut s) = self.session_info_suffix.lock() {
            s.clear();
        }
        if let Ok(mut s) = self.rate_limit_suffix.lock() {
            s.clear();
        }
        self.println(&format!(
            "{} Story {} — {}",
            self.glyph_progress(),
            style(key).bold(),
            title,
        ));
    }

    fn story_complete(&self, key: &str, pr_url: Option<&str>) {
        let arrow = self.glyph_url_arrow();
        let suffix = match pr_url {
            Some(url) => format!(" {arrow} {url}"),
            None => String::new(),
        };
        self.println(&format!(
            "{} Story {} complete{}",
            self.glyph_ok(),
            style(key).bold(),
            suffix,
        ));
    }

    fn story_error(&self, key: &str, error: &str) {
        self.println(&format!(
            "{} Story {} — {}",
            self.glyph_err(),
            style(key).bold(),
            error,
        ));
    }

    fn story_escalated(&self, key: &str, reason: &str) {
        self.println(&format!(
            "{} Story {} escalated — {}",
            self.glyph_warn(),
            style(key).bold(),
            reason,
        ));
    }

    fn batch_start(&self, count: usize) {
        self.println(&format!(
            "{} Batch started — {} stories",
            self.glyph_progress(),
            count,
        ));
    }

    fn batch_complete(&self, summary: &str) {
        self.println(&format!("{} Batch complete — {}", self.glyph_ok(), summary,));
    }

    // ── Phase events ────────────────────────────────────────────────

    fn phase_start(&self, phase_name: &str) {
        let sub = self.has_active_spinners();
        if let Ok(mut name) = self.active_phase_name.lock() {
            *name = phase_name.to_string();
        }
        let pb = self.create_spinner(phase_name.to_string(), sub);
        self.store_spinner(phase_name.to_string(), pb, sub);
    }

    fn phase_complete(&self, phase_name: &str, duration: Duration) {
        let indent = match self.take_spinner(phase_name) {
            Some((pb, sub)) => {
                pb.finish_and_clear();
                if sub { "    " } else { "  " }
            }
            None => "  ",
        };
        self.println(&format!(
            "{}{} {} [{}]",
            indent,
            self.glyph_ok(),
            phase_name,
            format_duration(duration),
        ));
    }

    fn phase_complete_with_result(
        &self,
        phase_name: &str,
        duration: Duration,
        result: &str,
    ) {
        let indent = match self.take_spinner(phase_name) {
            Some((pb, sub)) => {
                pb.finish_and_clear();
                if sub { "    " } else { "  " }
            }
            None => "  ",
        };
        self.println(&format!(
            "{}{} {} [{}] — {}",
            indent,
            self.glyph_ok(),
            phase_name,
            format_duration(duration),
            result,
        ));
    }

    fn phase_error(&self, phase_name: &str, error: &str) {
        let indent = match self.take_spinner(phase_name) {
            Some((pb, sub)) => {
                pb.finish_and_clear();
                if sub { "    " } else { "  " }
            }
            None => "  ",
        };
        self.println(&format!(
            "{}{} {} — {}",
            indent,
            self.glyph_err(),
            phase_name,
            error,
        ));
    }

    // ── Session events ──────────────────────────────────────────────

    fn chat_turn(&self, turn: u32, summary: &str) {
        if turn == 0 {
            if self.plain_mode {
                self.println(&format!("    > {summary}"));
            } else {
                self.println(&format!("    {} {}", style("▸").dim(), style(summary).dim()));
            }
        } else {
            self.println(&format!(
                "    {} turn {} — {}",
                self.glyph_sub(),
                turn,
                summary,
            ));
        }
    }

    fn activation_start(&self) {
        self.println(&format!(
            "    {} {}",
            self.glyph_progress(),
            if self.plain_mode { "Session started".to_string() } else { style("Session started").bold().to_string() },
        ));
    }

    fn activation_complete(&self) {
        self.println(&format!(
            "    {} {}",
            self.glyph_ok(),
            if self.plain_mode { "Session done".to_string() } else { style("Session done").bold().to_string() },
        ));
    }

    fn completion_detected(&self, story_key: &str) {
        self.println(&format!(
            "    {} Completion detected for {}",
            self.glyph_ok(),
            style(story_key).bold(),
        ));
    }

    // ── Tool events ─────────────────────────────────────────────────

    /// A tool call was initiated — displayed as `● {HumanName} {detail}`.
    ///
    /// Uses a green `●` glyph and a humanized tool name (e.g. `Read` instead
    /// of `read_file`) for a cleaner Copilot-style output.
    fn tool_call(&self, tool_name: &str, detail: &str) {
        let human = self.humanize_tool_name(tool_name);
        if self.plain_mode {
            self.println(&format!("      [ok] {human} {detail}"));
        } else {
            self.println(&format!(
                "      {} {} {}",
                style("●").green().dim(),
                style(human).bold(),
                style(detail).dim(),
            ));
        }
    }

    /// A tool call returned a result — displayed as a sub-line `└ {detail}`.
    ///
    /// When `detail` contains newlines (e.g. `think` tool output), the first
    /// line is shown next to the `└` glyph and subsequent lines are rendered
    /// with the content pipe prefix for readability.
    fn tool_result(&self, _tool_name: &str, detail: &str) {
        if detail.trim().is_empty() {
            return;
        }
        let mut lines = detail.lines();
        let first_line = lines.next().unwrap_or("");

        // Sub-line: └ detail
        let tree = self.glyph_sub_tree();
        if self.plain_mode {
            self.println(&format!("        {tree} {first_line}"));
        } else {
            self.println(&format!("        {} {}", tree, style(first_line).dim(),));
        }

        // Remaining lines: pipe-prefixed continuation
        let pipe = self.glyph_content_pipe();
        for line in lines {
            if self.plain_mode {
                self.println(&format!("        {pipe} {line}"));
            } else {
                self.println(&format!(
                    "        {} {}",
                    style(pipe).dim(),
                    style(line).dim()
                ));
            }
        }
    }

    // ── LLM events ──────────────────────────────────────────────────

    fn llm_request(&self, label: &str, turn: u32) {
        self.println(&format!(
            "    {} [{}] turn {}",
            self.glyph_arrow_out(),
            label,
            turn,
        ));
    }

    fn llm_response(&self, label: &str, turn: u32, response_len: usize) {
        self.println(&format!(
            "    {} [{}] turn {} — {} bytes",
            self.glyph_arrow_in(),
            label,
            turn,
            response_len,
        ));
    }

    fn llm_error(&self, label: &str, turn: u32, error: &str) {
        self.println(&format!(
            "    {} [{}] turn {} — {}",
            self.glyph_err(),
            label,
            turn,
            error,
        ));
    }

    fn llm_retry(&self, label: &str, turn: u32, retry_count: u32, delay_secs: f64) {
        self.println(&format!(
            "    {} [{}] turn {} — retry {} in {:.1}s",
            self.glyph_warn(),
            label,
            turn,
            retry_count,
            delay_secs,
        ));
    }

    fn llm_request_content(&self, _label: &str, _turn: u32, preview: &str) {
        if !self.verbose {
            return;
        }
        for line in self.format_content_preview(preview) {
            self.println(&line);
        }
    }

    fn llm_response_content(&self, _label: &str, _turn: u32, preview: &str) {
        if !self.verbose {
            return;
        }
        for line in self.format_content_preview(preview) {
            self.println(&line);
        }
    }

    // ── Consultation events ───────────────────────────────────────────

    fn consultation_start(&self, consultation_type: &str, _story_key: &str, detail: Option<&str>) {
        let spinner_name = match consultation_type {
            "adversarial" => "adversarial reviewer",
            "critic" => "story critic",
            "review-critic" => "review critic",
            _ => consultation_type,
        };
        let msg = if self.plain_mode {
            match detail {
                Some(d) => format!("Consulting {spinner_name} for {d}..."),
                None => format!("Consulting {spinner_name}..."),
            }
        } else {
            let emoji = if consultation_type == "adversarial" {
                "\u{1f50d}" // 🔍
            } else if consultation_type.contains("critic") {
                "\u{1f9e0}" // 🧠
            } else {
                "\u{1f4cb}" // 📋
            };
            match detail {
                Some(d) => format!("{emoji} Consulting {spinner_name} for {d}..."),
                None => format!("{emoji} Consulting {spinner_name}..."),
            }
        };
        let pb = self.create_spinner(msg, true);
        self.store_spinner(format!("consult:{consultation_type}"), pb, true);
    }

    fn consultation_complete(
        &self,
        consultation_type: &str,
        findings_count: usize,
        duration: Duration,
    ) {
        let (display_name, noun_singular) = consultation_display_name(consultation_type);
        let noun_plural = format!("{noun_singular}s");

        let count_text = match findings_count {
            0 => format!("no {noun_plural}"),
            1 => format!("1 {noun_singular}"),
            n => format!("{n} {noun_plural}"),
        };

        let critic_suffix = if consultation_type.contains("critic") {
            ", memory updated"
        } else {
            ""
        };

        let indent = match self.take_spinner(&format!("consult:{consultation_type}")) {
            Some((pb, _)) => {
                pb.finish_and_clear();
                "    "
            }
            None => "    ",
        };

        self.println(&format!(
            "{}{} {} done: {}{} [{}]",
            indent,
            self.glyph_ok(),
            display_name,
            count_text,
            critic_suffix,
            format_duration(duration),
        ));
    }

    fn consultation_error(&self, consultation_type: &str, error: &str) {
        let (display_name, _) = consultation_display_name(consultation_type);

        let indent = match self.take_spinner(&format!("consult:{consultation_type}")) {
            Some((pb, _)) => {
                pb.finish_and_clear();
                "    "
            }
            None => "    ",
        };

        self.println(&format!(
            "{}{} {} — {}",
            indent,
            self.glyph_err(),
            display_name,
            error,
        ));
    }

    fn critic_memory_update(&self, story_key: &str) {
        tracing::debug!(
            action = "critic_memory_update",
            story_key = %story_key,
            "Critic memory update event received"
        );
    }

    // ── System events ───────────────────────────────────────────────

    fn daemon_start(&self, config_summary: &str) {
        self.println(&format!(
            "{} Daemon started — {}",
            self.glyph_ok(),
            config_summary,
        ));
    }

    /// M3 fix: `poll_cycle` is a system-level event — uses `└` dim as a
    /// lightweight heartbeat marker (sub-detail of the running daemon), not
    /// `●` which signals a completed discrete action.
    fn poll_cycle(&self, cycle_num: u32) {
        self.println(&format!("{} Poll #{cycle_num}", self.glyph_sub()));
    }

    fn stories_found(&self, count: usize) {
        self.println(&format!(
            "{} Found {} eligible stories",
            self.glyph_ok(),
            count,
        ));
    }

    fn crash_recovery_start(&self) {
        self.println(&format!("{} Crash recovery initiated…", self.glyph_warn(),));
    }

    fn crash_recovery_complete(&self, story_key: &str) {
        self.println(&format!(
            "{} Crash recovery complete for {}",
            self.glyph_ok(),
            style(story_key).bold(),
        ));
    }

    fn sdk_session_info(&self, provider: &str, model: &str) {
        let tag = format!("[{provider}/{model}]");
        if let Ok(mut s) = self.session_info_suffix.lock() {
            *s = tag.clone();
        }
        self.update_active_spinner_combined();
    }

    fn sdk_input(&self, text: &str) {
        if self.plain_mode {
            self.println(&format!("      > {text}"));
        } else {
            self.println(&format!("      {} {}", style("▸").dim(), style(text).dim()));
        }
    }

    fn sdk_text(&self, text: &str) {
        if self.plain_mode {
            self.println(&format!("      | {text}"));
        } else {
            self.println(&format!("      {} {}", style("|").dim(), style(text).dim()));
        }
    }

    fn rate_limit_status(&self, resets_at: Option<u64>, limit_type: &str, percent_used: Option<f64>) {
        let window = match limit_type {
            "five_hour" => "5h",
            "daily" => "24h",
            other if other.is_empty() => "?",
            other => other,
        };
        let reset_str = resets_at
            .map(|ts| {
                chrono::DateTime::from_timestamp(ts as i64, 0)
                    .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
                    .unwrap_or_else(|| "??:??".to_string())
            })
            .unwrap_or_else(|| "??:??".to_string());

        let suffix = if let Some(pct) = percent_used {
            format!("[{window}:{pct:.0}%→{reset_str}]")
        } else {
            format!("[{window}→{reset_str}]")
        };

        if let Ok(mut prev) = self.rate_limit_suffix.lock() {
            *prev = suffix;
        }

        self.update_active_spinner_combined();
    }

    fn shutdown_requested(&self) {
        // Drain active spinners BEFORE printing so that indicatif
        // animation frames don't linger on screen after Ctrl+C.
        self.clear_all_spinners();
        self.println(&format!(
            "{} Shutdown requested — finishing current work…",
            self.glyph_warn(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_duration tests (Task 1.4) ────────────────────────────

    #[test]
    fn test_format_duration_zero_seconds() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0s");
    }

    #[test]
    fn test_format_duration_under_60s() {
        assert_eq!(format_duration(Duration::from_secs(47)), "47s");
    }

    #[test]
    fn test_format_duration_exactly_60s() {
        assert_eq!(format_duration(Duration::from_secs(60)), "1m 0s");
    }

    #[test]
    fn test_format_duration_90s() {
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
    }

    #[test]
    fn test_format_duration_just_under_1h() {
        assert_eq!(format_duration(Duration::from_secs(3599)), "59m 59s");
    }

    #[test]
    fn test_format_duration_exactly_1h() {
        assert_eq!(format_duration(Duration::from_secs(3600)), "1h 0m");
    }

    #[test]
    fn test_format_duration_over_1h() {
        assert_eq!(format_duration(Duration::from_secs(5000)), "1h 23m");
    }

    #[test]
    fn test_format_duration_ignores_sub_second() {
        // 47.999s should display as 47s (truncated, not rounded)
        assert_eq!(format_duration(Duration::from_millis(47_999)), "47s");
    }

    // ── ConsoleRenderer (fancy mode) tests ──────────────────────────

    /// Compile-time proof that `ConsoleRenderer` is `Send + Sync` (M2).
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_console_renderer_is_send_sync() {
        assert_send_sync::<ConsoleRenderer>();
    }

    #[test]
    fn test_console_renderer_new_does_not_panic() {
        let _r = ConsoleRenderer::new(false, false);
    }

    /// H2: exercise every `UiRenderer` method on a real `ConsoleRenderer`
    /// to catch any internal panic (poisoned lock, bad ProgressStyle template, etc.).
    #[test]
    fn test_console_renderer_all_methods_do_not_panic() {
        let r = ConsoleRenderer::new(false, false);

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
        let r = ConsoleRenderer::new(false, false);

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
        let r = ConsoleRenderer::new(false, false);
        r.phase_complete("Ghost Phase", Duration::from_secs(0));
    }

    /// H3: `phase_error` on a phase that was never started must not panic.
    #[test]
    fn test_phase_error_without_start_does_not_panic() {
        let r = ConsoleRenderer::new(false, false);
        r.phase_error("Ghost Phase", "never started");
    }

    // ── ConsoleRenderer (plain mode) tests (Task 3.7) ───────────────

    /// Plain mode: construct with `plain=true` and exercise all methods
    /// without panic. Verifies ASCII glyph code paths are reachable.
    #[test]
    fn test_console_renderer_plain_mode_all_methods_do_not_panic() {
        let r = ConsoleRenderer::new(true, false);

        // Pipeline
        r.story_start("10-5", "Polish");
        r.story_complete("10-5", Some("https://example.com/pr/5"));
        r.story_complete("10-5", None);
        r.story_error("10-5", "compile error");
        r.story_escalated("10-5", "needs clarification");
        r.batch_start(2);
        r.batch_complete("2/2 done");

        // Phase
        r.phase_start("Dev Session");
        r.phase_complete("Dev Session", Duration::from_secs(120));
        r.phase_start("Code Review");
        r.phase_error("Code Review", "timeout");

        // Session
        r.chat_turn(1, "task done");
        r.activation_start();
        r.activation_complete();
        r.completion_detected("10-5");

        // Tool
        r.tool_call("git", "commit msg");
        r.tool_result("git", "ok");

        // LLM
        r.llm_request("dev", 1);
        r.llm_response("dev", 1, 2048);
        r.llm_error("dev", 2, "rate limited");
        r.llm_retry("dev", 2, 1, 3.0);

        // System
        r.daemon_start("poll=30s");
        r.poll_cycle(1);
        r.stories_found(1);
        r.crash_recovery_start();
        r.crash_recovery_complete("10-5");
        r.shutdown_requested();
    }

    #[test]
    fn test_console_renderer_plain_mode_field_stored() {
        let fancy = ConsoleRenderer::new(false, false);
        assert!(!fancy.plain_mode);

        let plain = ConsoleRenderer::new(true, false);
        assert!(plain.plain_mode);
    }

    // ── Verbose content preview tests (Task 4.7) ────────────────────

    #[test]
    fn test_format_content_preview_short_single_line() {
        let r = ConsoleRenderer::new(true, true);
        let lines = r.format_content_preview("hello world");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("hello world"));
        assert!(!lines[0].contains('…'));
    }

    #[test]
    fn test_format_content_preview_no_truncation() {
        let r = ConsoleRenderer::new(true, true);
        let long = "a".repeat(1000);
        let lines = r.format_content_preview(&long);
        // Full content preserved — no ellipsis
        let joined = lines.join("\n");
        assert!(!joined.contains('…'), "content must NOT be truncated");
        assert!(joined.contains(&"a".repeat(1000)));
    }

    #[test]
    fn test_format_content_preview_multiline_gets_pipe_prefix() {
        let r = ConsoleRenderer::new(true, true);
        let lines = r.format_content_preview("line1\nline2\nline3");
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert!(
                line.contains('|'),
                "each line should have pipe prefix, got: {line}"
            );
        }
    }

    #[test]
    fn test_console_renderer_verbose_mode_field_stored() {
        let non_verbose = ConsoleRenderer::new(false, false);
        assert!(!non_verbose.verbose);

        let verbose = ConsoleRenderer::new(false, true);
        assert!(verbose.verbose);
    }

    #[test]
    fn test_console_renderer_llm_content_methods_do_not_panic() {
        // Verbose mode — should render content
        let r = ConsoleRenderer::new(false, true);
        r.llm_request_content("dev", 1, "test request preview");
        r.llm_response_content("dev", 1, "test response preview");

        // Non-verbose mode — should no-op
        let r2 = ConsoleRenderer::new(false, false);
        r2.llm_request_content("dev", 1, "should not render");
        r2.llm_response_content("dev", 1, "should not render");
    }

    #[test]
    fn test_console_renderer_plain_verbose_uses_ascii_pipe() {
        let r = ConsoleRenderer::new(true, true);
        let lines = r.format_content_preview("hello\nworld");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("| hello"));
        assert!(lines[1].contains("| world"));
        // Must NOT contain the Unicode pipe
        assert!(
            !lines[0].contains('│'),
            "plain mode should NOT use Unicode pipe, got: {}",
            lines[0]
        );
    }

    // ── Consultation event tests (Story 13.11, Task 6) ──────────────

    #[test]
    fn test_console_consultation_start_complete_lifecycle() {
        let r = ConsoleRenderer::new(true, false);
        r.consultation_start("adversarial", "13-11", None);
        assert!(
            r.has_active_spinners(),
            "spinner should be active after consultation_start"
        );
        r.consultation_complete("adversarial", 5, Duration::from_secs(30));
        assert!(
            !r.has_active_spinners(),
            "spinner should be cleaned up after consultation_complete"
        );
    }

    #[test]
    fn test_console_consultation_complete_without_start() {
        let r = ConsoleRenderer::new(true, false);
        r.consultation_complete("unknown", 3, Duration::from_secs(10));
    }

    #[test]
    fn test_console_consultation_error_resolves_spinner() {
        let r = ConsoleRenderer::new(true, false);
        r.consultation_start("critic", "13-11", None);
        assert!(r.has_active_spinners());
        r.consultation_error("critic", "LLM timeout");
        assert!(
            !r.has_active_spinners(),
            "spinner should be cleaned up after consultation_error"
        );
    }

    #[test]
    fn test_console_sequential_consultations_no_orphaned_spinners() {
        let r = ConsoleRenderer::new(true, false);
        r.consultation_start("adversarial", "13-11", None);
        r.consultation_complete("adversarial", 7, Duration::from_secs(72));
        r.consultation_start("critic", "13-11", None);
        r.consultation_complete("critic", 3, Duration::from_secs(45));
        r.critic_memory_update("13-11");
        assert!(
            !r.has_active_spinners(),
            "no orphaned spinners after sequential consultation flow"
        );
    }

    #[test]
    fn test_console_consultation_start_with_detail() {
        let r = ConsoleRenderer::new(true, false);
        r.consultation_start("review-critic", "13-11", Some("2 decision-needed findings"));
        assert!(r.has_active_spinners());
        r.consultation_complete("review-critic", 1, Duration::from_secs(20));
        assert!(
            !r.has_active_spinners(),
            "spinner should be cleaned up after consultation_complete with detail"
        );
    }

    #[test]
    fn test_console_critic_memory_update_is_noop() {
        let plain = ConsoleRenderer::new(true, false);
        plain.critic_memory_update("13-11");
        assert!(!plain.has_active_spinners());

        let fancy = ConsoleRenderer::new(false, false);
        fancy.critic_memory_update("13-11");
        assert!(!fancy.has_active_spinners());
    }
}
