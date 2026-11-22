---
paths:
  - "src/audio/**"
  - "tools/fetch-materials.{sh,bat}"
  - "CREDITS.md"
---

# Verifying audio changes

<!-- Loaded by Claude Code when a file under `paths:` is read; listed in
AGENTS.md's *Read before you touch* for every other agent. Moved out of
the always-loaded instructions because it only matters here. -->

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
