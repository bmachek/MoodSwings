---
description: Run the pre-PR gate — fmt, clippy -D warnings, tests, build — and report only what failed
argument-hint: "[module or path that changed, e.g. world::layer]"
---

Hand this to the `rust-verify` subagent so the compiler output stays out of this
conversation, and give it the changed area so it can pick a test filter first:

$ARGUMENTS

It runs what CI runs — `cargo fmt --all -- --check`, `cargo clippy --locked
--workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`, and
`cargo build --locked` when linking could be affected — and may fix formatting
and mechanical lints itself. Report its verdict, the failures with their
`file:line`, and anything it could not run and why.
