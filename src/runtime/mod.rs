pub mod sdk;
pub mod sdk_claude;
pub mod sdk_codex;
pub mod sdk_wal;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{BotConfig, BotSecrets};
use crate::llm::AgentFactory;
use crate::llm::agent_factory::LlmRole;
use crate::session::agent::ShutdownFlag;
use crate::session::runner::SessionRunner;
use crate::ui::UiHandle;

pub use sdk::SdkRuntime;

// ---------------------------------------------------------------------------
// RuntimeDeps — shared daemon dependencies for SessionRuntime construction
// ---------------------------------------------------------------------------

/// Shared daemon dependencies needed to construct a [`SessionRuntime`].
pub struct RuntimeDeps {
    pub config: Arc<BotConfig>,
    pub secrets: Arc<BotSecrets>,
    pub config_path: PathBuf,
    pub shutdown: ShutdownFlag,
    pub mcp_manager: Arc<crate::mcp::McpManager>,
    pub sub_agent_sessions: Arc<
        std::sync::Mutex<std::collections::HashMap<String, crate::tools::SubAgentState>>,
    >,
    pub sub_agent_in_flight: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub ui: UiHandle,
}

// ---------------------------------------------------------------------------
// SkillPaths — resolve-once, use-everywhere skill path registry
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SkillPaths {
    pub dev_story: String,
    pub create_story: String,
    pub code_review: String,
}

impl SkillPaths {
    pub fn resolve(project_root: &Path) -> Self {
        let manifest_path = project_root.join("_bmad/_config/manifest.yaml");
        let base = match Self::read_skill_base(&manifest_path) {
            Some(base) => base,
            None => {
                tracing::warn!(
                    "BMAD manifest not found or empty ides[], falling back to .claude/skills/"
                );
                ".claude/skills".to_string()
            }
        };
        Self {
            dev_story: format!("{base}/bmad-dev-story/SKILL.md"),
            create_story: format!("{base}/bmad-create-story/SKILL.md"),
            code_review: format!("{base}/bmad-code-review/SKILL.md"),
        }
    }

    fn read_skill_base(manifest_path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(manifest_path).ok()?;
        let doc: serde_yml::Value = serde_yml::from_str(&content).ok()?;
        let ides = doc.get("ides")?.as_sequence()?;
        let first_ide = ides.first()?.as_str()?;
        Some(Self::ide_to_skill_dir(first_ide).to_string())
    }

    fn ide_to_skill_dir(ide: &str) -> &str {
        crate::config::sdk_provider_skill_dir(ide)
    }
}

// ---------------------------------------------------------------------------
// RawSessionResult — runtime-agnostic session output (Phase B)
// ---------------------------------------------------------------------------

/// Raw result from a runtime execution. No interpretation, no business logic.
/// The pipeline interprets this into a `SessionOutcome`.
pub type RawSessionResult = sdk::SdkSessionResult;

// ---------------------------------------------------------------------------
// RuntimeCommand — what the pipeline asks a runtime to do (Phase B)
// ---------------------------------------------------------------------------

/// Command dispatched by the pipeline to a runtime for execution.
#[derive(Debug)]
pub enum RuntimeCommand {
    /// Start a fresh session.
    Start {
        role: LlmRole,
        phase: String,
        story_key: String,
        prompt: String,
        skill_path: Option<String>,
        preamble: Option<String>,
        needs_supervisor: bool,
    },
    /// Resume an existing session with additional input.
    Resume {
        session_id: String,
        prompt: String,
        role: LlmRole,
        story_key: String,
    },
}

// ---------------------------------------------------------------------------
// SessionRuntime — dual runtime dispatch
// ---------------------------------------------------------------------------

pub enum SessionRuntime {
    Api(Box<ApiRuntime>),
    Sdk(SdkRuntime),
    Dual {
        api: Box<ApiRuntime>,
        sdk: SdkRuntime,
        config: Arc<BotConfig>,
    },
}

impl SessionRuntime {
    /// Execute a single runtime command and return raw results.
    /// No interpretation, no auto-response, no consultations.
    /// The pipeline layer handles all orchestration on top of this.
    pub async fn execute(&self, command: RuntimeCommand) -> RawSessionResult {
        match &command {
            RuntimeCommand::Start { role, phase, story_key, prompt, .. } => {
                let head = &prompt[..prompt.len().min(200)];
                tracing::info!(
                    action = "runtime_execute_start",
                    role = ?role,
                    phase = %phase,
                    story_key = %story_key,
                    prompt_len = prompt.len(),
                    prompt_head = %head,
                    "Sending Start command to runtime"
                );
                if let Some(ui) = self.ui() {
                    ui.sdk_text(&format!("→ Start [{phase}] {head}"));
                }
            }
            RuntimeCommand::Resume { session_id, prompt, story_key, .. } => {
                let head = &prompt[..prompt.len().min(200)];
                tracing::info!(
                    action = "runtime_execute_resume",
                    session_id = %session_id,
                    story_key = %story_key,
                    prompt_len = prompt.len(),
                    prompt_head = %head,
                    "Sending Resume command to runtime"
                );
                if let Some(ui) = self.ui() {
                    ui.sdk_text(&format!("→ Resume {head}"));
                }
            }
        }
        let role = match &command {
            RuntimeCommand::Start { role, .. } => role.clone(),
            RuntimeCommand::Resume { role, .. } => role.clone(),
        };
        match self {
            Self::Sdk(sdk) => sdk.execute_command(command).await,
            Self::Api(_api) => {
                // TODO Phase C: implement API runtime execute
                RawSessionResult {
                    session_id: None,
                    exit_code: Some(1),
                    stderr: "API runtime execute() not yet implemented".to_string(),
                    timed_out: false,
                    shutdown_requested: false,
                    completion_text: None,
                    stream_error: Some("Not implemented".to_string()),
                    rate_limit_resets_at: None,
                }
            }
            Self::Dual { sdk, config, .. } => {
                let role_config = resolve_role_config(&config.llm, &role);
                if role_config.is_sdk_provider() {
                    sdk.execute_command(command).await
                } else {
                    // TODO Phase C: implement API runtime execute
                    RawSessionResult {
                        session_id: None,
                        exit_code: Some(1),
                        stderr: "API runtime execute() not yet implemented".to_string(),
                        timed_out: false,
                        shutdown_requested: false,
                        completion_text: None,
                        stream_error: Some("Not implemented".to_string()),
                        rate_limit_resets_at: None,
                    }
                }
            }
        }
    }

    pub fn api_session_runner(&self) -> Option<&SessionRunner> {
        match self {
            Self::Api(api) => Some(api.session_runner()),
            Self::Sdk(_) => None,
            Self::Dual { api, .. } => Some(api.session_runner()),
        }
    }

    pub fn sdk_runtime(&self) -> Option<&SdkRuntime> {
        match self {
            Self::Api(_) => None,
            Self::Sdk(sdk) => Some(sdk),
            Self::Dual { sdk, .. } => Some(sdk),
        }
    }

    fn ui(&self) -> Option<&UiHandle> {
        match self {
            Self::Sdk(sdk) => Some(sdk.ui_handle()),
            Self::Dual { sdk, .. } => Some(sdk.ui_handle()),
            Self::Api(_) => None,
        }
    }

    pub fn from_config(deps: RuntimeDeps) -> Self {
        let RuntimeDeps {
            config,
            secrets,
            config_path,
            shutdown,
            mcp_manager,
            sub_agent_sessions,
            sub_agent_in_flight,
            ui,
        } = deps;
        let llm = &config.llm;
        let all_roles = [&llm.dev, &llm.review, &llm.supervisor];
        let optional_roles: Vec<&crate::config::LlmRoleConfig> =
            [&llm.epic_review, &llm.critic, &llm.utility]
                .into_iter()
                .filter(|r| !r.provider.is_empty())
                .collect();

        let has_api = all_roles.iter().any(|r| r.is_api_provider())
            || optional_roles.iter().any(|r| r.is_api_provider());
        let has_sdk = all_roles.iter().any(|r| r.is_sdk_provider())
            || optional_roles.iter().any(|r| r.is_sdk_provider());

        let skill_paths = SkillPaths::resolve(Path::new(&config.bmad_paths.project_root));

        let build_api = || -> Box<ApiRuntime> {
            let agent_factory =
                Arc::new(AgentFactory::new(Arc::clone(&config), Arc::clone(&secrets)));
            let session_runner = SessionRunner::new(
                Arc::clone(&config),
                agent_factory,
                Arc::clone(&shutdown),
                Arc::clone(&mcp_manager),
                Arc::clone(&sub_agent_sessions),
                Arc::clone(&sub_agent_in_flight),
                ui.clone(),
                skill_paths.dev_story.clone(),
            );
            Box::new(ApiRuntime::new(session_runner, skill_paths.clone()))
        };

        let build_sdk = || -> SdkRuntime {
            SdkRuntime::new(
                Arc::clone(&config),
                Arc::clone(&secrets),
                config_path.clone(),
                Arc::clone(&shutdown),
                ui.clone(),
            )
        };

        match (has_api, has_sdk) {
            (true, false) => Self::Api(build_api()),
            (false, true) => Self::Sdk(build_sdk()),
            (true, true) => Self::Dual {
                api: build_api(),
                sdk: build_sdk(),
                config: Arc::clone(&config),
            },
            (false, false) => {
                tracing::warn!("No recognized providers configured, defaulting to API runtime");
                Self::Api(build_api())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ApiRuntime — thin wrapper around the existing rig-based SessionRunner
// ---------------------------------------------------------------------------

pub struct ApiRuntime {
    session_runner: SessionRunner,
    skill_paths: SkillPaths,
}

impl ApiRuntime {
    pub fn new(session_runner: SessionRunner, skill_paths: SkillPaths) -> Self {
        Self {
            session_runner,
            skill_paths,
        }
    }

    pub fn session_runner(&self) -> &SessionRunner {
        &self.session_runner
    }
}

// ---------------------------------------------------------------------------
// Role → config helper (avoids circular dependency config→llm)
// ---------------------------------------------------------------------------

pub(crate) fn resolve_role_config<'a>(
    llm: &'a crate::config::LlmConfig,
    role: &LlmRole,
) -> &'a crate::config::LlmRoleConfig {
    match role {
        LlmRole::Dev => &llm.dev,
        LlmRole::Review => &llm.review,
        LlmRole::Supervisor => &llm.supervisor,
        LlmRole::EpicReview => {
            if llm.epic_review.provider.is_empty() {
                &llm.review
            } else {
                &llm.epic_review
            }
        }
        LlmRole::Critic => {
            if llm.critic.provider.is_empty() {
                &llm.review
            } else {
                &llm.critic
            }
        }
        LlmRole::Utility => {
            if llm.utility.provider.is_empty() {
                &llm.review
            } else {
                &llm.utility
            }
        }
    }
}

// SdkRuntime re-exported from sdk module above

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_skill_paths_resolve_claude_code() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_dir = dir.path().join("_bmad/_config");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(
            manifest_dir.join("manifest.yaml"),
            "ides:\n  - claude-code\n",
        )
        .unwrap();

        let paths = SkillPaths::resolve(dir.path());
        assert_eq!(paths.dev_story, ".claude/skills/bmad-dev-story/SKILL.md");
        assert_eq!(
            paths.create_story,
            ".claude/skills/bmad-create-story/SKILL.md"
        );
        assert_eq!(
            paths.code_review,
            ".claude/skills/bmad-code-review/SKILL.md"
        );
    }

    #[test]
    fn test_skill_paths_resolve_codex() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_dir = dir.path().join("_bmad/_config");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(manifest_dir.join("manifest.yaml"), "ides:\n  - codex\n").unwrap();

        let paths = SkillPaths::resolve(dir.path());
        assert_eq!(paths.dev_story, ".agents/skills/bmad-dev-story/SKILL.md");
        assert_eq!(
            paths.create_story,
            ".agents/skills/bmad-create-story/SKILL.md"
        );
        assert_eq!(
            paths.code_review,
            ".agents/skills/bmad-code-review/SKILL.md"
        );
    }

    #[test]
    fn test_skill_paths_resolve_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let paths = SkillPaths::resolve(dir.path());
        assert_eq!(paths.dev_story, ".claude/skills/bmad-dev-story/SKILL.md");
        assert_eq!(
            paths.create_story,
            ".claude/skills/bmad-create-story/SKILL.md"
        );
        assert_eq!(
            paths.code_review,
            ".claude/skills/bmad-code-review/SKILL.md"
        );
    }

    #[test]
    fn test_skill_paths_resolve_empty_ides() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_dir = dir.path().join("_bmad/_config");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(manifest_dir.join("manifest.yaml"), "ides: []\n").unwrap();

        let paths = SkillPaths::resolve(dir.path());
        assert_eq!(paths.dev_story, ".claude/skills/bmad-dev-story/SKILL.md");
    }

    #[test]
    fn test_ide_to_skill_dir_mapping() {
        assert_eq!(
            SkillPaths::ide_to_skill_dir("claude-code"),
            ".claude/skills"
        );
        assert_eq!(SkillPaths::ide_to_skill_dir("codex"), ".agents/skills");
        assert_eq!(
            SkillPaths::ide_to_skill_dir("unknown-ide"),
            ".claude/skills"
        );
    }

    #[test]
    fn test_session_runtime_api_variant_construction() {
        let dir = tempfile::tempdir().unwrap();
        let skill_paths = SkillPaths::resolve(dir.path());
        let config = make_test_config(dir.path());
        let runner = make_test_session_runner(&config, &skill_paths);
        let api = ApiRuntime::new(runner, skill_paths);
        let _runtime = SessionRuntime::Api(Box::new(api));
    }

    #[test]
    fn test_session_runtime_sdk_variant_construction() {
        let config = make_test_config(tempfile::tempdir().unwrap().path());
        let secrets = std::sync::Arc::new(crate::config::BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sdk = SdkRuntime::new(
            config,
            secrets,
            PathBuf::from("test-config.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
        );
        let _runtime = SessionRuntime::Sdk(sdk);
    }

    #[test]
    fn test_skill_paths_resolve_first_ide_wins() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_dir = dir.path().join("_bmad/_config");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(
            manifest_dir.join("manifest.yaml"),
            "ides:\n  - codex\n  - claude-code\n",
        )
        .unwrap();

        let paths = SkillPaths::resolve(dir.path());
        assert_eq!(
            paths.dev_story, ".agents/skills/bmad-dev-story/SKILL.md",
            "first IDE in list should win"
        );
    }

    // -- Test helpers --

    fn make_test_config(dir: &Path) -> std::sync::Arc<crate::config::BotConfig> {
        let mut config = crate::config::BotConfig::_test_minimal("pretty", "info");
        config.bmad_paths.implementation_artifacts = dir.to_string_lossy().to_string();
        config.bmad_paths.project_root = dir.to_string_lossy().to_string();
        std::sync::Arc::new(config)
    }

    fn make_test_session_runner(
        config: &std::sync::Arc<crate::config::BotConfig>,
        skill_paths: &SkillPaths,
    ) -> SessionRunner {
        let secrets = std::sync::Arc::new(crate::config::BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let agent_factory = std::sync::Arc::new(crate::llm::AgentFactory::new(
            std::sync::Arc::clone(config),
            secrets,
        ));
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mcp = std::sync::Arc::new(crate::mcp::McpManager::empty());
        let subs = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<
            String,
            crate::tools::SubAgentState,
        >::new()));
        let inflt = std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashSet::<String>::new(),
        ));
        SessionRunner::new(
            std::sync::Arc::clone(config),
            agent_factory,
            shutdown,
            mcp,
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            skill_paths.dev_story.clone(),
        )
    }

    fn make_test_secrets() -> std::sync::Arc<crate::config::BotSecrets> {
        std::sync::Arc::new(crate::config::BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        })
    }

    fn make_test_deps(
        config: std::sync::Arc<crate::config::BotConfig>,
    ) -> RuntimeDeps {
        RuntimeDeps {
            config,
            secrets: make_test_secrets(),
            config_path: PathBuf::from("test.yaml"),
            shutdown: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mcp_manager: std::sync::Arc::new(crate::mcp::McpManager::empty()),
            sub_agent_sessions: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            sub_agent_in_flight: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )),
            ui: crate::ui::UiHandle::null(),
        }
    }

    // -- Story 15.7: Dual runtime tests --

    #[test]
    fn test_session_runtime_dual_variant_construction() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_test_config(dir.path());
        let secrets = make_test_secrets();
        let skill_paths = SkillPaths::resolve(dir.path());
        let runner = make_test_session_runner(&config, &skill_paths);
        let api = Box::new(ApiRuntime::new(runner, skill_paths));
        let sdk = SdkRuntime::new(
            std::sync::Arc::clone(&config),
            secrets,
            PathBuf::from("test-config.yaml"),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            crate::ui::UiHandle::null(),
        );
        let _runtime = SessionRuntime::Dual { api, sdk, config };
    }

    #[test]
    fn test_api_session_runner_returns_some_for_api() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_test_config(dir.path());
        let skill_paths = SkillPaths::resolve(dir.path());
        let runner = make_test_session_runner(&config, &skill_paths);
        let api = ApiRuntime::new(runner, skill_paths);
        let runtime = SessionRuntime::Api(Box::new(api));
        assert!(runtime.api_session_runner().is_some());
    }

    #[test]
    fn test_api_session_runner_returns_none_for_sdk() {
        let config = make_test_config(tempfile::tempdir().unwrap().path());
        let secrets = make_test_secrets();
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sdk = SdkRuntime::new(
            config,
            secrets,
            PathBuf::from("test-config.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
        );
        let runtime = SessionRuntime::Sdk(sdk);
        assert!(runtime.api_session_runner().is_none());
    }

    #[test]
    fn test_api_session_runner_returns_some_for_dual() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_test_config(dir.path());
        let secrets = make_test_secrets();
        let skill_paths = SkillPaths::resolve(dir.path());
        let runner = make_test_session_runner(&config, &skill_paths);
        let api = Box::new(ApiRuntime::new(runner, skill_paths));
        let sdk = SdkRuntime::new(
            std::sync::Arc::clone(&config),
            std::sync::Arc::clone(&secrets),
            PathBuf::from("test-config.yaml"),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            crate::ui::UiHandle::null(),
        );
        let runtime = SessionRuntime::Dual { api, sdk, config };
        assert!(runtime.api_session_runner().is_some());
    }

    #[test]
    fn test_sdk_runtime_returns_some_for_sdk() {
        let config = make_test_config(tempfile::tempdir().unwrap().path());
        let secrets = make_test_secrets();
        let sdk = SdkRuntime::new(
            config,
            secrets,
            PathBuf::from("test-config.yaml"),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            crate::ui::UiHandle::null(),
        );
        let runtime = SessionRuntime::Sdk(sdk);
        assert!(runtime.sdk_runtime().is_some());
    }

    #[test]
    fn test_sdk_runtime_returns_none_for_api() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_test_config(dir.path());
        let skill_paths = SkillPaths::resolve(dir.path());
        let runner = make_test_session_runner(&config, &skill_paths);
        let runtime = SessionRuntime::Api(Box::new(ApiRuntime::new(runner, skill_paths)));
        assert!(runtime.sdk_runtime().is_none());
    }

    #[test]
    fn test_from_config_all_api_returns_api_variant() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_test_config(dir.path());
        let runtime = SessionRuntime::from_config(make_test_deps(config));
        assert!(runtime.api_session_runner().is_some());
        assert!(runtime.sdk_runtime().is_none());
    }

    #[test]
    fn test_from_config_all_sdk_returns_sdk_variant() {
        let dir = tempfile::tempdir().unwrap();
        let mut raw_config = crate::config::BotConfig::_test_minimal("pretty", "info");
        raw_config.bmad_paths.implementation_artifacts = dir.path().to_string_lossy().to_string();
        raw_config.bmad_paths.project_root = dir.path().to_string_lossy().to_string();
        raw_config.llm.dev.provider = "claude-code".to_string();
        raw_config.llm.review.provider = "claude-code".to_string();
        raw_config.llm.supervisor.provider = "codex".to_string();
        let config = std::sync::Arc::new(raw_config);
        let runtime = SessionRuntime::from_config(make_test_deps(config));
        assert!(runtime.api_session_runner().is_none());
        assert!(runtime.sdk_runtime().is_some());
    }

    #[test]
    fn test_from_config_mixed_returns_dual_variant() {
        let dir = tempfile::tempdir().unwrap();
        let mut raw_config = crate::config::BotConfig::_test_minimal("pretty", "info");
        raw_config.bmad_paths.implementation_artifacts = dir.path().to_string_lossy().to_string();
        raw_config.bmad_paths.project_root = dir.path().to_string_lossy().to_string();
        raw_config.llm.dev.provider = "claude-code".to_string();
        raw_config.llm.review.provider = "anthropic".to_string();
        raw_config.llm.supervisor.provider = "anthropic".to_string();
        let config = std::sync::Arc::new(raw_config);
        let runtime = SessionRuntime::from_config(make_test_deps(config));
        assert!(runtime.api_session_runner().is_some());
        assert!(runtime.sdk_runtime().is_some());
    }

    #[test]
    fn test_from_config_backward_compat_existing_api_only() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_test_config(dir.path());
        let runtime = SessionRuntime::from_config(make_test_deps(config));
        assert!(
            runtime.api_session_runner().is_some(),
            "backward compat: default config should be API"
        );
        assert!(runtime.sdk_runtime().is_none());
    }

    #[test]
    fn test_resolve_role_config_dev() {
        let config = crate::config::BotConfig::_test_minimal("pretty", "info");
        let role_config = resolve_role_config(&config.llm, &LlmRole::Dev);
        assert_eq!(role_config.provider, "anthropic");
    }

    #[test]
    fn test_resolve_role_config_review() {
        let config = crate::config::BotConfig::_test_minimal("pretty", "info");
        let role_config = resolve_role_config(&config.llm, &LlmRole::Review);
        assert_eq!(role_config.provider, "anthropic");
    }

    #[test]
    fn test_resolve_role_config_epic_review_falls_back_to_review() {
        let config = crate::config::BotConfig::_test_minimal("pretty", "info");
        let role_config = resolve_role_config(&config.llm, &LlmRole::EpicReview);
        assert_eq!(role_config.provider, config.llm.review.provider);
    }

    #[test]
    fn test_resolve_role_config_critic_falls_back_to_review() {
        let config = crate::config::BotConfig::_test_minimal("pretty", "info");
        let role_config = resolve_role_config(&config.llm, &LlmRole::Critic);
        assert_eq!(role_config.provider, config.llm.review.provider);
    }
}
