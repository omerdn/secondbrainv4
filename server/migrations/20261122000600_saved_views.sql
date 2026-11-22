-- Saved views (ARCHITECTURE §5.3, PRD F2, T1.2.7). Owned by the `items` crate. Additive only.
-- A view belongs to one person. No foreign key to users (owned by another module).

CREATE TABLE saved_views (
    id         uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    user_id    uuid        NOT NULL,
    name       text        NOT NULL CHECK (btrim(name) <> '' AND char_length(name) <= 60),
    filter     jsonb       NOT NULL,
    pinned     boolean     NOT NULL DEFAULT false,
    position   integer     NOT NULL DEFAULT 0 CHECK (position >= 0),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

CREATE UNIQUE INDEX saved_views_user_name_idx ON saved_views (user_id, lower(name));
