---
name: docs-sync
description: Bring the written record back in line — AGENTS.md and the CLAUDE.md that imports it, the rules chapters, the Codex agents, README, CREDITS
argument-hint: "[what changed, if it is not in the diff]"
context: fork
agent: docs-steward
background: false
---

Bring the written record in line with the change. What changed, if it is not
obvious from the diff: $ARGUMENTS

Start with `tools/check-docs.sh`. `--fix` regenerates what is generated — the
Codex agents from `.claude/agents/`, the *Read before you touch* table from the
chapters' `paths:` — and everything else it reports needs a hand.

Then the diff (`git diff HEAD`, and `git status` for new files) for what else
it owes: the module table in AGENTS.md for a new module, the README flag table
for a new flag, the postcard sections for a new `CityStyle`, `CREDITS.md` for a
new recording or material set, the `trap-reviewer` checklist for a new trap, a
`.claude/rules/` glob for a renamed file, and the paragraph that records any
decision this change reverses.

Report which files you changed and why, and anything stale you left because it
needs a decision rather than an edit.
