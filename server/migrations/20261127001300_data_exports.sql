-- Personal data exports (PRD HS3, T4.3.2). Owned by `app::exports`. Additive only.
--
-- A job builds the export (JSON and Markdown) of what the user can see and stores it here; a
-- random token makes the download link, which works until `expires_at`. Expired rows are
-- deleted daily, contents first. No files are written to disk (PRD §17).

CREATE TABLE data_exports (
    id         uuid        PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status     text        NOT NULL CHECK (status IN ('pending', 'ready', 'failed')),
    token      text        NOT NULL UNIQUE,
    json       text,
    markdown   text,
    created_at timestamptz NOT NULL,
    ready_at   timestamptz,
    expires_at timestamptz NOT NULL,
    CONSTRAINT data_exports_ready_check CHECK ((status = 'ready') = (json IS NOT NULL AND markdown IS NOT NULL))
);

CREATE INDEX data_exports_user_idx ON data_exports (user_id, created_at DESC);
CREATE INDEX data_exports_expires_idx ON data_exports (expires_at);
