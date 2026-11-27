-- Notification center (PRD BL1–BL3, ARCHITECTURE §5.10, §14, T2.8.1). Owned by the
-- `notifications` crate. Additive only.
--
-- A notification is made from one outbox event for one user: `(source_event_id, user_id)` is
-- unique, so a redelivered event never makes a second one. The payload holds what the center
-- shows (ids and a few names), written when the notification is made.
-- `notification_prefs` holds the channels a user chose per type (BL3); without a row the
-- type's defaults apply.

CREATE TABLE notifications (
    id              uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    user_id         uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    type            text        NOT NULL CHECK (type IN (
                        'invitation', 'assignment', 'routine_reminder', 'nudge', 'reaction',
                        'sharing_change', 'forgotten_reminder', 'weekly_summary')),
    payload         jsonb       NOT NULL,
    source_event_id uuid        NOT NULL,
    read_at         timestamptz,
    created_at      timestamptz NOT NULL,
    CONSTRAINT notifications_source_unique UNIQUE (source_event_id, user_id)
);

-- The center: a user's notifications, newest first; and the unread count.
CREATE INDEX notifications_user_created_idx ON notifications (user_id, created_at DESC, id DESC);
CREATE INDEX notifications_user_unread_idx ON notifications (user_id) WHERE read_at IS NULL;

CREATE TABLE notification_prefs (
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    type       text        NOT NULL CHECK (type IN (
                   'invitation', 'assignment', 'routine_reminder', 'nudge', 'reaction',
                   'sharing_change', 'forgotten_reminder', 'weekly_summary')),
    channels   text[]      NOT NULL CHECK (channels <@ ARRAY['in_app', 'push', 'email']::text[]),
    updated_at timestamptz NOT NULL,
    PRIMARY KEY (user_id, type)
);
