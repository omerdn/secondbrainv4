//! Runtime configuration read from environment variables.
//!
//! Parsing is a pure function over a key lookup ([`Config::from_lookup`]) so it can be tested
//! without touching the process environment.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;

/// Server configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Address the HTTP server listens on (`APP_HOST`, `APP_PORT`).
    pub bind_addr: SocketAddr,
    /// Maximum time to wait for in-flight requests after a shutdown signal
    /// (`SHUTDOWN_TIMEOUT_SECS`).
    pub shutdown_timeout: Duration,
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

    /// Builds the configuration from an arbitrary key lookup. Missing or empty values fall back
    /// to defaults; present but malformed values are an error.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let host: IpAddr = parse(&lookup, "APP_HOST", Self::DEFAULT_HOST)?;
        let port: u16 = parse(&lookup, "APP_PORT", Self::DEFAULT_PORT)?;
        let shutdown_secs: u64 = parse(
            &lookup,
            "SHUTDOWN_TIMEOUT_SECS",
            Self::DEFAULT_SHUTDOWN_TIMEOUT_SECS,
        )?;

        Ok(Self {
            bind_addr: SocketAddr::new(host, port),
            shutdown_timeout: Duration::from_secs(shutdown_secs),
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
            Self::Invalid { key, reason } => write!(f, "invalid value for {key}: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|key| vars.get(key).cloned())
    }

    #[test]
    fn uses_safe_defaults_when_unset() {
        let config = config(&[]).unwrap();
        assert_eq!(config.bind_addr, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(config.shutdown_timeout, Duration::from_secs(30));
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
