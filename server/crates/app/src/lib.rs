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
pub mod idempotency;
pub mod server;
pub mod telemetry;

pub use config::{Config, ConfigError};

/// Starts the application: connects to the database, applies migrations (unless disabled),
/// starts the outbox dispatcher, job worker and scheduler, then serves HTTP on `listener` until `shutdown` resolves.
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

    // Domain modules register their subscribers here as they are implemented.
    let dispatcher = db::outbox::Dispatcher::new(db::outbox::DispatchConfig::default());
    let (stop_dispatcher, dispatcher_stopped) = tokio::sync::oneshot::channel::<()>();
    let dispatcher_pool = pool.clone();
    let dispatcher_task = tokio::spawn(async move {
        dispatcher
            .run(&dispatcher_pool, async {
                let _ = dispatcher_stopped.await;
            })
            .await;
    });

    // Background jobs: modules register handlers and schedules here as they are implemented.
    let clock: kernel::SharedClock = std::sync::Arc::new(kernel::SystemClock);
    let mut worker = jobs::Worker::new(jobs::WorkerConfig::default());
    let mut scheduler = jobs::Scheduler::new(jobs::SchedulerConfig::default());
    {
        let pool = pool.clone();
        let clock = clock.clone();
        worker.register(move |_: idempotency::CleanupIdempotencyKeys, _| {
            let pool = pool.clone();
            let before = clock.now() - idempotency::RETENTION;
            async move {
                idempotency::delete_expired(&pool, before).await?;
                Ok(())
            }
        });
    }
    scheduler
        .add(
            "idempotency-cleanup",
            idempotency::CLEANUP_CRON,
            idempotency::CleanupIdempotencyKeys {},
        )
        .map_err(RunError::Schedule)?;
    let (stop_jobs, jobs_stopped) = tokio::sync::watch::channel(false);
    let jobs_task = {
        let pool = pool.clone();
        let mut stopped = jobs_stopped.clone();
        tokio::spawn(async move {
            worker
                .run(&pool, async move {
                    let _ = stopped.wait_for(|stop| *stop).await;
                })
                .await;
        })
    };
    let scheduler_task = {
        let pool = pool.clone();
        let mut stopped = jobs_stopped;
        tokio::spawn(async move {
            scheduler
                .run(&pool, &kernel::SystemClock, async move {
                    let _ = stopped.wait_for(|stop| *stop).await;
                })
                .await;
        })
    };

    if let Ok(addr) = listener.local_addr() {
        tracing::info!(%addr, "listening");
    }

    // `/auth/*` is exempt from idempotency keys (see `idempotency` module docs). Domain modules
    // add their routes inside `protect(..)` as they are implemented.
    let email_sender: kernel::SharedEmailSender = match config.email_driver {
        config::EmailDriver::Log => std::sync::Arc::new(kernel::email::LogEmailSender),
    };
    let mut identity = identity::IdentityService::new(
        pool.clone(),
        clock.clone(),
        identity::EmailSettings {
            sender: email_sender,
            web_url: config.public_web_url.clone(),
        },
    );
    if let Some(google) = &config.google {
        let google = identity::oauth::google::Google::new(identity::oauth::google::GoogleConfig {
            web_client_id: google.web_client_id.clone(),
            web_client_secret: google.web_client_secret.clone(),
            native_client_ids: google.native_client_ids.clone(),
            callback_url: format!("{}/auth/google/callback", config.public_api_url),
            web_url: config.public_web_url.clone(),
            auth_url: identity::oauth::google::AUTH_URL.to_owned(),
            token_url: identity::oauth::google::TOKEN_URL.to_owned(),
        })
        .map_err(|err| RunError::Google(setup_error(err)))?;
        identity = identity.with_google(google);
        tracing::info!("google sign-in enabled");
    }
    if let Some(apple) = &config.apple {
        let apple = identity::oauth::apple::Apple::new(identity::oauth::apple::AppleConfig {
            team_id: apple.team_id.clone(),
            key_id: apple.key_id.clone(),
            private_key_pem: apple.private_key_pem.clone(),
            services_id: apple.services_id.clone(),
            bundle_ids: apple.bundle_ids.clone(),
            callback_url: format!("{}/auth/apple/callback", config.public_api_url),
            web_url: config.public_web_url.clone(),
            auth_url: identity::oauth::apple::AUTH_URL.to_owned(),
            token_url: identity::oauth::apple::TOKEN_URL.to_owned(),
        })
        .map_err(|err| RunError::Apple(setup_error(err)))?;
        identity = identity.with_apple(apple);
        tracing::info!("sign in with apple enabled");
    }
    let idempotency = idempotency::Idempotency::new(pool.clone(), identity.clone(), clock);
    let limits = identity::rate_limit::RateLimits {
        trust_proxy_headers: config.trust_proxy_headers,
        ..identity::rate_limit::RateLimits::default()
    };
    let modules = identity::http::rate_limited_router(identity, limits)
        .merge(idempotency::protect(axum::Router::new(), idempotency));
    let result = server::serve(
        listener,
        http::router_with(modules),
        shutdown,
        config.shutdown_timeout,
    )
    .await
    .map_err(RunError::Serve);

    // Let background tasks finish their current batch before closing the pool.
    let _ = stop_dispatcher.send(());
    let _ = stop_jobs.send(true);
    for (name, task) in [
        ("outbox dispatcher", dispatcher_task),
        ("job worker", jobs_task),
        ("job scheduler", scheduler_task),
    ] {
        if task.await.is_err() {
            tracing::error!(task = name, "background task panicked");
        }
    }
    pool.close().await;
    tracing::info!("shut down");
    result
}

/// Startup message for a provider setup error: the cause, not the generic "internal error".
fn setup_error(err: kernel::AppError) -> String {
    match err {
        kernel::AppError::Internal(cause) => cause.to_string(),
        other => other.to_string(),
    }
}

/// Fatal errors while starting or running the server.
#[derive(Debug)]
pub enum RunError {
    /// Could not connect to the database.
    Database(db::DbError),
    /// A migration failed.
    Migrate(db::MigrateError),
    /// The Google sign-in client could not be built.
    Google(String),
    /// Sign in with Apple could not be set up (e.g. invalid private key).
    Apple(String),
    /// A built-in recurring job has an invalid schedule.
    Schedule(jobs::ScheduleError),
    /// The HTTP server failed or could not drain within the shutdown timeout.
    Serve(io::Error),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(err) => write!(f, "database connection failed: {err}"),
            Self::Migrate(err) => write!(f, "database migration failed: {err}"),
            Self::Google(err) => write!(f, "google sign-in setup failed: {err}"),
            Self::Apple(err) => write!(f, "sign in with apple setup failed: {err}"),
            Self::Schedule(err) => write!(f, "invalid job schedule: {err}"),
            Self::Serve(err) => write!(f, "server error: {err}"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(err) => Some(err),
            Self::Migrate(err) => Some(err),
            Self::Google(_) | Self::Apple(_) => None,
            Self::Schedule(err) => Some(err),
            Self::Serve(err) => Some(err),
        }
    }
}
