---
name: shot
description: Verify a rendering change by capture — the same framings before and after
argument-hint: "[framings, e.g. street,night] [--town landshut]"
context: fork
agent: render-shooter
---

Verify a rendering change by capture. Framings, `street,night` if none are
named: $ARGUMENTS

Follow the protocol, and report having followed it:

1. **Before.** If the change is not in the tree yet, `tools/shoot.sh --only
   <framings> --out shots/before`. If it is — the usual case — shoot the base
   from a worktree rather than stashing somebody's uncommitted work:

   ```sh
   repo=$(git rev-parse --show-toplevel)
   wt=$(tools/base-worktree.sh HEAD)      # or the branch's base
   (cd "$wt" && CARGO_TARGET_DIR="$repo/target" tools/shoot.sh --only <framings> --out "$repo/shots/before")
   ```
2. **After.** `tools/shoot.sh --only <framings> --out shots/after` in the tree.
3. Look at every PNG in both directories and describe what moved in the image.
4. `tools/base-worktree.sh --remove`.

`--hour` is already pinned in every framing; keep it that way. `shots/*/` is
gitignored — never overwrite the tracked `shots/*.png`, which are the README
gallery. If `tools/dev-setup.sh --report` says `gpu: none`, capture cannot run:
take the numeric route (`cargo run -- --survey`) and say so rather than guessing
at pixels.
