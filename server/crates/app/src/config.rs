//! Runtime configuration read from environment variables.
//!
//! Parsing is a pure function over a key lookup ([`Config::from_lookup`]) so it can be tested
//! without touching the process environment.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;

use db::{DatabaseUrl, PoolConfig};
use sentry::types::Dsn;
use tracing_subscriber::EnvFilter;

use crate::telemetry::{DEFAULT_LOG_FILTER, LogFormat, TelemetryConfig};

/// Server configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Address the HTTP server listens on (`APP_HOST`, `APP_PORT`).
    pub bind_addr: SocketAddr,
    /// Maximum time to wait for in-flight requests after a shutdown signal
    /// (`SHUTDOWN_TIMEOUT_SECS`).
    pub shutdown_timeout: Duration,
    /// Database pool settings (`DATABASE_URL` required, `DB_MAX_CONNECTIONS`).
    pub database: PoolConfig,
    /// Apply pending migrations before serving (`DB_MIGRATE_ON_START`, default `true`).
    pub migrate_on_start: bool,
    /// Logging and error tracking (`LOG_FORMAT`, `RUST_LOG`, `SENTRY_DSN`, `APP_ENV`).
    pub telemetry: TelemetryConfig,
    /// Outgoing e-mail driver (`EMAIL_DRIVER`).
    pub email_driver: EmailDriver,
    /// Resend settings (`RESEND_API_KEY`, `EMAIL_FROM`); set exactly when the driver is `resend`.
    pub resend: Option<ResendSettings>,
    /// Web Push (`VAPID_PRIVATE_KEY`, `VAPID_SUBJECT`, T4.2.4); `None` = push is off.
    pub web_push: Option<WebPushSettings>,
    /// Directory of the mobile OTA updates (`UPDATES_DIR`, T4.2.6); `None` = `/updates` is off.
    pub updates_dir: Option<std::path::PathBuf>,
    /// Minimum supported client versions (`MIN_VERSION_*`, `DOWNLOAD_URL_*`, T4.2.7).
    pub client_policy: crate::client_policy::ClientPolicy,
    /// Public URL of the web app for links in e-mails (`PUBLIC_WEB_URL`).
    pub public_web_url: String,
    /// Public URL of this server for OAuth callbacks (`PUBLIC_API_URL`).
    pub public_api_url: String,
    /// Browser origins allowed to call the API with credentials (CORS): the origin of
    /// `PUBLIC_WEB_URL` plus `CORS_EXTRA_ORIGINS` (comma-separated). Never a wildcard.
    pub cors_origins: Vec<String>,
    /// Google sign-in clients; `None` = disabled.
    pub google: Option<GoogleClients>,
    /// Sign in with Apple settings; `None` = disabled.
    pub apple: Option<AppleSettings>,
    /// Take the client IP for rate limits from proxy headers (`TRUST_PROXY_HEADERS`).
    pub trust_proxy_headers: bool,
    /// It has no page permissions yet (T0.6.2, E2.5): development only, never in production.
    /// Access recompute tuning (`ACCESS_SYNC_LIMIT`, T1.1.4).
    pub access: access::AccessConfig,
}

/// Sign in with Apple (`APPLE_*`).
#[derive(Clone, PartialEq, Eq)]
pub struct AppleSettings {
    pub team_id: String,
    pub key_id: String,
    /// PKCS#8 PEM of the `.p8` key.
    pub private_key_pem: String,
    pub services_id: String,
    pub bundle_ids: Vec<String>,
}

/// The private key is never printed.
/// Resend e-mail driver (ADR-0022, T0.7.7).
#[derive(Clone, PartialEq, Eq)]
pub struct ResendSettings {
    /// `RESEND_API_KEY` (secret).
    pub api_key: String,
    /// `EMAIL_FROM`, e.g. `Routune <noreply@routune.dev>`.
    pub from: String,
}

/// The API key is a secret: never in debug output.
impl fmt::Debug for ResendSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResendSettings")
            .field("from", &self.from)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for AppleSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppleSettings")
            .field("team_id", &self.team_id)
            .field("services_id", &self.services_id)
            .field("bundle_ids", &self.bundle_ids)
            .finish_non_exhaustive()
    }
}

fn comma_list(value: Option<String>) -> Vec<String> {
    value
        .map(|ids| {
            ids.split(',')
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Web Push VAPID settings, checked at start-up (T4.2.4).
#[derive(Clone, PartialEq, Eq)]
pub struct WebPushSettings {
    /// base64url raw P-256 private key (secret).
    pub private_key: String,
    /// Contact for push services: `VAPID_SUBJECT`, else `PUBLIC_WEB_URL`.
    pub subject: String,
}

/// The private key is never printed.
impl fmt::Debug for WebPushSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebPushSettings")
            .field("subject", &self.subject)
            .finish_non_exhaustive()
    }
}

impl WebPushSettings {
    /// The parsed key pair; never fails for settings that came from [`Config::from_lookup`].
    ///
    /// # Errors
    /// [`notifications::push::VapidKeyError`] for settings built by hand.
    pub fn key(&self) -> Result<notifications::push::VapidKey, notifications::push::VapidKeyError> {
        notifications::push::VapidKey::parse(&self.private_key, &self.subject)
    }
}

/// Google OAuth clients (`GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_NATIVE_CLIENT_IDS`).
#[derive(Clone, PartialEq, Eq)]
pub struct GoogleClients {
    pub web_client_id: String,
    pub web_client_secret: String,
    pub native_client_ids: Vec<String>,
}

/// The secret is never printed.
impl fmt::Debug for GoogleClients {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleClients")
            .field("web_client_id", &self.web_client_id)
            .field("native_client_ids", &self.native_client_ids)
            .finish_non_exhaustive()
    }
}

/// Outgoing e-mail driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailDriver {
    /// Print to the log instead of sending (local development only).
    Log,
    /// Send through Resend (ADR-0022).
    Resend,
}

impl FromStr for EmailDriver {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "log" => Ok(Self::Log),
            "resend" => Ok(Self::Resend),
            other => Err(format!("unknown driver `{other}` (expected: log, resend)")),
        }
    }
}

impl Config {
    /// Default `APP_HOST`: loopback, so a dev server is never exposed on the network by accident.
    pub const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    /// Default `APP_PORT`.
    pub const DEFAULT_PORT: u16 = 8080;
    /// Default `SHUTDOWN_TIMEOUT_SECS`.
    pub const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 30;
    /// Expo's web dev server.
    pub const DEFAULT_PUBLIC_WEB_URL: &str = "http://localhost:8081";
    /// This server in local development.
    pub const DEFAULT_PUBLIC_API_URL: &str = "http://localhost:8080";

    /// Reads the configuration from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Builds the configuration from an arbitrary key lookup. Missing or empty optional values
    /// fall back to defaults; a missing `DATABASE_URL` or a malformed value is an error.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let host: IpAddr = parse(&lookup, "APP_HOST", Self::DEFAULT_HOST)?;
        let port: u16 = parse(&lookup, "APP_PORT", Self::DEFAULT_PORT)?;
        let shutdown_secs: u64 = parse(
            &lookup,
            "SHUTDOWN_TIMEOUT_SECS",
            Self::DEFAULT_SHUTDOWN_TIMEOUT_SECS,
        )?;

        let database_url = lookup("DATABASE_URL")
            .filter(|url| !url.trim().is_empty())
            .ok_or(ConfigError::Missing {
                key: "DATABASE_URL",
            })?;
        let mut database = PoolConfig::new(DatabaseUrl::new(database_url.trim()));
        database.max_connections = parse(
            &lookup,
            "DB_MAX_CONNECTIONS",
            PoolConfig::DEFAULT_MAX_CONNECTIONS,
        )?;
        if database.max_connections == 0 {
            return Err(ConfigError::Invalid {
                key: "DB_MAX_CONNECTIONS",
                reason: "must be at least 1".into(),
            });
        }
        let migrate_on_start: bool = parse(&lookup, "DB_MIGRATE_ON_START", true)?;

        let log_filter = lookup("RUST_LOG")
            .map(|f| f.trim().to_owned())
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| DEFAULT_LOG_FILTER.to_owned());
        if let Err(err) = EnvFilter::try_new(&log_filter) {
            return Err(ConfigError::Invalid {
                key: "RUST_LOG",
                reason: err.to_string(),
            });
        }
        let sentry_dsn: Option<Dsn> = match lookup("SENTRY_DSN") {
            Some(raw) if !raw.trim().is_empty() => {
                Some(raw.trim().parse().map_err(|_| ConfigError::Invalid {
                    key: "SENTRY_DSN",
                    reason: "not a valid Sentry DSN".into(),
                })?)
            }
            _ => None,
        };
        let telemetry = TelemetryConfig {
            log_format: parse(&lookup, "LOG_FORMAT", LogFormat::Json)?,
            log_filter,
            sentry_dsn,
            environment: lookup("APP_ENV")
                .map(|e| e.trim().to_owned())
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| "local".to_owned()),
        };

        let email_driver: EmailDriver = parse(&lookup, "EMAIL_DRIVER", EmailDriver::Log)?;
        if email_driver == EmailDriver::Log && telemetry.environment == "production" {
            return Err(ConfigError::Invalid {
                key: "EMAIL_DRIVER",
                reason: "the log driver prints e-mail contents and is not allowed in production"
                    .into(),
            });
        }
        let public_web_url = http_url(&lookup, "PUBLIC_WEB_URL", Self::DEFAULT_PUBLIC_WEB_URL)?;

        let public_api_url = http_url(&lookup, "PUBLIC_API_URL", Self::DEFAULT_PUBLIC_API_URL)?;
        let non_empty = |key: &str| {
            lookup(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let updates_dir = non_empty("UPDATES_DIR").map(std::path::PathBuf::from);
        let platform_policy =
            |version_key: &'static str,
             url_key: &'static str|
             -> Result<crate::client_policy::PlatformPolicy, ConfigError> {
                let min_version = non_empty(version_key);
                if let Some(version) = &min_version
                    && !crate::client_policy::is_version(version)
                {
                    return Err(ConfigError::Invalid {
                        key: version_key,
                        reason: "must be major.minor.patch, e.g. 0.2.0".into(),
                    });
                }
                let download_url = match non_empty(url_key) {
                    Some(_) => Some(http_url(&lookup, url_key, "")?),
                    None => None,
                };
                Ok(crate::client_policy::PlatformPolicy {
                    min_version,
                    download_url,
                })
            };
        let client_policy = crate::client_policy::ClientPolicy {
            ios: platform_policy("MIN_VERSION_IOS", "DOWNLOAD_URL_IOS")?,
            android: platform_policy("MIN_VERSION_ANDROID", "DOWNLOAD_URL_ANDROID")?,
            desktop: platform_policy("MIN_VERSION_DESKTOP", "DOWNLOAD_URL_DESKTOP")?,
        };
        let resend = match email_driver {
            EmailDriver::Log => None,
            EmailDriver::Resend => {
                let missing = |key: &'static str| ConfigError::Invalid {
                    key,
                    reason: "required when EMAIL_DRIVER=resend".into(),
                };
                let api_key =
                    non_empty("RESEND_API_KEY").ok_or_else(|| missing("RESEND_API_KEY"))?;
                let from = non_empty("EMAIL_FROM").ok_or_else(|| missing("EMAIL_FROM"))?;
                if !from.contains('@') {
                    return Err(ConfigError::Invalid {
                        key: "EMAIL_FROM",
                        reason: "must be an address, e.g. `Routune <noreply@routune.dev>`".into(),
                    });
                }
                Some(ResendSettings { api_key, from })
            }
        };

        let web_push = match non_empty("VAPID_PRIVATE_KEY") {
            None => None,
            Some(private_key) => {
                let settings = WebPushSettings {
                    private_key,
                    subject: non_empty("VAPID_SUBJECT").unwrap_or_else(|| public_web_url.clone()),
                };
                settings.key().map_err(|err| ConfigError::Invalid {
                    key: match err {
                        notifications::push::VapidKeyError::PrivateKey => "VAPID_PRIVATE_KEY",
                        notifications::push::VapidKeyError::Subject => "VAPID_SUBJECT",
                    },
                    reason: err.to_string(),
                })?;
                Some(settings)
            }
        };

        let google = match (
            non_empty("GOOGLE_CLIENT_ID"),
            non_empty("GOOGLE_CLIENT_SECRET"),
        ) {
            (Some(web_client_id), Some(web_client_secret)) => Some(GoogleClients {
                web_client_id,
                web_client_secret,
                native_client_ids: comma_list(non_empty("GOOGLE_NATIVE_CLIENT_IDS")),
            }),
            (None, None) => None,
            _ => {
                return Err(ConfigError::Invalid {
                    key: "GOOGLE_CLIENT_ID",
                    reason: "GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET must be set together".into(),
                });
            }
        };

        let apple_key = match (
            non_empty("APPLE_PRIVATE_KEY"),
            non_empty("APPLE_PRIVATE_KEY_PATH"),
        ) {
            (Some(pem), _) => Some(pem.replace("\\n", "\n")),
            (None, Some(path)) => {
                Some(
                    std::fs::read_to_string(&path).map_err(|err| ConfigError::Invalid {
                        key: "APPLE_PRIVATE_KEY_PATH",
                        reason: format!("cannot read the key file: {}", err.kind()),
                    })?,
                )
            }
            (None, None) => None,
        };
        let apple_parts = (
            non_empty("APPLE_TEAM_ID"),
            non_empty("APPLE_KEY_ID"),
            apple_key,
            non_empty("APPLE_SERVICES_ID"),
        );
        let apple = match apple_parts {
            (Some(team_id), Some(key_id), Some(private_key_pem), Some(services_id)) => {
                Some(AppleSettings {
                    team_id,
                    key_id,
                    private_key_pem,
                    services_id,
                    bundle_ids: comma_list(non_empty("APPLE_BUNDLE_IDS")),
                })
            }
            (None, None, None, None) => None,
            _ => {
                return Err(ConfigError::Invalid {
                    key: "APPLE_TEAM_ID",
                    reason: "APPLE_TEAM_ID, APPLE_KEY_ID, APPLE_PRIVATE_KEY(_PATH) and \
                             APPLE_SERVICES_ID must be set together"
                        .into(),
                });
            }
        };

        let trust_proxy_headers: bool = parse(&lookup, "TRUST_PROXY_HEADERS", false)?;
        let access = access::AccessConfig {
            sync_limit: parse(
                &lookup,
                "ACCESS_SYNC_LIMIT",
                access::AccessConfig::DEFAULT_SYNC_LIMIT,
            )?,
        };

        let mut cors_origins = vec![origin_of(&public_web_url, "PUBLIC_WEB_URL", WEB_SCHEMES)?];
        for extra in comma_list(non_empty("CORS_EXTRA_ORIGINS")) {
            let origin = origin_of(
                extra.trim_end_matches('/'),
                "CORS_EXTRA_ORIGINS",
                CORS_EXTRA_SCHEMES,
            )?;
            if !origin.eq_ignore_ascii_case(extra.trim_end_matches('/')) {
                return Err(ConfigError::Invalid {
                    key: "CORS_EXTRA_ORIGINS",
                    reason: "each entry must be an origin (scheme://host[:port]) without a path"
                        .into(),
                });
            }
            if !cors_origins.contains(&origin) {
                cors_origins.push(origin);
            }
        }

        Ok(Self {
            cors_origins,
            trust_proxy_headers,
            access,
            apple,
            public_api_url,
            google,
            email_driver,
            resend,
            web_push,
            updates_dir,
            client_policy,
            public_web_url,
            bind_addr: SocketAddr::new(host, port),
            shutdown_timeout: Duration::from_secs(shutdown_secs),
            database,
            migrate_on_start,
            telemetry,
        })
    }
}

/// An `http(s)://` URL without a trailing slash.
fn http_url(
    lookup: &impl Fn(&str) -> Option<String>,
    key: &'static str,
    default: &str,
) -> Result<String, ConfigError> {
    let url = lookup(key)
        .map(|u| u.trim().trim_end_matches('/').to_owned())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| default.to_owned());
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(url)
    } else {
        Err(ConfigError::Invalid {
            key,
            reason: "must start with http:// or https://".into(),
        })
    }
}

/// The origin (`scheme://host[:port]`, lower-cased) of an `http(s)://` URL. Rejects wildcards
/// and anything that is not a plain host.
/// Schemes of a web app origin.
const WEB_SCHEMES: &[&str] = &["http", "https"];

/// Extra CORS origins may also be the Tauri desktop shell's custom-protocol origin
/// (`tauri://localhost` on macOS/Linux; Windows uses `http(s)://tauri.localhost`). ADR-0020,
/// T0.6.4.
const CORS_EXTRA_SCHEMES: &[&str] = &["http", "https", "tauri"];

fn origin_of(url: &str, key: &'static str, schemes: &[&str]) -> Result<String, ConfigError> {
    let invalid = |reason: String| ConfigError::Invalid { key, reason };
    let (scheme, rest) = url
        .split_once("://")
        .filter(|(scheme, _)| schemes.contains(scheme))
        .ok_or_else(|| invalid(format!("scheme must be one of: {}", schemes.join(", "))))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let valid_host = !authority.is_empty()
        && authority
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'));
    if !valid_host {
        return Err(invalid(
            "must name a host (no wildcards or credentials)".into(),
        ));
    }
    Ok(format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

fn parse<T>(
    lookup: &impl Fn(&str) -> Option<String>,
    key: &'static str,
    default: T,
) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    match lookup(key) {
        Some(raw) if !raw.trim().is_empty() => {
            raw.trim()
                .parse()
                .map_err(|err: T::Err| ConfigError::Invalid {
                    key,
                    reason: err.to_string(),
                })
        }
        _ => Ok(default),
    }
}

/// A configuration value could not be parsed.
///
/// The offending value is deliberately not included: configuration may contain secrets and
/// error messages end up in logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A required `key` is not set.
    Missing {
        /// Environment variable name.
        key: &'static str,
    },
    /// `key` was set but could not be parsed.
    Invalid {
        /// Environment variable name.
        key: &'static str,
        /// Parser error message.
        reason: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { key } => write!(f, "{key} is required"),
            Self::Invalid { key, reason } => write!(f, "invalid value for {key}: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const URL: &str = "postgres://app:pw@localhost:5432/secondbrain";

    /// Lookup with `DATABASE_URL` preset (overridable by `vars`).
    fn config(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let mut map: HashMap<String, String> =
            HashMap::from([("DATABASE_URL".to_owned(), URL.to_owned())]);
        map.extend(vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
        Config::from_lookup(|key| map.get(key).cloned())
    }

    #[test]
    fn uses_safe_defaults_when_unset() {
        let config = config(&[]).unwrap();
        assert_eq!(config.bind_addr, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(config.shutdown_timeout, Duration::from_secs(30));
        assert_eq!(config.database.url.expose(), URL);
        assert_eq!(
            config.database.max_connections,
            PoolConfig::DEFAULT_MAX_CONNECTIONS
        );
        assert!(config.migrate_on_start);
    }

    #[test]
    fn telemetry_defaults_to_json_without_sentry() {
        let telemetry = config(&[]).unwrap().telemetry;
        assert_eq!(telemetry.log_format, LogFormat::Json);
        assert_eq!(telemetry.log_filter, DEFAULT_LOG_FILTER);
        assert!(telemetry.sentry_dsn.is_none());
        assert_eq!(telemetry.environment, "local");
    }

    #[test]
    fn reads_telemetry_settings() {
        let telemetry = config(&[
            ("LOG_FORMAT", "Pretty"),
            ("RUST_LOG", "debug,hyper=warn"),
            ("SENTRY_DSN", "https://public@o0.ingest.sentry.io/42"),
            ("APP_ENV", "staging"),
        ])
        .unwrap()
        .telemetry;
        assert_eq!(telemetry.log_format, LogFormat::Pretty);
        assert_eq!(telemetry.log_filter, "debug,hyper=warn");
        assert!(telemetry.sentry_dsn.is_some());
        assert_eq!(telemetry.environment, "staging");
    }

    #[test]
    fn rejects_invalid_telemetry_settings() {
        for (key, value) in [
            ("LOG_FORMAT", "xml"),
            ("RUST_LOG", "info,[unclosed"),
            ("SENTRY_DSN", "not a dsn"),
        ] {
            let err = config(&[(key, value)]).unwrap_err();
            assert!(
                matches!(err, ConfigError::Invalid { key: k, .. } if k == key),
                "{key}: {err:?}"
            );
        }
    }

    #[test]
    fn database_url_is_required() {
        let err = config(&[("DATABASE_URL", "  ")]).unwrap_err();
        assert_eq!(
            err,
            ConfigError::Missing {
                key: "DATABASE_URL"
            }
        );
        assert_eq!(err.to_string(), "DATABASE_URL is required");
    }

    #[test]
    fn reads_database_settings() {
        let parsed = config(&[
            ("DB_MAX_CONNECTIONS", "3"),
            ("DB_MIGRATE_ON_START", "false"),
        ])
        .unwrap();
        assert_eq!(parsed.database.max_connections, 3);
        assert!(!parsed.migrate_on_start);

        let err = config(&[("DB_MAX_CONNECTIONS", "0")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "DB_MAX_CONNECTIONS",
                ..
            }
        ));
        let err = config(&[("DB_MIGRATE_ON_START", "yes")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "DB_MIGRATE_ON_START",
                ..
            }
        ));
    }

    #[test]
    fn debug_output_redacts_database_password() {
        let config = config(&[]).unwrap();
        assert!(!format!("{config:?}").contains(":pw@"));
    }

    #[test]
    fn reads_overrides() {
        let config = config(&[
            ("APP_HOST", "0.0.0.0"),
            ("APP_PORT", "9000"),
            ("SHUTDOWN_TIMEOUT_SECS", "5"),
        ])
        .unwrap();
        assert_eq!(config.bind_addr, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.shutdown_timeout, Duration::from_secs(5));
    }

    #[test]
    fn supports_ipv6_hosts() {
        let config = config(&[("APP_HOST", "::1")]).unwrap();
        assert_eq!(config.bind_addr, "[::1]:8080".parse().unwrap());
    }

    #[test]
    fn treats_empty_values_as_unset() {
        let config = config(&[("APP_PORT", "  "), ("APP_HOST", "")]).unwrap();
        assert_eq!(config.bind_addr, "127.0.0.1:8080".parse().unwrap());
    }

    #[test]
    fn rejects_malformed_values_naming_the_key() {
        let err = config(&[("APP_PORT", "eighty")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "APP_PORT",
                ..
            }
        ));

        let err = config(&[("APP_PORT", "70000")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "APP_PORT",
                ..
            }
        ));

        let err = config(&[("APP_HOST", "not-an-ip")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "APP_HOST",
                ..
            }
        ));
    }

    #[test]
    fn error_message_does_not_echo_the_value() {
        let err = config(&[("SHUTDOWN_TIMEOUT_SECS", "s3cr3t-value")]).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("SHUTDOWN_TIMEOUT_SECS"));
        assert!(!message.contains("s3cr3t-value"));
    }

    #[test]
    fn email_defaults_to_log_driver_and_local_web_url() {
        let config = config(&[("DATABASE_URL", URL)]).unwrap();
        assert_eq!(config.email_driver, EmailDriver::Log);
        assert_eq!(config.public_web_url, "http://localhost:8081");
    }

    #[test]
    fn log_email_driver_is_refused_in_production() {
        let err = config(&[("DATABASE_URL", URL), ("APP_ENV", "production")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "EMAIL_DRIVER",
                ..
            }
        ));
        assert!(config(&[("DATABASE_URL", URL), ("APP_ENV", "staging")]).is_ok());
    }

    #[test]
    fn resend_driver_needs_a_key_and_a_sender_address() {
        for (vars, key) in [
            (vec![("EMAIL_DRIVER", "resend")], "RESEND_API_KEY"),
            (
                vec![("EMAIL_DRIVER", "resend"), ("RESEND_API_KEY", "re_x")],
                "EMAIL_FROM",
            ),
            (
                vec![
                    ("EMAIL_DRIVER", "resend"),
                    ("RESEND_API_KEY", "re_x"),
                    ("EMAIL_FROM", "Routune"),
                ],
                "EMAIL_FROM",
            ),
        ] {
            let mut all = vec![("DATABASE_URL", URL)];
            all.extend(vars);
            let err = config(&all).unwrap_err();
            assert!(
                matches!(&err, ConfigError::Invalid { key: k, .. } if *k == key),
                "{err:?}"
            );
        }
    }

    #[test]
    fn resend_driver_is_allowed_in_production_and_hides_the_key() {
        let config = config(&[
            ("DATABASE_URL", URL),
            ("APP_ENV", "production"),
            ("EMAIL_DRIVER", "resend"),
            ("RESEND_API_KEY", "re_s3cret"),
            ("EMAIL_FROM", "Routune <noreply@routune.dev>"),
        ])
        .unwrap();
        assert_eq!(config.email_driver, EmailDriver::Resend);
        let resend = config.resend.clone().unwrap();
        assert_eq!(resend.from, "Routune <noreply@routune.dev>");
        assert!(!format!("{config:?}").contains("re_s3cret"));
    }

    #[test]
    fn web_push_is_off_without_a_key_and_checked_with_one() {
        assert!(config(&[]).unwrap().web_push.is_none());
        let private = notifications::push::VapidKey::generate_private_key();
        let config_with = config(&[("VAPID_PRIVATE_KEY", &private)]).unwrap();
        let web_push = config_with.web_push.clone().unwrap();
        assert_eq!(web_push.subject, "http://localhost:8081");
        assert!(web_push.key().is_ok());
        assert!(!format!("{config_with:?}").contains(&private));
        let subject = config(&[
            ("VAPID_PRIVATE_KEY", &private),
            ("VAPID_SUBJECT", "mailto:noreply@routune.dev"),
        ])
        .unwrap();
        assert_eq!(
            subject.web_push.unwrap().subject,
            "mailto:noreply@routune.dev"
        );
        assert!(matches!(
            config(&[("VAPID_PRIVATE_KEY", "not-a-key")]),
            Err(ConfigError::Invalid {
                key: "VAPID_PRIVATE_KEY",
                ..
            })
        ));
        assert!(matches!(
            config(&[
                ("VAPID_PRIVATE_KEY", &private),
                ("VAPID_SUBJECT", "ftp://x")
            ]),
            Err(ConfigError::Invalid {
                key: "VAPID_SUBJECT",
                ..
            })
        ));
    }

    #[test]
    fn client_policy_reads_versions_and_download_urls() {
        let config = config(&[
            ("DATABASE_URL", URL),
            ("MIN_VERSION_DESKTOP", "0.2.0"),
            ("DOWNLOAD_URL_DESKTOP", "https://example.test/desktop/"),
        ])
        .unwrap();
        let desktop = &config.client_policy.desktop;
        assert_eq!(desktop.min_version.as_deref(), Some("0.2.0"));
        assert_eq!(
            desktop.download_url.as_deref(),
            Some("https://example.test/desktop")
        );
        assert_eq!(config.client_policy.ios, Default::default());
        for (key, value) in [
            ("MIN_VERSION_IOS", "1.2"),
            ("DOWNLOAD_URL_ANDROID", "ftp://example.test/app.apk"),
        ] {
            let err = config_err(&[("DATABASE_URL", URL), (key, value)]);
            assert!(
                matches!(&err, ConfigError::Invalid { key: k, .. } if *k == key),
                "{err:?}"
            );
        }
    }

    #[test]
    fn ota_updates_are_off_unless_a_directory_is_set() {
        assert_eq!(config(&[("DATABASE_URL", URL)]).unwrap().updates_dir, None);
        let config = config(&[("DATABASE_URL", URL), ("UPDATES_DIR", "/srv/updates")]).unwrap();
        assert_eq!(
            config.updates_dir.as_deref(),
            Some(std::path::Path::new("/srv/updates"))
        );
    }

    #[test]
    fn unknown_email_driver_is_rejected() {
        let err = config(&[("DATABASE_URL", URL), ("EMAIL_DRIVER", "smtp")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "EMAIL_DRIVER",
                ..
            }
        ));
    }

    #[test]
    fn public_web_url_is_normalized_and_checked() {
        let config = config(&[
            ("DATABASE_URL", URL),
            ("PUBLIC_WEB_URL", " https://app.example.com/ "),
        ])
        .unwrap();
        assert_eq!(config.public_web_url, "https://app.example.com");
        let err = config_err(&[("DATABASE_URL", URL), ("PUBLIC_WEB_URL", "app.example.com")]);
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "PUBLIC_WEB_URL",
                ..
            }
        ));
    }

    fn config_err(vars: &[(&str, &str)]) -> ConfigError {
        config(vars).unwrap_err()
    }

    #[test]
    fn google_is_disabled_unless_both_web_credentials_are_set() {
        assert_eq!(config(&[]).unwrap().google, None);
        let err = config_err(&[("GOOGLE_CLIENT_ID", "id")]);
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "GOOGLE_CLIENT_ID",
                ..
            }
        ));
        let google = config(&[
            ("GOOGLE_CLIENT_ID", "id"),
            ("GOOGLE_CLIENT_SECRET", "s3cr3t"),
            ("GOOGLE_NATIVE_CLIENT_IDS", " ios , android ,"),
        ])
        .unwrap()
        .google
        .unwrap();
        assert_eq!(google.native_client_ids, ["ios", "android"]);
        assert!(!format!("{google:?}").contains("s3cr3t"));
    }

    #[test]
    fn public_api_url_defaults_to_local_server() {
        assert_eq!(config(&[]).unwrap().public_api_url, "http://localhost:8080");
    }

    #[test]
    fn apple_requires_all_settings_together() {
        assert_eq!(config(&[]).unwrap().apple, None);
        let err = config_err(&[("APPLE_TEAM_ID", "T")]);
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "APPLE_TEAM_ID",
                ..
            }
        ));
        let apple = config(&[
            ("APPLE_TEAM_ID", "T"),
            ("APPLE_KEY_ID", "K"),
            (
                "APPLE_PRIVATE_KEY",
                "-----BEGIN PRIVATE KEY-----\\nabc\\n-----END PRIVATE KEY-----",
            ),
            ("APPLE_SERVICES_ID", "com.example.web"),
            ("APPLE_BUNDLE_IDS", "com.example.app"),
        ])
        .unwrap()
        .apple
        .unwrap();
        assert!(
            apple.private_key_pem.contains("\nabc\n"),
            "escaped newlines are expanded"
        );
        assert_eq!(apple.bundle_ids, ["com.example.app"]);
        assert!(!format!("{apple:?}").contains("PRIVATE KEY"));
    }

    #[test]
    fn unreadable_apple_key_file_is_reported() {
        let err = config_err(&[
            ("APPLE_TEAM_ID", "T"),
            ("APPLE_KEY_ID", "K"),
            ("APPLE_PRIVATE_KEY_PATH", "/nonexistent/key.p8"),
            ("APPLE_SERVICES_ID", "s"),
        ]);
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "APPLE_PRIVATE_KEY_PATH",
                ..
            }
        ));
    }

    #[test]
    fn cors_allows_only_the_web_origin_by_default() {
        assert_eq!(config(&[]).unwrap().cors_origins, ["http://localhost:8081"]);
        let config = config(&[("PUBLIC_WEB_URL", "https://App.Example.com/app/")]).unwrap();
        assert_eq!(config.cors_origins, ["https://app.example.com"]);
    }

    #[test]
    fn cors_extra_origins_are_added_once() {
        let config = config(&[(
            "CORS_EXTRA_ORIGINS",
            " http://localhost:19006 , http://localhost:8081/, file://localhost",
        )]);
        // An unsupported scheme is rejected rather than silently dropped.
        assert!(matches!(
            config.unwrap_err(),
            ConfigError::Invalid {
                key: "CORS_EXTRA_ORIGINS",
                ..
            }
        ));
        let config = config_ok(&[(
            "CORS_EXTRA_ORIGINS",
            "http://localhost:19006,http://localhost:8081/",
        )]);
        assert_eq!(
            config.cors_origins,
            ["http://localhost:8081", "http://localhost:19006"]
        );
    }

    #[test]
    fn cors_extra_origins_accept_tauri_desktop_origins() {
        let config = config_ok(&[(
            "CORS_EXTRA_ORIGINS",
            "tauri://localhost, http://tauri.localhost, https://Tauri.localhost/",
        )]);
        assert_eq!(
            config.cors_origins,
            [
                "http://localhost:8081",
                "tauri://localhost",
                "http://tauri.localhost",
                "https://tauri.localhost",
            ]
        );
    }

    #[test]
    fn the_web_url_must_stay_http() {
        let err = config_err(&[("PUBLIC_WEB_URL", "tauri://localhost")]);
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "PUBLIC_WEB_URL",
                ..
            }
        ));
    }

    #[test]
    fn cors_rejects_wildcards_and_paths() {
        for value in [
            "*",
            "https://*.example.com",
            "https://example.com/app",
            "https://u:p@x.com",
            "tauri://*",
            "tauri://localhost/index.html",
            "tauri://",
        ] {
            let err = config_err(&[("CORS_EXTRA_ORIGINS", value)]);
            assert!(
                matches!(
                    err,
                    ConfigError::Invalid {
                        key: "CORS_EXTRA_ORIGINS",
                        ..
                    }
                ),
                "{value}: {err:?}"
            );
        }
    }

    fn config_ok(vars: &[(&str, &str)]) -> Config {
        config(vars).unwrap()
    }

    #[test]
    fn access_sync_limit_defaults_and_parses() {
        assert_eq!(config(&[]).unwrap().access.sync_limit, 500);
        assert_eq!(
            config(&[("ACCESS_SYNC_LIMIT", "50")])
                .unwrap()
                .access
                .sync_limit,
            50
        );
        assert!(config(&[("ACCESS_SYNC_LIMIT", "-1")]).is_err());
    }

    #[test]
    fn proxy_headers_are_not_trusted_by_default() {
        assert!(!config(&[]).unwrap().trust_proxy_headers);
        assert!(
            config(&[("TRUST_PROXY_HEADERS", "true")])
                .unwrap()
                .trust_proxy_headers
        );
    }
}
