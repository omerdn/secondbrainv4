-- Process pages (PRD S1, S2, T3, ARCHITECTURE §5.8, §9, T2.5.5). Owned by the `notebooks` crate.
-- Additive only.
--
-- An item has at most one process page; concurrent first notes create exactly one.

CREATE UNIQUE INDEX pages_process_of_idx ON pages (process_of_item_id)
    WHERE process_of_item_id IS NOT NULL;
