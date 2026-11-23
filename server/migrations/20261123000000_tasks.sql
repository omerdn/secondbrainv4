-- Tasks, subtasks and assignees (ARCHITECTURE §5.4, PRD G1, G2, G4, T2.1.1). Owned by the `tasks`
-- crate. Additive only.
--
-- `tasks` is the detail table of a `task` item (ARCHITECTURE §5.3), so it follows the item's
-- lifetime through the foreign key: `tasks` never reads or writes `items`. No foreign keys to
-- users (owned by another module). Timestamps come from the application clock.

CREATE TABLE tasks (
    item_id    uuid        PRIMARY KEY REFERENCES items (id) ON DELETE CASCADE,
    start_date date,
    due_date   date,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT tasks_dates_check CHECK (
        start_date IS NULL OR due_date IS NULL OR start_date <= due_date
    )
);

CREATE TABLE subtasks (
    id         uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    task_id    uuid        NOT NULL REFERENCES tasks (item_id) ON DELETE CASCADE,
    title      text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 500),
    position   integer     NOT NULL,
    done_at    timestamptz,
    done_by    uuid,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT subtasks_position_unique UNIQUE (task_id, position),
    -- G4: a completed subtask always records who completed it.
    CONSTRAINT subtasks_done_check CHECK ((done_at IS NULL) = (done_by IS NULL))
);

CREATE TABLE subtask_assignees (
    subtask_id uuid        NOT NULL REFERENCES subtasks (id) ON DELETE CASCADE,
    user_id    uuid        NOT NULL,
    created_at timestamptz NOT NULL,
    PRIMARY KEY (subtask_id, user_id)
);

CREATE INDEX subtask_assignees_user_idx ON subtask_assignees (user_id);
