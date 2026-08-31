-- Recurring work, and edges between tasks.

CREATE TABLE schedules (
    id           TEXT PRIMARY KEY NOT NULL,
    agent_id     TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    name         TEXT NOT NULL UNIQUE,
    -- The objective each firing gets. Trusted control-plane text, exactly like
    -- the objective on a task somebody typed.
    objective    TEXT NOT NULL,
    -- JSON, the serialised `Cadence`. Kept as one column because the shape
    -- differs per variant and splitting it would mean nullable columns that
    -- only make sense in combination.
    cadence      TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'active',
    next_run_at  TEXT,
    last_run_at  TEXT,
    last_task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

-- The scheduler's hot query is "what is due?", which reads this index alone.
CREATE INDEX idx_schedules_due   ON schedules(status, next_run_at);
CREATE INDEX idx_schedules_agent ON schedules(agent_id, created_at DESC);

-- Edges of a task graph: `task_id` waits for `depends_on_task_id`.
--
-- A DAG rather than the tree `tasks.parent_task_id` already expresses, because
-- the common shape is a fan-in — three gathering tasks and one that summarises
-- all of them — which a parent pointer cannot represent.
--
-- Acyclicity is enforced in the runtime before the edge is written, not here.
-- SQLite has no recursive constraint, and a cycle discovered at insert time can
-- name the path it would have closed, which a trigger firing on a single row
-- could not.
CREATE TABLE task_dependencies (
    task_id            TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    depends_on_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    created_at         TEXT NOT NULL,
    PRIMARY KEY (task_id, depends_on_task_id),
    -- The one cycle a single row can express.
    CHECK (task_id <> depends_on_task_id)
);
CREATE INDEX idx_task_dependencies_reverse ON task_dependencies(depends_on_task_id);

-- The earliest moment a task may start. NULL means "as soon as something picks
-- it up", which is every task that existed before schedules did.
ALTER TABLE tasks ADD COLUMN scheduled_for TEXT;

-- Which schedule created this task, when one did. No foreign key: SQLite cannot
-- add a referencing column to an existing table, and the value is only ever
-- written by the scheduler from a schedule it just read.
ALTER TABLE tasks ADD COLUMN schedule_id TEXT;

CREATE INDEX idx_tasks_due      ON tasks(status, scheduled_for);
CREATE INDEX idx_tasks_schedule ON tasks(schedule_id, created_at DESC);
