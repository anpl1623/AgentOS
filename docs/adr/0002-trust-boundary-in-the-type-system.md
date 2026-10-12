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

Taint is derived from the declared provenance of the bytes, not from anything a tool says about
itself. Every result carries a `DataSource`; a tool that returns external data cannot opt out of
raising taint. The provenance label is itself a claim, so it is not believed downwards: when a call
may read — its tool name or any capability in its authorised plan is not one of the actions known
only to change something (a filesystem write, delete, copy or move; a click, keystroke or pointer
movement) — and the output is labelled as the operator's or the runtime's own, the run is tainted
with the tool as the source. The list is of what does *not* read, so a domain nobody has listed yet
is held to the floor. A filesystem tool that does not itself read, such as `filesystem.copy`, reads
its source only to write it, and that read does not count. The text of a failed call is observed by
the same rule: a failed read taints, a failed write does not. A denial is composed by the runtime
and taints nothing; the resources the plan resolved (a symlink's target, a redirected origin) are
kept in the audit record and left out of the text the model is given. `returns_untrusted_data`
survives only as catalogue metadata for `agentos tools`, with no authorisation effect.

The cost: a tool that labels its output wrongly, or plans a read it does not return to the model,
produces approval prompts it did not need to. That is the error surfacing, and it fails towards a
human looking rather than towards a silent action. Likewise a tool's capability manifest is checked
against every plan and any excess is written to the audit log, but never refused — the policy engine
evaluates the plan itself, so a stale manifest misleads a policy author and nothing more.

## Rejected

**Prompt-based mitigation alone.** Fails exactly when it matters and leaves no evidence.

**Sanitising untrusted content before showing it.** Unbounded problem — natural language has no
reliable "this is an instruction" marker — and destroys content the agent legitimately needs.

**Refusing to show untrusted content to the model.** Then the agent cannot do its job. The goal is to
read hostile text safely, not to avoid reading it.
