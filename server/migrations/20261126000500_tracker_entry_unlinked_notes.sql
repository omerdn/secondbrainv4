-- Entries whose note has not reached a process page yet (T3, T2.5.5). Owned by the `tracking`
-- crate. Additive only. Feeds the sweep that moves stored notes to process pages.

CREATE INDEX tracker_entries_unlinked_notes_idx ON tracker_entries (created_at, id)
    WHERE note IS NOT NULL AND note_block_id IS NULL AND deleted_at IS NULL;
