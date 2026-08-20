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
| LLMs       | Provider-agnostic |
| Platforms  | macOS / Windows   |

---

# Project Status

🚧 **Early development**

AgentOS is currently under active development.

The architecture and APIs are expected to change while the core runtime is being established.

The current priority is building a reliable foundation for:

1. Agent runtime
2. Tool execution
3. Computer control
4. Browser automation
5. Permissions
6. Human approval
7. Persistent state
8. Audit logging

The project is **not yet intended for unrestricted autonomous operation of production businesses.**

Do not give experimental agents access to sensitive production systems or financial accounts.

---

# Roadmap

## Phase 1 — Agent Runtime

- [ ] Tauri desktop application
- [ ] Rust agent runtime
- [ ] LLM provider abstraction
- [ ] Agent lifecycle
- [ ] Task execution
- [ ] Tool registry
- [ ] Structured events
- [ ] SQLite persistence

## Phase 2 — Computer Control

- [ ] macOS computer control
- [ ] Windows computer control
- [ ] Screenshots
- [ ] Mouse interaction
- [ ] Keyboard interaction
- [ ] Application interaction
- [ ] Accessibility permission detection

## Phase 3 — Browser

- [ ] Browser sessions
- [ ] Navigation
- [ ] Page interaction
- [ ] Text extraction
- [ ] Browser state
- [ ] Browser permissions

## Phase 4 — Safety

- [ ] Permission engine
- [ ] Approval system
- [ ] Filesystem sandbox
- [ ] Terminal restrictions
- [ ] Secure credential storage
- [ ] Audit logs
- [ ] Cancellation
- [ ] Prompt-injection defenses

## Phase 5 — Memory & Orchestration

- [ ] Persistent memory
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

- Node.js
- pnpm
- Rust
- Tauri prerequisites
- macOS or Windows

## Getting Started

```bash
git clone https://github.com/anpl1623/agentos.git
cd agentos

pnpm install
pnpm dev
```

> The exact setup commands may change during early development.

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
