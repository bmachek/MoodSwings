#!/usr/bin/env bash
# Fetch a town's street network from OpenStreetMap and bake it into a city the
# game can build.
#
#   tools/fetch-city.sh landshut
#
# The result lands in assets/cities/<name>.ron and is committed, so a fresh
# clone builds the real Landshut without touching the network. This script
# exists to make that file *reproducible* rather than mysterious: rerun it and
# you get the same city back, a year newer.
#
# ## Licence
#
# The data is © OpenStreetMap contributors and licensed ODbL 1.0. What comes
# out of the bake is a derived database and carries the same licence — which is
# why the attribution is baked into the file, printed in the loading log, and
# written down in CREDITS.md. This is the only asset in the repository that is
# not CC0, and it is the one asset that could not be.
#
# ## Being a good neighbour
#
# Overpass is a free service run on donated hardware. This asks for one bounding
# box of one small town in three requests and the answer is a few megabytes. Do
# not put it in a loop.
#
# Three rather than one because one is too much for it: the streets, the
# buildings and the water/landuse asked together returned a 504 every time, and
# the buildings alone are three megabytes of the four.
set -euo pipefail

cd "$(dirname "$0")/.."

town="${1:-landshut}"

case "$town" in
    landshut)
        # The Altstadt and a ring of the town around it: about 2.3km east to
        # west, which is the square the game builds.
        south=48.5270; west=12.1380; north=48.5480; east=12.1680
        lat=48.5375; lon=12.1508
        label="Landshut"
        ;;
    *)
        echo "unknown town: $town" >&2
        echo "add its bounding box and centre to $0" >&2
        exit 1
        ;;
esac

raw="$(mktemp)"
built="$(mktemp)"
ground="$(mktemp)"

roads="[out:json][timeout:180];
(
  way[\"highway\"~\"^(motorway|motorway_link|trunk|trunk_link|primary|primary_link|secondary|secondary_link|tertiary|tertiary_link|unclassified|residential|living_street|pedestrian)\$\"]($south,$west,$north,$east);
);
out geom;"

# Everything that stands beside a street, in one request rather than three.
#
# Buildings carry their own geometry as a closed way; the bake reduces each to
# the smallest rotated rectangle that contains it, which is the shape the game's
# `Building` already is -- a frontage by a depth at a yaw -- so a real footprint
# costs no new geometry in the renderer.
#
# The water is the point of the second half. Landshut is a town on the Isar and
# the game had no river at all: the centrelines are `waterway=river` (Isar,
# Große Isar, Kleine Isar) and the surface is one `natural=water` multipolygon
# whose inner rings are the islands.
# Kept for a bake without Overture (`--extras "$built"` still works and goes
# through the same polygon pipeline); not fetched by default, since the
# Overture theme carries the same footprints with roof shapes as well.
buildings="[out:json][timeout:240];
(
  way[\"building\"]($south,$west,$north,$east);
  relation[\"building\"]($south,$west,$north,$east);
);
out geom;"

land="[out:json][timeout:240];
(
  way[\"waterway\"~\"^(river|stream|canal)\$\"]($south,$west,$north,$east);
  way[\"natural\"=\"water\"]($south,$west,$north,$east);
  relation[\"natural\"=\"water\"]($south,$west,$north,$east);
  way[\"landuse\"]($south,$west,$north,$east);
  way[\"leisure\"]($south,$west,$north,$east);
);
out geom;"

# Two more datasets, from Amazon S3 rather than Overpass, because Overpass
# cannot supply either: the buildings as *polygons* with roof shapes (Overture
# Maps, which is OpenStreetMap's buildings plus Microsoft's traced ones, ODbL)
# and the shape of the ground (Copernicus DEM GLO-30). `tools/fetch-overture.py`
# does both with plain HTTPS range requests; see its header for the licences.
#
# A machine that can reach the buckets but not Overpass can regenerate the
# buildings and the relief alone from the committed file:
#
#   python3 tools/bake-city.py --from-ron assets/cities/landshut.ron assets/cities/landshut.ron \
#       --buildings-parquet data/landshut_buildings.parquet --dem data/Copernicus_DSM_*.tif
#
# which keeps the streets, water and open ground the file already has. A bake
# of its own output is its own output, so running it twice changes nothing.
overture="${OVERTURE_RELEASE:-2026-08-19.0}"
data="$(mktemp -d)"
trap 'rm -rf "$raw" "$built" "$ground" "$data"' EXIT

echo "asking Overpass for $label ($south,$west .. $north,$east)"
curl -fsS -m 300 -X POST -d "$roads" https://overpass-api.de/api/interpreter -o "$raw"
echo "asking Overpass for its water and its open ground"
curl -fsS -m 400 -X POST -d "$land" https://overpass-api.de/api/interpreter -o "$ground"
echo "asking Overture ($overture) for its buildings"
python3 tools/fetch-overture.py buildings --release "$overture" \
    --bbox "$west" "$south" "$east" "$north" --out "$data/buildings.parquet"
echo "asking Copernicus for the ground"
python3 tools/fetch-overture.py dem --bbox "$west" "$south" "$east" "$north" --out-dir "$data"

mkdir -p assets/cities
dems=()
for tile in "$data"/Copernicus_DSM_*.tif; do dems+=(--dem "$tile"); done
python3 tools/bake-city.py "$raw" "assets/cities/$town.ron" \
    --extras "$ground" --buildings-parquet "$data/buildings.parquet" "${dems[@]}" \
    --centre "$lat" "$lon" --name "$label"

echo "map data (c) OpenStreetMap contributors and (c) Microsoft, ODbL 1.0;"
echo "relief (c) DLR e.V. and (c) Airbus Defence and Space GmbH, Copernicus — see CREDITS.md"
