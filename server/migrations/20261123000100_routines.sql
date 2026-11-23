-- Routines and plan versions (ARCHITECTURE §5.6, PRD R1-R3, R11, R13, T2.2.1). Owned by the
-- `routines` crate. Additive only.
--
-- `routines` is the detail table of a `routine` item (ARCHITECTURE §5.3), so it follows the item's
-- lifetime through the foreign key: `routines` never reads or writes `items`. Occurrences are
-- never stored (ADR-0008). The daily target lives on the plan version, so changing it leaves past
-- days as they were (R11). Timestamps come from the application clock.

CREATE TABLE routines (
    item_id        uuid        PRIMARY KEY REFERENCES items (id) ON DELETE CASCADE,
    timezone       text        NOT NULL CHECK (btrim(timezone) <> ''),
    start_date     date        NOT NULL,
    end_date       date,
    reminder_times time[]      NOT NULL DEFAULT '{}',
    created_at     timestamptz NOT NULL,
    updated_at     timestamptz NOT NULL,
    CONSTRAINT routines_dates_check CHECK (end_date IS NULL OR start_date <= end_date)
);

CREATE TABLE routine_schedules (
    id           uuid    PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    routine_id   uuid    NOT NULL REFERENCES routines (item_id) ON DELETE CASCADE,
    rrule        text    NOT NULL CHECK (btrim(rrule) <> ''),
    daily_target integer NOT NULL DEFAULT 1 CHECK (daily_target BETWEEN 1 AND 100),
    valid_from   date    NOT NULL,
    valid_to     date,
    CONSTRAINT routine_schedules_dates_check CHECK (valid_to IS NULL OR valid_from <= valid_to),
    CONSTRAINT routine_schedules_start_unique UNIQUE (routine_id, valid_from)
);
