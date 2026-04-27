//! SDK consultation orchestration: post-session trigger detection,
//! consultation execution via API, and SDK session resume with findings.

use std::collections::HashSet;
use std::path::Path;

use crate::session::SessionOutcome;
use crate::session::consultation::ConsultationConfig;
use crate::watcher::StoryInfo;

use super::SdkRuntime;
use super::sdk::SdkSessionResult;

const MAX_SDK_CONSULTATION_ROUNDS: usize = 3;

pub struct SdkConsultationRunner<'a> {
    sdk_runtime: &'a SdkRuntime,
    consultations: Vec<ConsultationConfig>,
    fired: HashSet<String>,
    round: usize,
}

impl<'a> SdkConsultationRunner<'a> {
    pub fn new(sdk_runtime: &'a SdkRuntime, consultations: Vec<ConsultationConfig>) -> Self {
        Self {
            sdk_runtime,
            consultations,
            fired: HashSet::new(),
            round: 0,
        }
    }

    pub async fn run_with_consultations(
        &mut self,
        story: &StoryInfo,
        phase: &str,
        initial_outcome: SessionOutcome,
        initial_result: &SdkSessionResult,
        role: &crate::llm::agent_factory::LlmRole,
        agent_factory: Option<&std::sync::Arc<crate::llm::AgentFactory>>,
    ) -> SessionOutcome {
        if self.consultations.is_empty() {
            return initial_outcome;
        }

        let Some(agent_factory) = agent_factory else {
            tracing::warn!("No API agent factory available for SDK consultations — skipping");
            return initial_outcome;
        };

        let mut current_outcome = initial_outcome;
        let mut current_session_id = initial_result.session_id.clone();
        let mut current_completion = initial_result.completion_text.clone();

        loop {
            if self.round >= MAX_SDK_CONSULTATION_ROUNDS {
                tracing::warn!(
                    round = self.round,
                    "SDK consultation max rounds reached, returning last outcome"
                );
                break;
            }

            let trigger_text = self
                .capture_trigger_text(story, phase, current_completion.as_deref())
                .await;

            let triggered = self.find_triggered_consultation(&trigger_text);
            let Some(consultation) = triggered else {
                break;
            };

            tracing::info!(
                label = %consultation.label,
                round = self.round,
                "SDK consultation triggered"
            );

            let findings = self
                .run_api_consultation(&consultation, story, &trigger_text, agent_factory)
                .await;

            let Some(findings) = findings else {
                self.round += 1;
                continue;
            };

            let Some(ref session_id) = current_session_id else {
                tracing::warn!("No SDK session ID available for resume after consultation");
                break;
            };

            let provider = self.sdk_runtime.config_for_role(role).provider.clone();
            let resume_prompt = consultation
                .resume_message_template
                .replace("{findings}", &findings);

            let (outcome, resume_result) = self
                .sdk_runtime
                .resume_sdk_session(&provider, session_id, &resume_prompt, story, role)
                .await;
            current_outcome = outcome;

            if let Some(res) = resume_result {
                current_completion = res.completion_text;
                if res.session_id.is_some() {
                    current_session_id = res.session_id;
                }
            }

            if let SessionOutcome::Completed { .. } = &current_outcome {
                // keep going — check for more consultations
            } else {
                break;
            }

            self.round += 1;
        }

        current_outcome
    }

    async fn capture_trigger_text(
        &self,
        story: &StoryInfo,
        phase: &str,
        completion_text: Option<&str>,
    ) -> String {
        let mut text = String::new();

        if let Ok(content) = tokio::fs::read_to_string(&story.specs_path).await {
            text.push_str(&content);
        }

        if phase != crate::session::state::PHASE_CREATE {
            if let Some(completion) = completion_text {
                text.push('\n');
                text.push_str(completion);
            }
        }

        text
    }

    fn find_triggered_consultation(&mut self, trigger_text: &str) -> Option<ConsultationConfig> {
        for consultation in &self.consultations {
            if self.fired.contains(&consultation.label) {
                continue;
            }
            if let Ok(re) = regex::Regex::new(&consultation.trigger_pattern) {
                if re.is_match(trigger_text) {
                    self.fired.insert(consultation.label.clone());
                    return Some(consultation.clone());
                }
            } else {
                tracing::warn!(
                    pattern = %consultation.trigger_pattern,
                    label = %consultation.label,
                    "Invalid trigger regex, skipping consultation"
                );
            }
        }
        None
    }

    async fn run_api_consultation(
        &self,
        consultation: &ConsultationConfig,
        story: &StoryInfo,
        trigger_text: &str,
        agent_factory: &std::sync::Arc<crate::llm::AgentFactory>,
    ) -> Option<String> {
        let role_config =
            super::resolve_role_config(&self.sdk_runtime.config().llm, &consultation.role);
        let fallback_config = if role_config.is_sdk_provider() {
            tracing::warn!(
                role = ?consultation.role,
                label = %consultation.label,
                "Consultation role uses SDK provider — falling back to supervisor API config"
            );
            let supervisor_config = &self.sdk_runtime.config().llm.supervisor;
            if supervisor_config.is_sdk_provider() {
                tracing::error!(
                    "No API provider available for consultation (supervisor is also SDK) — skipping"
                );
                return None;
            }
            let mut c = consultation.clone();
            c.role = crate::llm::agent_factory::LlmRole::Supervisor;
            Some(c)
        } else {
            None
        };

        let effective = fallback_config.as_ref().unwrap_or(consultation);

        let consultation_runner = crate::session::consultation::ConsultationRunner::new(
            agent_factory.clone(),
            self.sdk_runtime.shutdown_flag().clone(),
            crate::ui::UiHandle::null(),
            std::path::PathBuf::from(&self.sdk_runtime.config().bmad_paths.project_root),
        );

        let _ = trigger_text;
        let _ = story;
        match consultation_runner.execute(effective).await {
            Ok(findings) => {
                if findings.is_empty() {
                    None
                } else {
                    Some(findings)
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    label = %consultation.label,
                    "SDK consultation failed"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_factory::LlmRole;
    use crate::session::consultation::ConsultationToolSet;

    fn make_test_consultation(label: &str, pattern: &str) -> ConsultationConfig {
        ConsultationConfig {
            label: label.to_string(),
            skill_path: Some(".claude/skills/bmad-test/SKILL.md".to_string()),
            preamble_override: None,
            role: LlmRole::Review,
            tool_set: ConsultationToolSet::Restricted,
            context_files: vec![],
            trigger_pattern: pattern.to_string(),
            prompt_template: "Review: {context}".to_string(),
            resume_message_template: "Review findings:\n{findings}".to_string(),
            pipeline_phase: None,
        }
    }

    #[test]
    fn test_find_triggered_consultation_matches() {
        let config = std::sync::Arc::new(crate::config::BotConfig::_test_minimal("pretty", "info"));
        let secrets = std::sync::Arc::new(crate::config::BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sdk = super::SdkRuntime::new(
            config,
            secrets,
            std::path::PathBuf::from("test.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
        );

        let consultations = vec![
            make_test_consultation("adversarial", "STORY CONTEXT CREATED"),
            make_test_consultation("critic", "corrections applied"),
        ];
        let mut runner = SdkConsultationRunner::new(&sdk, consultations);

        let result = runner.find_triggered_consultation("The STORY CONTEXT CREATED successfully");
        assert!(result.is_some());
        assert_eq!(result.unwrap().label, "adversarial");
    }

    #[test]
    fn test_find_triggered_consultation_no_match() {
        let config = std::sync::Arc::new(crate::config::BotConfig::_test_minimal("pretty", "info"));
        let secrets = std::sync::Arc::new(crate::config::BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sdk = super::SdkRuntime::new(
            config,
            secrets,
            std::path::PathBuf::from("test.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
        );

        let consultations = vec![make_test_consultation(
            "adversarial",
            "STORY CONTEXT CREATED",
        )];
        let mut runner = SdkConsultationRunner::new(&sdk, consultations);

        let result = runner.find_triggered_consultation("nothing interesting here");
        assert!(result.is_none());
    }

    #[test]
    fn test_find_triggered_consultation_fires_only_once() {
        let config = std::sync::Arc::new(crate::config::BotConfig::_test_minimal("pretty", "info"));
        let secrets = std::sync::Arc::new(crate::config::BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sdk = super::SdkRuntime::new(
            config,
            secrets,
            std::path::PathBuf::from("test.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
        );

        let consultations = vec![make_test_consultation(
            "adversarial",
            "STORY CONTEXT CREATED",
        )];
        let mut runner = SdkConsultationRunner::new(&sdk, consultations);

        let first = runner.find_triggered_consultation("STORY CONTEXT CREATED");
        assert!(first.is_some());

        let second = runner.find_triggered_consultation("STORY CONTEXT CREATED again");
        assert!(second.is_none(), "consultation should only fire once");
    }

    #[test]
    fn test_max_consultation_rounds_constant() {
        assert_eq!(MAX_SDK_CONSULTATION_ROUNDS, 3);
    }

    #[test]
    fn test_invalid_regex_skips_consultation() {
        let config = std::sync::Arc::new(crate::config::BotConfig::_test_minimal("pretty", "info"));
        let secrets = std::sync::Arc::new(crate::config::BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sdk = super::SdkRuntime::new(
            config,
            secrets,
            std::path::PathBuf::from("test.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
        );

        let consultations = vec![make_test_consultation("bad", "[invalid regex")];
        let mut runner = SdkConsultationRunner::new(&sdk, consultations);

        let result = runner.find_triggered_consultation("anything");
        assert!(result.is_none());
    }
}
