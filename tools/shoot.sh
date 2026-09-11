#!/usr/bin/env bash
# Renders the standard battery of framings, so a rendering change can be judged
# against the last one rather than against a memory of it.
#
# Every framing here exists because it is the only view that shows something:
# the aerial is the only one that shows the roofline and the shadow distance,
# `--at-car` is the only one that shows bodywork, and the night, rain and
# overcast shots are where the lighting model is actually under load. Adding a
# framing is cheap; the discipline is shooting the same ones every time.
#
# The dawn framing is the one carrying hard coordinates rather than `--at-node`,
# and they are the default seed's: it has to look *into* the low sun down an open
# street, because that is the direction the air lights up from, and a node number
# says nothing about which way its streets run. The face and street-mood
# framings carry coordinates for the same reason and one more: they have to be
# pointed at the *player*, and `--at-node` puts the camera somewhere else in the
# city entirely — where there is nobody, because the crowd is spawned around
# whoever is playing.
#
# Weather is pinned in every framing, including the ones that do not mention it.
# It runs on the game clock now, so a shot without `--hour` would drift its own
# sky between two runs and turn every comparison into an argument about the
# weather rather than about the change under test.
#
#   tools/shoot.sh                       # the default preset, into shots/
#   tools/shoot.sh --quality ultra       # one named preset
#   tools/shoot.sh --all-presets         # every preset, into shots/<preset>/
#   tools/shoot.sh --out shots/before    # somewhere else, for a before/after
#   tools/shoot.sh --only street,night   # just these framings
#   tools/shoot.sh --town landshut       # the Landshut battery instead
#
# This is the visual half of judging a town. The numeric half is
# `cargo run -- --survey --city landshuepf`, which builds the same layout
# without a window and prints a scorecard in under a second; run it first,
# because a change that moved a number is a change worth shooting.
#
# `--town` is a whole different battery rather than a `--city` spliced into the
# framings above, and it has to be. Those framings carry hard coordinates of the
# *generated* city on the default seed — a gas station, a basketball court, a
# parade forming point — and none of them is anywhere in particular in a town
# read off a map. So Landshut gets its own list, each entry naming its own city,
# and `--only <name>` keeps meaning exactly one thing.
#
# Frame times are logged for every shot and collected at the end. A screenshot
# says a change looks right; it says nothing about whether it can be afforded.
set -euo pipefail

cd "$(dirname "$0")/.."

# name|flags. Anything with a fixed --hour also freezes the clock, so the
# warmup frames cannot drift the sky between two runs of the same shot.
FRAMINGS=(
    "aerial|--at 0,620,900 --look 0,20,-200 --stream-radius 1800 --hour 10"
    "street|--at-node 300 --eye 1.7 --hour 16"
    "dusk|--at-node 300 --eye 1.7 --hour 19.4"
    "night|--at-node 300 --eye 1.7 --hour 22.5"
    # Two opposed views at the same night hour, into where the sun rises (+X)
    # and where it set (-X). The halo bug was a glow whose azimuth circled the
    # city overnight; only a pair of framings looking opposite ways at a fixed
    # hour can show that the night sky no longer glows from anywhere in
    # particular. Kept close to the face framings' street so the foreground is
    # a real one.
    "night-east|--at -4,1.7,3 --look 396,25,3 --hour 21.5"
    "night-west|--at -4,1.7,3 --look -404,25,3 --hour 21.5"
    "rain|--at-node 300 --eye 1.7 --hour 21.5 --wet 0.9 --cover 1"
    "dawn|--at -163.6,1.7,-744.3 --look 836,25,-604 --hour 6.4"
    "overcast|--at-node 300 --eye 1.7 --hour 13 --cover 1"
    "facade|--at -163.0,4.5,-759.5 --look -172.6,6.6,-759.5 --hour 15"
    # A filling station and a stocked parking lot, hard default-seed
    # coordinates like the dawn framing: `--at-node` cannot know which lots
    # the vacancy roll left empty, and these are the only framings that show
    # the vacant-lot furnishing (canopy, pumps, bay paint, parked rows).
    "gasstation|--at -905,8,66 --look -888,2,35 --hour 15"
    "parkinglot|--at -840,14,-800 --look -804,2,-767 --hour 15"
    # A basketball court from above the baseline: the one framing that shows
    # a whole set of painted markings at once, which is the only way to see
    # whether they are a court or four lines and a hoop.
    "court|--at -889.8,32,-155 --look -889.8,2,-190 --hour 12"
    "park|--at 522,1.8,-986 --look 610,7,-902 --hour 9"
    # The open doors of M9: a supermarket frontage by day and night (the
    # night shot is the lit-shop test), the view from inside the shop, a
    # parking-garage deck from inside its mouth, and the fire station's
    # painted roller doors. Hard default-options-seed coordinates like the
    # other lot framings.
    "door|--at -830,2.0,541 --look -844,3.5,541 --hour 15"
    "door-night|--at -830,2.0,541 --look -844,3.5,541 --hour 22"
    "interior|--at -872,1.6,545 --look -845,1.4,540 --hour 15"
    "parkhaus|--at 65,2.5,146 --look 88,5.5,140 --hour 15"
    "feuerwehr|--at -831,3,313 --look -800,5,313 --hour 15"
    # Down onto the carriageway from a first-floor window. The only framing
    # that shows the road *surface*: from head height a decal is four pixels
    # tall and every one of them is on the horizon.
    "wear|--at -163.6,12,-744.3 --look -138.9,0,-740.8 --hour 11"
    "cars|--at-car --hour 11"
    "showroom|--showroom --hour 11"
    # The cast, standing still in draw order. The rare archetypes are the
    # ones whose costume goes wrong and the ones a street framing cannot be
    # relied on to contain — a wheelchair is four hundredths of the draw.
    "cast|--lineup --hour 12"
    # The hydrant geyser. Long enough for the column to fill with droplets.
    "geyser|--geyser --frames 240 --hour 15"
    # The CSD column at its default-seed forming point (day zero's route),
    # long enough for it to form and start marching. `--event` is the one
    # door through the events-sit-out-capture rule.
    "parade|--at -4,4,-714 --look -4,1.5,-748 --hour 12 --event csd --frames 400"
    "driving|--follow --drive --frames 2000 --hour 15"
    "map|--follow --map"
    # The faces, held at three points on the scale by `--mood`. Nothing else
    # can shoot these: a city has to be *put* in a mood before it wears one,
    # and waiting for it to arrive there on its own is not a screenshot.
    "face-angry|--at -4.28,2.15,3.4 --look -4.28,2.12,4.9 --hour 12 --mood -1"
    "face-calm|--at -4.28,2.15,3.4 --look -4.28,2.12,4.9 --hour 12 --mood 0"
    "face-happy|--at -4.28,2.15,3.4 --look -4.28,2.12,4.9 --hour 12 --mood 1"
    # And the street in both moods. Long enough for the crowd to have walked in
    # from the ring it spawns on and started provoking each other.
    "rage|--at -4,3,62 --look -4,1.2,10 --hour 12 --frames 240 --mood -1"
    "delight|--at -4,3,62 --look -4,1.2,10 --hour 12 --frames 240 --mood 1"
)

# The Landshut battery. Same discipline, different town: every framing pins
# `--city Landshuepf` and an `--hour`, and the coordinates are metres about the
# centre of the OSM extract (48.5375, 12.1508), north on -Z.
#
# The style name is `Landshuepf`, not `landshut` — `--city` matches the label or
# the debug name and panics on anything else, and `--city landshut` has already
# killed one run of this script.
#
# Note the heights are in `--at`, not in `--eye`: `--eye` is only read when the
# camera is placed by `--at-node`, and a framing that carries both is quietly
# ignoring one of them.
LANDSHUT=(
    # The Altstadt looking south down the market street: the postcard, and the
    # one framing that shows the setts, the gabled terrace and the width of the
    # street in the same frame.
    # On the market street's own centreline, which the bake now puts between
    # the house rows -- a framing on the old line stands inside a real house.
    "altstadt|--city Landshuepf --at 34.2,1.7,197.7 --look -1.3,1.4,301.6 --hour 12 --frames 200"
    "altstadt-night|--city Landshuepf --at 34.2,1.7,197.7 --look -1.3,1.4,301.6 --hour 21.5 --frames 200"
    # And back the other way, north towards the Isar, in the afternoon light.
    "altstadt-north|--city Landshuepf --at -1.3,1.7,301.6 --look 43.7,1.4,170.0 --hour 15 --frames 200"
    # Down onto the paving from a first-floor window. The only framing that
    # shows the size of a sett, which is the thing a plan view cannot lie about.
    "setts|--city Landshuepf --at 34.2,7.0,197.7 --look 40,0,168 --hour 12 --frames 120"
    # A junction from above: pavements, mitres, the junction plate and whether
    # anything is standing in the road.
    "junction|--city Landshuepf --at 43.7,45,170 --look 43.7,0,169 --hour 12 --frames 120"
    # Where the Isar runs, and does not yet.
    # From above the riverside trees, which the terrain now plants.
    "isar|--city Landshuepf --at 40,14,-224 --look -25,1,-329 --hour 12 --frames 120"
    "isar-air|--city Landshuepf --at 30,170,-70 --look -20,0,-310 --stream-radius 1500 --hour 12 --frames 200"
    # The bare ground inside the town, which is the complaint this battery was
    # written to be able to see.
    "empty|--city Landshuepf --at -300,2.0,-475 --look -180,12,-395 --hour 12 --frames 120"
    "empty-air|--city Landshuepf --at 0,240,0 --look 0,0,-1 --stream-radius 1500 --hour 12 --frames 200"
    # The whole town, for the roofline and for whether it still stops dead at
    # the edge of the built area.
    "air|--city Landshuepf --at 0,620,900 --look 0,20,-200 --stream-radius 1800 --hour 10 --frames 200"
    # The Hofberg, from the Altstadt looking south up the hill: the one
    # framing that shows the relief standing behind the roofs, and whether
    # the town's flat floor meets it as a slope or as a cliff. The castle is
    # up there on its own ground, so `trausnitz` looks across at it from the
    # hill's own height rather than up from the street.
    "hofberg|--city Landshuepf --at -37.1,1.7,429.7 --look 150,60,700 --stream-radius 1500 --hour 15 --frames 200"
    "trausnitz|--city Landshuepf --at 200,45,380 --look 140,60,660 --hour 15 --frames 200"
    # Down onto the Altstadt roofs from sixty metres: the roof shapes the map
    # gave each building are only visible from above, and a gable that the
    # map says is a hip is a mistake that no street framing can see.
    "roofs|--city Landshuepf --at 36,60,330 --look 60,20,180 --stream-radius 1500 --hour 15 --frames 200"
    # From high over the Isar plain, looking south across the whole town to
    # the hill behind it: the postcard the relief was baked for.
    "south-air|--city Landshuepf --at -300,420,-500 --look 150,30,600 --stream-radius 1800 --hour 10 --frames 200"
    # One house front from nine metres at eye height, on the Altstadt. The
    # framing that shows a render, a window reveal and a shutter at the size a
    # person sees them, which is the size at which a facade is wrong.
    "facade-close|--city Landshuepf --at 40,1.7,200 --look 56,6,197 --hour 15 --frames 120"
    # A narrow side lane: the Ländgasse, four metres of setts running between
    # the Altstadt and the Isar, from its midpoint looking along it. The
    # market street is the postcard; a lane is where the street wall, the
    # gap between two rows and the paving are all within arm's reach.
    "lane|--city Landshuepf --at 41.8,1.7,37 --look 63.4,1.4,3.4 --hour 15 --frames 120"
    "cast|--city Landshuepf --lineup --hour 12"
)

presets=()
out=""
only=""
profile="--release"
town=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --quality)     presets=("$2"); shift 2 ;;
        --all-presets) presets=(low medium high ultra photo); shift ;;
        --out)         out="$2"; shift 2 ;;
        --only)        only="$2"; shift 2 ;;
        --town)        town="$2"; shift 2 ;;
        --debug)       profile=""; shift ;;
        *) echo "unknown flag: $1" >&2; exit 2 ;;
    esac
done

case "$town" in
    "")        ;;
    landshut)  FRAMINGS=("${LANDSHUT[@]}") ;;
    *) echo "unknown town: $town (try landshut)" >&2; exit 2 ;;
esac
[[ ${#presets[@]} -eq 0 ]] && presets=(high)

# The capture harness renders to an offscreen texture but still opens a window,
# so an unattended run needs something for it to open onto. Wrapping in Xvfb
# only when there is no display keeps an interactive run untouched.
# macOS has neither variable and needs neither: its window server is always
# there. Without this check every run on a Mac prints a warning about a display
# it does not use.
# `command` is a no-op prefix rather than an empty array, because macOS ships
# bash 3.2, where expanding an empty array under `set -u` is an unbound
# variable and kills the run on the first shot.
runner=(command)
if [[ "$(uname)" != Darwin && -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
    if command -v xvfb-run >/dev/null; then
        runner=(xvfb-run -a)
        echo "no display; running under Xvfb"
    else
        echo "warning: no display and no xvfb-run — captures will probably fail" >&2
    fi
fi

# Build once up front. Otherwise the first shot's frame times include the tail
# of a compile, which is the sort of thing that gets read as a regression.
echo "building..."
cargo build $profile

summary=$(mktemp)
trap 'rm -f "$summary"' EXIT

for preset in "${presets[@]}"; do
    # Spelt out rather than folded into one `${out:-...}` with a command
    # substitution in it. The clever version had a `[[ ]] && echo` inside the
    # substitution, so on a single preset — the common case — the substitution
    # exited non-zero, and `set -e` killed the script between building and the
    # first shot, silently and with no output at all.
    if [[ -n "$out" ]]; then
        dir="$out"
    elif [[ ${#presets[@]} -gt 1 ]]; then
        dir="shots/$preset"
    else
        dir="shots"
    fi
    mkdir -p "$dir"

    for framing in "${FRAMINGS[@]}"; do
        name="${framing%%|*}"
        flags="${framing#*|}"

        if [[ -n "$only" && ",$only," != *",$name,"* ]]; then
            continue
        fi

        path="$dir/$name.png"
        printf 'shoot   %-9s %s\n' "$name" "$path"

        # Frame times go to the summary; everything else is Bevy's startup
        # chatter and is only worth seeing when the shot fails.
        log=$(mktemp)
        if "${runner[@]}" cargo run $profile -- \
                --screenshot "$path" --quality "$preset" --fps-log $flags \
                >"$log" 2>&1; then
            grep -h "frame times" "$log" \
                | sed "s|^|$preset/$name  |" >>"$summary" || true
        else
            echo "        FAILED — last lines:" >&2
            tail -20 "$log" >&2
            rm -f "$log"
            exit 1
        fi
        rm -f "$log"
    done
done

echo
echo "frame times"
echo "-----------"
cat "$summary"
