//! The material the road surface is drawn with.
//!
//! A [`StandardMaterial`] extended with standing water. See
//! `assets/shaders/road.wgsl` for the reasoning; briefly, wetness used to be
//! one number applied to the whole road — darken it, drop its roughness — which
//! is right about what water does and wrong about where it is. Rain puddles. A
//! road that goes uniformly glossy reads as varnish.
//!
//! The mask is computed in world space rather than in the mesh's UV, for the
//! same reason the facade's grain is: the road's UVs repeat every six metres,
//! so a mask sampled in them gives puddles on a six-metre grid, which is a
//! pattern rather than weather. (The ground used to be one quad forty
//! kilometres across with its UVs scaled by six thousand, which was worse than
//! a pattern — see `world::CELL_REPEATS` for what that did to the mip
//! selection.)
//!
//! This is also where wetness stops being a material mutation. `WetSurfaces`
//! still recomputes the pavement's colour and roughness each time the weather
//! moves, because a pavement really does just go uniformly damp; the road's
//! wetness is a uniform the shader reads, so it varies per fragment and costs
//! one buffer write per change rather than a pass over the materials.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use super::weather::Weather;

const SHADER: &str = "shaders/road.wgsl";

/// Metres across one repeat of the puddle field.
///
/// Puddles want to be the size of the dips that hold them — a couple of metres
/// across on a road that has settled, not the six-metre grid a UV-space mask
/// would give and not the fifty-metre lakes a much larger figure would.
const PUDDLE_TILE: f32 = 9.5;

// Puddles want to be the size of the dips that hold them. Below the asphalt
// tile this stops being weather and becomes a texture pattern; far above it,
// the road floods in lakes. Checked at compile time rather than in a test,
// because both sides are constants and a test would only ever be re-running
// the compiler's arithmetic.
const _: () = assert!(PUDDLE_TILE > super::ASPHALT_TILE);
const _: () = assert!(PUDDLE_TILE < 30.0);

#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
pub struct RoadSettings {
    pub wetness: f32,
    pub tile: f32,
    /// Seconds. Held at zero when it is not raining, so a merely damp road is
    /// still rather than trembling.
    pub time: f32,
    /// How hard it is falling, which is what decides ripple strength. Not
    /// inferred from `wetness`: a road left glossy after a shower must be still,
    /// and that is a state the weather has and a single dial did not.
    pub fall: f32,
    /// How much of the asphalt ageing this surface takes, 0 to 1.
    ///
    /// One for tarmac, zero for anything else. The patches and cracks the
    /// shader draws are things that happen to a *poured* surface: it was laid
    /// in bays, dug up for a main and made good in a different mix, and it
    /// cracks where the ground moves under it. A cobbled street does none of
    /// that — its joints take the movement, which is why it has joints — so
    /// running the same wear over setts paints tar seams across granite.
    ///
    /// Everything else in this shader applies to any carriageway: standing
    /// water finds the low spots on a market square exactly as it does on a
    /// road, and the relief has to lie down at a grazing angle whatever it is
    /// relief of.
    pub wear: f32,
    /// How coarse this surface's relief is, 0 to 1.
    ///
    /// The shader lays a carriageway's normal map flat as the view goes flat
    /// along it, because asphalt's relief is a millimetre of chipping and a
    /// millimetre of chipping seen edge-on does not tilt a reflection, it
    /// occludes it — see `settle` in `road.wgsl`. That is right for tarmac and
    /// wrong for setts: the joint between two cobbles is two centimetres deep
    /// and fifteen apart, and the shadow in it is most of what a cobbled street
    /// *is* when you look down one. So the coarser the paving, the more of its
    /// relief survives the grazing angle.
    pub relief: f32,
    /// How far this surface has settled over what is buried under it, in
    /// metres.
    ///
    /// A road is not a plane. It is a lid over trenches and made-good bays, and
    /// it dips a centimetre or two over each of them at about the length of a
    /// car — which is why a wet street reflects in bands rather than evenly,
    /// and why standing water is where it is. Every carriageway settles,
    /// including a cobbled one; what differs is how much.
    pub sag: f32,
    /// How pitted it is, 0 to 1.
    ///
    /// Asphalt pots and granite does not, and that is the same argument
    /// [`Self::wear`] makes: a pothole is what happens when water gets under a
    /// *poured* surface and freezes. A sett has joints for exactly that, which
    /// is why it has joints, so a cobbled street loses a stone rather than
    /// growing a crater.
    pub pits: f32,
}

impl Default for RoadSettings {
    fn default() -> Self {
        Self {
            wetness: 0.0,
            tile: PUDDLE_TILE,
            time: 0.0,
            fall: 0.0,
            wear: 1.0,
            relief: 0.0,
            sag: 0.020,
            pits: 1.0,
        }
    }
}

/// How far a carriageway's middle stands above its gutters, as a fraction of
/// its half-width.
///
/// Two per cent. A real crossfall is two to two and a half, and it is there so
/// the road drains — which is exactly why it is worth having here: it is what
/// puts the standing water at the kerb rather than in the middle of the lane,
/// and it is why a wet street reflects in two bands and not one sheet.
pub const CROSSFALL: f32 = 0.014;

/// The most a crown may rise, in metres.
///
/// The Altstadt is thirteen metres wide and a market square is wider still;
/// unbounded, the crossfall would put a fifteen-centimetre hump down the middle
/// of it, which is a hump a car climbs rather than a road that drains. Real
/// wide streets crown less, not more.
pub const CROWN_CAP: f32 = 0.045;

// Checked where it is written down rather than in a test, the way `layer`'s
// stack is: a crown is what a car's tyre has to hide, because traffic rides the
// flat ground collider under the road rather than the road itself. Past about a
// seventh of a wheel it stops reading as camber and starts reading as a rut.
const _: () = assert!(CROWN_CAP < 0.05);

/// How far a crown takes to flatten out into a junction, in metres.
///
/// A crossing is flat: it has to be, because two crowns meeting at right
/// angles is a saddle, and a pedestrian crossing painted over a saddle is
/// four stripes at four heights. Real junctions are laid flat and drained to
/// their corners for the same reason.
pub const CROWN_FLAT: f32 = 9.0;

/// How high a carriageway stands above its own bed, `across` metres from its
/// centreline.
///
/// A parabola, which is what a road is: the crown is not a roof with a ridge,
/// it is a continuous curve, and the tell of getting it wrong is a hard line
/// down the middle of the street where the two planes meet.
pub fn crown(width: f32, across: f32) -> f32 {
    let half = (width * 0.5).max(0.1);
    let u = (across / half).abs().min(1.0);
    (half * CROSSFALL).min(CROWN_CAP) * (1.0 - u * u)
}

/// The slope of that curve, for the surface normal.
///
/// Analytic rather than sampled, because these normals are the whole point:
/// what a two-centimetre crown actually shows at eye height is not its
/// silhouette, it is the two halves of the road catching the sky differently.
pub fn crown_slope(width: f32, across: f32) -> f32 {
    let half = (width * 0.5).max(0.1);
    if across.abs() >= half {
        return 0.0;
    }
    (half * CROSSFALL).min(CROWN_CAP) * -2.0 * across / (half * half)
}

/// How much of the crown survives this far along a street, and how fast that
/// is changing.
///
/// `flat` says which of the two ends is a junction. A bend is not: a street
/// that merely turns keeps its crown right through the turn, which matters
/// here because six nodes in seven of a town read off a map are bends and the
/// median segment between them is under fourteen metres. Flattening at every
/// node would leave a town with no crown anywhere.
pub fn crown_fade(along: f32, length: f32, flat: (bool, bool)) -> (f32, f32) {
    let reach = CROWN_FLAT.min(length * 0.5);
    if reach <= 0.0 {
        return (1.0, 0.0);
    }
    let mut level = 1.0f32;
    let mut rate = 0.0f32;
    for (at_end, distance) in [(flat.0, along), (flat.1, length - along)] {
        if !at_end || distance >= reach {
            continue;
        }
        let t = (distance / reach).clamp(0.0, 1.0);
        // Smoothstep, so the crown does not arrive at the junction with a
        // crease across it.
        let eased = t * t * (3.0 - 2.0 * t);
        if eased < level {
            level = eased;
            // d/d(along) of the smoothstep, signed by which end it is.
            let slope = 6.0 * t * (1.0 - t) / reach;
            rate = if at_end && distance == along {
                slope
            } else {
                -slope
            };
        }
    }
    (level, rate)
}

/// The standing-water half of the road material.
#[derive(Asset, AsBindGroup, Reflect, Clone, Default)]
pub struct RoadSheen {
    #[uniform(100)]
    pub settings: RoadSettings,
}

impl MaterialExtension for RoadSheen {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    /// The same file, branching on `PREPASS_PIPELINE`. This one is not optional
    /// in the way the forward path is: screen-space reflections read the
    /// g-buffer, so a puddle whose low roughness never got written into it
    /// would reflect nothing — which is the entire point of the puddle.
    fn deferred_fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub type RoadMaterial = ExtendedMaterial<StandardMaterial, RoadSheen>;

pub struct RoadPlugin;

impl Plugin for RoadPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<RoadMaterial>::default())
            .add_systems(Update, soak_the_road);
    }
}

/// Pushes the current weather into the road material.
///
/// One uniform write when the weather moves, against `WetSurfaces`' pass over
/// every registered material. The clock is only advanced while it is actually
/// raining: a dry road holding a nonzero time would keep re-uploading the
/// uniform every frame for a ripple nobody can see.
fn soak_the_road(
    weather: Res<Weather>,
    time: Res<Time>,
    mut materials: ResMut<Assets<RoadMaterial>>,
    roads: Query<&MeshMaterial3d<RoadMaterial>>,
) {
    let wetness = weather.wetness.clamp(0.0, 1.0);
    let fall = weather.rain.clamp(0.0, 1.0);

    for handle in &roads {
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        let settings = &mut material.extension.settings;

        if fall > 0.0 {
            settings.time = time.elapsed_secs();
        }
        if (settings.wetness - wetness).abs() > f32::EPSILON
            || (settings.fall - fall).abs() > f32::EPSILON
        {
            settings.wetness = wetness;
            settings.fall = fall;
        }
    }
}

#[cfg(test)]
mod tests {
    /// A road drains to its gutters, and does it as a curve.
    #[test]
    fn a_carriageway_stands_highest_down_its_middle() {
        let width = 7.5f32;
        let ridge = crown(width, 0.0);
        assert!(
            (0.02..=CROWN_CAP).contains(&ridge),
            "a {width} m street crowns {ridge} m"
        );
        // Zero at the kerb line and never negative: the crown is built upward
        // from the road bed, because the ground under this town is one plane
        // fourteen millimetres below it and anything cut lower comes out as a
        // stripe of back-land grit down the gutter.
        assert_eq!(crown(width, width * 0.5), 0.0);
        assert_eq!(crown(width, width), 0.0);
        for step in 0..40 {
            let across = step as f32 / 39.0 * width;
            assert!(crown(width, across) >= 0.0);
            assert_eq!(crown(width, across), crown(width, -across));
        }
        // A curve, not a roof: no hard line down the middle. The gradient at
        // the crown is zero and grows away from it.
        assert!(crown_slope(width, 0.0).abs() < 1e-6);
        assert!(crown_slope(width, 1.0) < crown_slope(width, 0.5));
        assert!(crown_slope(width, -1.0) > 0.0);
        // And it matches the height it claims to differentiate.
        let step = 0.01;
        for across in [-3.0f32, -1.0, 0.5, 2.0] {
            let measured =
                (crown(width, across + step) - crown(width, across - step)) / (2.0 * step);
            assert!(
                (measured - crown_slope(width, across)).abs() < 1e-3,
                "at {across} m the slope says {} and the height says {measured}",
                crown_slope(width, across)
            );
        }
    }

    /// A wide street crowns less steeply, not more.
    #[test]
    fn a_market_square_is_not_a_hump() {
        // Up to the cap a wider street crowns higher, because the crossfall is
        // a gradient and it has further to fall.
        assert!(crown(6.0, 0.0) > crown(4.0, 0.0));
        // Past it they are all the same, which is the point of having one: the
        // Altstadt is thirteen metres wide and a market square is wider still,
        // and an honest crossfall across one of those is a hump a car climbs
        // rather than a road that drains. Real wide streets crown less.
        for width in [12.0f32, 20.0, 30.0] {
            assert_eq!(crown(width, 0.0), CROWN_CAP, "{width} m");
        }
    }

    /// The crown flattens into a junction and runs straight through a bend.
    #[test]
    fn a_crown_flattens_into_a_crossing_but_not_into_a_corner() {
        let length = 60.0;
        // A bend at both ends: full crown from end to end, because six nodes in
        // seven of a town read off a map are bends and the median segment
        // between them is under fourteen metres. Flattening at every node would
        // leave the town with no crown at all.
        for along in [0.0f32, 5.0, 30.0, 55.0, 60.0] {
            assert_eq!(crown_fade(along, length, (false, false)).0, 1.0);
        }
        // A junction at the start only.
        let flat = (true, false);
        assert_eq!(
            crown_fade(0.0, length, flat).0,
            0.0,
            "not flat at the crossing"
        );
        assert!(crown_fade(CROWN_FLAT * 0.5, length, flat).0 > 0.1);
        assert_eq!(crown_fade(CROWN_FLAT, length, flat).0, 1.0);
        assert_eq!(
            crown_fade(length, length, flat).0,
            1.0,
            "the far end is a bend"
        );
        // Monotonic on the way in, so there is no crease across the approach.
        let mut last = -1.0;
        for step in 0..=20 {
            let level = crown_fade(step as f32 / 20.0 * CROWN_FLAT, length, flat).0;
            assert!(level >= last, "the fade went backwards at step {step}");
            last = level;
        }
        // A street shorter than two fade lengths still crowns somewhere in the
        // middle rather than being flat end to end.
        let short = 10.0;
        assert!(crown_fade(short * 0.5, short, (true, true)).0 > 0.5);
    }

    use super::*;

    /// The puddle field and the asphalt are two patterns laid over the same
    /// surface, and the road only reads as road while they are on different
    /// scales. Both sides are constants, so the interesting half of this is the
    /// compile-time assertion above; this checks the value that reaches the GPU.
    #[test]
    fn a_fresh_road_is_dry_and_still() {
        let settings = RoadSettings::default();
        assert_eq!(settings.wetness, 0.0);
        assert_eq!(settings.fall, 0.0);
        assert_eq!(settings.time, 0.0);
        assert_eq!(settings.tile, PUDDLE_TILE);
    }
}
