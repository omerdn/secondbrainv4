-- Page edits (ARCHITECTURE §9, PRD Y3, T2.5.2). Owned by the `notebooks` crate. Additive only.
--
-- Every Yjs update a client sends is stored here, append-only, before it is applied in memory
-- and passed on to the other clients, so nothing that was acknowledged is lost when the server
-- restarts (Y3). A room is rebuilt by applying the rows in order; compaction into snapshots comes
-- with T2.5.3. Timestamps come from the application clock.

CREATE TABLE page_updates (
    id         bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    page_id    uuid        NOT NULL REFERENCES pages (item_id) ON DELETE CASCADE,
    update     bytea       NOT NULL CHECK (octet_length(update) > 0),
    created_by uuid        NOT NULL,
    created_at timestamptz NOT NULL
);

CREATE INDEX page_updates_page_idx ON page_updates (page_id, id);
