-- The item-level part of a team's role × permission matrix (PRD TK3, V8, T5.1.2). Owned by the
-- `access` crate: it is an input of the access computation. Additive only. A team without a
-- row uses the built-in defaults (`access::RolePolicy::default`). Values are `Perms` bits.

CREATE TABLE team_role_policies (
    team_id    uuid        PRIMARY KEY,
    admin      smallint    NOT NULL CHECK (admin BETWEEN 0 AND 63),
    manager    smallint    NOT NULL CHECK (manager BETWEEN 0 AND 63),
    member     smallint    NOT NULL CHECK (member BETWEEN 0 AND 63),
    guest      smallint    NOT NULL CHECK (guest BETWEEN 0 AND 63),
    updated_at timestamptz NOT NULL DEFAULT now()
);
