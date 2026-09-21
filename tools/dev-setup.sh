#!/usr/bin/env bash
# Makes a fresh checkout — a cloud container in particular — able to build and
# verify this game, and says plainly what it still cannot do.
#
# Four things stop a clone dead and not one of them fails loudly:
#
#   * the workspace needs a rustc at least as new as `rust-version` in
#     Cargo.toml, and a container image pinned a few months ago carries an
#     older stable. Cargo refuses the whole workspace with one error naming
#     eleven packages, which reads like a dependency problem and is not;
#   * Bevy links ALSA and libudev on Linux and neither dev package is on a
#     slim image — the same reason .github/workflows/ci.yml installs them;
#   * the recorded sound bank is a *required* fetch. A clone without it starts,
#     and every sound in it plays as a short silence with a warning;
#   * the capture harness needs a Vulkan device. A cloud container has no GPU,
#     so without a software rasteriser every `--screenshot`, `--patrol` and
#     `--film` dies at adapter selection — while `--survey`, `--audition` and
#     `cargo test` never open a window and work anywhere.
#
# It only ever installs what is missing, so it is safe to run repeatedly, and
# it never touches a machine it cannot install on without saying so: on macOS
# and Windows, and as a non-root user, it reports and leaves the commands to
# you.
#
#   tools/dev-setup.sh              # report, and fix what it can fix
#   tools/dev-setup.sh --report     # report only, change nothing
#   tools/dev-setup.sh --sounds     # also run tools/fetch-materials.sh (a big download)
#   tools/dev-setup.sh --gpu        # also install the software Vulkan driver
#   tools/dev-setup.sh --hook       # one line of JSON, for the SessionStart hook
#
# `--hook` is the form .claude/settings.json runs. It fixes the toolchain and
# the build dependencies (a session that cannot compile is worth two minutes at
# startup), skips both big downloads, and hands the session one line saying
# which instruments are available in it — so an agent reaches for `--survey`
# instead of discovering the hard way that there is no GPU.
set -uo pipefail

cd "$(dirname "$0")/.."

MODE=fix
WANT_SOUNDS=0
WANT_GPU=0
for arg in "$@"; do
    case "$arg" in
        --report) MODE=report ;;
        --hook) MODE=hook ;;
        --sounds) WANT_SOUNDS=1 ;;
        --gpu) WANT_GPU=1 ;;
        -h | --help)
            sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "dev-setup: unknown argument: $arg" >&2
            exit 2
            ;;
    esac
done

# NOTES builds the one-line summary, PROBLEMS what is still wrong. Both are
# strings rather than arrays because `${#arr[@]}` under `set -u` is an error on
# the bash 3.2 macOS still ships, and this is the first script a contributor
# runs — it cannot be the one that needs a newer shell.
NOTES=""
PROBLEMS=""
note() { NOTES="${NOTES:+$NOTES; }$1"; }
problem() { PROBLEMS="${PROBLEMS}  - $1
"; }

# `sort -V` is GNU. awk is everywhere.
ver_ge() {
    awk -v a="$1" -v b="$2" 'BEGIN {
        na = split(a, x, "."); nb = split(b, y, ".")
        n = na > nb ? na : nb
        for (i = 1; i <= n; i++) {
            ia = i <= na ? x[i] + 0 : 0
            ib = i <= nb ? y[i] + 0 : 0
            if (ia > ib) { exit 0 }
            if (ia < ib) { exit 1 }
        }
        exit 0
    }'
}

say() { [ "$MODE" = hook ] || printf '%s\n' "$*"; }
step() { [ "$MODE" = hook ] || printf '  %s\n' "$*"; }

# Anything that installs runs only when this says so: not in --report, and only
# where apt and root are both present. A dev box is not ours to reshape.
can_install() {
    [ "$MODE" != report ] && [ "$(id -u)" = 0 ] && command -v apt-get >/dev/null 2>&1
}

apt_install() {
    apt-get update -qq >/dev/null 2>&1
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$@" >/dev/null 2>&1
}

say "== rust toolchain =="
# `rust-version` is the contract; read it rather than hard-coding a number here
# that would go stale the next time Bevy raises its floor.
WANT_RUSTC=$(sed -n 's/^rust-version *= *"\([0-9.]*\)".*/\1/p' Cargo.toml | head -1)
HAVE_RUSTC=$(rustc --version 2>/dev/null | awk '{print $2}')
if [ -z "${HAVE_RUSTC:-}" ]; then
    problem "no rustc on PATH — install from https://rustup.rs"
    note "rustc: MISSING"
    step "no rustc on PATH"
elif [ -n "$WANT_RUSTC" ] && ! ver_ge "$HAVE_RUSTC" "$WANT_RUSTC"; then
    step "rustc $HAVE_RUSTC is older than the required $WANT_RUSTC"
    if [ "$MODE" != report ] && command -v rustup >/dev/null 2>&1; then
        step "updating the stable toolchain (a few minutes on a cold container)"
        rustup update stable >/dev/null 2>&1
        HAVE_RUSTC=$(rustc --version 2>/dev/null | awk '{print $2}')
    fi
    if ! ver_ge "$HAVE_RUSTC" "$WANT_RUSTC"; then
        problem "rustc $HAVE_RUSTC < $WANT_RUSTC required — run: rustup update stable"
        note "rustc: $HAVE_RUSTC, TOO OLD for $WANT_RUSTC (nothing compiles)"
    else
        step "rustc $HAVE_RUSTC"
        note "rustc: $HAVE_RUSTC"
    fi
else
    step "rustc $HAVE_RUSTC (needs $WANT_RUSTC)"
    note "rustc: $HAVE_RUSTC"
fi

say "== linux build dependencies =="
if [ "$(uname -s)" = Linux ]; then
    MISSING=""
    command -v pkg-config >/dev/null 2>&1 || MISSING="$MISSING pkg-config"
    pkg-config --exists alsa 2>/dev/null || MISSING="$MISSING libasound2-dev"
    pkg-config --exists libudev 2>/dev/null || MISSING="$MISSING libudev-dev"
    MISSING="${MISSING# }"
    if [ -z "$MISSING" ]; then
        step "alsa and libudev headers present"
    elif can_install; then
        step "installing: $MISSING"
        # Deliberately unquoted: this is a list of package names, no spaces in
        # any of them.
        # shellcheck disable=SC2086
        apt_install $MISSING
        if pkg-config --exists alsa 2>/dev/null && pkg-config --exists libudev 2>/dev/null; then
            step "installed"
        else
            problem "ALSA/libudev headers still missing — Bevy will not link"
            note "build deps: MISSING"
        fi
    else
        problem "missing $MISSING — run: sudo apt-get install -y --no-install-recommends $MISSING"
        note "build deps: MISSING ($MISSING)"
    fi
else
    step "$(uname -s): nothing to install"
fi

say "== sound bank =="
# REGISTER is the one list of what the bank contains; count it rather than
# keeping a second number here that could disagree with it.
WANT_SOUND_COUNT=$(awk '/pub const REGISTER/,/^\];/' src/audio/bank.rs | grep -cE '^    \("')
HAVE_SOUND_COUNT=$(find assets/sounds -type f \( -name '*.wav' -o -name '*.flac' -o -name '*.ogg' -o -name '*.mp3' \) 2>/dev/null | wc -l | tr -d ' ')
if [ "$HAVE_SOUND_COUNT" -eq 0 ]; then
    if [ "$WANT_SOUNDS" = 1 ] && [ "$MODE" != report ]; then
        # There is no half of the fetch script: it takes no arguments and
        # pulls the required sounds and the optional material sets together.
        step "running tools/fetch-materials.sh (sounds, and the optional PBR sets with them)"
        tools/fetch-materials.sh >/dev/null 2>&1
        HAVE_SOUND_COUNT=$(find assets/sounds -type f 2>/dev/null | wc -l | tr -d ' ')
        step "assets/sounds: $HAVE_SOUND_COUNT files"
        note "sounds: $HAVE_SOUND_COUNT/$WANT_SOUND_COUNT"
    else
        step "assets/sounds is empty — all $WANT_SOUND_COUNT sounds will play as silence"
        step "run: tools/fetch-materials.sh   (or tools/dev-setup.sh --sounds)"
        note "sounds: NONE of $WANT_SOUND_COUNT fetched (every sound is a warning + silence)"
    fi
else
    step "assets/sounds: $HAVE_SOUND_COUNT files for $WANT_SOUND_COUNT register entries"
    note "sounds: $HAVE_SOUND_COUNT/$WANT_SOUND_COUNT"
fi

say "== scanned materials (optional) =="
MAT_COUNT=$(find assets/materials -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')
step "assets/materials: $MAT_COUNT sets — a clone renders a complete city without them"

say "== gpu =="
# What decides whether the visual harness runs at all. On Linux wgpu wants a
# Vulkan adapter; llvmpipe is one, in software, and slow enough that it is a
# different instrument rather than the same one without a graphics card.
#
# macOS and Windows are not that. wgpu takes Metal on one and DX12 on the
# other, every Mac has a GPU, and neither has `/dev/dri` — so the Linux test
# below called every Mac "none" and every session on one opened by being told
# the camera, the patrol and the film were unavailable. They were not: this was
# written on an M4 Pro and `--screenshot` takes about nine seconds. Check the
# platform before checking the Linux-only evidence.
GPU=none
case "$(uname -s)" in
    Darwin) GPU=hardware ;;
    MINGW* | MSYS* | CYGWIN* | Windows_NT) GPU=hardware ;;
esac
if [ "$GPU" = hardware ]; then
    :
elif [ -d /dev/dri ]; then
    GPU=hardware
elif ls /usr/share/vulkan/icd.d/lvp_icd*.json >/dev/null 2>&1; then
    GPU=llvmpipe
elif [ "$WANT_GPU" = 1 ] && can_install; then
    step "installing the software Vulkan driver (llvmpipe)"
    apt_install mesa-vulkan-drivers vulkan-tools
    ls /usr/share/vulkan/icd.d/lvp_icd*.json >/dev/null 2>&1 && GPU=llvmpipe
fi
case "$GPU" in
    hardware) step "a real GPU: the whole capture and patrol battery is available" ;;
    llvmpipe)
        step "no GPU; llvmpipe (software Vulkan) is installed"
        step "capture works but costs minutes a frame: --quality low, one framing at a time"
        ;;
    none)
        step "no GPU and no software driver: --screenshot, --patrol and --film cannot run"
        step "run: tools/dev-setup.sh --gpu   (installs llvmpipe, slow but real)"
        ;;
esac
note "gpu: $GPU"

if [ "$MODE" = hook ]; then
    # One line of context for the session, with nothing in it that needs
    # escaping. What an agent needs to know before it picks an instrument.
    SUMMARY=$(printf '%s' "$NOTES" | tr -d '"\\' | tr -d '\n')
    printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"Project setup (tools/dev-setup.sh): %s. Window-free instruments (cargo test, --survey, --audition) work regardless; --screenshot/--patrol/--film need the gpu line above."}}\n' "$SUMMARY"
    exit 0
fi

echo
if [ -z "$PROBLEMS" ]; then
    echo "Ready: cargo test, cargo clippy, cargo run -- --survey, cargo run -- --audition."
    [ "$GPU" = none ] && echo "Not ready: anything that renders — see the gpu section above."
    [ "$GPU" = hardware ] && echo "Also ready: --screenshot, --patrol, --film, --fps-log."
    exit 0
fi
echo "Still to do:"
printf '%s' "$PROBLEMS"
exit 1
