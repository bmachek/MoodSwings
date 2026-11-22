---
paths:
  - "src/world/{church,interior}.rs"
---

# A church you can walk into

<!-- Loaded by Claude Code when a file under `paths:` is read; listed in
AGENTS.md's *Read before you touch* for every other agent. Moved out of
the always-loaded instructions because it only matters here. -->

`world::interior` has built real rooms behind real doors for a while — floor,
lined walls, an emissive ceiling, furniture with colliders and one member of
staff with a full set of feelings. Churches were not on that path:
`BuildingKind::enterable()` names four shop-shaped kinds, and `world::church`
raised its nave as one solid box whose only door was a slab of painted shadow
with a collider in it.

They are on it now, and the route is their own rather than `enterable()`'s,
because a church's door is its own architecture: you come in under the west
tower. `church::spawn` draws the nave and the tower's ground storey as
*shells* — four walls and a vault, with the pierced face built as two jambs
and a lintel — and hands back a `Nave` that `world::buildings` furnishes
through the same `interior::spawn` the shops use. `Doorframe` grew `clear`,
`head` and `opening` for it: a shopfront's glass runs to the ceiling so its
room height and its doorway are one number, and a basilica's are 13 m and
4.6 m.

Three things were invisible while the nave was solid masonry and are not any
more, and all three are the same lesson:

- **The pitched roof hangs into the room.** It is a cube turned 45° about the
  ridge, so its lower vertex drops as far below the eaves as the ridge stands
  above them — fifteen metres on St. Martin. The vault goes under it, with
  `VAULT_CLEARANCE` to spare.
- **Two of the tower's four corner buttresses stand inside the nave.** The
  tower's back corners are past the nave's front wall by construction. A
  walk-in church drops them; a west tower's rear buttresses are absorbed into
  the nave anyway.
- **The stone vault and the plaster ceiling lining were laid at the same
  height**, two down-facing faces in one plane — `world::layer`'s rule, which
  turns out to apply indoors too.

`Shape::of` is the pure function all the proportions come out of, so every one
of those is a `cargo test` rather than a screenshot. What it cannot check is
what the room looks like; that is `--screenshot` with the camera inside it.
