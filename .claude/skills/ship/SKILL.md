---
name: ship
description: The full pre-offer pipeline — verify, the traps, the docs and a correctness review in parallel, then say what is still unverified
argument-hint: "[what changed]"
---

Everything a change owes before it is offered to anybody. $ARGUMENTS

Start these **in parallel**, in one message, because none of them depends on
another. Subagents do not see this conversation, so give each one a sentence on
what changed and where:

- `rust-verify` — fmt, clippy `-D warnings`, tests, and a build if linking could
  have changed.
- `trap-reviewer` — the diff against all ten documented traps and the
  load-bearing conventions.
- `docs-steward` — `tools/check-docs.sh`, and whatever else the change owes:
  module table, flag table, `CREDITS.md`, a reversed decision's paragraph.
- The bundled `/code-review` at `high` on the same diff — the plain correctness
  bugs (a wrong sign, a missed case, an off-by-one) that a list of this
  project's traps was never meant to catch.

Then, and only where the change touches them:

- `town-surveyor` for anything in `world` — it costs a second and needs no GPU.
- `render-shooter` for anything visible, `patrol-warden` for anything that only
  goes wrong while running, `audio-keeper` for the bank, `perf-scout` for
  anything that might cost frame time.

Where two of them report the same thing, say it once. Finish with three things:
what passed, what you fixed, and — listed plainly — **what remains unverified
and why**. An unrun check is never a passed one, and "no GPU in this container"
is a complete and acceptable reason. Do not commit, push or open a pull request
unless it was asked for.
