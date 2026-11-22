---
name: perf
description: Measure frame cost with a method that can support the number
argument-hint: "[framing] [base ref to compare against]"
context: fork
agent: perf-scout
---

Measure what the change costs a frame. Framing and base, if given: $ARGUMENTS

The protocol is not optional, because this repository has already reported both
"eleven percent slower" and "twelve percent faster" about the same change:
`--release`, `--frames 200` (the default window times a half-built scene), two
or three runs, and both commits built back to back — the base in
`tools/base-worktree.sh <base>`, never by stashing the tree — and measured in
the same minute. A before/after half an hour apart is measuring the fan.

If the spread is as large as the difference, the answer is "no measurable
change", not a direction. On a software rasteriser or with no GPU, say so and
give an algorithmic reading of the diff instead of a figure.
