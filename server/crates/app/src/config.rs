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
}

impl Config {
    /// Default `APP_HOST`: loopback, so a dev server is never exposed on the network by accident.
    pub const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    /// Default `APP_PORT`.
    pub const DEFAULT_PORT: u16 = 8080;
    /// Default `SHUTDOWN_TIMEOUT_SECS`.
    pub const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 30;

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

        Ok(Self {
            bind_addr: SocketAddr::new(host, port),
            shutdown_timeout: Duration::from_secs(shutdown_secs),
            database,
            migrate_on_start,
            telemetry,
        })
    }
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
}
