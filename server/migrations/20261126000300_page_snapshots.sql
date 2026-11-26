-- Page snapshots (ARCHITECTURE §5.8, §9, PRD Y3, T2.5.3). Owned by the `notebooks` crate.
-- Additive only.
--
-- Compaction folds a page's stored updates into a new snapshot and deletes exactly the updates
-- it folded, in one transaction. A page is the latest snapshot plus every update still stored;
-- applying an update twice changes nothing (Yjs), so a late commit is never lost. `plain_text`
-- feeds search (ARCHITECTURE §12). Snapshots are the version history (T2.5.4).

CREATE TABLE page_snapshots (
    page_id    uuid        NOT NULL REFERENCES pages (item_id) ON DELETE CASCADE,
    version    integer     NOT NULL CHECK (version > 0),
    state      bytea       NOT NULL,
    plain_text text        NOT NULL,
    created_at timestamptz NOT NULL,
    PRIMARY KEY (page_id, version)
);
