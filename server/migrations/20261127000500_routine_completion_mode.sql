-- Shared routine completion modes (PRD R5, ARCHITECTURE §5.3, T3.3.1). Owned by the `routines`
-- crate. Additive only: every existing routine is a shared pool, which is what a personal routine
-- already was.

ALTER TABLE routines
    ADD COLUMN completion_mode text NOT NULL DEFAULT 'pool'
        CHECK (completion_mode IN ('pool', 'individual'));
