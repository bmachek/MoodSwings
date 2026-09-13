---
description: Bring the written record back in line — the CLAUDE.md/AGENTS.md twins, README, CREDITS
argument-hint: "[what changed, if it is not in the diff]"
---

Run `tools/check-docs.sh` first, then hand the work to the `docs-steward`
subagent: $ARGUMENTS

The twins must be identical from line 4 down; `CLAUDE.md` is the source and
`tools/check-docs.sh --fix` rewrites `AGENTS.md` from it, keeping its own first
three lines. If an edit was made in `AGENTS.md` only, carry it into `CLAUDE.md`
by hand before fixing, or it is lost.

Then check what else the change owes: the module table for a new module, the
README flag table for a new flag, the postcard sections for a new `CityStyle`,
`CREDITS.md` for a new recording or material set, and the paragraph that records
any decision this change reverses.
