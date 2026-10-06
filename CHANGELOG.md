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
- **Standing instructions are audited.** Creating, pausing, resuming and deleting a schedule,
  queuing a task or making one wait for another, recording, revising or forgetting a memory, and
  starting or stopping a scheduler are `operator.*` records on the security record, from the CLI and
  the desktop alike. A memory a person records is always recorded as theirs: no client can name
  another source for it, and revising an untrusted memory leaves it untrusted.
- **One task, one run, however many clients.** A run claims its task with a compare-and-set against
  the status the client read, before it writes anything, so two schedulers, or a scheduler and a
  person pressing retry, can no longer start one task twice, and a schedule's occurrence fires once.
  A scheduler's read made before the operator cancelled a task, or before the task was told to wait
  for another, starts nothing. Running a task that succeeded again is refused by the runtime, not
  only by the desktop. A new task is written in one transaction with what it waits for, so no
  scheduler can start it before it has been told to wait, and a claim is written in one transaction
  with its run, so a crash between the two can no longer leave a task running that nothing will
  ever start or reap.
- **One scheduler per installation.** A running scheduler holds `scheduler.lock` in the data
  directory. `agentos schedule run`, `--once` included, refuses to start while the desktop's
  scheduler or another one holds it. Both take the lease and record the start before the first tick,
  so nothing a scheduler starts precedes the record that it was running.
- **The desktop's scheduler starts again on launch if it was left on, and asks no one.** It runs
  behind the gate that refuses every approval, with no setting to change that; whatever would have
  asked a person is refused with a note the model can plan around. A start on launch is recorded as
  one. Saving the same pacing again leaves a running scheduler alone, and new pacing that would
  restart it while its runs are in progress is refused rather than allowed to stop them.
- **Closing the desktop says what it will stop.** Closing the window, or quitting with Cmd+Q, while
  runs are live, the scheduler is on or approvals are waiting asks first. However the application
  exits, live runs are then cancelled and recorded as cancelled rather than left for the next launch
  to fail. A window whose interface does not acknowledge the question within three seconds closes
  anyway, so a crashed interface cannot leave a window that will not close.
- **An approval card shows what will run.** Direction controls and invisible characters, including
  every default-ignorable code point a webview draws at zero width, are drawn as `⟨U+…⟩` and counted
  in the arguments and in the card's summary, reason, affected resources and taint sources alike;
  Copy JSON writes them as `\u` escapes. A string argument is drawn in quotes, so `"false"` cannot
  pass for `false`. A pending request is never folded, and a value whose lines or blank stretches
  push its tail out of sight says so above the block.
- **Two processes writing the audit log no longer lose records.** The writer that loses a position
  in the chain re-reads the tip and reseals after the winner. A record the process still fails to
  write is counted, and the desktop's chain health reports the count rather than calling an
  incomplete log intact.
- **`network.request`: one audited way out, priced by what it carries.** Each call is scoped to the
  canonical origin it goes to, and is a `fetch` (medium) only for `GET`, `HEAD` or `OPTIONS` with no
  body, at most 256 bytes of path, query and fragment, and at most 256 bytes of custom headers.
  Anything else is a `send` (high), so a policy can let an agent read an API without letting it write
  to one, and a kilobyte in a query string or a header is priced as the upload it is. Headers that
  override the method are refused outright, as are `Authorization`, cookies, `Host` and the framing
  headers. Redirects are not followed: a 3xx comes back as the result with its `Location`, and going
  there is a second request with its own decision. Nothing sets the tool's address policy but the
  runtime, which builds it strict.
- **`network.request` refuses every address that is not public, and connects only to what it
  checked.** Every address a name resolves to is checked, not the first, after reducing IPv6 forms
  that embed an IPv4 address (mapped, compatible, translated, NAT64, 6to4) to that address. Loopback,
  private, link-local (the cloud metadata address among them), carrier-grade NAT, unique-local,
  multicast, documentation and benchmarking addresses, and any IPv6 address outside `2000::/3`,
  refuse the call whatever the policy allows. The connection is then pinned to the addresses that
  were checked, so a second DNS answer cannot move it, and proxy variables in the environment are
  ignored.
- **Network credentials are bound to an origin and spent as a separate grant.** A request names a
  stored credential and never carries one; the value is looked up for the URL's own origin, so a
  credential stored for one origin cannot be sent to another, and a call is given only the
  credentials its authorised plan named. Spending one needs `network.credential` on
  `{origin}/{name}` as well as the request's own grant, and raises the call's risk a level. The value
  never enters the arguments, the plan, the approval card or the audit chain. Anything a call
  returns, on success or failure, that contains a credential the run has released, or a run of eight
  or more of its bytes, reads `[redacted credential]` before the model or the audit log sees it; the
  body is redacted before it is cut to the caller's `max_bytes`, and a credentialed request cannot
  ask for a `Range` of the response. `Set-Cookie` is never shown.
- **Credential use and credential changes are audited.** Each release of a credential to a call is
  recorded as `network.credential.used`, naming the origin, the name and the tool, at the moment of
  release and before the tool holds the value; a credential whose use cannot be recorded is not
  released. Storing and removing one are `operator.credential.set` and `operator.credential.removed`.
  All three are security-relevant and none carries the value.
- **A browser navigation that carries something is priced as a send.** `browser.navigate` to a URL
  with a query or a fragment, or with more than 256 bytes of path, query and fragment, is high risk
  rather than medium, and the approval card shows the decoded query, fragment or path. **Some
  navigations that ran without asking under a medium ceiling or an `ask` above medium will now be
  refused or put to a person.**
- **The browser does not stay on a page it was sent to from another origin.** A navigation that ends
  on another origin, by an HTTP redirect, a meta refresh or a script run on load, now fails naming
  where it landed, and the browser leaves the page; that origin needs a navigation of its own. A page
  that moves itself to another origin later is not acted on: every browser tool that acts on the
  current page checks, when it runs, that the page is still on the origin it was authorised for.

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
- Desktop: the scheduler runs inside the application. Turning it on is remembered for the next
  launch, and each start and stop is recorded, with whether the start came from the launch.
- Desktop: approval cards are ordered most dangerous first. Approve takes two presses, as Deny does,
  either can carry a note, and no key answers a card. A risky or tainted request offers "Deny and
  stop the run". A new request is announced in one line to a screen reader, not read out card and
  all.
- Desktop: the dashboard opens with what needs you, what failed recently and what was refused, each
  row opening its run; a Tools panel shows refusals against runs over 7 or 30 days; a setup block
  says what is missing before anything can work.
- Desktop: Activity's Security only asks the log itself, the feed filters by text and kind, and a
  stored event opens its audit record with both hashes; closing it returns to its row. Copy visible
  writes JSON Lines.
- The desktop's command surface gains schedules and cadence checks, memories, queued tasks and the
  task graph, and a report of how far each granted tool's policy reaches, computed by the permission
  engine. The report errs towards saying a tool can do more than it can, never less, and its
  documentation lists where. Their screens follow.
- `network.request`, for HTTP requests to a named origin, with a `network` block in the policy
  vocabulary (`fetch`, `send`, `credential`) and a commented example in the starter policy. It is
  registered for every runtime and granted to no agent until someone adds it.
- `agentos credential set <origin> <name>`, `agentos credential list` and
  `agentos credential remove <origin> <name>`. The secret is read from a prompt that does not echo,
  or from standard input, never from the command line; the list shows origins and names only.
- Desktop: a run has its own screen. It merges steps and tool calls into one timeline with times,
  can be read by calls or by steps, anchors a failure to what produced it, shows where the time went
  and the time spent waiting on you, lists the run's audit records with both hashes, switches between
  attempts, and offers Retry, Stop this agent and Copy this run. It refreshes on the run's own
  events, with a slow reload behind them.
- Desktop: Tasks has a Queued panel saying what each waiting task is waiting for, and a queued task
  or a new one can be made to wait for another; a refused edge shows the runtime's reason.
- Desktop: an agent's page shows how far the policy reaches for each granted tool, as the permission
  engine reads it, and marks a tool no rule allows. Its memory can be added to, edited and forgotten,
  and a policy that does not check shows the engine's message as written.
- Desktop: Schedules lists standing schedules with their next and last firing, creates one with the
  cadence checked as it is typed, and pauses, resumes or deletes it.
- Desktop: Settings stores and removes network credentials, listing each as origin and name and
  never its value; runs and stops the scheduler, saying in plain words when another process holds
  it; and lists the tool catalogue by domain with the agents each tool is granted to. Removing a
  provider key asks first.

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
- `agentos task create --at` now combines with `--depends-on`; the task waits for both.
- A queued task whose agent is disabled, or whose provider cannot be built, is failed once rather
  than tried again on every tick. The dashboard says so of a failed task that never ran, rather
  than blaming a dependency.
- The task graph names the same failed dependency the scheduler names when it abandons a task that
  waits on two.
- `Runtime::create_task` takes dependencies and a time, and replaces `create_task_after` and
  `create_task_at`; `add_task_dependency` and `set_schedule_paused` replace `add_dependency`,
  `pause_schedule` and `resume_schedule`. Each records its change.
- `ToolContext` carries the capabilities a call was authorised against.
- Browser tools that act on the current page refuse to run outside the tool pipeline, which is what
  tells them the origin they were authorised for.
- `browser.navigate` watches a page for half a second after it loads before it returns.

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
