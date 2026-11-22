-- Tracking engine (ARCHITECTURE §5.5, PRD T1, T2, T1.3.1). Owned by the `tracking` crate.
-- Additive only.
--
-- `trackers` is the detail table of a `tracker` item (ARCHITECTURE §5.3), so it follows the item's
-- lifetime through the foreign key: `tracking` never reads or writes `items`. No foreign keys to
-- users (owned by another module). Timestamps come from the application clock.

CREATE TABLE trackers (
    item_id      uuid           PRIMARY KEY REFERENCES items (id) ON DELETE CASCADE,
    unit         text           NOT NULL CHECK (btrim(unit) <> '' AND char_length(unit) <= 30),
    target_value numeric(20, 4) NOT NULL CHECK (target_value > 0),
    period       text           NOT NULL CHECK (period IN ('day', 'week', 'month', 'total', 'range')),
    range_start  date,
    range_end    date,
    aggregation  text           NOT NULL CHECK (aggregation IN ('sum', 'count', 'last', 'max')),
    direction    text           NOT NULL CHECK (direction IN ('increase', 'limit')),
    owner_kind   text           NOT NULL CHECK (owner_kind IN ('standalone', 'routine', 'module')),
    created_at   timestamptz    NOT NULL,
    updated_at   timestamptz    NOT NULL,
    -- T1 "target range": the dates exist exactly for the `range` period.
    CONSTRAINT trackers_range_check CHECK (
        (period = 'range' AND range_start IS NOT NULL AND range_end IS NOT NULL
             AND range_start <= range_end)
        OR (period <> 'range' AND range_start IS NULL AND range_end IS NULL)
    )
);

CREATE TABLE tracker_entries (
    id              uuid           PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    tracker_item_id uuid           NOT NULL REFERENCES trackers (item_id) ON DELETE CASCADE,
    user_id         uuid           NOT NULL,
    value           numeric(20, 4) NOT NULL,
    occurred_at     timestamptz    NOT NULL,
    local_date      date           NOT NULL,  -- the user's calendar day when it occurred
    note_block_id   uuid,                     -- the line on the process page (T3)
    created_at      timestamptz    NOT NULL,
    deleted_at      timestamptz
);

-- Period calculations read a tracker's live entries by day.
CREATE INDEX tracker_entries_tracker_day_idx
    ON tracker_entries (tracker_item_id, local_date, occurred_at)
    WHERE deleted_at IS NULL;
