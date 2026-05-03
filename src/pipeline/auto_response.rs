//! BMAD-specific auto-response logic for interactive CLI prompts.
//!
//! When a session returns with an interactive prompt (checkpoint, Y/N
//! confirmation, numeric choice), these functions determine what to
//! respond — or whether the session is truly done.

/// Determine the automatic response for an interactive prompt, if any.
///
/// Returns `Some(response)` if the prompt is recognized, `None` if the session
/// should not be auto-resumed (legitimate completion or unrecognized prompt).
pub fn auto_response_for_prompt(text: &str) -> Option<String> {
    let lower = text.to_lowercase();

    if is_checkpoint_prompt(&lower) {
        return Some("proceed".to_string());
    }

    if !is_confirmation_prompt(text) && !is_numeric_choice_prompt(text) {
        return None;
    }

    if lower.contains("chunk") && is_confirmation_prompt(text) {
        return Some("Y — review all chunks in order, starting from the first group".to_string());
    }

    if is_numeric_choice_prompt(text) {
        if lower.contains("what would you like to do next")
            || lower.contains("next step")
            || (lower.contains("done") && lower.contains("re-run"))
        {
            return None;
        }

        if lower.contains("patch") && lower.contains("handle") {
            let tail = if text.len() > 500 {
                &text[text.len() - 500..]
            } else {
                text
            };
            let has_option_0 = tail.lines().any(|l| {
                let t = l.trim();
                t.starts_with("0.")
                    || t.starts_with("0)")
                    || t.starts_with("0 —")
                    || t.starts_with("0-")
            });
            return if has_option_0 {
                Some("0".to_string())
            } else {
                Some("1".to_string())
            };
        }

        if lower.contains("[a] approv") || lower.contains("[a] approve") {
            return Some("A".to_string());
        }

        tracing::warn!(
            action = "auto_confirm_unknown_prompt",
            tail = %&text[text.len().saturating_sub(200)..],
            "Unknown numeric choice prompt — not auto-responding"
        );
        return None;
    }

    Some("Y".to_string())
}

/// Detect BMAD skill checkpoint prompts that wait for free-text confirmation.
///
/// Only matches the last ~200 chars (the actual ask) to avoid false positives
/// from earlier narrative text like "I will proceed to review...".
pub fn is_checkpoint_prompt(lower: &str) -> bool {
    let tail = if lower.len() > 200 {
        &lower[lower.len() - 200..]
    } else {
        lower
    };
    if tail.contains("reply") && tail.contains("proceed") {
        return true;
    }
    if tail.contains("confirm") && tail.contains("proceed") {
        return true;
    }
    if tail.contains("halt") && tail.contains("wait") {
        return true;
    }
    false
}

/// Detect if the completion text ends with a numeric choice prompt (1, 2, 3...).
pub fn is_numeric_choice_prompt(text: &str) -> bool {
    let tail = if text.len() > 500 {
        &text[text.len() - 500..]
    } else {
        text
    };
    let has_option_1 = tail.lines().any(|l| {
        let t = l.trim();
        t.starts_with("1.") || t.starts_with("1)") || t.starts_with("1 —") || t.starts_with("1-")
    });
    let has_option_2 = tail.lines().any(|l| {
        let t = l.trim();
        t.starts_with("2.") || t.starts_with("2)") || t.starts_with("2 —") || t.starts_with("2-")
    });
    has_option_1 && has_option_2
}

/// Detect if the completion text ends with a BMAD skill confirmation prompt.
///
/// Matches patterns like `[Y]/[N]`, `[Y] pour confirmer`, `[Y] Yes`, etc.
/// Only considers the last ~500 chars to avoid false positives in large outputs.
pub fn is_confirmation_prompt(text: &str) -> bool {
    let tail = if text.len() > 500 {
        &text[text.len() - 500..]
    } else {
        text
    };
    let lower = tail.to_lowercase();
    let has_y = lower.contains("`y`") || lower.contains("[y]");
    let has_n = lower.contains("`n`") || lower.contains("[n]");
    if has_y && has_n {
        return true;
    }
    if lower.contains("[y]") {
        if let Some(pos) = lower.rfind("[y]") {
            let after = &lower[pos + 3..];
            let trimmed = after.trim_start();
            if trimmed.starts_with("pour")
                || trimmed.starts_with("yes")
                || trimmed.starts_with("oui")
                || trimmed.starts_with("to confirm")
                || trimmed.is_empty()
                || trimmed.starts_with('\n')
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_reply_proceed() {
        let text = "I've completed the analysis. Reply `proceed` to continue.";
        assert_eq!(
            auto_response_for_prompt(text),
            Some("proceed".to_string())
        );
    }

    #[test]
    fn test_checkpoint_halt_and_wait() {
        let text = "HALT and wait for user confirmation before continuing.";
        assert_eq!(
            auto_response_for_prompt(text),
            Some("proceed".to_string())
        );
    }

    #[test]
    fn test_confirmation_yn_prompt() {
        let text = "Would you like to proceed? [Y]/[N]";
        assert_eq!(auto_response_for_prompt(text), Some("Y".to_string()));
    }

    #[test]
    fn test_confirmation_y_only() {
        let text = "Ready to apply changes. [Y] to confirm";
        assert_eq!(auto_response_for_prompt(text), Some("Y".to_string()));
    }

    #[test]
    fn test_numeric_choice_next_steps_stops() {
        let text = "What would you like to do next?\n1. Continue\n2. Stop";
        assert_eq!(auto_response_for_prompt(text), None);
    }

    #[test]
    fn test_numeric_choice_patch_with_option_0() {
        let text = "How to handle the patch?\n0. Batch apply\n1. Apply individually\n2. Skip";
        assert_eq!(auto_response_for_prompt(text), Some("0".to_string()));
    }

    #[test]
    fn test_numeric_choice_patch_without_option_0() {
        let text = "How to handle the patch?\n1. Apply all\n2. Skip";
        assert_eq!(auto_response_for_prompt(text), Some("1".to_string()));
    }

    #[test]
    fn test_numeric_approve_spec() {
        let text = "Review complete.\n1. [A] Approve spec\n2. Request changes";
        assert_eq!(auto_response_for_prompt(text), Some("A".to_string()));
    }

    #[test]
    fn test_chunk_review_confirmation() {
        let text = "I'll chunk the review by file group. Proceed? [Y]/[N]";
        assert_eq!(
            auto_response_for_prompt(text),
            Some("Y — review all chunks in order, starting from the first group".to_string())
        );
    }

    #[test]
    fn test_no_prompt_detected() {
        let text = "Implementation complete. All tests passing.";
        assert_eq!(auto_response_for_prompt(text), None);
    }

    #[test]
    fn test_is_checkpoint_confirm_and_proceed() {
        assert!(is_checkpoint_prompt("confirm and I'll proceed with the implementation"));
    }

    #[test]
    fn test_is_checkpoint_not_matched() {
        assert!(!is_checkpoint_prompt("I will proceed to review the code now"));
    }

    #[test]
    fn test_is_numeric_choice_prompt_basic() {
        assert!(is_numeric_choice_prompt("Options:\n1. First\n2. Second"));
        assert!(!is_numeric_choice_prompt("Only one option:\n1. First"));
    }

    #[test]
    fn test_is_confirmation_yn() {
        assert!(is_confirmation_prompt("Continue? [Y]/[N]"));
        assert!(is_confirmation_prompt("Ready? `Y`/`N`"));
    }

    #[test]
    fn test_is_confirmation_y_only_pour() {
        assert!(is_confirmation_prompt("[Y] pour confirmer"));
    }

    #[test]
    fn test_is_confirmation_not_matched() {
        assert!(!is_confirmation_prompt("The code looks good."));
    }
}
