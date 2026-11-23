-- Personal kanban (ARCHITECTURE §5.4, PRD G6, ADR-0014, T2.1.3). Owned by the `tasks` crate.
-- Additive only.
--
-- Columns and placements belong to one user: nobody else's board reads or changes them. No foreign
-- keys to users (owned by another module). `position` is a fractional index compared byte by byte
-- (COLLATE "C"). Its uniqueness is deferred so that a whole column can be renumbered in one
-- transaction. Timestamps come from the application clock.

CREATE TABLE user_board_columns (
    id         uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    user_id    uuid        NOT NULL,
    name       text        NOT NULL CHECK (btrim(name) <> '' AND char_length(name) <= 60),
    position   text        COLLATE "C" NOT NULL CHECK (position ~ '^[0-9A-Za-z]*[1-9A-Za-z]$'),
    is_inbox   boolean     NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT user_board_columns_position_unique UNIQUE (user_id, position)
        DEFERRABLE INITIALLY DEFERRED
);

-- G6: every user has exactly one inbox.
CREATE UNIQUE INDEX user_board_columns_inbox_idx ON user_board_columns (user_id) WHERE is_inbox;

CREATE TABLE user_task_placements (
    user_id    uuid        NOT NULL,
    task_id    uuid        NOT NULL REFERENCES tasks (item_id) ON DELETE CASCADE,
    column_id  uuid        NOT NULL REFERENCES user_board_columns (id),
    position   text        COLLATE "C" NOT NULL CHECK (position ~ '^[0-9A-Za-z]*[1-9A-Za-z]$'),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    PRIMARY KEY (user_id, task_id),
    CONSTRAINT user_task_placements_position_unique UNIQUE (column_id, position)
        DEFERRABLE INITIALLY DEFERRED
);

CREATE INDEX user_task_placements_task_idx ON user_task_placements (task_id);
