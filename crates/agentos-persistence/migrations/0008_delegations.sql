-- Delegation: one run asking another agent to carry out part of its work.
--
-- A row is written when the parent asks, before the child run is prepared, so
-- child_run_id starts NULL and state starts 'requested'. A delegation that
-- cannot be recorded is one that is not performed; the same ordering approval
-- requests follow, for the same reason.
--
-- This table is for reading a tree back. It is not the evidence: every edge is
-- also sealed into the audit chain, in the payload of agent.delegation.started,
-- which names the parent run and the child run inside the hash. A row here can
-- be edited; that record cannot be without the chain saying so.
CREATE TABLE delegations (
    id              TEXT PRIMARY KEY NOT NULL,
    parent_run_id   TEXT NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    parent_agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    child_agent_id  TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    child_task_id   TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    child_run_id    TEXT REFERENCES task_runs(id) ON DELETE SET NULL,
    -- Written by the parent's model, and the child's objective. Not operator
    -- text, though it is stored where operator text usually is.
    objective       TEXT NOT NULL,
    -- How far below the root run the child sits. A root run is depth 0 and
    -- has no row; its children are depth 1.
    depth           INTEGER NOT NULL CHECK (depth >= 1),
    state           TEXT NOT NULL DEFAULT 'requested'
                    CHECK (state IN ('requested', 'running', 'completed', 'failed',
                                     'refused', 'cancelled')),
    steps_spent     INTEGER NOT NULL DEFAULT 0 CHECK (steps_spent >= 0),
    requested_at    TEXT NOT NULL,
    completed_at    TEXT
);

-- A run's children, in the order it asked for them.
CREATE INDEX idx_delegations_parent ON delegations(parent_run_id, requested_at);

-- The upward edge, which a trace reads to say what started a run. Unique: a
-- run is started by one delegation or by none, and a second row claiming the
-- same child would make "who asked for this" ambiguous. SQLite admits any
-- number of NULLs, which is every delegation not yet attached to its run.
CREATE UNIQUE INDEX idx_delegations_child ON delegations(child_run_id);

-- The task a delegation created. Read before a task is started on its own,
-- which a delegated task never is: it runs only through its delegation, under
-- every policy above it.
CREATE INDEX idx_delegations_child_task ON delegations(child_task_id);

-- Which agents a request was made on behalf of, as a JSON array of names from
-- the root run to the agent asking. Rows written before delegation existed
-- were made by undelegated runs, and the empty array says exactly that.
ALTER TABLE approvals ADD COLUMN delegation_chain TEXT NOT NULL DEFAULT '[]';
