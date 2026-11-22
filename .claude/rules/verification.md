---
paths:
  - "src/core/{capture,patrol,survey}.rs"
  - "tools/shoot.sh"
  - "src/render/**"
---

# The instruments in full

<!-- Loaded by Claude Code when a file under `paths:` is read; listed in
AGENTS.md's *Read before you touch* for every other agent. Moved out of
the always-loaded instructions because it only matters here. -->

## Verifying rendering changes

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

## Letting the game find its own bugs

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
and shoot them in the same minute — `tools/base-worktree.sh <base>` checks
the base out beside the tree, `cargo build --release` there against the shared
`target/`, measure, come back. (This used to be `git stash -u` and
`git checkout <base>` in the tree itself, which put somebody's uncommitted work
in a stash for the length of a measurement, and an interrupted run left it
there.) It costs three minutes of compiling and it is the difference between
"eleven percent slower" and "twelve percent faster", both of which this
repository has now reported about the same change.

The seed a capture builds is the *persisted* one from the player's options file,
not the code default — so a position probed in a citygen unit test is a position
in a different city. Probe with a temporary `info!` in the spawn path instead.

Capture mode is not just a camera: `core::capture::is_capture_mode()` gates the
dev panel off (`ui`) and mutes audio, and several systems check it. Anything that
would spoil an unattended shot should check it too.
