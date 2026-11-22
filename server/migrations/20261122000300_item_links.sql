-- Links between items (ARCHITECTURE §5.3, PRD B1-B3, T1.2.3). Owned by the `items` crate.
-- Additive only. Access that flows through a page link lives in `access_relations`; this table
-- records the link itself. Text mentions (B3) are never stored here.

CREATE TABLE item_links (
    id             uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    source_item_id uuid        NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    target_item_id uuid        NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    created_by     uuid        NOT NULL,
    created_at     timestamptz NOT NULL,
    CONSTRAINT item_links_no_self_link CHECK (source_item_id <> target_item_id),
    CONSTRAINT item_links_unique UNIQUE (source_item_id, target_item_id)
);

-- Links arriving at an item.
CREATE INDEX item_links_target_idx ON item_links (target_item_id);
