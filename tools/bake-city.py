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
difference between a street plan and *this* street plan.

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
        width = WIDTH.get(kind)
        if width is None:
            continue

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
            "points": [(x, z) for x, z, _ in thinned],
        })

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
                f'points: {ron_points(street["points"])}),\n'
            )
        out.write("    ],\n")
        out.write(")\n")

    total = sum(len(s["points"]) for s in streets)
    print(
        f"{len(streets)} streets, {total} points "
        f"({dropped} ways outside the square) -> {args.out}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
