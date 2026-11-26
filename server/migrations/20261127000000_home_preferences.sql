-- Today screen preferences (PRD H4, T2.7.1). Owned by the `today` crate. Additive only.
--
-- `forgotten_days`: an active item untouched for this many days is "forgotten"; 7 when the
-- user has no row.

CREATE TABLE home_preferences (
    user_id        uuid        PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    forgotten_days integer     NOT NULL CHECK (forgotten_days BETWEEN 1 AND 365),
    updated_at     timestamptz NOT NULL
);
