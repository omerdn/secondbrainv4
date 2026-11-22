-- The short note of an entry (PRD T2, T1.3.4). Additive only.
--
-- Interim home of the note: once process pages exist (E2.5), notes move to a dated line of the
-- page, referenced by `note_block_id`, which becomes their only copy (T3). This column is then
-- migrated and dropped in a later release.

ALTER TABLE tracker_entries
    ADD COLUMN note text CHECK (btrim(note) <> '' AND char_length(note) <= 500);

-- The history of a tracker, newest first.
CREATE INDEX tracker_entries_history_idx
    ON tracker_entries (tracker_item_id, occurred_at DESC, id DESC)
    WHERE deleted_at IS NULL;
