// Weather in the sky.
//
// Bevy's atmosphere is a scattering model, and a scattering model has no clouds
// in it: the dome overhead comes out a clean gradient at every hour and in every
// weather, which is both the single largest surface in most frames and the one
// that says most loudly that nobody is home. Everything else the cover does —
// the sun dimming, the shadows going soft, the haze closing in — was already
// right. There was just nothing up there to have caused it.
//
// So this draws a deck of cloud on the inside of a dome that follows the camera.
// Not volumetric: one horizontal layer, hit by the view ray, with the noise
// field sampled where the ray crosses it. That is the oldest trick there is for
// this and it holds up for the same reason the interior mapping in `facade.wgsl`
// does — the thing being faked is a long way away, and parallax is all the eye
// was going to get from it anyway.
//
// Two details are the whole of whether it reads as cloud or as marble.
//
// The lighting is a *gradient*, not a level. A cloud is lit on the side facing
// the sun and shadowed on the other, and what tells you which is which is that
// the field gets denser going one way. So the field is sampled a second time,
// stepped towards the sun, and the difference between the two samples is the
// shading. It costs one more fbm and it is the difference between a cloud with
// a shape and a grey stain.
//
// And the horizon has to be given up on. A ray flat enough to be near it crosses
// tens of kilometres of deck, so the field compresses past what any number of
// samples can resolve and turns to noise; the cloud is faded out into the sky
// before it gets there. Real cloud does converge into haze down there, which is
// the excuse, but the reason is aliasing.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct SkySettings {
    // Cloud lit by the sun, and cloud in its own shadow, both in the same
    // nits the rest of the scene is lit in — this material is unlit, so what it
    // writes goes straight into an HDR buffer that a real exposure divides.
    // Anything normalised to one comes out black at noon.
    sunlit: vec4<f32>,
    shade: vec4<f32>,
    // Where the camera is, so the ray can start there. Passed rather than read
    // from the view binding because the dome is *moved* to the camera every
    // frame anyway, and the system doing the moving already has the number.
    eye: vec4<f32>,
    // Direction to the sun, and how far up the day is in `w`.
    sun: vec4<f32>,
    // How far the deck has blown, in metres. Accumulated on the CPU rather than
    // derived from a clock here: a shader that multiplies a rising time by a
    // wind speed loses its low bits within an hour of play and the cloud starts
    // to judder.
    drift: vec2<f32>,
    // 0 for a clear sky, 1 for a solid overcast.
    coverage: f32,
    // Metres from the ground to the base of the deck.
    height: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: SkySettings;

// Metres across one repeat of the cloud field.
const SCALE: f32 = 2600.0;
// How far towards the sun the second sample is taken, in metres. About the
// width of one cloud: much less and the two samples say the same thing, much
// more and the shading belongs to the next cloud along.
const SUN_STEP: f32 = 620.0;
// Where the cloud gives up and becomes haze, as a sine of the ray's elevation.
const HORIZON: f32 = 0.115;

// An integer hash, and the integers are the point.
//
// This was `fract(sin(dot(p, k)) * 43758.5453)`, which is the noise everybody
// writes and which cannot be reproduced off the GPU: `sin` at a large argument
// differs in its last bits between one implementation and another, and a hash
// amplifies a last-bit difference into a completely different number. That
// stopped mattering the moment the ground had to know where the clouds were —
// `world::sky::shade` evaluates this same field on the CPU to work out whether
// the sun is behind one, and a field that only agrees with itself on one of the
// two machines would dim the sun under a clear patch of sky.
//
// Integer arithmetic is exact everywhere. This is `world::texture::hash`, which
// the rest of the game's noise has always used, with the lattice cell taken as
// a signed integer and reinterpreted — the deck runs tens of kilometres either
// side of the origin, so the coordinates are very much signed.
fn hash2(cell: vec2<i32>) -> f32 {
    var h = bitcast<u32>(cell.x) * 0x9E3779B1u
        ^ bitcast<u32>(cell.y) * 0x85EBCA77u
        ^ 0xC2B2AE3Du;
    h ^= h >> 15u;
    h = h * 0x2545F491u;
    h ^= h >> 13u;
    return f32(h) / 4294967295.0;
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let cell = vec2<i32>(i);
    let a = hash2(cell);
    let b = hash2(cell + vec2<i32>(1, 0));
    let c = hash2(cell + vec2<i32>(0, 1));
    let d = hash2(cell + vec2<i32>(1, 1));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Five octaves, and the ratio is not two.
//
// A power-of-two lacunarity puts every octave's lattice on top of the last
// one's, and on a field this large the alignment shows as a faint grid of
// squares across the whole sky. An irrational-ish step costs nothing and has no
// alignment to show.
fn fbm(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var total = 0.0;
    var at = p;
    for (var octave = 0; octave < 5; octave += 1) {
        sum += value_noise(at) * amplitude;
        total += amplitude;
        at = at * 2.17 + vec2(19.3, -7.1);
        amplitude *= 0.52;
    }
    return sum / total;
}

// Where a ray leaving the eye crosses the deck, in metres, plus the drift.
fn deck(dir: vec3<f32>) -> vec2<f32> {
    let along = sky.height / max(dir.y, 0.02);
    return sky.eye.xz + dir.xz * along + sky.drift;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The dome is centred on the eye, so the ray to this fragment *is* the ray
    // through the sky at this pixel.
    let dir = normalize(in.world_position.xyz - sky.eye.xyz);

    // Nothing below the horizon, and a long fade into it. `smoothstep` twice
    // over: once for the geometry and once because a linear fade leaves a
    // visible line where it reaches zero.
    let up = smoothstep(0.0, HORIZON, dir.y);
    if up <= 0.0 {
        return vec4(0.0);
    }

    let here = deck(dir) / SCALE;
    let density = fbm(here);

    // Coverage is a threshold, not a multiplier. Cloud has edges: raising the
    // cover floods more of the field past the line rather than making the same
    // clouds more opaque, which is what an overcast actually does.
    let line = mix(0.615, 0.185, clamp(sky.coverage, 0.0, 1.0));
    let mass = smoothstep(line, line + 0.155, density);
    if mass <= 0.0 {
        return vec4(0.0);
    }

    // The second sample, stepped towards the sun across the deck. Where the
    // field rises that way this point is behind cloud and in its shadow; where
    // it falls, this is the side the light is on.
    let towards = normalize(vec2(sky.sun.x, sky.sun.z) + vec2(1e-4, 0.0));
    let ahead = fbm(here + towards * (SUN_STEP / SCALE));
    let facing = clamp(0.5 + (density - ahead) * 3.4, 0.0, 1.0);

    var color = mix(sky.shade.rgb, sky.sunlit.rgb, facing);

    // The silver lining: cloud scatters strongly forwards, so the edge of one
    // in front of the sun is brighter than its lit side ever gets. Tied to the
    // thin part of the mass, because that is where the light is coming through
    // rather than off.
    let toward_sun = clamp(dot(dir, sky.sun.xyz), 0.0, 1.0);
    let rim = pow(toward_sun, 24.0) * (1.0 - mass) * sky.sun.w;
    color += sky.sunlit.rgb * rim * 1.6;

    // The aperture, applied by hand. Bevy multiplies by the view's exposure
    // inside the lighting functions, and this material does not call them — so
    // an unlit shader writing radiance straight into the HDR buffer is writing
    // it in units nothing downstream will divide, and every cloud comes out a
    // flat clipped white at noon and stays white at midnight. Reading it from
    // the view rather than passing it in matters too: auto exposure adjusts it
    // in a compute pass, and the CPU-side value is only where the clock left it.
    color *= view.exposure;

    // Thin cloud is see-through, thick cloud is not, and the fade at the horizon
    // takes both.
    return vec4(color, mass * up);
}
