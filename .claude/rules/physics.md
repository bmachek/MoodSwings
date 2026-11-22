---
paths:
  - "src/bounce/**"
  - "src/vehicle/{controller,impact,spec}.rs"
  - "src/player/on_foot.rs"
  - "src/world/mod.rs"
---

# Verifying physics without a window

<!-- Loaded by Claude Code when a file under `paths:` is read; listed in
AGENTS.md's *Read before you touch* for every other agent. Moved out of
the always-loaded instructions because it only matters here. -->

`bounce::testing::physics_app` is a headless Avian world built the way
`world::mod` builds the real one — the same interpolation, the same
`DefaultRestitution` and solver settings — with `ground()` and `kerb()` to put
something under a body and `finish()` to do what `run()` would. It is the only
instrument here that needs neither a GPU nor a window, and it answers the
questions a screenshot cannot: where the low point of a hop is, how long a body
takes to come down off a kerb, whether a facing written every frame costs a
body its walk. Drive it with `TimeUpdateStrategy::ManualDuration`, and pass a
frame *shorter* than the tick to ask what happens above the tick rate, which is
where interpolation does the most work and where two of the bugs it has caught
only exist.

Two rules it enforces that nothing else could. Physics tests run over
`Gait::ALL`, because the default is walking and every physics test used to
drive the bouncing city by leaving `hop_scale` at its constructed 1.0. And
`initialises(system)` is four lines that trip Bevy's B0001 query-conflict check
without any of the resources the system reads — the check panics at first run,
not at compile time, and the capture harness was the only thing catching it.
