-- Notebooks and pages (ARCHITECTURE §5.8, PRD Y1, Y2, Y5, T2.5.1). Owned by the `notebooks`
-- crate. Additive only.
--
-- Both are detail tables of items (ARCHITECTURE §5.3) and follow the item's lifetime through the
-- foreign key. The owner lives on the item: a page's item is owned by its notebook's owner (Y5).
-- Access to a page comes from its notebook through the `in_notebook` relation in `access`. The
-- page title is the item title. Timestamps come from the application clock.

CREATE TABLE notebooks (
    item_id     uuid        PRIMARY KEY REFERENCES items (id) ON DELETE CASCADE,
    -- Set only on a user's default notebook, so each user has at most one (T2.5.1).
    default_for uuid        UNIQUE,
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL
);

CREATE TABLE pages (
    item_id            uuid        PRIMARY KEY REFERENCES items (id) ON DELETE CASCADE,
    notebook_id        uuid        NOT NULL REFERENCES notebooks (item_id) ON DELETE CASCADE,
    -- Set when this is the process page of an item (ARCHITECTURE §9, T2.5.5).
    process_of_item_id uuid        REFERENCES items (id) ON DELETE SET NULL,
    created_at         timestamptz NOT NULL,
    updated_at         timestamptz NOT NULL
);

CREATE INDEX pages_notebook_idx ON pages (notebook_id, created_at);
