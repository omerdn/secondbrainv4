-- Nudges (PRD AK3, AK2, T3.5.1). Owned by the `social` crate. Additive only.
--
-- One row per sender, recipient and the sender's local day: the primary key is the "once a day"
-- rule, so two nudges sent at once cannot both pass.

CREATE TABLE nudges (
    from_user  uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    to_user    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    local_date date        NOT NULL,
    created_at timestamptz NOT NULL,
    PRIMARY KEY (from_user, to_user, local_date),
    CHECK (from_user <> to_user)
);
