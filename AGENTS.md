# AGENTS.md

Guidance for coding agents working in this repository. Codex reads this file as
it stands; Claude Code reads it through the `@AGENTS.md` import in `CLAUDE.md`.
There is one copy of it, and this is the copy.

## What this game is

It was an open-world crime sandbox and it is now a comedy one. Everything is
made of rubber and bounces; there are no weapons, no police, no health and no
fail state. The verb is provocation — a raspberry and a whistle — and the
readout is a mood that every citizen carries, wears as a face, says out loud
and catches off the neighbours. When a change has to break a tie, break it
towards the joke.

## Commands

```sh
cargo run                      # debug; ~2-10s incremental after the first build
cargo run --release            # smoother frame rate, slower to compile
cargo run --features dev       # bevy dynamic_linking — fastest iteration
cargo test                     # every unit test, all inline #[cfg(test)]; --workspace adds the multiplayer crate
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

## Verifying a change

There is no way to eyeball this game from a terminal except by instrument,
and every instrument has a protocol that is easy to get subtly wrong. The
protocols are carried in full by the agents below and by
`.claude/rules/verification.md`; these are the parts that get broken:

- **Capture** — `--screenshot` and `tools/shoot.sh` render one *posed* frame
  offscreen and exit. Shoot the same framings before and after, and pin
  `--hour` on anything compared: the clock and the weather run together.
  `--city` builds a style the player has not selected, `--lineup` is the cast's
  showroom, and the seed a capture builds is the *persisted* one, not the code
  default.
- **Patrol** — `--patrol <seconds>` plays instead of posing while `Watch` takes
  the city's vital signs; `--film <dir>` puts a camera on it. It is the only
  instrument for what goes wrong only while the game is running.
- **Frame time** — `--fps-log` means `--release`, `--frames 200`, two or three
  runs, and both commits built back to back and measured in the same minute.
  Each of those has a false reading in this repository's history behind it.
- **Physics** — `bounce::testing::physics_app` is a headless Avian world built
  the way the real one is; drive it with `TimeUpdateStrategy::ManualDuration`,
  over `Gait::ALL` (`.claude/rules/physics.md`).
- **Audio** — `--audition <dir>` writes the processed bank as WAVs and exits
  without starting Bevy (`.claude/rules/audio.md`).
- **Capture mode is not just a camera.** `core::capture::is_capture_mode()`
  gates the dev panel off and mutes audio, and anything that would spoil an
  unattended shot should check it too.

`cargo test`, `--survey` and `--audition` never open a window; `--screenshot`,
`--patrol`, `--film` and `--fps-log` need a GPU. The full flag table is in
README.md.

## The instruments have agents

Every instrument above — the gate, the camera, the patrol, the survey, the
audition — is also a subagent in `.claude/agents/`, because each one produces
either thousands of lines of compiler output or a protocol that is easy to get
subtly wrong, and both are better kept out of the main conversation. Delegate to
them; they carry the discipline that the section above only summarises.

| Agent | Reach for it when |
|---|---|
| `rust-verify` | anything changed — fmt, clippy `-D warnings`, tests, the build CI does separately |
| `code-explorer` | the question sweeps several of the 130-odd modules and you want the answer, not the files |
| `trap-reviewer` | before offering a change: the diff against the traps below and the conventions that are rules |
| `town-surveyor` | `world` moved — the numeric scorecard, the atlas and the bake; no window needed |
| `render-shooter` | anything visible moved — the same framings before and after, and a look at the PNGs |
| `patrol-warden` | it can only go wrong while running — the patrol, the Watch, `--film` |
| `audio-keeper` | the bank — the audition, and `REGISTER` against both fetch scripts and `CREDITS.md` |
| `perf-scout` | a frame-time claim, held to the protocol that keeps it from being noise |
| `docs-steward` | the written record — this file, the `.claude/rules/` chapters, README, CREDITS — and the Codex copies of these agents |

The agents are written once, in `.claude/agents/`. `.codex/agents/` holds the
same agents for Codex — generated from those files by `tools/check-docs.sh
--fix` and held to them by the same script, so edit the Markdown, never the
TOML. In Claude Code each agent is also a slash command; `CLAUDE.md` lists
them.

Three scripts hold up the parts that used to be discovered the hard way:

```sh
tools/dev-setup.sh          # the four things that stop a clone dead, none of them loudly
tools/check-docs.sh         # this file, CLAUDE.md, the rules and both agent rosters agree
tools/base-worktree.sh REF  # the base of a before/after, checked out beside the tree
```

`tools/dev-setup.sh` runs at session start (see `.claude/settings.json` and
`.codex/hooks.json`) and reports the one fact that decides which instrument is
even available: whether there is a GPU. `cargo test`, `--survey` and `--audition` never open a window and
work anywhere; `--screenshot`, `--patrol`, `--film` and `--fps-log` need an
adapter, and a container has none until something installs one. An unrun check is
never a passed one — say which it was.

## Read before you touch

Some chapters matter only to some files. They live beside the rest of the agent
configuration, in `.claude/rules/`, and Claude Code loads each one by itself the
first time a session reads a file its `paths:` names. Every other agent: open
the chapter before you change those files. `tools/check-docs.sh` keeps this
table, the chapters and their paths in agreement — the first column *is* each
chapter's `paths:`.

| Before you change | Read |
|---|---|
| `src/world/{atlas,streetside,trading,streetname,roadsign,buildings,signage}.rs`, `tools/{bake-city.py,fetch-city.sh,fetch-overture.py}`, `assets/cities/**` | `.claude/rules/atlas.md` — a postcard that is a real town, and a mapped building that is several boxes |
| `src/world/{church,interior}.rs` | `.claude/rules/church.md` — the walk-in church, and the three things a solid nave was hiding |
| `src/world/{river,bridge}.rs` | `.claude/rules/bridge.md` — why a bridge is free, and what that cannot give you |
| `src/core/{capture,patrol,survey}.rs`, `tools/shoot.sh`, `src/render/**` | `.claude/rules/verification.md` — the capture, patrol, film and frame-time protocols in full |
| `src/bounce/**`, `src/vehicle/{controller,impact,spec}.rs`, `src/player/on_foot.rs`, `src/world/mod.rs` | `.claude/rules/physics.md` — the headless Avian world, and the two rules it enforces |
| `src/audio/**`, `tools/fetch-materials.{sh,bat}`, `CREDITS.md` | `.claude/rules/audio.md` — the audition, and what is held to the bank's rules at load |
| `.github/workflows/release.yml`, `.github/release-notes.md`, `Cargo.toml` | `.claude/rules/release.md` — the tag, the gate, the three archives and what each one is checked for |

## Architecture

Bevy 0.19 app; `main.rs` installs `DefaultPlugins` then one plugin per top-level
module. Physics is Avian 3D, input is leafwing-input-manager, the dev panel is
bevy_egui, saves are RON.

| Module | What lives there |
|---|---|
| `core` | States, schedule sets, `GameConfig` tunables, persisted settings/keybindings (`core::settings`), deterministic RNG, asset-root resolution, the screenshot harness |
| `world` | City generator (incl. building kinds & vacant-lot zoning), road graph, chunk streaming, day/night, weather, the cloud deck overhead (`sky`), the shape of the ground and the hills round the town (`terrain`), the variation that keeps open ground from being one green (`ground`), how high everything lying flat on it is laid (`layer`), facades/LOD shells, window interiors, walk-in ground floors and the nave of a walk-in church (`interior`), drivable parking decks (`garage`), painted signs, ad posters & civic frontages (`signage`), park monuments (`statues`), the fountains and columns a real town keeps on its own squares (`monument`), real street networks baked from OpenStreetMap (`atlas`), what the game is allowed to call the buildings on them (`trading`), the frontages that fill them (`streetside`), the enamel plates on their corners (`streetname`) and the law under those — one-way arrows, no-entry discs, the boundary of the Fussgaengerzone (`roadsign`), churches & cathedrals, hollowed out where they are big enough to enter (`church`), stepped gables and pitched roofs for the old-town postcards (`gable`), the stadium and its Welle (`stadium`), the canal and its bridges (`river`), the parapets, lamps and pier heads where a street crosses a real river (`bridge`), lot furnishing (`lots`), what a building hangs on its face and puts out in front of it — pipes, boards, window boxes, bikes, terraces, dishes, tags and roller shutters on a clock (`frontage`), rubbish that scatters when you walk through it (`litter`), streetworks (`worksite`), pennants and washing strung over the narrow streets (`bunting`), chimney smoke and gully steam (`plume`), road wear, vegetation, props, world damage (`mayhem`), procedural + scanned textures |
| `bounce` | The elastic simulation: bounce controller, impact response, launch, squash |
| `player` | Input mapping, on-foot movement, camera rig, enter/exit |
| `vehicle` | Arcade vehicle physics, specs, bodywork, comedy crash response (`impact`), lights, parked-car spawning, vans stopped with their hazards on and the courier unloading them (`delivery`) |
| `mood` | How a flummi feels (`feeling`), the painted face it wears (`face`), what it says (`voice`), taunting and cheering (`provoke`), retaliation (`grudge`), the collar-grab (`scuffle`), making peace with a flower (`apology`), and what the crowd feels about chaos it only watches (`schadenfreude`) |
| `ai` | Traffic, pedestrians, archetypes (the cast), shared steering, walk cycles, the figure itself, the animals among their feet, pigeon flocks that scatter (`pigeon`), umbrellas when it rains (`brolly`), a citizen who has stopped to play and the ring the city's mood gathers round them (`busker`), the one point everything ambient is kept around (`focus`), somewhere to be (`errands`), standing in line for it and minding who pushes in (`queue`), and stepping off a kerb (`crossing`) |
| `events` | The city's calendar: scheduled parades (CSD, demos) marching graph routes; `--event` is capture's door in |
| `render` | Quality presets, atmosphere, exposure, bloom, shadows, volumetrics, post stack |
| `ui` | HUD, minimap, egui dev tuning panel, the `Escape` pause menu |
| `audio` | The recorded sound bank (`bank::REGISTER`), the load-time discipline (`files`), triggers, the master limiter, the WAV audition tool |
| `save` | RON quick save / load |

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

### The traps

Ten things here have bitten more than once and none of them fail loudly:

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
- **Parry's heightfield lattice is indexed `[x][z]`, outer index first.** Avian's
  own doc says so and `world::mod`'s comment said the opposite, so
  `ground_collider` nested its loops the wrong way and handed Avian the
  *transpose* of the terrain: the ground you stood on was the ground you saw
  mirrored about `x == z`. Nothing showed it while every town was flat — the
  transpose of zero is zero — and under Landshut's real relief it was 74 m out,
  which is a Hofberg you drive straight through and an invisible fifty-metre
  shelf across a street on the other diagonal. `the_collider_lattice_runs_x_first`
  is four lines and asks the linked parry rather than anybody's memory of it;
  `the_ground_collider_agrees_with_the_ground_you_can_see` walks the committed
  Landshut off the lattice's own sample points, because a transpose is invisible
  wherever `x == z`.
- **`bevy_egui` gives its context to the first camera it finds, and then latches.**
  This app spawns two in one unordered `Startup` — the player's and the
  minimap's, which renders into a 320 by 320 texture — so which one won was
  archetype order, and it was the minimap's. Every egui surface in the game, the
  pause menu and the F3 panel both, was drawn correctly into a widget nobody was
  looking at: `Escape` froze the world and freed the cursor and no menu ever
  appeared. `ui::menu` turns `auto_create_primary_context` off and
  `player::camera::spawn_camera` carries `PrimaryEguiContext`, next to the
  `IsDefaultUiCamera` that says the same thing for Bevy's own UI. Anything that
  adds a camera has to leave both alone.
- **Pausing `Time<Physics>` stops Avian clearing forces, not systems applying
  them.** `vehicle::controller::drive_vehicles` runs in `FixedUpdate`, which the
  pause menu does not stop, so a menu left open charged the suspension up and
  fired the car on resume — five metres after a second, seventy after five, the
  height scaling with how long the player read the menu. It needs a run
  condition, not a `GameSet` gate. Anything else that ever applies a force
  outside the physics schedule needs the same one.
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
short silence with a loud warning. A release archive carries the bank (see
*Cutting a release*); a clone still has to fetch it. Adding a scanned material set means adding
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
- The agent instructions are one copy. This file is the source; `CLAUDE.md`
  imports it and adds only what is Claude Code's alone; a chapter that matters
  to a few files goes to `.claude/rules/` with `paths:` and a row in *Read before
  you touch*. `tools/check-docs.sh` holds all of it, including a 32 KiB ceiling
  on this file — the size at which Codex stops reading it.
