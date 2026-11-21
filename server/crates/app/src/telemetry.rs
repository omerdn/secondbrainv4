//! Logging and error tracking (T0.2.4, ARCHITECTURE §2: `tracing` + Sentry).
//!
//! - Logs are structured: one JSON object per line on stdout (`LOG_FORMAT=json`, default) or
//!   human-readable (`LOG_FORMAT=pretty`, for local development). Level filter: `RUST_LOG`.
//! - Sentry is optional: enabled only when `SENTRY_DSN` is set. Only `error`-level events are
//!   sent, and only an **allowlist** of data: the event message, its `error` field (already
//!   redacted at the source) and the request's `method`, `route` and `request_id` as tags.
//!   Other event fields never reach Sentry. `send_default_pii` is off as well.
//!
//! What may be logged is defined in `crate::http` (request logs) and `kernel::redact`
//! (error text).

use std::fmt;
use std::io;
use std::str::FromStr;

use sentry::ClientOptions;
use sentry::types::Dsn;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Default `RUST_LOG`.
pub const DEFAULT_LOG_FILTER: &str = "info,sqlx=warn";

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// One JSON object per line (production, log shipping).
    Json,
    /// Human-readable, colored (local development).
    Pretty,
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "pretty" => Ok(Self::Pretty),
            _ => Err("expected `json` or `pretty`".into()),
        }
    }
}

/// Telemetry settings, part of [`crate::Config`].
#[derive(Debug, Clone, PartialEq)]
pub struct TelemetryConfig {
    /// `LOG_FORMAT`.
    pub log_format: LogFormat,
    /// `RUST_LOG` directives (validated when the config is parsed).
    pub log_filter: String,
    /// `SENTRY_DSN`; `None` disables Sentry.
    pub sentry_dsn: Option<Dsn>,
    /// `APP_ENV`, reported to Sentry (`local`, `staging`, `production`).
    pub environment: String,
}

/// The fmt layer used in production and in tests (with a capturing writer).
pub fn fmt_layer<S, W>(format: LogFormat, writer: W) -> Box<dyn Layer<S> + Send + Sync>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    match format {
        LogFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .with_writer(writer)
            .boxed(),
        LogFormat::Pretty => tracing_subscriber::fmt::layer().with_writer(writer).boxed(),
    }
}

/// Sentry client options, or `None` if Sentry is disabled.
pub fn sentry_options(config: &TelemetryConfig) -> Option<ClientOptions> {
    config.sentry_dsn.as_ref().map(|dsn| {
        ClientOptions::new()
            .dsn(&dsn.to_string())
            .environment(config.environment.clone())
            .maybe_release(sentry::release_name!())
            .send_default_pii(false)
            // Error tracking only; no performance tracing.
            .traces_sample_rate(0.0)
    })
}

/// Sends `error`-level events to Sentry (see the module docs for exactly what is sent).
pub fn sentry_layer() -> SentryErrorLayer {
    SentryErrorLayer
}

/// Allowlist-based Sentry layer. Records the fields of `request` spans (created in
/// `crate::http`) and attaches them as tags to error events emitted inside those spans.
#[derive(Debug, Clone, Copy, Default)]
pub struct SentryErrorLayer;

/// Name of the per-request span created in `crate::http`.
const REQUEST_SPAN: &str = "request";

#[derive(Debug, Clone, Default)]
struct RequestContext {
    method: Option<String>,
    route: Option<String>,
    request_id: Option<String>,
}

impl Visit for RequestContext {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let value = format!("{value:?}");
        match field.name() {
            "method" => self.method = Some(value),
            "route" => self.route = Some(value),
            "request_id" => self.request_id = Some(value),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_debug(field, &format_args!("{value}"));
    }
}

/// The only event fields forwarded to Sentry.
#[derive(Debug, Default)]
struct AllowedEventFields {
    message: Option<String>,
    error: Option<String>,
}

impl Visit for AllowedEventFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "message" => self.message = Some(format!("{value:?}")),
            "error" => self.error = Some(format!("{value:?}")),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_debug(field, &format_args!("{value}"));
    }
}

impl<S> Layer<S> for SentryErrorLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if attrs.metadata().name() != REQUEST_SPAN {
            return;
        }
        let mut context = RequestContext::default();
        attrs.record(&mut context);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(context);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if *event.metadata().level() != Level::ERROR {
            return;
        }
        let mut fields = AllowedEventFields::default();
        event.record(&mut fields);

        let mut sentry_event = sentry::protocol::Event {
            level: sentry::Level::Error,
            message: fields.message,
            logger: Some(event.metadata().target().to_owned()),
            ..Default::default()
        };
        if let Some(error) = fields.error {
            sentry_event.extra.insert("error".into(), error.into());
        }

        let request = ctx.event_scope(event).and_then(|scope| {
            scope
                .from_root()
                .filter_map(|span| span.extensions().get::<RequestContext>().cloned())
                .last()
        });
        if let Some(request) = request {
            for (key, value) in [
                ("method", request.method),
                ("route", request.route),
                ("request_id", request.request_id),
            ] {
                if let Some(value) = value {
                    sentry_event.tags.insert(key.into(), value);
                }
            }
        }

        sentry::capture_event(sentry_event);
    }
}

/// Keeps Sentry alive and flushes pending events on drop. Hold it for the life of `main`.
#[must_use = "dropping the guard shuts down error reporting"]
pub struct TelemetryGuard {
    _sentry: Option<sentry::ClientInitGuard>,
}

impl fmt::Debug for TelemetryGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryGuard")
            .field("sentry", &self._sentry.is_some())
            .finish()
    }
}

/// Installs the global subscriber (and Sentry, if configured). Call once at startup.
pub fn init(config: &TelemetryConfig) -> Result<TelemetryGuard, TelemetryError> {
    let filter = EnvFilter::try_new(&config.log_filter).map_err(|_| TelemetryError::Filter)?;
    let sentry_guard = sentry_options(config).map(sentry::init);
    let sentry_layer = sentry_guard.as_ref().map(|_| sentry_layer());

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer(config.log_format, io::stdout))
        .with(sentry_layer)
        .try_init()
        .map_err(|_| TelemetryError::AlreadyInitialized)?;

    Ok(TelemetryGuard {
        _sentry: sentry_guard,
    })
}

/// Telemetry could not be initialised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryError {
    /// `RUST_LOG` is not a valid filter.
    Filter,
    /// A global subscriber was already installed.
    AlreadyInitialized,
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Filter => f.write_str("invalid RUST_LOG filter"),
            Self::AlreadyInitialized => f.write_str("logging is already initialised"),
        }
    }
}

impl std::error::Error for TelemetryError {}
