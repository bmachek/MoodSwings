---
description: Let the game play itself and report what the Watch complains about
argument-hint: "[seconds, default 120] [--city minga] [--film]"
---

Hand this to the `patrol-warden` subagent: $ARGUMENTS

`cargo run --release -- --patrol <seconds>` walks the city junction by junction
while `Watch` takes its vital signs once a second. A patrol ending with no
complaints is the point.

Every complaint should come back marked new or documented — the asset count
climbing as a chunk comes back is the known streaming leak; entities outliving
their `ChunkOf` is not. Add `--film shots/take` when a complaint needs to be seen
rather than deduced. Needs a GPU: if there is none, it must say so instead of
reporting a clean run.
