-- Work sessions (PRD W1–W3, ARCHITECTURE §5.9, §11, ADR-0015, T2.6.1). Owned by the `sessions`
-- crate. Additive only.
--
-- The server is the only authority on the timer: `timer_state`, `timer_started_at` (set while
-- running), `timer_accumulated_ms` (of the current segment) and `timer_planned_ms`. Clients
-- compute the remaining time themselves. A session stays open until the user closes it (W1);
-- segments are the log (W4) and only ever statistics (W5). Shared sessions come with T3.6.1.

CREATE TABLE work_sessions (
    id                   uuid        PRIMARY KEY,
    created_by           uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status               text        NOT NULL CHECK (status IN ('open', 'closed')),
    is_shared            boolean     NOT NULL DEFAULT false,
    timer_state          text        NOT NULL CHECK (timer_state IN ('running', 'paused', 'stopped')),
    timer_mode           text        CHECK (timer_mode IN ('pomodoro', 'stopwatch', 'break')),
    timer_item_id        uuid        REFERENCES items (id) ON DELETE SET NULL,
    timer_started_at     timestamptz,
    timer_accumulated_ms bigint      NOT NULL DEFAULT 0 CHECK (timer_accumulated_ms >= 0),
    timer_planned_ms     bigint      CHECK (timer_planned_ms > 0),
    pomodoro_config      jsonb       NOT NULL,
    created_at           timestamptz NOT NULL,
    closed_at            timestamptz,
    CHECK ((timer_state = 'running') = (timer_started_at IS NOT NULL)),
    CHECK ((status = 'closed') = (closed_at IS NOT NULL))
);

-- One open personal session per user (W1).
CREATE UNIQUE INDEX work_sessions_one_open_idx ON work_sessions (created_by)
    WHERE status = 'open' AND NOT is_shared;

CREATE TABLE work_session_participants (
    session_id uuid        NOT NULL REFERENCES work_sessions (id) ON DELETE CASCADE,
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    joined_at  timestamptz NOT NULL,
    left_at    timestamptz,
    PRIMARY KEY (session_id, user_id)
);

CREATE TABLE work_session_segments (
    id         uuid        PRIMARY KEY,
    session_id uuid        NOT NULL REFERENCES work_sessions (id) ON DELETE CASCADE,
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    mode       text        NOT NULL CHECK (mode IN ('pomodoro', 'stopwatch', 'break')),
    item_id    uuid        REFERENCES items (id) ON DELETE SET NULL,
    started_at timestamptz NOT NULL,
    ended_at   timestamptz,
    active_ms  bigint      NOT NULL DEFAULT 0 CHECK (active_ms >= 0)
);

-- At most one segment of a user's session is still going.
CREATE UNIQUE INDEX work_session_segments_open_idx ON work_session_segments (session_id, user_id)
    WHERE ended_at IS NULL;
CREATE INDEX work_session_segments_user_idx ON work_session_segments (user_id, started_at);
