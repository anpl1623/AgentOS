# Changelog

Notable changes to AgentOS. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the major version is `0`, a minor bump may break things. The security properties in
[`SECURITY.md`](SECURITY.md) are the part that will not be broken quietly: any change to what a
policy permits, to the trust boundary, or to the audit chain gets its own entry here, whether or not
it is technically a breaking change.

## [Unreleased]

### Security

- **Taint follows provenance, not a flag.** A tool used to decide for itself whether its output
  tainted the run, through `returns_untrusted_data`. That field is now catalogue metadata with no
  effect on authorisation. A call taints the run when the bytes it returned came from outside the
  trust boundary, or when its plan holds a capability that reads — whatever the tool says about its
  own output. A failed read taints the run too, because its error text reaches the model.
- **Taint survives the run boundary.** A retry starts from every source an earlier attempt at the
  same task recorded, and a memory written from an external source taints the next run before its
  first tool call. Both used to start clean. When taint is why a person is being asked, the approval
  reason now names where the data came from.
- **Origin rules bind every spelling, and a deny now covers every port.** Requests and policy
  patterns are both reduced to a canonical origin: lower-case scheme and host, no default port. A
  pattern without a port now means the default port only; write `:*` for any port. Previously a host
  wildcard ran on into the port, so `https://*` admitted `https://evil.example:8443` past a deny of
  `https://evil.example`. **A policy that relied on a wildcard reaching a non-default port must now
  say `:*`.** IPv4 addresses written as IPv6 (`[::ffff:127.0.0.1]`) and the unspecified addresses
  (`0.0.0.0`, `[::]`) are refused, because each reaches this machine under a rule that names neither.
- **Manifest drift is audited.** A tool that plans a capability its manifest does not declare is
  recorded as `tool.manifest_exceeded`. The call is not failed — the policy engine evaluates the real
  plan either way — but the drift is now in the audit chain rather than only in a log line.
- **Operator changes are audited.** Creating an agent, installing a policy, enabling or disabling an
  agent, and storing or removing a provider key are recorded in the hash chain as `operator.*`
  events, from the CLI and the desktop alike, and are security-relevant. A policy record carries the
  whole document, so a policy widened before a bad action and narrowed after it leaves a trace. A
  provider record names the provider and never the key.
- **A run may ask a person at most ten times by default.** The eleventh request in a run is denied
  without being shown to anyone. Set the limit with `max_per_run` in the policy's new
  `approval_budget` block, or lift it with `max_per_run: ~`. A request past the budget is recorded as
  denied with the count and the budget in its note, and its `approval.denied` record carries
  `over_budget: true`, so the chain tells the budget's refusal from a person's. Requests the CLI's
  `--auto-approve-up-to` settles without a prompt do not count. There is no bulk approve.
- **A search is bound by every rule inside its root.** `filesystem.search` asks the policy about each
  path it walks into, with the request shape that authorised the call, and only an outright allow
  lets it name or read that path. A deny on a subdirectory therefore binds the walk, and an approved
  search does not reach paths an `ask` rule covers. Symlinked directories are never entered, and a
  file is read only if the handle opened is the file that was admitted, so a path swapped for a link
  mid-search is skipped; on Windows that check is weaker, as `SECURITY.md` says.
- **Approvals left by a crashed run are closed.** At startup a pending request whose run has ended is
  marked expired with a note, rather than waiting in the queue forever. Requests of reaped runs are
  now expired rather than cancelled.
- **A live run is never reaped by another process.** The desktop at launch and `agentos doctor` used
  to fail every unfinished run, including one waiting at a terminal in another process, and a yes
  given there afterwards was still acted on. A process driving runs now holds a lock in the data
  directory (`runs.lock`), and nothing is reaped while any process holds it. Independently, an
  approval whose decision cannot be recorded, because the request was closed meanwhile or the write
  failed, is not acted on.
- **A log missing its oldest records no longer verifies.** `agentos audit verify`, `agentos doctor`
  and the desktop required nothing of the first record, so deleting the head of the chain passed.
  The first record must now be sequence 1 and name the genesis hash. The desktop's full verification
  also checks the records since its last routine check against what that check proved, and its
  verdict no longer clears a break the routine check found.
- **Decision notes are bounded.** A note is stripped of control characters other than line breaks,
  terminal escape sequences included, and cut to 2,000 characters before it reaches the approvals
  table or the audit chain, from any client. The desktop refuses a longer note rather than cutting
  it.

### Added

- `filesystem.search`, by name glob and by content, with depth, result and byte caps and a count of
  everything skipped and why. New agents get it with `filesystem.read` and `filesystem.list`.
- `filesystem.read` takes `offset` (from 1) and `limit` for a range of lines.
- An approval can carry a note either way it is answered, kept with the decision and in the audit
  record. The approval card says what the policy alone would have decided, and where the run is in
  its budget.
- Desktop: every screen has an address, back and forward work, a command palette (Cmd/Ctrl+K) and
  screen shortcuts (Cmd/Ctrl+1–7), none of which can answer an approval. Leaving unsaved work asks
  first, a screen returned to opens where it was left, and a failed run raises an alert that opens
  it. The dock badge counts waiting approvals, and a new request asks for attention when the window
  is behind another.

### Changed

- The `schedule_fired` and `task_abandoned` event tags are now `schedule.fired` and
  `agent.task.abandoned`, matching every other event and their own `kind`. Records written under the
  old tags are still read.
- The scheduler abandons a whole dead dependency chain in one tick, rather than one layer per tick.
- `agentos audit tail --security` selects security records in the query, so it shows the most recent
  of them rather than those among the newest `--limit` records.
- The desktop checks the audit chain incrementally, verifying only records written since its last
  check; the first check after launch covers the whole chain, and the full verification in Settings
  is unchanged. When a check fails, the dashboard says so instead of keeping its last verdict up. The
  runtime refuses provider keys for unknown providers.
- The dashboard lists failed tasks by when they last failed, so a retry that failed again today is
  not hidden behind newer tasks that failed earlier.
- The minimum supported Rust version is 1.94, which the database driver already required; the
  stated 1.85 was out of date.

## [0.2.0]

### Added

- **Schedules.** A standing instruction to give an agent the same objective on a cadence — once, a
  fixed interval, or a cron expression read against UTC or local time. Each firing creates its own
  task. `agentos schedule create | list | pause | resume | delete | run`.
- **Task graphs.** Tasks can wait for other tasks. A DAG rather than a tree, with cycles refused when
  an edge is written and the whole path named in the error. `agentos task create --depends-on`.
- **A scheduler.** Fires due schedules, starts tasks whose dependencies have succeeded, and cancels
  branches whose dependency failed rather than leaving them waiting.
- **An install script.** `scripts/install.sh` fetches the CLI build for the current platform from
  the release, verifies the published checksum before unpacking it, and installs to
  `~/.local/bin` without root. `AGENTOS_INSTALL_DIR` and `AGENTOS_VERSION` override where and
  which.

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

[Unreleased]: https://github.com/anpl1623/AgentOS/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/anpl1623/AgentOS/releases/tag/v0.2.0
[0.1.0]: https://github.com/anpl1623/AgentOS/releases/tag/v0.1.0
