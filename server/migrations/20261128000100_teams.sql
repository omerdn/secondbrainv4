-- Teams and membership (PRD TK1, TK2, V1, ARCHITECTURE §5.2, T5.1.1). Owned by the `teams`
-- crate. Additive only.
--
-- A member row is `invited` until the person accepts (TK2); declining deletes it. Only active
-- members hold a team role, which is also written as an `access` relation (team → user) so team
-- items (T5.1.3) can be computed.

CREATE TABLE teams (
    id         uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    name       text        NOT NULL CHECK (btrim(name) <> '' AND char_length(name) <= 80),
    color      text        NOT NULL CHECK (color ~ '^#[0-9a-f]{6}$'),
    logo_url   text        CHECK (logo_url ~ '^https://' AND char_length(logo_url) <= 2048),
    created_by uuid        NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

CREATE TABLE team_members (
    team_id    uuid        NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role       text        NOT NULL CHECK (role IN ('admin', 'manager', 'member', 'guest')),
    status     text        NOT NULL CHECK (status IN ('invited', 'active')),
    invited_by uuid,
    invited_at timestamptz NOT NULL,
    joined_at  timestamptz,
    PRIMARY KEY (team_id, user_id),
    CONSTRAINT team_members_joined_check CHECK ((status = 'active') = (joined_at IS NOT NULL))
);

-- "My teams" and "my team invitations".
CREATE INDEX team_members_user_idx ON team_members (user_id, status);
