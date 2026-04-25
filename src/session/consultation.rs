//! Daemon-orchestrated consultation mechanism (Architecture Decision 10).
//!
//! Provides the infrastructure for pausing an active session, running a fresh
//! consultation agent (e.g., adversarial reviewer, story critic), and feeding
//! findings back into the paused session via `ResponseAction::Continue { reply }`.

use crate::configure_agent_tools;
use crate::llm::agent_factory::{AgentFactory, LlmRole, ShutdownFlag};
use crate::llm::context::ContextBuilder;
use crate::tools::{EditFileTool, FindPathTool, GrepTool, ListDirectoryTool, ReadFileTool};
use crate::ui::UiHandle;
use rig::completion::Message;
use rig::tools::think::ThinkTool;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::agent::create_base_tools;

// ---------------------------------------------------------------------------
// ConsultationToolSet
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsultationToolSet {
    /// All base tools + ThinkTool: git, read_file, edit_file, grep, find_path,
    /// list_directory, terminal. Same set as sub-agents (Story 12.3).
    Full,
    /// Restricted tool set: read_file, edit_file, grep, find_path,
    /// list_directory + ThinkTool. No git, no terminal. Includes edit_file because
    /// the Critic needs it for critic-memory.md updates (Story 13.9).
    Restricted,
}

// ---------------------------------------------------------------------------
// ConsultationConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ConsultationConfig {
    pub label: String,
    pub skill_path: Option<String>,
    pub preamble_override: Option<String>,
    pub role: LlmRole,
    pub tool_set: ConsultationToolSet,
    pub context_files: Vec<String>,
    pub trigger_pattern: String,
    pub prompt_template: String,
    pub resume_message_template: String,
    /// WAL pipeline phase to set when this consultation triggers.
    /// When `Some`, `check_consultation_triggers()` updates the WAL phase
    /// before executing. When `None`, the WAL phase is unchanged.
    pub pipeline_phase: Option<String>,
}

impl ConsultationConfig {
    pub fn trigger_regex(&self) -> Result<regex::Regex, regex::Error> {
        regex::Regex::new(&self.trigger_pattern)
    }

    #[cfg(test)]
    pub fn trigger_matches(&self, response: &str) -> bool {
        self.trigger_regex()
            .map(|re| re.is_match(response))
            .unwrap_or(false)
    }

    pub fn format_findings(&self, findings: &str) -> String {
        self.resume_message_template.replace("{findings}", findings)
    }

    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.skill_path.is_none() && self.preamble_override.is_none() {
            warnings.push(format!(
                "Consultation '{}': neither skill_path nor preamble_override set — agent will have empty preamble",
                self.label
            ));
        }
        if self.skill_path.is_some() && self.preamble_override.is_some() {
            warnings.push(format!(
                "Consultation '{}': both skill_path and preamble_override set — skill_path takes precedence",
                self.label
            ));
        }
        warnings
    }
}

// ---------------------------------------------------------------------------
// ConsultationError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ConsultationError {
    #[error("Failed to build consultation agent '{label}': {reason}")]
    AgentBuildFailed { label: String, reason: String },

    #[error("Failed to activate consultation agent '{label}': {reason}")]
    ActivationFailed { label: String, reason: String },

    #[error("Consultation '{label}' stream_chat failed: {reason}")]
    StreamChatFailed { label: String, reason: String },

    #[error("Context file not found for consultation '{label}': {path}")]
    ContextFileNotFound { label: String, path: String },

    #[error("Shutdown requested during consultation '{label}'")]
    ShutdownRequested { label: String },
}

// ---------------------------------------------------------------------------
// ConsultationState
// ---------------------------------------------------------------------------

pub(crate) struct ConsultationState {
    pub config: ConsultationConfig,
    pub triggered: bool,
    pub compiled_regex: regex::Regex,
}

impl ConsultationState {
    pub fn from_configs(configs: Vec<ConsultationConfig>) -> Vec<Self> {
        configs
            .into_iter()
            .filter_map(|config| {
                for warning in config.validate() {
                    tracing::warn!(label = %config.label, "{}", warning);
                }
                match config.trigger_regex() {
                    Ok(regex) => Some(Self {
                        config,
                        triggered: false,
                        compiled_regex: regex,
                    }),
                    Err(e) => {
                        tracing::error!(
                            label = %config.label,
                            pattern = %config.trigger_pattern,
                            error = %e,
                            "Invalid consultation trigger regex — skipping"
                        );
                        None
                    }
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// ConsultationRunner
// ---------------------------------------------------------------------------

const MAX_CONSULTATION_TURNS: usize = 30;
const JOB_DONE_SENTINEL: &str = "<<BMAD_JOB_DONE>>";

pub struct ConsultationRunner {
    agent_factory: Arc<AgentFactory>,
    shutdown: ShutdownFlag,
    ui: UiHandle,
    project_root: PathBuf,
}

impl ConsultationRunner {
    pub fn new(
        agent_factory: Arc<AgentFactory>,
        shutdown: ShutdownFlag,
        ui: UiHandle,
        project_root: PathBuf,
    ) -> Self {
        Self {
            agent_factory,
            shutdown,
            ui,
            project_root,
        }
    }

    pub async fn execute(&self, config: &ConsultationConfig) -> Result<String, ConsultationError> {
        // 1. Validate & build preamble
        let preamble = if config.skill_path.is_some() {
            if config.preamble_override.is_some() {
                tracing::warn!(
                    label = %config.label,
                    "Both skill_path and preamble_override set — skill_path takes precedence"
                );
            }
            build_consultation_preamble()
        } else if let Some(ref override_preamble) = config.preamble_override {
            override_preamble.clone()
        } else {
            return Err(ConsultationError::AgentBuildFailed {
                label: config.label.clone(),
                reason: "neither skill_path nor preamble_override provided".to_string(),
            });
        };

        // 2. Build tools & agent
        let agent = match config.tool_set {
            ConsultationToolSet::Full => {
                let (git, read_file, edit_file, grep, find_path, list_dir, terminal) =
                    create_base_tools(&self.project_root);
                self.agent_factory
                    .build(
                        config.role,
                        &preamble,
                        configure_agent_tools!(
                            git, read_file, edit_file, grep, find_path, list_dir, terminal,
                            ThinkTool
                        ),
                    )
                    .await
            }
            ConsultationToolSet::Restricted => {
                let read_file = ReadFileTool::new(self.project_root.clone());
                let edit_file = EditFileTool::new(self.project_root.clone());
                let grep = GrepTool::new(self.project_root.clone());
                let find_path = FindPathTool::new(self.project_root.clone());
                let list_dir = ListDirectoryTool::new(self.project_root.clone());
                self.agent_factory
                    .build(
                        config.role,
                        &preamble,
                        configure_agent_tools!(
                            read_file, edit_file, grep, find_path, list_dir, ThinkTool
                        ),
                    )
                    .await
            }
        }
        .map_err(|e| ConsultationError::AgentBuildFailed {
            label: config.label.clone(),
            reason: e.to_string(),
        })?;

        // 3. Activate (if skill-based)
        let mut history: Vec<Message> = Vec::new();
        if let Some(ref skill_path) = config.skill_path {
            let (activation_history, _chat_messages) = agent
                .activate_agent(
                    self.project_root.to_str().unwrap_or(""),
                    skill_path,
                    &config.label,
                    Some(&self.shutdown),
                    Some(&self.ui),
                )
                .await
                .map_err(|e| ConsultationError::ActivationFailed {
                    label: config.label.clone(),
                    reason: e,
                })?;
            history = activation_history;
        }

        // 4. Build context
        let context_xml = self.build_context_xml(config)?;

        // 5. Render prompt
        let rendered_prompt = config.prompt_template.replace("{context}", &context_xml);

        // 6. Run consultation loop
        let mut last_response = String::new();
        for turn in 0..MAX_CONSULTATION_TURNS {
            let msg = if turn == 0 {
                rendered_prompt.clone()
            } else {
                "Continue.".to_string()
            };

            if self.shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(ConsultationError::ShutdownRequested {
                    label: config.label.clone(),
                });
            }

            let (text, new_history) = agent
                .stream_chat(
                    Message::user(msg),
                    history,
                    Some(&self.shutdown),
                    Some(&self.ui),
                )
                .await
                .map_err(|e| ConsultationError::StreamChatFailed {
                    label: config.label.clone(),
                    reason: e.to_string(),
                })?;

            history = new_history;
            last_response = text;

            if last_response.contains(JOB_DONE_SENTINEL) {
                break;
            }
        }

        let findings = last_response
            .replace(JOB_DONE_SENTINEL, "")
            .trim()
            .to_string();
        Ok(findings)
    }

    fn build_context_xml(&self, config: &ConsultationConfig) -> Result<String, ConsultationError> {
        let mut builder = ContextBuilder::new().description("Consultation context");
        for file_path in &config.context_files {
            let path = Path::new(file_path);
            if !path.exists() {
                return Err(ConsultationError::ContextFileNotFound {
                    label: config.label.clone(),
                    path: file_path.clone(),
                });
            }
            builder = builder.add_file_from_disk(path).map_err(|e| {
                ConsultationError::ContextFileNotFound {
                    label: config.label.clone(),
                    path: format!("{file_path}: {e}"),
                }
            })?;
        }
        Ok(builder.build())
    }
}

fn build_consultation_preamble() -> String {
    r#"You are an AI consultation agent providing an independent review.

## Tool Usage Rules
- Use read_file to examine files before commenting on them
- Use edit_file only for files you are explicitly allowed to modify
- Use grep and find_path to discover relevant code
- Think carefully before making recommendations

## Communication
- Respond in English
- Be direct, specific, and constructive
- Output your complete findings in a single response when possible
- Signal completion with <<BMAD_JOB_DONE>> when finished"#
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ConsultationConfig {
        ConsultationConfig {
            label: "test".to_string(),
            skill_path: None,
            preamble_override: Some("You are a reviewer".to_string()),
            role: LlmRole::Review,
            tool_set: ConsultationToolSet::Full,
            context_files: vec![],
            trigger_pattern: r"(?i)story\s+file\s+created".to_string(),
            prompt_template: "Review this: {context}".to_string(),
            resume_message_template: "Findings:\n{findings}\nPlease fix.".to_string(),
            pipeline_phase: None,
        }
    }

    // -- Trigger matching --

    #[test]
    fn test_consultation_config_trigger_matches() {
        let config = default_config();
        assert!(config.trigger_matches("The story file created successfully."));
        assert!(config.trigger_matches("Story File Created and validated."));
    }

    #[test]
    fn test_consultation_config_trigger_no_match() {
        let config = default_config();
        assert!(!config.trigger_matches("Working on implementation..."));
        assert!(!config.trigger_matches("Creating the file now"));
    }

    #[test]
    fn test_consultation_config_trigger_complex_regex() {
        let config = ConsultationConfig {
            trigger_pattern: r"(?i)(story\s+file|spec)\s+(created|generated|written)".to_string(),
            ..default_config()
        };
        assert!(config.trigger_matches("The spec generated for review."));
        assert!(config.trigger_matches("Story file written to disk."));
        assert!(!config.trigger_matches("Generating the spec now..."));
    }

    // -- Findings formatting --

    #[test]
    fn test_consultation_config_format_findings() {
        let config = ConsultationConfig {
            resume_message_template: "An external reviewer found:\n\n{findings}\n\nPlease fix."
                .to_string(),
            ..default_config()
        };
        let result = config.format_findings("Issue 1: Missing AC\nIssue 2: Wrong lib");
        assert!(result.contains("Issue 1: Missing AC"));
        assert!(result.starts_with("An external reviewer found:"));
        assert!(result.ends_with("Please fix."));
    }

    #[test]
    fn test_consultation_config_format_findings_no_placeholder() {
        let config = ConsultationConfig {
            resume_message_template: "No placeholder here.".to_string(),
            ..default_config()
        };
        let result = config.format_findings("Some findings");
        assert_eq!(result, "No placeholder here.");
    }

    // -- Error display --

    #[test]
    fn test_consultation_error_display() {
        let err = ConsultationError::AgentBuildFailed {
            label: "critic".to_string(),
            reason: "invalid API key".to_string(),
        };
        assert!(err.to_string().contains("critic"));
        assert!(err.to_string().contains("invalid API key"));

        let err = ConsultationError::ActivationFailed {
            label: "adversarial".to_string(),
            reason: "skill not found".to_string(),
        };
        assert!(err.to_string().contains("adversarial"));
        assert!(err.to_string().contains("skill not found"));

        let err = ConsultationError::StreamChatFailed {
            label: "reviewer".to_string(),
            reason: "connection timeout".to_string(),
        };
        assert!(err.to_string().contains("reviewer"));
        assert!(err.to_string().contains("connection timeout"));

        let err = ConsultationError::ContextFileNotFound {
            label: "critic".to_string(),
            path: "/missing/file.md".to_string(),
        };
        assert!(err.to_string().contains("critic"));
        assert!(err.to_string().contains("/missing/file.md"));

        let err = ConsultationError::ShutdownRequested {
            label: "adversarial".to_string(),
        };
        assert!(err.to_string().contains("adversarial"));
        assert!(err.to_string().contains("Shutdown"));
    }

    // -- Tool set variants --

    #[test]
    fn test_consultation_tool_set_variants() {
        let full = ConsultationToolSet::Full;
        let restricted = ConsultationToolSet::Restricted;
        assert_ne!(full, restricted);
        assert_eq!(full, ConsultationToolSet::Full);
        assert_eq!(restricted, ConsultationToolSet::Restricted);
        // Debug impl
        let debug = format!("{full:?}");
        assert!(debug.contains("Full"));
    }

    // -- Sentinel stripping --

    #[test]
    fn test_strip_job_done_sentinel() {
        let raw = "Here are my findings.\n\n<<BMAD_JOB_DONE>>";
        let stripped = raw.replace("<<BMAD_JOB_DONE>>", "").trim().to_string();
        assert_eq!(stripped, "Here are my findings.");

        let raw_embedded = "Findings:\n<<BMAD_JOB_DONE>>\nExtra text";
        let stripped = raw_embedded
            .replace("<<BMAD_JOB_DONE>>", "")
            .trim()
            .to_string();
        assert_eq!(stripped, "Findings:\n\nExtra text");
    }

    // -- Validation --

    #[test]
    fn test_consultation_config_validate_both_none() {
        let config = ConsultationConfig {
            skill_path: None,
            preamble_override: None,
            ..default_config()
        };
        let warnings = config.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("neither skill_path nor preamble_override"));
    }

    #[test]
    fn test_consultation_config_validate_both_some() {
        let config = ConsultationConfig {
            skill_path: Some("skill.md".to_string()),
            preamble_override: Some("You are a reviewer".to_string()),
            ..default_config()
        };
        let warnings = config.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("skill_path takes precedence"));
    }

    #[test]
    fn test_consultation_config_validate_ok() {
        let config = ConsultationConfig {
            skill_path: Some("skill.md".to_string()),
            preamble_override: None,
            ..default_config()
        };
        assert!(config.validate().is_empty());
    }

    // -- ConsultationState --

    #[test]
    fn test_consultation_state_from_configs_valid() {
        let configs = vec![
            ConsultationConfig {
                trigger_pattern: "foo".to_string(),
                ..default_config()
            },
            ConsultationConfig {
                trigger_pattern: "bar".to_string(),
                ..default_config()
            },
        ];
        let states = ConsultationState::from_configs(configs);
        assert_eq!(states.len(), 2);
        assert!(!states[0].triggered);
        assert!(!states[1].triggered);
    }

    #[test]
    fn test_consultation_state_from_configs_invalid_regex_skipped() {
        let configs = vec![
            ConsultationConfig {
                trigger_pattern: "[invalid".to_string(),
                ..default_config()
            },
            ConsultationConfig {
                trigger_pattern: "valid".to_string(),
                ..default_config()
            },
        ];
        let states = ConsultationState::from_configs(configs);
        assert_eq!(states.len(), 1);
    }
}
