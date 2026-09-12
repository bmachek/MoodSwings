# Living city implementation

The intended outcome is a city of persistent residents with reasons to travel,
relationships and remembered encounters, sharing a functioning transport
network. Rubber physics, provocation, forgiveness and emergent comedy remain
the game's verbs. Everyday routines must work before interruptions become funny.

## First increment: observe agents and preserve traffic queues

- `ai::observe` measures final movement intent against ground progress after
  gameplay systems. It distinguishes requested motion, waiting, blocked motion
  and being launched/knocked down. Behaviour markers are listed together: they
  are evidence, not a claim that arbitration has already been implemented.
- The developer `agents` window selects nearby live entities, shows route,
  mood, measured/desired speed, traffic obstacles and six recent transitions.
  Thresholds live in `GameConfig::agent_watch` and are adjustable there.
- Observations expire with their entities. Their ids are **not** persistent
  resident ids. No save-format change or offscreen life simulation has landed.
- Traffic now distinguishes a same-direction upright traffic leader from a
  static obstruction, oncoming car or overturned vehicle. A queue can wait
  past the recovery timeout without being deleted. Impatient honking remains.
- Actual recovery still exists for obstruction/unexplained immobilisation and
  is logged with entity, position, cause and duration. This is a temporary
  fallback, not successful routing. Intersection deadlock resolution is pending.
- Patrol logs include live blocked agents and cumulative blocked episodes.
  These are diagnostics, not automatic failures: deliberate player obstruction
  and crowd interactions also need interpretation.

### Resident-profile foundation

- `ai::resident::CitizenId` is separate from an ECS entity id. The profile
  registry keeps cast role, age, temperament, voice pitch, walking pace and
  outfit while a body is streamed out or enters a shop. Reappearance uses a
  compatible inactive profile, so the figure is recognisably the same person.
- The spawn streams still consume their normal draws before selecting a stored
  profile. Reintroducing an old resident therefore cannot reshuffle later
  anonymous arrivals.
- Profiles are deliberately in-memory only. Mood, route, relationships,
  errands, locations, save/load migration and an offscreen daily simulation
  are not claimed by this increment. The current mood is now copied back into
  the profile while a body is visible, so a recently provoked resident keeps
  that feeling when it returns; the remaining mutable facts are the next data
  model once places have stable ids.

### Single-file streets

The first thing measured rather than assumed. Two ninety-second Landshut
patrols recorded 42 and 49 traffic recoveries — cars deleted after twenty-six
seconds of going nowhere — and matching each against the street it happened on
put **63% of them on a four-metre carriageway**. That is not a routing bug. On
four metres `steering::lane_offset` puts the two travel lanes 1.90 m apart and
the cars in this game are 1.80 m to 2.10 m across, so two of them meeting in a
Gasse overlap; they touch, they stop, and a single forward ray at that lateral
separation cannot even see what stopped them. 45% of Landshut's 45 km of road
is under 4.5 m.

- `steering::single_file` answers "can two cars pass here at all" from the lane
  geometry and the widest car in the game, not from a width threshold picked by
  eye. `passing_gap` adds the two lanes rather than doubling one, because a
  street parked down one kerb is not symmetrical.
- `ai::giveway` reserves such a stretch for one direction at a time. A *run*
  reaches from one passing place to the next, and a passing place is a junction
  — three streets meeting leave somewhere to pull aside, a bend does not.
  Landshut has 184 of them among its 1565 streets — median 84 m, longest 481 m.
  Whoever is in a run has it and a convoy may follow; the moment anybody stands
  at the far mouth the near mouth stops admitting and the run drains, which is
  the whole starvation rule. A run with one way in takes one car rather than a
  convoy, because the one at the bottom of it has turned round and is coming
  back — and 102 of the 184 are that shape, which was not the expectation.
- A held car is `DriverObservation::Yielding`: a legitimate wait the recovery
  timer leaves alone. That exemption is the dangerous part, because a yielding
  car also reads to `ai::observe` as *waiting* rather than blocked — it is not
  asking to move — so a give-way that has gone wrong is the one kind of stuck
  with no symptom anywhere. Hence: every way a reservation can outlive its cars
  has its own timeout, a wreck holding a run is logged by name, the patrol
  complains about a long wait, the dev panel shows the queue, and the exemption
  itself expires after 75 seconds.
- Traffic no longer fades in on a single-file street, and prefers not to turn
  into one. Both are preferences and not bans: banning them outright shatters
  the drivable network into 88 pieces, the largest a tenth of the whole,
  because one pinch point between two houses cuts off everything past it.
- A car told to stop at a line had nothing to stop it with. Below 0.5 m/s a
  negative throttle is the reverse gear rather than the brake, below 0.4 m/s
  `throttle_for_speed` has a deadband, and the only other longitudinal force is
  quadratic drag — so it coasted through the line at 0.4 m/s. It now holds the
  handbrake, which `vehicle::controller` no longer applies as a constant shove
  backwards at a standstill: `f32::signum(0.0)` is 1.0, and every abandoned car
  in the city was sitting on that.
- Measured, interleaved on one machine, four ninety-second Landshut patrols
  run back to back with the two binaries alternating: **40 and 41 traffic
  recoveries before, 15 and 13 after**. An earlier pair, on a machine that was
  not yet swapping, read 47 and 37 against 26 and 29. Both pairs point the same
  way and neither is a controlled experiment — `core::patrol` picks its next
  junction from elapsed time, so no two runs walk the same streets, and the
  second pair ran ten to twenty times slower than realtime under memory
  pressure, which both binaries felt equally. The give-way readout in the last
  sample of the last run was `4 waiting at a mouth over 10 runs`: the rule is
  firing, not merely installed.
- The A/B also caught a regression, and a real bug under it. Both of the first
  after-runs launched fourteen cars skyward at one second from the same
  handful of positions. `maintain_population` checks a spawn candidate against
  every vehicle in the world, but a car it spawned two lines earlier is behind
  a `Commands` queue and is not in that query until the next frame — and the
  first tick of a session spawns the whole population at once, fifty cars none
  of which can see each other. Halving the candidate streets turned a rare
  overlap into a likely one. Candidates are now checked against the positions
  this tick has already used, and the cluster is gone from both after-runs.
- Not claimed: junction right of way, pedestrian gates, or any directed lane
  topology. A run is a piece of road two cars may not share; a crossing is the
  other one, and `roadgraph::movements_conflict` is still waiting for a caller
  — and still cannot tell two perpendicular straight-through movements apart,
  which is the first thing that has to change when it gets one.

### The van that was parked in the road

With the alleys sorted, the same question again, asked of the same log: where
do the *remaining* recoveries happen? The answer inverted. Before, 63% were on
a four-metre carriageway and 25% on the Altstadt; after, 25% are on a Gasse and
**68% are on the Altstadt band** — and 25 of the 28 read `Obstacle`, blocked by
low-index, generation-zero entities, which is to say by bodies that were
standing there before the traffic arrived and never moved.

`vehicle::delivery` was one of them, and its own module note says it should not
have been. The note is explicit that the van stands *in the parking lane*
rather than the running one, because "`ai::traffic` drives edge to edge with no
idea the van is there". The code did not do that:

- It stood the van `width * 0.5 - 1.9` from the centreline, a number justified
  by a comment saying parked cars sit 1.6 m in. They have stood at
  `spawn::kerb_offset` since the row learned its own depth, so the van was
  further into the carriageway than the row it was supposed to be in. The test
  agreed because both numbers were written down in the test.
- It did that on *every* street, including the ones with no parking lane at
  all. On a four-metre Gasse `width * 0.5 - 1.9` is ten centimetres off the
  centreline: a van parked across a street two cars already could not pass on.
- It never checked whether anything was already standing there. A van dropped
  into an occupied bay is two bodies in one place, which Avian resolves by
  firing one of them over the rooftops — the patrol's "a parked car is going up
  at 25 m/s" complaint, which had been showing up unexplained for two rounds of
  QA.

It now stands exactly where a parked one of the same body would, on the kerb
`steering::parked_kerb` names, only on streets that have a parking lane, and
only where eight places stepped along the street find one that is clear.

### Measured while measuring something else

Two findings the width arithmetic turned up, both recorded rather than acted on:

- **A band can be wider than the gap between its own walls.** Measuring the
  committed atlas's building rectangles the way `cap_widths` measures them,
  Leukstraße is drawn at 8.0 m between walls 6.8 m apart — `BAND_FLOOR = 8.0`
  overrode a real narrowing, so its houses stand in its road. It was the third
  worst street for recoveries in the baseline.
- **The Altstadt's pavements are right.** 15.1 m of carriageway plus two 3.2 m
  pavements is 21.5 m in a 21.9 m gap: the pavement stops 20 cm short of the
  wall. An aerial frame reads as though there were bare ground between the two,
  and there is not — that is the sett carriageway, which really is fifteen
  metres wide. Wittstraße (54 m between walls, 22 m drawn) and Gutenbergweg
  (43 against 26) are where an unpaved margin genuinely exists.

### Stable place foundation

- Every streamed `Shopfront` now carries a deterministic `PlaceId` derived from
  its world position. The id survives chunk despawn and is stored on an
  `Errand` alongside the target position, so a future schedule can record
  visits without retaining a render entity.
- The first place catalogue is intentionally the existing shopfront set;
  entrances, opening hours, capacities and home/work assignment remain part of
  the next data-model step. The id is quantised to decimetres and ignores
  height, keeping regeneration stable while distinguishing adjacent fronts.

## Next increments, in dependency order

1. **Persistent residents and places.** Separate `CitizenId` and resident state
   from the visible entity. Generate a finite roster and place catalogue from
   independent RNG streams. Preserve appearance, disposition, location, current
   activity and relevant mood/relationship deltas across streaming. Places need
   stable ids, entrances, opening hours and capacities independent of meshes.
   Save the dynamic state with explicit format versioning/migration. Establish
   invariants against duplicate residents, lost identities and growing records.
2. **Directed transport topology.** Derive lanes, sidewalk sides, crossings and
   turning connections from the resident road layout. Distinguish polyline bend
   nodes from intersections. Extend the atlas bake for direction/access/speed
   metadata, with explicit defaults where absent. Rendering and routing consume
   the same road rules. Preserve ODbL attribution and deterministic generation.
   The current seed of this work is the shared `TurnKind` classification and
   distance based approach braking; signals and conflict reservations build on
   that intent instead of guessing from steering after the fact.
3. **Driving and intersections.** Route-aware leader detection and predictive
   following adapted to the existing Avian controller, stop lines, conflicting
   movements, right of way, traffic lights, downstream space and pedestrian/
   cyclist interaction. Add recovery for actual deadlocks. A normal queue must
   not be solved by removing its cars.

   Partly landed. `ai::junction` reserves a crossing per *movement* and holds
   the cars whose movements conflict with one already granted, at the same
   stop line `ai::giveway` uses; `world::signals` drives the masts that have
   stood dark since they were built, and a car may only claim a signalled
   crossing on a green. Conflict is geometric rather than a turn-kind table —
   see the note below — and priority is main road, then *rechts vor links*,
   then the longest wait, then the entity id.

   What is **not** done from this increment: downstream space (nothing checks
   there is room on the far side before letting a car in, only that the
   claimant has not stopped short for three seconds), pedestrian and cyclist
   interaction with the phase (nothing on foot reads a signal), predictive
   following, and deadlock recovery beyond the twenty-two-second timeout that
   takes a crossing back from a wreck. The reservation is also a point rather
   than a set of lane connectors: two movements whose arcs miss each other are
   still made to take turns if their chords cross.

   The conflict rule is worth recording, because the version it replaces was
   wrong in a way that read as reasonable. `movements_conflict` was a table
   over `TurnKind`, and a turn kind is named relative to the car making it —
   so two cars arriving from perpendicular arms and going straight on are both
   `Straight`, and the table let them share the junction. They meet in the
   middle of it. The rule now tests whether the two movements' chords across
   the junction cross, each drawn in its own travel lane, which gets the
   perpendicular case right and gives "a left turn waits for the oncoming
   straight and a right turn does not" for free rather than as another table.
4. **One complete day.** Needs and commitments select goals; small action plans
   execute them. Start with home/work/shop/cafe, queues, parking and walking to
   the entrance. Interrupted activities can resume or choose a valid alternate.
   Match simulation-clock speed to believable travel and service durations.
5. **Relationships and memory.** Bounded memories and actual witnessed events
   affect future encounters. Build on mood, grudge, apology and social systems.
   Gestures and attention make motives readable; quiet periods limit spectacle.
6. **Citywide simulation.** Detailed nearby bodies, simplified route progress in
   the middle distance, scheduled events for distant/indoor residents. Preserve
   identities, occupancy and queue continuity at every transition. Add delivery
   demand, events and eventually transit after the basic daily loop is sound.

## Acceptance and measurement

Use a small complete neighbourhood first (proposed: 100 residents, 20 cars,
several junction types, workplaces, homes, a shop, cafe and delivery). These are
test targets, not measured capacity promises.

- Pure tests: waiting vs blocked movement, uninterrupted queues, right-of-way
  conflicts, route completion/failure, identity uniqueness, save round trips and
  deterministic generation independent of visitation order.
- Runtime: capture starts the full schedule to catch ECS query conflicts. Film
  patrols to examine movement and compare controlled scenarios with and without
  player interference. Add deterministic scenario fixtures as routing lands.
- Test scenes: a long queue, a static obstruction, an oncoming vehicle, a fallen
  car, a narrow bend, a pedestrian crossing, a delivery and a blocked junction.
- Measure completed trips, waiting distributions, near collisions, recoveries,
  blocked episodes, population, asset counts and CPU/frame-time percentiles.
  Compare settled captures with `--frames 200`, pinned hour/style/seed and
  adjacent baseline/candidate runs. Never infer performance from filmed FPS.
- Maintain bounded history and resource ownership. World layout determinism
  stays mandatory; deterministic decision replay is a separate future contract,
  not a promise of cross-platform bit-identical rigid-body physics.

Technical references: [IDM](https://traffic-simulation.de/info/info_IDM.html),
[SUMO intersections](https://sumo.dlr.de/userdoc/Simulation/Intersections.html),
[ORCA](https://gamma.cs.unc.edu/ORCA/). These are design references rather than
new dependencies; collision avoidance needs adaptation to dynamic rubber bodies.

## First recorded QA, 2026-09-11

Validation: 591 unit tests pass (including 10 new observer/traffic tests),
`cargo clippy --all-targets -- -D warnings` passes, release build succeeds,
and the full-app filmed capture completes without a panic. Changed Rust files
pass rustfmt; a repository-wide format check also finds pre-existing formatting
differences in unrelated files, which were left alone.

Artifacts are under `shots/living-city-qa/`: `patrol.mp4`, `patrol.log`,
`metrics.json`, `contact-sheet.png` and `QA.md`. The film uses saved-frame log
timestamps for playback durations; it is sparsely sampled, not full-frame-rate
video or a performance benchmark.

The 90-second Landshuepf patrol visits 7 junctions and records 43 traffic
recoveries. Ten-second samples see up to 37 blocked agents; the last sample
contains 204 observed bodies and 188 cumulative blocked episodes (episodes can
repeat for one body). The existing patrol watchdog reports 3 complaints: a
parked car launched upwards and two twelve-second player stalls. These results
are a baseline for further work, not a clean navigation acceptance or a measured
before/after improvement. Unit tests specifically verify queue preservation.

Before expanding the population, investigate the recorded narrow-street and
obstacle failures and establish reproducible routing fixtures. In the film the
automatic player drives near streetworks and later remains against a building
corner in a narrow lane. Resolve these in the transport/navigation increments;
the resident registry remains needed for persistent lives.
