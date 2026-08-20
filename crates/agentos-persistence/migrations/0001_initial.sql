-- AgentOS initial schema.
--
-- Conventions:
--   * identifiers are UUID text
--   * timestamps are RFC3339 text in UTC, which sorts lexicographically
--   * JSON columns hold serde-serialised values and are documented per column
--
-- The audit table is append-only, enforced by triggers at the bottom of this
-- file rather than by application convention.

CREATE TABLE agents (
    id                TEXT    PRIMARY KEY NOT NULL,
    name              TEXT    NOT NULL UNIQUE,
    instructions      TEXT    NOT NULL,
    provider          TEXT    NOT NULL,
    model             TEXT    NOT NULL,
    temperature       REAL,
    max_output_tokens INTEGER,
    base_url          TEXT,
    -- JSON array of tool names.
    enabled_tools     TEXT    NOT NULL DEFAULT '[]',
    status            TEXT    NOT NULL DEFAULT 'enabled',
    max_steps         INTEGER NOT NULL DEFAULT 24,
    -- Arbitrary JSON for plugins and UI.
    metadata          TEXT    NOT NULL DEFAULT 'null',
    created_at        TEXT    NOT NULL,
    updated_at        TEXT    NOT NULL
);

-- One policy per agent, stored as its YAML source so the operator's comments
-- and formatting survive a round trip through the UI.
CREATE TABLE policies (
    agent_id   TEXT    PRIMARY KEY NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    document   TEXT    NOT NULL,
    version    INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT    NOT NULL
);

CREATE TABLE tasks (
    id             TEXT PRIMARY KEY NOT NULL,
    agent_id       TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    objective      TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'pending',
    parent_task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    created_at     TEXT NOT NULL,
    started_at     TEXT,
    completed_at   TEXT
);
CREATE INDEX idx_tasks_agent   ON tasks(agent_id, created_at DESC);
CREATE INDEX idx_tasks_status  ON tasks(status, created_at DESC);
CREATE INDEX idx_tasks_parent  ON tasks(parent_task_id);

CREATE TABLE task_runs (
    id            TEXT    PRIMARY KEY NOT NULL,
    task_id       TEXT    NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    attempt       INTEGER NOT NULL,
    state         TEXT    NOT NULL DEFAULT 'idle',
    tainted       INTEGER NOT NULL DEFAULT 0,
    steps_taken   INTEGER NOT NULL DEFAULT 0,
    result        TEXT,
    -- JSON-serialised TaskFailure.
    failure       TEXT,
    input_tokens  INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    started_at    TEXT    NOT NULL,
    completed_at  TEXT,
    UNIQUE (task_id, attempt)
);
CREATE INDEX idx_runs_task  ON task_runs(task_id, attempt DESC);
CREATE INDEX idx_runs_state ON task_runs(state);

CREATE TABLE task_steps (
    id                TEXT    PRIMARY KEY NOT NULL,
    run_id            TEXT    NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    ordinal           INTEGER NOT NULL,
    kind              TEXT    NOT NULL,
    state             TEXT    NOT NULL,
    summary           TEXT    NOT NULL,
    tool_execution_id TEXT,
    -- Arbitrary JSON detail for the trace view.
    detail            TEXT,
    at                TEXT    NOT NULL,
    UNIQUE (run_id, ordinal)
);
CREATE INDEX idx_steps_run ON task_steps(run_id, ordinal);

CREATE TABLE tool_executions (
    id            TEXT    PRIMARY KEY NOT NULL,
    run_id        TEXT    NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    tool          TEXT    NOT NULL,
    call_id       TEXT    NOT NULL,
    -- JSON arguments, after schema validation.
    arguments     TEXT    NOT NULL,
    outcome       TEXT    NOT NULL,
    -- The permission effect that was applied: allow / ask / deny.
    effect        TEXT    NOT NULL,
    risk          TEXT    NOT NULL,
    tainted       INTEGER NOT NULL DEFAULT 0,
    approval_id   TEXT,
    output_bytes  INTEGER NOT NULL DEFAULT 0,
    error         TEXT,
    duration_ms   INTEGER NOT NULL DEFAULT 0,
    started_at    TEXT    NOT NULL,
    completed_at  TEXT
);
CREATE INDEX idx_executions_run  ON tool_executions(run_id, started_at);
CREATE INDEX idx_executions_tool ON tool_executions(tool, started_at DESC);

CREATE TABLE approvals (
    id                 TEXT PRIMARY KEY NOT NULL,
    agent_id           TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    agent_name         TEXT NOT NULL,
    task_id            TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    run_id             TEXT NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    tool               TEXT NOT NULL,
    arguments          TEXT NOT NULL,
    -- JSON-serialised Capability.
    capability         TEXT NOT NULL,
    risk               TEXT NOT NULL,
    reason             TEXT NOT NULL,
    explanation        TEXT NOT NULL,
    -- JSON array of strings.
    affected_resources TEXT NOT NULL DEFAULT '[]',
    tainted            INTEGER NOT NULL DEFAULT 0,
    status             TEXT NOT NULL DEFAULT 'pending',
    requested_at       TEXT NOT NULL,
    decided_at         TEXT,
    decision_note      TEXT
);
CREATE INDEX idx_approvals_status ON approvals(status, requested_at);
CREATE INDEX idx_approvals_run    ON approvals(run_id);

CREATE TABLE memories (
    id         TEXT PRIMARY KEY NOT NULL,
    agent_id   TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    content    TEXT NOT NULL,
    -- JSON-serialised DataSource, so retrieval can say where a claim came from.
    source     TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 1.0,
    task_id    TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_memories_agent ON memories(agent_id, updated_at DESC);
CREATE INDEX idx_memories_kind  ON memories(agent_id, kind);

CREATE TABLE audit_events (
    id         TEXT    PRIMARY KEY NOT NULL,
    sequence   INTEGER NOT NULL UNIQUE,
    at         TEXT    NOT NULL,
    kind       TEXT    NOT NULL,
    agent_id   TEXT,
    task_id    TEXT,
    run_id     TEXT,
    payload    TEXT    NOT NULL,
    prev_hash  TEXT    NOT NULL,
    hash       TEXT    NOT NULL
);
CREATE INDEX idx_audit_at   ON audit_events(at DESC);
CREATE INDEX idx_audit_kind ON audit_events(kind, at DESC);
CREATE INDEX idx_audit_run  ON audit_events(run_id, sequence);

-- Append-only enforcement.
--
-- The application layer has no code path that updates or deletes an audit row,
-- but "we did not write that code" is not a control. These triggers make the
-- database refuse, so a bug, a future contributor or anyone with a SQL prompt
-- gets an error rather than a silently rewritten history.
CREATE TRIGGER audit_events_no_update
BEFORE UPDATE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit_events is append-only: rows cannot be updated');
END;

CREATE TRIGGER audit_events_no_delete
BEFORE DELETE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit_events is append-only: rows cannot be deleted');
END;

CREATE TABLE settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
