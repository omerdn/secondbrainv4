-- Transactional outbox (ARCHITECTURE §4.5, T0.2.5).
-- Rows are inserted in the same transaction as the domain change and delivered to subscribers
-- by the dispatcher (db::outbox). A row is pending while dispatched_at IS NULL.
CREATE TABLE outbox (
    id            uuid        PRIMARY KEY,           -- UUIDv7: creation order
    event_type    text        NOT NULL,
    payload       jsonb       NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    available_at  timestamptz NOT NULL DEFAULT now(), -- not before (retry backoff)
    attempts      integer     NOT NULL DEFAULT 0,     -- failed delivery attempts
    last_error    text,                               -- redacted, see kernel::redact
    dispatched_at timestamptz
);

-- The dispatcher only scans pending rows.
CREATE INDEX outbox_pending_idx ON outbox (id) WHERE dispatched_at IS NULL;
