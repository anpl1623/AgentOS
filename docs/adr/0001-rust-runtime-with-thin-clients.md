# 1. A Rust runtime with thin clients

- **Status:** accepted
- **Date:** 2026-08-20

## Context

AgentOS needs a desktop application, and the specification frames the first milestone around one. It
also requires that the system remain usable without that application, with a CLI as a peer client and
no second implementation behind either.

Those two pull in opposite directions if the UI is built first: the runtime becomes whatever the UI
happens to need, and the CLI arrives later as a shim over UI-shaped abstractions.

## Decision

The agent runtime is a Rust library crate. Every client — the CLI today, a Tauri desktop application
next, potentially a local HTTP API later — is a thin consumer of it holding no agent behaviour of its
own.

The CLI is built first, and the desktop application comes after the runtime is stable.

## Consequences

The runtime API gets exercised by a real client from day one, which is a better design pressure than
a hypothetical one. `agentos-cli` contains only argument parsing, terminal rendering and calls into
`agentos-runtime`; the moment logic starts accumulating there it is visible in review.

Headless operation — cron, CI, servers — works for free rather than being retrofitted.

The cost is that there is no graphical interface yet, which makes the project harder to evaluate at a
glance. Accepted: a demo built on an unstable runtime would have been rewritten anyway.

## Rejected

**Building the desktop application first.** Would have inverted the dependency the specification
explicitly forbids, and guaranteed a rewrite of whatever runtime emerged.

**Electron or a web stack for the core.** Computer control, subprocess management and filesystem
sandboxing all want native access, and a permission system implemented in the same runtime the agent
code runs in is a weaker boundary.
