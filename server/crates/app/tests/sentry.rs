//! Sentry integration (T0.2.4): only error events are sent, with request context and without
//! personal data. Uses Sentry's in-memory test transport; nothing leaves the machine.

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::telemetry::{LogFormat, TelemetryConfig, sentry_layer, sentry_options};
use tracing_subscriber::layer::SubscriberExt;

fn telemetry(dsn: Option<&str>) -> TelemetryConfig {
    TelemetryConfig {
        log_format: LogFormat::Json,
        log_filter: "info".into(),
        sentry_dsn: dsn.map(|d| d.parse().unwrap()),
        environment: "staging".into(),
    }
}

#[test]
fn sentry_is_disabled_without_dsn() {
    assert!(sentry_options(&telemetry(None)).is_none());
}

#[test]
fn sentry_options_never_send_default_pii() {
    let options =
        sentry_options(&telemetry(Some("https://public@o0.ingest.sentry.io/42"))).unwrap();
    assert!(!options.send_default_pii);
    assert_eq!(options.environment.as_deref(), Some("staging"));
    assert!(options.dsn.is_some());
}

#[test]
fn only_error_events_are_captured_with_request_context() {
    let subscriber = tracing_subscriber::registry().with(sentry_layer());
    let _guard = tracing::subscriber::set_default(subscriber);

    let events = sentry::test::with_captured_events(|| {
        let span = tracing::info_span!(
            "request",
            method = "GET",
            route = "/items/{id}",
            request_id = "req-1"
        );
        let _entered = span.enter();
        tracing::info!("request completed");
        tracing::warn!("slow query");
        tracing::error!(
            error = "Key (email)=(***) already exists",
            user_email = "ali@example.com",
            "internal error"
        );
    });

    assert_eq!(events.len(), 1, "only the error is sent: {events:#?}");
    let event = &events[0];
    assert_eq!(event.level, sentry::Level::Error);
    let serialized = serde_json::to_string(event).unwrap();
    assert_eq!(event.message.as_deref(), Some("internal error"));
    assert_eq!(event.tags["route"], "/items/{id}");
    assert_eq!(event.tags["method"], "GET");
    assert_eq!(event.tags["request_id"], "req-1");
    assert_eq!(event.extra["error"], "Key (email)=(***) already exists");
    // Fields outside the allowlist never reach Sentry.
    assert!(!serialized.contains("ali@example.com"), "{serialized}");
    assert!(!serialized.contains("user_email"), "{serialized}");
    assert!(event.user.is_none());
}
