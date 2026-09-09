// A kilometre of ground that is not one colour, and a town that has a floor.
//
// The land around a town read off a map is one quad with a grass scan tiled
// across it. Every square metre of that is correct and every square metre is the
// same, so from a road or a rooftop it comes out as a flat poster-paint green
// plain — the single loudest surface in an aerial framing and the one with
// nothing on it. Tiling faster does not help and tiling slower helps less: the
// texture is right at the scale it was photographed, and what is missing is
// everything above that scale.
//
// Which, on real ground, is a lot. Fields were sown at different times, the
// hollows hold water and the ridges dry out, and the whole thing is a patchwork
// hundreds of metres across. None of that needs geometry or another texture —
// it is two octaves of noise in world space, modulating what the scan already
// says.
//
// World space, not UV, for the reason every mask in this project is: the ground
// tiles every few metres, so anything sampled in its UVs repeats at that size
// and comes out as a pattern rather than as terrain.
//
// The first version of this did only the two things above, and it was still
// wrong, in two ways that the noise could never have fixed:
//
//  1. It varied *value and hue* but never *surface*. Every square metre stayed
//     grass; the result was "green, and slightly different green". Worse, the
//     drying was a multiply — `color * vec3(1.42, 1.15, 0.52)` — so it made the
//     high ground brighter, which on an albedo that was already two and a half
//     times too high (see `ground::TURF_TINT`) gave chartreuse rather than
//     straw. Drying now moves towards a *fixed* colour instead: dry grass is
//     straw, and straw is straw whatever it dried from. Bare earth and
//     courtyard grit arrive the same way, at their own scales.
//
//  2. It had no idea where the town was. One material covers forty kilometres
//     while the town is two of them, so a fragment in a market square and a
//     fragment in a field two kilometres out were shaded identically — which is
//     why the inside of every block was meadow running up to the kerb. The town
//     now arrives as a mask rasterised from the road graph once at startup:
//     red for the backland immediately behind a pavement, green for anywhere
//     inside the built-up envelope. It is not a texture of the ground, it is a
//     map of the town, and one bilinear tap of it is what turns a lawn between
//     two streets into a yard.

#import bevy_pbr::{
    pbr_types::PbrInput,
    pbr_functions::alpha_discard,
    pbr_fragment::pbr_input_from_standard_material,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}
#endif

struct GroundSettings {
    // Metres across one repeat of the largest scale of variation.
    tile: f32,
    // How far that variation may move the surface's value, either way.
    value: f32,
    // How much of it goes dry and yellow rather than merely pale.
    dry: f32,
    // How much of the surface's own hue survives the pull towards luminance.
    saturation: f32,
    // How much bare earth comes through where the cover is thin.
    dirt: f32,
    // How loudly the town mask speaks. Zero for a lawn, which has no mask
    // bound and would otherwise read the white fallback as "all town".
    urban: f32,
    // Metres from the middle of the world to the edge of the mask.
    half_extent: f32,
    // Unused; a uniform is padded to sixteen bytes whether or not it is
    // written that way, and naming the padding is cheaper than discovering it.
    pad: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> ground: GroundSettings;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var town: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var town_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var yard: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var yard_sampler: sampler;

// The three surfaces the grass is allowed to stop being, in linear albedo.
//
// Fixed colours rather than multipliers of the grass, which is the whole point:
// a multiplier keeps the scan's hue and only pushes it about, so every state of
// the ground stayed a state of *grass*. Straw is straw, soil is soil, and a
// courtyard is neither.
//
// All three sit between 0.055 and 0.12 linear, which is where dry ground
// measures. Anything brighter competes with the render's concrete and starts
// the poster paint again from the other end.
const STRAW = vec3(0.118, 0.104, 0.058);
const EARTH = vec3(0.078, 0.062, 0.044);
const GRIT = vec3(0.083, 0.075, 0.061);

// The same lattice noise the road and the sky use. A third copy, deliberately:
// see the note in `sky.wgsl` — a shared import has to be a loaded shader asset
// with its own ordering hazard, for eleven lines that have never changed.
fn hash2(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2(1.0, 0.0));
    let c = hash2(i + vec2(0.0, 1.0));
    let d = hash2(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var total = 0.0;
    var at = p;
    for (var octave = 0; octave < 4; octave += 1) {
        sum += value_noise(at) * amplitude;
        total += amplitude;
        at = at * 2.11 + vec2(11.7, -3.3);
        amplitude *= 0.5;
    }
    return sum / total;
}

// Pushes a value noise's distribution out towards its ends.
//
// Four octaves averaged is a sum of uniform variables, so it is close to normal
// and spends nearly all of itself within a fifth of the middle. Thresholding
// such a field at 0.56 catches a sliver; multiplying by it barely moves
// anything. Every use of noise as *terrain* rather than as grain wants the
// opposite shape — mostly one thing or the other, with edges between.
fn spread(value: f32) -> f32 {
    return clamp((value - 0.5) * 1.9 + 0.5, 0.0, 1.0);
}

// What the grit scan has to be multiplied by to be a yard rather than a
// quarry. `Gravel023` is white chippings in the sun with a mean linear albedo
// of 0.70 — the brightest map in the library by three times — and the same
// argument `ground::TURF_TINT` makes about grass applies to it twice over.
const GRIT_TINT: vec3<f32> = vec3(0.20, 0.185, 0.165);

// Metres of yard one repeat of it covers. In world space rather than in the
// mesh's UVs, for the reason the rest of this file is: the ground restarts its
// coordinates every cell, and a surface sampled in them would carry the seam.
const YARD_TILE: f32 = 2.1;

// What one field is against the next: a little greener, or a little more gone
// over. Multipliers rather than targets, so whatever the ground already is
// stays recognisably itself — this is the difference between two fields of the
// same crop, not between a field and a quarry.
const CROP_LUSH: vec3<f32> = vec3(0.86, 1.10, 0.80);
const CROP_PALE: vec3<f32> = vec3(1.14, 1.02, 0.78);

fn vary(input: PbrInput) -> PbrInput {
    var pbr_input = input;
    let here = pbr_input.world_position.xz;

    // Two scales. The broad one is the field, the close one is what is going on
    // inside it; without the second, the plain is smooth blobs, which is a
    // different wrong answer from a flat green but not a better one.
    let broad = spread(fbm(here / ground.tile));
    let close = fbm(here / (ground.tile * 0.23));
    let field = broad * 0.66 + close * 0.34;

    // Where the town is. One tap, clamped at the edges, so everything past the
    // extract's own square reads as open country — which it is.
    let uv = here / (ground.half_extent * 2.0) + 0.5;
    let mask = textureSample(town, town_sampler, uv);
    // Backland: the strip immediately behind a pavement, and the wedge at an
    // oblique corner that no pavement covers. Nothing grows there.
    let backland = mask.r * ground.urban;
    // And anywhere at all inside the built-up envelope, which is where the
    // block interiors are.
    let inside = mask.g * ground.urban;
    // How hard the ground is worked, which is *not* the same as being inside
    // the town.
    //
    // The envelope reaches a hundred and forty metres from a street, and the
    // gap between two of Landshut's is about a hundred and fifty — so the
    // middle of a big block reads as fully inside the town while being two
    // hundred metres from anything. Shaded as courtyard it came back as one
    // flat brown expanse, which is the largest surface in an aerial and is not
    // a courtyard, it is a field with a town round it. What is actually in the
    // middle of a European block is gardens.
    //
    // So the *hard* ground follows the back land — the strip behind a pavement
    // — and the envelope only leans on it. Inside stays green.
    let hard = clamp(backland * 0.85 + inside * 0.22, 0.0, 1.0);

    var color = pbr_input.material.base_color.rgb;

    // The order here is: choose a surface, then shade it. The first version did
    // the opposite — value first, then hue — which meant every mix towards a
    // fixed target threw the shading away again, so the moment the town mask
    // arrived the whole of the back land came out as one flat slab of grit with
    // no variation in it at all. A patch of gravel is lit and worn the same way
    // a patch of grass is; the value belongs at the end, over all of them.

    // Saturation first, before anything is mixed towards a target, so the
    // targets are not themselves desaturated. Town ground is greyer than
    // country ground for the ordinary reason: it is walked on, parked on and
    // covered in the dust off a road.
    let luma = dot(color, vec3(0.2126, 0.7152, 0.0722));
    // Town ground is greyer than country ground for the ordinary reason: it is
    // walked on, parked on and covered in the dust off a road. Open country
    // keeps rather more of its own green than the first pass left it — at 0.58
    // everywhere, the fields two hundred metres out came back at sRGB
    // (79, 78, 67), which is not a field, it is mud, and it is the largest
    // surface in an aerial.
    color = mix(vec3(luma), color, ground.saturation * mix(1.30, 0.66, hard));

    // The high ground goes off. Towards straw, not towards `color * 1.42`:
    // grass that has dried is not brighter grass, it is a different and much
    // duller colour, and multiplying an albedo that was already too high is
    // exactly how the plain acquired its chartreuse patches.
    // Patchy rather than general. A wide smoothstep browns *most* of the plain
    // by some amount, which is a plain that is uniformly half straw; a narrow
    // one gives fields, some cut and some not, which is what farmland is. The
    // town keeps the wider one, because a yard really is uniformly worn.
    let edge = mix(0.16, 0.06, hard);
    let parched = smoothstep(0.62 - edge, 0.62 + edge, broad) * ground.dry;
    color = mix(color, STRAW, parched);

    // And one field is not the next. A slow hue swing at the field scale, off
    // its own offset so it does not follow the drying: some are in crop, some
    // are cut, some are pasture. Held to the town's own share of it, where a
    // yard has no crop in it to vary.
    let crop = fbm(here / (ground.tile * 1.7) + vec2(-113.0, 61.0));
    color = mix(color, color * mix(CROP_LUSH, CROP_PALE, crop), 1.0 - hard);

    // Bare earth, at a third of the field's scale and offset off it, so soil
    // shows through where the cover happens to be thin rather than along the
    // same contours the drying follows. More of it inside the town, where the
    // ground is walked over.
    let thin = fbm(here / (ground.tile * 0.31) + vec2(37.0, -19.0));
    let bare = smoothstep(0.58, 0.86, thin) * clamp(ground.dirt * mix(1.0, 2.6, hard), 0.0, 0.9);
    color = mix(color, EARTH, bare);

    // And the town's own floor. A yard, a forecourt, the gravel behind a row of
    // houses: worn to grit, and it is what a block interior is actually made
    // of. Grit broken with earth rather than flat grit, at the close scale, so
    // that a courtyard forty metres across has something happening across it —
    // one constant is what a slab is, and a slab between two streets is the
    // failure this replaced, turned inside out.
    // What the floor is made of, and it is a *photograph* rather than a
    // constant. The first pass mixed towards the three fixed albedos below and
    // nothing else, and a colour is not a surface: the only image bound was
    // grass, so the moment the mask said "town" a courtyard forty metres across
    // became one smooth beige plane with a soft edge round it — which reads
    // worse than the meadow it replaced, because meadow at least had a
    // photograph under it. Sampled in the ground's own UVs, so it tiles at the
    // same few metres the grass does.
    let grit = textureSample(yard, yard_sampler, here / YARD_TILE).rgb * GRIT_TINT;
    let floor = mix(grit, mix(GRIT, EARTH, smoothstep(0.40, 0.80, close) * 0.55), 0.35);
    // And not everywhere. The land behind a terrace is yards *and* gardens, and
    // a town whose whole back land is one gravel is exactly as wrong as one
    // whose whole back land is one lawn — which is what the first pass with the
    // mask in produced, a beige field with houses on it instead of a green one.
    // `close` is the fourteen-metre octave, which is about the size of the
    // thing being decided: this yard is gravel, that one is somebody's garden.
    let kept = smoothstep(0.22, 0.64, close);
    // Held well under one on top of that, so weeds still come through a yard
    // and the ground under the town never becomes a second carriageway.
    let worn = clamp(
        (smoothstep(0.10, 0.85, backland) * 0.72 + smoothstep(0.55, 1.0, inside) * 0.14)
            * mix(0.28, 1.0, kept),
        0.0,
        0.82,
    );
    color = mix(color, floor, worn);

    // Now the value, over whatever the surface turned out to be. See the note
    // this replaced: a third either way in linear light comes back as about a
    // tenth of a code either way in the image, because sRGB spends most of its
    // range below mid grey and ground sits above it. It is the quietest of the
    // four terms and it is here for the long shallow gradients, not for the
    // patchwork.
    color = color * (1.0 + (field - 0.5) * 2.0 * ground.value);

    pbr_input.material.base_color = vec4(color, pbr_input.material.base_color.a);

    // Damp ground is a little glossier than dry, which is most of what makes a
    // hollow read as a hollow from a distance. The swing used to be a sixteenth
    // either way, which on a base of one is under the noise floor of the
    // roughness map itself; a fifth reads. Grit and earth go back the other
    // way, because neither has ever been glossy.
    let rough = pbr_input.material.perceptual_roughness
        - (0.5 - field) * 0.40
        + bare * 0.10
        + worn * 0.12;
    pbr_input.material.perceptual_roughness = clamp(rough, 0.45, 1.0);

    return pbr_input;
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let in = vertex_output;

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color =
        alpha_discard(pbr_input.material, pbr_input.material.base_color);

    pbr_input = vary(pbr_input);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
