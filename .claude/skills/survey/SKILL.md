---
name: survey
description: Judge a town by numbers instead of pixels — the scorecard, without a window
argument-hint: "[city style, e.g. landshuepf] [--seed N]"
context: fork
agent: town-surveyor
background: false
---

Survey the town: $ARGUMENTS

`cargo run -- --survey --city <style>` builds exactly what
`world::generate_city` builds and prints a scorecard in about a second, without
starting Bevy, so it works with no GPU and it is the first thing to run on any
layout change. Take it before *and* after: for a change already in the tree,
the before comes from `tools/base-worktree.sh HEAD` (see its header) — never
from a stash.

Report the sections that moved, before → after, name the ones that did not,
and say which capture framings, if any, the move now justifies. A figure that
moved nothing needs no GPU at all.
