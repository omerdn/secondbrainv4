-- Shared routine rotation and swaps (PRD R6, ARCHITECTURE §5.6, §8, T3.3.2). Owned by the
-- `routines` crate. Additive only: every existing routine keeps fixed assignees (rotation NULL).
--
-- The responsible person is computed, never stored (ADR-0008): `routine_rotation` only keeps the
-- order people take turns in and `routine_rotation_overrides` the swapped dates. A stored position
-- of someone who is no longer a member is ignored when the rotation is read. A swap remembers the
-- turn order it was made under (`turn_order`) and counts only while that order holds: when someone
-- joins or leaves, every turn shifts, so the old swaps no longer mean what was agreed.

ALTER TABLE routines
    ADD COLUMN rotation text CHECK (rotation IN ('every_occurrence', 'weekly'));

CREATE TABLE routine_rotation (
    routine_id uuid    NOT NULL REFERENCES routines (item_id) ON DELETE CASCADE,
    user_id    uuid    NOT NULL,
    position   integer NOT NULL CHECK (position >= 0),
    PRIMARY KEY (routine_id, user_id),
    CONSTRAINT routine_rotation_position_unique UNIQUE (routine_id, position)
);

CREATE TABLE routine_rotation_overrides (
    routine_id      uuid NOT NULL REFERENCES routines (item_id) ON DELETE CASCADE,
    occurrence_date date NOT NULL,
    user_id         uuid NOT NULL,
    turn_order      uuid[] NOT NULL,
    PRIMARY KEY (routine_id, occurrence_date)
);
