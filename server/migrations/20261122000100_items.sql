-- Items: the shared base of tasks, routines, goals, trackers, notebooks, pages and module objects
-- (ARCHITECTURE §5.3, PRD N1–N4, T1.2.1). Owned by the `items` crate; other crates never read
-- these tables with SQL (ADR-0007). Who may see or change an item lives in `access`.
--
-- No foreign keys to users or teams (owned by other modules). Timestamps come from the
-- application clock.

CREATE TABLE items (
    id              uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    kind            text        NOT NULL CHECK (
        kind IN ('task', 'routine', 'goal', 'tracker', 'notebook', 'page')
        OR kind ~ '^module:[a-z0-9_-]{1,40}$'
    ),
    owner_user_id   uuid,
    owner_team_id   uuid,
    title           text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 500),
    description     text        CHECK (char_length(description) <= 20000),
    status          text,                   -- kind-specific, set by the owning module
    team_visibility text        CHECK (team_visibility IN ('all', 'assignees')),  -- V12
    created_by      uuid        NOT NULL,
    created_at      timestamptz NOT NULL,
    updated_at      timestamptz NOT NULL,
    last_touched_at timestamptz NOT NULL,   -- H4 "forgotten items" (T1.2.6)
    archived_at     timestamptz,
    deleted_at      timestamptz,            -- in the trash since; purged after 30 days (N4)
    -- N1: exactly one owner, a person or a team.
    CONSTRAINT items_owner_check CHECK ((owner_user_id IS NULL) <> (owner_team_id IS NULL)),
    -- The visibility mode exists exactly for team items.
    CONSTRAINT items_team_mode_check CHECK ((owner_team_id IS NULL) = (team_visibility IS NULL))
);

-- The daily purge looks for items whose trash time has run out.
CREATE INDEX items_deleted_at_idx ON items (deleted_at) WHERE deleted_at IS NOT NULL;
