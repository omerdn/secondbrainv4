-- Tags and their attachment to items (ARCHITECTURE §5.3, PRD E1, E2, E4, T1.2.2). Owned by the
-- `items` crate. Additive only. Team tags (owner_team_id) are used from F5 on.
--
-- No foreign keys to users or teams (owned by other modules). Timestamps come from the
-- application clock.

CREATE TABLE tags (
    id            uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    owner_user_id uuid,
    owner_team_id uuid,
    name          text        NOT NULL CHECK (btrim(name) <> '' AND char_length(name) <= 50),
    color         text        CHECK (color ~ '^#[0-9a-f]{6}$'),
    created_at    timestamptz NOT NULL,
    updated_at    timestamptz NOT NULL,
    -- E1: exactly one owner, a person or a team.
    CONSTRAINT tags_owner_check CHECK ((owner_user_id IS NULL) <> (owner_team_id IS NULL))
);

CREATE UNIQUE INDEX tags_user_name_idx ON tags (owner_user_id, lower(name))
    WHERE owner_user_id IS NOT NULL;
CREATE UNIQUE INDEX tags_team_name_idx ON tags (owner_team_id, lower(name))
    WHERE owner_team_id IS NOT NULL;

CREATE TABLE item_tags (
    item_id    uuid        NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    tag_id     uuid        NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    tagged_by  uuid        NOT NULL,
    created_at timestamptz NOT NULL,
    PRIMARY KEY (item_id, tag_id)
);

-- "Items with this tag" (E4).
CREATE INDEX item_tags_tag_id_idx ON item_tags (tag_id);
