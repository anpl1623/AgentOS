# 4. SQLite, with an append-only hash-chained audit log

- **Status:** accepted
- **Date:** 2026-08-20

## Context

AgentOS is local-first. It needs durable storage for agents, tasks, runs, execution traces,
approvals, memory and audit events, on a user's machine, with no server to administer.

The audit log has a stronger requirement than the rest: it is the record of what an agent was allowed
to do, and it must be worth trusting after the fact.

## Decision

An embedded SQLite database at `~/.agentos/agentos.db`, accessed through `sqlx` with WAL mode and
foreign keys on. Queries are runtime-checked rather than macro-checked, so building the project never
requires a live `DATABASE_URL`.

`audit_events` gets two additional properties:

**Append-only**, enforced by `BEFORE UPDATE` and `BEFORE DELETE` triggers that `RAISE(ABORT)`.

**Tamper-evident**, via a SHA-256 chain: each record hashes its own contents together with its
predecessor's hash, with fields length-prefixed so that shifting a character between adjacent fields
changes the digest.

## Consequences

No daemon, no connection string, no setup. WAL means the desktop UI can read while a run writes.

Editing an audit record breaks its own hash; recomputing that hash breaks the next record's
back-pointer. Partial edits — the realistic case — are detectable, and `agentos audit verify` reports
every break rather than stopping at the first.

The triggers matter more than they look. The application has no code path that updates an audit row,
but that is a property of today's code rather than a control. With the triggers, a bug, a future
contributor or anyone with a SQL prompt gets an error instead of a silently rewritten history. There
is a test that runs a raw `UPDATE` and asserts it is refused.

Limits, stated plainly: someone with write access to the file can rewrite the entire chain
consistently. The chain makes tampering detectable, not impossible, and no local-only design can do
better.

SQLite's write concurrency is modest. For one machine coordinating agents this is not a constraint;
if it becomes one, the repository layer is the seam.

## Rejected

**PostgreSQL.** A server to install, run and back up, for a single-user local application. The
repository interfaces do not preclude it later.

**Append-only files (JSONL).** Simpler to make append-only, much worse to query, and the UI needs to
query — by run, by kind, by time.

**Compile-time-checked queries (`sqlx::query!`).** Stronger typing, but every contributor would need
a prepared database to build. Wrong trade for an open-source project; the repository tests cover the
same ground.
