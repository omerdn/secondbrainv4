//! `db`: connection pool, migrations and database test helpers (ARCHITECTURE §3).
//!
//! - [`connect`] builds the shared [`PgPool`].
//! - [`MIGRATOR`] embeds every file in `server/migrations/` into the binary; [`migrate`] applies
//!   pending ones. Migrations are append-only: never edit a migration that has been merged.
//! - `testing` (feature `testing`): [`testing::TestDb`] gives each test its own schema in a
//!   shared Postgres 17 container.
//!
//! Module boundary (ADR-0007): this crate owns no domain tables. Each domain crate queries only
//! its own tables through the pool it receives.

use std::fmt;
use std::time::Duration;

pub use sqlx::Error as DbError;
pub use sqlx::PgPool;
pub use sqlx::migrate::MigrateError;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;

#[cfg(feature = "testing")]
pub mod testing;

/// All migrations in `server/migrations/`, embedded at compile time.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// A Postgres connection string. `Debug` and `Display` redact the password so the URL can be
/// part of logged configuration.
#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseUrl(String);

impl DatabaseUrl {
    /// Wraps a connection string.
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }

    /// The full connection string, including credentials. Do not log.
    pub fn expose(&self) -> &str {
        &self.0
    }

    fn redacted(&self) -> String {
        // postgres://user:password@host/db -> postgres://user:***@host/db
        match (self.0.find("://"), self.0.rfind('@')) {
            (Some(scheme_end), Some(at)) if at > scheme_end => {
                let creds = &self.0[scheme_end + 3..at];
                match creds.find(':') {
                    Some(colon) => format!(
                        "{}{}:***{}",
                        &self.0[..scheme_end + 3],
                        &creds[..colon],
                        &self.0[at..]
                    ),
                    None => self.0.clone(),
                }
            }
            _ => self.0.clone(),
        }
    }
}

impl fmt::Debug for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DatabaseUrl({})", self.redacted())
    }
}

impl fmt::Display for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

/// Pool settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolConfig {
    /// Connection string.
    pub url: DatabaseUrl,
    /// Upper bound on open connections.
    pub max_connections: u32,
    /// How long a request waits for a free connection before failing.
    pub acquire_timeout: Duration,
}

impl PoolConfig {
    /// Default `max_connections`.
    pub const DEFAULT_MAX_CONNECTIONS: u32 = 10;
    /// Default `acquire_timeout`.
    pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

    /// Settings with defaults for everything but the URL.
    pub fn new(url: DatabaseUrl) -> Self {
        Self {
            url,
            max_connections: Self::DEFAULT_MAX_CONNECTIONS,
            acquire_timeout: Self::DEFAULT_ACQUIRE_TIMEOUT,
        }
    }
}

/// Opens a connection pool and verifies that the database is reachable.
pub async fn connect(config: &PoolConfig) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(config.acquire_timeout)
        .connect(config.url.expose())
        .await
}

/// Applies all pending migrations. Safe to run repeatedly and from several instances at once
/// (sqlx takes an advisory lock).
pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    MIGRATOR.run(pool).await
}

/// Round-trips a trivial query; used by readiness checks.
pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_url_redacts_password() {
        let url = DatabaseUrl::new("postgres://app:hunter2@db.internal:5432/secondbrain");
        assert_eq!(
            url.to_string(),
            "postgres://app:***@db.internal:5432/secondbrain"
        );
        assert!(!format!("{url:?}").contains("hunter2"));
        assert_eq!(
            url.expose(),
            "postgres://app:hunter2@db.internal:5432/secondbrain"
        );
    }

    #[test]
    fn database_url_without_password_is_unchanged() {
        let url = DatabaseUrl::new("postgres://localhost/secondbrain");
        assert_eq!(url.to_string(), "postgres://localhost/secondbrain");
    }

    #[test]
    fn migrations_have_unique_versions_and_descriptions() {
        let migrations: Vec<_> = MIGRATOR.iter().collect();
        assert!(!migrations.is_empty());
        let mut versions: Vec<i64> = migrations.iter().map(|m| m.version).collect();
        versions.dedup();
        assert_eq!(
            versions.len(),
            migrations.len(),
            "duplicate migration version"
        );
        assert!(migrations.iter().all(|m| !m.description.is_empty()));
    }
}
