-- Friendships and blocks (PRD AK1, AK2, V1, ARCHITECTURE §5.1, T3.1.1). Owned by the `social`
-- crate. Additive only.
--
-- A request is addressed by e-mail. `to_user` is set when the address belongs to someone; the
-- request reaches them only if it is not `silenced` (they blocked the sender). An unknown
-- address and a silenced request look to the sender exactly like a pending one, so neither the
-- existence of an account nor a block leaks (AK2). Friendship is one row per pair, `user_a`
-- before `user_b`.

CREATE TABLE friend_requests (
    id           uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    from_user    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    to_email     text        NOT NULL CHECK (to_email <> '' AND to_email = lower(btrim(to_email))),
    to_user      uuid        REFERENCES users (id) ON DELETE CASCADE,
    silenced     boolean     NOT NULL DEFAULT false,
    status       text        NOT NULL CHECK (status IN ('pending', 'accepted', 'declined', 'cancelled')),
    created_at   timestamptz NOT NULL,
    responded_at timestamptz,
    CHECK ((status = 'pending') = (responded_at IS NULL))
);

-- One open request from a sender to an address.
CREATE UNIQUE INDEX friend_requests_pending_key ON friend_requests (from_user, to_email)
    WHERE status = 'pending';
-- The sender's recent requests (rate limit) and a recipient's incoming ones.
CREATE INDEX friend_requests_from_idx ON friend_requests (from_user, created_at DESC);
CREATE INDEX friend_requests_to_idx ON friend_requests (to_user, created_at DESC)
    WHERE status = 'pending' AND NOT silenced;

CREATE TABLE friendships (
    user_a     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    user_b     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    PRIMARY KEY (user_a, user_b),
    CHECK (user_a < user_b)
);
CREATE INDEX friendships_user_b_idx ON friendships (user_b);

CREATE TABLE blocks (
    blocker    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    blocked    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    PRIMARY KEY (blocker, blocked),
    CHECK (blocker <> blocked)
);
CREATE INDEX blocks_blocked_idx ON blocks (blocked);
