-- Shared work sessions (PRD W6, ADR-0015, ARCHITECTURE §11, T3.6.1). Owned by the `sessions`
-- crate. Additive only.
--
-- The timer stays on `work_sessions` and is the same for everyone. Each participant keeps their
-- own item (`work_session_participants.item_id`) and their own segments. A segment remembers
-- the timer's active time when it opened (`base_ms`), so someone who joins a running timer counts
-- from then on; personal sessions always start at 0.

ALTER TABLE work_session_participants
    ADD COLUMN item_id uuid REFERENCES items (id) ON DELETE SET NULL;

ALTER TABLE work_session_segments
    ADD COLUMN base_ms bigint NOT NULL DEFAULT 0 CHECK (base_ms >= 0);

-- Only an invited friend can join (W6, V1).
CREATE TABLE work_session_invitations (
    session_id uuid        NOT NULL REFERENCES work_sessions (id) ON DELETE CASCADE,
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    invited_by uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    PRIMARY KEY (session_id, user_id)
);
CREATE INDEX work_session_invitations_user_idx ON work_session_invitations (user_id);
CREATE INDEX work_session_participants_user_idx ON work_session_participants (user_id)
    WHERE left_at IS NULL;
