-- Pending browser OAuth sign-ins (T0.3.4, ADR-0016). Owned by `identity`.
-- A row lives from the redirect to the provider until the callback (at most a few minutes) and
-- is deleted when used. Only the SHA-256 digest of the `state` value is stored; the PKCE
-- verifier is a one-time secret that is useless without the provider's authorization code.
CREATE TABLE oauth_states (
    state_hash    bytea       PRIMARY KEY CHECK (octet_length(state_hash) = 32),
    provider      text        NOT NULL CHECK (provider IN ('google', 'apple')),
    pkce_verifier text        NOT NULL,
    created_at    timestamptz NOT NULL,
    expires_at    timestamptz NOT NULL
);

CREATE INDEX oauth_states_expires_at_idx ON oauth_states (expires_at);
