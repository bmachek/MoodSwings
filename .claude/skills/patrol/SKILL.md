---
name: patrol
description: Let the game play itself and report what the Watch complains about
argument-hint: "[seconds, default 120] [--city minga] [--film]"
context: fork
agent: patrol-warden
---

Patrol the city: $ARGUMENTS

`cargo run --release -- --patrol <seconds>` (120 unless a length was given)
walks the city junction by junction while `Watch` takes its vital signs once a
second. A patrol ending with no complaints is the point.

Mark every complaint new or documented — the asset count climbing as a chunk
comes back is the known streaming leak; entities outliving their `ChunkOf` is
not. Add `--film shots/take` when a complaint needs to be seen rather than
deduced. It needs a GPU: if there is none, say so instead of reporting a clean
run.
