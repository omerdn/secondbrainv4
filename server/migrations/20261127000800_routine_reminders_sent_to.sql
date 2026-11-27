-- Routine reminders sent per person (PRD R13, R5, R6, T3.3.4). Owned by the `routines` crate.
-- Additive only: shared routines remind each responsible person, so a sent reminder is now one
-- row per person. `routine_reminders_sent` (one row per reminder, the owner's) is no longer
-- written; it is kept until a later contract step drops it.

CREATE TABLE routine_reminders_sent_to (
    routine_id    uuid        NOT NULL REFERENCES routines (item_id) ON DELETE CASCADE,
    local_date    date        NOT NULL,
    reminder_time time        NOT NULL,
    user_id       uuid        NOT NULL,
    sent_at       timestamptz NOT NULL,
    PRIMARY KEY (routine_id, local_date, reminder_time, user_id)
);

-- Reminders the owner already got must not go out again after the deploy.
-- copy-sent-reminders
INSERT INTO routine_reminders_sent_to (routine_id, local_date, reminder_time, user_id, sent_at)
SELECT s.routine_id, s.local_date, s.reminder_time, i.owner_user_id, s.sent_at
FROM routine_reminders_sent s
JOIN items i ON i.id = s.routine_id
WHERE i.owner_user_id IS NOT NULL;
