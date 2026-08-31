# ADR 8: Scheduling, and edges between tasks

**Status:** accepted

## Context

Every task AgentOS could run was one a person started and watched. `tasks.parent_task_id` existed and
was documented as being "for orchestrated task graphs", but nothing wrote it, nothing read it, and a
parent pointer cannot express the shape that actually comes up — three tasks gathering and a fourth
that summarises all of them.

There was also no way to say "every weekday at nine". The runtime that a business is supposed to run
on could only act while somebody sat in front of it.

## Decision

Two things, deliberately kept separate, because they answer different questions about the same task:
*when* may this start, and *what* is it waiting for.

**Dependencies are a DAG in their own table.** `task_dependencies(task_id, depends_on_task_id)`, with
a `CHECK` against self-loops and cascade deletes on both sides. A task with unmet dependencies is
stored `Blocked`, a new `TaskStatus`, so "why has this not started?" has two different answers rather
than one ambiguous one: a pending task is waiting for a scheduler; a blocked one is waiting for
something that might never happen.

Whether a dependency is *satisfied* is not stored. It is computed, in SQL, from the current status of
the tasks upstream — a `NOT EXISTS` over the join. Storing it would mean a second source of truth
that somebody has to remember to update, and a task stuck because nobody did.

**Cycles are refused when an edge is written.** `Runtime::add_dependency` walks the existing edges
breadth-first from the proposed dependency; if the waiting task is reachable, the edge would close a
loop and the error names the whole path. "There is a cycle" is not actionable; "A waits for B waits
for C waits for A" is. The check lives in the runtime rather than in a database trigger because
SQLite has no recursive constraint, and because a trigger firing on a single row could not name the
path.

**A schedule is not a task.** It is a standing instruction that *creates* tasks, one per firing, so
each occurrence keeps its own runs, traces, approvals and audit entries. Cadences are `Once`, `Every
{ seconds }` with a sixty-second floor, and `Cron { expression, clock }`.

**Missed firings do not accumulate.** The next occurrence is computed forward from the moment a
schedule actually fires, not from the slot it was supposed to fill. A machine asleep for three days
wakes up owing one run. The alternative — replaying every missed slot — turns a closed laptop into a
denial-of-service against the operator's own API budget.

**A dead branch is reported, not left waiting.** When a dependency fails or is cancelled, everything
downstream is cancelled and an `agent.task.abandoned` event names the task that ended it. A task that
waits forever is indistinguishable, from the outside, from one nobody has got to yet.

**The scheduler denies every approval.** This is the decision the rest of it rests on. A scheduled run
happens with nobody present, so it is driven behind `DenyAllGate`: anything the policy permits
outright proceeds, and anything that would have put a card in front of a person is refused with a
note the model can read and re-plan around. There is no configuration that changes this. A scheduler
that could approve on the operator's behalf would make the approval gate decorative, and the approval
gate is most of why this project exists.

`Scheduler::with_approvals` exists for tests and for a client that genuinely has somebody attached.
It is documented as not being a way around the paragraph above.

**Concurrency defaults to one.** Unattended runs cost money and touch the world. An operator who
wants several at once should have to say so.

## Consequences

Migration `0004` adds `schedules` and `task_dependencies`, and two nullable columns on `tasks`.
Existing rows read as "no clock constraint, no schedule", which is what they were.

`cron` becomes a dependency. Five-field expressions — what `crontab` takes and what anybody will
actually type — gain a leading `0` for seconds rather than an error.

A cron expression is read against UTC or against the host's local time, and nothing else. AgentOS
carries no timezone database, so a named IANA zone is not something it can honour, and pretending
otherwise would mean a schedule that silently drifts. `Local` will shift by an hour across a
daylight-saving boundary; that is stated rather than hidden.

The CLI cannot create a cycle. `task create --depends-on` only names tasks that already exist, and a
task being created has nothing waiting on it yet, so the edge cannot close a loop. Cycle detection
exists for the runtime API, which orchestration will use.

The desktop application has no schedules screen yet. Tasks a schedule created appear in its task list
like any other, which is honest but not the same as being able to manage a schedule from the window.

## Alternatives considered

**Reusing `parent_task_id` for dependencies.** A tree cannot express a fan-in, which is the common
case. Rejected.

**Storing a "dependencies satisfied" flag on the task.** Faster to read, and a second source of truth
that goes stale the first time a status changes by a path that forgets to update it. Rejected.

**Backfilling missed firings.** Defensible for a job queue whose work is cheap and idempotent. Every
firing here is an agent run against a paid model that touches the outside world. Rejected.

**An approval gate that parks a scheduled run until somebody resolves it from the desktop.** Genuinely
useful, and it needs cross-process approval delivery that does not exist yet — the desktop's gate is
an in-process channel. Deferred rather than half-built: a gate that appeared to wait but silently
timed out into approval would be worse than an honest refusal.

**A resident daemon with a service manifest.** `agentos schedule run` in the foreground is a process
somebody can supervise with whatever they already use. Packaging a launchd plist and a systemd unit
is a separate piece of work from making the loop correct.
