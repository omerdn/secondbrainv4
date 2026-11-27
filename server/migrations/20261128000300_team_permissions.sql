-- The team half of a team's role × permission matrix (PRD TK3, T5.1.2). Owned by the `teams`
-- crate. Additive only. A role without a row uses the built-in defaults; admins are never
-- stored (they have everything).

CREATE TABLE team_permissions (
    team_id     uuid   NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    role        text   NOT NULL CHECK (role IN ('manager', 'member', 'guest')),
    permissions text[] NOT NULL CHECK (permissions <@ ARRAY[
        'create_items', 'assign', 'invite_members', 'manage_tags', 'create_notebooks',
        'view_activity']::text[]),
    PRIMARY KEY (team_id, role)
);
