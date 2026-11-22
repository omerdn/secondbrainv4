-- Access: relation tuples and the derived access cache (ARCHITECTURE §5.11, §6.1, §6.2, ADR-0004,
-- PRD V8, T1.1.1). Owned by the `access` crate; other crates never read or write these tables with
-- SQL (ADR-0007) and go through `access`'s API (grant/revoke/link/unlink, PermissionService).
--
-- No foreign keys: objects and subjects live in other modules' tables (items, teams, users).
-- Cleanup on deletion is driven by domain events, not by cascades.

-- Zanzibar-style tuples: "<object_type>:<object_id> <relation> <subject_type>:<subject_id>".
-- The single source of truth for everything that affects access (§6.1).
CREATE TABLE access_relations (
    object_type  text        NOT NULL CHECK (object_type IN ('item', 'team')),
    object_id    uuid        NOT NULL,
    relation     text        NOT NULL,
    subject_type text        NOT NULL CHECK (subject_type IN ('user', 'team', 'item')),
    subject_id   uuid        NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (object_type, object_id, relation, subject_type, subject_id),
    -- Each relation is only valid between specific object and subject types (mirrors
    -- access::Relation::{object_type, subject_type}).
    CONSTRAINT access_relations_shape_check CHECK (
        CASE relation
            -- ownership
            WHEN 'owner'       THEN object_type = 'item' AND subject_type = 'user'
            WHEN 'owned_by'    THEN object_type = 'item' AND subject_type = 'team'
            -- item membership
            WHEN 'assignee'    THEN object_type = 'item' AND subject_type = 'user'
            WHEN 'watcher'     THEN object_type = 'item' AND subject_type = 'user'
            -- team roles
            WHEN 'admin'       THEN object_type = 'team' AND subject_type = 'user'
            WHEN 'manager'     THEN object_type = 'team' AND subject_type = 'user'
            WHEN 'member'      THEN object_type = 'team' AND subject_type = 'user'
            WHEN 'guest'       THEN object_type = 'team' AND subject_type = 'user'
            -- notebooks: page -> notebook, notebook membership
            WHEN 'in_notebook' THEN object_type = 'item' AND subject_type = 'item'
            WHEN 'editor'      THEN object_type = 'item' AND subject_type = 'user'
            WHEN 'viewer'      THEN object_type = 'item' AND subject_type = 'user'
            -- links: page -> item
            WHEN 'linked_to'   THEN object_type = 'item' AND subject_type = 'item'
            ELSE false
        END
    )
);

-- Reverse lookups for the affected-set computation (§6.4): "what does this user own / which teams
-- is she in", "which pages link to item T1", "which pages are in notebook N1".
CREATE INDEX access_relations_subject_idx
    ON access_relations (subject_type, subject_id, relation);

-- Derived access cache (§6.2): one row per (item, user) with any permission. Rewritten by the
-- access engine whenever relations change (V8); listing, filtering and search JOIN on it.
--   perms:   bitmask of access::Perms (VIEW=1, EDIT=2, SHARE=4, LINK=8, MANAGE=16, DELETE=32).
--   reasons: why the user has access, for "Why am I seeing this?" (V11).
CREATE TABLE item_access (
    item_id    uuid        NOT NULL,
    user_id    uuid        NOT NULL,
    perms      integer     NOT NULL CHECK (perms BETWEEN 1 AND 63),
    reasons    jsonb       NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(reasons) = 'array'),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (item_id, user_id)
);

-- "Items this user can access" (list/filter/search JOINs on user_id = $me).
CREATE INDEX item_access_user_idx ON item_access (user_id, item_id);
