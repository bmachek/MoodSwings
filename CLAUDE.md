# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
cargo run                      # debug; ~2-10s incremental after the first build
cargo run --release            # smoother frame rate, slower to compile
cargo run --features dev       # bevy dynamic_linking — fastest iteration
cargo test                     # 741 unit tests (750 with --workspace), all inline #[cfg(test)]
cargo test citygen             # one module's tests (filter by name substring)
cargo clippy --all-targets -- -D warnings
cargo fmt
tools/fetch-materials.sh       # CC0 assets: PBR sets (optional) and the recorded sound bank (REQUIRED — see below)
tools/fetch-materials.bat      # the same for Windows — KEEP THE TWO IN SYNC (see below)
tools/bake-city.py --relabel assets/cities/landshut.ron   # re-apply the landmark register
cargo run -- --audition shots/audio   # write the whole sound bank out as WAVs
```

Optional cargo features: `raytracing` (pulls in `bevy_solari`), `dlss` (needs an
NVIDIA GPU and the vendor SDK). Neither is on by default, and neither is wired
to a pass yet — say so before promising either to anybody. `raytracing` gates
exactly one block (`render::mod`), which asks the GPU whether it has the wgpu
features Solari needs; nothing ever attaches `SolariLighting`.
`Upscaling::Dlss` is reachable from the dev panel and from a save file, no
preset selects it, and `GraphicsSettings::downgrade` rewrites it to TAA
unconditionally because nothing attaches Bevy's `Dlss` component —
`render::quality` says as much in its own comments. What they are today is
compile-time capability probes, and CI compiles `raytracing` on every pull
request so that at least stays true.

### Verifying rendering changes

There is no way to eyeball this game from a terminal except the capture harness.
`core::capture` renders to an offscreen texture (window capture returns black when
the OS never composited the window), holds a few warmup frames, writes a PNG and
exits:

```sh
cargo run -- --screenshot shots/street.png --at-node 300 --eye 1.7 --hour 21.5
cargo run -- --screenshot shots/city.png --at 0,620,900 --look 0,20,-200 --stream-radius 1800
cargo run -- --screenshot shots/cast.png --lineup --hour 12
cargo run -- --screenshot shots/minga.png --city minga --at-node 300 --hour 12
tools/shoot.sh                 # the whole battery of framings into shots/
tools/shoot.sh --only street,night --out shots/after
```

### Letting the game find its own bugs

`core::capture` renders one *posed* frame. `core::patrol` plays instead:

```sh
cargo run --release -- --patrol 120        # two minutes, then a report
cargo run --release -- --patrol 60 --city minga
```

It writes `ActionState<Action>` directly — gameplay has never read a key, only
the action — and walks the city junction by junction, taunting, whistling and
taking a car, while `Watch` takes the city's vital signs once a second: the
player's position and speed, moods outside their range, entity and *asset*
counts that only ever climb (the documented leak: a mesh built in the streaming
path is added afresh every time a chunk comes back), audio sources being mixed,
frame hitches, and anything that belongs on the road found above the rooftops.
A patrol ending with no complaints is the point. It found the parked cars that
were being fired into the sky by the kerb collider arriving inside them.

`--lineup` stands one of every archetype in a row and points the camera down
it — the cast's `--showroom`. The rare archetypes are the ones whose costume
goes wrong and the ones a street framing cannot be relied on to contain, so
"shoot a street and hope" is not a check on them.

`tools/shoot.sh` exists so a rendering change is judged against the last render
rather than against a memory of it: shoot the same framings before and after.
Pin `--hour` on any shot being compared — the clock and the weather run together,
so an unpinned shot drifts its own sky between runs. `--city` builds a given
`CityStyle` instead of the persisted one, which is the only way to shoot a
postcard the player has not selected. Full flag table is in README.md.

`--film <dir>` records instead of posing: a numbered frame every time the
encoder is free, for as long as the run lasts. Paired with `--patrol`, which
drives the player, it is the game playing itself with a camera on it, and the
frames assemble into a video with `ffmpeg`. It is the only instrument that
catches what is wrong only while the game is *running* — the first take found
an audio-source leak, two parked cars inside each other and a patrol that could
not walk round a building.

`--fps-log` reports median, p95 and worst frame time — over the *warmup* frames,
and the default warmup is short enough that it is timing a half-built scene.
Always pass `--frames 200` for a number worth quoting: the same street framing
reads about 19ms over the default window and about 28ms once everything is
resident and the crowd is up. Mixing the two windows has produced two false
readings already — a "free" change and a "2ms regression" that were both noise —
so compare like with like, and rerun two or three times, because the spread over
a settled window is under half a millisecond and over the default one is not.

Compare like with like in *time* as well. The same binary and the same framing
measured 30.4 ms with the machine cool and 35.5 ms twenty minutes later, which
is bigger than most changes worth making; a before/after taken half an hour
apart is measuring the fan. Build the old commit and the new one back to back
and shoot them in the same minute — `git stash -u`, `git checkout <base>`,
`cargo build --release`, measure, come back. It costs three minutes of
compiling and it is the difference between "eleven percent slower" and "twelve
percent faster", both of which this repository has now reported about the same
change.

The seed a capture builds is the *persisted* one from the player's options file,
not the code default — so a position probed in a citygen unit test is a position
in a different city. Probe with a temporary `info!` in the spawn path instead.

Capture mode is not just a camera: `core::capture::is_capture_mode()` gates the
dev panel off (`ui`) and mutes audio, and several systems check it. Anything that
would spoil an unattended shot should check it too.

### Verifying physics without a window

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

### Verifying audio changes

Same problem, same answer. `--audition <dir>` writes every sound in the bank to
a WAV and exits without starting Bevy at all, so a curse can be listened to
without finding a flummi cross enough to say one. `audio::bank::REGISTER` is
what it enumerates — the one list of every sound, its peak and its shape, which
the loader and the fetch-script sync tests read too, so a sound cannot exist in
one of them and be forgotten by another. What is auditioned is the *processed*
buffer: every recording is held to the bank's rules mechanically at load (mono
mix, resample, fade, normalise, seam-wrap), and the load pipeline itself is
tested with fixture files. A register entry with no recording on disk prints
as MISSING and plays in-game as a short silence with a warning.

## The instruments have agents

Every instrument above — the gate, the camera, the patrol, the survey, the
audition — is also a subagent in `.claude/agents/`, because each one produces
either thousands of lines of compiler output or a protocol that is easy to get
subtly wrong, and both are better kept out of the main conversation. Delegate to
them; they carry the discipline that the section above spends its length
explaining.

| Agent | Reach for it when |
|---|---|
| `rust-verify` | anything changed — fmt, clippy `-D warnings`, tests, the build CI does separately |
| `code-explorer` | the question sweeps several of the 123 modules and you want the answer, not the files |
| `trap-reviewer` | before offering a change: the diff against the traps below and the conventions that are rules |
| `town-surveyor` | `world` moved — the numeric scorecard, the atlas and the bake; no window needed |
| `render-shooter` | anything visible moved — the same framings before and after, and a look at the PNGs |
| `patrol-warden` | it can only go wrong while running — the patrol, the Watch, `--film` |
| `audio-keeper` | the bank — the audition, and `REGISTER` against both fetch scripts and `CREDITS.md` |
| `perf-scout` | a frame-time claim, held to the protocol that keeps it from being noise |
| `docs-steward` | the written record, starting with the `CLAUDE.md`/`AGENTS.md` twins |

`/ship` runs the three that every change owes — verify, traps, docs — in
parallel, and ends with what remains *unverified*. The rest are `/verify`,
`/shot`, `/patrol`, `/survey`, `/audition`, `/traps`, `/perf`, `/docs-sync` and
`/setup`.

Two scripts hold up the parts that used to be discovered the hard way:

```sh
tools/dev-setup.sh          # the four things that stop a clone dead, none of them loudly
tools/check-docs.sh         # CLAUDE.md and AGENTS.md are twins; --fix rewrites the second
```

`tools/dev-setup.sh` runs at session start (see `.claude/settings.json`) and
reports the one fact that decides which instrument is even available: whether
there is a GPU. `cargo test`, `--survey` and `--audition` never open a window and
work anywhere; `--screenshot`, `--patrol`, `--film` and `--fps-log` need an
adapter, and a container has none until something installs one. An unrun check is
never a passed one — say which it was.

## Architecture

Bevy 0.19 app; `main.rs` installs `DefaultPlugins` then one plugin per top-level
module. Physics is Avian 3D, input is leafwing-input-manager, the dev panel is
bevy_egui, saves are RON.

| Module | What lives there |
|---|---|
| `core` | States, schedule sets, `GameConfig` tunables, persisted settings/keybindings (`core::settings`), deterministic RNG, asset-root resolution, the screenshot harness |
| `world` | City generator (incl. building kinds & vacant-lot zoning), road graph, chunk streaming, day/night, weather, the cloud deck overhead (`sky`), the shape of the ground and the hills round the town (`terrain`), the variation that keeps open ground from being one green (`ground`), how high everything lying flat on it is laid (`layer`), facades/LOD shells, window interiors, walk-in ground floors (`interior`), drivable parking decks (`garage`), painted signs, ad posters & civic frontages (`signage`), park monuments (`statues`), the fountains and columns a real town keeps on its own squares (`monument`), real street networks baked from OpenStreetMap (`atlas`), the frontages that fill them (`streetside`), the enamel plates on their corners (`streetname`) and the law under those — one-way arrows, no-entry discs, the boundary of the Fussgaengerzone (`roadsign`), churches & cathedrals (`church`), stepped gables and pitched roofs for the old-town postcards (`gable`), the stadium and its Welle (`stadium`), the canal and its bridges (`river`), the parapets, lamps and pier heads where a street crosses a real river (`bridge`), lot furnishing (`lots`), what a building hangs on its face and puts out in front of it — pipes, boards, window boxes, bikes, terraces, dishes, tags and roller shutters on a clock (`frontage`), rubbish that scatters when you walk through it (`litter`), streetworks (`worksite`), pennants and washing strung over the narrow streets (`bunting`), chimney smoke and gully steam (`plume`), road wear, vegetation, props, world damage (`mayhem`), procedural + scanned textures |
| `bounce` | The elastic simulation: bounce controller, impact response, launch, squash |
| `player` | Input mapping, on-foot movement, camera rig, enter/exit |
| `vehicle` | Arcade vehicle physics, specs, bodywork, comedy crash response (`impact`), lights, parked-car spawning, vans stopped with their hazards on and the courier unloading them (`delivery`) |
| `mood` | How a flummi feels (`feeling`), the painted face it wears (`face`), what it says (`voice`), taunting and cheering (`provoke`), and retaliation (`grudge`) |
| `ai` | Traffic, pedestrians, archetypes (the cast), shared steering, walk cycles, the figure itself, the animals among their feet, pigeon flocks that scatter (`pigeon`), umbrellas when it rains (`brolly`), a citizen who has stopped to play and the ring the city's mood gathers round them (`busker`), the one point everything ambient is kept around (`focus`), somewhere to be (`errands`), standing in line for it and minding who pushes in (`queue`), and stepping off a kerb (`crossing`) |
| `events` | The city's calendar: scheduled parades (CSD, demos) marching graph routes; `--event` is capture's door in |
| `render` | Quality presets, atmosphere, exposure, bloom, shadows, volumetrics, post stack |
| `ui` | HUD, minimap, egui dev tuning panel, the `Escape` pause menu |
| `audio` | The recorded sound bank (`bank::REGISTER`), the load-time discipline (`files`), triggers, the master limiter, the WAV audition tool |
| `save` | RON quick save / load |

### What this game is

It was an open-world crime sandbox and it is now a comedy one. Everything is
made of rubber and bounces; there are no weapons, no police, no health and no
fail state. The verb is provocation — a raspberry and a whistle — and the
readout is a mood that every citizen carries, wears as a face, says out loud
and catches off the neighbours. When a change has to break a tie, break it
towards the joke.

### Determinism is the load-bearing constraint

The whole city is regenerated from `GameConfig::world_seed` on demand — chunks
are respawned, not stored — so generation must be pure and reproducible. Hence
`core::rng`: every subsystem draws from its own stream (`stream_for(seed, key)`,
`stream_for_chunk(...)` for per-chunk work) with a fixed key from `rng::stream`.
Never share a stream between subsystems and never reuse a key value: with one
shared RNG, adding a draw in the building generator silently reshuffles every
street downstream. Subsystems that sample noise rather than draw use `key_for`.

Because the world derives from the seed, `save::SaveGame` stores only what does
not: seed, player position, hour. Bump `SAVE_VERSION` on any incompatible change.
The mood is deliberately not in there — see the README's limitations.

Temperaments and voices draw from `stream::MOOD`, not `stream::PEDESTRIANS`.
Sharing would mean that retuning a fuse also moves where the next citizen spawns
and which street they walk down.

### Schedule

`core::schedule::GameSet` is the one ordering for game logic:
`Input → Ai → Simulation → Camera → Ui`, chained in `Update`, with `Ai`,
`Simulation` and `Camera` gated on both `AppState::InGame` and
`InGameState::Playing`. Put new gameplay systems in a set rather than growing
`.after()` chains across plugins. Physics is deliberately outside it — Avian
owns `PhysicsSchedule`, and vehicle forces are applied in `FixedUpdate`
(`vehicle::controller`) because Avian clears forces each tick — which is also
why `ui::menu` pauses `Time<Physics>` itself rather than relying on the
`GameSet` gate alone when `Escape` opens the pause menu (`InGameState::Paused`).
States are `AppState` (Loading/Menu/InGame) with an `InGameState` sub-state;
startup currently skips straight into the game.

### Layout vs. entities

`world::citygen` builds the whole layout (blocks, streets, road graph) once and
keeps it resident — traffic and the minimap query parts of the city the player
cannot see. Only meshes and colliders stream, per 250 m chunk, in
`world::streaming`. Anything spawned by streaming must keep its `ChunkOf`, or it
leaks.

`CityStyle` (`core::config`) is the second input to that layout, next to the
seed: a postcard, and — for exactly one style — a map underneath it. It is a handful of dials — height scale, how far
lots subdivide, whether the low buildings step into a gable, palette override,
how many churches and whether one of them is a cathedral, how wide the market
band runs, how much the walls advertise — so a style is a *tuning*
of the same generator, and a new one costs a match arm rather than a data file.
`CityStyle::atlas` is the exception that proves the rest: `Landshuepf` names a
baked OpenStreetMap extract (`assets/cities/landshut.ron`) and gets Landshut's
real street network, and every other dial still applies on top of it. A town
read off a map has no rectangular blocks, so it is filled by `world::streetside`
instead — frontages marched down each side of each street — and its buildings
carry `Building::facing`. The layout is then a pure function of
`(seed, style, atlas)`; chunks still respawn identically, which is what the rule
was protecting. The map data is ODbL, the only non-CC0 asset here: see
CREDITS.md before touching it.

Anything a style decides belongs on `CityStyle`, not as a `match` at the use
site; the layout must stay a pure function of `(seed, style)` or the chunks
stop respawning the same city.

### A postcard can also be a real town

`CityStyle` is a tuning of the generator — heights, a ceiling over them
(`height_cap`), plot width, gables, palette, whether the walls are rendered
(`rendered`) or faced with a scanned grain, how many spires. Anything a style
decides belongs on `CityStyle` rather than as a `match` at the use site, and
`heights` in particular exists because the generator and the atlas both draw
building heights and had each been applying `height_scale` on their own.

A style may also name a baked OSM extract (`atlas()`), and Landshüpf does: the
road graph is the real Landshut, the blocks are empty, and `streetside::lots`
marches plots along every street instead. A plot there is placed square to *its*
street and therefore at some arbitrary angle to every other one, which is why
`streetside::Oblong` tests candidates against the road corridors with a
separating-axis test rather than a box overlap. Street names ride beside the
layout in `atlas::Signposts` — a name is a fact about a street and an edge is
one segment of one.

The atlas also carries the town's real buildings, each polygon cut at bake time
into the rectangles that cover it (`Footprint::group` gathers the parts of one
building back into one `Block` sharing a height, a palette and a kind — an L is
two parts, a courtyard block four, and `streetside::lots` files every part), the
roof shape where the map recorded one (`Building::roof`), and the shape of the
ground (`atlas::Relief`, two grids of metres above the valley floor off the
Copernicus elevation model). The bake is `tools/bake-city.py`; it reads Overture
Maps for the polygons and the DEM tile for the relief because Overpass cannot
supply either, and a bake of its own output is its own output — `--from-ron`
regenerates the buildings and the relief from a committed file, so do not add a
step that measures something the previous bake already moved (the band tidy in
`tidy_bands` is the model: it cuts a looped band to its spine and re-attaches
the side streets, and a second pass finds no loop and nothing loose). The tests
on the committed file fail when the file is there and does not load; they used
to return early, which is how a bake that wrote `group: 0` where the runtime
wanted `Some(0)` passed every test while the game quietly fell back to the
generator.

An OSM way can be a *loop* — the Altstadt is one closed way round the market —
and after `merge_parallel` and the recentring both of its lanes lie on one
centreline, so the runtime would draw every edge twice and lay a pavement
across the carriageway at each turn-round. Look at the band's polyline before
trusting a ribbon artefact to the renderer: `runs_of` in the bake is the test.

### One mapped building is several boxes

The bake cuts a real building's polygon into up to six rectangles sharing a
`group` — an L is two, a courtyard block four — and `atlas::footprints` emits
every one of them as a `Building`, because every one of them is a wall that has
to be drawn. Only one of them is *the* building, and `Building::annex` says
which: the first part the bake emits is the largest, and everything a building
has exactly one of hangs off that. The sign over the door, the door, the room
behind it, the painted civic ground storey, the advert on the blind flank, and
the whole structure of a kind that owns one (`BuildingKind::owns_its_structure`
— church, gate, tower, stadium, garage). Before that existed the Stadtresidenz
wore six identical RATHAUS plaques and the one hotel in the Altstadt advertised
itself under four names, one per wing.

Two rules ride with it. A part thinner than `buildings::SLIVER` is a jog in a
wall rather than a wing — 490 of Landshut's 4672 parts have a side under three
metres — so it keeps its wall and takes a flat deck instead of growing its own
gable out of the side of the roof next to it. And `streetside::lots` drops parts
one at a time, so whatever survives, the largest survivor is promoted back out
of `annex`: a building whose principal part stood in a road was otherwise left
as wings with no door and no name.

A named landmark wears its own name (`signage::SignKit::landmark`), not its
kind's plaque. The generic plaques are written for an invented city — every
church in it is SANKT BOING, every civic building is RATHAUS — and Landshut's
six town-hall-shaped buildings are a palace, a ministry and three courts, none
of which is the Rathaus.

What a landmark *is* comes from `KNOWN_LANDMARKS` in `tools/bake-city.py`, and
that register is a decision rather than a measurement: it changes more often
than the map does. `--relabel` re-applies it to the committed atlas by line
surgery — filling a `kind` the source left blank, overriding a height that has
a citation, touching nothing else — so a name arriving on the list costs one
diff hunk rather than a re-download of Overpass, Overture and a DEM tile. It is
idempotent, it measures nothing (see the `tidy_bands` rule above), and a Rust
test fails if the committed file has not had it run.

`--relabel` is one of three line-surgery modes, and they share that contract:
each writes one kind of fact onto a committed atlas, fills rather than
overrules, measures nothing that is written down, and writes the same file
twice.

```sh
tools/bake-city.py --relabel assets/cities/landshut.ron
tools/bake-city.py --reflag  assets/cities/landshut.ron roads.json
tools/bake-city.py --rename  assets/cities/landshut.ron built.json
```

The other two exist because the bake has always held tags it never wrote down.
`--reflag` takes the roads dump's `oneway` and `highway=pedestrian`: nine per
cent of Landshut's road length is one-way and 4.8 km of it is a
Fussgaengerzone, the Altstadt included, and `roadgraph::Rules` is what
`ai::traffic`, `vehicle::spawn` and `world::roadsign` read. `--rename` takes
the buildings dump's `name` and its `kind_of` tags, which Overture — where the
footprints come from — does not carry: 81 named buildings became 171, and the
Finanzamt, the Galeria and the Sparkasse wear their own names.

Both match by geometry rather than by identity, because the bake keeps no OSM
ids: a street by walking its polyline and letting the samples vote, a building
by containment with a size guard in *both* directions — a forty-square-metre
`man_made=tower` next door will otherwise hand its tags to the six-hundred-
metre hall whose middle lands beside it, and the runtime draws a thirty-seven-
metre pyramid of brick. A name or a kind is written per *building*, never per
part: one wing named and its siblings blank is two buildings that touch.

### A bridge cannot be raised

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

### The traps

Seven things here have bitten more than once and none of them fail loudly:

- **The ground is only flat where the town is.** `world::terrain` displaces it,
  and about thirty spawners write a world y directly (`SIDEWALK_HEIGHT`,
  `resting_height(spec)`, a bare `0.0`) meaning "the ground here is at zero".
  That stays true only because `Terrain::height` returns *exactly* zero inside
  a corridor rasterised from the road graph — and, with a real relief under
  Landshut, because `atlas::HILL` clips any street the map takes more than
  twelve metres up the Hofberg rather than cutting the hill away round it.
  Anything new that places geometry well away from a street has to ask
  `Terrain` for the height (the hillside wood and the mapped open ground do),
  and anything that widens where the town builds has to widen
  `terrain::LEVEL_REACH` with it. The one thing built off the floor is a
  landmark kept up on the hill: it carries `Building::ground`, every y in its
  path is written off that, and the terrain holds a plateau at that height
  under it — the level field carries a height as well as a weight for exactly
  this. A test walks every edge of the committed Landshut and asserts the
  corridor, and another asserts the plateau under the castle.
- **Nothing flat may be laid at the same height as anything else flat.**
  `world::layer` owns the whole ground stack in whole millimetres, with a slot
  per instance. The depth buffer is not the constraint — it resolves microns —
  the constraint is that a world coordinate is an `f32` and this town runs to
  1700 m, where the spacing between representable numbers is 200 µm. Two
  surfaces closer than that come out at the same depth, TAA's per-frame jitter
  picks the winner, and the result flickers. Add a layer to that table rather
  than picking a number beside the spawner.


- **Restitution is a property of a contact.** A body held off the ground by a
  spring — a floating character controller, a car on raycast suspension — never
  forms one, so declaring it elastic does nothing. `bounce::controller` applies
  the hop by hand for that reason. The corollary cost a year: a landing that
  fires anywhere inside the ground probe's generous reach is a *second* way of
  never forming a contact, and the rebound being assigned rather than added
  makes that height free for ever. `grounded` is the wide reach, for steering
  and for jump permission; `touching` is the narrow one, and only it decides a
  landing. A fast fall crosses the whole contact shell between two frames, so a
  landing is also recognised by the fall reversing.
- **A `Transform` written from `Update` onto a rigid body is a teleport.**
  `world::mod` runs Avian with `PhysicsInterpolationPlugin::interpolate_all()`,
  and the easing clears its state — translation included — for any body whose
  `Transform` a game system changed. Avian then syncs the eased, lagging
  position back into `Position`. Twelve systems wrote a facing that way and the
  crowd either walked at a third of its speed or never finished turning,
  depending on how the scheduler ordered an unordered pair. Write Avian's
  `Rotation` (and `Position`) instead; `ai::steering::face` is the one place
  that does it for a facing. None of it shows in a still, so the capture
  harness cannot catch it — `bounce::testing::physics_app` installs the same
  interpolation so a `cargo test` can.
- **Avian scales a collider by its transform.** Squash and stretch is applied to
  a figure's *children*, off their `Rest` pose; scaling the body entity would
  flatten the collider and sink the figure through the pavement. A child with no
  `Rest` is skipped by `figure::animate`, so a new part needs one or it will not
  follow the squash.
- **Two queries in one system may not both touch a component if either is
  mutable.** Bevy panics at first run, not at compile time, and no unit test
  builds the whole app — so the capture harness is the integration test for the
  schedule. It has caught this twice. Split the system or use a `ParamSet`; do
  not make the queries disjoint with a filter that quietly changes behaviour.
  Capture cannot reach a system that only runs behind a flag, though, and
  `multiplayer::receive` — three mutable `Transform` queries, local body,
  remote replica, remote actor — died the moment a server answered. Where the
  markers really are exclusive, spelling that out in `Without` is the honest
  fix, and `multiplayer`'s test is the cheap way to prove it: initialising a
  system is enough to trip the check, and needs none of the resources it reads.
- **`figure::FigureAssets` is inserted by `pedestrian::setup` and read by the
  player spawn**, and `mood::face::FaceAssets` by both. The ordering is
  Startup → PostStartup and is silently load-bearing.

### Rendering is preset-driven

Never switch a renderer feature on directly. A `QualityPreset` (low → photo)
resolves in `render::quality` to a flat `GraphicsSettings` block, which
`GraphicsSettings::downgrade` walks back to what the GPU actually reports, and
`render::sync_camera_stack` then makes the camera match — so preset changes take
effect live from the dev panel. `GraphicsSettings` is serialised into saves,
which is why the types are ours rather than Bevy's, and preset/downgrade rules
are pure functions so they can be unit-tested without a GPU.

### Assets are generated, or absent

No third-party art ships in the repo. Every texture is painted per-pixel at
startup (`world::texture`), and `world::material` returns `Option` for every
scanned-set lookup — the CC0 material downloads remain an *optional* upgrade,
and a fresh clone renders identically without them. Sound stopped being
symmetrical: the synthesised bank was retired (the recordings won), so the
CC0 sound half of `tools/fetch-materials.sh` is a REQUIRED setup step. A
clone that has not run it still starts, but every missing sound plays as a
short silence with a loud warning. Adding a scanned material set means adding
its name to both `world::material::set` and the fetch scripts; adding a sound
means an entry in `audio::bank::REGISTER` (name, peak, shape) plus a download
in both fetch scripts — a test fails if either script lacks one. Bevy has no runtime mip generator, so the texture
modules build mip chains on the CPU (averaged in linear space for sRGB
images).

`tools/fetch-materials.sh` and `tools/fetch-materials.bat` are twins and MUST
be kept in sync: any material, sound, or behaviour change in one gets mirrored
in the other in the same commit. The lists are the contract; only the shell
plumbing may differ.

The asset root is resolved explicitly in `core::assets::root()` and handed to
`AssetPlugin`, because Bevy's default resolves against the executable and
`target/release/assets/` does not exist — the failure is silent.

Custom shaders live in `assets/shaders/` (`facade.wgsl`, `road.wgsl`) behind
`MaterialExtension`s. Uniform struct field order in Rust must match the WGSL
declaration order.

## Conventions

- Comments here explain *why*, often at length, and frequently record what was
  tried and rejected. Match that: a change that reverses a documented decision
  should update the comment that documents it.
- Player-facing text is German (`ui::menu`, `ui::hud`); everything a developer
  reads — the dev panel, log lines, identifiers, comments — is English.
- Crate-level lints in `main.rs` allow `dead_code` (foundations land a milestone
  before their callers), `clippy::type_complexity` and `clippy::too_many_arguments`
  (Bevy query filters and system params are the meaning). Clippy must otherwise
  pass with `-D warnings`.
- Tests are inline `#[cfg(test)]` modules next to the code, with sentence-shaped
  names (`the_root_is_the_source_trees_assets_directory`). Pure logic — layout,
  preset tables, road-graph queries — is tested; rendering is verified by capture.
- Feel constants belong in `core::config::GameConfig` so the dev panel can tune
  them at runtime, not as literals at the use site. The five temperaments are a
  `Tempers` resource for the same reason.
- The project is GPL-3.0-or-later (`LICENSE`). `CREDITS.md` names everybody
  whose CC0 recording is in the bank — the licence does not require it and we
  do it anyway — and `CONTRIBUTING.md` carries the etiquette a patch is held
  to. A new sound or material set means a line in `CREDITS.md` as well as in
  `bank::REGISTER` and both fetch scripts.
- Every sound is a CC0 recording, held to the bank's rules at load: one-shots
  start at exactly zero and everything peaks at its `REGISTER` peak (inside
  `0.3..=1.0`). Add new sounds to `audio::bank::REGISTER` and to both fetch
  scripts; the register is what the loader, the audition tool and the
  fetch-sync tests all enumerate.
