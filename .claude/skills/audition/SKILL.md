---
name: audition
description: Write the whole sound bank out as WAVs and check the register, both fetch scripts and CREDITS agree
argument-hint: "[sound name, if only one matters]"
context: fork
agent: audio-keeper
---

Audition the bank. The sound that matters, if only one does: $ARGUMENTS

`cargo run -- --audition shots/audio` writes every entry in
`audio::bank::REGISTER` as the *processed* buffer the game actually plays, and
exits without starting Bevy at all — no GPU needed. Report MISSING entries, any
file that breaks one of the bank's rules with the number that proves it, and
whether `REGISTER`, `tools/fetch-materials.sh`, `tools/fetch-materials.bat` and
`CREDITS.md` agree. Never claim to have listened to a file.
