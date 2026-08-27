# 6. Computer control is scoped to the application in front

- **Status:** accepted
- **Date:** 2026-08-20

## Context

Every other capability in AgentOS has a resource the runtime can resolve and then check. A path is
canonicalised before the policy sees it. An origin comes from a browser AgentOS launched itself.
Resolve, then check, then act — that ordering is what makes the permission engine mean anything.

Synthetic input has nothing like it. A keystroke goes wherever keyboard focus is, and what it does
there depends on what is underneath. A screenshot reads whatever happens to be on the display. The
obvious shapes both fail:

- **Unscoped.** `computer: { type: allow }` is "this agent may type", which is indistinguishable from
  handing over the keyboard. An operator cannot write a policy narrower than everything.
- **Scoped by coordinate.** A rectangle is not a security boundary. Windows move, and a policy
  written in pixels is wrong the moment somebody drags something.

There is a third problem the first two obscure. An agent that can click can click AgentOS's own
Approve button, and an agent that can type can answer the CLI's `Approve? [y/N]` prompt. The
approval gate is the compensating control that every other mitigation here leans on, and synthetic
input can reach it. `IMMUTABLE_DENY` cannot express this: it is a list of `(domain, action)` pairs
with no resource dimension, so it can forbid the capability `runtime.disable_approvals` and cannot
forbid achieving the same thing physically.

## Decision

The unit of scope is the **application in front**, as a new `ResourceRef::Application`, matched by
glob the way programs and origins already are. A policy reads:

```yaml
permissions:
  computer:
    type:
      effect: ask
      applications: [Mail]
```

Three rules make that mean something.

**The caller names its target.** Every `computer.*` call takes an `application` argument, and
planning refuses unless that application is the one with focus. It is not discovered by the tool,
because a tool that resolved the target for itself at execution time would faithfully type the
message it was authorised to send to Mail into whatever had taken focus in the meantime.

**Not knowing is a refusal.** If nothing is in front, the call fails. It does not fall back to an
unscoped capability — an unscoped capability is matched by any rule that lists no resources, so "I
could not tell what was in front" would quietly widen the grant.

**AgentOS is never a target.** The tools refuse to send input to their own process, regardless of
policy. This is the one place in the codebase where a tool overrides the permission engine rather
than consulting it, and it is deliberate: the engine has no vocabulary for the rule.

Two further decisions follow from the same reasoning. Focus is re-read before **every individual
event**, not once per call, so a twenty-character password does not get split across two windows.
And a keystroke that commits — a newline, or Return — is priced as `Critical` rather than `High`,
which is the keyboard's version of the `submit: true` distinction `browser.type` already draws.

The backend exists on macOS and Windows. Elsewhere the crate compiles and every tool refuses.

## Consequences

An operator can grant an agent one application without granting it the keyboard. That is a real
narrowing of the worst case, and it is the narrowing the README's `Applications ASK` sketch always
implied.

The authorisation logic is testable everywhere. `Desktop` is the entire platform boundary, and the
shipped `RecordingDesktop` lets a policy be tested with no screen involved — so the interesting part
of this feature is covered on the Linux CI runner that cannot possibly run it.

Reading the screen raises taint, so an agent that has looked at the desktop is held to a higher bar
for everything it does afterwards. Window titles and screenshots are both content somebody else
wrote. `browser.screenshot` was corrected to match, having previously reported its output as coming
from the runtime.

The scope binds who receives an event and not what the event does. `Cmd-Q`, "send", and the letter
`a` are one capability on one resource, and no amount of policy fixes that. The residual gaps are
written down in `SECURITY.md` rather than left for a reader to find, because a control that is
narrower than it looks is worse than no control at all.

The target application's name is self-asserted, and focus is arrangeable by any process that can
raise a window. The check stops the ordinary failure — typing into whatever happened to be in front —
and does not stop an attacker who already has code running on the machine.

## Rejected

**Coordinate-scoped permissions.** A policy in pixels is wrong as soon as a window moves, and it
would read as a guarantee while being a coincidence.

**Prompting for the operating-system permission on first use.** macOS will put up its own
Accessibility dialogue if asked. Doing that mid-run means a system prompt appearing with no
explanation of what asked for it; `agentos doctor` reports the missing grant instead, and says where
to give it.

**Holding one connection to the window server for the process lifetime.** Cheaper per call, but
`build_registry` is called by `agentos tools` and `agentos doctor`, and listing the catalogue must
not be a reason to ask macOS for permission to control the computer.

**Reusing `ResourceRef::Named`.** It would have compiled and needed no new variant. It would also
have made a rule about an application indistinguishable from a rule about an integration account of
the same name, and the shorthand `computer: { type: [Mail] }` already routed there — compiling to a
pattern that could never match anything, which is the worst kind of wrong: silent, and safe enough
that nobody investigates.
