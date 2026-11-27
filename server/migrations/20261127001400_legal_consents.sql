-- Legal consent (PRD HS5, HS1, ARCHITECTURE §15, T4.3.3). Owned by the `identity` crate.
-- Additive only.
--
-- `legal_consents`: which version of which text a user accepted, and when. A sign-in needs the
-- current version of every text; a new version asks again at the next sign-in.
-- `consent_tickets`: a sign-in held for consent (password, Google or Apple). The token is
-- hashed; a provider sign-in of someone without an account keeps the provider's facts here
-- until they accept, so no account exists before consent. Tickets live 15 minutes and are
-- removed by the daily cleanup.

CREATE TABLE legal_consents (
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    document    text        NOT NULL CHECK (document IN ('privacy', 'kvkk', 'terms')),
    version     integer     NOT NULL CHECK (version > 0),
    accepted_at timestamptz NOT NULL,
    PRIMARY KEY (user_id, document, version)
);

CREATE TABLE consent_tickets (
    token_hash bytea       PRIMARY KEY CHECK (length(token_hash) = 32),
    user_id    uuid        REFERENCES users (id) ON DELETE CASCADE,
    provider   text        CHECK (provider IN ('google', 'apple')),
    claims     jsonb,
    device     text,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    -- A known account, or a provider sign-in waiting to become one.
    CONSTRAINT consent_tickets_subject_check CHECK (
        (user_id IS NOT NULL) <> (provider IS NOT NULL AND claims IS NOT NULL)
    )
);

CREATE INDEX consent_tickets_expires_idx ON consent_tickets (expires_at);
