// Standing water on the road.
//
// Wetness used to be one number applied to the whole road material: darken it,
// drop its roughness, done. That is right about what water does and wrong about
// where it is. A street does not go uniformly glossy in the rain — it puddles,
// in the ruts and the settled patches and against the kerb, and the parts
// between stay damp matte. A uniformly polished road reads as varnish.
//
// The mask has to be computed in world space. The road is a single quad forty
// kilometres across with its UVs multiplied by about six thousand, so anything
// sampled in UV repeats every six metres — puddles on a six-metre grid are a
// pattern, not weather. World space costs nothing extra here and the puddles
// come out metres across, which is the size they actually are.
//
// This runs in both pipelines for the same reason `facade.wgsl` does: the road
// is opaque, so it shades through the g-buffer, and screen-space reflections
// read that g-buffer. A puddle whose low roughness never reached the g-buffer
// would reflect nothing at all, which is the entire point of it.

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

struct RoadSettings {
    // How wet the road is overall, 0 to 1.
    wetness: f32,
    // Metres across one repeat of the puddle field.
    tile: f32,
    // Seconds, for the ripple. Held at zero when it is not actually raining, so
    // a merely damp road is still rather than trembling.
    time: f32,
    // How hard the rain is falling, which is what decides ripple strength.
    fall: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> road: RoadSettings;

// Value noise on a wrapping lattice, matching `world::texture`'s so the two
// agree about what a metre of grain looks like.
fn hash2(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    // Smoothstep rather than linear, or the lattice shows as a diamond grid.
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
    var at = p;
    for (var octave = 0; octave < 4; octave += 1) {
        sum += value_noise(at) * amplitude;
        at *= 2.0;
        amplitude *= 0.5;
    }
    return sum;
}

// How deep the water is here, 0 to 1.
//
// Thresholded rather than used raw: water finds a level, so a puddle has an
// edge. A smooth gradient of wetness across the road is what varnish looks
// like, and the whole reason for this function is not to produce one.
fn depth(world_position: vec2<f32>, wetness: f32) -> f32 {
    let low = fbm(world_position / road.tile);
    // Rising wetness floods progressively more of the road: at a drizzle only
    // the lowest patches hold water, and by the time it is pouring most of the
    // surface is under it.
    let level = mix(0.72, 0.28, wetness);
    return smoothstep(level, level + 0.13, low);
}

// Metres across one repeat of the largest scale of road wear — the patch a
// utility dug up and made good, the bay that was resurfaced on its own.
const PATCH_TILE: f32 = 34.0;
// How far a patch is allowed to move the road's value either way.
const PATCH: f32 = 0.30;
// Metres across one repeat of the crack field, and how much of that field is
// actually cracked.
const CRACK_TILE: f32 = 4.2;
const CRACK_LINE: f32 = 0.982;
// Metres across the field that decides *where* the road is cracked at all.
const CRACK_AREA: f32 = 26.0;

// Ages the road.
//
// One scan tiled at six metres gives a surface that is correct everywhere and
// the same everywhere: stand in the middle of a junction and the tarmac fifty
// metres down each of the four streets is the identical grey. Real tarmac is
// not, and not because of anything at the scale of the aggregate — it is
// because a road is a patchwork. It was laid in bays, dug up for a main and
// made good in a slightly different mix, sealed along the joins, and cracked
// where the ground moved under it.
//
// So a second field, an order of magnitude larger than the texture, in world
// space where it belongs. It costs no texture fetch: the noise is already here
// for the puddles, which is also why the two are tuned to different tile sizes —
// wear at the size of the puddles would read as one thing, not two.
fn age(input: PbrInput) -> PbrInput {
    var pbr_input = input;
    let here = pbr_input.world_position.xz;

    // The patchwork. A generous smoothstep either side of the middle, so most
    // of the road is near its own colour and the made-good bays have edges.
    let field = fbm(here / PATCH_TILE);
    let mend = (smoothstep(0.34, 0.44, field) + smoothstep(0.66, 0.56, field) - 1.0);
    let value = 1.0 + mend * PATCH;

    // Cracks, drawn as a ridge through a higher-frequency field and cut off
    // hard. A soft threshold gives tarmac rivers rather than a crack — the same
    // lesson `texture::asphalt_height` records, at ten times the size, because
    // a crack in a road runs for metres and the ones in the scan run for
    // centimetres.
    let ridge = 1.0 - abs(fbm(here / CRACK_TILE) * 2.0 - 1.0);
    // Gated by a third, slower field, because a ridge through smooth noise runs
    // unbroken for as far as the noise does — which came out as a single line
    // wandering the length of a street and reading as a cable somebody had
    // dropped rather than as a crack. A road cracks in patches, where the ground
    // under *that bit* moved, and everywhere else is intact.
    let cracked = smoothstep(0.46, 0.62, fbm(here / CRACK_AREA));
    let crack = smoothstep(CRACK_LINE, 1.0, ridge) * cracked;

    pbr_input.material.base_color = vec4(
        pbr_input.material.base_color.rgb * value * (1.0 - crack * 0.45),
        pbr_input.material.base_color.a,
    );
    // A fresh patch is blacker *and* less worn, so it is rougher; the old
    // surface around it has been polished by tyres. And a crack is a hole,
    // which reflects nothing at all.
    pbr_input.material.perceptual_roughness = clamp(
        pbr_input.material.perceptual_roughness + mend * 0.12 + crack * 0.25,
        0.04,
        1.0,
    );

    return pbr_input;
}

// How much of the scan's relief survives, looking straight down at the road and
// looking along it.
const RELIEF_FACE_ON: f32 = 0.70;
const RELIEF_GRAZING: f32 = 0.10;

// Lays the asphalt's relief back down as the view goes flat along it.
//
// A road is the pathological case for normal mapping and it took a while to
// recognise why. Its albedo is four percent — almost nothing comes back off it
// by diffusion — so nearly all of what the eye sees down a street is the sky,
// reflected. That reflection is the specular term, and at a grazing angle the
// specular term is both very large and very sensitive to the exact normal.
//
// The scan's normal map varies by more than a pixel's footprint can average, so
// the reflection became a coin toss: one pixel lands on a chipping angled to
// catch the sky and the next on one angled away, five times darker. On screen
// that is dense dark speckle over grey, thickest right in front of the camera
// where the view is most grazing, thinning into the distance where the mip chain
// does the averaging instead. It reads as broken rendering, and it is —
// the surface is under-sampled.
//
// Widening the specular lobe to cover the missing samples is the textbook answer
// and it was tried first; it made the picture worse, because at a grazing angle
// a wider GGX lobe loses energy rather than spreading it, so the speckle became
// a general dimming. What works is the honest observation underneath: a
// millimetre of asphalt relief seen edge-on does not tilt the reflection, it
// *occludes* it, and the average of a tilt this small over a whole pixel is the
// plane. So the relief is laid down as the view flattens — which is also what a
// normal map's own mip chain would do, if a mip chain could know which way the
// camera was looking.
fn settle(input: PbrInput) -> PbrInput {
    var pbr_input = input;

    // One at a bird's-eye view of the road, zero looking along it.
    let facing = saturate(dot(pbr_input.V, pbr_input.world_normal));
    let relief = mix(RELIEF_GRAZING, RELIEF_FACE_ON, facing);
    pbr_input.N = normalize(mix(pbr_input.world_normal, pbr_input.N, relief));

    return pbr_input;
}

fn wet(input: PbrInput) -> PbrInput {
    var pbr_input = input;
    if road.wetness <= 0.001 {
        return pbr_input;
    }

    let here = pbr_input.world_position.xz;
    let pool = depth(here, road.wetness);

    // Two states, blended. Damp asphalt is darker and a little glossier;
    // standing water is much darker and close to a mirror. Interpolating
    // between them rather than scaling one is what keeps the puddle edge
    // visible instead of washing it into a gradient.
    let damp = road.wetness * 0.35;
    let soak = max(damp, pool * road.wetness);

    let darken = 1.0 - soak * 0.55;
    pbr_input.material.base_color = vec4(
        pbr_input.material.base_color.rgb * darken,
        pbr_input.material.base_color.a,
    );

    // Not to zero. Water lying on asphalt still has the road's texture under
    // it, and a true mirror finish reads as sheet ice rather than as a puddle.
    pbr_input.material.perceptual_roughness =
        mix(pbr_input.material.perceptual_roughness, 0.08, soak);

    // Water fills the texture it is lying in. Where it is deep, the surface
    // light reflects off is the top of the water, not the aggregate under it,
    // so the road's own normal map has to be flattened back towards the plane.
    //
    // This is not a nicety. Dropping the roughness without doing it leaves the
    // asphalt's relief driving a near-mirror, and every grain of chipping
    // throws its own highlight: the road comes out glittering like crushed
    // glass, which is exactly what the first pass looked like. Damp asphalt
    // keeps its texture; a puddle does not have one.
    pbr_input.N = normalize(mix(pbr_input.N, pbr_input.world_normal, pool * road.wetness));

    // Rings, only where there is actually water to ring and only while it is
    // still falling. Two sets at different rates, because one is visibly a
    // single expanding pattern.
    if road.fall > 0.01 && pool > 0.01 {
        let ripple_a = sin(dot(here, vec2(5.7, 4.1)) - road.time * 9.0);
        let ripple_b = sin(dot(here, vec2(-3.9, 6.3)) - road.time * 12.7);
        let shake = (ripple_a + ripple_b) * 0.5 * road.fall * pool * 0.035;
        let tilt = vec3(shake, 0.0, shake * 0.7);
        pbr_input.N = normalize(pbr_input.N + tilt);
    }

    return pbr_input;
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let in = vertex_output;

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color =
        alpha_discard(pbr_input.material, pbr_input.material.base_color);

    // Order: what the road is, then how flat it looks from here, then what is
    // lying on it. Water goes last because it covers everything under it — a
    // puddle over a crack is a puddle, and the wetness pass is the one that
    // knows that.
    pbr_input = age(pbr_input);
    pbr_input = settle(pbr_input);
    pbr_input = wet(pbr_input);

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
