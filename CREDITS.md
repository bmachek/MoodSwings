# Credits

Mood Swings is an original work. The city, the vehicles, the crowd, every
texture and every face are generated at runtime from a seed — there is no
third-party art in this repository and no trademark is used.

There are now two exceptions, and the second one arrived with Landshut — see
*Map data* below. Everything else still holds.

The one thing the game does not make for itself is sound. Every sound in the
bank is a **CC0 1.0** recording made by somebody else, fetched by
`tools/fetch-materials.sh` (or `tools/fetch-materials.bat`) into
`assets/sounds/`, which is gitignored. The optional scanned PBR material sets
are CC0 too.

CC0 asks for nothing — no attribution, no notice, no conditions. Everybody
below is named anyway, because the whole cast of this city speaks with
borrowed voices and it would be a poor joke to pretend otherwise.

`tools/fetch-materials.sh` is the authoritative list: it carries the exact URL
of every file, and the comments there say what each one is doing in the game.
This page is the human-readable version of it.

## Materials — [ambientCG](https://ambientcg.com), CC0 1.0

Ten scanned PBR sets, at 2K JPEG: `Asphalt031`, `PavingStones138`,
`Concrete034`, `Concrete046`, `Bricks097`, `Bricks104`, `Bricks075A`,
`PaintedPlaster006`, `Gravel023`, `Grass005`.

These are an *optional* upgrade. `world::texture` paints a procedural stand-in
for anything missing, and a fresh clone that never runs the fetch script
renders a complete city.

## Sound — [OpenGameArt](https://opengameart.org), CC0 1.0

- **qubodup** — the crash, and the car pack the engine loop and door slam come
  out of.
- **rubberduck** — two CC0 SFX hundreds: the footstep, the highway loop that
  *is* the city's traffic bed, the forecourt machine hum, and the cough, spit
  and burp in the taunt rotation.
- The boing, the bicycle horn that every car in the city is fitted with, the
  chunky explosion, the park birdsong and river, and the shouting crowd behind
  the barricades.

## Sound — [Freesound](https://freesound.org), CC0 1.0

Fetched from the CDN preview (128 kbps MP3) rather than the original, because
original downloads sit behind a login and the previews do not. The licence is
the same either way, and the bank remixes everything to mono at its own rate
on load.

- **Reitanna** — the giggles, the grumbles, one of the whistles, and the
  tongue raspberry that is half the game's verb. More of this city's voice
  than anybody else's.
- **elle-trudgett**, **elijahgoodson**, **Willygoat**, **OwlStorm** — the rest
  of the whistles: the innocent one, the tune, the calm one, the wolf whistle.
- **Rocotilos**, **lipalearning**, **ssierra1202**, **WakuWakuWakuWaku**,
  **punisherman** — the curses, which are angry grunts.
- **kanyonwyvern** — the gasp.
- **dodrio** — the murmur series: wordless small talk for the indifferent
  middle of the mood scale.
- **theuncertainman** — the apology, the one recording in the bank with an
  actual word in it.
- **Blubberfreak** — the fart.
- **mefrancis13**, **bbrocer** — the retch and the gag, which belong to one
  particular gentleman and are kept out of the general rotation.
- **v0idation** — the swanee whistle a launched flummi rides through the air.
- **magnuswaker** — the jaw-harp twang of a street prop leaving its bolts.
- **audible-edge** — real tyres squealing round a real corner.
- **mycompasstv** — restaurant chatter, hung on the frontages.
- **kyles** — a basketball dribbled on real tarmac, for the courts.
- **fimrod** — the warehouse drone over the industrial blocks.
- **kwahmah_02**, **skymary** — the dog's bark and the cat's meow, one take
  each, pitched per animal the same way the crowd's voices are.
- **deadrobotmusic** — the strummed acoustic loop the busker plays whenever
  the street grants him an audience.
- **Kodack** — the camera shutter click, one decisive moment at a time.
- **Jackie-makes-noiz** — the porch mandolin that hangs over Klein-Neapel.
- **f-r-a-g-i-l-e** — the lotus guzheng that hangs over the Fernost-Viertel.

## Map data — [OpenStreetMap](https://www.openstreetmap.org), ODbL 1.0

`assets/cities/landshut.ron` is Landshut's street network: five hundred and
fifty-four streets with their real names, widths and shapes, in a square about
two kilometres across centred on the Altstadt. It is baked from an Overpass
extract by `tools/fetch-city.sh`, which downloads it, and `tools/bake-city.py`,
which projects it into metres.

**Map data © OpenStreetMap contributors**, licensed under the
[Open Database Licence 1.0](https://opendatacommons.org/licenses/odbl/1-0/).

This is the one asset in the repository that is not CC0, and it is the one
asset that could not be: nobody can generate a real town from a seed. Three
things follow from the licence and all three are done rather than promised.

The attribution is *carried*: it is in the header of the baked file, in the log
line `world::atlas` prints every time the city loads, and here.

The baked file is a **derived database** and stays under ODbL. Anyone who takes
it — or a further derivative of it — has the same obligations: attribute
OpenStreetMap, and keep any adapted database under ODbL.

The *game* is not a derived database. Screenshots, recordings and the rendered
world are Produced Works, which ODbL allows under any terms; the game's own
code stays GPL-3.0-or-later as it always was.

Only the roads were taken. No building footprints, no addresses, no points of
interest — every building standing on those streets is generated from the seed
by `world::streetside`, and any resemblance to the house actually standing
there is a coincidence of arithmetic.

## Software

Built on [Bevy](https://bevyengine.org), with [Avian](https://github.com/Jondolf/avian)
for physics, [leafwing-input-manager](https://github.com/Leafwing-Studios/leafwing-input-manager)
for input, [bevy_egui](https://github.com/vladbat00/bevy_egui) for the developer
panel, and [rodio](https://github.com/RustAudio/rodio) underneath the mixer.
Their own licences travel with them; `cargo tree` and each crate's repository
are the authority.
