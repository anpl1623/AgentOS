# Contributing to AgentOS

Thanks for considering it. AgentOS is a security-sensitive project, so a few of the conventions below
are stricter than you might expect. They are all there for a reason, and the reasons are stated.

## Getting set up

Requires [Rust](https://rustup.rs) 1.85 or newer. There is nothing else to install — the database is
embedded and the test suite needs no network, no API key and no external service.

```bash
git clone https://github.com/anpl1623/AgentOS.git
cd AgentOS
cargo test --workspace
```

Before opening a pull request:

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

## Where things live

Read [docs/architecture.md](docs/architecture.md) first — it will save you time.

The dependency direction is strictly acyclic and worth preserving:

```
core ← permissions ← tools ← runtime ← cli
core ← persistence, audit, providers, secrets
```

`agentos-core` has no I/O and no async runtime. If you find yourself wanting to add a database call
to it, the type you are adding probably belongs somewhere else.

## Adding a tool

Adding a capability means writing a [`Tool`](crates/agentos-tools/src/tool.rs). It should not mean
touching the agent loop; if it does, please say so in the pull request, because that is a design
smell worth discussing.

1. Define a typed argument struct deriving `Deserialize` and `JsonSchema`, with
   `#[serde(deny_unknown_fields)]`. The schema advertised to the model and the struct used to
   validate its reply come from the same type, so they cannot drift.
2. Implement `metadata`, `validate`, `plan` and `execute`.
3. In `plan`, declare every capability the call needs — **including both ends of a transfer**.
   Reading a file you may read and writing it somewhere you may not is still exfiltration.
4. Set `returns_untrusted_data: true` if the output could be influenced by anyone other than the
   operator. This is what drives taint escalation, and getting it wrong quietly removes a control.
5. Register it in `standard_registry()`.
6. Write tests for the ways it could be misused, not only the way it is meant to be used.

`plan` must be free of side effects. It runs before authorisation, and the whole model depends on
nothing having happened by the time the policy engine is consulted.

## Coding conventions

- **No `unwrap`, `expect` or `panic!` in library code.** Enforced by clippy. Tests may use them.
- `thiserror` enums per crate; `anyhow` only in the CLI binary.
- Errors carry context — which path, which tool, which rule. "operation failed" helps nobody.
- Comments explain *why*, not *what*. If a line looks wrong but is right, say why it is right.
- Prefer boring code. Deleting an abstraction is a good pull request.

## Tests

Tests are not optional, and the interesting ones are about misuse:

- Name them for the property they protect, not the function they call.
  `symlink_escape_is_blocked` beats `test_resolve_path_2`.
- If you fix a bug, add the test that would have caught it.
- Anything touching permissions, paths, subprocesses or the trust boundary needs a test showing the
  bad case is refused — not just that the good case works.

Mock external services. A test that needs somebody's real Gmail account is not a test anyone else can
run.

The browser tests drive a real Chromium and skip, loudly, when none is installed. If you are
debugging the browser layer, `cargo run -p agentos-browser --example spike -- <url>` time-boxes each
step and prints as it goes, so a hang is attributed to a step rather than to "the browser".

## Security-sensitive changes

If your change touches any of the following, please say so explicitly in the pull request description
and explain what property you believe still holds afterwards:

- the policy engine or its precedence rules
- path resolution or filesystem scoping
- the `Content` / trust-boundary types
- taint tracking
- the approval gate
- the audit log, its schema, or its triggers

These are the parts where a plausible-looking simplification can silently remove a control. Reviews
of them are slower on purpose.

## Architecture decisions

Substantial design changes get an ADR in [docs/adr](docs/adr). Copy the format of an existing one:
context, decision, consequences, and — most usefully — what was rejected and why.

## Commits and pull requests

- One logical change per pull request.
- Explain the reasoning, not just the diff. What did you consider and reject?
- Leave `main` in a working state: it should always build, pass, and run.

## Code of conduct

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
