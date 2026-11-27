-- Emoji reactions to friends' completions (PRD AK4, T3.5.2). Owned by the `items` crate, next to
-- the activity log it reacts to. Additive only.
--
-- One reaction per person and completion event; reacting again replaces the emoji.

CREATE TABLE activity_reactions (
    activity_id uuid        NOT NULL REFERENCES activity_events (id) ON DELETE CASCADE,
    user_id     uuid        NOT NULL,
    emoji       text        NOT NULL,
    created_at  timestamptz NOT NULL,
    PRIMARY KEY (activity_id, user_id)
);
