---
paths:
  - "src/world/{atlas,streetside,trading,streetname,roadsign,buildings,signage}.rs"
  - "tools/{bake-city.py,fetch-city.sh,fetch-overture.py}"
  - "assets/cities/**"
---

# A town read off a map

<!-- Loaded by Claude Code when a file under `paths:` is read; listed in
AGENTS.md's *Read before you touch* for every other agent. Moved out of
the always-loaded instructions because it only matters here. -->

## A postcard can also be a real town

`CityStyle` is a tuning of the generator — heights, a ceiling over them
(`height_cap`), plot width, gables, palette, whether the walls are rendered
(`rendered`) or faced with a scanned grain, how many spires. Anything a style
decides belongs on `CityStyle` rather than as a `match` at the use site, and
`heights` in particular exists because the generator and the atlas both draw
building heights and had each been applying `height_scale` on their own.

A style may also name a baked OSM extract (`atlas()`), and Landshüpf does: the
road graph is the real Landshut, the blocks are empty, and `streetside::lots`
marches plots along every street instead. A plot there is placed square to *its*
street and therefore at some arbitrary angle to every other one, which is why
`streetside::Oblong` tests candidates against the road corridors with a
separating-axis test rather than a box overlap. Street names ride beside the
layout in `atlas::Signposts` — a name is a fact about a street and an edge is
one segment of one.

The atlas also carries the town's real buildings, each polygon cut at bake time
into the rectangles that cover it (`Footprint::group` gathers the parts of one
building back into one `Block` sharing a height, a palette and a kind — an L is
two parts, a courtyard block four, and `streetside::lots` files every part), the
roof shape where the map recorded one (`Building::roof`), and the shape of the
ground (`atlas::Relief`, two grids of metres above the valley floor off the
Copernicus elevation model). The bake is `tools/bake-city.py`; it reads Overture
Maps for the polygons and the DEM tile for the relief because Overpass cannot
supply either, and a bake of its own output is its own output — `--from-ron`
regenerates the buildings and the relief from a committed file, so do not add a
step that measures something the previous bake already moved (the band tidy in
`tidy_bands` is the model: it cuts a looped band to its spine and re-attaches
the side streets, and a second pass finds no loop and nothing loose). The tests
on the committed file fail when the file is there and does not load; they used
to return early, which is how a bake that wrote `group: 0` where the runtime
wanted `Some(0)` passed every test while the game quietly fell back to the
generator.

An OSM way can be a *loop* — the Altstadt is one closed way round the market —
and after `merge_parallel` and the recentring both of its lanes lie on one
centreline, so the runtime would draw every edge twice and lay a pavement
across the carriageway at each turn-round. Look at the band's polyline before
trusting a ribbon artefact to the renderer: `runs_of` in the bake is the test.

## One mapped building is several boxes

The bake cuts a real building's polygon into up to six rectangles sharing a
`group` — an L is two, a courtyard block four — and `atlas::footprints` emits
every one of them as a `Building`, because every one of them is a wall that has
to be drawn. Only one of them is *the* building, and `Building::annex` says
which: the first part the bake emits is the largest, and everything a building
has exactly one of hangs off that. The sign over the door, the door, the room
behind it, the painted civic ground storey, the advert on the blind flank, and
the whole structure of a kind that owns one (`BuildingKind::owns_its_structure`
— church, gate, tower, stadium, garage). Before that existed the Stadtresidenz
wore six identical RATHAUS plaques and the one hotel in the Altstadt advertised
itself under four names, one per wing.

Two rules ride with it. A part thinner than `buildings::SLIVER` is a jog in a
wall rather than a wing — 490 of Landshut's 4672 parts have a side under three
metres — so it keeps its wall and takes a flat deck instead of growing its own
gable out of the side of the roof next to it. And `streetside::lots` drops parts
one at a time, so whatever survives, the largest survivor is promoted back out
of `annex`: a building whose principal part stood in a road was otherwise left
as wings with no door and no name.

A named landmark wears its own name (`signage::SignKit::landmark`), not its
kind's plaque. The generic plaques are written for an invented city — every
church in it is SANKT BOING, every civic building is RATHAUS — and Landshut's
six town-hall-shaped buildings are a palace, a ministry and three courts, none
of which is the Rathaus.

What a landmark *is* comes from `KNOWN_LANDMARKS` in `tools/bake-city.py`, and
that register is a decision rather than a measurement: it changes more often
than the map does. `--relabel` re-applies it to the committed atlas by line
surgery — filling a `kind` the source left blank, overriding a height that has
a citation, touching nothing else — so a name arriving on the list costs one
diff hunk rather than a re-download of Overpass, Overture and a DEM tile. It is
idempotent, it measures nothing (see the `tidy_bands` rule above), and a Rust
test fails if the committed file has not had it run.

`--relabel` is one of three line-surgery modes, and they share that contract:
each writes one kind of fact onto a committed atlas, fills rather than
overrules, measures nothing that is written down, and writes the same file
twice.

```sh
tools/bake-city.py --relabel assets/cities/landshut.ron
tools/bake-city.py --reflag  assets/cities/landshut.ron roads.json
tools/bake-city.py --rename  assets/cities/landshut.ron built.json
```

The other two exist because the bake has always held tags it never wrote down.
One thing the map says that the game does not repeat: what the shops are
called. `--rename` takes the OSM `name` tag so the Finanzamt and the
Stadtresidenz wear their own, and the same tag names living companies and, in
one case, a private person. `world::trading` sits between the extract and the
painted sign: the town is renamed to Landshüpf wherever a name mentions it,
and a register replaces the forty-five trading names in the committed atlas
with invented ones. Both halves of the decision are written out — the renamed
and the kept — and a test holds their union against the committed file
exactly, so a re-bake that brings a new business into town fails until
somebody has decided what the game calls it. The civic and the historic keep
their real names, which is the whole point of reading a real map.

`--reflag` takes the roads dump's `oneway` and `highway=pedestrian`: nine per
cent of Landshut's road length is one-way and 4.8 km of it is a
Fussgaengerzone, the Altstadt included, and `roadgraph::Rules` is what
`ai::traffic`, `vehicle::spawn` and `world::roadsign` read. `--rename` takes
the buildings dump's `name` and its `kind_of` tags, which Overture — where the
footprints come from — does not carry: 81 named buildings became 171, and the
Finanzamt, the Galeria and the Sparkasse wear their own names.

Both match by geometry rather than by identity, because the bake keeps no OSM
ids: a street by walking its polyline and letting the samples vote, a building
by containment with a size guard in *both* directions — a forty-square-metre
`man_made=tower` next door will otherwise hand its tags to the six-hundred-
metre hall whose middle lands beside it, and the runtime draws a thirty-seven-
metre pyramid of brick. A name or a kind is written per *building*, never per
part: one wing named and its siblings blank is two buildings that touch.
