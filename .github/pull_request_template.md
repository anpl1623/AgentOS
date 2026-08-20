## What this changes

<!-- What does it do, and why? Link an issue if there is one. -->

## Reasoning

<!-- What did you consider and reject? This is the most useful part of the description. -->

## Security review

<!-- Delete this section if the change touches none of the following. -->

This change touches:

- [ ] the policy engine or its precedence rules
- [ ] path resolution or filesystem scoping
- [ ] the `Content` / trust-boundary types
- [ ] taint tracking
- [ ] the approval gate
- [ ] the audit log, its schema, or its triggers

What property still holds afterwards, and what test demonstrates it?

## Checklist

- [ ] `cargo fmt --all` is clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [ ] `cargo test --workspace` passes
- [ ] New behaviour has tests, including for the ways it could be misused
- [ ] Docs or ADRs updated if the design changed
