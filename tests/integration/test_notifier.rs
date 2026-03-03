//! Integration tests for the notification subsystem.
//!
//! These tests verify the notifier module's public API from the external crate
//! boundary (`bmad_bot::notifier::*`, `bmad_bot::config::*`), complementing
//! the 18+ unit tests in `src/notifier/mod.rs`.

use std::time::Duration;

use bmad_bot::config::{BotSecrets, NotificationConfig, TelegramConfig};
use bmad_bot::notifier::{
    create_notifier, Notifier, NotifierError, RunSummary, StoryNotification,
    StoryStatus, TelegramNotifier,
};

use super::helpers::mocks::MockNotifier;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Construct a `BotSecrets` with only the Telegram token set; all other
/// secrets are `None`.
fn make_test_secrets_with_telegram(token: Option<String>) -> BotSecrets {
    BotSecrets {
        anthropic_api_key: None,
        openai_api_key: None,
        github_copilot_oauth_token: None,
        github_token: None,
        gitlab_token: None,
        telegram_bot_token: token,
    }
}

/// Build a `NotificationConfig` with the given enabled flag and chat_id.
fn make_notification_config(enabled: bool, chat_id: &str) -> NotificationConfig {
    NotificationConfig {
        telegram: TelegramConfig {
            enabled,
            chat_id: chat_id.to_string(),
        },
    }
}

/// Build a completed `StoryNotification` with a PR URL.
fn make_completed_notification() -> StoryNotification {
    StoryNotification {
        story_id: "6.1".to_string(),
        story_key: "6-1-telegram-notifications".to_string(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/repo/pull/42".to_string()),
        reason: None,
    }
}

/// Build a `RunSummary` with mixed statuses: 2 completed, 1 blocked, 1 errored.
fn make_mixed_run_summary() -> RunSummary {
    RunSummary {
        stories: vec![
            StoryNotification {
                story_id: "1.1".to_string(),
                story_key: "1-1-first".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("https://github.com/test/repo/pull/1".to_string()),
                reason: None,
            },
            StoryNotification {
                story_id: "1.2".to_string(),
                story_key: "1-2-second".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("https://github.com/test/repo/pull/2".to_string()),
                reason: None,
            },
            StoryNotification {
                story_id: "1.3".to_string(),
                story_key: "1-3-third".to_string(),
                status: StoryStatus::Blocked,
                pr_url: None,
                reason: Some("dependency not met".to_string()),
            },
            StoryNotification {
                story_id: "1.4".to_string(),
                story_key: "1-4-fourth".to_string(),
                status: StoryStatus::Error,
                pr_url: None,
                reason: Some("LLM timeout".to_string()),
            },
        ],
        total_processed: 4,
        completed: 2,
        blocked: 1,
        errored: 1,
        fatal: false,
    }
}

// ===========================================================================
// Task 2 — TelegramNotifier construction and type dispatch (AC #1)
// ===========================================================================

#[test]
fn test_notifier_telegram_new_success() {
    let config = TelegramConfig {
        enabled: true,
        chat_id: "12345".to_string(),
    };
    let result = TelegramNotifier::new(&config, "bot123:ABCDEF-test-DO-NOT-USE".to_string());
    assert!(result.is_ok(), "TelegramNotifier::new with enabled config and valid token should succeed");
}

#[test]
fn test_notifier_telegram_new_disabled_returns_err() {
    let config = TelegramConfig {
        enabled: false,
        chat_id: "12345".to_string(),
    };
    let result = TelegramNotifier::new(&config, "bot123:ABCDEF-test-DO-NOT-USE".to_string());
    assert!(result.is_err(), "TelegramNotifier::new with disabled config should return Err");
    let err = result.unwrap_err();
    assert!(
        matches!(err, NotifierError::Disabled),
        "Expected NotifierError::Disabled, got: {err:?}"
    );
}

#[test]
fn test_notifier_story_notification_struct_construction() {
    let notification = make_completed_notification();
    assert_eq!(notification.story_id, "6.1");
    assert_eq!(notification.story_key, "6-1-telegram-notifications");
    assert_eq!(notification.status, StoryStatus::Completed);
    assert_eq!(
        notification.pr_url.as_deref(),
        Some("https://github.com/test/repo/pull/42")
    );
    assert!(notification.reason.is_none());
}

// ===========================================================================
// Task 3 — create_notifier() factory — disabled path (AC #2)
// ===========================================================================

#[tokio::test]
async fn test_notifier_factory_disabled_returns_noop() {
    let config = make_notification_config(false, "");
    let secrets = make_test_secrets_with_telegram(None);
    let notifier = create_notifier(&config, &secrets);

    // NoopNotifier returns Ok(()) for notify_story
    let notification = make_completed_notification();
    let result = notifier.notify_story(&notification).await;
    assert!(result.is_ok(), "Disabled factory notifier should return Ok for notify_story");
}

#[tokio::test]
async fn test_notifier_factory_disabled_notify_run_summary_succeeds() {
    let config = make_notification_config(false, "");
    let secrets = make_test_secrets_with_telegram(None);
    let notifier = create_notifier(&config, &secrets);

    let summary = make_mixed_run_summary();
    let result = notifier.notify_run_summary(&summary).await;
    assert!(result.is_ok(), "Disabled factory notifier should return Ok for notify_run_summary");
}

// ===========================================================================
// Task 4 — create_notifier() factory — graceful fallback path (AC #3)
// ===========================================================================

#[tokio::test]
async fn test_notifier_factory_enabled_no_token_returns_noop() {
    let config = make_notification_config(true, "12345");
    let secrets = make_test_secrets_with_telegram(None);
    let notifier = create_notifier(&config, &secrets);

    // Should behave as NoopNotifier — returns Ok(()) for both methods
    let notification = make_completed_notification();
    let result = notifier.notify_story(&notification).await;
    assert!(result.is_ok(), "Enabled but no token should fallback to NoopNotifier for notify_story");

    let summary = make_mixed_run_summary();
    let result = notifier.notify_run_summary(&summary).await;
    assert!(result.is_ok(), "Enabled but no token should fallback to NoopNotifier for notify_run_summary");
}

#[tokio::test]
async fn test_notifier_factory_enabled_empty_token_returns_noop() {
    let config = make_notification_config(true, "12345");
    let secrets = make_test_secrets_with_telegram(Some(String::new()));
    let notifier = create_notifier(&config, &secrets);

    // Empty token should also fallback to NoopNotifier
    let notification = make_completed_notification();
    let result = notifier.notify_story(&notification).await;
    assert!(result.is_ok(), "Enabled but empty token should fallback to NoopNotifier for notify_story");

    let summary = make_mixed_run_summary();
    let result = notifier.notify_run_summary(&summary).await;
    assert!(result.is_ok(), "Enabled but empty token should fallback to NoopNotifier for notify_run_summary");
}

// ===========================================================================
// Task 5 — create_notifier() factory — enabled + valid token path (AC #1)
// ===========================================================================

#[tokio::test]
async fn test_notifier_factory_enabled_with_token_returns_telegram() {
    let config = make_notification_config(true, "12345");
    let secrets = make_test_secrets_with_telegram(Some("bot123:ABCDEF-test-DO-NOT-USE".to_string()));
    let notifier = create_notifier(&config, &secrets);

    // A TelegramNotifier will attempt real HTTP and fail — confirming it is NOT a NoopNotifier.
    // NoopNotifier would return Ok(()).
    // Cap the network call at 5 s so a hung connection never blocks CI indefinitely.
    // (reqwest::Client has no default timeout; the retry policy has up to 3 retries.)
    let notification = make_completed_notification();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        notifier.notify_story(&notification),
    )
    .await
    .expect("notify_story timed out after 5 s — possible hung network connection in test environment");

    // The TelegramNotifier must fail (HTTP error / Telegram 401) — NOT Ok(()) like a NoopNotifier would.
    assert!(
        result.is_err(),
        "Factory with valid token should create TelegramNotifier that attempts real HTTP send (not NoopNotifier)"
    );
}

// ===========================================================================
// Task 6 — RunSummary construction and MockNotifier capture (AC #4)
// ===========================================================================

#[test]
fn test_notifier_run_summary_construction_counts() {
    let summary = make_mixed_run_summary();
    assert_eq!(summary.total_processed, 4);
    assert_eq!(summary.completed, 2);
    assert_eq!(summary.blocked, 1);
    assert_eq!(summary.errored, 1);
    assert_eq!(summary.stories.len(), 4);
    assert!(!summary.fatal);
}

#[tokio::test]
async fn test_notifier_run_summary_mixed_statuses_on_mock() {
    let mock = MockNotifier::new();
    let summary = make_mixed_run_summary();

    let result = mock.notify_run_summary(&summary).await;
    assert!(result.is_ok());

    let captured = mock.summary_calls();
    assert_eq!(captured.len(), 1, "MockNotifier should capture exactly 1 summary call");

    let s = &captured[0];
    assert_eq!(s.total_processed, 4);
    assert_eq!(s.completed, 2);
    assert_eq!(s.blocked, 1);
    assert_eq!(s.errored, 1);
}

#[tokio::test]
async fn test_notifier_story_notifications_captured_by_mock() {
    let mock = MockNotifier::new();

    let notifications = vec![
        StoryNotification {
            story_id: "1.1".to_string(),
            story_key: "1-1-auth".to_string(),
            status: StoryStatus::Completed,
            pr_url: Some("https://github.com/test/repo/pull/10".to_string()),
            reason: None,
        },
        StoryNotification {
            story_id: "1.2".to_string(),
            story_key: "1-2-config".to_string(),
            status: StoryStatus::Blocked,
            pr_url: None,
            reason: Some("missing dependency".to_string()),
        },
        StoryNotification {
            story_id: "1.3".to_string(),
            story_key: "1-3-watcher".to_string(),
            status: StoryStatus::Error,
            pr_url: None,
            reason: Some("timeout".to_string()),
        },
    ];

    for n in &notifications {
        let result = mock.notify_story(n).await;
        assert!(result.is_ok());
    }

    let captured = mock.story_calls();
    assert_eq!(captured.len(), 3, "MockNotifier should capture all 3 story notifications");

    // Verify first notification
    assert_eq!(captured[0].story_id, "1.1");
    assert_eq!(captured[0].story_key, "1-1-auth");
    assert_eq!(captured[0].status, StoryStatus::Completed);
    assert_eq!(captured[0].pr_url.as_deref(), Some("https://github.com/test/repo/pull/10"));

    // Verify second notification
    assert_eq!(captured[1].story_id, "1.2");
    assert_eq!(captured[1].story_key, "1-2-config");
    assert_eq!(captured[1].status, StoryStatus::Blocked);
    assert!(captured[1].pr_url.is_none());

    // Verify third notification
    assert_eq!(captured[2].story_id, "1.3");
    assert_eq!(captured[2].story_key, "1-3-watcher");
    assert_eq!(captured[2].status, StoryStatus::Error);
    assert!(captured[2].pr_url.is_none());
}

// ===========================================================================
// Task 7 — StoryStatus display and data integrity (AC #1, #4)
// ===========================================================================

#[test]
fn test_notifier_story_status_display_completed() {
    let display = StoryStatus::Completed.to_string();
    assert!(
        display.contains("completed"),
        "StoryStatus::Completed display should contain 'completed', got: {display}"
    );
}

#[test]
fn test_notifier_story_status_display_blocked() {
    let display = StoryStatus::Blocked.to_string();
    assert!(
        display.contains("blocked"),
        "StoryStatus::Blocked display should contain 'blocked', got: {display}"
    );
}

#[test]
fn test_notifier_story_status_display_error() {
    let display = StoryStatus::Error.to_string();
    assert!(
        display.contains("error"),
        "StoryStatus::Error display should contain 'error', got: {display}"
    );
}

// ===========================================================================
// Task 8 — NotifierError variants (AC ALL)
// ===========================================================================

#[test]
fn test_notifier_error_disabled_display() {
    let err = NotifierError::Disabled;
    let display = format!("{err}");
    assert!(
        display.contains("disabled") || display.contains("Disabled"),
        "NotifierError::Disabled display should mention disabled, got: {display}"
    );
}

#[test]
fn test_notifier_error_types_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NotifierError>();
}
