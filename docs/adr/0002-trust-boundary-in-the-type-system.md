# 2. The trust boundary lives in the type system

- **Status:** accepted
- **Date:** 2026-08-20

## Context

An agent reads webpages, files, emails and command output. Any of it may have been written by someone
trying to redirect the agent. The industry-standard mitigation is a paragraph in the system prompt
asking the model to disregard instructions found in data.

That is a request, not a control. It works until a model is persuaded, at which point it provides no
protection at all — and it provides no *evidence* either, because there is nothing in the system that
distinguishes the two kinds of text.

## Decision

The distinction is structural.

`Content` has four variants and exactly one is trusted: `Control`, carrying operator instructions and
the objective. `Model` (model prose), `ToolCall` and `Untrusted` (every tool result, without
exception) are not.

There is no API that converts a tool result into `Control`. `ToolResult::content` is typed
`UntrustedContent` and no control-plane constructor accepts it.

When untrusted content is rendered for a model it is wrapped in a nonce-tagged envelope, with
closing-delimiter lookalikes neutralised case-insensitively and the nonce stripped from the body.

Crucially: **authorisation never reads any of this.** Permission decisions come from the policy, the
tool's declared capability requirements and the run's taint state.

## Consequences

A fully compromised model can request anything and be refused, because the refusal does not depend on
its cooperation. This is testable, and it is tested: a scripted provider that obeys an injected
instruction has every resulting call denied while the run still completes.

`Model` output being untrusted is the subtle part, and it is what stops a model asserting authority
it was not given.

The envelope makes the boundary visible to a cooperative model and makes injection attempts legible
in the audit log.

The cost: tool authors must correctly set `returns_untrusted_data`. Getting it wrong silently removes
taint escalation for that tool, so it is called out in the contributing guide and covered by a test
asserting the shipped tools are marked correctly.

## Rejected

**Prompt-based mitigation alone.** Fails exactly when it matters and leaves no evidence.

**Sanitising untrusted content before showing it.** Unbounded problem — natural language has no
reliable "this is an instruction" marker — and destroys content the agent legitimately needs.

**Refusing to show untrusted content to the model.** Then the agent cannot do its job. The goal is to
read hostile text safely, not to avoid reading it.
