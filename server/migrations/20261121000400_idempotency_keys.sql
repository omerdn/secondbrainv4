-- Server-side idempotency keys (ARCHITECTURE §10 "Yazma işlemleri", ADR-0003, T0.2.8).
-- Owned by the `app` crate (app::idempotency). One row per (user, client-generated key): the
-- first request with a key is executed, its response stored, and repeats get that response.
CREATE TABLE idempotency_keys (
    user_id       uuid        NOT NULL,  -- signed-in user (no FK: owned by another module)
    key           text        NOT NULL CHECK (char_length(key) BETWEEN 1 AND 255),
    request_hash  bytea       NOT NULL CHECK (octet_length(request_hash) = 32), -- SHA-256 of method, path, body
    status        smallint,              -- NULL while the first request is still running
    content_type  text,
    response_body bytea,
    created_at    timestamptz NOT NULL,  -- start of the current attempt (lease start)
    completed_at  timestamptz,
    PRIMARY KEY (user_id, key),
    CONSTRAINT idempotency_keys_completed_check CHECK ((status IS NULL) = (completed_at IS NULL))
);

-- Periodic cleanup deletes rows by age.
CREATE INDEX idempotency_keys_created_at_idx ON idempotency_keys (created_at);
