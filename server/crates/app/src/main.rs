//! Server entry point.

use std::fmt;
use std::io;

use app::{Config, ConfigError, RunError};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), StartupError> {
    // `.env` is a local-development convenience; in staging/production the environment is set
    // by the container, so a missing file is not an error.
    let _ = dotenvy::dotenv();

    let config = Config::from_env().map_err(StartupError::Config)?;
    let listener = TcpListener::bind(config.bind_addr)
        .await
        .map_err(StartupError::Bind)?;

    app::run(config, listener, app::server::shutdown_signal())
        .await
        .map_err(StartupError::Run)
}

/// Fatal startup errors. `Debug` delegates to `Display` because `main` prints the error with
/// `{:?}` on exit.
enum StartupError {
    Config(ConfigError),
    Bind(io::Error),
    Run(RunError),
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(f, "configuration error: {err}"),
            Self::Bind(err) => write!(f, "could not bind listener: {err}"),
            Self::Run(err) => write!(f, "{err}"),
        }
    }
}

impl fmt::Debug for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
