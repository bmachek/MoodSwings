#!/usr/bin/env bash
# The base of a before/after comparison, checked out *beside* the working tree
# rather than in it.
#
#   tools/base-worktree.sh <ref>     # prints the path of a worktree at <ref>
#   tools/base-worktree.sh --remove  # takes it away again
#
# Every before/after here — a pair of captures, a pair of frame times — needs
# the old commit built and run in the same minute as the new one. That used to
# be `git stash -u`, `git checkout <base>`, build, measure, `git checkout -`,
# `git stash pop`, in the tree being compared: somebody's uncommitted work sat
# in a stash for the length of a measurement, an interrupted run left it there,
# and an agent doing it unattended was one conflicted pop away from mangling a
# change that was not its own. A worktree never touches the tree.
#
# Build in it against the main checkout's target directory. The dependencies,
# which are most of a Bevy build, are then compiled once and shared, and only
# the game crate is built again for each side — the same three minutes the
# stash dance cost:
#
#   repo=$(git rev-parse --show-toplevel)
#   wt=$(tools/base-worktree.sh main)
#   (cd "$wt" && CARGO_TARGET_DIR="$repo/target" cargo build --release)
#   cp "$repo/target/release/mood_swings" "$repo/target/mood_swings-base"
#   cargo build --release                       # the change, back in the tree
#
#   (cd "$wt" && CARGO_TARGET_DIR="$repo/target" tools/shoot.sh --only street --out "$repo/shots/before")
#
# A binary built there finds its assets in the worktree (`core::assets::root()`
# prefers the manifest directory), so the base is shot with the base's shaders.
# What git does not carry — the scanned materials and the recorded bank — is
# linked in from the main checkout, or the base would render without the
# scanned sets and every frame time would favour it.
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
wt="$repo/target/worktrees/base"

# `cargo clean` takes target/ and the worktree with it; git still has it on its
# list until told otherwise.
git -C "$repo" worktree prune

if [ "${1:-}" = --remove ]; then
    if [ -d "$wt" ]; then
        git -C "$repo" worktree remove --force "$wt"
    fi
    exit 0
fi

ref="${1:?usage: tools/base-worktree.sh <ref> | --remove}"
commit="$(git -C "$repo" rev-parse --verify --quiet "$ref^{commit}")" || {
    echo "base-worktree: $ref is not a commit" >&2
    exit 2
}

if [ -d "$wt/.git" ] || [ -f "$wt/.git" ]; then
    git -C "$wt" checkout --quiet --detach --force "$commit"
    git -C "$wt" clean --quiet -fd
else
    mkdir -p "$(dirname "$wt")"
    git -C "$repo" worktree add --quiet --detach --force "$wt" "$commit" >&2
fi

for dir in assets/materials assets/sounds; do
    if [ -d "$repo/$dir" ] && [ ! -e "$wt/$dir" ]; then
        ln -s "$repo/$dir" "$wt/$dir"
    fi
done

echo "$wt"
