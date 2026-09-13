---
description: Review the current diff against the documented traps and load-bearing conventions
argument-hint: "[base ref, default the working diff]"
---

Hand this to the `trap-reviewer` subagent: $ARGUMENTS

It reads the diff and judges it against what has already bitten this project —
RNG streams and determinism, the `layer` millimetre table, `ChunkOf`, terrain
height outside the corridor, restitution on a sprung body, collider scaling and
`Rest`, two mutable queries touching one component — and against the conventions
that are rules here: preset-driven rendering, feel constants in `GameConfig`,
`CityStyle` over a `match` at the use site, German UI text, comments that explain
why and are updated when the decision they record is reversed.

Report its findings most-serious-first with `file:line`, then what the change
still needs to be trusted: which framings, a patrol, a survey, an audition.
