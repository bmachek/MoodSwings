#!/usr/bin/env python3
"""Bake a town's map data into a city the game can build.

Usage:
    tools/fetch-city.sh landshut            # fetches everything and runs this
    tools/bake-city.py <roads.json> <out.ron> --centre LAT LON --name "Landshut" \\
        --extras <water-and-landuse.json> [--extras <buildings.json>] \\
        --buildings-parquet <overture.parquet> --dem <copernicus.tif>
    tools/bake-city.py --from-ron <old.ron> <out.ron> \\
        --buildings-parquet <overture.parquet> --dem <copernicus.tif>

What comes out is a `world::atlas::Atlas` in RON: the streets, each a
polyline in *metres* about the extract's own centre with a width and a class;
the buildings, each as one or more rectangles; the rivers; the open ground;
and the relief of the land underneath all of it.

Three decisions worth stating, because none of them is reversible once the
file is committed.

*Metres, not degrees.* The game has no projection and never wants one — its
world is a flat plane in metres and its physics, its streaming and its road
graph all assume that. Projecting here means the runtime never has to know
what a latitude is. The projection is equirectangular about the centre of the
extract, which over a two-kilometre town is accurate to a few centimetres and
is four lines of arithmetic.

*North is -Z.* The game's forward is -Z and its minimap is drawn with +Z down,
so putting north on -Z makes a screenshot of the minimap the same way up as a
paper map of the same place.

*The names come too.* They cost a few kilobytes and they are the whole
difference between a street plan and *this* street plan. So does the surface,
and so does the width the mappers actually recorded: an Altstadt paved in setts
and thirteen metres wide is not a fact the game could have guessed, and it is
the difference between a real town and a street plan of one.

## Where each part comes from

The streets, the water and the open ground come from Overpass, as they always
have. The buildings come from Overture Maps' GeoParquet by preference and from
Overpass as a fallback — the same polygon pipeline either way, so a bake from
either source writes the same file. And the relief comes from the Copernicus
GLO-30 surface model. See `tools/fetch-overture.py` for both of those and for
their licences, and `tools/fetch-city.sh` for how they are strung together.

## Buildings are rectangles, several to a building

The game draws a building as `frontage x depth` at a yaw, and that is not
going to change: every shell, gable, sign, doorway and chimney hangs off that
frame. The bake used to reduce each footprint to its smallest enclosing box
and throw away the ones a box was a lie about — the L-shapes, the courtyard
blocks, the long terraces mapped as one polygon — which was a tenth of the
town, and the tenth with the courtyards in it.

Now a footprint is *decomposed*: turned into its own principal frame (the
direction its walls mostly run, weighted by wall length, modulo a quarter
turn), snapped to a half-metre grid there, and carved greedily into the
largest axis-aligned rectangles until six of them or ninety-seven percent of
it are covered. The parts of one building share a `group`, the true polygon
`area`, a roof shape and a storey count, so the runtime can give them one
height and one palette and let them overlap each other without complaint.
Against the real polygons that is a mean IoU of 0.96 where a box managed
0.91, and the courtyards come out as courtyards. Where the box is honestly the
better fit, the box is kept.

## The relief

A surface model, not a terrain model: GLO-30 measures the top of whatever is
there, trees and roofs included. So before anything is sampled the tile is
run through a three-by-three minimum (about ninety metres, wider than any
roof or crown in the town) and a light three-by-three mean to take the stair
steps back out. What that keeps is the hill; what it loses is the odd
embankment narrower than a hundred metres, which the town has none of. The
values are metres above a `datum`, the median surface height under the
streets, so that zero is the valley floor the town is built on and the runtime
can hold the ground level exactly there without knowing what a metre above sea
level is.

The data this reads is © OpenStreetMap contributors, ODbL 1.0 (the Overture
buildings too, and Microsoft's ML footprints among them). Anything baked out
of it carries the same licence — see CREDITS.md. The relief is Copernicus
DEM GLO-30 © DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH
2014-2018, provided under COPERNICUS by the European Union and ESA; all
rights reserved — free to use with that attribution, which the file carries.
"""

import argparse
import collections
import json
import math
import os
import re
import statistics
import sys

import numpy as np
import shapely
from shapely import affinity
from shapely.geometry import LineString, Polygon, box
from shapely.ops import linemerge, unary_union

EARTH = 6_371_000.0

# Carriageway width in metres per OSM highway class.
#
# Not lane counts: this is what the game draws asphalt across, and the game's
# own generator runs 9.5m minor streets and 17m arterials. These are picked to
# land in that range so that a real street plan and a generated one furnish,
# park and drive the same way.
WIDTH = {
    "motorway": 15.0,
    "motorway_link": 8.0,
    "trunk": 14.0,
    "trunk_link": 8.0,
    "primary": 12.5,
    "primary_link": 7.5,
    "secondary": 11.0,
    "secondary_link": 7.5,
    "tertiary": 9.5,
    "tertiary_link": 7.0,
    "unclassified": 7.5,
    "residential": 7.5,
    "living_street": 6.5,
    "pedestrian": 6.0,
}

# Which classes the game treats as arterial: signals go up at their junctions,
# and its traffic prefers them.
ARTERIAL = {"motorway", "trunk", "primary", "secondary"}

# Metres of carriageway per marked lane, and the margin either side of them.
#
# The class table above is a guess about a road nobody measured; `lanes` is
# something a mapper stood in the street and counted, and it is on four hundred
# of Landshut's six hundred and seventy ways. A German urban lane is about three
# metres, and the margin is the gutter and the parking that the class widths
# already have folded into them — without it, every two-lane residential street
# comes out at six metres and the town reads as a model railway.
LANE = 3.05
MARGIN = 1.7

# What the game may believe about a width, in metres. An OSM `width` is free
# text and occasionally says "3;4" or the width of the whole square.
NARROWEST = 4.0
WIDEST = 22.0

# OSM `surface` values, collapsed to what the game can draw. Everything not
# named here is asphalt, which is what four fifths of any town is.
#
# The two that matter are the ones Landshut is actually made of: `sett` is
# Kopfsteinpflaster, the dressed granite blocks the Altstadt is laid in, and
# `paving_stones` is the sawn rectangular slabs of the Neustadt. They are
# ninety-six and forty ways of this extract respectively — a fifth of the town
# — and the bake used to throw both away.
SURFACE = {
    "sett": "Sett",
    "cobblestone": "Sett",
    "unhewn_cobblestone": "Sett",
    "paving_stones": "Slabs",
    "concrete:plates": "Slabs",
    "bricks": "Slabs",
    "paved": "Asphalt",
    "asphalt": "Asphalt",
    "concrete": "Asphalt",
    "gravel": "Gravel",
    "fine_gravel": "Gravel",
    "compacted": "Gravel",
    "unpaved": "Gravel",
    "ground": "Gravel",
    "dirt": "Gravel",
    "earth": "Gravel",
    "grass": "Gravel",
    "sand": "Gravel",
    "pebblestone": "Gravel",
}


# ------------------------------------------------------------- buildings ----
#
# Everything that stands beside a street: the buildings, the water, and the
# ground that is not built on. All three are areas rather than lines, and all
# three arrive as closed ways or as multipolygon relations — or, for the
# buildings, as WKB polygons in a GeoParquet.

# How a building's OSM tag (or Overture class, which is the OSM `building`
# value by another name) becomes one of the game's building kinds. Only the
# ones the game draws differently are worth distinguishing; everything else is
# an apartment block, which is what four fifths of a European town is.
KIND = {
    "church": "Church",
    "cathedral": "Cathedral",
    "chapel": "Church",
    "school": "School",
    "university": "School",
    "kindergarten": "School",
    "hotel": "Hotel",
    "supermarket": "Supermarket",
    "retail": "Supermarket",
    "shop": "Supermarket",
    "commercial": "Offices",
    "office": "Offices",
    "industrial": "Offices",
    "warehouse": "Offices",
    "fire_station": "FireStation",
    "civic": "TownHall",
    "public": "TownHall",
    "townhall": "TownHall",
    "government": "TownHall",
    "museum": "Museum",
    "hospital": "Offices",
    "parking": "ParkingGarage",
    "garage": "ParkingGarage",
    "garages": "ParkingGarage",
    "restaurant": "Restaurant",
    "cafe": "Restaurant",
    "bar": "Restaurant",
    "pub": "Restaurant",
    "fast_food": "Restaurant",
    "tower": "Tower",
    "water_tower": "Tower",
    "city_gate": "Gate",
}

# Overture's `subtype` is coarser than its `class` and is all that a
# `building=yes` with a `shop` or an `amenity` on it gets. Used only where the
# class said nothing.
SUBTYPE_KIND = {
    "civic": "TownHall",
    "education": "School",
    "commercial": "Offices",
    "industrial": "Offices",
    "medical": "Offices",
    "religious": "Church",
}

# The back-garden classes. A private garage is not a parking deck and a shed
# is not an office: with no name they get no kind at all and stand as the
# plain box they are. Named, the class table above still applies -- a named
# `garages` is a row of them somebody thought worth a sign.
OUTBUILDING = {"garage", "garages", "shed", "allotment_house", "outbuilding", "roof", "hut",
               "carport", "greenhouse", "cabin", "farm_auxiliary", "cowshed"}

# OSM `roof:shape` (Overture `roof_shape`, same vocabulary) to the roofs the
# game can raise. Anything else — domes, onions, gambrels — is `None`, and the
# style decides as it does for the unmapped nine tenths.
ROOF = {
    "gabled": "Gabled",
    "hipped": "Hipped",
    "half_hipped": "HalfHipped",
    "flat": "Flat",
    "pyramidal": "Pyramidal",
    "skillion": "Skillion",
}

# Metres per storey. German `building:levels` counts full storeys above ground;
# a Bavarian townhouse floor is a little over three metres and the roof adds
# most of another.
STOREY = 3.15

# What makes a building a landmark: something a person would walk across town to
# look at, and would call by name.
#
# Deliberately narrower than `heritage`, which in an Altstadt is on every second
# townhouse. Two things hang off this and both want the narrow reading. A
# landmark is exempt from the size limit below, because that exists to stop a
# shopping centre being drawn as one building and it was throwing away exactly
# the buildings the town is known for. And a landmark's height is believed as
# it stands rather than clamped into the city style's band, because an OSM
# `height` on an ordinary house is as often the ridge as the eaves and as often
# a typo as either. St. Martin really is a hundred and thirty metres; the
# listed house next to it is not.
def is_named(tags):
    if not tags.get("name"):
        return False
    if tags["name"] in KNOWN_LANDMARKS:
        return True
    if tags.get("man_made") in ("tower", "water_tower", "observatory"):
        return True
    if tags.get("historic") in ("church", "castle", "city_gate", "tower", "monastery", "chapel"):
        return True
    if tags.get("building") in ("church", "cathedral", "chapel", "castle", "temple"):
        return True
    if tags.get("tourism") in ("attraction", "museum"):
        return True
    return bool(tags.get("wikidata"))


# The Overture classes that make a named building a landmark on their own.
# Overture carries no `historic`, no `tourism` and no `wikidata`, so the rest
# of what `is_named` reads off the tags has to come from the register below.
LANDMARK_CLASSES = {"church", "cathedral", "chapel", "castle", "temple", "tower",
                    "water_tower", "city_gate", "museum"}

# The register: every building the Overpass bake found to be a landmark by its
# `historic`, `man_made`, `tourism` and `wikidata` tags, and the kind those
# tags gave it, as it stood in the committed `landshut.ron`.
#
# Kept here because Overture strips exactly those tags, and without them the
# seven wall towers and four gates that make Landshut's skyline would arrive
# as nameless houses four metres square. A name on this list is a landmark in
# either source, and the kind here stands in wherever the source's own class
# says nothing -- so the skyline survives the change of source, and a future
# Overpass bake, which reads the tags, agrees with this one.
# And what a landmark is called, where the parquet carries no tag to say so.
# Overture keeps a building's name and drops `historic`, `wikidata` and
# `tourism`, which is what the Overpass bake read; so the wings of the castle
# -- Dürnitztrakt, Fürstenbau, Damenstock, Kellereigebäude, the Torhaus --
# arrived as anonymous houses and, standing up on the Hofberg, were left to
# the hill along with everything else up there. A name with one of these in it
# is a building somebody would walk up a hill to look at. A shop's name is not
# on this list, and a shop is not a landmark: a landmark's measured height is
# believed as it stands and its box is never cut, which a shopping centre
# mapped as one polygon must not be given.
LANDMARK_WORDS = re.compile(
    r"(turm|torwart|tor$|kapelle|kirche|kloster|schlo(ss|ß)|trakt|bau$|stock$|stöckl|"
    r"residenz|rathaus|zeughaus|münz|burg\b|dom$|basilika|spital|palais|museum|theater|"
    r"bibliothek|gericht|dürnitz|torhaus|pfaffen|jägerhaus|gerichtsdiener)",
    re.IGNORECASE,
)

KNOWN_LANDMARKS = {
    "Afrakapelle": "Church",
    "Alt St. Nikola": "Church",
    "Alte Post": None,
    "Alter Bahnhof": None,
    "Altes Franziskanerkloster": None,
    "Amtsgericht Landshut": "TownHall",
    "Aussegnungshalle": None,
    "Basilika Sankt Martin": "Church",
    "Burghauser Tor": "Gate",
    "Christuskirche": "Church",
    "City Hotel Isar-Residenz": "Hotel",
    "Dominikanerkirche St. Blasius": "Church",
    "Ehem. Maschinenbaufachschule": None,
    "Falkenturm": "Tower",
    "Frauenkapelle": "Church",
    "Freundschaftstempel": None,
    "Gewerbe-Haus": None,
    "Hauptgebäude (HCG)": "School",
    "Heilig Geist": "Church",
    "Heilig-Geist-Spital Alten- und Pflegeheim": None,
    "Herzogschlößl": "TownHall",
    "Hofgärtnerhaus": None,
    "Hofstallgebäude": None,
    "Hungerturm": "Tower",
    "Jesuitenkirche Sankt Ignatius": "Church",
    "Königmuseum im Hofberg": None,
    "LANDSHUTmuseum": None,
    "Lebenshilfe": "Supermarket",
    "Lipp": "Supermarket",
    "Ländtor": "Gate",
    "Magdalenenheim": None,
    "Marstall": None,
    "Maxwehr": None,
    "Moserbräu": None,
    "Münzturm": "Tower",
    "Neuapostolische Kirche": "Church",
    "Ottonianum": None,
    "Pappenbergerhaus": None,
    "Pavillion": None,
    "Pfarrheim Sankt Martin": None,
    "Pulverturm": "Tower",
    "Realschulgebäude (HCG)": "School",
    "Regierung von Niederbayern": "TownHall",
    "Rochuskapelle": "Church",
    "Rumänisch-Orthodoxe Kirche Johannes der Wallache": "Church",
    "Salzstadl": None,
    "Sankt Pius": "Church",
    "Schwedentor": "Gate",
    "Sozialgericht Landshut, Arbeitsgericht Regensburg": None,
    "Sparkasse Landshut": None,
    "St Sebastian": "Church",
    "St. Jodok": "Church",
    "St. Konrad": "Church",
    "St. Nikola": "Church",
    "St. Nikolaus": "Church",
    "Stadtresidenz": "TownHall",
    "Sternwarte Seligenthal": "Tower",
    "Telefonladen Landshut": "Supermarket",
    "Theklakapelle": "Church",
    "Ursulinenkirche": "Church",
    "Ussar Villa": None,
    "Volkshochschule Landshut": None,
    "Waffenturm": "Tower",
    "Wartturm": "Tower",
    "Wittelsbacherturm": "Tower",
    "Zeughaus": None,
    "ehemaliges Ursulinenkloster Sankt Joseph": "School",
    "Äußeres Torwarthaus": "Gate",
}


# The one thing a landmark needs that neither the map nor a photograph can be
# read for: how tall it is.
#
# Only numbers that have a source. The rest are left to `kind_of` and the city
# style, because a guessed skyline is worse than an honest one -- and the game
# has always been able to make a plausible church out of a footprint.
#
# St. Martin is the tallest brick tower in the world and the reason Landshut has
# a skyline at all: 130.6 m, and the footprint the map gives it (91 m long)
# matches the 92 m interior the literature quotes, so the two agree.
LANDMARK_HEIGHT = {
    "Basilika Sankt Martin": 130.6,
}

# And what a landmark is, where the plain building tag does not say. A gate is
# not a house with a hole in it and a tower is not a thin house.
LANDMARK_KIND = {
    "castle": "TownHall",
    "city_gate": "Gate",
    "tower": "Tower",
    "chapel": "Church",
    "church": "Church",
    "monastery": "Church",
}

# What the game will build. Anything smaller is a bin store, a garden shed or a
# mapping artefact, and stamping it costs a draw call to show a doorstep.
SMALLEST = 24.0
# And anything bigger than this is a multipolygon whose outer ring went round
# the whole block, or a mapping accident. Two hectares: the largest honest
# building in Landshut, the shopping centre, is a little over one. A landmark
# is exempt.
BIGGEST = 20000.0

# The decomposition. Coordinates in a building's own frame are snapped to this
# grid; a cell counts as built on when its centre is inside the polygon and
# more than half its area is; rectangles are carved out largest first until
# `COVERAGE` of the built cells or `MAX_PARTS` are taken, and any rectangle
# with a side under `MIN_SIDE` is a sliver nobody would draw.
GRID = 0.5
MIN_SIDE = 1.5
MAX_PARTS = 6
COVERAGE = 0.97

# No single part longer than this. The renderer draws a part as one extruded
# box with one roof over it, and a hundred-metre ridge is a hangar; a part
# past `PART_LONGEST` is cut into equal segments of at most `PART_SEGMENT`
# along its long axis, all in the same group, so the shells and the roofs stay
# the size of buildings.
PART_LONGEST = 60.0
PART_SEGMENT = 40.0

# Metres of pavement the width cap leaves either side of a carriageway. The
# game's own pavement is 3.2 m but it tolerates one; 2.2 is what the Altstadt
# actually has between its kerb and its doorsteps.
# The pavement the game lays on each side of every carriageway
# (`citygen::SIDEWALK_WIDTH`), which is what a building's front stands behind.
PAVEMENT = 3.2
# A street that was folded out of several parallel ways is a band by design,
# and the cap may not take it below this share of that band.
# The narrowest a band may be capped to, in metres: absolute, not a share of
# the band, so a second bake of the file does not floor it a second time.
BAND_FLOOR = 8.0
# How far either side of a band's line its walls are looked for, and how much
# narrower a measured width has to be than the file's before it counts.
BAND_REACH = 45.0
STREET_REACH = 18.0
CAP_SLACK = 0.5
# Closer than this to a centreline, a building is in the road at any width
# the game can draw, and the cap leaves it to the runtime.
IN_ROAD = NARROWEST * 0.5

# Metres above the datum from which the runtime treats ground as hill rather
# than town. Mirrors `HILL` on the Rust side; the report counts what is above.
HILL = 12.0


def area_of(points):
    """Twice the signed area of a polygon, halved. Shoelace."""
    total = 0.0
    for i in range(len(points)):
        x1, y1 = points[i]
        x2, y2 = points[(i + 1) % len(points)]
        total += x1 * y2 - x2 * y1
    return abs(total) * 0.5


def hull(points):
    """Andrew's monotone chain. Returns the convex hull, anticlockwise."""
    points = sorted(set(points))
    if len(points) < 3:
        return points

    def half(source):
        out = []
        for p in source:
            while len(out) >= 2:
                (ax, ay), (bx, by) = out[-2], out[-1]
                if (bx - ax) * (p[1] - ay) - (by - ay) * (p[0] - ax) > 0:
                    break
                out.pop()
            out.append(p)
        return out

    return half(points)[:-1] + half(reversed(points))[:-1]


def smallest_box(points):
    """The smallest-area rotated rectangle containing `points`.

    Rotating calipers, in its simplest form: the minimum-area enclosing
    rectangle always has a side flush with a hull edge, so try each hull edge as
    the frontage direction and keep the cheapest. A hull here is a few dozen
    points and there are a few thousand buildings, which is nothing.

    Returns (centre, yaw, frontage, depth) with the frontage along the *longer*
    side, because that is what a plot is: wider on the street than it is deep is
    the exception, but the generator's own `Building` reads its footprint as
    frontage-by-depth in the street's frame and something has to be chosen.
    """
    ring = hull(points)
    if len(ring) < 3:
        return None
    best = None
    for i in range(len(ring)):
        ax, ay = ring[i]
        bx, by = ring[(i + 1) % len(ring)]
        edge = math.hypot(bx - ax, by - ay)
        if edge < 1e-6:
            continue
        ux, uy = (bx - ax) / edge, (by - ay) / edge
        # The perpendicular, so the hull can be measured in this edge's frame.
        vx, vy = -uy, ux
        us = [(px - ax) * ux + (py - ay) * uy for px, py in ring]
        vs = [(px - ax) * vx + (py - ay) * vy for px, py in ring]
        wide, deep = max(us) - min(us), max(vs) - min(vs)
        if best is None or wide * deep < best[0]:
            mid_u, mid_v = (max(us) + min(us)) * 0.5, (max(vs) + min(vs)) * 0.5
            centre = (ax + ux * mid_u + vx * mid_v, ay + uy * mid_u + vy * mid_v)
            best = (wide * deep, centre, (ux, uy), wide, deep)
    if best is None:
        return None
    _, centre, (ux, uy), wide, deep = best
    if deep > wide:
        # Turn a quarter so the frontage is the long side.
        ux, uy, wide, deep = -uy, ux, deep, wide
    # The game's yaw sends local +Z outward across the pavement and local +X
    # along the frontage. `Building::facing` is read by `buildings::site_in` as
    # `Vec2::new(yaw.cos(), -yaw.sin())` being the frontage direction, so:
    yaw = math.atan2(-uy, ux)
    return centre, yaw, wide, deep


def part_of(centre, along, wide, deep):
    """One rectangle as the file carries it: centre, yaw, frontage, depth.

    `along` is the unit direction of the `wide` side in world (x, z). The
    frontage is always the longer side — a plot is wider on the street than it
    is deep, and the runtime turns the box to face its street anyway — so a
    part that arrives deeper than it is wide is turned a quarter here.
    """
    ux, uy = along
    if deep > wide:
        ux, uy, wide, deep = -uy, ux, deep, wide
    return {
        "centre": centre,
        "yaw": math.atan2(-uy, ux),
        "frontage": wide,
        "depth": deep,
    }


def frontage_axis(part):
    """The unit direction of a part's frontage in world (x, z), from its yaw.

    The inverse of `part_of`: `yaw = atan2(-uy, ux)` means `ux = cos(yaw)`
    and `uy = -sin(yaw)`.
    """
    return math.cos(part["yaw"]), -math.sin(part["yaw"])


def rectangle_of(part):
    """A part as a shapely polygon, for measuring against."""
    ux, uy = frontage_axis(part)
    vx, vy = -uy, ux
    cx, cz = part["centre"]
    hw, hd = part["frontage"] * 0.5, part["depth"] * 0.5
    return Polygon([
        (cx + ux * hw + vx * hd, cz + uy * hw + vy * hd),
        (cx - ux * hw + vx * hd, cz - uy * hw + vy * hd),
        (cx - ux * hw - vx * hd, cz - uy * hw - vy * hd),
        (cx + ux * hw - vx * hd, cz + uy * hw - vy * hd),
    ])


def principal_angle(polygon):
    """The direction a building's walls mostly run, modulo a quarter turn.

    Every edge votes with its length for its own bearing times four, so that
    walls at right angles to each other vote for the same thing; the mean of
    the votes, divided back by four, is the frame. Weighting by length is
    what makes a long facade with a bay window on it come out square to the
    facade rather than to the bay.
    """
    sx = sy = 0.0
    for ring in [polygon.exterior, *polygon.interiors]:
        xs, ys = ring.coords.xy
        for i in range(len(xs) - 1):
            dx, dy = xs[i + 1] - xs[i], ys[i + 1] - ys[i]
            length = math.hypot(dx, dy)
            if length < 0.3:
                continue
            turned = math.atan2(dy, dx) * 4.0
            sx += length * math.cos(turned)
            sy += length * math.sin(turned)
    return math.atan2(sy, sx) / 4.0


def grid_lines(polygon):
    """Where the cell boundaries fall in a building's own frame.

    Not a uniform grid: the lines are the polygon's own vertex coordinates,
    snapped to `GRID`. A rectangle then costs one cell and an L-shape three,
    and a wall that is really at 4.3 m is drawn at 4.5 rather than blurred
    across whichever uniform cell it landed in.
    """
    minx, miny, maxx, maxy = polygon.bounds
    xs = {0, round((maxx - minx) / GRID)}
    ys = {0, round((maxy - miny) / GRID)}
    for ring in [polygon.exterior, *polygon.interiors]:
        rx, ry = ring.coords.xy
        xs.update(round((x - minx) / GRID) for x in rx)
        ys.update(round((y - miny) / GRID) for y in ry)
    return ([minx + v * GRID for v in sorted(xs)],
            [miny + v * GRID for v in sorted(ys)])


def built_cells(polygon, xs, ys):
    """Which grid cells the building stands on: centre inside, and more than
    half the cell inside. Boolean, rows are y."""
    nx, ny = len(xs) - 1, len(ys) - 1
    if nx <= 0 or ny <= 0:
        return None
    x0 = np.repeat(np.asarray(xs[:-1])[None, :], ny, axis=0)
    x1 = np.repeat(np.asarray(xs[1:])[None, :], ny, axis=0)
    y0 = np.repeat(np.asarray(ys[:-1])[:, None], nx, axis=1)
    y1 = np.repeat(np.asarray(ys[1:])[:, None], nx, axis=1)
    inside = shapely.contains_xy(polygon, (x0 + x1) * 0.5, (y0 + y1) * 0.5)
    if not inside.any():
        return inside
    # The cells whose centre is in but that straddle the wall: only those
    # need the area test, and it is the expensive one.
    cells = shapely.box(x0[inside], y0[inside], x1[inside], y1[inside])
    shapely.prepare(polygon)
    whole = shapely.contains_properly(polygon, cells)
    partial = ~whole
    if partial.any():
        overlap = shapely.area(shapely.intersection(polygon, cells[partial]))
        full = (x1[inside][partial] - x0[inside][partial]) * (y1[inside][partial] - y0[inside][partial])
        whole[partial] = overlap > 0.5 * full
    inside[inside] = whole
    return inside


def largest_rectangle(free, xs, ys):
    """The largest rectangle of free cells, in metres, or None.

    The classic largest-rectangle-in-a-histogram, one row at a time: for
    each row as a top edge the bars are how many free cells run down from
    it, and a stack finds the widest span each bar can be the bottom of. The
    grid is not uniform so the area is measured in metres, but the argument
    still holds — for a given bottom the widest span is the best span — so
    the stack's answer is the optimum. O(cells) per call.
    """
    ny, nx = free.shape
    down = np.zeros((ny + 1, nx), dtype=np.int32)
    for j in range(ny - 1, -1, -1):
        down[j] = np.where(free[j], down[j + 1] + 1, 0)
    best = None
    for j in range(ny):
        bars = down[j]
        if not bars.any():
            continue
        stack = []  # (start column, height)
        for i in range(nx + 1):
            height = int(bars[i]) if i < nx else 0
            start = i
            while stack and stack[-1][1] >= height:
                left, tall = stack.pop()
                area = (xs[i] - xs[left]) * (ys[j + tall] - ys[j])
                if best is None or area > best[0]:
                    best = (area, left, i, j, j + tall)
                start = left
            if height > 0:
                stack.append((start, height))
    return best


def carve(polygon):
    """Decomposes a polygon in its own frame into rectangles, largest first.

    Returns (rectangles as (minx, miny, maxx, maxy), area of the built cells).
    Stops at `MAX_PARTS`, at `COVERAGE` of the built area, or when what is
    left is slivers.
    """
    xs, ys = grid_lines(polygon)
    inside = built_cells(polygon, xs, ys)
    if inside is None or not inside.any():
        return [], 0.0
    widths = np.diff(np.asarray(xs))[None, :]
    heights = np.diff(np.asarray(ys))[:, None]
    total = float((inside * widths * heights).sum())
    free = inside.copy()
    rects, taken = [], 0.0
    while len(rects) < MAX_PARTS and taken < COVERAGE * total:
        found = largest_rectangle(free, xs, ys)
        if found is None:
            break
        area, i0, i1, j0, j1 = found
        free[j0:j1, i0:i1] = False
        if xs[i1] - xs[i0] < MIN_SIDE or ys[j1] - ys[j0] < MIN_SIDE:
            # Largest first, so everything after this is a sliver too.
            break
        rects.append((xs[i0], ys[j0], xs[i1], ys[j1]))
        taken += area
    return rects, total


def iou(a, b):
    union = a.union(b).area
    return a.intersection(b).area / union if union > 0 else 0.0


# The kinds the game builds out of one box and its own idea of the anatomy: a
# church is a nave, a tower and a spire raised on its footprint by
# `world::church`, and cut into wings it would be three small churches; a
# parking garage is decks and ramps by `world::garage`. These keep the one box
# a landmark always was, and are never cut for length.
ONE_BOX_KINDS = {"Church", "Cathedral", "Gate", "Tower", "Stadium", "ParkingGarage"}


def decompose(polygon, whole=False):
    """One building polygon (metres, world frame) into parts.

    `whole` keeps the one box whatever the polygon does — see `ONE_BOX_KINDS`.

    Returns (parts, iou, iou of the one box, method). The decomposition is
    measured against the one box the bake used to emit, and the box is kept
    wherever it fits the polygon at least as well — a plain rectangular house
    is one box either way, and the box has exact walls where the grid has
    half-metre ones.
    """
    corners = list(polygon.exterior.coords)
    fitted = smallest_box(corners)
    if fitted is None:
        return [], 0.0, 0.0, "none"
    (bx, bz), byaw, bwide, bdeep = fitted
    box_part = {"centre": (bx, bz), "yaw": byaw, "frontage": bwide, "depth": bdeep}
    iou_box = iou(rectangle_of(box_part), polygon)
    if whole:
        return [box_part], iou_box, iou_box, "box"

    angle = principal_angle(polygon)
    pivot = polygon.centroid
    local = affinity.rotate(polygon, -math.degrees(angle), origin=(pivot.x, pivot.y))
    rects, _ = carve(local)
    if not rects:
        return [box_part], iou_box, iou_box, "box"
    union = unary_union([box(*r) for r in rects])
    iou_grid = iou(union, local)
    if iou_grid < iou_box:
        return [box_part], iou_box, iou_box, "box"

    cos, sin = math.cos(angle), math.sin(angle)
    parts = []
    for minx, miny, maxx, maxy in rects:
        lx, ly = (minx + maxx) * 0.5 - pivot.x, (miny + maxy) * 0.5 - pivot.y
        centre = (pivot.x + lx * cos - ly * sin, pivot.y + lx * sin + ly * cos)
        parts.append(part_of(centre, (cos, sin), maxx - minx, maxy - miny))
    return parts, iou_grid, iou_box, "grid"


def split_long(part):
    """Cuts a part longer than `PART_LONGEST` into segments of at most
    `PART_SEGMENT` along its long axis, and again if the pieces are still too
    deep the other way."""
    if part["frontage"] <= PART_LONGEST:
        return [part]
    count = math.ceil(part["frontage"] / PART_SEGMENT)
    length = part["frontage"] / count
    ux, uy = frontage_axis(part)
    cx, cz = part["centre"]
    out = []
    for k in range(count):
        along = length * (k + 0.5) - part["frontage"] * 0.5
        piece = part_of((cx + ux * along, cz + uy * along), (ux, uy), length, part["depth"])
        out.extend(split_long(piece))
    return out


def storeys(tags):
    """How tall, in metres, or None to let the city style decide."""
    raw = tags.get("height")
    if raw:
        try:
            metres = float(str(raw).split()[0].replace(",", "."))
            if 2.0 <= metres <= 140.0:
                return round(metres, 1)
        except ValueError:
            pass
    levels = levels_of(tags)
    if levels is not None:
        # Plus the ground floor's extra height and a roof.
        return round(levels * STOREY + 1.6, 1)
    return None


def levels_of(tags):
    raw = tags.get("building:levels")
    if raw:
        try:
            levels = float(str(raw).split(";")[0].replace(",", "."))
            if 0.5 <= levels <= 45.0:
                return levels
        except ValueError:
            pass
    return None


def kind_of(tags):
    if tags.get("man_made") in ("tower", "water_tower", "observatory"):
        return "Tower"
    for key in ("historic", "castle_type"):
        value = tags.get(key)
        if value and value in LANDMARK_KIND:
            return LANDMARK_KIND[value]
    building = tags.get("building")
    if building in OUTBUILDING and not tags.get("name"):
        return None
    for key in ("building", "amenity", "shop", "man_made"):
        value = tags.get(key)
        if value and value in KIND:
            return KIND[value]
    if tags.get("shop"):
        return "Supermarket"
    if tags.get("amenity") in ("restaurant", "cafe", "bar", "pub", "fast_food"):
        return "Restaurant"
    if tags.get("tourism") == "hotel":
        return "Hotel"
    return None


def outer_rings(element):
    """The outer ring(s) of a way or a multipolygon relation, as coordinates."""
    if element.get("type") == "way":
        geometry = element.get("geometry") or []
        return [[(n["lat"], n["lon"]) for n in geometry]] if len(geometry) >= 4 else []
    rings = []
    for member in element.get("members", []) or []:
        if member.get("role") not in ("outer", ""):
            continue
        geometry = member.get("geometry") or []
        if len(geometry) >= 4:
            rings.append([(n["lat"], n["lon"]) for n in geometry])
    return rings


def polygons_of(element, lat0, lon0):
    """A building element as shapely polygons in metres, holes and all.

    A relation's members are open ways as often as closed rings — an outer
    ring drawn as four ways meeting at the corners — so the members of each
    role are merged end to end first and whatever closes is a ring. Inner
    rings become the holes of whichever outer contains them.
    """
    if element.get("type") == "way":
        geometry = element.get("geometry") or []
        if len(geometry) < 4:
            return []
        ring = [project(n["lat"], n["lon"], lat0, lon0) for n in geometry]
        return valid_polygons(Polygon(ring))

    def rings(role):
        lines = []
        for member in element.get("members", []) or []:
            if (member.get("role") or "outer") != role:
                continue
            geometry = member.get("geometry") or []
            if len(geometry) >= 2:
                lines.append(LineString([project(n["lat"], n["lon"], lat0, lon0) for n in geometry]))
        if not lines:
            return []
        merged = linemerge(lines) if len(lines) > 1 else lines[0]
        parts = list(merged.geoms) if hasattr(merged, "geoms") else [merged]
        return [Polygon(line.coords) for line in parts if line.is_ring and len(line.coords) >= 4]

    outers, inners = rings("outer"), rings("inner")
    out = []
    for outer in outers:
        holes = [inner.exterior.coords for inner in inners if outer.contains(inner.representative_point())]
        out.extend(valid_polygons(Polygon(outer.exterior.coords, holes)))
    return out


def valid_polygons(geometry):
    """Whatever polygons a geometry honestly contains, made valid."""
    if geometry.is_empty:
        return []
    if not geometry.is_valid:
        geometry = shapely.make_valid(geometry)
    if geometry.geom_type == "Polygon":
        return [geometry]
    if hasattr(geometry, "geoms"):
        out = []
        for piece in geometry.geoms:
            out.extend(valid_polygons(piece))
        return out
    return []


def candidates_from_overpass(elements, lat0, lon0):
    """Buildings out of an Overpass dump, as the records the pipeline reads."""
    out = []
    for element in elements:
        tags = element.get("tags", {}) or {}
        if "building" not in tags:
            continue
        polygons = polygons_of(element, lat0, lon0)
        if not polygons:
            continue
        name = tags.get("name", "")
        landmark = is_named(tags)
        levels = levels_of(tags)
        out.append({
            "polygons": polygons,
            "name": name if landmark else "",
            "landmark": landmark,
            "kind": kind_of(tags) or (KNOWN_LANDMARKS.get(name) if landmark else None),
            "height": LANDMARK_HEIGHT.get(name) if landmark and name in LANDMARK_HEIGHT else storeys(tags),
            "levels": int(levels) if levels is not None and levels == int(levels) else None,
            "roof": ROOF.get(tags.get("roof:shape", "")),
        })
    return out


def ml_estimated(sources, what):
    """Whether Overture's value for a property is Microsoft's guess.

    Overture merges OSM with Microsoft's imagery-traced footprints, and where
    OSM has no `height` it fills one in from the ML dataset. Those are
    estimates from a shadow — St. Martin comes out at twenty metres — and the
    `sources` list is where the file admits it: an entry whose dataset is the
    ML one and whose property names the value.
    """
    for source in sources or []:
        if source.get("dataset") == "Microsoft ML Buildings" and what in (source.get("property") or ""):
            return True
    return False


def kind_of_overture(cls, subtype, name):
    if cls in OUTBUILDING and not name:
        return None
    if cls in KIND:
        return KIND[cls]
    return SUBTYPE_KIND.get(subtype)


def candidates_from_overture(path, lat0, lon0):
    """Buildings out of an Overture GeoParquet, as the same records."""
    import pyarrow.parquet as pq

    table = pq.read_table(path)
    provenance = {k.decode(): v.decode() for k, v in (table.schema.metadata or {}).items()
                  if k in (b"overture_release", b"overture_theme", b"bbox", b"fetched")}
    rows = table.to_pydict()
    count = table.num_rows
    cos0 = math.cos(math.radians(lat0))

    def to_metres(coords):
        out = np.empty_like(coords)
        out[:, 0] = np.radians(coords[:, 0] - lon0) * cos0 * EARTH
        out[:, 1] = -np.radians(coords[:, 1] - lat0) * EARTH
        return out

    out = []
    ml_footprints = 0
    for i in range(count):
        geometry = shapely.transform(shapely.from_wkb(rows["geometry"][i]), to_metres)
        polygons = valid_polygons(geometry)
        if not polygons:
            continue
        sources = rows["sources"][i] or []
        if any(s.get("dataset") == "Microsoft ML Buildings" and not s.get("property") for s in sources):
            ml_footprints += 1
        names = rows["names"][i] or {}
        name = names.get("primary") or ""
        cls, subtype = rows["class"][i], rows["subtype"][i]
        landmark = bool(name) and (
            cls in LANDMARK_CLASSES or subtype == "religious" or name in KNOWN_LANDMARKS
            or LANDMARK_WORDS.search(name) is not None
        )
        height = None
        raw = rows["height"][i]
        if raw is not None and not ml_estimated(sources, "height") and 2.0 <= raw <= 140.0:
            height = round(float(raw), 1)
        levels = rows["num_floors"][i]
        if levels is not None and (ml_estimated(sources, "floors") or not 0.5 <= levels <= 45.0):
            levels = None
        if height is None and levels is not None:
            height = round(levels * STOREY + 1.6, 1)
        if landmark and name in LANDMARK_HEIGHT:
            height = LANDMARK_HEIGHT[name]
        out.append({
            "polygons": polygons,
            "name": name if landmark else "",
            "landmark": landmark,
            "kind": kind_of_overture(cls, subtype, name) or (KNOWN_LANDMARKS.get(name) if landmark else None),
            "height": height,
            "levels": int(levels) if levels is not None else None,
            "roof": ROOF.get(rows["roof_shape"][i] or ""),
        })
    provenance["ml_footprints"] = ml_footprints
    return out, provenance


def bake_buildings(candidates, half):
    """Runs every candidate through the decomposition. Returns the entries the
    file carries and the numbers worth printing about them."""
    square = box(-half, -half, half, half)
    stats = collections.Counter()
    ious, box_ious, parts_hist = [], [], collections.Counter()
    covered = true_area = 0.0
    entries = []
    group = 0
    for cand in candidates:
        polygons = [p for p in cand["polygons"] if p.intersects(square)]
        if not polygons:
            stats["outside"] += 1
            continue
        area = sum(p.area for p in cand["polygons"])
        if area < SMALLEST and not cand["landmark"]:
            stats["small"] += 1
            continue
        if area > BIGGEST and not cand["landmark"]:
            stats["big"] += 1
            continue
        parts, weights, boxes = [], [], []
        # Not `whole`: that name is the union of the polygons a few lines
        # down, and it is truthy.
        one_box = cand["kind"] in ONE_BOX_KINDS
        for polygon in polygons:
            found, score, score_box, method = decompose(polygon, one_box)
            if not found:
                continue
            stats[method] += 1
            boxes.append((score_box, polygon.area))
            weights.append((score, polygon.area))
            parts.extend(found)
        if not parts:
            stats["degenerate"] += 1
            continue
        whole = unary_union(polygons)
        union = unary_union([rectangle_of(p) for p in parts])
        covered += union.intersection(whole).area
        true_area += whole.area
        ious.append(sum(s * a for s, a in weights) / sum(a for _, a in weights))
        box_ious.append(sum(s * a for s, a in boxes) / sum(a for _, a in boxes))
        parts_hist[len(parts)] += 1
        pieces = []
        for part in parts:
            cut = [part] if one_box else split_long(part)
            if len(cut) > 1:
                stats["split"] += 1
            pieces.extend(cut)
        # Largest first, after the cutting: the runtime reads the first part
        # of a group as the building -- where it stands, which district, which
        # height band -- so the first part has to be the one that matters.
        pieces.sort(key=lambda piece: -(piece["frontage"] * piece["depth"]))
        for piece in pieces:
            entries.append({
                "name": cand["name"],
                "centre": piece["centre"],
                "yaw": piece["yaw"],
                "frontage": piece["frontage"],
                "depth": piece["depth"],
                "height": cand["height"],
                "kind": cand["kind"],
                "group": group,
                "area": area,
                "roof": cand["roof"],
                "levels": cand["levels"],
            })
        group += 1
        stats["kept"] += 1
        if cand["landmark"]:
            stats["landmarks"] += 1
    return entries, {
        "counts": stats,
        "parts": parts_hist,
        "iou": ious,
        "box_iou": box_ious,
        "coverage": covered / true_area if true_area else 0.0,
    }


# ----------------------------------------------------- open ground, water ----

# What open ground the game knows how to dress. Everything else in `landuse` is
# a label on a district rather than a surface -- `residential`, `commercial`,
# `retail` say what the buildings are for, not what the ground between them
# looks like, and the game already decides that from its own districts.
OPEN_GROUND = {
    "grass": "Grass",
    "meadow": "Grass",
    "village_green": "Grass",
    "recreation_ground": "Grass",
    "park": "Park",
    "garden": "Park",
    "cemetery": "Cemetery",
    "orchard": "Trees",
    "forest": "Trees",
    "wood": "Trees",
    "allotments": "Allotments",
    "farmland": "Field",
    "pitch": "Pitch",
    "playground": "Playground",
    "parking": "Parking",
}

# How wide the game draws a waterway, per class, when the mappers did not say.
WATER_WIDTH = {"river": 42.0, "canal": 14.0, "stream": 5.0}
# And what it will believe if they did. The Isar's two arms both carry
# `width=60`, which is the whole braided channel including the islands: drawn at
# sixty each they overlap Wittstraße by four metres.
WATER_RANGE = {"river": (18.0, 52.0), "canal": (5.0, 24.0), "stream": (2.0, 10.0)}

# How far apart the points of a bank may be, in metres.
#
# The opposite of what the streets get. A street is *thinned* to four metres
# because the game draws a straight ribbon between consecutive points and a
# kerb traced at sub-metre precision is geometry nobody sees. A river is the
# other way round: Landshut's six river ways are forty-three segments over five
# kilometres, so a bank drawn straight between them is a polygon, and the one
# thing a river must not look like is a canal.
WATER_STEP = 12.0


def river_width(tags, kind):
    low, high = WATER_RANGE[kind]
    raw = tags.get("width")
    if raw:
        try:
            measured = float(str(raw).split(";")[0].replace("m", "").strip())
            return round(min(max(measured, low), high), 1)
        except ValueError:
            pass
    return WATER_WIDTH[kind]


def densify(points):
    """Splits any run longer than `WATER_STEP` until none is."""
    out = [points[0]]
    for ahead in points[1:]:
        behind = out[-1]
        run = math.dist(behind, ahead)
        # Rounded up, not truncated: at `int` a 23 m run is one step and stays
        # a 23 m straight, which is the one thing a river must not be.
        steps = max(1, math.ceil(run / WATER_STEP))
        for step in range(1, steps + 1):
            t = step / steps
            out.append((behind[0] + (ahead[0] - behind[0]) * t,
                        behind[1] + (ahead[1] - behind[1]) * t))
    return out


def smooth(points, passes=2):
    """Chaikin's corner cutting, keeping the two ends put.

    Two passes turn a polyline's corners into something a bank can be drawn
    along. Any more and the river shrinks away from its own islands.
    """
    for _ in range(passes):
        if len(points) < 3:
            break
        cut = [points[0]]
        for behind, ahead in zip(points, points[1:]):
            cut.append((behind[0] * 0.75 + ahead[0] * 0.25,
                        behind[1] * 0.75 + ahead[1] * 0.25))
            cut.append((behind[0] * 0.25 + ahead[0] * 0.75,
                        behind[1] * 0.25 + ahead[1] * 0.75))
        cut.append(points[-1])
        points = cut
    return points


def thin_ring(points):
    """Drops points a park's outline will not miss."""
    out = [points[0]]
    for point in points[1:-1]:
        if math.dist(point, out[-1]) >= 6.0:
            out.append(point)
    out.append(points[-1])
    return out


# --------------------------------------------------------------- streets ----

# One street is often several ways, and the game cannot afford to believe that.
#
# "Altstadt" is twenty-two ways in this extract: 1908 m of centreline for a
# street the literature gives as seven hundred metres long and thirty wide,
# because the carriageway, the parking lanes and the pedestrian halves are each
# mapped separately. The runtime draws every one of them as a full street with
# a carriageway and two pavements, and what that produces is not a market
# square -- it is nine parallel stripes of alternating paving, a thicket of
# lamp posts, and kerbs marooned in the middle of it with nothing behind them.
#
# So ways of one name that run alongside each other become one way as wide as
# the band they cover. Two things keep it honest: they must actually be
# parallel (a street that turns a corner and keeps its name is not two lanes of
# itself), and the survivor is moved to the middle of the band rather than left
# on whichever lane happened to be longest.
PARALLEL_GAP = 34.0
PARALLEL_ANGLE = 0.44  # about 25 degrees
# How much of a way has to run alongside the other before it is the same street.
PARALLEL_SHARE = 0.6


def bearing_of(a, b):
    return math.atan2(b[1] - a[1], b[0] - a[0])


def nearest_on(points, at):
    """Distance from `at` to a polyline, and the bearing of the segment it is
    nearest to."""
    best = (float("inf"), 0.0, 0.0)
    for behind, ahead in zip(points, points[1:]):
        dx, dy = ahead[0] - behind[0], ahead[1] - behind[1]
        span = dx * dx + dy * dy
        if span < 1e-9:
            continue
        t = max(0.0, min(1.0, ((at[0] - behind[0]) * dx + (at[1] - behind[1]) * dy) / span))
        foot = (behind[0] + dx * t, behind[1] + dy * t)
        gap = math.dist(at, foot)
        if gap < best[0]:
            # Signed: which side of the line this point is on, so a band's
            # extent can be measured rather than just its width.
            side = ((at[0] - behind[0]) * dy - (at[1] - behind[1]) * dx) / math.sqrt(span)
            best = (gap, bearing_of(behind, ahead), side)
    return best


def alongside(leader, other):
    """Does `other` run beside `leader`? Returns the signed offsets if so."""
    offsets = []
    beside = 0
    for at in other["points"]:
        gap, bearing, side = nearest_on(leader["points"], at)
        if gap > PARALLEL_GAP:
            continue
        # Modulo a half turn: a way mapped in the opposite direction is still
        # the same street.
        turn = abs((bearing_of(*other["points"][:2]) - bearing + math.pi / 2) % math.pi - math.pi / 2)
        if turn > PARALLEL_ANGLE:
            continue
        beside += 1
        offsets.append(side)
    if beside < max(2, len(other["points"]) * PARALLEL_SHARE):
        return None
    return offsets


# Over how many metres the shift eases away from a junction.
WELD_EASE = 22.0


def distance_to_weld(street):
    """How far each point is, along the way, from the nearest shared one."""
    points, welds = street["points"], street.get("welds") or []
    count = len(points)
    far = [float("inf")] * count
    # The two ends are joins too, whether or not the extract marked them: a way
    # that stops is a way another one may start at.
    for index in range(count):
        if index == 0 or index == count - 1 or (index < len(welds) and welds[index]):
            far[index] = 0.0
    for index in range(1, count):
        step = math.dist(points[index - 1], points[index])
        far[index] = min(far[index], far[index - 1] + step)
    for index in range(count - 2, -1, -1):
        step = math.dist(points[index], points[index + 1])
        far[index] = min(far[index], far[index + 1] + step)
    return far


def merge_parallel(streets):
    """Folds ways of one name that run alongside each other into one."""
    by_name = {}
    for street in streets:
        by_name.setdefault(street["name"], []).append(street)

    out, folded = [], 0
    for name, group in by_name.items():
        if not name or len(group) < 2:
            out.extend(group)
            continue
        # Longest first, so the survivor is the one that describes the street.
        group.sort(key=lambda s: -s["length"])
        taken = [False] * len(group)
        for i, leader in enumerate(group):
            if taken[i]:
                continue
            taken[i] = True
            low = -leader["width"] * 0.5
            high = leader["width"] * 0.5
            for j in range(i + 1, len(group)):
                if taken[j]:
                    continue
                offsets = alongside(leader, group[j])
                if offsets is None:
                    continue
                taken[j] = True
                folded += 1
                half = group[j]["width"] * 0.5
                low = min(low, min(offsets) - half)
                high = max(high, max(offsets) + half)
            width = min(high - low, PARALLEL_GAP)
            if width > leader["width"]:
                # Move the survivor to the middle of the band it now covers,
                # rather than leaving it on whichever lane happened to be
                # longest -- but never move a weld.
                #
                # A shared coordinate is the only thing that joins two streets
                # into one graph when the runtime loads this, so a shifted
                # junction is a junction that stops existing. Moving them all
                # cost the town its connectivity: the patrol went from walking
                # fifteen junctions a minute to one.
                #
                # So the shift eases in from every weld over `WELD_EASE` metres
                # and is full only where the street is on its own.
                shift = (high + low) * 0.5
                free = distance_to_weld(leader)
                moved = []
                for index, at in enumerate(leader["points"]):
                    behind = leader["points"][max(index - 1, 0)]
                    ahead = leader["points"][min(index + 1, len(leader["points"]) - 1)]
                    bearing = bearing_of(behind, ahead)
                    ease = min(free[index] / WELD_EASE, 1.0)
                    # The normal, on the same hand `nearest_on` measures from.
                    moved.append((at[0] + math.sin(bearing) * shift * ease,
                                  at[1] - math.cos(bearing) * shift * ease))
                leader["points"] = moved
                leader["width"] = round(width, 1)
                # Remembered for the width cap: a band is wide on purpose.
                leader["band"] = True
            out.append(leader)
    return out, folded


def carriageway(tags, kind):
    """How wide to draw this way, in metres.

    Three sources, most specific first: what a mapper measured, what a mapper
    counted, and what the class implies. Only five ways in Landshut carry a
    `width` and four hundred carry `lanes`, so the middle one is where nearly
    all of the variety comes from.
    """
    raw = tags.get("width")
    if raw:
        try:
            # Free text: "8", "8 m", "3;4" for a way that changes width.
            measured = float(raw.split(";")[0].replace("m", "").strip())
            if NARROWEST <= measured <= WIDEST:
                return round(measured, 1)
        except ValueError:
            pass

    lanes = tags.get("lanes")
    if lanes:
        try:
            counted = float(lanes.split(";")[0])
            if counted >= 1.0:
                return round(min(max(counted * LANE + MARGIN, NARROWEST), WIDEST), 1)
        except ValueError:
            pass

    return WIDTH.get(kind)


def streets_from_overpass(data, lat0, lon0, half):
    """The ways of a roads dump, thinned, as the street records."""
    # Which nodes two or more ways have in common. Those are the junctions, and
    # they are the whole reason the runtime gets a connected graph rather than
    # five hundred and fifty loose polylines: it welds points that share a
    # coordinate, and a coordinate is only shared if the node survived the
    # thinning below.
    #
    # Counted from the node ids rather than guessed from the geometry, because
    # a way does not have to *end* at a junction — it can pass straight through
    # one — and thinning such a point away silently disconnects two streets
    # that meet in the middle of a third.
    shared = {}
    for element in data.get("elements", []):
        for node in element.get("nodes", []) or []:
            shared[node] = shared.get(node, 0) + 1

    streets = []
    dropped = 0
    for element in data.get("elements", []):
        if element.get("type") != "way":
            continue
        tags = element.get("tags", {})
        kind = tags.get("highway")
        if kind not in WIDTH:
            continue
        width = carriageway(tags, kind)
        surface = SURFACE.get(tags.get("surface", ""), "Asphalt")

        ids = element.get("nodes", []) or []
        points = []
        for index, node in enumerate(element.get("geometry", []) or []):
            x, z = project(node["lat"], node["lon"], lat0, lon0)
            junction = index < len(ids) and shared.get(ids[index], 0) > 1
            points.append((x, z, junction))
        if len(points) < 2:
            continue
        # Keep a way if any of it is inside the square; the runtime clips the
        # rest. Dropping on the first point alone would cut a street in half
        # at the boundary and leave the junction hanging.
        if not any(abs(x) <= half and abs(z) <= half for x, z, _ in points):
            dropped += 1
            continue

        # Collapse points closer together than a step. An OSM way traces a kerb
        # line at sub-metre precision and the game draws a straight ribbon
        # between consecutive points, so a metre of detail is a metre of
        # geometry nobody will ever see.
        #
        # Junctions and the two ends are never collapsed, whatever they are
        # near. They are the only points another street can be welded to.
        thinned = [points[0]]
        for point in points[1:-1]:
            previous = thinned[-1]
            if point[2] or math.dist(point[:2], previous[:2]) >= 4.0:
                thinned.append(point)
        thinned.append(points[-1])
        if len(thinned) < 2:
            continue

        points = [(x, z) for x, z, _ in thinned]
        streets.append({
            "name": tags.get("name", ""),
            "width": width,
            "arterial": kind in ARTERIAL,
            "surface": surface,
            "points": points,
            # Which of those points another way also uses. The merge below must
            # not move one: a shared coordinate is the *only* thing that welds
            # two streets into one graph at load time, so shifting one silently
            # disconnects every side street that met there.
            "welds": [bool(j) for _, _, j in thinned],
            "length": sum(math.dist(a, b) for a, b in zip(points, points[1:])),
            # A way that stays under something is not one the buildings above
            # it stand in.
            "covered": tags.get("tunnel") in ("yes", "building_passage") or "tunnel" in tags.get("name", "").lower(),
        })
    return streets, dropped


# How far a band's centreline may be moved to sit between its walls, per
# point, and how far from a corner the two walls have to agree before a point
# trusts them.
RECENTRE_MAX = 16.0
RECENTRE_REACH = 40.0
RECENTRE_DEAD = 1.0


def shared_points(streets):
    """Which coordinates two or more streets have in common: the welds."""
    seen, shared = {}, set()
    for index, street in enumerate(streets):
        for point in street["points"]:
            key = (round(point[0], 1), round(point[1], 1))
            owner = seen.setdefault(key, index)
            if owner != index:
                shared.add(key)
    return shared


def recentre_bands_once(streets, entries, half, tree, rectangles, centres, welds):
    """Moves a folded band's centreline into the middle of its street wall.

    `merge_parallel` widens the survivor to the band it covers but leaves its
    line on whichever lane it was, easing towards the middle only where no
    weld pins it — and along the Altstadt a side street joins every thirty
    metres, so it never got there at all. Measured against the real buildings,
    the line ran four metres from the western wall and twenty-five from the
    eastern: drawn at the band's width, the carriageway reached twelve metres
    into the western houses, the runtime dropped every one of them as
    standing in a street, and the market was a paved field with a wall on one
    side of it.

    With the buildings in hand the middle is measurable. Per segment, the
    nearest part on either side; per point, the offset that puts it half way
    between them, averaged over the segments the point belongs to and capped
    at `RECENTRE_MAX`; a weld moves with every street that shares it, which
    is what keeps the junction a junction — the side street simply reaches
    that much further into the square, which is where it always went.
    """
    moves = {}
    moved_bands = 0
    for street in streets:
        if not street.get("band") or street.get("covered"):
            continue
        points = street["points"]
        reach = street["width"] + RECENTRE_REACH
        offsets = [[] for _ in points]
        for index, (a, b) in enumerate(zip(points, points[1:])):
            if not (inside(a, half) or inside(b, half)):
                continue
            segment = LineString([a, b])
            minx, miny, maxx, maxy = segment.bounds
            found = tree.query(box(minx - reach, miny - reach, maxx + reach, maxy + reach))
            if len(found) == 0:
                continue
            gaps = shapely.distance(segment, tree.geometries.take(found))
            dx, dy = b[0] - a[0], b[1] - a[1]
            cross = dx * (centres[found, 1] - a[1]) - dy * (centres[found, 0] - a[0])
            nearest = [float("inf"), float("inf")]
            for gap, hand in zip(gaps, cross):
                if gap > reach:
                    continue
                side = 0 if hand < 0 else 1
                nearest[side] = min(nearest[side], gap)
            if any(n == float("inf") for n in nearest):
                continue
            # Positive towards the side `cross > 0` lies on.
            shift = (nearest[1] - nearest[0]) * 0.5
            offsets[index].append(shift)
            offsets[index + 1].append(shift)
        if not any(offsets):
            continue
        moved_bands += 1
        for index, at in enumerate(points):
            if not offsets[index]:
                continue
            shift = max(-RECENTRE_MAX, min(RECENTRE_MAX, statistics.median(offsets[index])))
            # A line already within a metre of the middle stays put, which is
            # what makes a second bake of the same file the same file: the
            # first pass leaves every point a few decimetres off the exact
            # median of its walls, and without a dead band the second pass
            # would chase that and the cap would measure a different street.
            if abs(shift) < RECENTRE_DEAD:
                continue
            behind = points[max(index - 1, 0)]
            ahead = points[min(index + 1, len(points) - 1)]
            dx, dy = ahead[0] - behind[0], ahead[1] - behind[1]
            length = math.hypot(dx, dy)
            if length < 1e-6:
                continue
            # The hand `cross` measures on: the normal (-dy, dx) is the side
            # where cross > 0.
            normal = (-dy / length, dx / length)
            moves[(round(at[0], 1), round(at[1], 1))] = (
                at[0] + normal[0] * shift,
                at[1] + normal[1] * shift,
            )
    if not moves:
        return 0, 0, 0
    # Apply, to the band and to every street welded to a moved point.
    for street in streets:
        street["points"] = [
            moves.get((round(p[0], 1), round(p[1], 1)), p) for p in street["points"]
        ]
    moved_welds = sum(1 for key in moves if key in welds)
    return moved_bands, len(moves), moved_welds


# How many times the recentring is run. One pass measures each segment
# against walls it then moves away from, so the line lands a little short of
# the middle; three more settle it, and a bake run again on its own output
# then finds nothing left to move -- which is what makes the file the bake
# writes a fixed point of the bake.
RECENTRE_PASSES = 4


def recentre_bands(streets, entries, half):
    """See `recentre_bands_once`; this runs it to convergence."""
    if not entries:
        return 0
    rectangles = [rectangle_of(e) for e in entries]
    centres = np.asarray([e["centre"] for e in entries])
    tree = shapely.STRtree(rectangles)
    welds = shared_points(streets)
    bands = points = junctions = 0
    for _ in range(RECENTRE_PASSES):
        b, p, j = recentre_bands_once(streets, entries, half, tree, rectangles, centres, welds)
        bands = max(bands, b)
        points += p
        junctions += j
        if p == 0:
            break
    if points:
        print(
            f"{bands} bands recentred between their walls ({points} point moves over "
            f"the passes, {junctions} of them junctions taken along)",
            file=sys.stderr,
        )
    return bands


def cap_widths(streets, entries, half, quantile=0.0):
    """Narrows each street to what fits between the real buildings on it.

    The runtime draws a carriageway and a pavement either side of the
    centreline and drops any building that would stand in the carriageway.
    That is right for a building that really is in the road and wrong for a
    residential street the class table calls seven and a half metres wide
    whose houses are three metres from its centreline — which is most of the
    Altstadt's lanes — and there it was dropping the street wall.

    So, per segment inside the square, the distance from the centreline to
    the nearest part on either side; per street, the tightest of those (or
    the `quantile` of them, for a bake that would rather lose the odd corner
    building than narrow a whole street for it), so a street keeps one
    width; and the width becomes what leaves `PAVEMENT` on both sides of
    that. A part within `IN_ROAD` of the centreline is left out: no width
    the game can draw would clear it, so that building is in the road (or
    over a tunnel) whatever this decides, and the runtime is the right place
    to decide about it.

    A band folded by `merge_parallel` is measured differently, because its
    centreline is not in its middle: the survivor stays on its own lane at
    every weld, so one wall is three metres off and the other twenty-five.
    What is wanted there is the band's wall-to-wall clearance, so a band is
    read as the median of left plus right over the segments that see both
    walls, and is never capped below `BAND_FLOOR` whatever that says.
    Runs after the merge for exactly that reason.
    """
    if not entries:
        return []
    rectangles = [rectangle_of(e) for e in entries]
    centres = np.asarray([e["centre"] for e in entries])
    tree = shapely.STRtree(rectangles)
    narrowed = []
    for street in streets:
        if street.get("covered"):
            continue
        band = bool(street.get("band"))
        points = street["points"]
        # How far out to look for a wall. Fixed rather than read off the
        # width being capped, or a second bake of the file looks less far
        # than the first did and caps the same street again.
        reach = BAND_REACH if band else STREET_REACH
        tights, clearances = [], []
        for a, b in zip(points, points[1:]):
            if not (inside(a, half) or inside(b, half)):
                continue
            segment = LineString([a, b])
            minx, miny, maxx, maxy = segment.bounds
            found = tree.query(box(minx - reach, miny - reach, maxx + reach, maxy + reach))
            if len(found) == 0:
                continue
            gaps = shapely.distance(segment, tree.geometries.take(found))
            dx, dy = b[0] - a[0], b[1] - a[1]
            cross = dx * (centres[found, 1] - a[1]) - dy * (centres[found, 0] - a[0])
            nearest = [float("inf"), float("inf")]
            for gap, hand in zip(gaps, cross):
                if gap < IN_ROAD or gap > reach:
                    continue
                side = 0 if hand < 0 else 1
                nearest[side] = min(nearest[side], gap)
            if band:
                if all(n < float("inf") for n in nearest):
                    clearances.append(nearest[0] + nearest[1])
            elif min(nearest) < float("inf"):
                tights.append(min(nearest))
        if band:
            if not clearances:
                continue
            width = statistics.median(clearances) - 2.0 * PAVEMENT
            width = max(width, BAND_FLOOR)
        else:
            if not tights:
                continue
            width = 2.0 * float(np.quantile(tights, quantile)) - 2.0 * PAVEMENT
        width = round(min(street["width"], max(NARROWEST, width)), 1)
        # Only a real narrowing: the gaps are measured off coordinates the
        # file keeps to a decimetre, and chasing the last two decimetres would
        # narrow every street a little more on every bake.
        if width < street["width"] - CAP_SLACK:
            narrowed.append((street["name"], street["width"], width, band))
            street["width"] = width
    return narrowed


# A band folded out of a loop is the same street twice. The Altstadt is
# mapped as one closed way round the market -- up one lane, round the top,
# down the other, out along the Theaterstrasse spur and back -- and it is the
# longest way of its name, so `merge_parallel` makes it the leader and the
# recentring then pulls both of its lanes onto the one centreline. What that
# leaves is a band that runs north, turns round and runs south over itself:
# every edge drawn twice, and at each turn-round a pavement laid straight
# across the carriageway, which is the slab that was standing in the road at
# the north end of the market. So a band is cut into its monotone runs, the
# longest run is the street, and the others are dropped where something else
# already covers them.
#
# The welds go with the dropped runs, and every side street that ended on
# one is put back: a point of another street inside the band's carriageway
# is projected onto the spine and the spine takes the foot as a junction. A
# street that stays inside for a stretch is trimmed to where it entered.
REVERSAL_LOOK = 40.0
REVERSAL_DOT = -0.5
SPINE_SNAP = 2.0
COVERED_SHARE = 0.7
COVERED_GAP = 6.0


def unit(a, b):
    dx, dy = b[0] - a[0], b[1] - a[1]
    length = math.hypot(dx, dy)
    return (dx / length, dy / length) if length > 1e-9 else (0.0, 0.0)


def runs_of(points):
    """Cuts a polyline where it turns round on itself.

    Both directions are taken over `REVERSAL_LOOK` metres of the way rather
    than from one segment: the turn-round at the top of a loop is a few short
    segments in every direction, and a street that steps aside for one point
    to meet a side street and comes back has not turned round. Metres *along*
    the way, not as the crow flies -- a loop's far lane comes back inside any
    radius, and measured that way the cut landed forty metres short of the
    turn. A reversal is where the way behind a point and the way ahead of it
    disagree by more than a hundred and twenty degrees.
    """
    runs, start = [], 0
    for k in range(1, len(points) - 1):
        back, walked = k, 0.0
        while back > start and walked < REVERSAL_LOOK:
            walked += math.dist(points[back - 1], points[back])
            back -= 1
        ahead, walked = k, 0.0
        while ahead < len(points) - 1 and walked < REVERSAL_LOOK:
            walked += math.dist(points[ahead], points[ahead + 1])
            ahead += 1
        behind_dir = unit(points[back], points[k])
        ahead_dir = unit(points[k], points[ahead])
        if behind_dir[0] * ahead_dir[0] + behind_dir[1] * ahead_dir[1] < REVERSAL_DOT:
            runs.append(points[start:k + 1])
            start = k
    runs.append(points[start:])
    return [run for run in runs if len(run) >= 2]


def unjog(points):
    """Drops any point a polyline steps back to reach. The ends stay."""
    points = list(points)
    changed = True
    while changed and len(points) > 2:
        changed = False
        for k in range(1, len(points) - 1):
            a, b, c = points[k - 1], points[k], points[k + 1]
            trend = (c[0] - a[0], c[1] - a[1])
            there = (b[0] - a[0], b[1] - a[1])
            on = (c[0] - b[0], c[1] - b[1])
            if there[0] * trend[0] + there[1] * trend[1] < 0 or on[0] * trend[0] + on[1] * trend[1] < 0:
                del points[k]
                changed = True
                break
    return points


def polyline_length(points):
    return sum(math.dist(a, b) for a, b in zip(points, points[1:]))


def covered(run, streets):
    """Is this run mostly lying in streets that are still there?

    The band itself counts, and is the usual answer: by the time a run is
    tested the band is its spine, and the loop's other lane lies on it.
    """
    hits = 0
    for at in run:
        for street in streets:
            gap, _, _ = nearest_on(street["points"], at)
            if gap < street["width"] * 0.5 + COVERED_GAP:
                hits += 1
                break
    return hits >= len(run) * COVERED_SHARE


def spine_the_bands(streets):
    """Cuts every looped band down to one run of itself."""
    spined, dropped, kept = 0, 0, 0
    for band in [s for s in streets if s.get("band")]:
        runs = runs_of(band["points"])
        spine = unjog(max(runs, key=polyline_length)) if runs else band["points"]
        if len(runs) < 2 and len(spine) == len(band["points"]):
            continue
        spined += 1
        band["points"] = spine
        band["length"] = polyline_length(spine)
        print(
            f"  {band['name']} ({band['width']} m): {len(runs)} runs of "
            f"{', '.join(f'{polyline_length(run):.0f}' for run in runs)} m; "
            f"the spine keeps {len(spine)} of {sum(len(run) for run in runs)} points",
            file=sys.stderr,
        )
        for run in runs:
            if run is max(runs, key=polyline_length):
                continue
            if covered(run, streets):
                dropped += 1
                continue
            # Nothing else runs here, so the loop was the only way this
            # street was mapped; keep it as an ordinary street. Which width
            # the lane had before the fold is gone, so it gets a plain one.
            kept += 1
            print(
                f"    kept a {polyline_length(run):.0f} m run of {band['name']} from "
                f"({run[0][0]:.1f},{run[0][1]:.1f}) to ({run[-1][0]:.1f},{run[-1][1]:.1f}) as a street",
                file=sys.stderr,
            )
            streets.append({
                "name": band["name"],
                "width": min(band["width"], 8.0),
                "arterial": band["arterial"],
                "surface": band["surface"],
                "points": unjog(run),
                "length": polyline_length(run),
                "band": False,
                "recentred": band.get("recentred", False),
                "covered": band.get("covered", False),
            })
    return spined, dropped, kept


def attach(spine, at):
    """The junction on `spine` for a point that ought to meet it there.

    An existing spine point within `SPINE_SNAP` is that junction; otherwise
    the foot of the perpendicular is, and the spine takes it as a new point
    so the two streets share a coordinate -- which is the only thing that
    welds them when the runtime loads this.
    """
    best = None
    for index, (behind, ahead) in enumerate(zip(spine, spine[1:])):
        dx, dy = ahead[0] - behind[0], ahead[1] - behind[1]
        span = dx * dx + dy * dy
        if span < 1e-9:
            continue
        t = max(0.0, min(1.0, ((at[0] - behind[0]) * dx + (at[1] - behind[1]) * dy) / span))
        foot = (behind[0] + dx * t, behind[1] + dy * t)
        gap = math.dist(at, foot)
        if best is None or gap < best[0]:
            best = (gap, index, foot)
    _, index, foot = best
    foot = (round(foot[0], 1), round(foot[1], 1))
    near = min(spine, key=lambda p: math.dist(p, foot))
    if math.dist(near, foot) <= SPINE_SNAP:
        return near
    spine.insert(index + 1, foot)
    return foot


def reattach_to_bands(streets):
    """Puts every side street back on the band it met before the fold."""
    attached, split, dropped = 0, 0, 0
    for band in [s for s in streets if s.get("band")]:
        spine = band["points"]
        # A point of another street is in the band if it is on the
        # carriageway or halfway across the pavement; an *end* of one is,
        # out to a metre behind the pavement, because a street that stops
        # there stopped at the band's kerb and was meant to meet it.
        reach = band["width"] * 0.5 + PAVEMENT * 0.5
        end_reach = band["width"] * 0.5 + PAVEMENT + 1.0
        for street in list(streets):
            if street is band:
                continue
            points = street["points"]
            # Loose: inside the carriageway and not already a spine point.
            loose = []
            for index, at in enumerate(points):
                gap, _, _ = nearest_on(spine, at)
                on_spine = any(math.dist(at, p) < 0.05 for p in spine)
                within = end_reach if index in (0, len(points) - 1) else reach
                loose.append(gap < within and not on_spine)
            if not any(loose):
                continue
            if all(loose):
                streets.remove(street)
                dropped += 1
                continue
            # Runs of loose points, as index ranges over `points`.
            runs, start = [], None
            for k, is_loose in enumerate(loose + [False]):
                if is_loose and start is None:
                    start = k
                elif not is_loose and start is not None:
                    runs.append((start, k - 1))
                    start = None
            pieces, prefix, cursor = [], [], 0
            for a, b in runs:
                if a == 0:
                    # Came in from the band: the junction is where the street
                    # leaves the carriageway, the rest of the run is in the road.
                    prefix, cursor = [attach(spine, points[b])], b + 1
                    attached += 1
                elif b == len(points) - 1:
                    # Ends in the band: the junction is where it entered.
                    pieces.append(prefix + points[cursor:a] + [attach(spine, points[a])])
                    attached += 1
                    cursor = None
                else:
                    # Crosses the band: two streets, each meeting it.
                    into = attach(spine, points[a])
                    out = attach(spine, points[b])
                    if math.dist(into, out) < SPINE_SNAP:
                        out = into
                    pieces.append(prefix + points[cursor:a] + [into])
                    prefix, cursor = [out], b + 1
                    split += 1
            if cursor is not None:
                pieces.append(prefix + points[cursor:])
            pieces = [piece for piece in pieces if len(piece) >= 2]
            if not pieces:
                streets.remove(street)
                dropped += 1
                continue
            street["points"] = pieces[0]
            street["length"] = polyline_length(pieces[0])
            for extra in pieces[1:]:
                streets.append(dict(street, points=extra, length=polyline_length(extra)))
        band["length"] = polyline_length(spine)
    return attached, split, dropped


def drop_band_duplicates(streets):
    """Cuts out of every other street the segments that *are* a band's.

    A side street mapped through two of the loop's junction points in a row
    -- the Steckengasse steps along the Altstadt for six metres between the
    two lanes' junctions, and the churchyard path has a three-point loop that
    lies entirely on it -- keeps those points after the re-attachment, since
    each is a spine point already, and the segment between them is the band's
    own edge drawn a second time. A segment whose ends are both spine points
    and whose middle is in the carriageway is that, and goes; the street is
    split round it. Consecutive duplicate points, which an attach can leave
    where the junction was the next point anyway, go with them.
    """
    cut, dropped = 0, 0
    for band in [s for s in streets if s.get("band")]:
        spine = band["points"]
        half = band["width"] * 0.5
        for street in list(streets):
            if street is band:
                continue
            points = []
            for at in street["points"]:
                if not points or math.dist(points[-1], at) > 0.05:
                    points.append(at)
            on_spine = [any(math.dist(at, p) < 0.05 for p in spine) for at in points]
            pieces, current = [], [points[0]] if points else []
            for k in range(1, len(points)):
                a, b = points[k - 1], points[k]
                middle = ((a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5)
                if on_spine[k - 1] and on_spine[k] and nearest_on(spine, middle)[0] < half:
                    cut += 1
                    pieces.append(current)
                    current = [b]
                else:
                    current.append(b)
            pieces.append(current)
            pieces = [piece for piece in pieces if len(piece) >= 2]
            if len(pieces) == 1 and pieces[0] == street["points"]:
                continue
            if not pieces:
                streets.remove(street)
                dropped += 1
                continue
            street["points"] = pieces[0]
            street["length"] = polyline_length(pieces[0])
            for extra in pieces[1:]:
                streets.append(dict(street, points=extra, length=polyline_length(extra)))
    return cut, dropped


def tidy_bands(streets):
    """See `spine_the_bands`: a looped band becomes one street, every street
    that met the loop meets the spine instead, and nothing else draws the
    band's own edges."""
    spined, dropped_runs, kept = spine_the_bands(streets)
    attached, split, dropped = reattach_to_bands(streets)
    cut, gone = drop_band_duplicates(streets)
    return spined, dropped_runs, kept, attached, split, dropped + gone, cut


def inside(point, half):
    return abs(point[0]) <= half and abs(point[1]) <= half


def project(lat, lon, lat0, lon0):
    """Equirectangular metres about (lat0, lon0), north on -Z."""
    x = math.radians(lon - lon0) * math.cos(math.radians(lat0)) * EARTH
    z = -math.radians(lat - lat0) * EARTH
    return x, z


def unproject(x, z, lat0, lon0):
    """The inverse: metres back to degrees. Vectorised, for the relief."""
    lon = lon0 + np.degrees(np.asarray(x) / (math.cos(math.radians(lat0)) * EARTH))
    lat = lat0 - np.degrees(np.asarray(z) / EARTH)
    return lat, lon


# ---------------------------------------------------------------- relief ----

# The relief is sampled onto two square grids of nodes about the origin: a
# near one every 25 m over ±1600 m, which is the town and the hills that
# shoulder it, and a far one every 100 m over ±6400 m for the horizon. Both
# are 129 nodes a side — 2 * 1600 / 25 + 1 — so a node sits exactly on the
# origin and the grid is symmetric about it; the values are metres above the
# datum to one decimal.
NEAR_STEP, NEAR_HALF = 25.0, 1600.0
FAR_STEP, FAR_HALF = 100.0, 6400.0
# The filter: a minimum over this many DEM pixels a side, then a mean over
# the same. One arc-second is thirty metres north-south, so three is ninety —
# wider than a roof or a tree crown, narrower than a hill.
FILTER = 3


class Dem:
    """One or more Copernicus tiles, filtered, sampled bilinearly."""

    def __init__(self, paths, lat0, lon0):
        import tifffile

        # The window the two grids can ask about, in degrees, with a margin
        # for the filter and the interpolation. Everything outside is never
        # touched, which keeps a tile from costing ten copies of itself.
        far_lat, far_lon = unproject([-FAR_HALF, FAR_HALF], [FAR_HALF, -FAR_HALF], lat0, lon0)
        self.tiles = []
        for path in paths:
            with tifffile.TiffFile(path) as tif:
                page = tif.pages[0]
                scale = page.tags["ModelPixelScaleTag"].value
                tie = page.tags["ModelTiepointTag"].value
                raster = page.asarray().astype(np.float32)
            sx, sy = float(scale[0]), float(scale[1])
            # Tiepoint: raster (i, j) -> (lon, lat), pixel-is-point.
            lon_at, lat_at = float(tie[3]), float(tie[4])
            rows, cols = raster.shape
            col0 = max(0, int((far_lon.min() - lon_at) / sx) - FILTER - 1)
            col1 = min(cols, int((far_lon.max() - lon_at) / sx) + FILTER + 2)
            row0 = max(0, int((lat_at - far_lat.max()) / sy) - FILTER - 1)
            row1 = min(rows, int((lat_at - far_lat.min()) / sy) + FILTER + 2)
            if col1 <= col0 or row1 <= row0:
                continue
            crop = raster[row0:row1, col0:col1]
            crop = mean_filter(min_filter(crop))
            self.tiles.append({
                "values": crop,
                "lon0": lon_at + col0 * sx,
                "lat0": lat_at - row0 * sy,
                "sx": sx,
                "sy": sy,
                "path": path,
            })
        if not self.tiles:
            sys.exit("no DEM tile covers the town")

    def sample(self, lat, lon):
        """Bilinear, in metres above sea level. NaN where no tile has it."""
        lat, lon = np.asarray(lat, dtype=np.float64), np.asarray(lon, dtype=np.float64)
        out = np.full(lat.shape, np.nan, dtype=np.float64)
        for tile in self.tiles:
            values = tile["values"]
            rows, cols = values.shape
            fc = (lon - tile["lon0"]) / tile["sx"]
            fr = (tile["lat0"] - lat) / tile["sy"]
            ok = (fc >= 0) & (fc <= cols - 1) & (fr >= 0) & (fr <= rows - 1) & np.isnan(out)
            if not ok.any():
                continue
            c0 = np.clip(np.floor(fc[ok]).astype(int), 0, cols - 2)
            r0 = np.clip(np.floor(fr[ok]).astype(int), 0, rows - 2)
            tc, tr = fc[ok] - c0, fr[ok] - r0
            out[ok] = (
                values[r0, c0] * (1 - tc) * (1 - tr)
                + values[r0, c0 + 1] * tc * (1 - tr)
                + values[r0 + 1, c0] * (1 - tc) * tr
                + values[r0 + 1, c0 + 1] * tc * tr
            )
        return out


def shifted(values):
    """The nine one-pixel shifts of an array, edges repeated."""
    padded = np.pad(values, 1, mode="edge")
    rows, cols = values.shape
    for dr in range(FILTER):
        for dc in range(FILTER):
            yield padded[dr:dr + rows, dc:dc + cols]


def min_filter(values):
    out = None
    for view in shifted(values):
        out = view.copy() if out is None else np.minimum(out, view)
    return out


def mean_filter(values):
    out = np.zeros_like(values)
    for view in shifted(values):
        out += view
    return out / (FILTER * FILTER)


def relief_grid(dem, step, half, lat0, lon0):
    """A square grid of node samples, node (0, 0) at the origin corner and
    row 0 northernmost (z increasing by row), as (origin, cols, rows,
    values-in-metres-above-sea-level)."""
    count = int(round(2 * half / step)) + 1
    axis = -half + np.arange(count) * step
    xs, zs = np.meshgrid(axis, axis)  # zs varies by row
    lat, lon = unproject(xs.ravel(), zs.ravel(), lat0, lon0)
    values = dem.sample(lat, lon).reshape(count, count)
    return (-half, -half), count, count, values


def bake_relief(dem, streets, half, lat0, lon0):
    """The datum and the two grids. See the module docstring for why the
    datum is the median under the streets."""
    samples = []
    for street in streets:
        for a, b in zip(street["points"], street["points"][1:]):
            run = math.dist(a, b)
            steps = max(1, math.ceil(run / 10.0))
            for k in range(steps + 1):
                t = k / steps
                at = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
                if inside(at, half):
                    samples.append(at)
    if not samples:
        sys.exit("no street inside the square to take a datum from")
    lat, lon = unproject([x for x, _ in samples], [z for _, z in samples], lat0, lon0)
    under = dem.sample(lat, lon)
    # To the decimetre before anything is measured from it, so the file
    # carries the same datum it says it does and a re-bake subtracts the
    # same number.
    datum = round(float(np.nanmedian(under)), 1)

    near = relief_grid(dem, NEAR_STEP, NEAR_HALF, lat0, lon0)
    far = relief_grid(dem, FAR_STEP, FAR_HALF, lat0, lon0)
    grids = {}
    for label, (origin, cols, rows, values) in (("near", near), ("far", far)):
        if np.isnan(values).any():
            sys.exit(f"the {label} relief grid runs off the DEM tiles given")
        grids[label] = {
            "origin": origin,
            "step": NEAR_STEP if label == "near" else FAR_STEP,
            "cols": cols,
            "rows": rows,
            # `+ 0.0` turns a rounded -0.0 into the 0.0 it means.
            "values": np.round(values - datum, 1) + 0.0,
        }
    return {"datum": round(datum, 1), **grids}


def relief_at(relief, points):
    """Metres above datum at world points, bilinear off the near grid, the
    way the runtime will read it. For the report only."""
    grid = relief["near"]
    values, step = grid["values"], grid["step"]
    ox, oz = grid["origin"]
    out = []
    for x, z in points:
        fc = (x - ox) / step
        fr = (z - oz) / step
        c0 = int(min(max(math.floor(fc), 0), grid["cols"] - 2))
        r0 = int(min(max(math.floor(fr), 0), grid["rows"] - 2))
        tc, tr = min(max(fc - c0, 0.0), 1.0), min(max(fr - r0, 0.0), 1.0)
        out.append(
            values[r0, c0] * (1 - tc) * (1 - tr) + values[r0, c0 + 1] * tc * (1 - tr)
            + values[r0 + 1, c0] * (1 - tc) * tr + values[r0 + 1, c0 + 1] * tc * tr
        )
    return out


# ------------------------------------------------------------ the file ----

POINT = re.compile(r"\(([-\d.]+),([-\d.]+)\)")
STREET = re.compile(
    r'^\(name: "(?P<name>[^"]*)", width: (?P<width>[-\d.]+), '
    r"arterial: (?P<arterial>true|false), surface: (?P<surface>\w+), "
    r"points: \[(?P<points>.*)\](?:, band: (?P<band>true|false))?\),?$"
)
WATER = re.compile(
    r'^\(name: "(?P<name>[^"]*)", width: (?P<width>[-\d.]+), points: \[(?P<points>.*)\]\),?$'
)
GROUND = re.compile(r"^\(kind: (?P<kind>\w+), points: \[(?P<points>.*)\]\),?$")


def read_ron(path):
    """Reads back the streets, water and ground of a file this bake wrote.

    Not a RON parser: a reader for the one dialect `write_ron` produces, one
    entry per line, which is enough to regenerate a town's buildings and
    relief without asking Overpass for its streets again. Exists because the
    machine this was first run on could reach Amazon's buckets and not
    Overpass, and a bake that could not be rerun would have been a bake that
    was never checked.
    """
    text = open(path, encoding="utf-8").read()
    name = re.search(r'^\s*name: "([^"]*)",$', text, re.M)
    centre = re.search(r"^\s*centre: \(([-\d.]+), ([-\d.]+)\),$", text, re.M)
    if not name or not centre:
        sys.exit(f"{path} does not look like a baked atlas")
    sections = {}
    current = None
    for line in text.splitlines():
        head = re.match(r"^    (\w+): [\[(]$", line)
        if head:
            current = head.group(1)
            sections[current] = []
            continue
        if re.match(r"^    [\])],$", line):
            current = None
            continue
        if current is not None:
            sections[current].append(line.strip())

    def points_of(raw):
        return [(float(x), float(z)) for x, z in POINT.findall(raw)]

    streets = []
    for line in sections.get("streets", []):
        m = STREET.match(line)
        if not m:
            sys.exit(f"cannot read a street back: {line[:80]}")
        points = points_of(m["points"])
        width = float(m["width"])
        streets.append({
            "name": m["name"],
            "width": width,
            "arterial": m["arterial"] == "true",
            "surface": m["surface"],
            "points": points,
            "length": sum(math.dist(a, b) for a, b in zip(points, points[1:])),
            # The merge already happened when this was written. A file from
            # after the flag was added says which ways it folded; one from
            # before is read the only way it can be, by width -- which a
            # capped band no longer passes, so that file must not be capped
            # twice.
            "band": (m["band"] == "true") if m["band"] else width > WIDEST,
            # A file that says which ways it folded was written by a bake
            # that had already put its bands between their walls.
            "recentred": m["band"] is not None,
            "covered": "tunnel" in m["name"].lower(),
        })
    waters = []
    for line in sections.get("waters", []):
        m = WATER.match(line)
        if not m:
            sys.exit(f"cannot read a waterway back: {line[:80]}")
        waters.append({"name": m["name"], "width": float(m["width"]), "points": points_of(m["points"])})
    grounds = []
    for line in sections.get("grounds", []):
        m = GROUND.match(line)
        if not m:
            sys.exit(f"cannot read an open area back: {line[:80]}")
        grounds.append({"kind": m["kind"], "points": points_of(m["points"])})
    return {
        "name": name.group(1),
        "centre": (float(centre.group(1)), float(centre.group(2))),
        "streets": streets,
        "waters": waters,
        "grounds": grounds,
    }


def ron_points(points):
    return "[" + ",".join(f"({x:.1f},{z:.1f})" for x, z in points) + "]"


def ron_option(value, fmt="{}"):
    return "None" if value is None else "Some(" + fmt.format(value) + ")"


def write_ron(path, name, centre, streets, buildings, waters, grounds, relief, provenance):
    """Writes the atlas — to a sibling file first, then into place, so a bake
    that dies halfway leaves the old town rather than half of a new one."""
    lat0, lon0 = centre
    temp = path + ".part"
    with open(temp, "w", encoding="utf-8") as out:
        out.write("// Baked by tools/bake-city.py from an OpenStreetMap extract.\n")
        out.write("// Map data (c) OpenStreetMap contributors, ODbL 1.0.\n")
        if provenance.get("overture_release"):
            out.write(
                f"// Buildings from Overture Maps release {provenance['overture_release']} "
                f"(theme {provenance.get('overture_theme', 'buildings')}): "
                "(c) OpenStreetMap contributors and (c) Microsoft, ODbL 1.0.\n"
            )
        if relief is not None:
            out.write(
                "// Relief from Copernicus DEM GLO-30 (c) DLR e.V. 2010-2014 and "
                "(c) Airbus Defence and Space GmbH 2014-2018 provided under COPERNICUS\n"
                "// by the European Union and ESA; all rights reserved.\n"
            )
        out.write("// Do not edit by hand: rerun the fetch and the bake.\n")
        out.write("(\n")
        out.write(f'    name: "{name}",\n')
        out.write(f"    centre: ({lat0}, {lon0}),\n")

        out.write("    streets: [\n")
        for street in streets:
            label = street["name"].replace('"', "'")
            out.write(
                f'        (name: "{label}", width: {street["width"]}, '
                f'arterial: {str(street["arterial"]).lower()}, '
                f'surface: {street["surface"]}, '
                f'points: {ron_points(street["points"])}'
                + (", band: true" if street.get("band") else "")
                + "),\n"
            )
        out.write("    ],\n")

        out.write("    buildings: [\n")
        for b in buildings:
            label = b["name"].replace('"', "'")
            out.write(
                f'        (name: "{label}", '
                f"centre: ({b['centre'][0]:.1f},{b['centre'][1]:.1f}), "
                f"yaw: {b['yaw']:.4f}, frontage: {b['frontage']:.1f}, "
                f"depth: {b['depth']:.1f}, height: {ron_option(b['height'])}, "
                f"kind: {ron_option(b['kind'])}, group: Some({b['group']}), "
                f"area: {b['area']:.1f}, roof: {ron_option(b['roof'])}, "
                f"levels: {ron_option(b['levels'])}),\n"
            )
        out.write("    ],\n")

        out.write("    waters: [\n")
        for w in waters:
            label = w["name"].replace('"', "'")
            out.write(
                f'        (name: "{label}", width: {w["width"]:.1f}, '
                f"points: {ron_points(w['points'])}),\n"
            )
        out.write("    ],\n")

        out.write("    grounds: [\n")
        for g in grounds:
            out.write(
                f"        (kind: {g['kind']}, points: {ron_points(g['points'])}),\n"
            )
        out.write("    ],\n")

        if relief is not None:
            # Metres above `datum` at grid nodes `step` metres apart, row
            # major, one row per line, row 0 northernmost — so the value at
            # (col, row) is the height at world origin + (col, row) * step,
            # and `origin` is where node (0, 0) is. `Some` because the Rust
            # field is an `Option`: an atlas baked without a DEM has none.
            out.write("    relief: Some((\n")
            out.write(f"        datum: {relief['datum']},\n")
            for label in ("near", "far"):
                grid = relief[label]
                out.write(
                    f"        {label}: (origin: ({grid['origin'][0]:.1f}, {grid['origin'][1]:.1f}), "
                    f"step: {grid['step']}, cols: {grid['cols']}, rows: {grid['rows']}, values: [\n"
                )
                for row in grid["values"]:
                    out.write("            " + ",".join(f"{v:.1f}" for v in row) + ",\n")
                out.write("        ]),\n")
            out.write("    )),\n")
        out.write(")\n")
    os.replace(temp, path)


# ------------------------------------------------------------------ main ----


def main():
    ap = argparse.ArgumentParser(description="Bake a town's map data into an atlas the game can build.")
    ap.add_argument("source", help="an Overpass roads dump, or with --from-ron a baked atlas")
    ap.add_argument("out")
    ap.add_argument("--centre", nargs=2, type=float, metavar=("LAT", "LON"),
                    help="required unless --from-ron, whose file knows")
    ap.add_argument("--name", help="required unless --from-ron, whose file knows")
    ap.add_argument("--extras", action="append", default=[],
                    help="further Overpass dumps: buildings, water, landuse. "
                    "Repeatable, because asked in one request Overpass times "
                    "out. Optional -- an atlas without any is the street plan "
                    "the game built before any of this existed.")
    ap.add_argument("--buildings-parquet",
                    help="Overture buildings from tools/fetch-overture.py. Preferred over "
                    "the buildings in --extras, which are then ignored.")
    ap.add_argument("--dem", action="append", default=[],
                    help="a Copernicus GLO-30 tile from tools/fetch-overture.py dem; "
                    "repeatable where the town straddles a degree line")
    ap.add_argument("--from-ron", action="store_true",
                    help="SOURCE is an atlas this bake wrote: its streets, water and "
                    "open ground are kept and only the buildings and the relief are "
                    "regenerated. For a machine that can reach the buckets but not "
                    "Overpass; a full fetch is the normal path.")
    # Anything whose whole polyline falls outside this half-extent is dropped,
    # so the baked city matches the square the game builds.
    ap.add_argument("--half-extent", type=float, default=1000.0)
    ap.add_argument("--cap-quantile", type=float, default=0.0,
                    help="which of a street's per-segment gaps its width is capped to: "
                    "0 (the default) is the tightest, 0.25 lets the odd corner "
                    "building stand in the road rather than narrow the whole street")
    ap.add_argument("--report", action="store_true",
                    help="also count what stands on the hill")
    args = ap.parse_args()
    half = args.half_extent

    # ------------------------------------------------------------ streets --
    waters, grounds = [], []
    extras = []
    if args.from_ron:
        old = read_ron(args.source)
        name = args.name or old["name"]
        lat0, lon0 = old["centre"]
        if args.centre and (abs(args.centre[0] - lat0) > 1e-6 or abs(args.centre[1] - lon0) > 1e-6):
            sys.exit(f"--centre {args.centre} is not the centre {old['centre']} the atlas was baked about")
        streets, waters, grounds = old["streets"], old["waters"], old["grounds"]
        dropped = 0
        print(
            f"reusing {len(streets)} streets, {len(waters)} waterways and {len(grounds)} "
            f"open areas from {args.source}",
            file=sys.stderr,
        )
        # An --extras dump here can only be buildings: the water and the
        # ground are already in the file.
        for path in args.extras:
            extras += [e for e in json.load(open(path, encoding="utf-8")).get("elements", [])
                       if "building" in (e.get("tags") or {})]
    else:
        if not args.centre or not args.name:
            sys.exit("--centre and --name are required unless --from-ron")
        lat0, lon0 = args.centre
        name = args.name
        data = json.load(open(args.source, encoding="utf-8"))
        streets, dropped = streets_from_overpass(data, lat0, lon0, half)
        for path in args.extras:
            extras += json.load(open(path, encoding="utf-8")).get("elements", [])

    # ---------------------------------------------------- water and ground --
    for element in extras:
        tags = element.get("tags", {}) or {}
        waterway = tags.get("waterway")
        if waterway in ("river", "stream", "canal"):
            geometry = element.get("geometry") or []
            metres = [project(n["lat"], n["lon"], lat0, lon0) for n in geometry]
            if len(metres) < 2:
                continue
            if not any(inside(p, half) for p in metres):
                continue
            waters.append({
                "name": tags.get("name", ""),
                "width": river_width(tags, waterway),
                "points": smooth(densify(metres)),
            })
            continue
        ground = tags.get("landuse") or tags.get("leisure")
        if ground in OPEN_GROUND:
            for ring in outer_rings(element):
                metres = [project(lat, lon, lat0, lon0) for lat, lon in ring]
                if len(metres) < 4:
                    continue
                if not any(inside(p, half) for p in metres):
                    continue
                if area_of(metres) < 400.0:
                    continue
                grounds.append({
                    "kind": OPEN_GROUND[ground],
                    "points": thin_ring(metres),
                })

    # ---------------------------------------------------------- buildings --
    provenance = {}
    if args.buildings_parquet:
        candidates, provenance = candidates_from_overture(args.buildings_parquet, lat0, lon0)
        print(
            f"{len(candidates)} Overture buildings read (release "
            f"{provenance.get('overture_release', 'unknown')}; "
            f"{provenance.get('ml_footprints', 0)} traced by Microsoft, tagless)",
            file=sys.stderr,
        )
    else:
        candidates = candidates_from_overpass(extras, lat0, lon0)
        if candidates:
            print(f"{len(candidates)} Overpass buildings read", file=sys.stderr)
    buildings, built = bake_buildings(candidates, half)
    if candidates:
        c = built["counts"]
        print(
            f"  {c['kept']} buildings kept ({c['landmarks']} of them landmarks) as "
            f"{len(buildings)} parts; dropped {c['small']} too small, {c['big']} too big, "
            f"{c['outside']} outside the square, {c['degenerate']} degenerate",
            file=sys.stderr,
        )
        print(
            "  parts per building: "
            + ", ".join(f"{n}x{k}" for k, n in sorted(built["parts"].items()))
            + f"; {c['grid']} decomposed, {c['box']} kept as their box, "
            f"{c['split']} parts cut for length",
            file=sys.stderr,
        )
        if built["iou"]:
            print(
                f"  IoU vs the polygons: mean {statistics.mean(built['iou']):.3f}, "
                f"p10 {np.percentile(built['iou'], 10):.3f} "
                f"(one box: mean {statistics.mean(built['box_iou']):.3f}, "
                f"p10 {np.percentile(built['box_iou'], 10):.3f}); "
                f"{100 * built['coverage']:.1f}% of the mapped footprint covered",
                file=sys.stderr,
            )
    if extras or args.from_ron:
        print(f"  {len(waters)} waterways, {len(grounds)} open areas", file=sys.stderr)

    # One street, not the four ways the mappers drew it as. See `merge_parallel`.
    # Already done when the file was written, in --from-ron mode.
    if not args.from_ron:
        before = len(streets)
        streets, folded = merge_parallel(streets)
        print(
            f"{folded} parallel ways folded into the street they belong to "
            f"({before} -> {len(streets)})",
            file=sys.stderr,
        )

    # A band sits between its walls, and then no street is wider than the
    # buildings allow. After the merge: the band is the thing being measured.
    #
    # Not twice. A file this bake wrote has its bands where the walls put
    # them, and measuring them again against the same walls moves them by
    # the decimetres the measurement is uncertain by -- and the cap, which
    # reads the moved line, then narrows a different street. Once is the
    # fixed point; a second bake of the file reproduces it.
    if not any(street.get("recentred") for street in streets):
        recentre_bands(streets, buildings, half)
    # A band that was a loop is one street from here on. Before the cap: the
    # cap measures each street against the walls along it, and a side street
    # cut back to the spine is measured along a different run of wall than the
    # one that reached into the loop -- so capping first and tidying second
    # gave a second bake of the file two narrower Kirchgassen than the first.
    # Tidied first, both bakes measure the same streets, and a second bake of
    # the file finds no loop and nothing loose.
    spined, dropped_runs, kept, attached, split, dropped, cut = tidy_bands(streets)
    if spined:
        print(
            f"{spined} looped bands cut to their spine ({dropped_runs} duplicate runs dropped, "
            f"{kept} kept as streets); {attached} side streets put back on a band, "
            f"{split} crossings split, {dropped} streets inside a band dropped, "
            f"{cut} segments along a band cut out",
            file=sys.stderr,
        )
    elif attached or split or dropped or cut:
        print(
            f"{attached} side streets put back on a band, {split} crossings split, "
            f"{dropped} streets inside a band dropped, {cut} segments along a band cut out",
            file=sys.stderr,
        )
    narrowed = cap_widths(streets, buildings, half, args.cap_quantile)
    if buildings:
        taken = sorted(was - now for _, was, now, _ in narrowed)
        median = taken[len(taken) // 2] if taken else 0.0
        print(
            f"{len(narrowed)} of {len(streets)} streets narrowed to fit between their buildings "
            f"(median {median:.1f} m taken off; {sum(1 for _, _, now, _ in narrowed if now <= NARROWEST)} "
            f"down to the {NARROWEST} m floor)",
            file=sys.stderr,
        )
        bands = [(n, was, now) for n, was, now, band in narrowed if band]
        if bands:
            print("  bands: " + ", ".join(f"{n} {was}->{now}" for n, was, now in bands), file=sys.stderr)
        for label in ("Altstadt", "Neustadt"):
            widths = [s["width"] for s in streets if s["name"] == label]
            if widths:
                print(f"  {label} now {', '.join(str(w) for w in widths)} m", file=sys.stderr)

    # ------------------------------------------------------------- relief --
    relief = None
    if args.dem:
        dem = Dem(args.dem, lat0, lon0)
        relief = bake_relief(dem, streets, half, lat0, lon0)
        near, far = relief["near"]["values"], relief["far"]["values"]
        town = np.abs(-NEAR_HALF + np.arange(relief["near"]["cols"]) * NEAR_STEP) <= half
        square = near[np.ix_(town, town)]
        print(
            f"relief: datum {relief['datum']} m above sea level; near {near.min():.1f}..{near.max():.1f} m, "
            f"far {far.min():.1f}..{far.max():.1f} m; "
            f"{100 * (square > HILL).mean():.1f}% of the square above +{HILL:.0f} m",
            file=sys.stderr,
        )
        if args.report:
            points = [p for s in streets for p in s["points"] if inside(p, half)]
            high_points = sum(1 for h in relief_at(relief, points) if h > HILL)
            groups = {}
            for b in buildings:
                groups.setdefault(b["group"], b["centre"])
            centres = [c for c in groups.values() if inside(c, half)]
            high_buildings = sum(1 for h in relief_at(relief, centres) if h > HILL)
            print(
                f"  on the hill: {high_points} of {len(points)} street points and "
                f"{high_buildings} of {len(centres)} buildings stand above +{HILL:.0f} m",
                file=sys.stderr,
            )

    # --------------------------------------------------------------- write --
    write_ron(args.out, name, (lat0, lon0), streets, buildings, waters, grounds, relief, provenance)

    total = sum(len(s["points"]) for s in streets)
    surfaces = {}
    for street in streets:
        surfaces[street["surface"]] = surfaces.get(street["surface"], 0) + 1
    widths = sorted(s["width"] for s in streets)
    print(
        f"{len(streets)} streets, {total} points "
        f"({dropped} ways outside the square) -> {args.out} "
        f"({os.path.getsize(args.out) / 1024:.0f} KiB)",
        file=sys.stderr,
    )
    print(
        f"  widths {widths[0]}..{widths[-1]}m, median {widths[len(widths) // 2]}m; "
        + ", ".join(f"{n} {surface}" for surface, n in sorted(surfaces.items())),
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
