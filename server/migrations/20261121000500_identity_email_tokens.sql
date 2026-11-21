-- E-mail verification and password reset (T0.3.3, ARCHITECTURE §15). Owned by `identity`.

-- When the current address was verified. Changing the address must reset it.
ALTER TABLE users ADD COLUMN email_verified_at timestamptz;

-- Single-use tokens sent by e-mail. As with sessions, only the SHA-256 digest of the 256-bit
-- token is stored.
CREATE TABLE email_tokens (
    id         uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    purpose    text        NOT NULL CHECK (purpose IN ('verify_email', 'reset_password')),
    token_hash bytea       NOT NULL CHECK (octet_length(token_hash) = 32),
    email      text        NOT NULL,  -- address the token was sent to; must still match on use
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    used_at    timestamptz,
    CONSTRAINT email_tokens_token_hash_key UNIQUE (token_hash),
    CONSTRAINT email_tokens_expiry_check CHECK (expires_at > created_at)
);

CREATE INDEX email_tokens_user_purpose_idx ON email_tokens (user_id, purpose);
CREATE INDEX email_tokens_expires_at_idx ON email_tokens (expires_at);
