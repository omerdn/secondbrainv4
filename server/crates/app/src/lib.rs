//! `app`: configuration, router composition and server startup (T0.2.1, T0.2.3).
//!
//! The binary (`main.rs`) only reads the environment and calls [`run`]; everything here is
//! testable in-process.

use std::fmt;
use std::future::Future;
use std::io;

use tokio::net::TcpListener;

pub mod client_policy;
pub mod config;
pub mod http;
pub mod idempotency;
pub mod server;
pub mod telemetry;

pub use config::{Config, ConfigError};

/// Starts the application: connects to the database, applies migrations (unless disabled),
/// starts the realtime bridge, the outbox dispatcher, job worker and scheduler, then serves HTTP on `listener` until `shutdown` resolves.
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

    // Domain modules publish "something changed" events with `realtime::Notifier` (Postgres
    // NOTIFY); the bridge delivers them, from every instance, to this instance's connections.
    let hub = realtime::Hub::new();
    let bridge = realtime::Bridge::start(&pool, realtime::DEFAULT_NOTIFY_CHANNEL, hub.clone())
        .await
        .map_err(RunError::Realtime)?;

    let clock: kernel::SharedClock = std::sync::Arc::new(kernel::SystemClock);
    // Access engine (T1.1.3, T1.1.4) with item facts from `items`; team policies are defaults
    // until `teams` provides them (T5.1.2).
    let access_engine = access::AccessEngine::new(items::ItemsAccessContext, config.access);
    let items = items::ItemsService::new(pool.clone(), clock.clone(), access_engine.clone());

    // Domain modules register their subscribers here as they are implemented.
    let mut dispatcher = db::outbox::Dispatcher::new(db::outbox::DispatchConfig::default());
    realtime::sessions::register(&mut dispatcher, pool.clone(), realtime::Notifier::default());
    realtime::access::register(&mut dispatcher, pool.clone(), realtime::Notifier::default());
    items::register_link_cleanup(&mut dispatcher, items.clone());
    let (stop_dispatcher, dispatcher_stopped) = tokio::sync::oneshot::channel::<()>();
    let dispatcher_pool = pool.clone();
    let dispatcher_task = tokio::spawn(async move {
        dispatcher
            .run(&dispatcher_pool, async {
                let _ = dispatcher_stopped.await;
            })
            .await;
    });

    // `Config` guarantees `resend` is set exactly when `EMAIL_DRIVER=resend`.
    let email_sender: kernel::SharedEmailSender = match &config.resend {
        Some(resend) => std::sync::Arc::new(kernel::email::ResendEmailSender::new(
            resend.api_key.clone(),
            resend.from.clone(),
        )),
        None => std::sync::Arc::new(kernel::email::LogEmailSender),
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

    // Background jobs: modules register handlers and schedules here as they are implemented.
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
    identity::cleanup::register(identity.clone(), &mut worker, &mut scheduler)
        .map_err(RunError::Schedule)?;
    access_engine.register(&mut worker, pool.clone());
    items::purge::register(items.clone(), clock.clone(), &mut worker, &mut scheduler)
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
    let idempotency = idempotency::Idempotency::new(pool.clone(), identity.clone(), clock.clone());
    let limits = identity::rate_limit::RateLimits {
        trust_proxy_headers: config.trust_proxy_headers,
        ..identity::rate_limit::RateLimits::default()
    };
    let profile = identity::http::profile_router(identity.clone());
    let ws = realtime::router(realtime::RealtimeState {
        hub,
        identity: identity.clone(),
        items: realtime::ItemViewCheck::new(access_engine.permissions(), pool.clone()),
        clock: clock.clone(),
        config: realtime::RealtimeConfig::new(config.cors_origins.clone()),
    });
    let mut modules = identity::http::rate_limited_router(identity.clone(), limits)
        .merge(idempotency::protect(
            profile
                .merge(items::http::router(items::http::ItemsHttpState {
                    items: items.clone(),
                    identity: identity.clone(),
                }))
                .merge(notebooks::http::router(
                    notebooks::http::NotebooksHttpState {
                        notebooks: notebooks::NotebooksService::new(
                            pool.clone(),
                            clock.clone(),
                            access_engine.clone(),
                            items.clone(),
                        ),
                        identity: identity.clone(),
                    },
                ))
                .merge(goals::http::router(goals::http::GoalsHttpState {
                    goals: goals::GoalsService::new(
                        pool.clone(),
                        clock.clone(),
                        access_engine.permissions(),
                        items.clone(),
                    )
                    .with_progress(
                        "task",
                        std::sync::Arc::new(tasks::TaskProgress::new(pool.clone())),
                    )
                    .with_progress(
                        "routine",
                        std::sync::Arc::new(routines::RoutineProgress::new(pool.clone())),
                    )
                    .with_progress(
                        "tracker",
                        std::sync::Arc::new(tracking::TrackerProgress::new(pool.clone())),
                    ),
                    identity: identity.clone(),
                }))
                .merge(routines::http::router(routines::http::RoutinesHttpState {
                    routines: routines::RoutinesService::new(
                        pool.clone(),
                        clock.clone(),
                        access_engine.permissions(),
                        items.clone(),
                    ),
                    identity: identity.clone(),
                }))
                .merge(tasks::http::router(tasks::http::TasksHttpState {
                    tasks: tasks::TasksService::new(
                        pool.clone(),
                        clock.clone(),
                        access_engine.permissions(),
                        items.clone(),
                    ),
                    identity: identity.clone(),
                }))
                .merge(tracking::http::router(tracking::http::TrackingHttpState {
                    entries: tracking::TrackerEntries::new(
                        pool.clone(),
                        clock.clone(),
                        access_engine.permissions(),
                        items,
                        identity.clone(),
                    ),
                    identity: identity.clone(),
                })),
            idempotency,
        ))
        .merge(ws)
        .merge(access::http::router(access::http::AccessHttpState {
            permissions: access_engine.permissions(),
            identity: identity.clone(),
            pool: pool.clone(),
        }));
    // Minimum supported client versions (T4.2.7): public, read before sign-in.
    modules = modules.merge(client_policy::router(config.client_policy.clone()));
    if let Some(dir) = &config.updates_dir {
        // Mobile OTA updates (T4.2.6, ADR-0023): files only, no auth (public JS bundles).
        modules = modules.merge(updates::router(updates::UpdatesConfig {
            dir: dir.clone(),
            public_url: config.public_api_url.clone(),
        }));
    }
    if config.collab_spike_enabled {
        // Spike T0.6.2: no page permissions or persistence yet (E2.5).
        tracing::warn!("collab spike enabled: /collab/{{page_id}} has no page permissions");
        modules = modules.merge(notebooks::collab::router(notebooks::collab::CollabState {
            collab: notebooks::collab::Collab::new(),
            identity,
            allowed_origins: config.cors_origins.clone(),
        }));
    }
    let result = server::serve(
        listener,
        http::router_with(modules, &config.cors_origins),
        shutdown,
        config.shutdown_timeout,
    )
    .await
    .map_err(RunError::Serve);

    // Let background tasks finish their current batch before closing the pool.
    bridge.stop().await;
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
    /// The realtime LISTEN/NOTIFY bridge could not start listening.
    Realtime(db::DbError),
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
            Self::Realtime(err) => write!(f, "realtime bridge failed to start: {err}"),
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
            Self::Realtime(err) => Some(err),
            Self::Schedule(err) => Some(err),
            Self::Serve(err) => Some(err),
        }
    }
}
