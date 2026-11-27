-- Tracker entries become activity (PRD AK4, A1, T3.5.4). Owned by the `items` crate. Widens the
-- allowed types only: older servers never write the new one.

ALTER TABLE activity_events DROP CONSTRAINT IF EXISTS activity_events_type_check;
ALTER TABLE activity_events ADD CONSTRAINT activity_events_type_check CHECK (
    type IN ('created', 'completed', 'assigned', 'shared', 'linked', 'unlinked', 'recorded')
);
