# 5. Browser automation is deterministic, not vision-based

- **Status:** accepted
- **Date:** 2026-08-20

## Context

An agent needs to use websites. There are two ways to build that:

- **Vision-based:** screenshot the page, ask a model where to click, click at those coordinates. This
  is how general computer-use agents work, and it works anywhere.
- **Protocol-based:** speak the Chrome DevTools Protocol, address elements by CSS selector, read the
  DOM. This works only in a browser you control.

Vision is more general. It is also slower, more expensive, less reliable, and — the part that matters
most here — much harder to audit. `click at (412, 908)` tells a reviewer nothing about what was
clicked, and quietly starts doing something else when a layout shifts.

## Decision

Browser automation is protocol-based, over CDP, via `chromiumoxide`. Elements are addressed by CSS
selector. `browser.inspect` enumerates the interactive elements on a page and returns a stable
selector for each, so the model has something concrete to name rather than guessing.

Screenshots exist — `browser.screenshot` — for a human to look at and for a future vision fallback.
They are not how the agent decides where to click.

The browser layer is kept separate from computer control. They solve different problems: a browser
exposes structure, a native application generally does not. See
[ADR 6](0006-computer-control.md).

AgentOS does not bundle or download a browser. It looks for one already installed, in a defined
order, including Playwright-managed builds that many developers already have.

## Consequences

Actions are legible. An audit entry reading `browser.click #send-button` is reviewable in a way that
a coordinate pair is not, and an approval card can say what will be clicked.

Interaction is fast and deterministic. The end-to-end demo — a real browser, a real local CRM, the
full permission pipeline — runs in well under a second, which is what makes it viable as a test that
runs on every commit rather than a demo someone runs by hand.

Capabilities are scoped by origin, so a policy can grant an agent one site without granting it the
web. That is only expressible because the runtime knows which page it is on.

The cost: sites that are hostile to automation, or built entirely from canvas, are not reachable this
way. That is the gap the vision fallback will fill, and it is a smaller gap than it looks — the
business systems this project targets are ordinary web applications.

Not bundling a browser means a first run can fail with "no browser found". That is the right failure:
a security-sensitive tool that silently downloads and executes a hundred megabytes of binary on first
use has made a decision the user should have made.

## Rejected

**Vision-first interaction.** Slower, costlier, less reliable, and it produces an audit trail nobody
can review. It belongs as a fallback, not a foundation.

**Driving Playwright through a Node sidecar.** Mature and capable, but it puts a Node runtime and an
IPC boundary inside the trust boundary of a Rust security-sensitive process, and makes distribution
considerably worse.

**Using the operator's own browser profile.** Convenient — the agent would already be logged in
everywhere — and exactly the reason not to. Each run gets a fresh profile, which is removed when the
run ends.
