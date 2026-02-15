//! Integration tests for the `notifier` module.
//!
//! Tests cross-module boundary (config → notifier), factory logic,
//! data contracts, and MockNotifier capture from the external crate perspective.

use bmad_bot::config::{BotSecrets, NotificationConfig, TelegramConfig};
use bmad_bot::notifier::{
    create_notifier, Notifier, NotifierError, RunSummary, StoryNotification,
    StoryStatus, TelegramNotifier,
};

// Re-use MockNotifier from shared integration infrastructure (Story 7.1).
use super::helpers::mocks::MockNotifier;

// ---------------------------------------------------------------------------
// Helper: construct BotSecrets with only the telegram_bot_token varied.
// ---------------------------------------------------------------------------

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

fn make_telegram_config(enabled: bool) -> TelegramConfig {
    TelegramConfig {
        enabled,
        chat_id: "123456".to_string(),
    }
}

fn make_notification_config(enabled: bool) -> NotificationConfig {
    NotificationConfig {
        telegram: make_telegram_config(enabled),
    }
}

fn make_story_notification(
    story_id: &str,
    story_key: &str,
    status: StoryStatus,
    pr_url: Option<&str>,
) -> StoryNotification {
    StoryNotification {
        story_id: story_id.to_string(),
        story_key: story_key.to_string(),
        status,
        pr_url: pr_url.map(|s| s.to_string()),
        reason: None,
    }
}

// ===========================================================================
// Task 2: TelegramNotifier construction and type dispatch (AC #1)
// ===========================================================================

#[test]
fn test_notifier_telegram_new_success() {
    let config = make_telegram_config(true);
    let result = TelegramNotifier::new(&config, "bot123:ABCDEF-test-DO-NOT-USE".to_string());
    assert!(result.is_ok(), "TelegramNotifier::new should succeed with enabled config + valid token");
}

#[test]
fn test_notifier_telegram_new_disabled_returns_err() {
    let config = make_telegram_config(false);
    let result = TelegramNotifier::new(&config, "bot123:ABCDEF-test-DO-NOT-USE".to_string());
    assert!(result.is_err(), "TelegramNotifier::new should fail with disabled config");
    let err = result.unwrap_err();
    assert!(
        matches!(err, NotifierError::Disabled),
        "Expected NotifierError::Disabled, got: {err:?}"
    );
}

#[test]
fn test_notifier_story_notification_struct_construction() {
    let notification = StoryNotification {
        story_id: "6.1".to_string(),
        story_key: "6-1-telegram-notifications".to_string(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/org/repo/pull/42".to_string()),
        reason: None,
    };

    assert_eq!(notification.story_id, "6.1");
    assert_eq!(notification.story_key, "6-1-telegram-notifications");
    assert_eq!(notification.status, StoryStatus::Completed);
    assert_eq!(
        notification.pr_url.as_deref(),
        Some("https://github.com/org/repo/pull/42")
    );
    assert!(notification.reason.is_none());
}

// ===========================================================================
// Task 3: create_notifier() factory — disabled path (AC #2)
// ===========================================================================

#[tokio::test]
async fn test_notifier_factory_disabled_returns_noop() {
    let config = make_notification_config(false);
    let secrets = make_test_secrets_with_telegram(None);
    let notifier = create_notifier(&config, &secrets);

    // NoopNotifier returns Ok(()) for notify_story
    let notification = make_story_notification(
        "7.7",
        "7-7-test",
        StoryStatus::Completed,
        Some("https://example.com/pr/1"),
    );
    let result = notifier.notify_story(&notification).await;
    assert!(result.is_ok(), "NoopNotifier should return Ok for notify_story");
}

#[tokio::test]
async fn test_notifier_factory_disabled_notify_run_summary_succeeds() {
    let config = make_notification_config(false);
    let secrets = make_test_secrets_with_telegram(None);
    let notifier = create_notifier(&config, &secrets);

    let summary = RunSummary {
        stories: vec![],
        total_processed: 1,
        completed: 1,
        blocked: 0,
        errored: 0,
        fatal: false,
    };
    let result = notifier.notify_run_summary(&summary).await;
    assert!(result.is_ok(), "NoopNotifier should return Ok for notify_run_summary");
}

// ===========================================================================
// Task 4: create_notifier() factory — graceful fallback path (AC #3)
// ===========================================================================

#[tokio::test]
async fn test_notifier_factory_enabled_no_token_returns_noop() {
    let config = make_notification_config(true);
    let secrets = make_test_secrets_with_telegram(None);
    let notifier = create_notifier(&config, &secrets);

    // Should behave as NoopNotifier — returns Ok(()) for both methods.
    let notification = make_story_notification("7.7", "7-7-test", StoryStatus::Completed, None);
    assert!(notifier.notify_story(&notification).await.is_ok());

    let summary = RunSummary {
        stories: vec![],
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
        fatal: false,
    };
    assert!(notifier.notify_run_summary(&summary).await.is_ok());
}

#[tokio::test]
async fn test_notifier_factory_enabled_empty_token_returns_noop() {
    let config = make_notification_config(true);
    let secrets = make_test_secrets_with_telegram(Some("".to_string()));
    let notifier = create_notifier(&config, &secrets);

    // Empty token → NoopNotifier fallback. Both methods return Ok(()).
    let notification = make_story_notification("7.7", "7-7-test", StoryStatus::Blocked, None);
    assert!(notifier.notify_story(&notification).await.is_ok());

    let summary = RunSummary {
        stories: vec![],
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
        fatal: false,
    };
    assert!(notifier.notify_run_summary(&summary).await.is_ok());
}

// ===========================================================================
// Task 5: create_notifier() factory — enabled + valid token path (AC #1)
// ===========================================================================

#[tokio::test]
async fn test_notifier_factory_enabled_with_token_returns_telegram() {
    let config = make_notification_config(true);
    let secrets =
        make_test_secrets_with_telegram(Some("bot123:ABCDEF-test-DO-NOT-USE".to_string()));
    let notifier = create_notifier(&config, &secrets);

    // TelegramNotifier will attempt a real HTTP POST to Telegram API.
    // Since there is no real server, it should fail with an HTTP error —
    // confirming it is NOT a NoopNotifier (which would return Ok(())).
    let notification = make_story_notification(
        "7.7",
        "7-7-test",
        StoryStatus::Completed,
        Some("https://example.com/pr/1"),
    );
    let result = notifier.notify_story(&notification).await;
    assert!(
        result.is_err(),
        "TelegramNotifier should fail with HTTP error (no real Telegram server), \
         proving it is NOT a NoopNotifier. Got Ok(()) instead."
    );
}

// ===========================================================================
// Task 6: RunSummary construction and MockNotifier capture (AC #4)
// ===========================================================================

#[test]
fn test_notifier_run_summary_construction_counts() {
    let stories = vec![
        make_story_notification("1.1", "1-1-story", StoryStatus::Completed, Some("url1")),
        make_story_notification("1.2", "1-2-story", StoryStatus::Completed, Some("url2")),
        make_story_notification("2.1", "2-1-story", StoryStatus::Blocked, None),
        make_story_notification("3.1", "3-1-story", StoryStatus::Error, None),
    ];

    let summary = RunSummary {
        stories,
        total_processed: 4,
        completed: 2,
        blocked: 1,
        errored: 1,
        fatal: false,
    };

    assert_eq!(summary.total_processed, 4);
    assert_eq!(summary.completed, 2);
    assert_eq!(summary.blocked, 1);
    assert_eq!(summary.errored, 1);
    assert!(!summary.fatal);
    assert_eq!(summary.stories.len(), 4);
}

#[tokio::test]
async fn test_notifier_run_summary_mixed_statuses_on_mock() {
    let mock = MockNotifier::new();

    let summary = RunSummary {
        stories: vec![
            make_story_notification("1.1", "1-1-s", StoryStatus::Completed, Some("url1")),
            make_story_notification("2.1", "2-1-s", StoryStatus::Blocked, None),
            make_story_notification("3.1", "3-1-s", StoryStatus::Error, None),
            make_story_notification("4.1", "4-1-s", StoryStatus::Completed, Some("url4")),
        ],
        total_processed: 4,
        completed: 2,
        blocked: 1,
        errored: 1,
        fatal: false,
    };

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

    let n1 = make_story_notification(
        "6.1",
        "6-1-telegram",
        StoryStatus::Completed,
        Some("https://github.com/pr/1"),
    );
    let n2 = make_story_notification("6.2", "6-2-retry", StoryStatus::Blocked, None);
    let n3 = make_story_notification("6.3", "6-3-crash", StoryStatus::Error, None);

    assert!(mock.notify_story(&n1).await.is_ok());
    assert!(mock.notify_story(&n2).await.is_ok());
    assert!(mock.notify_story(&n3).await.is_ok());

    let captured = mock.story_calls();
    assert_eq!(captured.len(), 3, "MockNotifier should capture 3 story calls");

    // Verify first call
    assert_eq!(captured[0].story_id, "6.1");
    assert_eq!(captured[0].story_key, "6-1-telegram");
    assert_eq!(captured[0].status, StoryStatus::Completed);
    assert_eq!(
        captured[0].pr_url.as_deref(),
        Some("https://github.com/pr/1")
    );

    // Verify second call
    assert_eq!(captured[1].story_id, "6.2");
    assert_eq!(captured[1].story_key, "6-2-retry");
    assert_eq!(captured[1].status, StoryStatus::Blocked);
    assert!(captured[1].pr_url.is_none());

    // Verify third call
    assert_eq!(captured[2].story_id, "6.3");
    assert_eq!(captured[2].story_key, "6-3-crash");
    assert_eq!(captured[2].status, StoryStatus::Error);
    assert!(captured[2].pr_url.is_none());
}

// ===========================================================================
// Task 7: StoryStatus display and data integrity (AC #1, #4)
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
// Task 8: NotifierError variants (AC ALL)
// ===========================================================================

#[test]
fn test_notifier_error_disabled_display() {
    let err = NotifierError::Disabled;
    let msg = err.to_string();
    assert!(
        msg.contains("disabled"),
        "NotifierError::Disabled display should mention 'disabled', got: {msg}"
    );
}

#[test]
fn test_notifier_error_types_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NotifierError>();
}
