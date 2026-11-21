//! `app`: configuration, router composition and server startup (T0.2.1, T0.2.3).
//!
//! The binary (`main.rs`) only reads the environment and calls [`run`]; everything here is
//! testable in-process.

use std::fmt;
use std::future::Future;
use std::io;

use tokio::net::TcpListener;

pub mod config;
pub mod http;
pub mod server;
pub mod telemetry;

pub use config::{Config, ConfigError};

/// Starts the application: connects to the database, applies migrations (unless disabled),
/// then serves HTTP on `listener` until `shutdown` resolves.
pub async fn run<S>(config: Config, listener: TcpListener, shutdown: S) -> Result<(), RunError>
where
    S: Future<Output = ()> + Send + 'static,
{
    let pool = db::connect(&config.database)
        .await
        .map_err(RunError::Database)?;

    if config.migrate_on_start {
        db::migrate(&pool).await.map_err(RunError::Migrate)?;
        tracing::info!("database migrations applied");
    }

    if let Ok(addr) = listener.local_addr() {
        tracing::info!(%addr, "listening");
    }

    // The pool becomes router state once modules expose endpoints that need it.
    let result = server::serve(listener, http::router(), shutdown, config.shutdown_timeout)
        .await
        .map_err(RunError::Serve);

    pool.close().await;
    tracing::info!("shut down");
    result
}

/// Fatal errors while starting or running the server.
#[derive(Debug)]
pub enum RunError {
    /// Could not connect to the database.
    Database(db::DbError),
    /// A migration failed.
    Migrate(db::MigrateError),
    /// The HTTP server failed or could not drain within the shutdown timeout.
    Serve(io::Error),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(err) => write!(f, "database connection failed: {err}"),
            Self::Migrate(err) => write!(f, "database migration failed: {err}"),
            Self::Serve(err) => write!(f, "server error: {err}"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(err) => Some(err),
            Self::Migrate(err) => Some(err),
            Self::Serve(err) => Some(err),
        }
    }
}
