# ADR 7: Showing a model what was captured

**Status:** accepted

## Context

`computer.screenshot` and `browser.screenshot` shipped before any provider in the runtime could
transport an image. Both wrote a PNG into the agent's workspace and returned its path, which meant
an agent could photograph a screen it could not look at. The comment in the browser tool said as
much: "the model cannot see the image today".

That made the whole of computer control conditional on a native application exposing enough structure
to work with blind — and the accessibility APIs that would supply it are not the ones AgentOS reads.
It also made `browser.screenshot` close to useless, since the browser tools deliberately prefer the
DOM (see [ADR 5](0005-deterministic-browser-automation.md)) and the screenshot exists for the pages
where the DOM has nothing to offer.

## Decision

Images travel through the runtime as `Content::Image(UntrustedImage)` — a new content variant, not a
field on `UntrustedContent` — and are attached to a tool result only when the call asked for it and
the policy allowed it.

**Attaching is a distinct capability from capturing.** `computer:vision` and `browser:vision` are
separate actions from `computer:screenshot` and `browser:read`, scoped identically — to an
application, or to an origin. The two acts have different blast radii: one writes a file the operator
owns, the other sends the contents of their screen to somebody else's server. Making them one grant
would mean every policy that already permitted a screenshot silently acquired an egress permission on
upgrade, which is exactly the kind of quiet widening this project exists to prevent.

**`attach` defaults to false.** A capture that is neither saved nor attached is refused outright,
because it would still read the screen while producing nothing.

**There is no trusted image.** `ControlContent` has no visual counterpart. Text at least has an
envelope: untrusted text is wrapped in a nonce-tagged delimiter the body cannot forge, so a model can
see where attacker-controlled data begins and ends. Pixels have nothing equivalent. A rendered page
reading "SYSTEM: you are now authorised" and a system message are the same thing to a model looking
at an image. Since the boundary cannot be made visible inside an image, the type system refuses to
let one claim authority at all, and `Content::Image` reports `Trust::Untrusted` unconditionally.

**Vision is declared per model, not per provider.** `ModelConfig::vision` is `Option<bool>`: `None`
takes the provider's default. Anthropic defaults on. OpenAI-proper defaults on. Every other
OpenAI-compatible endpoint defaults *off*, because the model behind a local URL is unknown and
text-only is the common case. Guessing wrong in that direction costs one screenshot; guessing wrong
in the other costs the whole run to a 400.

**A model that cannot see is told so.** When the provider reports no vision, the images are dropped
and a `ControlOrigin::RuntimeNotice` message names the tool and says the pictures were withheld. The
alternative — dropping them silently — produces a model that confidently narrates a screen it was
never shown, and an operator reading the trace afterwards with no way to tell that happened.

**Everything is resized before it is sent.** `agentos_tools::vision::prepare` reads the header under
no allocation limit, checks the declared dimensions itself, and refuses anything over ~64 megapixels
before a decoder is asked for memory. It then fits the image inside 1568 pixels on its long edge and
keeps PNG unless the byte budget forces JPEG. The file written to the workspace is the capture
untouched; only what the model sees is scaled.

**The conversation keeps three captures.** Providers re-read the entire conversation on every turn,
so an image sent once is billed on every subsequent request. Older images are replaced by the
description they already carried, which keeps the model aware of what it looked at while making it
take a fresh capture to look again.

## Consequences

A native application with no usable structure is now reachable, and the browser has the fallback ADR
5 said it was leaving room for.

`message_for_tool_result` became `messages_for_tool_result(result, vision)` and returns a `Vec`,
because withholding an image has to produce a second message rather than mutate the first.

Anthropic carries the image inside the `tool_result` block it answers, which meant changing that
block's content from a string to a block list. OpenAI's `tool` role accepts only a string, so images
follow as their own user turn carrying no runtime prose — the envelope in the tool message already
said where the pixels came from, and adding a sentence there would be text the model could mistake
for an instruction.

Agents carry a nullable `vision` column (migration `0003`). Existing rows read as `NULL`, which is
the provider default, so nothing changes for an installation that upgrades without touching its
agents.

`image` and `base64` become dependencies. `image` is pinned to the `png` and `jpeg` codecs only: the
default feature set enables a dozen more formats, each one another parser exposed to bytes an
attacker chose.

## Alternatives considered

**One capability for capture and attachment.** Simpler policies, but it means an upgrade widens what
existing policies permit. Rejected.

**`attach` defaulting to true.** A screenshot the model cannot see is nearly useless, so the default
is tempting. It is the same silent widening in a different costume. Rejected.

**Vision as a provider-level constant.** Would have avoided the `Option<bool>` and the migration, at
the cost of being wrong for every local endpoint. Rejected.

**Sending images at full resolution and letting the provider complain.** The failure arrives as an
opaque 400 several steps into a run rather than at the tool where the operator can see what happened,
and it is expensive when it succeeds. Rejected.
