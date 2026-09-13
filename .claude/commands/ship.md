---
description: The full pre-offer pipeline — verify, review against the traps, check the docs, and say what is still unverified
argument-hint: "[what changed]"
---

Everything a change owes before it is offered to anybody. $ARGUMENTS

Run these **in parallel**, in one message, because none of them depends on
another:

- `rust-verify` — fmt, clippy `-D warnings`, tests, and a build if linking could
  have changed.
- `trap-reviewer` — the diff against the six documented traps and the
  load-bearing conventions.
- `docs-steward` — the CLAUDE.md/AGENTS.md twins, plus whatever else the change
  owes: module table, flag table, `CREDITS.md`, a reversed decision's paragraph.

Then, and only where the change touches them:

- `town-surveyor` for anything in `world` — it costs a second and needs no GPU.
- `render-shooter` for anything visible, `patrol-warden` for anything that only
  goes wrong while running, `audio-keeper` for the bank, `perf-scout` for
  anything that might cost frame time.

Finish with three things: what passed, what you fixed, and — listed plainly —
**what remains unverified and why**. An unrun check is never a passed one, and
"no GPU in this container" is a complete and acceptable reason. Do not commit,
push or open a pull request unless it was asked for.
