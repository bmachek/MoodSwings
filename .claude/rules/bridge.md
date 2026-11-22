---
paths:
  - "src/world/{river,bridge}.rs"
---

# A bridge cannot be raised

<!-- Loaded by Claude Code when a file under `paths:` is read; listed in
AGENTS.md's *Read before you touch* for every other agent. Moved out of
the always-loaded instructions because it only matters here. -->

`world::river`'s trick is that a bridge is free: the water is laid at
`layer::WATER`, one millimetre off the grass and thirteen under the lowest
carriageway, so every street that crosses it is already drawn over it. Nothing
finds the crossing, cuts the water or ramps anything up.

What that cannot give you is a bridge you can *see*. The deck is flush — it has
to be, because `Terrain::height` is exactly zero wherever anything is built —
so there is no soffit, no arch and nowhere to hang one: anything under the deck
is under thirteen millimetres of water. `world::bridge` therefore builds
upward and into the river — parapet, lamp standards, and the pier heads that
would break the surface — and that is the whole of what a low bridge shows from
a bank anyway.

The crossings are worked out once (`bridge::crossings`) because three things
need the same answer, and two of them were wrong before there was a list: the
bank wall was being laid straight across every deck (Landshut's six bridges
each carried two 0.9 m stone walls over the carriageway, colliderless, so the
traffic drove through them and nothing complained), and the river's trampoline
took `BOUNCE_LEVEL` from the canal — whose water is at sixty millimetres, not
one — which stood a 0.95-restitution collider two centimetres *above* the road.
Every crossing of the Isar was a launch ramp.
