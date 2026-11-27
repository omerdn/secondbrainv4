-- Web Push subscriptions (PRD BL1, BL3, ARCHITECTURE §14, T4.2.4). Owned by the
-- `notifications` crate. Additive only.
--
-- One row per browser subscription (`PushSubscription` of the Push API). The endpoint is a
-- capability URL of the browser's push service and is unique: the same browser signed in as
-- someone else moves the row to that user. `p256dh` and `auth` are the browser's keys
-- (base64url) that encrypt each message (RFC 8291).

CREATE TABLE push_subscriptions (
    id         uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    endpoint   text        NOT NULL UNIQUE CHECK (length(endpoint) <= 2048),
    p256dh     text        NOT NULL,
    auth       text        NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

CREATE INDEX push_subscriptions_user_idx ON push_subscriptions (user_id);
