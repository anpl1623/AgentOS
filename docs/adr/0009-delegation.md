# ADR 9: Delegation

**Status:** accepted

## Context

Every run AgentOS could perform belonged to one agent. [ADR 8](0008-scheduling-and-task-graphs.md)
gave tasks edges between them, but a person writes that graph before anything starts. The shape that
keeps coming up is decided during a run instead: a supervisor reads the objective, concludes that it
needs a researcher and a writer, asks them, and reads back what they did.

Delegation is also the easiest way there is to launder authority, and every one of the ways is
ordinary rather than exotic:

- An agent denied something asks an agent that is not.
- An agent that has read a hostile page asks a clean one to send the mail. The second agent never saw
  the page, so its run is untainted and nothing it does asks anybody.
- The child's report comes back to the parent as prose, written by a model about things it read
  outside, and the parent treats it as its own work.
- A person is shown a card saying `writer` wants to run `email.send`, with nothing on it to say that
  `writer` was asked to by `supervisor`, which was asked to by nobody present.
- Four children each given their own step budget cost four times what the operator agreed to, and an
  agent that may ask itself costs without limit.

A design that closes most of these has made the remaining one the path of least resistance.

## Decision

**Delegation is a tool.** `agent.delegate` takes a list of `{ agent, objective }` requests and is
authorised like any other call: it requires `runtime.delegate`, scoped by
`ResourceRef::Named { name }` to the agent being asked, once per child, and the strictest answer
governs the whole call. Its risk is `high`, because it causes a second agent to act in the world. It
takes a list so that fanning out and gathering back happen inside one parent run, where taint,
approvals and cancellation already work, rather than as tasks a scheduler would start later,
unattended and clean. `runtime.delegate` is not in the starter policy, so a new agent cannot delegate
until an operator says so, and it is not in `IMMUTABLE_DENY` either, because it has to be grantable.
Both are tested, so that a later edit cannot confuse it with `runtime.modify_policy`.

**A child is evaluated by both policies.** Its engine is
`IntersectionEngine::new(vec![parent_engine, child_engine])`: every request goes to both live
policies, the strictest effect wins, and the decision reported is the winning engine's own, so a
denial's `matched_rule` names the rule that actually refused. A grandchild is evaluated by its
parent's intersection plus its own engine, so the intersection runs all the way to the root. An
empty intersection denies — absence of a policy never means absence of restriction.

**Taint is shared, not copied.** A child run uses its parent's own `TaintTracker`. A parent's taint
reaches the child, so a delegate is never cleaner than its delegator, and a child's taint reaches the
parent, so a supervisor does not act clean on a report built from a page it never read itself. The
report is returned as `DataSource::Agent { agent }`, which is externally influenced: a sub-agent's
summary is a model's prose about things somebody else wrote, and believing it would launder every
taint the child picked up. The sources are persisted on both runs.

**One step budget for the whole tree.** Every turn any agent in the tree takes is spent from the
root's budget, through one shared `RunBudget`. The supervisor's `max_steps` is what the operator
agreed to pay for, and a delegation does not change the price.

**The limits belong to the installation.** `RuntimeConfig.delegation` holds `max_depth: 2` and
`max_children_per_run: 4`. Two, because a supervisor of supervisors is already hard to reason about;
four, because a run that wants five sub-agents wants a task graph. Every refusal is decided when the
call is planned, before authorisation and before any side effect: depth, fan-out, an unknown or
disabled agent, an exhausted budget, and an agent asking itself or one of its ancestors. A cycle is
refused with the path spelt out — `sales -> researcher -> sales` — for the same reason ADR 8 names a
dependency cycle: "there is a cycle" is not actionable.

**A delegation is recorded before it is performed.** A row in the `delegations` table is written in
state `requested` before the child run is prepared — the ordering `RunApprovalGate` already uses for
approvals, and for the same reason: a delegation that cannot be recorded is one that must not happen.
The child's task is created with `parent_task_id` set, which gives that column its first writer.
`agent.delegation.started` is emitted once the child run is attached, and carries the delegation, the
parent run, the child task and the child run; `agent.delegation.completed` follows it, and a refusal
is `agent.delegation.refused`, which is security-relevant: it is an agent reaching past its grant,
the same class of signal as asking for a tool that does not exist.

**A child's approvals go to the operator, and say who asked.** The child's gate is the operator's own
gate, wrapped in a fresh `RunApprovalGate` around the child's own state machine. Wrapping the parent's
wrapper instead would drive the parent's state machine from a child's approval. `ApprovalRequest`
carries `delegation_chain`, the agent names from the root run to the one asking, stamped by the gate
before the request is persisted or shown, so the tools never see it. The card says it plainly:
"worker, asked by supervisor". Each child run is held to its own approval budget like any other run.

**A failed child is a result.** The supervisor reads "researcher failed: step budget exhausted" in
the tool's output and re-plans, exactly as it reads a refused approval today. A child failing is not
the parent's tool erroring. Cancelling the parent cancels every child through tokens derived from the
parent's.

**The audit record format does not change.** The chain is per installation, and it already orders
every event from every agent totally, so a delegation tree needs no verification of its own. The
edges are sealed inside the hashed payloads instead: `agent.delegation.started` names both runs, so
rewriting who started whom breaks the chain like any other edit. What a chain cannot show is an
absence, so `verify_coverage(records, run)` reads the payloads and reports what should be there and
is not — a `tool.execution.started` with nothing after it, an `approval.granted` with no
`approval.requested` before it, a delegated child run with no records at all. The desktop's full
audit check reports those gaps beside chain breaks, so "intact" means complete as well as consistent.

## Consequences

**A supervisor's policy is the ceiling for its whole tree.** A worker can only do what both policies
permit, so for a worker to write a file the supervisor's policy must allow that write too, even though
the supervisor never holds `filesystem.write`. Its tool list decides what it does itself; its policy
decides what anything it asks may do. A supervisor's policy therefore needs `runtime.delegate` naming
each agent it may ask, every capability its workers will use, and a `max_risk` of at least `high`,
because `agent.delegate` is high risk and the starter's `medium` refuses it. Read on its own, such a
policy looks wider than the agent it belongs to. That is accurate rather than misleading: it is wide
on behalf of the agents it directs.

**A supervisor is tainted by its first delegation.** The report is external data, so everything
after it that would have been allowed at or above the escalation threshold asks instead. The extra
cards are the third case in the context being closed, and the price of closing it.

**Unattended delegation runs behind the deny-all gate.** Under the scheduler, or
`task run --unattended`, a child's asks are refused exactly as the parent's are. Delegation is not a
route by which a scheduled run obtains a yes; a tree that needs a person needs a person.

**A delegated task cannot be run on its own.** Its task is an ordinary row, and a failed child looks
like any failed task, so a retry of it would be a root run: its own policy alone, a fresh step budget,
no depth, no chain on its cards. The runtime refuses to start a task that a delegation created, from
every client, and the desktop does not offer the button. To try the work again, an operator runs the
supervisor again.

**`max_concurrent_runs` counts every live run, children included.** The scheduler used to count the
tasks it had started. A scheduled supervisor running at a concurrency of one now holds the scheduler
while its children work, and a tree of five runs is five runs. That is what the setting was always
meant to bound — unattended runs cost money and touch the world, whoever started them.

**`agent.delegate` has no per-call time limit.** Every other tool call is abandoned once the
context's timeout passes, a minute by default. A delegation lasts as long as its children's runs,
which take several model turns and may wait at an approval for as long as a person takes, so a fixed
limit would fail exactly the delegations that are working. The tool says so through
`Tool::time_limit`, which every other tool leaves at the context's timeout. What bounds it instead is
what bounds its children: each model request and tool call inside them keeps its own limit, the
shared step budget bounds how many there are, and the parent's token stops them all.

Migration `0008` adds `delegations` and a `delegation_chain` column on `approvals` that defaults to an
empty list. Existing approvals read as an empty chain, which is what they were.

The CLI prints each child beneath the `agent.delegate` step that started it, and
`agentos task show --tree` prints the whole tree; the default stays flat, because somebody debugging
one run should not have to page through a forest. The desktop trace shows the same, with a line naming
who started a delegated run.

The limits are the same for every agent. An operator cannot give one supervisor a deeper tree than
another without raising the limit for all of them.

## Alternatives considered

**A static merge of the two policies.** `Policy::intersect` would combine the rule sets, default
effects, ceilings and taint settings element by element into one policy for the child. It is a second
policy evaluator, written to agree with the first on specificity, overlapping globs and ties, and
waiting for the first edit that makes them disagree. Evaluating both real policies is right by
construction, and it can name the rule that refused. Rejected.

**Refusing a delegation whose child holds tools the parent does not.** It forbids the main use: a
supervisor holding little more than `agent.delegate`, directing workers that hold the tools. The tool
list is a convenience filter, not a boundary; the intersection is the boundary. Rejected.

**Limits on each agent.** Depth and fan-out columns on `agents` would let an operator tune them per
agent, and would leave open whose limit governs once a tree is three agents deep. The shape of a tree
is a property of the installation running it. Rejected.

**Copying the parent's taint into a fresh tracker for the child.** It stops the delegate being
cleaner than its delegator and misses the return path: the supervisor would act untainted on a report
built from a page only the child read. Rejected.

**A step budget for each child.** Easier to explain per run, and a fan-out of four would cost four
times what the operator agreed to. Rejected.

**Putting causal edges into the audit hash.** `caused_by` and `delegated_from` fields on every
record, fed into the hash, with a version number so that older records verify under the old field
order. It is a migration over an append-only table, triggers dropped and recreated, and two hash
formats verified for ever. A mistake in it makes every existing installation report tampering on
upgrade, which is the worst false positive this mechanism can produce. The edges are sealed in the
payloads already, and coverage, which is what the rework was for, can be computed from them.
Rejected.
