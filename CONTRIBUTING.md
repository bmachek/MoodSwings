# Contributing

Patches, bug reports and screenshots of the city doing something stupid are all
welcome.

By contributing you agree that your contribution is licensed under the
**GPL-3.0-or-later**, the same terms as the rest of the project. See `LICENSE`.

## Getting set up

```sh
git clone https://github.com/bmachek/MoodSwings
cd MoodSwings
tools/fetch-materials.sh    # or tools/fetch-materials.bat on Windows
cargo run
```

`tools/fetch-materials.sh` fetches two things. The scanned PBR material sets
are an **optional** upgrade — `world::texture` paints a procedural stand-in for
anything missing, and a fresh clone renders a complete city without them. The
CC0 sound bank is **required**: a clone that has not fetched it still starts,
but every missing sound plays as a short silence with a warning in the log.

## Before you open a pull request

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

CI runs all three. A change that touches rendering also needs before-and-after
screenshots — see below.

## How this codebase works

`CLAUDE.md` is the short architectural tour: the module table, the schedule,
the four traps that have bitten more than once. Read it before a first change.
The rest of this page is the etiquette.

### Comments explain *why*

Often at length, and frequently recording what was tried and rejected. Match
that. A change that reverses a decision should update the comment that records
the decision, rather than leaving a note behind that argues against the code
under it.

### Determinism is load-bearing

The whole city is regenerated from `GameConfig::world_seed` on demand — chunks
are respawned, not stored — so generation must be pure and reproducible. Every
subsystem draws from its own RNG stream (`core::rng::stream_for`) with a fixed
key. Never share a stream between subsystems and never reuse a key: with one
shared generator, adding a draw in the building generator silently reshuffles
every street downstream.

The same rule applies *within* a stream. Where a value is drawn but then
overridden — an archetype's fixed coat, say — the draw still happens. A fixed
answer is not a skipped question, and skipping it moves everything after it.

### Tests are inline, and named in sentences

`#[cfg(test)]` modules next to the code, with names like
`the_root_is_the_source_trees_assets_directory`. Pure logic — layout, preset
tables, road-graph queries, mixer curves — gets a test. Constants that only
have to agree with each other get a `const _: () = { assert!(…) };` block
instead, because there is nothing to run.

Rendering is not unit-tested. It is verified by capture.

### Verifying a rendering change

There is no way to eyeball this game from a terminal except the capture
harness, which renders to an offscreen texture, holds a few warmup frames,
writes a PNG and exits:

```sh
tools/shoot.sh --only street,night --out shots/before
# ... make the change ...
tools/shoot.sh --only street,night --out shots/after
```

Shoot the same framings before and after, and pin `--hour` on anything being
compared — the clock and the weather run together, so an unpinned shot drifts
its own sky between runs. The full flag table is in `README.md`.

The capture harness is also the integration test for the schedule: no unit test
builds the whole app, so a Bevy query conflict — two queries in one system both
touching a component when either is mutable — panics on first run and nowhere
earlier. If your change adds a system, take a screenshot with it.

### Verifying an audio change

Same problem, same answer:

```sh
cargo run -- --audition shots/audio
```

writes every sound in `audio::bank::REGISTER` out as a WAV and exits without
starting Bevy at all. What it writes is the *processed* buffer — the bank holds
every recording to its rules mechanically at load — so it is what the game
actually plays.

### Player-facing text is German

The menu and the HUD speak German. Everything a developer reads — the dev
panel, log lines, identifiers, comments — is English.

### Feel constants live in `GameConfig`

So the dev panel can tune them at runtime. A number that decides how something
feels does not belong as a literal at the use site.

## Adding assets

No third-party art ships in this repository, and that is a rule rather than an
accident. Every texture is painted per-pixel at startup; every sound is fetched
by the script.

- **A new material set**: add its name to `world::material::set` *and* to both
  fetch scripts. It must be CC0.
- **A new sound**: add an entry to `audio::bank::REGISTER` (name, peak, shape)
  *and* a download to both fetch scripts. A test fails if either script lacks
  one. It must be CC0, and it must be a recording — the synthesiser was retired
  on purpose.

`tools/fetch-materials.sh` and `tools/fetch-materials.bat` are twins and **must
be kept in sync**, in the same commit. The lists are the contract; only the
shell plumbing may differ.

Anything new also goes in `CREDITS.md`. CC0 asks for nothing, and we name
people anyway.

## When a change has to break a tie

This was an open-world crime sandbox and it is now a comedy one. There are no
weapons, no police, no health and no fail state. The verb is provocation and
the readout is a mood.

Break the tie towards the joke.
