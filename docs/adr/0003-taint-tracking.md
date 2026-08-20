# 3. Taint tracking raises the approval bar

- **Status:** accepted
- **Date:** 2026-08-20

## Context

Typed content and a policy engine stop an agent exceeding its permissions. They do not address the
harder case: an agent doing something **within** its permissions that it only decided to do because
an attacker told it to.

An agent allowed to read a directory and write files in it is behaving legitimately when it reads a
poisoned document and writes a file. The policy has nothing to object to. Yet the second action is
attacker-directed, and the operator would want to see it.

Detecting malicious intent in the model's plan is not a viable control — it is the same losing game
as detecting instructions in prose.

## Decision

Track *provenance* instead of intent.

Every tool declares whether its results can be externally influenced. Once a run receives data from
such a tool it is **tainted**. From that point, the policy engine escalates `allow` to `ask` for any
action at or above a configured risk level, `medium` by default.

Escalation only ever tightens. `deny` stays `deny`; a tainted run can never do more than a clean one.
Taint is never cleared within a run — later trusted input does not launder it.

The approval card names the sources the agent has read, because that is the single most
decision-relevant fact for the person deciding.

## Consequences

"Read a poisoned page, then exfiltrate" becomes loud instead of silent. The second step is not
blocked because the runtime recognised it as malicious; it is surfaced because the agent had read
something it did not write and the action mattered.

This is a genuine property rather than a heuristic: it does not depend on recognising an attack, only
on knowing where data came from.

The cost is more approval prompts for agents that read a lot. Mitigated by the risk threshold being
configurable and by low-risk reads not escalating — but approval fatigue is a real failure mode, and
if the threshold is set too low people stop reading the cards. The default was chosen with that in
mind: `medium` is the level at which an action first has an effect worth knowing about.

Configurable per policy, including off. Turning it off is a deliberate, visible choice, and
`agentos policy show` says so in yellow.

## Rejected

**Blocking rather than escalating once tainted.** Would make the common, legitimate workflow — read
something, then act on it — impossible.

**Per-datum taint propagation.** Tracking which specific outputs derive from which inputs would be
more precise, but requires reasoning about what the model did with the text, which is exactly the
thing that cannot be relied upon. Run-level taint is coarse and sound.
