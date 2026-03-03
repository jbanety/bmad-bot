//! Integration tests for the notification subsystem.
//!
//! Validates cross-module boundaries (config → notifier), factory dispatch,
//! `MockNotifier` capture, `RunSummary` construction, and `StoryStatus` display
//! from the external crate perspective.

use bmad_bot::config::{BotSecrets, NotificationConfig, TelegramConfig};
use bmad_bot::notifier::{
    create_notifier, Notifier, NotifierError, RunSummary, StoryNotification,
    StoryStatus, TelegramNotifier,
};

use super::helpers::mocks::MockNotifier;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Construct a `BotSecrets` with only the Telegram token set; all other keys `None`.
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

// ===========================================================================
// Task 2 — TelegramNotifier construction & type dispatch (AC #1)
// ===========================================================================

#[test]
fn test_notifier_telegram_new_success() {
    let config = TelegramConfig {
        enabled: true,
        chat_id: "12345".to_string(),
    };
    let result = TelegramNotifier::new(&config, "bot123:ABCDEF-test-DO-NOT-USE".to_string());
    assert!(result.is_ok(), "TelegramNotifier::new should succeed with enabled config and valid token");
}

#[test]
fn test_notifier_telegram_new_disabled_returns_err() {
    let config = TelegramConfig {
        enabled: false,
        chat_id: "12345".to_string(),
    };
    let result = TelegramNotifier::new(&config, "bot123:ABCDEF-test-DO-NOT-USE".to_string());
    match result {
        Err(NotifierError::Disabled) => {} // expected
        other => panic!("Expected NotifierError::Disabled, got: {other:?}"),
    }
}

#[test]
fn test_notifier_story_notification_struct_construction() {
    let notification = StoryNotification {
        story_id: "7.7".to_string(),
        story_key: "7-7-notification-flow".to_string(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/example/repo/pull/42".to_string()),
        reason: None,
    };

    assert_eq!(notification.story_id, "7.7");
    assert_eq!(notification.story_key, "7-7-notification-flow");
    assert_eq!(notification.status, StoryStatus::Completed);
    assert_eq!(
        notification.pr_url.as_deref(),
        Some("https://github.com/example/repo/pull/42")
    );
    assert!(notification.reason.is_none());
}

// ===========================================================================
// Task 3 — create_notifier() factory — disabled path (AC #2)
// ===========================================================================

#[tokio::test]
async fn test_notifier_factory_disabled_returns_noop() {
    let config = NotificationConfig {
        telegram: TelegramConfig {
            enabled: false,
            chat_id: String::new(),
        },
    };
    let secrets = make_test_secrets_with_telegram(None);
    let notifier = create_notifier(&config, &secrets);

    // NoopNotifier returns Ok(()) for notify_story
    let notification = StoryNotification {
        story_id: "1.0".to_string(),
        story_key: "1-0-test".to_string(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };
    let result = notifier.notify_story(&notification).await;
    assert!(result.is_ok(), "Disabled notifier should return Ok(())");
}

#[tokio::test]
async fn test_notifier_factory_disabled_notify_run_summary_succeeds() {
    let config = NotificationConfig {
        telegram: TelegramConfig {
            enabled: false,
            chat_id: String::new(),
        },
    };
    let secrets = make_test_secrets_with_telegram(None);
    let notifier = create_notifier(&config, &secrets);

    let summary = RunSummary {
        stories: vec![],
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
        fatal: false,
    };
    let result = notifier.notify_run_summary(&summary).await;
    assert!(result.is_ok(), "Disabled notifier should return Ok(()) for run summary");
}

// ===========================================================================
// Task 4 — create_notifier() factory — graceful fallback (AC #3)
// ===========================================================================

#[tokio::test]
async fn test_notifier_factory_enabled_no_token_returns_noop() {
    let config = NotificationConfig {
        telegram: TelegramConfig {
            enabled: true,
            chat_id: "12345".to_string(),
        },
    };
    let secrets = make_test_secrets_with_telegram(None);
    let notifier = create_notifier(&config, &secrets);

    let notification = StoryNotification {
        story_id: "1.0".to_string(),
        story_key: "1-0-test".to_string(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };
    // NoopNotifier fallback returns Ok
    let result = notifier.notify_story(&notification).await;
    assert!(result.is_ok(), "Enabled with no token should fallback to NoopNotifier");

    let summary = RunSummary {
        stories: vec![],
        total_processed: 0,
        completed: 0,
        blocked: 0,
        errored: 0,
        fatal: false,
    };
    let result = notifier.notify_run_summary(&summary).await;
    assert!(result.is_ok(), "NoopNotifier fallback should also succeed for run summary");
}

#[tokio::test]
async fn test_notifier_factory_enabled_empty_token_returns_noop() {
    let config = NotificationConfig {
        telegram: TelegramConfig {
            enabled: true,
            chat_id: "12345".to_string(),
        },
    };
    let secrets = make_test_secrets_with_telegram(Some(String::new()));
    let notifier = create_notifier(&config, &secrets);

    let notification = StoryNotification {
        story_id: "1.0".to_string(),
        story_key: "1-0-test".to_string(),
        status: StoryStatus::Completed,
        pr_url: None,
        reason: None,
    };
    let result = notifier.notify_story(&notification).await;
    assert!(result.is_ok(), "Empty token should fallback to NoopNotifier");
}

// ===========================================================================
// Task 5 — create_notifier() factory — enabled + valid token (AC #1)
// ===========================================================================

#[tokio::test]
async fn test_notifier_factory_enabled_with_token_returns_telegram() {
    let config = NotificationConfig {
        telegram: TelegramConfig {
            enabled: true,
            chat_id: "12345".to_string(),
        },
    };
    let secrets =
        make_test_secrets_with_telegram(Some("bot123:ABCDEF-test-DO-NOT-USE".to_string()));
    let notifier = create_notifier(&config, &secrets);

    // A real TelegramNotifier will attempt HTTP and fail.
    // NoopNotifier would return Ok(()). Expecting Err proves TelegramNotifier.
    let notification = StoryNotification {
        story_id: "5.1".to_string(),
        story_key: "5-1-test".to_string(),
        status: StoryStatus::Completed,
        pr_url: Some("https://github.com/test/pr/1".to_string()),
        reason: None,
    };
    let result = notifier.notify_story(&notification).await;
    // TelegramNotifier attempts a real HTTP send:
    // - In environments with network access: Telegram API returns 404 ApiError (invalid dummy token)
    // - In air-gapped CI: reqwest returns HttpRequest error (connection refused)
    // Either way, Err confirms this is a TelegramNotifier — NoopNotifier would return Ok(()).
    assert!(
        matches!(
            result,
            Err(NotifierError::HttpRequest { .. }) | Err(NotifierError::ApiError { .. })
        ),
        "TelegramNotifier should fail with HttpRequest or ApiError (confirming it attempted a real send, not a NoopNotifier Ok); got: {result:?}"
    );
}

// ===========================================================================
// Task 6 — RunSummary construction & MockNotifier capture (AC #4)
// ===========================================================================

#[test]
fn test_notifier_run_summary_construction_counts() {
    let stories = vec![
        StoryNotification {
            story_id: "1.0".to_string(),
            story_key: "1-0-a".to_string(),
            status: StoryStatus::Completed,
            pr_url: Some("https://pr/1".to_string()),
            reason: None,
        },
        StoryNotification {
            story_id: "2.0".to_string(),
            story_key: "2-0-b".to_string(),
            status: StoryStatus::Completed,
            pr_url: Some("https://pr/2".to_string()),
            reason: None,
        },
        StoryNotification {
            story_id: "3.0".to_string(),
            story_key: "3-0-c".to_string(),
            status: StoryStatus::Blocked,
            pr_url: None,
            reason: Some("dependency missing".to_string()),
        },
        StoryNotification {
            story_id: "4.0".to_string(),
            story_key: "4-0-d".to_string(),
            status: StoryStatus::Error,
            pr_url: None,
            reason: Some("compilation failed".to_string()),
        },
    ];

    let summary = RunSummary {
        stories: stories.clone(),
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
    assert_eq!(summary.stories.len(), 4);
    assert!(!summary.fatal);
}

#[tokio::test]
async fn test_notifier_run_summary_mixed_statuses_on_mock() {
    let mock = MockNotifier::new();

    let summary = RunSummary {
        stories: vec![
            StoryNotification {
                story_id: "1.0".to_string(),
                story_key: "1-0-a".to_string(),
                status: StoryStatus::Completed,
                pr_url: None,
                reason: None,
            },
            StoryNotification {
                story_id: "2.0".to_string(),
                story_key: "2-0-b".to_string(),
                status: StoryStatus::Blocked,
                pr_url: None,
                reason: Some("blocked".to_string()),
            },
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
    assert_eq!(captured[0].total_processed, 4);
    assert_eq!(captured[0].completed, 2);
    assert_eq!(captured[0].blocked, 1);
    assert_eq!(captured[0].errored, 1);
}

#[tokio::test]
async fn test_notifier_story_notifications_captured_by_mock() {
    let mock = MockNotifier::new();

    let notifications = vec![
        StoryNotification {
            story_id: "1.0".to_string(),
            story_key: "1-0-first".to_string(),
            status: StoryStatus::Completed,
            pr_url: Some("https://pr/1".to_string()),
            reason: None,
        },
        StoryNotification {
            story_id: "2.0".to_string(),
            story_key: "2-0-second".to_string(),
            status: StoryStatus::Blocked,
            pr_url: None,
            reason: Some("dependency missing".to_string()),
        },
        StoryNotification {
            story_id: "3.0".to_string(),
            story_key: "3-0-third".to_string(),
            status: StoryStatus::Error,
            pr_url: None,
            reason: Some("build failed".to_string()),
        },
    ];

    for n in &notifications {
        let result = mock.notify_story(n).await;
        assert!(result.is_ok());
    }

    let captured = mock.story_calls();
    assert_eq!(captured.len(), 3, "MockNotifier should capture all 3 story calls");

    assert_eq!(captured[0].story_id, "1.0");
    assert_eq!(captured[0].story_key, "1-0-first");
    assert_eq!(captured[0].status, StoryStatus::Completed);
    assert_eq!(captured[0].pr_url.as_deref(), Some("https://pr/1"));

    assert_eq!(captured[1].story_id, "2.0");
    assert_eq!(captured[1].story_key, "2-0-second");
    assert_eq!(captured[1].status, StoryStatus::Blocked);
    assert!(captured[1].pr_url.is_none());

    assert_eq!(captured[2].story_id, "3.0");
    assert_eq!(captured[2].story_key, "3-0-third");
    assert_eq!(captured[2].status, StoryStatus::Error);
}

// ===========================================================================
// Task 7 — StoryStatus display & data integrity (AC #1, #4)
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
// Task 8 — NotifierError variants (AC: ALL)
// ===========================================================================

#[test]
fn test_notifier_error_disabled_display() {
    let err = NotifierError::Disabled;
    let display = err.to_string();
    assert!(
        display.contains("disabled") || display.contains("Disabled"),
        "NotifierError::Disabled display should mention 'disabled', got: {display}"
    );
}

#[test]
fn test_notifier_error_types_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NotifierError>();
}
