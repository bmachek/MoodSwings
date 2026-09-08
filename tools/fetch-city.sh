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
# box of one small town, once, and the answer is under a megabyte. Do not put it
# in a loop.
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
trap 'rm -f "$raw"' EXIT

query="[out:json][timeout:120];
(
  way[\"highway\"~\"^(motorway|motorway_link|trunk|trunk_link|primary|primary_link|secondary|secondary_link|tertiary|tertiary_link|unclassified|residential|living_street|pedestrian)\$\"]($south,$west,$north,$east);
);
out geom;"

echo "asking Overpass for $label ($south,$west .. $north,$east)"
curl -fsS -m 240 -X POST -d "$query" https://overpass-api.de/api/interpreter -o "$raw"

mkdir -p assets/cities
python3 tools/bake-city.py "$raw" "assets/cities/$town.ron" \
    --centre "$lat" "$lon" --name "$label"

echo "map data (c) OpenStreetMap contributors, ODbL 1.0 — see CREDITS.md"
