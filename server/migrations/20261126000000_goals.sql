-- Goals and link weights (ARCHITECTURE §5.7, PRD D1-D3, T2.3.1). Owned by the `goals` crate.
-- Additive only.
--
-- `goals` is the detail table of a `goal` item (ARCHITECTURE §5.3) and follows the item's lifetime
-- through the foreign key. Whether an item is linked to a goal lives in `item_links` (owned by
-- `items`, B1); `goal_links` only keeps the weight of a link (D3), so a link without a row here
-- weighs 1. `goals` never reads `item_links`: it asks `ItemsService`. Timestamps come from the
-- application clock.

CREATE TABLE goals (
    item_id    uuid        PRIMARY KEY REFERENCES items (id) ON DELETE CASCADE,
    start_date date        NOT NULL,
    end_date   date        NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT goals_dates_check CHECK (start_date <= end_date)
);

CREATE TABLE goal_links (
    goal_id    uuid          NOT NULL REFERENCES goals (item_id) ON DELETE CASCADE,
    item_id    uuid          NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    weight     numeric(6, 2) NOT NULL DEFAULT 1 CHECK (weight > 0 AND weight <= 100),
    updated_at timestamptz   NOT NULL,
    PRIMARY KEY (goal_id, item_id)
);
