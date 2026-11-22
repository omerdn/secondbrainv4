-- Invitations (ARCHITECTURE §5.3, §6.1, PRD G5, V1, T1.2.4). Owned by the `items` crate.
-- Additive only. An invitation creates no access relation until it is accepted.
--
-- `object_id` is an item or, from F5 on, a team, so it has no foreign key; the trash purge
-- deletes an item's invitations. No foreign keys to users (owned by another module).

CREATE TABLE invitations (
    id           uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    kind         text        NOT NULL CHECK (kind IN ('item_member', 'team_member')),
    object_id    uuid        NOT NULL,
    from_user    uuid        NOT NULL,
    to_user      uuid        NOT NULL,
    role         text        NOT NULL CHECK (role IN ('assignee', 'watcher', 'editor', 'viewer')),
    status       text        NOT NULL DEFAULT 'pending'
                             CHECK (status IN ('pending', 'accepted', 'declined', 'expired')),
    created_at   timestamptz NOT NULL,
    responded_at timestamptz,
    CONSTRAINT invitations_not_to_self CHECK (from_user <> to_user)
);

-- One open invitation per person and object.
CREATE UNIQUE INDEX invitations_one_pending_idx ON invitations (kind, object_id, to_user)
    WHERE status = 'pending';
-- "My pending invitations".
CREATE INDEX invitations_inbox_idx ON invitations (to_user, created_at DESC)
    WHERE status = 'pending';
-- The trash purge.
CREATE INDEX invitations_object_idx ON invitations (kind, object_id);
