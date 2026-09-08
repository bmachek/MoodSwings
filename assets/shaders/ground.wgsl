// A kilometre of ground that is not one colour.
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
    // Unused; a uniform is padded to sixteen bytes whether or not it is
    // written that way, and naming the padding is cheaper than discovering it.
    pad: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> ground: GroundSettings;

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

fn vary(input: PbrInput) -> PbrInput {
    var pbr_input = input;
    let here = pbr_input.world_position.xz;

    // Two scales. The broad one is the field, the close one is what is going on
    // inside it; without the second, the plain is smooth blobs, which is a
    // different wrong answer from a flat green but not a better one.
    let broad = spread(fbm(here / ground.tile));
    let close = fbm(here / (ground.tile * 0.23));
    let field = broad * 0.66 + close * 0.34;

    let shade = 1.0 + (field - 0.5) * 2.0 * ground.value;
    // And the high ground goes off. Grass that has dried is not paler green, it
    // is a different colour — yellow, and much less saturated — so this is a hue
    // shift rather than another multiplier, and it is the half of this that
    // actually reads. Value alone does not: measured over a plain, a third
    // either way in linear light comes back as a tenth of a code either way in
    // the image, because sRGB spends most of its range below mid grey and grass
    // sits above it. Hue has no such compression.
    let parched = smoothstep(0.50, 0.76, broad) * ground.dry;

    var color = pbr_input.material.base_color.rgb * shade;
    color = mix(color, color * vec3(1.42, 1.15, 0.52), parched);
    pbr_input.material.base_color = vec4(color, pbr_input.material.base_color.a);

    // Damp ground is a little glossier than dry, which is most of what makes a
    // hollow read as a hollow from a distance.
    pbr_input.material.perceptual_roughness = clamp(
        pbr_input.material.perceptual_roughness - (0.5 - field) * 0.16,
        0.2,
        1.0,
    );

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
