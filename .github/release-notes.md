*For Daniel — the best son anyone could imagine.*

An open-world sandbox about being rude to strangers. Everything is made of
rubber, the only verb is provocation, and a mood spreads from neighbour to
neighbour.

| Platform | Archive |
|---|---|
| Windows 10/11, x64 | `…-windows-x64.zip` |
| macOS 11 or later, Apple silicon | `…-macos-arm64.tar.gz` |
| Linux, x64, glibc 2.35 or later | `…-linux-x64.tar.gz` |

Unpack it anywhere and start `mood_swings` in the folder it unpacked to. Every
archive carries the whole recorded sound bank and **Landshüpf**, the real
street network and buildings of Landshut baked from OpenStreetMap — map data
© OpenStreetMap contributors, [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/);
`CREDITS.md` in the archive has the rest. The optional scanned textures are
fetched in place by `tools/fetch-materials.sh` (`.bat` on Windows).

None of the binaries is signed. On Windows, SmartScreen asks first; on macOS,
lift the quarantine flag from the unpacked folder once with
`xattr -dr com.apple.quarantine <folder>`. On Linux the game wants a Vulkan
driver and the ALSA and libudev runtime libraries, which a desktop install
already has. `SHA256SUMS` lists every archive's digest.
