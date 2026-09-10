#!/usr/bin/env python3
"""Bake an Overpass road extract into a city the game can build.

Usage:
    tools/fetch-landshut.sh                # downloads the extract
    tools/bake-city.py <in.json> <out.ron> --centre LAT LON --name "Landshut"

What comes out is a `world::atlas::Atlas` in RON: a list of streets, each a
polyline in *metres* about the extract's own centre, with a width and a class.

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

The data this reads is © OpenStreetMap contributors, ODbL 1.0. Anything baked
out of it carries the same licence — see CREDITS.md.
"""

import argparse
import json
import math
import sys

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


# ---------------------------------------------------------------- extras ----
#
# Everything that stands beside a street: the buildings, the water, and the
# ground that is not built on. All three are areas rather than lines, and all
# three arrive as closed ways or as multipolygon relations.

# How a building's OSM tags become one of the game's building kinds. Only the
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
# landmark is exempt from the size limits below, because those exist to stop a
# retail shed being drawn as a box the length of a street and they were throwing
# away exactly the buildings the town is known for -- St. Martin is ninety-one
# metres long against a seventy-eight metre limit. And a landmark's height is
# believed as it stands rather than clamped into the city style's band, because
# an OSM `height` on an ordinary house is as often the ridge as the eaves and as
# often a typo as either. St. Martin really is a hundred and thirty metres; the
# listed house next to it is not, and a hundred-metre terrace mapped as one
# polygon is neither.
def is_named(tags):
    if not tags.get("name"):
        return False
    if tags.get("man_made") in ("tower", "water_tower", "observatory"):
        return True
    if tags.get("historic") in ("church", "castle", "city_gate", "tower", "monastery", "chapel"):
        return True
    if tags.get("building") in ("church", "cathedral", "chapel", "castle", "temple"):
        return True
    if tags.get("tourism") in ("attraction", "museum"):
        return True
    return bool(tags.get("wikidata"))


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
# And anything bigger than this is a shopping centre mapped as one polygon, or a
# multipolygon whose outer ring went round the whole block. The game has no mesh
# for either.
BIGGEST = 9000.0

# Nor either dimension past this. The area limit alone lets through a 136 m by
# 49 m slab -- a retail shed, mapped honestly -- and the game draws a building
# as one extruded box with a pitched roof, so what that becomes is a grey wall
# the length of a street. Dropped rather than clamped: a clamped box is a lie
# about where the walls are, and an invented terrace in its place is at least
# the right *kind* of wrong.
LONGEST = 78.0
DEEPEST = 46.0

# A landmark is allowed past those, but not past these. The exemption exists for
# St. Martin at 91 m long and the Stadtresidenz at 59 deep; what it must not let
# through is the two buildings that qualify as landmarks only because somebody
# gave them a Wikidata item -- a shopping centre at 114 by 98 and an ice rink at
# 92 by 90. Those are exactly the shed the limits were written for.
NAMED_LONGEST = 120.0
NAMED_DEEPEST = 66.0

# How much of a rectangle a footprint has to be before the rectangle is a fair
# stand-in for it. An L-shaped block fills about two thirds of its own bounding
# box; below this the box is mostly courtyard.
SQUARENESS = 0.52


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
    raw = tags.get("building:levels")
    if raw:
        try:
            levels = float(str(raw).split(";")[0].replace(",", "."))
            if 0.5 <= levels <= 45.0:
                # Plus the ground floor's extra height and a roof.
                return round(levels * STOREY + 1.6, 1)
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


def project(lat, lon, lat0, lon0):
    """Equirectangular metres about (lat0, lon0), north on -Z."""
    x = math.radians(lon - lon0) * math.cos(math.radians(lat0)) * EARTH
    z = -math.radians(lat - lat0) * EARTH
    return x, z


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("source")
    ap.add_argument("out")
    ap.add_argument("--centre", nargs=2, type=float, required=True,
                    metavar=("LAT", "LON"))
    ap.add_argument("--name", required=True)
    ap.add_argument("--extras", action="append", default=[],
                    help="further Overpass dumps: buildings, water, landuse. "
                    "Repeatable, because asked in one request Overpass times "
                    "out. Optional -- an atlas without any is the street plan "
                    "the game built before any of this existed.")
    # Anything whose whole polyline falls outside this half-extent is dropped,
    # so the baked city matches the square the game builds.
    ap.add_argument("--half-extent", type=float, default=1000.0)
    args = ap.parse_args()

    lat0, lon0 = args.centre
    data = json.load(open(args.source, encoding="utf-8"))

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
        half = args.half_extent
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

        streets.append({
            "name": tags.get("name", ""),
            "width": width,
            "arterial": kind in ARTERIAL,
            "surface": surface,
            "points": [(x, z) for x, z, _ in thinned],
        })

    # ---------------------------------------------------------- extras ----
    buildings, waters, grounds = [], [], []
    if args.extras:
        elements = []
        for path in args.extras:
            elements += json.load(open(path, encoding="utf-8")).get("elements", [])
        extras = {"elements": elements}
        half = args.half_extent
        kept = dropped_small = dropped_big = dropped_ragged = landmarks = 0
        for element in extras.get("elements", []):
            tags = element.get("tags", {}) or {}

            if "building" in tags:
                named = is_named(tags)
                for ring in outer_rings(element):
                    metres = [project(lat, lon, lat0, lon0) for lat, lon in ring]
                    if not any(abs(x) <= half and abs(z) <= half for x, z in metres):
                        continue
                    footprint = area_of(metres)
                    if footprint < SMALLEST and not named:
                        dropped_small += 1
                        continue
                    if footprint > BIGGEST * (4.0 if named else 1.0):
                        dropped_big += 1
                        continue
                    box = smallest_box(metres)
                    if box is None:
                        continue
                    (cx, cz), yaw, wide, deep = box
                    longest, deepest = (NAMED_LONGEST, NAMED_DEEPEST) if named else (LONGEST, DEEPEST)
                    if wide > longest or deep > deepest:
                        dropped_big += 1
                        continue
                    # How much of its own box the building actually fills. A
                    # courtyard block is a ring, and a rectangle stamped over one
                    # is a solid lump where a courtyard should be. A landmark is
                    # exempt: a church *is* a ring of buttresses round a nave,
                    # and it is not drawn as a box anyway.
                    if not named and footprint < wide * deep * SQUARENESS:
                        dropped_ragged += 1
                        continue
                    name = tags.get("name", "") if named else ""
                    height = LANDMARK_HEIGHT.get(name) or storeys(tags)
                    kind = kind_of(tags)
                    buildings.append({
                        "name": name,
                        "centre": (cx, cz),
                        "yaw": yaw,
                        "frontage": wide,
                        "depth": deep,
                        "height": height,
                        "kind": kind,
                    })
                    kept += 1
                    if named:
                        landmarks += 1
                continue

            waterway = tags.get("waterway")
            if waterway in ("river", "stream", "canal"):
                geometry = element.get("geometry") or []
                metres = [project(n["lat"], n["lon"], lat0, lon0) for n in geometry]
                if len(metres) < 2:
                    continue
                if not any(abs(x) <= half and abs(z) <= half for x, z in metres):
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
                    if not any(abs(x) <= half and abs(z) <= half for x, z in metres):
                        continue
                    if area_of(metres) < 400.0:
                        continue
                    grounds.append({
                        "kind": OPEN_GROUND[ground],
                        "points": thin_ring(metres),
                    })
                continue

        print(
            f"  {kept} buildings ({landmarks} of them landmarks; "
            f"{dropped_small} too small, {dropped_big} too "
            f"big, {dropped_ragged} too ragged for a rectangle), "
            f"{len(waters)} waterways, {len(grounds)} open areas",
            file=sys.stderr,
        )

    def ron_points(points):
        return "[" + ",".join(f"({x:.1f},{z:.1f})" for x, z in points) + "]"

    with open(args.out, "w", encoding="utf-8") as out:
        out.write("// Baked by tools/bake-city.py from an OpenStreetMap extract.\n")
        out.write("// Map data (c) OpenStreetMap contributors, ODbL 1.0.\n")
        out.write("// Do not edit by hand: rerun the fetch and the bake.\n")
        out.write("(\n")
        out.write(f'    name: "{args.name}",\n')
        out.write(f"    centre: ({lat0}, {lon0}),\n")
        out.write("    streets: [\n")
        for street in streets:
            name = street["name"].replace('"', "'")
            out.write(
                f'        (name: "{name}", width: {street["width"]}, '
                f'arterial: {str(street["arterial"]).lower()}, '
                f'surface: {street["surface"]}, '
                f'points: {ron_points(street["points"])}),\n'
            )
        out.write("    ],\n")

        out.write("    buildings: [\n")
        for b in buildings:
            height = f"Some({b['height']})" if b["height"] is not None else "None"
            kind = f"Some({b['kind']})" if b["kind"] else "None"
            name = b["name"].replace('"', "'")
            out.write(
                f'        (name: "{name}", '
                f"centre: ({b['centre'][0]:.1f},{b['centre'][1]:.1f}), "
                f"yaw: {b['yaw']:.4f}, frontage: {b['frontage']:.1f}, "
                f"depth: {b['depth']:.1f}, height: {height}, kind: {kind}),\n"
            )
        out.write("    ],\n")

        out.write("    waters: [\n")
        for w in waters:
            name = w["name"].replace('"', "'")
            out.write(
                f'        (name: "{name}", width: {w["width"]:.1f}, '
                f"points: {ron_points(w['points'])}),\n"
            )
        out.write("    ],\n")

        out.write("    grounds: [\n")
        for g in grounds:
            out.write(
                f"        (kind: {g['kind']}, points: {ron_points(g['points'])}),\n"
            )
        out.write("    ],\n")
        out.write(")\n")

    total = sum(len(s["points"]) for s in streets)
    surfaces = {}
    for street in streets:
        surfaces[street["surface"]] = surfaces.get(street["surface"], 0) + 1
    widths = sorted(s["width"] for s in streets)
    print(
        f"{len(streets)} streets, {total} points "
        f"({dropped} ways outside the square) -> {args.out}",
        file=sys.stderr,
    )
    print(
        f"  widths {widths[0]}..{widths[-1]}m, median {widths[len(widths) // 2]}m; "
        + ", ".join(f"{count} {name}" for name, count in sorted(surfaces.items())),
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
