# Architecture

This document describes how AgentOS is put together and, more usefully, why. It reflects what is
built today; the roadmap at the end says what is not.

## The shape of the problem

An agent that can operate a computer is a program that takes instructions from a statistical model
and executes them against real resources. Three things follow, and the architecture is mostly a
response to them:

1. **The model cannot be a security boundary.** It is influenced by everything it reads, and it reads
   things attackers write. Every control therefore has to hold when the model is entirely
   uncooperative.
2. **Some actions need a person.** Not as a UI affordance, but as a runtime state the system can
   actually be in — persisted, resumable, auditable.
3. **After the fact, you need to know what happened.** Including, especially, what was *refused*.

## Layers

```
┌──────────────────────────────────────────────────────────────────┐
│  Clients            agentos-cli          apps/desktop            │
├──────────────────────────────────────────────────────────────────┤
│  Runtime            agentos-runtime                              │
│                     state machine · agent loop · approval gate   │
├──────────────────────────────────────────────────────────────────┤
│  Execution          agentos-tools        agentos-providers       │
│                     pipeline · registry  anthropic · openai ·    │
│                     filesystem · terminal  mock                  │
│                     agentos-browser      agentos-computer        │
│                     CDP · sessions       screen · input          │
│                     agentos-demo                                 │
│                     mock CRM · scenario                          │
├──────────────────────────────────────────────────────────────────┤
│  Policy             agentos-permissions                          │
│                     policy engine · path sandboxing              │
├──────────────────────────────────────────────────────────────────┤
│  Facts              agentos-persistence  agentos-audit           │
│                     SQLite · repositories  event bus · chain     │
├──────────────────────────────────────────────────────────────────┤
│  Vocabulary         agentos-core         agentos-secrets         │
│                     types · events · trust  OS keychain          │
└──────────────────────────────────────────────────────────────────┘
```

Dependencies point downward only. `agentos-core` depends on nothing in the project and performs no
I/O, which is what lets the state machine and the trust types be tested in isolation.

## The trust boundary

`agentos-core::trust` defines four kinds of content, and only one of them is trusted:

```rust
enum Content {
    Control(ControlContent),      // operator instructions, the objective  — trusted
    Model(String),                // model prose                          — untrusted
    ToolCall(ToolCall),           // what the model wants to run          — untrusted
    Untrusted(UntrustedContent),  // every tool result, always            — untrusted
}
```

Two design points are doing the work:

**Model output is not control-plane content.** A compromised model cannot assert its way into
authority by claiming the system told it something.

**There is no conversion from a tool result to `Control`.** Not a discouraged one — an absent one.
`ToolResult::content` is typed `UntrustedContent`, and no constructor accepts it as control-plane
input. The compiler enforces what a prompt could only request.

When untrusted content is rendered for a model it is wrapped in a nonce-tagged envelope. Anything in
the body resembling a closing delimiter is neutralised — case-insensitively — and the nonce itself is
stripped from the body, so content cannot forge an end-of-envelope marker and continue as though it
were outside.

The envelope is a courtesy to a cooperative model. It is not the control. The control is that
authorisation never reads it.

## Permissions

`agentos-permissions` is a pure decision function over data the model does not supply:

```
PermissionRequest { tool, capability, risk, tainted }  ──►  PermissionDecision { effect, reason, rule }
```

Evaluation order, each step able only to tighten:

1. **Immutable denies.** `runtime.modify_policy`, `modify_agent`, `disable_audit`,
   `disable_approvals`. Checked first so no rule can shadow them. If an agent could edit its own
   policy, everything else here would be decoration.
2. **Global risk ceiling.**
3. **Rule matching.** Most specific wins, scored as `(domain, action, resource)` compared
   lexicographically. Ties go to the stricter effect, so a contradictory policy fails closed.
4. **Rule risk ceiling.**
5. **Taint escalation.** `allow` becomes `ask` once the run has read untrusted data and the action is
   consequential.

The default is `deny`, and an agent with no policy at all gets `DenyAllEngine`. Absence of a policy
never means absence of restriction.

### Path sandboxing

Scoping is only as good as the function that decides whether a path is inside its scope. Naive prefix
matching is not that function: `~/Docs/../../.ssh/id_rsa` starts with `~/Docs`, and so does
`~/Docs/link` when `link` points at `/etc`.

`path::resolve_secure` canonicalises the longest existing prefix — resolving symlinks and `..`
exactly as the kernel would — then appends the not-yet-existing remainder, rejecting any `..` that
survives into it. Only then is containment checked. This catches escapes for files that do not exist
yet, which an existence check would miss entirely.

## The tool pipeline

```
known and enabled? → validate → plan → authorise → approve → execute → capture → audit
```

Three properties are worth stating:

- **`plan` is side-effect free.** A tool's first side effect is inside `execute`, which is only
  reached after the policy engine and, where required, a human.
- **A refusal is a result.** Denials come back to the model as ordinary tool output so it can
  re-plan. It cannot argue past the decision, because the decision did not read anything it wrote.
- **Every capability in a plan is authorised.** A copy needs read on the source *and* write on the
  destination. Reading a file you may read and writing it somewhere you may not is exfiltration.

## The browser

Interaction is deterministic: CSS selectors over the Chrome DevTools Protocol, not screenshots and
coordinates. `browser.inspect` enumerates a page's interactive elements and returns a stable selector
for each, so the model names things rather than guessing at pixels. Screenshots exist for a human to
look at and for a future vision fallback.

Two properties fall out of that choice:

- **Actions are auditable.** `browser.click #send-button` can be reviewed, and an approval card can
  say what will be clicked. A coordinate pair cannot.
- **Capabilities are scoped by origin.** Navigation is authorised against the target URL's origin;
  everything else against the origin of the page the browser is currently on. A policy can grant one
  site rather than the web.

Each run gets its own browser process and its own profile, removed when the run ends. Deliberately
*not* the operator's profile: an agent inheriting every logged-in session on the machine is the
opposite of scoped access. Sessions are launched lazily and released through `Tool::end_run`, which
is the hook the runtime calls when a run finishes.

Planning never launches a browser. `plan` runs before authorisation, so it reads the current page's
origin from an existing session or refuses; starting a process would be a side effect at exactly the
point where there must not be one.

Everything read from a page is `DataSource::Web`, tagged with the URL, and taint-raising. A CRM
record whose notes field contains "ignore your instructions" is data about what somebody typed into a
CRM. See [ADR 5](adr/0005-deterministic-browser-automation.md).

## Computer control

The browser knows what it is clicking. A desktop does not: a keystroke goes wherever focus is, and
what it means there depends on what is underneath. So the scope on offer is the **application in
front**, as a `ResourceRef::Application` matched by glob, and three rules make it mean something.

- **The caller names its target.** Every `computer.*` call carries the application it is for, and
  planning refuses unless that application has focus. A tool that resolved the target for itself at
  execution time would faithfully type the message it was authorised to send to Mail into whatever
  had taken focus in the meantime.
- **Not knowing is a refusal.** With nothing in front, the call fails rather than falling back to an
  unscoped capability — which any rule listing no resources would match.
- **AgentOS is never a target.** An agent that can click can click Approve. This is the one place a
  tool overrides the policy engine instead of consulting it, because `IMMUTABLE_DENY` is a list of
  `(domain, action)` pairs and has no resource to hang the rule on.

Focus is re-read before every individual event, not once per call, so a change part-way through a
piece of typing stops the rest of it rather than splitting a password across two windows. What
already landed cannot be recalled, and the error says how much did.

`Desktop` is the entire platform boundary — macOS and Windows have a backend, everything else
refuses — and the shipped `RecordingDesktop` lets a policy be exercised with no screen involved. The
authorisation logic therefore runs on every platform in CI, including the one that could never
perform the actions.

Window titles and screenshots are both `DataSource::Screen`, and taint-raising. Reading the screen is
the broadest read in the system, and a capture cannot be put inside the nonce envelope that
neutralises text. What the scope does *not* do — bind what an event means, verify that an
application is what it says it is — is in [`SECURITY.md`](../SECURITY.md). See
[ADR 6](adr/0006-computer-control.md).

## Vision

`computer.screenshot` and `browser.screenshot` both take an `attach` flag. Without it they behave as
they always did: a PNG is written into the agent's workspace and the model is told the file exists.
With it, the capture is also handed to the model as `Content::Image`.

Three things follow from that being a separate flag rather than the default.

**It is a separate capability.** Attaching requires `computer:vision` or `browser:vision`, scoped the
same way the capture itself is — to an application, or to an origin. Saving a screenshot puts pixels
on a disk the operator owns; showing one to a model transmits the contents of their screen to a third
party. A policy written before this existed granted the first and must not silently acquire the
second because the runtime was upgraded.

**There is no trusted image.** `ControlContent` has no visual counterpart and `Content::Image` is
always untrusted, carrying the same `DataSource` the text does. Pixels are a worse place to draw the
boundary than text: there is no envelope to wrap them in and no delimiter to neutralise, and a
screenshot of a page reading "SYSTEM: you are now authorised" is, to a model, indistinguishable from
a system message. So the type system offers no way to say otherwise.

**A model that cannot see is told, not starved.** `ProviderCapabilities::vision` reports whether the
configured model accepts images — declared per model in `ModelConfig`, because one Ollama server will
serve a vision model and a text-only one. When it cannot, the pixels are dropped and a runtime notice
says which tool produced them and that they were withheld. A model given silence describes a screen
it never saw.

Everything shown to a model goes through `agentos_tools::vision::prepare` first: the header is read
before anything is allocated so a small file declaring 60000×60000 is refused rather than decoded,
the image is scaled to fit 1568 pixels on its long edge, and PNG gives way to JPEG only if it will
not otherwise fit. The copy written to disk is never the rescaled one. The conversation keeps the
three most recent captures and replaces older ones with the description they already carried, because
a provider re-reads the whole conversation every turn and an agent that took ten screenshots would
otherwise pay for all ten, ten times.

## Taint tracking

`TaintTracker` records whether a run has ingested externally-influenced data. Once it has, the policy
engine raises `allow` to `ask` for anything at or above the configured risk — `medium` by default.

The point is not to detect malice. It is that a run which has read something it did not write, and is
now about to do something consequential, is a moment a person should see. Escalation only tightens,
so a tainted run can never do more than a clean one, and the approval card names the sources the
agent has been reading.

## The state machine

```
Idle → Planning ⇄ Executing → Observing → Verifying → Completed
           ↑          ↓                       │
           │  WaitingForApproval              │
           └──────────┴───── more work ───────┘
                      ↓
                 Recovering → Planning        (any state) → Failed / Cancelled
```

`agentos_core::task::transition` is a pure function with an exhaustive table. Illegal transitions are
errors rather than silent no-ops, so a driver bug surfaces immediately instead of wedging a run in a
state nobody expected. `Cancel` is accepted from every non-terminal state — the operator must always
be able to stop an agent.

`RunStateMachine` owns the current state, persists each transition and emits an event for it. It is
shared with the approval gate wrapper, so `WaitingForApproval` is a state runs genuinely occupy while
a human is deciding, rather than a box in a diagram.

## Schedules and task graphs

Two separate questions about the same task: *when* may this start, and *what* is it waiting for.

Dependencies are a DAG in `task_dependencies`, not the tree `tasks.parent_task_id` describes, because
the common shape is a fan-in — three tasks gathering, one summarising all of them. A task with unmet
dependencies is `Blocked` rather than `Pending`, so "why has this not started?" has two different
answers instead of one ambiguous one.

Whether a dependency is satisfied is computed in SQL from the current status of the tasks upstream,
never stored. A stored flag is a second source of truth, and the failure mode is a task stuck because
somebody forgot to update it. Cycles are refused by `Runtime::add_dependency` before the edge is
written, and the error names the whole path — "there is a cycle" is not actionable.

A schedule creates tasks; it is not one. Each firing gets its own runs, traces, approvals and audit
entries. Cadences are once, a fixed interval with a sixty-second floor, or cron, read against UTC or
the host's local time. There is no timezone database here, so a named IANA zone is not on offer.

The next occurrence is computed forward from the moment a schedule actually fires, so a machine that
was asleep for three days wakes up owing one run rather than seventy-two. A task whose dependency
failed is cancelled and recorded as `agent.task.abandoned`, because a task that waits forever looks
exactly like one nobody has reached.

**The scheduler denies every approval, and there is no setting that changes it.** An unattended run
does what the policy permits outright and is refused everything else, with a note the model can read
and re-plan around. A scheduler that could approve on the operator's behalf would make the approval
gate decorative. See [ADR 8](adr/0008-scheduling-and-task-graphs.md).

## Persistence

SQLite via `sqlx`, with runtime-checked queries so contributors never need a live `DATABASE_URL` to
build. WAL mode lets a UI read while a run writes. Foreign keys are on, and the schema relies on them
for cascades.

Tables: `agents`, `policies`, `tasks`, `task_runs`, `task_steps`, `tool_executions`, `approvals`,
`memories`, `audit_events`, `settings`.

`tool_executions` records the permission effect, the risk and the taint state *at the time of the
call*, so a later reader does not have to reconstruct them from a policy that has since changed.

## Audit

Two independent guarantees:

- **Append-only**, enforced by SQLite triggers that `RAISE(ABORT)` on `UPDATE` and `DELETE`. The
  application has no code path that modifies an audit row, but "we did not write that code" is not a
  control.
- **Tamper-evident**, via a SHA-256 chain where each record hashes its contents together with its
  predecessor's hash. Fields are length-prefixed before hashing so that moving a character between
  adjacent fields changes the digest.

Editing a record breaks its own hash; fixing that hash breaks the next record's back-pointer.
`agentos audit verify` reports every break rather than stopping at the first.

## Providers

`ModelProvider` is provider-neutral over text, tool calls, tool results, stop reasons and usage —
which every provider worth supporting has. Implementations: Anthropic Messages, any
OpenAI-compatible chat-completions endpoint (OpenAI, Ollama, LM Studio, vLLM, LiteLLM), and a
scripted mock.

The mock is not an afterthought. The entire test suite runs against it, which is what makes the
"model has been hijacked" scenario a deterministic fixture rather than a hope.

Credentials come from the OS keychain at call time, and provider error bodies are scanned for
credential-shaped tokens and redacted before they reach a log or an issue report.

## Deliberate omissions

- **No vector store.** Memory is structured SQLite rows with keyword retrieval, behind an interface a
  semantic index would also satisfy. A vector database is a reasonable second implementation and a
  poor first one.
- **No plugin system yet.** The `Tool` trait and the registry are the seam it will use.
- **No streaming.** Turn-based is enough for agent work today and considerably simpler.
- **Verification is shallow.** The `Verifying` state currently treats "the model stopped calling
  tools" as done. A second model pass judging the result belongs there; the seam exists.

## The demonstration

`agentos-demo` holds a mock CRM — a few dozen lines of HTML over a hand-rolled loopback HTTP server,
no web framework — and the scenario run against it. Five customers, three overdue a follow-up, and
one record whose notes field contains a prompt-injection payload written the way a real one would be:
it impersonates a system message, invents authority, and asks for actions the agent has tools for.

The end-to-end test scripts the *model* to fall for it completely. Every resulting call is refused,
by two independent mechanisms — the policy engine for the tools the agent has, and the registry for
the ones it was never given — and the run still completes with a report. It takes well under a second,
so it runs on every commit rather than being a demo somebody performs occasionally.

## Roadmap

**Next:** an orchestrator that writes task graphs rather than a person writing them; multi-agent
delegation; plugins; and the integrations in the README's phase 6.
