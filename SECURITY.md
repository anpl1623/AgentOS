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
  and anything a plugin returns. Text that appears to give the agent instructions is treated as data
  reporting what somebody wrote, never as an instruction.
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
| Environment leakage | Child processes get an allowlist, never the parent environment |
| Agents cannot escalate | `runtime.modify_policy`, `modify_agent`, `disable_audit`, `disable_approvals` are permanently denied |
| Consequential actions | Policy `ask` effects route through a human approval gate before any side effect |
| Untrusted input | Taint tracking raises the approval bar for the rest of the run |
| Audit integrity | Append-only SQLite triggers, plus a SHA-256 hash chain |
| Credentials | OS keychain only; never in the database, never logged, redacted from provider errors |

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

## Reporting scope

In scope: sandbox escapes, policy bypasses, approval bypasses, credential leakage, audit tampering
that goes undetected, and injection that changes what the runtime permits.

Out of scope: a model behaving badly within permissions it was actually granted; the consequences of
a deliberately permissive policy; denial of service against your own machine.
