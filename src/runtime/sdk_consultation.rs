//! SDK consultation orchestration: post-session trigger detection,
//! consultation execution via API, and SDK session resume with findings.

use std::collections::HashSet;

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

        let empty_factory = std::sync::Arc::new(crate::llm::AgentFactory::new(
            self.sdk_runtime.config_arc(),
            std::sync::Arc::new(crate::config::BotSecrets {
                anthropic_api_key: None,
                openai_api_key: None,
                github_token: None,
                gitlab_token: None,
                telegram_bot_token: None,
            }),
        ));
        let agent_factory = agent_factory.unwrap_or(&empty_factory);

        let mut current_outcome = initial_outcome;
        let mut current_session_id = initial_result.session_id.clone();
        let mut current_completion = initial_result.completion_text.clone();

        self.sdk_runtime.set_suppress_activation_ui(true);

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
                tracing::info!("No consultation trigger matched — done");
                break;
            };

            tracing::info!(
                label = %consultation.label,
                round = self.round,
                "SDK consultation triggered"
            );
            self.sdk_runtime.ui().consultation_start(
                &consultation.label,
                &story.story_key,
                Some(&format!("round {}", self.round + 1)),
            );

            let consult_start = std::time::Instant::now();
            let consultation_result = self
                .run_consultation(&consultation, story, &trigger_text, agent_factory)
                .await;
            let consult_elapsed = consult_start.elapsed();

            let findings = match consultation_result {
                Ok(Some(f)) => {
                    let finding_count = f.lines().filter(|l| l.starts_with("- ")).count().max(1);
                    self.sdk_runtime.ui().consultation_complete(
                        &consultation.label,
                        finding_count,
                        consult_elapsed,
                    );
                    let preview = if f.len() > 300 {
                        format!("{}...", &f[..297])
                    } else {
                        f.clone()
                    };
                    self.sdk_runtime.ui().sdk_text(&preview);
                    f
                }
                Ok(None) => {
                    self.sdk_runtime.ui().consultation_complete(
                        &consultation.label,
                        0,
                        consult_elapsed,
                    );
                    self.round += 1;
                    continue;
                }
                Err(fatal_err) => {
                    self.sdk_runtime.ui().consultation_error(
                        &consultation.label,
                        &fatal_err,
                    );
                    return SessionOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: fatal_err,
                        decisions: vec![],
                    };
                }
            };

            tracing::info!(
                label = %consultation.label,
                findings_len = findings.len(),
                findings = %findings,
                "SDK consultation produced findings"
            );

            let Some(ref session_id) = current_session_id else {
                tracing::warn!("No SDK session ID available for resume after consultation");
                break;
            };

            let role_config = self.sdk_runtime.config_for_role(role);
            let provider = role_config.provider.clone();
            self.sdk_runtime.ui().sdk_session_info(&provider, &role_config.model);
            self.sdk_runtime.ui().sdk_text("Session resumed with consultation findings");

            let resume_prompt = consultation
                .resume_message_template
                .replace("{findings}", &findings);

            let preview = if resume_prompt.len() > 200 {
                format!("{}...", &resume_prompt[..197])
            } else {
                resume_prompt.clone()
            };
            self.sdk_runtime.ui().chat_turn(self.round as u32 + 1, &preview);

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

        self.sdk_runtime.set_suppress_activation_ui(false);
        current_outcome
    }

    async fn capture_trigger_text(
        &self,
        story: &StoryInfo,
        _phase: &str,
        completion_text: Option<&str>,
    ) -> String {
        let mut text = String::new();

        if let Ok(content) = tokio::fs::read_to_string(&story.specs_path).await {
            text.push_str(&content);
        }

        if let Some(completion) = completion_text {
            text.push('\n');
            text.push_str(completion);
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

    async fn run_consultation(
        &self,
        consultation: &ConsultationConfig,
        story: &StoryInfo,
        trigger_text: &str,
        agent_factory: &std::sync::Arc<crate::llm::AgentFactory>,
    ) -> Result<Option<String>, String> {
        let role_config =
            super::resolve_role_config(&self.sdk_runtime.config().llm, &consultation.role);

        if role_config.is_sdk_provider() {
            self.run_sdk_consultation(consultation, role_config, trigger_text)
                .await
        } else {
            Ok(self
                .run_api_consultation(consultation, story, trigger_text, agent_factory)
                .await)
        }
    }

    async fn run_sdk_consultation(
        &self,
        consultation: &ConsultationConfig,
        role_config: &crate::config::LlmRoleConfig,
        _trigger_text: &str,
    ) -> Result<Option<String>, String> {
        let prompt = if let Some(skill) = consultation_skill_command(&consultation.label) {
            let target_file = consultation.context_files.first().cloned().unwrap_or_default();
            format!("{skill} {target_file}")
        } else {
            let mut context_parts = Vec::new();
            for file_path in &consultation.context_files {
                match tokio::fs::read_to_string(file_path).await {
                    Ok(content) => {
                        context_parts.push(format!("--- {file_path} ---\n{content}"));
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
            let context = context_parts.join("\n\n");
            let rendered = consultation.prompt_template.replace("{context}", &context);
            let preamble = consultation
                .preamble_override
                .as_deref()
                .unwrap_or("You are an independent reviewer. Analyze the provided context and report your findings.");
            format!("{preamble}\n\n{rendered}")
        };

        self.sdk_runtime.ui().sdk_input(
            &if prompt.len() > 120 { format!("{}...", &prompt[..117]) } else { prompt.clone() },
        );

        let project_root =
            std::path::Path::new(&self.sdk_runtime.config().bmad_paths.project_root);

        let session_config = match role_config.provider.as_str() {
            "claude-code" => build_sdk_consultation_claude(role_config, project_root, &prompt),
            "codex" => build_sdk_consultation_codex(role_config, project_root, &prompt),
            other => {
                tracing::error!(
                    provider = %other,
                    label = %consultation.label,
                    "Unknown SDK provider for consultation"
                );
                return Ok(None);
            }
        };

        tracing::info!(
            label = %consultation.label,
            provider = %role_config.provider,
            "Running SDK consultation subprocess"
        );
        self.sdk_runtime.ui().sdk_session_info(&role_config.provider, &role_config.model);

        let is_claude = role_config.provider == "claude-code";

        let parser = move |line: &str| -> Option<super::sdk::SdkOutputEvent> {
            if is_claude {
                super::sdk_claude::parse_claude_code_line(line)
            } else {
                super::sdk_codex::parse_codex_line(line)
            }
        };

        match self.sdk_runtime.execute_session(session_config, parser).await {
            Ok(result) => {
                tracing::info!(
                    label = %consultation.label,
                    exit_code = ?result.exit_code,
                    has_completion = result.completion_text.is_some(),
                    completion_len = result.completion_text.as_deref().map(|s| s.len()).unwrap_or(0),
                    stream_error = ?result.stream_error,
                    stderr_len = result.stderr.len(),
                    "SDK consultation subprocess finished"
                );
                if result.exit_code == Some(0) {
                    let findings = result.completion_text.unwrap_or_default();
                    if findings.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(findings))
                    }
                } else {
                    let err = result.stream_error
                        .or_else(|| if result.stderr.is_empty() { None } else { Some(result.stderr) })
                        .unwrap_or_else(|| format!("exit code {:?}", result.exit_code));
                    tracing::error!(
                        label = %consultation.label,
                        error = %err,
                        "SDK consultation subprocess failed"
                    );
                    self.sdk_runtime.ui().consultation_error(&consultation.label, &err);
                    let lower = err.to_lowercase();
                    if lower.contains("model is not supported")
                        || lower.contains("invalid_request_error")
                    {
                        Err(err)
                    } else {
                        Ok(None)
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    label = %consultation.label,
                    "SDK consultation subprocess spawn failed"
                );
                Err(e.to_string())
            }
        }
    }

    async fn run_api_consultation(
        &self,
        consultation: &ConsultationConfig,
        story: &StoryInfo,
        trigger_text: &str,
        agent_factory: &std::sync::Arc<crate::llm::AgentFactory>,
    ) -> Option<String> {
        let consultation_runner = crate::session::consultation::ConsultationRunner::new(
            agent_factory.clone(),
            self.sdk_runtime.shutdown_flag().clone(),
            crate::ui::UiHandle::null(),
            std::path::PathBuf::from(&self.sdk_runtime.config().bmad_paths.project_root),
        );

        let _ = trigger_text;
        let _ = story;
        match consultation_runner.execute(consultation).await {
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
                    "API consultation failed"
                );
                None
            }
        }
    }
}

fn consultation_skill_command(label: &str) -> Option<&'static str> {
    match label {
        "adversarial" => Some("/bmad-review-adversarial-general"),
        "review-critic" => Some("/bmad-code-review"),
        _ => None,
    }
}

fn build_sdk_consultation_claude(
    role_config: &crate::config::LlmRoleConfig,
    project_root: &std::path::Path,
    prompt: &str,
) -> super::sdk::SdkSessionConfig {
    let command = role_config
        .cli_path
        .clone()
        .unwrap_or_else(|| "claude".to_string());

    let args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--verbose".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--model".to_string(),
        role_config.model.clone(),
        "--permission-mode".to_string(),
        "acceptEdits".to_string(),
        "--max-turns".to_string(),
        "50".to_string(),
    ];

    super::sdk::SdkSessionConfig {
        command,
        args,
        env: Vec::new(),
        working_directory: project_root.to_path_buf(),
        timeout: std::time::Duration::from_secs(10 * 60),
        sigterm_grace: std::time::Duration::from_secs(10),
        stdin_data: None,
    }
}

fn build_sdk_consultation_codex(
    role_config: &crate::config::LlmRoleConfig,
    project_root: &std::path::Path,
    prompt: &str,
) -> super::sdk::SdkSessionConfig {
    let command = role_config
        .cli_path
        .clone()
        .unwrap_or_else(|| "codex".to_string());

    let args = vec![
        "exec".to_string(),
        "--json".to_string(),
        "--dangerously-bypass-approvals-and-sandbox".to_string(),
        "--model".to_string(),
        role_config.model.clone(),
        "--cd".to_string(),
        project_root.to_string_lossy().to_string(),
    ];

    super::sdk::SdkSessionConfig {
        command,
        args,
        env: Vec::new(),
        working_directory: project_root.to_path_buf(),
        stdin_data: Some(prompt.to_string()),
        timeout: std::time::Duration::from_secs(10 * 60),
        sigterm_grace: std::time::Duration::from_secs(10),
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
