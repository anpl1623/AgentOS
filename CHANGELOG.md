# Changelog

Notable changes to AgentOS. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the major version is `0`, a minor bump may break things. The security properties in
[`SECURITY.md`](SECURITY.md) are the part that will not be broken quietly: any change to what a
policy permits, to the trust boundary, or to the audit chain gets its own entry here, whether or not
it is technically a breaking change.

## [Unreleased]

### Added

- **Schedules.** A standing instruction to give an agent the same objective on a cadence — once, a
  fixed interval, or a cron expression read against UTC or local time. Each firing creates its own
  task. `agentos schedule create | list | pause | resume | delete | run`.
- **Task graphs.** Tasks can wait for other tasks. A DAG rather than a tree, with cycles refused when
  an edge is written and the whole path named in the error. `agentos task create --depends-on`.
- **A scheduler.** Fires due schedules, starts tasks whose dependencies have succeeded, and cancels
  branches whose dependency failed rather than leaving them waiting.

### Security

- A scheduled run happens with nobody present, so it is driven behind a gate that **refuses every
  approval**. Anything the policy permits outright proceeds; anything that would have asked a person
  is denied with a note the agent can read. There is no setting that changes this, which means the
  policy is the whole of the control for unattended work. See [`SECURITY.md`](SECURITY.md).

## [0.1.0]

The first release. Runtime, safety, browser, computer control, CLI and desktop application.

### Added

**The runtime.** An explicit task state machine with retries, cancellation from any non-terminal
state, and recovery. A tool registry. Structured events. SQLite persistence for agents, tasks, runs,
traces, approvals and memory. Model providers for Anthropic, any OpenAI-compatible endpoint (OpenAI,
Ollama, LM Studio, vLLM) and a deterministic mock the whole test suite runs against.

**Safety, built alongside the execution loop rather than after it.** A deny-by-default policy engine
with specificity ordering and risk ceilings. A filesystem sandbox that resolves canonically and
survives `../` and symlinks. Terminal restrictions: no shell, a program allowlist, an environment
allowlist and timeouts. Credentials in the OS keychain, redacted from errors and logs. Human approval
as a persisted, resumable runtime state. An append-only audit log, made append-only by database
trigger and hash-chained so tampering is detectable. A trust boundary in the type system, with taint
tracking that raises the approval floor for the rest of a run once anything external has been read.

**Tools.** Filesystem and terminal. Browser automation over the Chrome DevTools Protocol —
deterministic and DOM-based, one isolated profile per run, every capability scoped by origin.
Computer control on macOS and Windows — screenshots, mouse, keyboard, and application interaction
scoped to whatever is in front. Vision: `computer.screenshot` and `browser.screenshot` can show the
model what they captured, behind the separate `computer:vision` and `browser:vision` capabilities.

**Clients.** The `agentos` CLI. A Tauri desktop application with six screens: dashboard, approvals,
tasks with live traces, agents, activity and settings.

**A demonstration.** A mock CRM on loopback, driven by a real browser, with a prompt-injection
payload planted in one customer record. The end-to-end test scripts the model to fall for it
completely and asserts that every resulting call is refused.

### Security

Documented in [`SECURITY.md`](SECURITY.md): what AgentOS defends against, and — more usefully — the
things it does not. Screen captures have no scope a policy can express, an application's name is its
own claim, and `computer.type` is as powerful as a keyboard.

[Unreleased]: https://github.com/anpl1623/AgentOS/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/anpl1623/AgentOS/releases/tag/v0.1.0
