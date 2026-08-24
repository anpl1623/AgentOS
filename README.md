# AgentOS

> **An open-source AI operating system for running your business from your computer.**

AgentOS is a local-first, open-source desktop platform that allows AI agents to safely operate computers, browsers, applications, files, and business systems on behalf of their users.

Instead of asking an AI to tell you **what** to do, AgentOS is designed to let the AI **actually do the work**.

```text
You
 │
 │ "Handle today's operations."
 ▼
┌─────────────────────────────────────┐
│             AgentOS                 │
│                                     │
│        AI Orchestrator              │
│               │                     │
│     ┌─────────┼─────────┐           │
│     ▼         ▼         ▼           │
│   Sales      Ops      Engineering   │
│     │         │         │           │
│     └─────────┼─────────┘           │
│               ▼                     │
│       Computer / Browser            │
│       Terminal / Files              │
│       Business Applications         │
└─────────────────────────────────────┘
```

## Why AgentOS?

Current AI agents are becoming increasingly capable at using computers, but there is still a significant gap between:

> **"AI can control a computer."**

and:

> **"AI can operate a business."**

AgentOS is designed to bridge that gap.

The long-term goal is an environment where you can delegate an objective to an AI agent and have it:

- understand the objective
- create a plan
- use your computer
- interact with websites and applications
- execute tasks
- coordinate with other agents
- remember important information
- recover from failures
- request approval for consequential actions
- verify its work
- report what happened

All while keeping the user in control.

---

# Core Principles

### 🖥️ Computer-Native

Agents should be able to interact with the same interfaces humans use.

AgentOS is designed around access to:

- Desktop applications
- Web browsers
- Terminal
- Filesystem
- APIs
- Business applications

### 🔐 User-Controlled

The AI does not own your computer.

**You do.**

Every capability is controlled by a permission system that can allow, deny, or require approval for an action.

```text
Computer
 ├── Screenshot       ALLOW
 ├── Mouse            ALLOW
 ├── Keyboard         ALLOW
 └── Applications     ASK

Filesystem
 ├── Read             ALLOW
 ├── Write            ASK
 └── Delete           DENY

Email
 ├── Read             ALLOW
 ├── Draft            ALLOW
 └── Send             ASK
```

### 🧠 Persistent

Agents need more than a conversation history.

AgentOS maintains structured state for:

- Tasks
- Memories
- Decisions
- Permissions
- Workflows
- Agent activity
- Audit history

### 🔌 Extensible

Everything an agent can do is represented as a tool.

Developers can build integrations and plugins without modifying the core agent runtime.

### 🌎 Open Source

AgentOS is designed to be genuinely open source.

The goal is for developers to build:

- Agents
- Tools
- Integrations
- Workflows
- Policies
- Automation systems

on top of the same runtime.

---

# Architecture

AgentOS is built around a modular agent runtime.

```text
┌──────────────────────────────────────────┐
│                Desktop UI                │
│             React + Tauri                │
└────────────────────┬─────────────────────┘
                     │
                     ▼
┌──────────────────────────────────────────┐
│              Agent Runtime               │
│                  Rust                    │
│                                          │
│  Planning │ Execution │ Memory │ Events │
└───────────────┬──────────────────────────┘
                │
       ┌────────┼────────┐
       ▼        ▼        ▼
   Computer   Browser  Terminal
       │        │        │
       └────────┼────────┘
                │
                ▼
        Permission Engine
                │
                ▼
        Approval / Audit
```

The runtime is intentionally separated from the desktop interface.

This allows AgentOS to eventually support:

- Desktop applications
- CLI clients
- Remote clients
- Headless agents
- Server deployments

without creating separate agent implementations.

Today the runtime and a CLI client exist. The desktop interface is next, and it will consume the
same runtime rather than reimplementing any of it.

For the detail — how the trust boundary, the policy engine, taint tracking and the audit chain
actually work — see [`docs/architecture.md`](docs/architecture.md) and the decision records in
[`docs/adr`](docs/adr).

### Crates

| Crate | Responsibility |
| --- | --- |
| `agentos-core` | Domain types, events, and the trust boundary |
| `agentos-permissions` | Policy engine and path sandboxing |
| `agentos-secrets` | OS keychain access |
| `agentos-persistence` | SQLite schema, migrations, repositories |
| `agentos-audit` | Event bus and the hash-chained append-only log |
| `agentos-tools` | Tool trait, registry, and the authorisation pipeline |
| `agentos-providers` | Anthropic, OpenAI-compatible, and mock model providers |
| `agentos-browser` | Deterministic browser automation over CDP |
| `agentos-demo` | The mock CRM and the demonstration scenario |
| `apps/desktop` | The desktop application — Tauri 2, React, TypeScript |
| `agentos-runtime` | Task state machine, agent loop, composition root |
| `agentos-cli` | The `agentos` binary |

---

# Agent Execution

Agents operate through an explicit execution lifecycle.

```text
Objective
   │
   ▼
Planning
   │
   ▼
Task Execution
   │
   ▼
Observation
   │
   ▼
Verification
   │
   ├───────────────┐
   │               │
   ▼               ▼
Approval         Failure
   │               │
   ▼               ▼
Execution       Recovery
   │               │
   └───────┬───────┘
           ▼
        Complete
```

Every significant action is observable and auditable.

Example:

```text
14:31:02  Task started
14:31:04  Browser opened
14:31:05  CRM loaded
14:31:08  Customer records retrieved
14:31:12  Follow-up candidates identified
14:31:15  Email draft created
14:31:16  Approval requested
14:31:27  User approved
14:31:29  Email sent
14:31:31  Delivery verified
14:31:32  Task completed
```

---

# Tools

AgentOS uses a tool-based architecture.

Initial capabilities include:

### Computer

- Screenshots
- Mouse control
- Keyboard input
- Clicking
- Dragging
- Scrolling

### Browser

- Navigation
- Page interaction
- Text extraction
- Screenshots
- Form interaction

### Terminal

- Command execution
- Process management
- Working directories
- Output capture
- Cancellation

### Filesystem

- Read
- Write
- List
- Copy
- Move
- Delete

All capabilities are subject to the AgentOS permission system.

**Built today:** filesystem, terminal and browser. Run `agentos tools` to see the current catalogue,
including which tools return attacker-controllable data and therefore raise the approval bar for the
rest of a run. Computer control is next.

Browser interaction is deterministic — CSS selectors over the Chrome DevTools Protocol, not
screenshots and coordinates. `browser.click #send-button` is a reviewable action in a way that
`click at (412, 908)` is not. Capabilities are scoped by origin, so an agent can be given one site
rather than the web. See [ADR 5](docs/adr/0005-deterministic-browser-automation.md).

---

# Business Operations

The long-term goal is to allow agents to operate across the systems businesses already use.

Planned integrations include:

- GitHub
- Slack
- Gmail
- Google Calendar
- Notion
- Linear
- Shopify
- Stripe
- HubSpot
- Salesforce

For example:

```text
"Prepare today's sales operations."

        ↓

Retrieve new leads
        ↓
Check CRM
        ↓
Identify overdue follow-ups
        ↓
Research prospects
        ↓
Draft responses
        ↓
Update CRM
        ↓
Request approval
        ↓
Send approved messages
        ↓
Generate report
```

---

# Multi-Agent Operations

AgentOS is designed to eventually support specialized agents working under an orchestrator.

```text
                    Orchestrator
                         │
          ┌──────────────┼──────────────┐
          ▼              ▼              ▼
       Sales           Operations    Engineering
        Agent             Agent         Agent
          │                │              │
          └────────────────┼──────────────┘
                           ▼
                     Shared Runtime
```

A high-level objective can be decomposed into smaller tasks and delegated to specialized agents.

For example:

> "Increase this month's revenue."

could eventually become:

```text
Revenue Objective

├── Identify sales opportunities
├── Follow up with existing customers
├── Analyze failed payments
├── Improve conversion funnel
├── Research new prospects
└── Report progress
```

---

# Security

Giving an AI access to a computer introduces serious security challenges.

AgentOS treats security as a core architectural concern rather than an afterthought.

The system is designed around:

- Explicit permissions
- Human approval
- Sandboxed filesystem access
- Command restrictions
- Secure credential storage
- Audit logs
- Tool argument validation
- Timeouts
- Cancellation
- Rate limiting
- Prompt-injection defenses

AgentOS treats external content as **untrusted input**.

A webpage, email, document, or application cannot redefine the agent's authority simply by instructing it to do something.

That is enforced structurally rather than by asking the model nicely:

- **The trust boundary is a type.** Operator instructions are the only trusted content. Model output
  and every tool result — without exception — are not, and there is no API that converts one into the
  other. Untrusted text is shown to the model inside a nonce-tagged envelope it cannot forge its way
  out of.
- **Authorisation never reads model output.** Permission decisions come from the operator's policy,
  the tool's declared requirements, and the run's state. A model that has been completely taken over
  can request anything and still be refused.
- **Taint tracking.** Once a run reads anything from outside, the approval bar rises for everything
  that follows. This is what makes "read a poisoned page, then exfiltrate" loud instead of silent.
- **No shell.** `terminal.exec` takes an argument vector, so `;`, `&&`, `$(...)` and globs are
  literal characters rather than an injection surface. Child processes get an environment allowlist,
  never the parent's — so an agent cannot read your API key back out through a subprocess. `.bat`
  and `.cmd` files are refused outright: they are the one case where Windows hands an argument
  vector to a shell.
- **Paths are resolved before they are checked.** `../` and symlinks are resolved to where they
  really point, including for files that do not exist yet, and only then tested for containment.
- **Agents cannot escalate.** `runtime.modify_policy`, `modify_agent`, `disable_audit` and
  `disable_approvals` are permanently denied and cannot be granted by any policy.
- **The audit log is append-only and tamper-evident.** SQLite triggers refuse `UPDATE` and `DELETE`;
  a SHA-256 chain makes edits detectable.

There is a test for the case that matters: a model scripted to obey injected instructions has every
resulting call refused, the run still completes, and every refusal is recorded.

The limits are written down honestly in [`SECURITY.md`](SECURITY.md).

---

# Local First

AgentOS is designed to run primarily on your own machine.

The goal is to keep:

- Business data
- Agent state
- Task history
- Credentials
- Memory
- Audit logs

under the user's control.

LLM providers are pluggable.

Planned providers include:

- OpenAI
- Anthropic
- Google
- OpenAI-compatible APIs
- Local models

---

# Technology

| Component  | Technology        |
| ---------- | ----------------- |
| Desktop    | Tauri 2           |
| Frontend   | React             |
| Language   | TypeScript        |
| Runtime    | Rust              |
| Database   | SQLite            |
| Build Tool | Vite              |
| CLI        | Rust              |
| LLMs       | Provider-agnostic |
| Platforms  | macOS / Windows   |

---

# Project Status

🚧 **Early development**

AgentOS is currently under active development. The architecture and APIs are expected to change.

**What works today**, tested end to end with no network and no API key required:

- The agent runtime: an explicit task state machine, the agent loop, cancellation, and recovery
- The permission engine: deny-by-default YAML policies, specificity ordering, risk ceilings,
  path sandboxing that survives `../` and symlinks
- The trust boundary and taint tracking
- Human approval as a runtime primitive — persisted, resumable, and auditable
- The append-only, hash-chained audit log
- SQLite persistence for agents, tasks, runs, traces, approvals and memory
- Filesystem, terminal and browser tools
- Model providers: Anthropic, any OpenAI-compatible endpoint (OpenAI, Ollama, LM Studio, vLLM),
  and a scripted mock
- The `agentos` CLI
- The desktop application: dashboard, the approval card, live execution traces, agent and policy
  editing, a streaming activity feed, and settings

- An end-to-end demonstration: a local mock CRM, driven by a real browser, with a prompt-injection
  payload planted in one of the customer records

**Not built yet:** computer control, the scheduler, the orchestrator, and integrations. See the
roadmap below.

The project is **not yet intended for unrestricted autonomous operation of production businesses.**

Do not give experimental agents access to sensitive production systems or financial accounts.

---

# Roadmap

Phases 1 and 4 were built together. Safety is not a feature that can be added to a runtime that
assumed it would not be needed, so the permission engine, approval gate and audit log went in
alongside the execution loop rather than after it.

## Phase 1 — Agent Runtime

- [x] Rust agent runtime
- [x] LLM provider abstraction (Anthropic, OpenAI-compatible, local, mock)
- [x] Agent lifecycle
- [x] Task execution — explicit state machine, retries, cancellation
- [x] Tool registry
- [x] Structured events
- [x] SQLite persistence
- [x] CLI client
- [x] Tauri desktop application — dashboard, approvals, tasks with live traces, agents, activity, settings

## Phase 2 — Computer Control

- [ ] macOS computer control
- [ ] Windows computer control
- [ ] Screenshots
- [ ] Mouse interaction
- [ ] Keyboard interaction
- [ ] Application interaction
- [ ] Accessibility permission detection

## Phase 3 — Browser

- [x] Browser sessions — one per run, isolated profile, closed when the run ends
- [x] Navigation
- [x] Page interaction — click, type, submit, history
- [x] Text extraction — returned as untrusted data tagged with its origin
- [x] Browser state — element inspection with stable selectors
- [x] Browser permissions — capabilities scoped by origin
- [ ] Vision fallback for pages with no usable structure

## Phase 4 — Safety *(done, ahead of phases 2 and 3)*

- [x] Permission engine — deny by default, specificity ordering, risk ceilings
- [x] Approval system — persisted, resumable, a real runtime state
- [x] Filesystem sandbox — canonical resolution, symlink- and traversal-proof
- [x] Terminal restrictions — no shell, program allowlist, environment allowlist, timeouts
- [x] Secure credential storage — OS keychain, redacted from errors and logs
- [x] Audit logs — append-only by database trigger, hash-chained, verifiable
- [x] Cancellation — from any non-terminal state, through every tool
- [x] Prompt-injection defenses — trust boundary in the type system, taint tracking

## Phase 5 — Memory & Orchestration

- [x] Persistent memory — structured, with provenance, behind a swappable interface
- [ ] Task graphs
- [ ] Task dependencies
- [ ] Scheduler
- [ ] Agent orchestration
- [ ] Multi-agent execution

## Phase 6 — Integrations

- [ ] GitHub
- [ ] Slack
- [ ] Gmail
- [ ] Google Calendar
- [ ] Notion
- [ ] Linear
- [ ] Shopify
- [ ] Stripe
- [ ] HubSpot
- [ ] Salesforce

## Phase 7 — Agent Ecosystem

- [ ] Plugin SDK
- [ ] Agent SDK
- [ ] Tool marketplace format
- [ ] Community agents
- [ ] Community integrations
- [ ] Community workflows

---

# Development

## Requirements

- [Rust](https://rustup.rs) 1.85 or newer — for the runtime and the CLI
- [Node](https://nodejs.org) 20 or newer — only for the desktop application

The database is embedded, and the test suite needs no network, no API key and no external service.

## Getting Started

```bash
git clone https://github.com/anpl1623/AgentOS.git
cd AgentOS
cargo build --release
```

```bash
./target/release/agentos doctor
```

Store a provider key. It goes into your operating system's keychain — never the database, never a
config file, never a log line:

```bash
./target/release/agentos provider set-key anthropic
```

On a machine with no keychain — a headless server, a container, CI — export it instead. `agentos
doctor` tells you which applies and shows where a credential is actually coming from:

```bash
export ANTHROPIC_API_KEY=…
```

Create an agent. Its starter policy denies everything except reading inside its own workspace:

```bash
./target/release/agentos agent create --name sales --tool filesystem.read --tool filesystem.list
```

Look at what it is allowed to do *before* you give it work:

```bash
./target/release/agentos policy show sales
```

Give it something to do:

```bash
./target/release/agentos task run "Summarise the files in my workspace." --agent sales
```

Then inspect what actually happened, and confirm the record has not been altered:

```bash
./target/release/agentos audit tail --security
```

```bash
./target/release/agentos audit verify
```

No provider key yet? Pass `--provider mock` when creating the agent to exercise the whole pipeline
without a network call.

## See the whole thing work

```bash
./target/release/agentos demo --scripted
```

This starts a mock CRM on loopback, gives an agent a policy scoped to it, and turns it loose with a
real browser. One of the customer records contains text impersonating a system message, instructing
the agent to read a private key, upload it, and delete a directory.

The interesting output is not that the agent read a website. It is the list of things it was refused
afterwards — and that none of those refusals depended on the model noticing anything was wrong.

`--scripted` needs no API key: it replays a fixed model transcript through the real runtime, so the
permission decisions you see are real ones. Drop the flag to run it against a configured provider,
and add `--headed` to watch the browser work.

## The desktop application

```bash
cd apps/desktop
npm install
npm run tauri dev
```

It is a client of the same runtime the CLI uses — it holds no agent logic of its own, and the two
cannot disagree about what an agent may do. Its TypeScript types are generated from the Rust view
models, so a change on one side fails to compile on the other.

Running `npm run dev` alone opens the interface in an ordinary browser against fixture data, which is
useful for working on a screen without launching the whole application. Those fixtures are
development-only and are removed from a production build.

## Commands

| Command | Purpose |
| --- | --- |
| `agentos doctor` | Check the installation and report what is missing |
| `agentos agent create \| list \| show \| set` | Manage agents |
| `agentos policy show \| set \| validate` | Inspect and edit permission policies |
| `agentos task run \| list \| show \| cancel` | Run and inspect tasks |
| `agentos audit tail \| verify` | Read the log and check its integrity |
| `agentos provider list \| set-key \| remove-key` | Manage credentials |
| `agentos demo` | Run the end-to-end demonstration against a local mock CRM |
| `agentos tools` | See what the runtime can offer an agent |

## Policies

Policies are YAML, deny by default, and enforced by the runtime rather than described to the model:

```yaml
default: deny
max_risk: high

taint_escalation:
  enabled: true
  escalate_at_or_above: medium

permissions:
  filesystem:
    read:  ["~/Documents/Sales"]
    write:
      effect: ask
      paths: ["~/Documents/Sales"]

  terminal:
    exec:
      effect: ask
      programs: [git, npm]

  browser:
    navigate:
      effect: allow
      origins: ["https://*.example.com"]

  payments:
    execute: deny
```

Conflicts resolve by specificity, and ties go to the stricter effect — a contradictory policy fails
closed.

## Tests

```bash
cargo test --workspace
```

Before opening a pull request:

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

The security-relevant tests live alongside the code they protect and cover sandbox escapes via `../`
and symlinks, shell metacharacters proving inert, an agent being unable to grant itself permissions,
a fully hijacked model having every request refused, and audit tampering being detected.

The end-to-end browser tests drive a real Chromium against the mock CRM. They skip, loudly, if no
browser is installed — a skipped test that says so is honest; one that quietly passes is not.

---

# Contributing

AgentOS is being built in the open.

Contributions are welcome across:

- Rust
- React
- AI/LLM infrastructure
- Browser automation
- Computer interaction
- Security
- Agent architectures
- Integrations
- Documentation
- Testing

Before submitting a large change, please open an issue or discussion to explain the proposed architecture.

See:

- `CONTRIBUTING.md`
- `CODE_OF_CONDUCT.md`
- `SECURITY.md`

for more information.

---

# Building Agents

The long-term goal is to make building an AgentOS agent as simple as defining:

```text
Objective
Instructions
Tools
Permissions
Memory
```

Developers should be able to build specialized agents without needing to understand the internals of the computer-control runtime.

Example future agent:

```text
SalesAgent

Tools:
  browser
  crm
  email

Permissions:
  CRM.read      → allow
  CRM.write     → allow
  Email.draft   → allow
  Email.send    → ask
  Payments      → deny
```

---

# Philosophy

AgentOS is based on a simple idea:

> **AI should not just answer questions. It should be able to do meaningful work.**

But autonomy without control is dangerous.

The goal is therefore not:

> Give an AI unrestricted access to your computer.

It is:

> **Give an AI controlled access to your computer and let the user decide what it is allowed to do.**

The result should feel less like chatting with an AI and more like **delegating work to a highly capable digital employee.**

---

# License

AgentOS is open source.

See `LICENSE` for the current license and terms.

---

## ⭐ Star the Project

If you're interested in the future of autonomous AI agents, computer-use agents, and open-source AI infrastructure, consider starring the repository and following development.

Contributions, ideas, security research, and experiments are welcome.

**Build the agent. Give it tools. Keep the human in control.**
