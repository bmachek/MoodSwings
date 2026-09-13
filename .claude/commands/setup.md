---
description: Make this checkout able to build and verify, and say what it still cannot do
argument-hint: "[--sounds] [--gpu] [--report]"
allowed-tools: Bash(tools/dev-setup.sh:*), Bash(tools/check-docs.sh:*), Read
---

Run `tools/dev-setup.sh $ARGUMENTS` and relay what it says.

It checks the four things that stop a clone dead without failing loudly: a rustc
older than `rust-version` in Cargo.toml, the ALSA and libudev headers Bevy links
on Linux, the *required* CC0 sound bank (`--sounds` fetches it), and whether
there is any Vulkan adapter at all (`--gpu` installs the software one). It fixes
what it can fix and prints the commands for what it cannot.

Afterwards, state plainly which instruments this session has: `cargo test`,
`--survey` and `--audition` need no window and always work; `--screenshot`,
`--patrol`, `--film` and `--fps-log` need the GPU line.
