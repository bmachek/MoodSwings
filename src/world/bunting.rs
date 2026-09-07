//! Lines strung across a street: pennants, and somebody's washing.
//!
//! Everything else in this world happens at ground level. Look up from a back
//! street in any European city and there is something overhead — a line of
//! triangles left from a festival nobody has taken down, a sheet on a pulley
//! between two windows — and its absence is one of the quieter reasons a
//! generated city reads as a model of one. It is also the only decoration in
//! the game that occupies the air, which means it is the only one that a
//! flummi launched off a bonnet goes *through*.
//!
//! ## Only across the narrow ones
//!
//! A line is anchored to the two building lines either side, so its span is the
//! carriageway plus both pavements. On a minor street that is sixteen metres,
//! which is a rope; on an arterial it is twenty-three, which is a suspension
//! bridge, and there is a reason nobody strings bunting across a dual
//! carriageway. Minor streets only, and the width is checked rather than
//! assumed — the generator's road classes may move.
//!
//! ## The sag is the whole thing
//!
//! A line drawn straight between two points reads as a cable, not as a rope
//! with things hanging off it. The curve here is a parabola rather than a real
//! catenary, which for a sag of a tenth of the span is a difference of
//! millimetres and about forty lines of arithmetic.
//!
//! Each hanging thing is a *pivot* at its point on the rope with the pennant or
//! the shirt below it, so swaying is a rotation about the rope's own axis
//! applied to the pivot — one quaternion per item per frame, and the item hangs
//! from its top edge the way real washing does rather than pirouetting about
//! its middle.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::buildings::ChunkOf;
use super::citygen::SIDEWALK_WIDTH;
use super::roadgraph::RoadEdge;
use crate::core::schedule::GameSet;

/// How far a line is drawn. Generous: it is eight metres up with nothing in
/// front of it, which makes it one of the few things in the city that is still
/// legible from the far end of a street.
pub const RANGE: f32 = 140.0;

/// Chance a given minor street has something strung across it.
const CHANCE: f32 = 0.085;

/// Widest carriageway anybody would string a line over.
const MAX_WIDTH: f32 = 12.5;

/// How high the anchors sit above the pavement.
const HEIGHT: (f32, f32) = (6.2, 8.6);

/// Sag at the middle, as a fraction of the span. A tenth is a rope somebody
/// pulled tight; a fifth is a rope somebody gave up on.
const SAG: f32 = 0.085;

/// How many segments the rope is drawn in, and how many things hang off it.
const SEGMENTS: usize = 8;

/// Degrees of swing at a stiff breeze, in radians.
const SWING: f32 = 0.42;
/// Wind speed, in metres per second, at which the swing is at full.
const GALE: f32 = 9.0;

// A line spans the carriageway plus both pavements, so the widest street this
// touches has to come out as something a person could plausibly have thrown a
// rope over — and the check has to exclude an arterial on width alone even if
// the arterial flag were ever dropped, which at seventeen metres it does.
const _: () = assert!(MAX_WIDTH < 17.0);
const _: () = assert!(MAX_WIDTH + SIDEWALK_WIDTH * 2.0 < 20.0);
// Nothing swings over the top of its own line: washing that goes past a right
// angle is washing that has come off it.
const _: () = assert!(SWING < std::f32::consts::FRAC_PI_2);
// With a sag of a tenth of the span, four segments put a visible corner at the
// bottom of the rope. Eight is where it stops reading as folded.
const _: () = assert!(SEGMENTS >= 6);

#[derive(Resource)]
pub struct BuntingKit {
    /// A unit cylinder for the rope, and a unit cube for the washing.
    rope: Handle<Mesh>,
    cloth: Handle<Mesh>,
    /// A cone, stood on its head, which is a pennant.
    pennant: Handle<Mesh>,
    hemp: Handle<StandardMaterial>,
    /// Festival colours, and the pale end of a laundry basket.
    flags: Vec<Handle<StandardMaterial>>,
    linen: Vec<Handle<StandardMaterial>>,
}

/// One pennant or shirt, hanging from its point on the rope.
///
/// The entity this is on is the *pivot* — the point on the rope — and the thing
/// itself is its child, hanging below. Rotating the pivot swings the item from
/// its top edge; rotating the item would spin it about its own middle, which is
/// a thing washing has never done.
#[derive(Component)]
pub struct Hanging {
    /// This item's offset into the swing, so a line does not wave in unison
    /// like a stadium crowd.
    phase: f32,
    /// How much this kind moves. A triangle of nylon is not a wet towel.
    give: f32,
    /// The pose it hangs in with no wind at all.
    rest: Quat,
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> BuntingKit {
    let cloth = |materials: &mut Assets<StandardMaterial>, color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.94,
            // Both faces: a flag seen from behind is still a flag, and a
            // single-sided one vanishes as you walk under it.
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    };
    BuntingKit {
        rope: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(5).build()),
        cloth: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        pennant: meshes.add(Cone::new(1.0, 1.0).mesh().resolution(3).build()),
        hemp: materials.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.27, 0.22),
            perceptual_roughness: 1.0,
            ..default()
        }),
        flags: [
            Color::srgb(0.78, 0.16, 0.14),
            Color::srgb(0.94, 0.74, 0.16),
            Color::srgb(0.16, 0.42, 0.66),
            Color::srgb(0.20, 0.52, 0.28),
            Color::srgb(0.88, 0.88, 0.85),
        ]
        .into_iter()
        .map(|color| cloth(materials, color))
        .collect(),
        linen: [
            Color::srgb(0.90, 0.90, 0.88),
            Color::srgb(0.72, 0.78, 0.85),
            Color::srgb(0.86, 0.80, 0.70),
            Color::srgb(0.62, 0.66, 0.70),
        ]
        .into_iter()
        .map(|color| cloth(materials, color))
        .collect(),
    }
}

/// Strings a line across one street, if this street has one.
#[allow(clippy::too_many_arguments)]
pub fn spawn_edge(
    commands: &mut Commands,
    kit: &BuntingKit,
    rng: &mut ChaCha8Rng,
    edge: &RoadEdge,
    from: Vec2,
    to: Vec2,
    chunk: IVec2,
    range: f32,
) {
    if edge.arterial || edge.width > MAX_WIDTH || edge.length < 20.0 {
        return;
    }
    if rng.random_range(0.0..1.0) > CHANCE {
        return;
    }
    let Ok(direction) = Dir2::new(to - from) else {
        return;
    };
    let normal = Vec2::new(-direction.y, direction.x);

    // Anchored on the two building lines, so the span is the carriageway plus
    // both pavements.
    let reach = edge.width * 0.5 + SIDEWALK_WIDTH - 0.4;
    let along = rng.random_range(0.2..0.8) * edge.length;
    let middle = from + *direction * along;
    let height = rng.random_range(HEIGHT.0..HEIGHT.1);
    let span = reach * 2.0;
    let sag = span * SAG;

    // Pennants twice as often as washing: bunting is left up for months and a
    // sheet is out for an afternoon.
    let festival = rng.random_range(0.0..1.0) < 0.66;

    let visibility = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: range..(range * 1.1),
        use_aabb: false,
    };

    // The rope's shape: a parabola through both anchors, sampled once and used
    // for both the segments and the things hanging off them, so a pennant
    // cannot end up a hand's breadth off its own rope.
    let point = |i: usize| {
        let t = i as f32 / SEGMENTS as f32;
        let across = (t * 2.0 - 1.0) * reach;
        // Nought at both ends, one in the middle.
        let dip = 1.0 - (t * 2.0 - 1.0).powi(2);
        let at = middle + normal * across;
        Vec3::new(at.x, height - sag * dip, at.y)
    };

    for i in 0..SEGMENTS {
        let (a, b) = (point(i), point(i + 1));
        let run = b - a;
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.rope.clone()),
            MeshMaterial3d(kit.hemp.clone()),
            Transform::from_translation(a.midpoint(b))
                .with_rotation(Quat::from_rotation_arc(Vec3::Y, run.normalize()))
                .with_scale(Vec3::new(0.012, run.length(), 0.012)),
            visibility.clone(),
        ));
    }

    // The rope runs along the street's normal, so that is the axis everything
    // hanging off it swings about.
    let axis = Vec3::new(normal.x, 0.0, normal.y);
    let lie = Quat::from_rotation_arc(Vec3::X, axis);

    for i in 1..SEGMENTS {
        let at = point(i);
        let (mesh, material, size, give) = if festival {
            (
                kit.pennant.clone(),
                kit.flags[rng.random_range(0..kit.flags.len())].clone(),
                Vec3::new(0.15, 0.34, 0.015),
                1.0,
            )
        } else {
            (
                kit.cloth.clone(),
                kit.linen[rng.random_range(0..kit.linen.len())].clone(),
                Vec3::new(0.42, 0.56, 0.012),
                0.45,
            )
        };

        let drop = size.y * 0.5;
        commands
            .spawn((
                ChunkOf(chunk),
                Hanging {
                    phase: rng.random_range(0.0..std::f32::consts::TAU),
                    give,
                    rest: lie,
                },
                Transform::from_translation(at).with_rotation(lie),
                Visibility::default(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    // Hanging below the pivot. A cone points up and a pennant
                    // points down, so the festival kind is turned over.
                    Transform::from_xyz(0.0, -drop, 0.0)
                        .with_rotation(if festival {
                            Quat::from_rotation_z(std::f32::consts::PI)
                        } else {
                            Quat::IDENTITY
                        })
                        .with_scale(size),
                    visibility.clone(),
                ));
            });
    }
}

pub struct BuntingPlugin;

impl Plugin for BuntingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sway.in_set(GameSet::Simulation));
    }
}

/// The wind, and everything hanging in it.
fn sway(
    time: Res<Time>,
    weather: Res<super::weather::Weather>,
    mut hanging: Query<(&Hanging, &mut Transform)>,
) {
    // A breeze that is always there. A line dead still on a clear day reads as
    // a line somebody forgot to animate, and the sky in this game is never
    // completely without weather anyway.
    let strength = (0.18 + weather.wind_speed() / GALE).min(1.0);
    let clock = time.elapsed_secs();

    for (item, mut transform) in &mut hanging {
        // Two frequencies, badly out of step, which is the cheapest thing that
        // does not read as a metronome.
        let t = clock * (1.4 + item.give) + item.phase;
        let wave = t.sin() * 0.7 + (t * 1.61).sin() * 0.3;
        transform.rotation = item.rest * Quat::from_rotation_x(wave * SWING * strength * item.give);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rope hangs, and it hangs from both ends.
    #[test]
    fn the_line_sags_in_the_middle_and_meets_its_anchors() {
        let reach = 8.0f32;
        let height = 7.0f32;
        let sag = reach * 2.0 * SAG;
        let y = |t: f32| height - sag * (1.0 - (t * 2.0 - 1.0).powi(2));

        assert!(
            (y(0.0) - height).abs() < 1e-5,
            "the left anchor has slipped"
        );
        assert!(
            (y(1.0) - height).abs() < 1e-5,
            "the right anchor has slipped"
        );
        assert!((y(0.5) - (height - sag)).abs() < 1e-5, "the middle is flat");
        // And it is monotone down to the middle and back up, or the rope has a
        // kink in it.
        for i in 0..8 {
            let (a, b) = (i as f32 / 16.0, (i + 1) as f32 / 16.0);
            assert!(y(a) > y(b), "the rope rises between {a} and {b}");
        }
    }

    /// It moves even on a still day, and it never whirls.
    #[test]
    fn the_swing_is_always_alive_and_never_wild() {
        for wind in [0.0f32, 3.0, 9.0, 40.0] {
            let strength = (0.18 + wind / GALE).min(1.0);
            assert!(
                strength > 0.1,
                "at {wind} m/s the line is nailed to the sky"
            );
            assert!(strength <= 1.0, "at {wind} m/s the line is a propeller");
        }
    }
}
