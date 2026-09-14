<!--
CONTRIBUTING.md carries the etiquette and CLAUDE.md the architecture; neither
is repeated here. This is only the list of things that have been forgotten
before.

Delete any section that does not apply. An empty checkbox with a sentence
saying why is a better answer than a ticked one that is not true.
-->

## What changed, and why

<!-- The why, at the length it deserves. If this reverses a decision that a
comment in the code records, say which comment — and update it. -->

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --all-targets -- -D warnings`
- [ ] `cargo test --locked`

<!-- Anything visible needs before-and-after captures of the same framings with
`--hour` pinned, because the clock and the weather run together:

    tools/shoot.sh --only street,night --out shots/before
    tools/shoot.sh --only street,night --out shots/after

Anything that can only go wrong while running needs a patrol — more than one,
because a single run proves nothing:

    cargo run --release -- --patrol 120

A frame-time claim needs the interleaved A/B with `--frames 200`, both binaries
built and measured in the same minute. Anything else is measuring the fan. -->

- [ ] Rendering changed → before/after shots attached, `--hour` pinned
- [ ] Something spawned per chunk, or AI, or physics → `--patrol` run, clean
- [ ] A frame-time claim is made → interleaved A/B, `--frames 200`, same minute
- [ ] Audio changed → `--audition` listened to
- [ ] Layout changed → `--survey` scorecard read

## The things that are rules

- [ ] **Determinism**: no stream shared between subsystems, no RNG key reused,
      and a draw that is later overridden still happens
- [ ] **`ChunkOf`**: anything spawned by streaming carries it
- [ ] **`world::layer`**: nothing flat laid at the same height as anything else
      flat — a new surface is a new entry in the table, not a number beside the
      spawner
- [ ] **Terrain**: anything placed away from a street asks `Terrain::height`
      rather than assuming zero
- [ ] **Presets**: no renderer feature switched on directly; it resolves
      through `QualityPreset` → `GraphicsSettings` → `downgrade`
- [ ] **Feel constants** live in `core::config::GameConfig`, not as literals
- [ ] Player-facing text is German; everything a developer reads is English
- [ ] Comments explain *why*, and a reversed decision updates the comment that
      records it

## Twins and contracts

- [ ] `CLAUDE.md` and `AGENTS.md` still agree (`tools/check-docs.sh`)
- [ ] A new sound is in `audio::bank::REGISTER`, **both** fetch scripts and
      `CREDITS.md`
- [ ] A new material set is in `world::material::set` and **both** fetch scripts
- [ ] A save-format change bumps `SAVE_VERSION`; a settings change keeps
      `#[serde(default)]` so an older `options.ron` still loads

## What is still unverified

<!-- Say it. An unrun check is not a passed one, and "no GPU here" is a
complete and acceptable answer. -->
