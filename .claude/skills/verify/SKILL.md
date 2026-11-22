---
name: verify
description: Run the pre-PR gate — fmt, clippy -D warnings, tests, build — and report only what failed
argument-hint: "[module or path that changed, e.g. world::layer]"
context: fork
agent: rust-verify
background: false
---

Run the gate CI runs on the tree as it stands now. The area that changed, so a
filtered test run can come first: $ARGUMENTS

If nothing was named there, take the area from `git status` and
`git diff --stat`. Run what CI runs — `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo test --locked --workspace`, and `cargo build --locked` when linking could
be affected — fixing formatting and mechanical lints yourself.

Report the verdict, each failure with its `file:line` and the one command that
reproduces just it, what you fixed, and anything you could not run and why.
