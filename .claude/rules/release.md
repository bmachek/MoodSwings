---
paths:
  - ".github/workflows/release.yml"
  - ".github/release-notes.md"
  - "Cargo.toml"
---

# Cutting a release

<!-- Loaded by Claude Code when a file under `paths:` is read; listed in
AGENTS.md's *Read before you touch* for every other agent. Moved out of
the always-loaded instructions because it only matters here. -->

```sh
git tag v0.1.0 && git push origin v0.1.0   # the tag must match Cargo.toml's version
```

`.github/workflows/release.yml` checks the tag against `Cargo.toml`, runs the
gate, fetches the sound bank once, builds Windows x64, macOS arm64 and Linux
x64 — the last on Ubuntu 22.04, because the build runner's glibc is the oldest
one the binary will start on — and drafts a release; publishing it is the human
sign-off. An archive is the source tree's layout: the binary beside the whole
of `assets/`, which is the shaders and Landshüpf's atlas, plus the recorded
bank, with the scanned textures left to the fetch script it also carries.
Each build unpacks its own archive and runs `--audition` and
`--survey --city landshuepf` on it with the checkout's `assets/` moved aside,
because `root()` prefers the manifest directory and would otherwise hide a
file the archive forgot — the packaging step once copied the shaders by name
and would have shipped Landshüpf as the generator. `workflow_dispatch` is the
rehearsal: everything except the release.
