---
description: Judge a town by numbers instead of pixels — the scorecard, without a window
argument-hint: "[city style, e.g. landshuepf] [--seed N]"
---

Hand this to the `town-surveyor` subagent: $ARGUMENTS

`cargo run -- --survey --city <style>` builds exactly what `world::generate_city`
builds and prints a scorecard in about a second, without starting Bevy — so it
works with no GPU and it is the first thing to run on any layout change. Take it
before *and* after the change; a figure that moved is a change worth shooting,
and one that moved nothing needs no GPU at all.

Report the sections that moved, before → after, name the ones that did not, and
say which capture framings (if any) the move now justifies.
