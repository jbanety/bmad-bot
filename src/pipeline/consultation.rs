//! Linear consultation orchestration helpers.
//!
//! Provides building blocks for the pipeline to run consultations sequentially.
//! The pipeline owns the loop; this module provides context-building and
//! session dispatch.

use crate::runtime::{RawSessionResult, RuntimeCommand, SessionRuntime};
use crate::session::consultation::ConsultationConfig;

/// Run a single consultation session and return raw findings.
///
/// Builds the prompt from context files + template, dispatches a fresh Start
/// to the runtime, and returns the raw result.
pub async fn run_single_consultation(
    runtime: &SessionRuntime,
    consultation: &ConsultationConfig,
    story_key: &str,
    ui: &crate::ui::UiHandle,
) -> RawSessionResult {
    let context = build_context(consultation).await;
    let rendered_prompt = consultation.prompt_template.replace("{context}", &context);

    let full_prompt = if let Some(ref preamble) = consultation.preamble_override {
        format!("{preamble}\n\n{rendered_prompt}")
    } else {
        rendered_prompt
    };

    runtime
        .execute(ui, RuntimeCommand::Start {
            role: consultation.role.clone(),
            phase: format!("consultation-{}", consultation.label),
            story_key: story_key.to_string(),
            prompt: full_prompt,
            skill_path: consultation.skill_path.clone(),
            preamble: None,
            needs_supervisor: false,
        })
        .await
}

/// Read context files listed in a consultation config and concatenate them.
async fn build_context(consultation: &ConsultationConfig) -> String {
    let mut parts = Vec::new();
    for file_path in &consultation.context_files {
        match tokio::fs::read_to_string(file_path).await {
            Ok(content) => {
                parts.push(format!("--- {file_path} ---\n{content}"));
            }
            Err(e) => {
                tracing::warn!(
                    path = %file_path,
                    error = %e,
                    "Failed to read consultation context file, skipping"
                );
            }
        }
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_factory::LlmRole;
    use crate::session::consultation::ConsultationToolSet;

    fn test_config() -> ConsultationConfig {
        ConsultationConfig {
            label: "test".to_string(),
            skill_path: None,
            preamble_override: Some("You are a reviewer".to_string()),
            role: LlmRole::Review,
            tool_set: ConsultationToolSet::Full,
            context_files: vec![],
            trigger_pattern: String::new(),
            prompt_template: "Review: {context}".to_string(),
            resume_message_template: "Findings:\n{findings}".to_string(),
            pipeline_phase: None,
        }
    }

    #[tokio::test]
    async fn test_build_context_missing_files() {
        let config = ConsultationConfig {
            context_files: vec!["/nonexistent/file1.md".to_string()],
            ..test_config()
        };
        let result = build_context(&config).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_build_context_empty_files() {
        let config = test_config();
        let result = build_context(&config).await;
        assert!(result.is_empty());
    }
}
