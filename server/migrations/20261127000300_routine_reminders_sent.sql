-- Routine reminders already sent (PRD R13, BL2, T2.2.7). Owned by the `routines` crate.
-- Additive only.
--
-- One row per reminder occurrence: a routine, a local day and a reminder time. The sweep
-- inserts the row and publishes `routine.reminder_due` in one transaction, so a reminder is
-- sent at most once however often, and on however many instances, the sweep runs.

CREATE TABLE routine_reminders_sent (
    routine_id    uuid        NOT NULL REFERENCES routines (item_id) ON DELETE CASCADE,
    local_date    date        NOT NULL,
    reminder_time time        NOT NULL,
    sent_at       timestamptz NOT NULL,
    PRIMARY KEY (routine_id, local_date, reminder_time)
);
