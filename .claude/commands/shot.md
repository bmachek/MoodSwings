---
description: Verify a rendering change by capture — the same framings before and after
argument-hint: "[framings, e.g. street,night] [--town landshut]"
---

Hand this to the `render-shooter` subagent: $ARGUMENTS

Framings default to `street,night` if none are named. The protocol it must
follow, and report having followed:

1. `tools/shoot.sh --only <framings> --out shots/before` **before** the change is
   applied — if the change is already in the tree, stash it (`git stash -u`),
   shoot, and restore it.
2. The change.
3. `tools/shoot.sh --only <framings> --out shots/after`.
4. Look at every PNG in both directories and describe what moved in the image.

`--hour` is already pinned in every framing; keep it that way. `shots/*/` is
gitignored — never overwrite the tracked `shots/*.png`, which are the README
gallery. If `tools/dev-setup.sh --report` says `gpu: none`, capture cannot run:
take the numeric route (`/survey`) and say so rather than guessing at pixels.
