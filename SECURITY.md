# Security

## What is actually exposed

Most of this project is a single-player game that opens no ports, parses no
remote input and runs no code it did not compile. Three parts are not that, and
they are the ones worth reporting against:

- **The dedicated multiplayer server** (`crates/multiplayer`, `Dockerfile.server`,
  port 7777/TCP). It accepts framed messages from unauthenticated clients and
  is the only component here that reads bytes from a stranger. Its framing is
  length-bounded and its player names are validated, which is a claim this
  document makes because the code intends it — not because anyone has fuzzed it.
- **The save and options files** (`saves/*.ron`). RON parsed from disk. A
  malicious file is a local-privilege problem rather than a remote one, but a
  parser that can be made to allocate without bound is still a bug.
- **`tools/fetch-materials.sh` / `.bat` and `tools/bake-city.py`**, which
  download from the network. They fetch CC0 assets and map extracts over HTTPS
  and write them into the working tree.

Nothing here handles credentials, payments or personal data. There is no
telemetry and no account system.

## Supported versions

The tip of `main`. This is a hobby project with no release branches; a fix
lands on `main` and the next tag carries it.

## Reporting

Use GitHub's **[private vulnerability reporting](https://github.com/bmachek/MoodSwings/security/advisories/new)**
rather than a public issue, for anything that would let one player affect
another player's machine or a server's host.

For everything else — a crash, a panic, a parser that falls over on a corrupt
save — a normal public issue is better: it is not a vulnerability and a public
report gets it fixed faster.

Expect a best-effort reply. One person maintains this in their own time, there
is no bounty, and there is no service-level commitment. What there is: a fix on
`main` and your name in the commit, unless you would rather not be named.

## Scope

Out of scope, because they are the design rather than a defect:

- The game trusts `assets/` and `saves/` on the local disk.
- The dedicated server has no authentication. It is a LAN and
  friends-with-an-IP arrangement, documented as such in `README.md`, and
  running one on the open internet is a decision this project does not make for
  you.
- The optional CC0 downloads are fetched over HTTPS from third-party hosts and
  are not checksummed against a manifest in this repository.
