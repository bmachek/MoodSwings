#!/usr/bin/env bash
# Fetches the scanned materials (optional) and the recorded sound bank
# (required — the synthesised bank was retired).
#
# Everything here is CC0 1.0 — public domain, no attribution required, no
# restrictions on use. The project is FOSS and any licence-compatible source
# would do (CC-BY with credit included), but so far every sound worth having
# has turned up under CC0, so the stronger guarantee is kept while it costs
# nothing. Materials come from ambientCG (https://ambientcg.com), sounds from
# OpenGameArt (https://opengameart.org) and Freesound (https://freesound.org).
#
# The freesound.org entries point at cdn previews (128kbps mp3) rather than
# the originals, because original downloads sit behind a login and the
# previews do not. The licence is the sound's licence either way, and the
# bank remixes everything to mono at its own rate on load, so the transcode
# is not the bottleneck.
#
# The download lands in assets/materials/ and assets/sounds/, both gitignored.
# The materials remain an optional upgrade — `world::texture` paints a
# procedural stand-in for anything missing — but the sounds stopped being
# one: the synthesised bank was retired, so this script is the required setup
# step for audio. A clone that has not run it still starts, and plays silence
# where the missing sounds should be, warning per gap in the log.
#
# KEEP IN SYNC with tools/fetch-materials.bat — the Windows twin of this
# script. Any material or sound added here must be added there too.
#
#   tools/fetch-materials.sh          # fetch anything missing
#   tools/fetch-materials.sh --force  # re-fetch everything
set -euo pipefail

cd "$(dirname "$0")/.."
DEST="assets/materials"
RESOLUTION="2K-JPG"

# The set the renderer looks for. Adding one here is all it takes for
# `world::texture` to prefer it over its procedural version.
MATERIALS=(
    Asphalt031        # road surface
    PavingStones138   # pavement slabs
    Concrete034       # facades
    Concrete046       # facades
    Bricks097         # facades
    Bricks104         # facades
    Bricks075A        # facades
    PaintedPlaster006 # facades
    Gravel023         # flat roofs
    Grass005          # parks
)

force=false
[[ "${1:-}" == "--force" ]] && force=true

mkdir -p "$DEST"
for material in "${MATERIALS[@]}"; do
    target="$DEST/$material"
    if [[ -d "$target" && "$force" == false ]]; then
        echo "have    $material"
        continue
    fi

    archive="$(mktemp -t "$material.XXXXXX").zip"
    url="https://ambientcg.com/get?file=${material}_${RESOLUTION}.zip"

    echo "fetch   $material"
    if ! curl -fsSL --retry 3 --retry-delay 2 -o "$archive" "$url"; then
        echo "        failed; skipping" >&2
        rm -f "$archive"
        continue
    fi

    rm -rf "$target"
    mkdir -p "$target"
    # -j: the archives are flat already, but this guarantees it.
    unzip -qoj "$archive" -d "$target"
    rm -f "$archive"

    # Drop what the renderer will never read, so the directory is a manifest of
    # what is actually used rather than of what happened to be in the zip.
    find "$target" -type f ! -name '*.jpg' -delete
done

# ------------------------------------------------------------------ sounds ----
#
# One entry per sound bank name (see `audio::bank::REGISTER`): "<name>|<url>".
# The file keeps its source extension; `audio::files` tries wav/flac/ogg/mp3
# in turn. This half of the script stopped being optional when the synthesis
# was retired: every sound the game makes is one of these recordings, a
# missing file plays as silence with a warning, and the bank's tests hold
# this list and the REGISTER against each other. A recording dropped into
# assets/sounds/ by hand still wins over a fetch, so replacing a take is
# just replacing a file.
SOUNDS_DEST="assets/sounds"
SOUNDS=(
    "boing|https://opengameart.org/sites/default/files/boing.flac"
    "crash|https://opengameart.org/sites/default/files/qubodup-crash.ogg"
    # A bicycle horn on a car is the joke; a car horn is only a car.
    "honk|https://opengameart.org/sites/default/files/bicycle-horn-1.wav"
    "explosion|https://opengameart.org/sites/default/files/Chunky%20Explosion.mp3"
    "birdsong|https://opengameart.org/sites/default/files/park_ambience_birds.wav"
    "spray|https://opengameart.org/sites/default/files/park_ambience_river.wav"
    "uproar|https://opengameart.org/sites/default/files/crowd_shouting_0.ogg"
    # The cheer, whistled by actual people (all CC0): elle-trudgett's
    # innocent whistle, elijahgoodson's tune, Willygoat's calm one. Three
    # takes because one whistle repeated is a doorbell — see `audio::bank`.
    "whistle-0|https://cdn.freesound.org/previews/146/146887_197046-hq.mp3"
    "whistle-1|https://cdn.freesound.org/previews/411/411578_7994683-hq.mp3"
    "whistle-2|https://cdn.freesound.org/previews/411/411062_7963328-hq.mp3"
    # An actual ACME swanee whistle sliding up (v0idation, CC0), for the arc
    # through the air; and a jaw-harp twang (magnuswaker, CC0) for a street
    # prop leaving its bolts.
    "wheee|https://cdn.freesound.org/previews/497/497092_942821-hq.mp3"
    "sproing|https://cdn.freesound.org/previews/540/540790_11537497-hq.mp3"
    # The voices, by actual people, all CC0 and all wordless — which was the
    # line that let recordings in here at all. Reitanna's giggle and her
    # three flavours of frustrated groan carry the grumbles; the curses are
    # angry grunts from Rocotilos, lipalearning and ssierra1202; the gasp is
    # kanyonwyvern's. Playback still pitches every take per speaker.
    "giggle|https://cdn.freesound.org/previews/323/323702_950925-hq.mp3"
    "grumble-0|https://cdn.freesound.org/previews/351/351163_950925-hq.mp3"
    "grumble-1|https://cdn.freesound.org/previews/343/343929_950925-hq.mp3"
    "grumble-2|https://cdn.freesound.org/previews/351/351157_950925-hq.mp3"
    "curse-0|https://cdn.freesound.org/previews/341/341489_1400623-hq.mp3"
    "curse-1|https://cdn.freesound.org/previews/427/427972_4687265-hq.mp3"
    "curse-2|https://cdn.freesound.org/previews/391/391939_6450069-hq.mp3"
    "gasp|https://cdn.freesound.org/previews/740/740310_15125504-hq.mp3"
    # The last of the taunt rotation to be recorded: Reitanna's tongue
    # raspberry and one of Blubberfreak's many farts. The sorry is
    # theuncertainman's contrite NPC — the one entry with a word in it,
    # allowed in since the wordless rule was retired by decision.
    "raspberry|https://cdn.freesound.org/previews/252/252262_950925-hq.mp3"
    "fart|https://cdn.freesound.org/previews/732/732936_15881540-hq.mp3"
    "sorry|https://cdn.freesound.org/previews/458/458075_6492957-hq.mp3"
    # The disgust department (both CC0): mefrancis13's dry retching and
    # bbrocer's cartoon gagging. Deliberately kept out of the taunt
    # rotation; they belong to one particular gentleman.
    "retch|https://cdn.freesound.org/previews/117/117605_2056891-hq.mp3"
    "gag|https://cdn.freesound.org/previews/382/382663_4297074-hq.mp3"
    # Real tyres squealing round a real corner (audible-edge, CC0). The slip
    # tracking that kept this synthesised for so long lives in playback
    # speed, which works on a recording just as well.
    "screech|https://cdn.freesound.org/previews/71/71739_995351-hq.mp3"
    # Small talk for the indifferent middle of the mood scale: dodrio's
    # wordless dialogue mumbles, three takes.
    "murmur-0|https://cdn.freesound.org/previews/554/554017_1433422-hq.mp3"
    "murmur-1|https://cdn.freesound.org/previews/554/554018_1433422-hq.mp3"
    "murmur-2|https://cdn.freesound.org/previews/554/554020_1433422-hq.mp3"
    # Zone ambience: mycompasstv's restaurant chatter, hung on restaurant
    # frontages by the streaming emitters.
    "chatter|https://cdn.freesound.org/previews/474/474740_3902754-hq.mp3"
    # The animals: kwahmah_02's single dog bark and skymary's short cat
    # meow, one take each, pitched per animal like the crowd's voices.
    "bark|https://cdn.freesound.org/previews/277/277058_4486188-hq.mp3"
    "meow|https://cdn.freesound.org/previews/412/412017_3652520-hq.mp3"
)
# These two live inside one zip (qubodup's CC0 car pack): "<name>|<member>".
CAR_PACK_URL="https://opengameart.org/sites/default/files/car_sound_effects_pack.zip"
CAR_PACK=(
    "engine|Car_Engine_Loop.ogg"
    "car-door|Car_Door_Close.ogg"
)
# More of the taunt rotation, from rubberduck's CC0 creature pack. cough_03
# is the double cough, spit_01 the tidiest spit, and burp_01 the burp that
# finally earned the mystery burp.ogg its slot in the rotation.
CREATURE_PACK_URL="https://opengameart.org/sites/default/files/80-CC0-creature-SFX_0.zip"
CREATURE_PACK=(
    "cough|cough_03.ogg"
    "spit|spit_01.ogg"
    "burp|burp_01.ogg"
)
# Footsteps and the traffic bed, from rubberduck's second CC0 SFX hundred.
# The highway loop *is* the city ambience: what the mood mixer wants from
# `ambience` is anonymous distant traffic, which is exactly this.
SFX100_PACK_URL="https://opengameart.org/sites/default/files/sfx_100_v2.zip"
SFX100_PACK=(
    "footstep|sfx100v2_footstep_01.ogg"
    "ambience|sfx100v2_loop_highway.ogg"
    # The filling-station forecourt hum, for the same emitters.
    "forecourt|sfx100v2_loop_machine_02.ogg"
)

mkdir -p "$SOUNDS_DEST"
for entry in "${SOUNDS[@]}"; do
    name="${entry%%|*}"
    url="${entry#*|}"
    ext="${url##*.}"
    target="$SOUNDS_DEST/$name.$ext"
    if [[ -f "$target" && "$force" == false ]]; then
        echo "have    $name"
        continue
    fi
    echo "fetch   $name"
    if ! curl -fsSL --retry 3 --retry-delay 2 -o "$target" "$url"; then
        echo "        failed; skipping (the game plays silence there until it is fetched)" >&2
        rm -f "$target"
    fi
done

need_pack=false
for entry in "${CAR_PACK[@]}"; do
    name="${entry%%|*}"
    member="${entry#*|}"
    [[ -f "$SOUNDS_DEST/$name.${member##*.}" && "$force" == false ]] || need_pack=true
done
if [[ "$need_pack" == true ]]; then
    echo "fetch   car sound pack"
    pack="$(mktemp -t carpack.XXXXXX).zip"
    if curl -fsSL --retry 3 --retry-delay 2 -o "$pack" "$CAR_PACK_URL"; then
        for entry in "${CAR_PACK[@]}"; do
            name="${entry%%|*}"
            member="${entry#*|}"
            unzip -qop "$pack" "$member" > "$SOUNDS_DEST/$name.${member##*.}"
        done
    else
        echo "        failed; skipping (the game plays silence there until they are fetched)" >&2
    fi
    rm -f "$pack"
fi

# The same dance for the creature pack. Duplicated rather than factored into a
# function because macOS still ships bash 3.2, which has no array namerefs.
need_pack=false
for entry in "${CREATURE_PACK[@]}"; do
    name="${entry%%|*}"
    member="${entry#*|}"
    [[ -f "$SOUNDS_DEST/$name.${member##*.}" && "$force" == false ]] || need_pack=true
done
if [[ "$need_pack" == true ]]; then
    echo "fetch   creature sound pack"
    pack="$(mktemp -t creaturepack.XXXXXX).zip"
    if curl -fsSL --retry 3 --retry-delay 2 -o "$pack" "$CREATURE_PACK_URL"; then
        for entry in "${CREATURE_PACK[@]}"; do
            name="${entry%%|*}"
            member="${entry#*|}"
            unzip -qop "$pack" "$member" > "$SOUNDS_DEST/$name.${member##*.}"
        done
    else
        echo "        failed; skipping (the game plays silence there until they are fetched)" >&2
    fi
    rm -f "$pack"
fi

# And once more for the SFX hundred (same bash-3.2 duplication as above).
need_pack=false
for entry in "${SFX100_PACK[@]}"; do
    name="${entry%%|*}"
    member="${entry#*|}"
    [[ -f "$SOUNDS_DEST/$name.${member##*.}" && "$force" == false ]] || need_pack=true
done
if [[ "$need_pack" == true ]]; then
    echo "fetch   sfx hundred pack"
    pack="$(mktemp -t sfx100pack.XXXXXX).zip"
    if curl -fsSL --retry 3 --retry-delay 2 -o "$pack" "$SFX100_PACK_URL"; then
        for entry in "${SFX100_PACK[@]}"; do
            name="${entry%%|*}"
            member="${entry#*|}"
            unzip -qop "$pack" "$member" > "$SOUNDS_DEST/$name.${member##*.}"
        done
    else
        echo "        failed; skipping (the game plays silence there until they are fetched)" >&2
    fi
    rm -f "$pack"
fi

echo
du -sh "$DEST" "$SOUNDS_DEST"
