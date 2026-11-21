-- Background job queue (ARCHITECTURE §14, ADR-0009, T0.2.6). Owned by the `jobs` crate.
-- A job is due when run_at <= now(), it is neither completed nor failed, and no live lease
-- (locked_until in the future) is held on it.
CREATE TABLE jobs (
    id           uuid        PRIMARY KEY,           -- UUIDv7
    kind         text        NOT NULL,
    payload      jsonb       NOT NULL,
    run_at       timestamptz NOT NULL DEFAULT now(),
    attempts     integer     NOT NULL DEFAULT 0,     -- started executions
    max_attempts integer     NOT NULL DEFAULT 10,
    locked_until timestamptz,                        -- lease expiry while a worker runs it
    locked_by    uuid,                               -- lease token of the current run
    last_error   text,                               -- redacted, see kernel::redact
    dedupe_key   text        UNIQUE,                 -- e.g. one row per cron occurrence
    created_at   timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    failed_at    timestamptz                         -- gave up after max_attempts
);

CREATE INDEX jobs_due_idx ON jobs (run_at) WHERE completed_at IS NULL AND failed_at IS NULL;
