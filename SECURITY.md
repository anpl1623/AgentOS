# Security policy

AgentOS runs AI agents against a real computer with real credentials. Security reports are welcome
and taken seriously.

## Reporting a vulnerability

Please **do not** open a public issue for a vulnerability.

Use [GitHub's private vulnerability reporting](https://github.com/anpl1623/AgentOS/security/advisories/new)
on this repository. Include what you found, how to reproduce it, and what an attacker could achieve.
A working proof of concept is helpful but not required.

You can expect an acknowledgement within a few days and an assessment shortly after. If a fix is
warranted we will coordinate disclosure with you and credit you unless you would rather we did not.

## Threat model

AgentOS assumes:

- **Any content an agent reads is hostile.** Webpages, files, emails, command output, API responses,
  window titles, the pixels in a screenshot, and anything a plugin returns. Text that appears to give
  the agent instructions is treated as data reporting what somebody wrote, never as an instruction.
- **The model may be fully compromised.** Not "might be nudged" — assume it has been persuaded and is
  now issuing an attacker's tool calls verbatim. Every control must hold anyway. There is a test for
  exactly this scenario.
- **The operator is trusted.** Someone who can edit the policy file can grant broad access, and that
  is their prerogative. AgentOS's job is to make what they granted visible and to stop the agent
  exceeding it.
- **The local machine is trusted.** Full disk access defeats most local software, this included. The
  audit chain makes tampering detectable, not impossible.

## What is enforced, and where

| Property | Mechanism |
|---|---|
| Agents cannot exceed their policy | `agentos-permissions`, consulted by the runtime, never by the model |
| Paths cannot escape their scope | Canonical resolution before the containment check, so `../` and symlinks resolve first |
| Shell injection | No shell — `terminal.exec` takes an argv vector, so metacharacters are literal |
| Environment leakage | Child processes get a per-platform allowlist of paths and machine properties, never the parent environment |
| Agents cannot escalate | `runtime.modify_policy`, `modify_agent`, `disable_audit`, `disable_approvals` are permanently denied |
| Consequential actions | Policy `ask` effects route through a human approval gate before any side effect |
| Untrusted input | Taint tracking raises the approval bar for the rest of the run |
| Audit integrity | Append-only SQLite triggers, plus a SHA-256 hash chain over a canonical timestamp format |
| Credentials | OS keychain where available; never in the database, never logged, redacted from provider errors |
| Batch files | `terminal.exec` refuses `.bat`/`.cmd`, the one path where Windows hands an argv to a shell |
| Input goes where it was authorised | Every `computer.*` call names the application it targets, which must be the one in front — re-checked before every individual event |
| Agents cannot approve themselves | The computer tools refuse to send input to AgentOS's own process, whatever the policy says |

The load-bearing point: **permission decisions are computed from the policy, the tool's declared
requirements and the run's taint state. Model output is not an input.** A model that has been
completely taken over can ask for anything and still be refused.

## What is not protected

Being explicit about the gaps is more useful than implying there are none:

- **A permissive policy.** If you grant `terminal.exec` on `*`, an agent can do whatever your user
  account can. The starter policy is deliberately restrictive; widening it is your decision.
- **Approval fatigue.** An operator who approves without reading has defeated the approval system.
  The card shows what will happen and flags tainted runs precisely to make reading worthwhile.
- **The model provider.** Conversations — including untrusted content the agent read — are sent to
  whichever provider you configured. Use a local one if that matters to you.
- **Local disk access.** An attacker who can write to `~/.agentos` can rewrite the whole audit log.
  The hash chain detects partial edits; it does not prevent a wholesale rewrite.
- **Side channels within an allowed scope.** An agent permitted to write to a directory you sync to
  the cloud can exfiltrate through it. Scope grants to what the task needs.
- **Synthetic input is not scopable to an action, and the check on it is racy.** Naming the target
  application binds *who receives* a keystroke. It cannot bind *what the keystroke does*: at the
  policy layer, Return on a focused dialogue, Cmd-Q, and the letter `a` are one capability on one
  resource. The only distinction AgentOS can draw is whether a keystroke commits — a newline, or
  Return — which it prices as a higher risk, the way `browser.type` prices `submit`. And the check is
  against a moving target: the operating system routes an injected event when it delivers it, after
  the call returns, so re-checking immediately beforehand narrows the window to one event delivery
  without closing it. A multi-character `type` is one event per character; focus is re-read before
  each, so a change part-way through stops the rest, and what already landed cannot be recalled. The
  honest summary: **anything you can do with a keyboard, an agent holding `computer.type` can do,
  including typing a shell command into a terminal window — where the shell gets your full
  environment rather than the allowlist `terminal.exec` would have given it.** Grant it the way you
  would grant `terminal.exec` on `*`, not the way you would grant `browser.type`.
- **A screenshot has no scope the policy can express.** A window capture is at least attributable to
  an application; a display capture is whatever is on that display — a password manager, an open
  private key, a two-factor code in a notification banner. Path scoping, origin scoping and the
  keychain protect data at rest and in transit, and none of them protects pixels. The audit log
  records that a capture happened and what it was of; it cannot record what was in it. Screen reads
  do at least raise taint, so what an agent does *after* looking is held to a higher bar.
- **Showing a capture to a model sends it off the machine.** `computer:vision` and `browser:vision`
  are separate capabilities from taking the capture, and both default to ungranted, precisely because
  the two acts differ: saving a screenshot writes a file you own, attaching one transmits your screen
  to whoever serves your model. Everything in the bullet above about what a display capture contains
  applies again, to a third party's servers and their retention policy. Grant `vision` scoped to the
  narrowest application or origin that does the job, and prefer a window capture over a display one.
  Nothing in the runtime can un-send an image.
- **An application's name is its own claim.** `application: Mail` on an approval card is what the
  process in front reports it is called, not a verified identity. It is the first resource kind in
  AgentOS whose value is not resolved by the runtime — a path is canonicalised, an origin comes from
  a browser AgentOS launched — and globs make it worse, since `Mail*` matches `Mail Stealer`. Any
  process can also raise itself to the front, so "Mail is in front" is a condition an attacker who
  already has code on the machine can arrange.
- **A coordinate is not reviewable.** `click (412, 908) in "Mail"` is everything the approval card
  can say, and it is not enough to decide anything with. [ADR 5](docs/adr/0005-deterministic-browser-automation.md)
  settled this for the browser by refusing to interact by coordinate at all; computer control has no
  such option, which is why the browser tools exist and should be preferred whenever the target is a
  web page.
- **Credentials in the environment.** A machine with no OS keychain — a headless server, a container,
  CI, some WSL setups — has nowhere secure to keep a key, so AgentOS falls back to reading
  `ANTHROPIC_API_KEY` and friends from the environment. Anything that can read the process
  environment can read those. The agent itself cannot: `terminal.exec` gives child processes a fixed
  allowlist (`PATH`, `HOME`, `LANG`, `LC_ALL`, `TZ`, `TMPDIR`) rather than the parent environment, so
  it cannot read a key back out through a subprocess. Prefer the keychain where you have one; the
  fallback exists because "unusable on a server" is not an acceptable security posture either.

## Reporting scope

In scope: sandbox escapes, policy bypasses, approval bypasses, credential leakage, audit tampering
that goes undetected, and injection that changes what the runtime permits.

Out of scope: a model behaving badly within permissions it was actually granted; the consequences of
a deliberately permissive policy; denial of service against your own machine.
