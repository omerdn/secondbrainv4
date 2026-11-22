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
    /// Mounts the collaborative editing spike `/collab/{page_id}` (`COLLAB_SPIKE_ENABLED`).
    /// It has no page permissions yet (T0.6.2, E2.5): development only, never in production.
    pub collab_spike_enabled: bool,
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
}

impl FromStr for EmailDriver {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "log" => Ok(Self::Log),
            other => Err(format!("unknown driver `{other}` (expected: log)")),
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
        let collab_spike_enabled: bool = parse(&lookup, "COLLAB_SPIKE_ENABLED", false)?;

        let mut cors_origins = vec![origin_of(&public_web_url, "PUBLIC_WEB_URL")?];
        for extra in comma_list(non_empty("CORS_EXTRA_ORIGINS")) {
            let origin = origin_of(extra.trim_end_matches('/'), "CORS_EXTRA_ORIGINS")?;
            if origin != extra.trim_end_matches('/') {
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
            collab_spike_enabled,
            apple,
            public_api_url,
            google,
            email_driver,
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
fn origin_of(url: &str, key: &'static str) -> Result<String, ConfigError> {
    let invalid = |reason: &str| ConfigError::Invalid {
        key,
        reason: reason.into(),
    };
    let (scheme, rest) = url
        .split_once("://")
        .filter(|(scheme, _)| matches!(*scheme, "http" | "https"))
        .ok_or_else(|| invalid("must start with http:// or https://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let valid_host = !authority.is_empty()
        && authority
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'));
    if !valid_host {
        return Err(invalid("must name a host (no wildcards or credentials)"));
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
            " http://localhost:19006 , http://localhost:8081/, tauri://localhost",
        )]);
        // `tauri://` is not http(s): rejected rather than silently dropped.
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
    fn cors_rejects_wildcards_and_paths() {
        for value in [
            "*",
            "https://*.example.com",
            "https://example.com/app",
            "https://u:p@x.com",
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
    fn the_collab_spike_is_off_by_default() {
        assert!(!config(&[]).unwrap().collab_spike_enabled);
        assert!(
            config(&[("COLLAB_SPIKE_ENABLED", "true")])
                .unwrap()
                .collab_spike_enabled
        );
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
