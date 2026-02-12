//! Notifier module — trait + Telegram implementation for human notifications.
//!
//! Provides a [`Notifier`] trait with two implementations:
//! - [`TelegramNotifier`]: sends HTML-formatted messages via the Telegram Bot API
//! - [`NoopNotifier`]: silent fallback when notifications are disabled
//!
//! Use [`create_notifier`] to obtain the appropriate implementation based on config.

use async_trait::async_trait;
use reqwest_middleware::ClientWithMiddleware;
use std::fmt;

use crate::config::{BotSecrets, NotificationConfig, TelegramConfig, build_http_client};

// ---------------------------------------------------------------------------
// NotifierError (Task 1)
// ---------------------------------------------------------------------------

/// Typed error enum for notification failures.
///
/// Follows the project-wide pattern of `{ reason: String }` fields mapped via
/// `.map_err(|e| ... { reason: e.to_string() })` — no `#[from]` on external errors.
#[derive(Debug, thiserror::Error)]
pub enum NotifierError {
    /// Network or middleware send failure.
    #[error("HTTP request failed: {reason}")]
    HttpRequest {
        /// Human-readable description of the network error.
        reason: String,
    },

    /// Telegram API returned a non-ok response.
    #[error("Telegram API error (HTTP {status}): {body}")]
    ApiError {
        /// HTTP status code returned by the API.
        status: u16,
        /// Response body or error description.
        body: String,
    },

    /// Failed to deserialize the API response.
    #[error("Response parse error: {reason}")]
    ResponseParse {
        /// Human-readable description of the parse failure.
        reason: String,
    },

    /// Notification attempted but Telegram is disabled in config.
    #[error("Telegram notifications are disabled")]
    Disabled,
}

// ---------------------------------------------------------------------------
// Data Types (Task 2)
// ---------------------------------------------------------------------------

/// Status of a story after processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoryStatus {
    /// Story completed successfully.
    Completed,
    /// Story is blocked and cannot proceed.
    Blocked,
    /// Story encountered an error during processing.
    Error,
}

impl fmt::Display for StoryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completed => write!(f, "✅ completed"),
            Self::Blocked => write!(f, "⚠️ blocked"),
            Self::Error => write!(f, "❌ error"),
        }
    }
}

/// Notification payload for a single story.
#[derive(Debug, Clone)]
pub struct StoryNotification {
    /// Story identifier (e.g. "6.1").
    pub story_id: String,
    /// Story key slug (e.g. "6-1-telegram-notifications").
    pub story_key: String,
    /// Outcome of story processing.
    pub status: StoryStatus,
    /// URL to the pull/merge request, if one was created.
    pub pr_url: Option<String>,
    /// Reason for blockage or error, if applicable.
    pub reason: Option<String>,
}

/// Summary of a complete daemon run across all eligible stories.
#[derive(Debug, Clone)]
pub struct RunSummary {
    /// Per-story results.
    pub stories: Vec<StoryNotification>,
    /// Total number of stories processed.
    pub total_processed: usize,
    /// Count of stories that completed successfully.
    pub completed: usize,
    /// Count of stories that were blocked.
    pub blocked: usize,
    /// Count of stories that errored.
    pub errored: usize,
    /// When `true`, a fatal error occurred (e.g. auth failure) and the daemon
    /// should halt immediately — continuing would produce the same failure.
    pub fatal: bool,
}

/// Internal representation of the Telegram `sendMessage` API response.
#[derive(serde::Deserialize)]
struct TelegramResponse {
    /// Whether the API call succeeded.
    ok: bool,
    /// Error description when `ok` is `false`.
    description: Option<String>,
}

// ---------------------------------------------------------------------------
// Notifier Trait (Task 3)
// ---------------------------------------------------------------------------

/// Trait for sending human-facing notifications about story processing results.
///
/// Object-safe for future extensibility (Slack, email, etc.).
#[async_trait]
pub trait Notifier: Send + Sync {
    /// Send a notification for a single story result.
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError>;

    /// Send a summary notification for a complete daemon run.
    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError>;
}

// ---------------------------------------------------------------------------
// Message Formatting & HTML Escaping (Task 5)
// ---------------------------------------------------------------------------

/// Maximum length for Telegram `sendMessage` text field.
const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

/// Escape HTML special characters for Telegram's HTML parse mode.
///
/// Replaces `&`, `<`, and `>` with their HTML entity equivalents.
/// Apply to all dynamic text (story keys, error reasons) but NOT to
/// HTML tags you generate or URLs inside `href` attributes.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Format a single-story notification as an HTML message for Telegram.
fn format_story_message(notification: &StoryNotification) -> String {
    let status_line = match notification.status {
        StoryStatus::Completed => {
            format!("✅ Story {} completed", escape_html(&notification.story_id))
        }
        StoryStatus::Blocked => format!("⚠️ Story {} blocked", escape_html(&notification.story_id)),
        StoryStatus::Error => format!("❌ Story {} error", escape_html(&notification.story_id)),
    };

    let mut msg = format!(
        "{status_line}\n<b>{}</b>",
        escape_html(&notification.story_key)
    );

    if let Some(ref url) = notification.pr_url {
        // Extract a short label from the URL for display text
        let label = url
            .rsplit('/')
            .next()
            .map(|n| format!("PR #{n}"))
            .unwrap_or_else(|| "PR".to_string());
        msg.push_str(&format!(
            "\nPR: <a href=\"{url}\">{}</a>",
            escape_html(&label)
        ));
    }

    if let Some(ref reason) = notification.reason {
        msg.push_str(&format!("\nReason: {}", escape_html(reason)));
    }

    msg
}

/// Format a run summary notification as an HTML message for Telegram.
fn format_run_summary(summary: &RunSummary) -> String {
    let mut msg = String::from("🏁 BMAD Bot Run Complete\n");
    msg.push_str(&format!(
        "📊 {} stories processed: ✅ {} | ⚠️ {} | ❌ {}\n",
        summary.total_processed, summary.completed, summary.blocked, summary.errored
    ));

    for story in &summary.stories {
        let emoji = match story.status {
            StoryStatus::Completed => "✅",
            StoryStatus::Blocked => "⚠️",
            StoryStatus::Error => "❌",
        };

        let key_escaped = escape_html(&story.story_key);

        match (&story.pr_url, &story.reason) {
            (Some(url), _) => {
                let label = url
                    .rsplit('/')
                    .next()
                    .map(|n| format!("PR #{n}"))
                    .unwrap_or_else(|| "PR".to_string());
                msg.push_str(&format!(
                    "\n{emoji} {key_escaped} → <a href=\"{url}\">{}</a>",
                    escape_html(&label)
                ));
            }
            (None, Some(reason)) => {
                msg.push_str(&format!(
                    "\n{emoji} {key_escaped} — {}",
                    escape_html(reason)
                ));
            }
            (None, None) => {
                msg.push_str(&format!("\n{emoji} {key_escaped}"));
            }
        }
    }

    msg
}

/// Truncate a message to fit within Telegram's 4096-character limit.
///
/// If the message exceeds the limit, it is truncated to 4093 characters
/// and `"..."` is appended.
fn truncate_message(text: &str) -> String {
    if text.len() > TELEGRAM_MAX_MESSAGE_LEN {
        tracing::warn!(
            action = "telegram_truncated",
            original_len = text.len(),
            "Message truncated to 4096 char Telegram limit"
        );
        let mut truncated = text[..4093].to_string();
        truncated.push_str("...");
        truncated
    } else {
        text.to_string()
    }
}

// ---------------------------------------------------------------------------
// TelegramNotifier (Task 4)
// ---------------------------------------------------------------------------

/// Sends notifications via the Telegram Bot API using `sendMessage`.
///
/// Uses [`build_http_client`] (retry middleware included) for HTTP transport.
/// Messages are HTML-formatted and truncated to 4096 characters if needed.
#[derive(Debug)]
pub struct TelegramNotifier {
    /// Shared HTTP client with retry middleware.
    http_client: ClientWithMiddleware,
    /// Telegram bot token from `TELEGRAM_BOT_TOKEN` env var.
    bot_token: String,
    /// Target chat ID from config.
    chat_id: String,
}

impl TelegramNotifier {
    /// Create a new `TelegramNotifier` from config and bot token.
    ///
    /// # Errors
    /// Returns [`NotifierError::Disabled`] if `config.enabled` is `false`.
    pub fn new(config: &TelegramConfig, bot_token: String) -> Result<Self, NotifierError> {
        if !config.enabled {
            return Err(NotifierError::Disabled);
        }

        let http_client = build_http_client();

        Ok(Self {
            http_client,
            bot_token,
            chat_id: config.chat_id.clone(),
        })
    }

    /// Send a text message via the Telegram Bot API `sendMessage` endpoint.
    ///
    /// Messages exceeding 4096 characters are truncated before sending.
    async fn send_message(&self, text: &str) -> Result<(), NotifierError> {
        let text = truncate_message(text);

        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);

        let json_body = serde_json::to_vec(&serde_json::json!({
            "chat_id": &self.chat_id,
            "text": text,
            "parse_mode": "HTML",
        }))
        .map_err(|e| NotifierError::ResponseParse {
            reason: e.to_string(),
        })?;

        let response = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(json_body)
            .send()
            .await
            .map_err(|e| NotifierError::HttpRequest {
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(NotifierError::ApiError { status, body });
        }

        let resp_bytes = response
            .bytes()
            .await
            .map_err(|e| NotifierError::ResponseParse {
                reason: e.to_string(),
            })?;

        let parsed: TelegramResponse =
            serde_json::from_slice(&resp_bytes).map_err(|e| NotifierError::ResponseParse {
                reason: e.to_string(),
            })?;

        if !parsed.ok {
            return Err(NotifierError::ApiError {
                status: 200,
                body: parsed.description.unwrap_or_default(),
            });
        }

        tracing::info!(action = "telegram_send", "Notification sent");

        Ok(())
    }
}

#[async_trait]
impl Notifier for TelegramNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        let message = format_story_message(notification);
        self.send_message(&message).await
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        let message = format_run_summary(summary);
        self.send_message(&message).await
    }
}

// ---------------------------------------------------------------------------
// NoopNotifier (Task 6)
// ---------------------------------------------------------------------------

/// Silent notifier used when Telegram is disabled.
///
/// Both trait methods log at `debug` level and return `Ok(())`.
pub struct NoopNotifier;

#[async_trait]
impl Notifier for NoopNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        tracing::debug!(
            action = "noop_notify_story",
            story_id = %notification.story_id,
            "Notification skipped (noop)"
        );
        Ok(())
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        tracing::debug!(
            action = "noop_notify_run_summary",
            total = summary.total_processed,
            "Run summary notification skipped (noop)"
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Factory Function (Task 7)
// ---------------------------------------------------------------------------

/// Create the appropriate [`Notifier`] implementation based on configuration.
///
/// Returns a [`TelegramNotifier`] if Telegram is enabled and the bot token
/// is available; otherwise returns a [`NoopNotifier`] with an info log.
/// This function never fails — worst case it returns `NoopNotifier`.
pub fn create_notifier(config: &NotificationConfig, secrets: &BotSecrets) -> Box<dyn Notifier> {
    if config.telegram.enabled {
        if let Some(ref token) = secrets
            .telegram_bot_token
            .as_ref()
            .filter(|t| !t.is_empty())
        {
            match TelegramNotifier::new(&config.telegram, token.to_string()) {
                Ok(notifier) => return Box::new(notifier),
                Err(e) => {
                    tracing::warn!(
                        action = "notifier_fallback",
                        error = %e,
                        "Failed to create TelegramNotifier — falling back to NoopNotifier"
                    );
                }
            }
        }
        tracing::warn!(
            action = "notifier_fallback",
            "Telegram enabled but bot token missing or empty — using NoopNotifier"
        );
    } else {
        tracing::info!("Telegram notifications disabled");
    }

    Box::new(NoopNotifier)
}

// ===========================================================================
// Unit Tests (Task 8)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // StoryStatus Display tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_story_status_display_completed() {
        let status = StoryStatus::Completed;
        let display = format!("{status}");
        assert!(display.contains("✅"));
        assert!(display.contains("completed"));
    }

    #[test]
    fn test_story_status_display_blocked() {
        let status = StoryStatus::Blocked;
        let display = format!("{status}");
        assert!(display.contains("⚠️"));
        assert!(display.contains("blocked"));
    }

    #[test]
    fn test_story_status_display_error() {
        let status = StoryStatus::Error;
        let display = format!("{status}");
        assert!(display.contains("❌"));
        assert!(display.contains("error"));
    }

    // -----------------------------------------------------------------------
    // HTML escaping tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_escape_html_special_chars() {
        assert_eq!(
            escape_html("<b>bold</b> & 'safe'"),
            "&lt;b&gt;bold&lt;/b&gt; &amp; 'safe'"
        );
    }

    #[test]
    fn test_escape_html_no_change_for_safe_text() {
        let safe = "Hello world 123";
        assert_eq!(escape_html(safe), safe);
    }

    // -----------------------------------------------------------------------
    // Message formatting tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_story_message_completed_with_pr() {
        let notification = StoryNotification {
            story_id: "6.1".to_string(),
            story_key: "6-1-telegram-notifications".to_string(),
            status: StoryStatus::Completed,
            pr_url: Some("https://github.com/org/repo/pull/42".to_string()),
            reason: None,
        };
        let msg = format_story_message(&notification);
        assert!(msg.contains("✅"));
        assert!(msg.contains("6.1"));
        assert!(msg.contains("<b>6-1-telegram-notifications</b>"));
        assert!(msg.contains("<a href=\"https://github.com/org/repo/pull/42\">PR #42</a>"));
    }

    #[test]
    fn test_format_story_message_blocked_with_reason() {
        let notification = StoryNotification {
            story_id: "6.2".to_string(),
            story_key: "6-2-http-retry".to_string(),
            status: StoryStatus::Blocked,
            pr_url: None,
            reason: Some("Dependency not available".to_string()),
        };
        let msg = format_story_message(&notification);
        assert!(msg.contains("⚠️"));
        assert!(msg.contains("Reason: Dependency not available"));
    }

    #[test]
    fn test_format_story_message_error_no_pr() {
        let notification = StoryNotification {
            story_id: "6.3".to_string(),
            story_key: "6-3-crash-recovery".to_string(),
            status: StoryStatus::Error,
            pr_url: None,
            reason: None,
        };
        let msg = format_story_message(&notification);
        assert!(msg.contains("❌"));
        assert!(msg.contains("6.3"));
        assert!(msg.contains("<b>6-3-crash-recovery</b>"));
        assert!(!msg.contains("PR:"));
        assert!(!msg.contains("Reason:"));
    }

    #[test]
    fn test_format_story_message_escapes_html_in_reason() {
        let notification = StoryNotification {
            story_id: "1.1".to_string(),
            story_key: "1-1-test".to_string(),
            status: StoryStatus::Error,
            pr_url: None,
            reason: Some("Connection <timeout> after 30s & retry".to_string()),
        };
        let msg = format_story_message(&notification);
        assert!(msg.contains("Connection &lt;timeout&gt; after 30s &amp; retry"));
        // Verify the raw < > & are NOT present outside of HTML tags
        assert!(!msg.contains("Connection <timeout>"));
    }

    #[test]
    fn test_format_run_summary_mixed_statuses() {
        let summary = RunSummary {
            stories: vec![
                StoryNotification {
                    story_id: "6.1".to_string(),
                    story_key: "6-1-telegram-notifications".to_string(),
                    status: StoryStatus::Completed,
                    pr_url: Some("https://github.com/org/repo/pull/42".to_string()),
                    reason: None,
                },
                StoryNotification {
                    story_id: "6.2".to_string(),
                    story_key: "6-2-http-retry".to_string(),
                    status: StoryStatus::Completed,
                    pr_url: Some("https://github.com/org/repo/pull/43".to_string()),
                    reason: None,
                },
                StoryNotification {
                    story_id: "6.3".to_string(),
                    story_key: "6-3-crash-recovery".to_string(),
                    status: StoryStatus::Error,
                    pr_url: None,
                    reason: Some("Context limit exceeded".to_string()),
                },
            ],
            total_processed: 3,
            completed: 2,
            blocked: 0,
            errored: 1,
            fatal: false,
        };
        let msg = format_run_summary(&summary);
        assert!(msg.contains("🏁 BMAD Bot Run Complete"));
        assert!(msg.contains("📊 3 stories processed: ✅ 2 | ⚠️ 0 | ❌ 1"));
        assert!(msg.contains("6-1-telegram-notifications"));
        assert!(msg.contains("6-2-http-retry"));
        assert!(msg.contains("6-3-crash-recovery — Context limit exceeded"));
    }

    #[test]
    fn test_format_run_summary_all_completed() {
        let summary = RunSummary {
            stories: vec![StoryNotification {
                story_id: "1.1".to_string(),
                story_key: "1-1-scaffolding".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("https://github.com/org/repo/pull/1".to_string()),
                reason: None,
            }],
            total_processed: 1,
            completed: 1,
            blocked: 0,
            errored: 0,
            fatal: false,
        };
        let msg = format_run_summary(&summary);
        assert!(msg.contains("✅ 1 | ⚠️ 0 | ❌ 0"));
        assert!(msg.contains("1-1-scaffolding"));
    }

    #[test]
    fn test_format_run_summary_truncation_long_message() {
        // Create a summary with enough stories to exceed 4096 chars
        let stories: Vec<StoryNotification> = (0..200)
            .map(|i| StoryNotification {
                story_id: format!("{i}.1"),
                story_key: format!("{i}-1-very-long-story-key-name-that-adds-up-quickly"),
                status: StoryStatus::Completed,
                pr_url: Some(format!("https://github.com/org/repo/pull/{i}")),
                reason: None,
            })
            .collect();
        let summary = RunSummary {
            stories: stories.clone(),
            total_processed: 200,
            completed: 200,
            blocked: 0,
            errored: 0,
            fatal: false,
        };
        let msg = format_run_summary(&summary);
        let truncated = truncate_message(&msg);
        assert!(truncated.len() <= TELEGRAM_MAX_MESSAGE_LEN);
        if msg.len() > TELEGRAM_MAX_MESSAGE_LEN {
            assert!(truncated.ends_with("..."));
        }
    }

    // -----------------------------------------------------------------------
    // NoopNotifier tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_noop_notifier_returns_ok() {
        let notifier = NoopNotifier;
        let summary = RunSummary {
            stories: vec![],
            total_processed: 0,
            completed: 0,
            blocked: 0,
            errored: 0,
            fatal: false,
        };
        let result = notifier.notify_run_summary(&summary).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_noop_notifier_story_returns_ok() {
        let notifier = NoopNotifier;
        let notification = StoryNotification {
            story_id: "1.1".to_string(),
            story_key: "1-1-test".to_string(),
            status: StoryStatus::Completed,
            pr_url: None,
            reason: None,
        };
        let result = notifier.notify_story(&notification).await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // TelegramNotifier constructor tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_telegram_notifier_new_disabled() {
        let config = TelegramConfig {
            enabled: false,
            chat_id: "12345".to_string(),
        };
        let result = TelegramNotifier::new(&config, "token".to_string());
        assert!(result.is_err());
        match result.unwrap_err() {
            NotifierError::Disabled => {} // expected
            other => panic!("Expected NotifierError::Disabled, got: {other}"),
        }
    }

    #[test]
    fn test_telegram_notifier_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TelegramNotifier>();
    }

    // -----------------------------------------------------------------------
    // Factory function tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_notifier_disabled_returns_noop() {
        let config = NotificationConfig {
            telegram: TelegramConfig {
                enabled: false,
                chat_id: "12345".to_string(),
            },
        };
        let secrets = BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_copilot_oauth_token: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        };
        let notifier = create_notifier(&config, &secrets);
        // NoopNotifier is returned — verify it doesn't panic on use
        // We can't downcast easily, but we can verify the trait object works
        let rt = tokio::runtime::Runtime::new().unwrap();
        let notification = StoryNotification {
            story_id: "1.1".to_string(),
            story_key: "1-1-test".to_string(),
            status: StoryStatus::Completed,
            pr_url: None,
            reason: None,
        };
        let result = rt.block_on(notifier.notify_story(&notification));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_notifier_enabled_returns_telegram() {
        let config = NotificationConfig {
            telegram: TelegramConfig {
                enabled: true,
                chat_id: "12345".to_string(),
            },
        };
        let secrets = BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_copilot_oauth_token: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: Some("bot123:ABCDEF".to_string()),
        };
        // Factory should return a TelegramNotifier (not NoopNotifier).
        // We verify indirectly: a NoopNotifier would return Ok for notify_story
        // without network, while TelegramNotifier would fail trying to reach the API.
        // Here we just verify the factory doesn't panic.
        let _notifier = create_notifier(&config, &secrets);
    }
}
