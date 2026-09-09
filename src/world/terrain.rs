//! The shape of the ground, and the one rule that makes it safe.
//!
//! The world was a plane. Not "nearly flat" — one quad every four hundred and
//! forty-eight metres, four and a half of them across the whole town, every
//! vertex at exactly y = 0. From a rooftop the horizon was a ruled line with a
//! haze band over it, and from the air the town sat on a green table. No amount
//! of shading fixes that: `ground.wgsl` can vary the colour of a plane all it
//! likes and it is still a plane, because what is missing is not hue, it is
//! *silhouette*.
//!
//! ## The rule
//!
//! Roughly thirty places in this codebase write a world y directly —
//! `SIDEWALK_HEIGHT` for a pavement, `SIDEWALK_HEIGHT + STAND_HEIGHT` for a
//! citizen, `resting_height(spec)` for a parked car, a bare `0.0` for a pigeon
//! that is not on a kerb — and every one of them means "the ground here is at
//! zero". Rewriting all thirty to ask a height function, and every collider and
//! every spawn with them, is a different and much larger change than this one,
//! and it is a change that fails *silently* wherever it is missed: a building
//! half a metre in the air, a car sunk to its axles, and no test that can tell.
//!
//! So the ground is displaced under exactly one condition:
//!
//! > **[`Terrain::height`] returns exactly zero anywhere anything is built.**
//!
//! That is not a convention, it is arithmetic. The relief is multiplied by a
//! field rasterised from the road graph itself — the same corridors
//! `world::urban_mask` stamps — which is one along every street, every
//! pavement and every plot fronting them, and falls to zero only out in the
//! back land where nothing stands. Past the town the field is zero everywhere
//! and the relief is free to become hills.
//!
//! Landshut is the reason that is not a compromise. It is a town on a valley
//! floor with the Hofberg behind it: the floor really is flat and the relief
//! really does start where the houses stop. The rule and the postcard happen to
//! want the same ground.
//!
//! ## Two fields, at two scales
//!
//! The gentle one lives inside the town, in the back land and the middle of
//! the blocks, and it is measured in *centimetres* — enough that a lawn between
//! two terraces catches a gradient across it instead of being one flat green,
//! and small enough that nothing standing on it can be seen to float.
//!
//! The far one is the landscape: five octaves at a nine-hundred-metre scale,
//! ramping in past the edge of the played extent and reaching fifty metres of
//! amplitude a couple of kilometres out. That is what puts a skyline behind the
//! roofs.
//!
//! ## Determinism
//!
//! `hash2`/`value_noise`/`fbm` here are the same integer-hash family
//! `world::sky` and `sky.wgsl` share — deliberately not the `fract(sin(...))`
//! family in `ground.wgsl` and `road.wgsl`, which cannot have a CPU twin at
//! all, because `sin` of a large argument is implementation-defined across GPU
//! vendors. Nothing here draws from an RNG; the field is keyed with
//! `rng::key_for` like the weather and the canal, because where a hill is is a
//! fact about the seed and finding one must never move a lot.

use bevy::prelude::*;

use super::citygen::{CityLayout, SIDEWALK_WIDTH};

/// How far from the edge of a pavement the ground is still held dead level.
///
/// A plot handed out by `streetside` is at most nineteen metres deep and a
/// building's apron reaches seven tenths past it, so twenty-eight metres of
/// corridor covers everything that stands on a street with room to spare. It is
/// a little less than `world::BACKLAND`, which is what the *shading* mask uses
/// — the two are asking different questions and it would be a coincidence if
/// they wanted the same number.
const LEVEL_REACH: f32 = 28.0;

/// And how far past that it takes to reach full relief.
///
/// Long, because this is a slope: the ground rises out of the flat over a
/// distance rather than at a line. Fifty metres over a gentle field measured in
/// centimetres is a gradient nobody can see start.
const LEVEL_FADE: f32 = 50.0;

/// Metres across one repeat of the gentle field, and how far it moves the
/// ground.
///
/// Forty centimetres, which is the whole of what is allowed inside the town.
/// It has to be small for the reason the module docs give: an object placed at
/// a bare `0.0` sits on the plane, not on this, and the field is what decides
/// how wrong that is allowed to look. Forty centimetres over a seventy-metre
/// wavelength is a gradient of about one in ninety — a lawn that is not
/// perfectly flat, and nothing that could tip a bin.
const GENTLE_TILE: f32 = 70.0;
const GENTLE_RISE: f32 = 0.40;

/// Metres across one repeat of the landscape, and how high it gets.
///
/// Seventy-eight metres of amplitude on a ridged field, which averages half
/// of it: about forty metres of hill, and the Hofberg above the real Landshut
/// is sixty-five. It has to be this big to survive the haze — the town reads
/// through two kilometres of aerial perspective as a pale band, and a ten-metre
/// rise in it is not a hill, it is a slightly uneven horizon.
const RELIEF_TILE: f32 = 900.0;
const RELIEF_RISE: f32 = 78.0;

/// Where the landscape starts and where it is at full height, as a distance
/// from the middle of the world along whichever axis is further.
///
/// The *square* distance, not the radius, because everything it has to stay
/// clear of is square: the graph is clipped to `world.half_extent` on each
/// axis, and so is the ground collider. Measured as a radius, the ramp reached
/// inside the corners of the town — a street at (1000, 1000) is fourteen
/// hundred metres from the middle and would have found itself on a hillside.
///
/// The first bound is outside everything the layout can reach: the graph is
/// clipped to `world.half_extent`, the ground collider covers that plus a
/// hundred metres, and the streaming radius is shorter than either. Nothing
/// the player can stand on is inside the ramp.
const RELIEF_START: f32 = 1_150.0;
const RELIEF_FULL: f32 = 2_600.0;

/// Texels a side of the level field.
///
/// Coarser than the shading mask, and it can be: what this resolves is a
/// twenty-eight metre corridor with a fifty-metre fade on it, and at five
/// hundred and twelve over two and a half kilometres a texel is five metres.
/// The field is bilinear-sampled, so the corridor's edge is smooth at a good
/// deal finer than a texel.
const FIELD_SIZE: usize = 512;

/// The ground's shape, held whole.
///
/// A resource rather than a free function because the level field is
/// rasterised from the road graph once at startup and has to be *the same one*
/// the mesh, the collider and every spawner ask — a second copy built from the
/// same inputs would be identical today and a bug the first time one of them
/// is given a different reach.
#[derive(Resource)]
pub struct Terrain {
    /// One where the ground must be dead level, falling to zero out in the
    /// open. Row-major, `FIELD_SIZE` square, covering `±reach`.
    level: Vec<f32>,
    /// Half the width of the world the field covers, in metres.
    reach: f32,
    /// Whether the gentle field runs at all. A generated grid city paves its
    /// whole footprint with block slabs laid at a fixed height, so there is no
    /// open ground inside it to roll — only the landscape past it.
    gentle: bool,
    /// The seed's own offset into the noise, so two seeds are two landscapes.
    offset: Vec2,
}

impl Terrain {
    /// Rasterises the level field from the layout.
    ///
    /// Stamped rather than sampled, for the same reason `world::urban_mask` is:
    /// asking every texel for its distance to every street is five hundred
    /// squared times two thousand. Each street visits only the texels its own
    /// corridor can reach.
    pub fn new(city: &CityLayout, reach: f32, gentle: bool, seed: u64) -> Self {
        let started = std::time::Instant::now();
        let per_texel = reach * 2.0 / FIELD_SIZE as f32;
        let at = |index: usize| -reach + (index as f32 + 0.5) * per_texel;

        let mut level = vec![0.0f32; FIELD_SIZE * FIELD_SIZE];
        for edge in city.graph.edges() {
            let a = city.graph.node(edge.a).pos;
            let b = city.graph.node(edge.b).pos;
            let corridor = edge.width * 0.5 + SIDEWALK_WIDTH;
            let span = corridor + LEVEL_REACH + LEVEL_FADE;
            let low = (a.min(b) - Vec2::splat(span) + Vec2::splat(reach)) / per_texel;
            let high = (a.max(b) + Vec2::splat(span) + Vec2::splat(reach)) / per_texel;
            let x0 = (low.x.floor().max(0.0) as usize).min(FIELD_SIZE - 1);
            let x1 = (high.x.ceil().max(0.0) as usize).min(FIELD_SIZE - 1);
            let z0 = (low.y.floor().max(0.0) as usize).min(FIELD_SIZE - 1);
            let z1 = (high.y.ceil().max(0.0) as usize).min(FIELD_SIZE - 1);
            let run = b - a;
            let length_squared = run.length_squared().max(1e-6);
            for z in z0..=z1 {
                let world_z = at(z);
                for x in x0..=x1 {
                    let point = Vec2::new(at(x), world_z);
                    let t = ((point - a).dot(run) / length_squared).clamp(0.0, 1.0);
                    let beyond = (point.distance(a + run * t) - corridor - LEVEL_REACH).max(0.0);
                    // Smooth, not linear: this field is a multiplier on a
                    // height and a linear ramp leaves a crease along its own
                    // edge that catches the light like a kerb nobody built.
                    let value = 1.0 - smoothstep(beyond / LEVEL_FADE);
                    let cell = &mut level[z * FIELD_SIZE + x];
                    *cell = cell.max(value);
                }
            }
        }
        info!(
            "level field rasterised in {:.1}ms at {FIELD_SIZE}²",
            started.elapsed().as_secs_f32() * 1000.0
        );

        // A seed's own corner of the noise. Hashed rather than sliced: the
        // seeds 1 and 2 differ in one bit, and an offset taken straight off the
        // key moved the landscape by a third of a metre against a
        // nine-hundred-metre feature — which is to say two seeds shared a
        // ridge line. Forty kilometres of offset either way is far enough that
        // no two seeds see the same hill and small enough to stay exact in an
        // `f32`.
        //
        // Keyed rather than drawn, because where a hill is is a fact about the
        // seed and finding one must never move a lot.
        let key = crate::core::rng::key_for(seed, crate::core::rng::stream::TERRAIN);
        let (low, high) = (key as i32, (key >> 32) as i32);
        let offset = Vec2::new(
            hash2(low, high) * 80_000.0 - 40_000.0,
            hash2(high, low ^ 0x5f3d_759d) * 80_000.0 - 40_000.0,
        );
        Self {
            level,
            reach,
            gentle,
            offset,
        }
    }

    /// A terrain with no field in it: the flat world, for a test and for a
    /// city that never asked for one.
    pub fn flat() -> Self {
        Self {
            level: vec![1.0; FIELD_SIZE * FIELD_SIZE],
            reach: 1.0,
            gentle: false,
            offset: Vec2::ZERO,
        }
    }

    /// How much of the ground here is held level: one on a street, zero out in
    /// the open. Bilinear, and clamped at the border — past the field there is
    /// no town, so there is nothing to hold flat.
    pub fn level_at(&self, at: Vec2) -> f32 {
        let uv =
            (at / (self.reach * 2.0) + Vec2::splat(0.5)) * FIELD_SIZE as f32 - Vec2::splat(0.5);
        let base = uv.floor();
        let f = uv - base;
        let sample = |x: f32, y: f32| -> f32 {
            let x = (x as i32).clamp(0, FIELD_SIZE as i32 - 1) as usize;
            let y = (y as i32).clamp(0, FIELD_SIZE as i32 - 1) as usize;
            self.level[y * FIELD_SIZE + x]
        };
        let a = sample(base.x, base.y);
        let b = sample(base.x + 1.0, base.y);
        let c = sample(base.x, base.y + 1.0);
        let d = sample(base.x + 1.0, base.y + 1.0);
        a.lerp(b, f.x).lerp(c.lerp(d, f.x), f.y)
    }

    /// The height of the ground at a point, in metres.
    pub fn height(&self, at: Vec2) -> f32 {
        let p = at + self.offset;
        let mut height = 0.0;

        if self.gentle {
            let level = self.level_at(at);
            if level < 0.999 {
                // Centred on zero, so the flat town is the *mean* of the
                // rolling ground rather than its floor — a floor would put a
                // step up all the way round the built-up area.
                height += (fbm(p / GENTLE_TILE) - 0.5) * 2.0 * GENTLE_RISE * (1.0 - level);
            }
        }

        // Square distance, not radius — see [`RELIEF_START`]. Held off wherever
        // the level field speaks at all, so that a lane running out past the
        // ramp takes its own flat ground with it rather than climbing a hill.
        let out =
            smoothstep((at.abs().max_element() - RELIEF_START) / (RELIEF_FULL - RELIEF_START))
                * (1.0 - self.level_at(at));
        if out > 0.0 {
            // Ridged rather than plain: `1 - |2n - 1|` turns a blobby field
            // into one with crests and long shallow flanks, which is what a
            // wooded valley side looks like from a town in the bottom of it.
            let n = fbm(p / RELIEF_TILE);
            let ridged = 1.0 - (n * 2.0 - 1.0).abs();
            height += ridged * RELIEF_RISE * out;
        }
        height
    }

    /// The surface normal, by finite difference over a `step`-metre cross.
    ///
    /// A step rather than an analytic derivative because the level field is a
    /// bilinear texture lookup and its derivative is discontinuous at every
    /// texel edge — differencing over a distance longer than a texel gives the
    /// normal the *surface* has rather than the one the interpolation has.
    pub fn normal(&self, at: Vec2, step: f32) -> Vec3 {
        let dx = self.height(at + Vec2::new(step, 0.0)) - self.height(at - Vec2::new(step, 0.0));
        let dz = self.height(at + Vec2::new(0.0, step)) - self.height(at - Vec2::new(0.0, step));
        Vec3::new(-dx, 2.0 * step, -dz).normalize()
    }
}

/// Hermite ease over 0..1, clamped.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `world::sky::hash2`, `world::texture::hash` and `hash2` in `sky.wgsl`. A
/// fourth copy of eight lines, and it is the *integer* family on purpose: the
/// `fract(sin(dot(p, k)) * 43758.5453)` in `ground.wgsl` and `road.wgsl` cannot
/// have a CPU twin, because `sin` of a large argument is implementation-defined
/// across GPU vendors and the mesh and the shader would disagree about where
/// the ground is.
fn hash2(x: i32, y: i32) -> f32 {
    let mut h =
        (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ 0xC2B2_AE3D;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_F491);
    h ^= h >> 13;
    h as f32 / u32::MAX as f32
}

fn value_noise(p: Vec2) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let (x, y) = (i.x as i32, i.y as i32);
    let a = hash2(x, y);
    let b = hash2(x + 1, y);
    let c = hash2(x, y + 1);
    let d = hash2(x + 1, y + 1);
    a.lerp(b, u.x).lerp(c.lerp(d, u.x), u.y)
}

fn fbm(p: Vec2) -> f32 {
    let mut sum = 0.0;
    let mut amplitude = 0.5;
    let mut total = 0.0;
    let mut at = p;
    for _ in 0..5 {
        sum += value_noise(at) * amplitude;
        total += amplitude;
        at = at * 2.17 + Vec2::new(19.3, -7.1);
        amplitude *= 0.52;
    }
    sum / total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::CityStyle;
    use crate::world::citygen;

    const HALF_EXTENT: f32 = 1_000.0;
    const REACH: f32 = HALF_EXTENT + 240.0;

    fn town() -> (citygen::CityLayout, Terrain) {
        let layout = citygen::generate(7, HALF_EXTENT, CityStyle::Landshuepf);
        let terrain = Terrain::new(&layout, REACH, true, 7);
        (layout, terrain)
    }

    /// The rule the whole module rests on. Roughly thirty spawners write a
    /// world y directly and mean "the ground is at zero"; if the ground is not
    /// at zero under a street, every one of them is wrong at once.
    #[test]
    fn the_ground_under_a_street_is_exactly_flat() {
        let (layout, terrain) = town();
        let mut worst: f32 = 0.0;
        for edge in layout.graph.edges() {
            let a = layout.graph.node(edge.a).pos;
            let b = layout.graph.node(edge.b).pos;
            for step in 0..=4 {
                let along = a.lerp(b, step as f32 / 4.0);
                // The carriageway, both pavements, and out to the back of the
                // deepest plot `streetside` hands out.
                for across in [0.0f32, 0.5, 1.0] {
                    let normal = (b - a).perp().normalize_or_zero();
                    let out = edge.width * 0.5 + SIDEWALK_WIDTH + across * 19.0;
                    for side in [-1.0f32, 1.0] {
                        worst = worst.max(terrain.height(along + normal * (out * side)).abs());
                    }
                }
            }
        }
        assert!(
            worst < 1.0e-4,
            "the ground under the town moves by {worst} m, and everything built on it assumes zero"
        );
    }

    /// And it is not flat everywhere, or none of this was worth doing.
    #[test]
    fn the_land_around_the_town_is_not_a_table() {
        let (_, terrain) = town();
        let far = terrain.height(Vec2::new(2_600.0, 900.0));
        assert!(
            far > 4.0,
            "the land two and a half kilometres out is {far} m above the valley floor"
        );
        // And it gets there without a cliff: the ramp is two kilometres long.
        let step = 40.0;
        let mut steepest: f32 = 0.0;
        for ring in 0..90 {
            let r = RELIEF_START - 200.0 + ring as f32 * step;
            for spoke in 0..24 {
                let angle = spoke as f32 / 24.0 * std::f32::consts::TAU;
                let at = Vec2::from_angle(angle) * r;
                let next = Vec2::from_angle(angle) * (r + step);
                steepest = steepest.max((terrain.height(next) - terrain.height(at)).abs() / step);
            }
        }
        // A wooded valley side runs to about thirty degrees and the Hofberg
        // above Landshut is steeper than that in places. What this is guarding
        // against is a *step* — a ramp that arrives all at once, which is what
        // a linear blend into the relief would give.
        assert!(
            steepest < 0.75,
            "the landscape reaches a gradient of {steepest}, which is a cliff not a hill"
        );
    }

    /// The open ground inside the town rolls, but only by centimetres — an
    /// object standing on it is placed at a bare zero and the field is what
    /// decides how wrong that looks.
    #[test]
    fn the_back_land_rolls_by_less_than_a_step() {
        let (_, terrain) = town();
        let mut worst: f32 = 0.0;
        let mut moved = false;
        for x in -30..=30 {
            for z in -30..=30 {
                let at = Vec2::new(x as f32 * 30.0, z as f32 * 30.0);
                if at.length() > 900.0 {
                    continue;
                }
                let height = terrain.height(at).abs();
                worst = worst.max(height);
                moved |= height > 0.02;
            }
        }
        assert!(moved, "the ground inside the town never moves at all");
        assert!(
            worst <= GENTLE_RISE + 1.0e-4,
            "the back land moves by {worst} m, which is more than a kerb"
        );
    }

    /// A generated grid city paves its own footprint, so it gets the landscape
    /// and nothing else.
    #[test]
    fn a_paved_city_keeps_a_level_floor() {
        let layout = citygen::generate(7, HALF_EXTENT, CityStyle::NewDork);
        let terrain = Terrain::new(&layout, REACH, false, 7);
        for x in -20..=20 {
            for z in -20..=20 {
                let at = Vec2::new(x as f32 * 50.0, z as f32 * 50.0);
                assert_eq!(terrain.height(at), 0.0, "the grid city moved at {at}");
            }
        }
    }

    /// Two seeds are two landscapes, and one seed is always the same one —
    /// the whole world regenerates its chunks from the seed, so a hill that
    /// moved between visits would be a hill that moved while you looked at it.
    #[test]
    fn a_seed_is_a_landscape() {
        let layout = citygen::generate(1, HALF_EXTENT, CityStyle::Landshuepf);
        let a = Terrain::new(&layout, REACH, true, 1);
        let b = Terrain::new(&layout, REACH, true, 1);
        let c = Terrain::new(&layout, REACH, true, 2);
        let probe = Vec2::new(2_100.0, -1_700.0);
        assert_eq!(a.height(probe), b.height(probe));
        assert!(
            (a.height(probe) - c.height(probe)).abs() > 0.5,
            "two seeds built the same hill"
        );
    }
}
