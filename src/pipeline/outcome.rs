//! Session outcome interpretation — maps raw runtime results to business outcomes.
//!
//! Reads decision sidecars, detects escalations, interprets exit codes and errors.
//! This logic is provider-agnostic and lives in the pipeline layer.

use std::path::Path;

use crate::runtime::sdk::SdkSessionResult;
use crate::session::SessionOutcome;
use crate::session::escalation::EscalationReport;
use crate::supervisor::decisions::{DecisionRecord, DecisionSource};
use crate::watcher::StoryInfo;

/// Interpret a raw SDK session result into a pipeline-level `SessionOutcome`.
pub async fn map_sdk_result_to_outcome(
    result: &SdkSessionResult,
    story: &StoryInfo,
    impl_artifacts_path: &Path,
) -> SessionOutcome {
    if let Some(ref text) = result.completion_text {
        tracing::info!(
            action = "sdk_completion",
            story_key = %story.story_key,
            exit_code = ?result.exit_code,
            len = text.len(),
            text = %text,
            "SDK session final completion"
        );
    }

    let decisions = read_decisions_json_sidecar(impl_artifacts_path, &story.story_key).await;

    if let Some(resets_at) = result.rate_limit_resets_at {
        return SessionOutcome::Failed {
            story_key: story.story_key.clone(),
            error: format!("RATE_LIMITED:{resets_at}"),
            decisions,
        };
    }

    if let Some((question, reason)) = detect_escalation(&decisions) {
        return SessionOutcome::Escalated {
            report: EscalationReport::new(
                story.story_key.clone(),
                question,
                reason,
                story.branch_name.clone(),
                "SDK session completed with escalation".to_string(),
            ),
            decisions,
        };
    }

    if result.timed_out {
        return SessionOutcome::Failed {
            story_key: story.story_key.clone(),
            error: "SDK session timed out".to_string(),
            decisions,
        };
    }

    if result.exit_code == Some(0) {
        let pr_context = result
            .completion_text
            .as_ref()
            .filter(|text| !text.is_empty())
            .map(|text| {
                if text.chars().count() > 2000 {
                    text.chars().take(2000).collect()
                } else {
                    text.clone()
                }
            });

        SessionOutcome::Completed {
            story_key: story.story_key.clone(),
            branch: story.branch_name.clone(),
            decisions,
            pr_context,
            pr_how_to_test: None,
            pr_additional_info: None,
        }
    } else {
        let mut error = format!("SDK session failed (exit code {:?})", result.exit_code);
        if let Some(ref stream_err) = result.stream_error {
            error.push_str(": ");
            error.push_str(stream_err);
        } else if !result.stderr.is_empty() {
            error.push_str(": ");
            error.push_str(&result.stderr);
        }

        let error_lower = error.to_lowercase();
        if (error_lower.contains("rate limit") || error_lower.contains("usage limit"))
            && result.rate_limit_resets_at.is_none()
        {
            return SessionOutcome::Failed {
                story_key: story.story_key.clone(),
                error: "RATE_LIMITED:0".to_string(),
                decisions,
            };
        }

        SessionOutcome::Failed {
            story_key: story.story_key.clone(),
            error,
            decisions,
        }
    }
}

/// Read supervisor decisions from the JSON sidecar file.
pub async fn read_decisions_json_sidecar(
    impl_artifacts_dir: &Path,
    story_key: &str,
) -> Vec<DecisionRecord> {
    let path = impl_artifacts_dir.join(format!("{story_key}-SUPERVISOR-DECISIONS.json"));
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_str(&content) {
        Ok(decisions) => decisions,
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "Failed to parse decisions JSON sidecar, treating as empty"
            );
            Vec::new()
        }
    }
}

/// Detect if any decision record represents an escalation.
pub fn detect_escalation(decisions: &[DecisionRecord]) -> Option<(String, String)> {
    for record in decisions {
        if record.source == DecisionSource::Escalation {
            return Some((record.question.clone(), record.reasoning.clone()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::decisions::DecisionSource;
    use std::path::PathBuf;

    #[test]
    fn test_detect_escalation_found() {
        let decisions = vec![DecisionRecord {
            question: "What auth library?".to_string(),
            context: None,
            answer: String::new(),
            reasoning: "Cannot determine from project docs".to_string(),
            source: DecisionSource::Escalation,
            alternatives: vec![],
            timestamp: "2026-05-02T00:00:00Z".to_string(),
        }];
        let result = detect_escalation(&decisions);
        assert!(result.is_some());
        let (q, r) = result.unwrap();
        assert_eq!(q, "What auth library?");
        assert_eq!(r, "Cannot determine from project docs");
    }

    #[test]
    fn test_detect_escalation_not_found() {
        let decisions = vec![DecisionRecord {
            question: "Should I proceed?".to_string(),
            context: None,
            answer: "Yes".to_string(),
            reasoning: "Standard confirmation".to_string(),
            source: DecisionSource::RuleEngine {
                rule_name: "confirmation_proceed".to_string(),
            },
            alternatives: vec![],
            timestamp: "2026-05-02T00:00:00Z".to_string(),
        }];
        assert!(detect_escalation(&decisions).is_none());
    }

    #[test]
    fn test_detect_escalation_empty() {
        assert!(detect_escalation(&[]).is_none());
    }

    #[tokio::test]
    async fn test_read_decisions_json_sidecar_missing_file() {
        let dir = PathBuf::from("/tmp/nonexistent-dir-bmad-test");
        let result = read_decisions_json_sidecar(&dir, "test-story").await;
        assert!(result.is_empty());
    }
}
