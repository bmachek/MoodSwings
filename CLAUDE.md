# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
cargo run                      # debug; ~2-10s incremental after the first build
cargo run --release            # smoother frame rate, slower to compile
cargo run --features dev       # bevy dynamic_linking — fastest iteration
cargo test                     # ~324 unit tests, all inline #[cfg(test)] modules
cargo test citygen             # one module's tests (filter by name substring)
cargo clippy --all-targets -- -D warnings
cargo fmt
tools/fetch-materials.sh       # CC0 assets: PBR sets (optional) and the recorded sound bank (REQUIRED — see below)
tools/fetch-materials.bat      # the same for Windows — KEEP THE TWO IN SYNC (see below)
cargo run -- --audition shots/audio   # write the whole sound bank out as WAVs
```

Optional cargo features: `raytracing` (pulls in `bevy_solari`), `dlss` (needs an
NVIDIA GPU and the vendor SDK). Neither is on by default.

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

`--lineup` stands one of every archetype in a row and points the camera down
it — the cast's `--showroom`. The rare archetypes are the ones whose costume
goes wrong and the ones a street framing cannot be relied on to contain, so
"shoot a street and hope" is not a check on them.

`tools/shoot.sh` exists so a rendering change is judged against the last render
rather than against a memory of it: shoot the same framings before and after.
Pin `--hour` on any shot being compared — the clock and the weather run together,
so an unpinned shot drifts its own sky between runs. `--city` builds a given
`CityStyle` instead of the persisted one, which is the only way to shoot a
postcard the player has not selected. `--fps-log` reports median, p95 and worst
frame time. Full flag table is in README.md.

The seed a capture builds is the *persisted* one from the player's options file,
not the code default — so a position probed in a citygen unit test is a position
in a different city. Probe with a temporary `info!` in the spawn path instead.

Capture mode is not just a camera: `core::capture::is_capture_mode()` gates the
dev panel off (`ui`) and mutes audio, and several systems check it. Anything that
would spoil an unattended shot should check it too.

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

## Architecture

Bevy 0.19 app; `main.rs` installs `DefaultPlugins` then one plugin per top-level
module. Physics is Avian 3D, input is leafwing-input-manager, the dev panel is
bevy_egui, saves are RON.

| Module | What lives there |
|---|---|
| `core` | States, schedule sets, `GameConfig` tunables, persisted settings/keybindings (`core::settings`), deterministic RNG, asset-root resolution, the screenshot harness |
| `world` | City generator (incl. building kinds & vacant-lot zoning), road graph, chunk streaming, day/night, weather, facades/LOD shells, window interiors, walk-in ground floors (`interior`), drivable parking decks (`garage`), painted signs, ad posters & civic frontages (`signage`), park monuments (`statues`), churches & cathedrals (`church`), the stadium and its Welle (`stadium`), the canal and its bridges (`river`), lot furnishing (`lots`), what a building puts out on its pavement — pipes, boards, window boxes, bikes, terraces (`frontage`), rubbish that scatters when you walk through it (`litter`), streetworks (`worksite`), road wear, vegetation, props, world damage (`mayhem`), procedural + scanned textures |
| `bounce` | The elastic simulation: bounce controller, impact response, launch, squash |
| `player` | Input mapping, on-foot movement, camera rig, enter/exit |
| `vehicle` | Arcade vehicle physics, specs, bodywork, comedy crash response (`impact`), lights, parked-car spawning |
| `mood` | How a flummi feels (`feeling`), the painted face it wears (`face`), what it says (`voice`), taunting and cheering (`provoke`), and retaliation (`grudge`) |
| `ai` | Traffic, pedestrians, archetypes (the cast), shared steering, walk cycles, the figure itself |
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
seed: a postcard, not a map. It is a handful of dials — height scale, palette
override, how many churches and whether one of them is a cathedral, how wide
the market band runs, how much the walls advertise — so a style is a *tuning*
of the same generator, and a new one costs a match arm rather than a data file.
Anything a style decides belongs on `CityStyle`, not as a `match` at the use
site; the layout must stay a pure function of `(seed, style)` or the chunks
stop respawning the same city.

### The traps

Four things here have bitten more than once and none of them fail loudly:

- **Restitution is a property of a contact.** A body held off the ground by a
  spring — a floating character controller, a car on raycast suspension — never
  forms one, so declaring it elastic does nothing. `bounce::controller` applies
  the hop by hand for that reason.
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
