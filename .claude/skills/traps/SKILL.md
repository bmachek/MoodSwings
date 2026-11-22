---
name: traps
description: Review the current diff against every documented trap and the load-bearing conventions
argument-hint: "[base ref, default the working diff]"
context: fork
agent: trap-reviewer
background: false
---

Review a change against what has already bitten this project. The base to
review against, if one was given: $ARGUMENTS

With no base, the change is the working tree against HEAD — `git diff HEAD` and
any untracked files `git status` shows. With a base, it is `git diff
<base>...HEAD` plus the working tree on top.

Judge it against every trap in AGENTS.md's `### The traps` — there are ten,
and your checklist turns each into what to look for — and against the
conventions that are rules: preset-driven rendering, feel constants in
`GameConfig`, `CityStyle` over a `match` at the use site, `ChunkOf` on anything
streamed, German player-facing text, comments that explain why and are rewritten
when the decision they record is reversed.

Report findings most serious first, each with its `file:line`, then what the
change still needs to be trusted: which framings, a patrol, a survey, an
audition. Say plainly when you found nothing.
