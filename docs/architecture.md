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
│  Clients            agentos-cli          (desktop app, later)    │
├──────────────────────────────────────────────────────────────────┤
│  Runtime            agentos-runtime                              │
│                     state machine · agent loop · approval gate   │
├──────────────────────────────────────────────────────────────────┤
│  Execution          agentos-tools        agentos-providers       │
│                     pipeline · registry  anthropic · openai ·    │
│                     filesystem · terminal  mock                  │
│                     agentos-browser      agentos-demo            │
│                     CDP · sessions       mock CRM · scenario     │
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

**Next:** the Tauri 2 desktop application — dashboard, agents, tasks with live traces, the approval
card, activity, settings — consuming this runtime with no logic of its own; computer control for
macOS and Windows behind one Rust interface; scheduler; orchestrator with task graphs; multi-agent
delegation; plugins.
