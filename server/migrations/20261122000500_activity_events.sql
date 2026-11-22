-- Activity log (ARCHITECTURE §5.3, PRD A1, A2, T1.2.5). Owned by the `items` crate. Additive only.
--
-- `actor_id` is NULL for changes the system made (V13). `related_item_id` is the other end of a
-- link: an event is shown only if the viewer can see both ends (A2, V9). Titles are never copied
-- here. History goes with the item when it is purged (N4). No foreign keys to users or teams.

CREATE TABLE activity_events (
    id              uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    actor_id        uuid,
    item_id         uuid        REFERENCES items (id) ON DELETE CASCADE,
    team_id         uuid,
    type            text        NOT NULL CHECK (
        type IN ('created', 'completed', 'assigned', 'shared', 'linked', 'unlinked')
    ),
    related_item_id uuid        REFERENCES items (id) ON DELETE CASCADE,
    payload         jsonb       NOT NULL DEFAULT '{}',
    created_at      timestamptz NOT NULL
);

CREATE INDEX activity_events_item_idx ON activity_events (item_id, created_at DESC, id DESC);
CREATE INDEX activity_events_feed_idx ON activity_events (created_at DESC, id DESC);
CREATE INDEX activity_events_related_idx ON activity_events (related_item_id)
    WHERE related_item_id IS NOT NULL;
