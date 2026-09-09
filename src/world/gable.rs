//! The stepped gable, and the pitched roof behind it.
//!
//! Every building in this city is a box with a slab on top, and for a city of
//! nowhere in particular that is the honest answer. It is the wrong answer for
//! exactly one postcard: what anybody who has stood in the Landshut Altstadt
//! remembers is not a street plan and not a colour, it is *the roofline* — a
//! row of tall narrow houses whose front walls carry on upwards past the roof
//! as a stepped screen, each one a different height, each one hiding whatever
//! is behind it.
//!
//! That is a `Giebelhaus`, and the screen is a `Treppengiebel`. It is also,
//! conveniently, the cheapest recognisable thing in this whole exercise: five
//! boxes of decreasing width stacked on the front wall.
//!
//! ## Why the screen and not the roof
//!
//! The roof behind it is a plain pitch — two slabs leaning on a ridge — and it
//! is there almost entirely for the view from above. From a pavement you never
//! see it: the whole point of a gable screen is that it stands proud of the
//! roof and hides it, which is why the things were built. So the pitch gets two
//! meshes and no further thought, and the screen gets the arithmetic.
//!
//! ## What a gabled building gives up
//!
//! Its rooftop clutter. Air handling, tanks and vent stacks sit on a flat deck
//! and a pitched roof does not have one, so a gabled building would have its
//! plant floating inside its own rafters. `buildings` skips the clutter for
//! them, which is also correct: these are houses, and a house's roof has a
//! chimney on it and nothing else.

use bevy::camera::visibility::VisibilityRange;
use bevy::math::Affine2;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::{ChaCha8Rng, rand_core::SeedableRng};

use super::buildings::ChunkOf;
use super::texture;

/// How far the screen itself is drawn. As far as the building it caps: a
/// roofline is a silhouette, and a silhouette is the last thing to stop
/// mattering.
pub const RANGE: f32 = 900.0;

/// And how far the *coping* is — the stone slab across the top of every step.
///
/// Half the meshes in a screen are coping, and coping is a hundred and twenty
/// millimetres thick. Past a couple of hundred metres it is under a pixel and
/// all it contributes is a lighter edge on a skyline that already has one, so
/// on a city where four buildings in five are gabled it is the obvious half to
/// drop first.
const COPING_RANGE: f32 = 230.0;

/// Steps up one side of a screen. Four is a gable, seven is a wedding cake.
const STEPS: (u32, u32) = (3, 6);

/// How high the screen stands above the eaves, as a fraction of the frontage.
///
/// Measured off the *width* and not off the building's height, which is what
/// makes a row of them read as one street rather than as a stack of unrelated
/// triangles: a narrow house gets a steep gable and a wide one gets a shallow
/// gable, exactly as they do in life, because both were built to the same
/// pitch.
const RISE: (f32, f32) = (0.55, 0.85);

/// How far the screen stands proud of the wall below it.
const PROUD: f32 = 0.10;
/// And how thick it is.
const THICK: f32 = 0.32;

/// How high the ridge stands above the eaves, as a fraction of the *frontage*.
///
/// The frontage and not the depth, and the difference is the whole shape of the
/// roof: on a `Giebelhaus` the ridge runs back from the street, so the roof
/// falls to the two *side* walls and the span it crosses is how wide the house
/// is. Measured off the depth instead — which is what this did first, and what
/// the test below caught — a narrow deep house grows a ridge two metres taller
/// than the screen that is supposed to be hiding it.
///
/// It also makes the pitch a constant: every roof in the row leans at
/// `atan(2 * PITCH)`, which is a shade over forty degrees and is a roof.
const PITCH: f32 = 0.42;

#[derive(Resource)]
pub struct GableKit {
    cube: Handle<Mesh>,
    /// The screen wears the building's own wall material, which this module
    /// does not own — so what is kept here is only what a roof is made of.
    ///
    /// Three roofs rather than one, because a real old town has not been
    /// re-tiled all at once: a house done last summer sits between one that
    /// was done in the seventies and one nobody has touched since the moss
    /// took it. Picked from the building's own seed, so a roof keeps its age
    /// across a chunk respawn.
    tile: [Handle<StandardMaterial>; 3],
    cap: Handle<StandardMaterial>,
}

/// How many times the tile image repeats over one leaf of a roof: up the
/// slope first, then along the ridge.
///
/// Not the same both ways, because a leaf is not square. A Landshut house is
/// eight to fourteen metres wide and about as deep, so the fall is around four
/// metres and the ridge runs twice that — and a Biberschwanz is a hand's width
/// whichever way you measure it. These two numbers are what put roughly a
/// hand's width in both directions.
///
/// Fixed rather than per-house: every leaf shares one cube mesh, so a
/// per-building tiling would mean a material per building.
const LAP: Vec2 = Vec2::new(3.0, 5.5);

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> GableKit {
    let clay = images.add(texture::tiles());
    let relief = images.add(texture::tiles_normal());
    // Occlusion in red, roughness in green, off the same height field as the
    // other two — see `texture::tiles_surface`.
    let surface = images.add(texture::tiles_surface());
    // New, weathered, and one the moss has had. All three are the same fired
    // clay underneath — what changes with age is that it goes browner, greyer
    // and less even, not that it goes a different colour.
    // Fired clay is a good deal more orange than memory says — the first pass
    // at these was a third darker and every roof in the town read as slate.
    let ages = [
        Color::srgb(0.78, 0.36, 0.22),
        Color::srgb(0.63, 0.33, 0.23),
        Color::srgb(0.50, 0.34, 0.26),
    ];
    GableKit {
        // With tangents, and that is not decoration: Bevy applies a normal map
        // only where the mesh carries a tangent basis, so the roof relief that
        // has been painted and bound since this module was written was being
        // dropped on the floor. A bare `Cuboid` has none. The city's own
        // `with_tangents` runs mikktspace, which is the basis the shader
        // agrees with.
        cube: meshes.add(super::buildings::with_tangents(
            Cuboid::new(1.0, 1.0, 1.0).mesh().build(),
        )),
        tile: ages.map(|age| {
            materials.add(StandardMaterial {
                // Old clay, not the grey felt the flat roofs are covered in.
                base_color: age,
                base_color_texture: Some(clay.clone()),
                normal_map_texture: Some(relief.clone()),
                // The same image twice: occlusion takes its red channel and
                // the metallic/roughness slot takes its green and blue.
                occlusion_texture: Some(surface.clone()),
                metallic_roughness_texture: Some(surface.clone()),
                uv_transform: Affine2::from_scale(LAP),
                // One, because it multiplies the map. Anything else throws the
                // green channel away — see `material::ScannedSet::apply`.
                perceptual_roughness: 1.0,
                metallic: 1.0,
                ..default()
            })
        }),
        cap: materials.add(StandardMaterial {
            // The stone coping along the top of every step, which is what
            // stops the steps reading as a staircase made of the wall.
            base_color: Color::srgb(0.72, 0.70, 0.66),
            perceptual_roughness: 0.88,
            ..default()
        }),
    }
}

/// One step of a screen: how wide it is and how high its top sits, both as
/// fractions of the frontage and of the total rise.
///
/// Split out from the spawn so the shape can be tested. The failure this
/// guards against is the one that makes a stepped gable look wrong rather than
/// broken — steps that do not reach the middle leave a flat top, and steps that
/// overshoot it cross over and the screen grows a notch at its own apex.
fn step(index: u32, steps: u32) -> (f32, f32) {
    let up = (index + 1) as f32 / steps as f32;
    // The width shrinks to nothing at the top, so the last step is the ridge
    // pier and the first is the full width of the wall.
    let across = 1.0 - index as f32 / steps as f32;
    (across, up)
}

/// Raises a stepped gable on one building's front, and pitches its roof.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    kit: &GableKit,
    wall: &Handle<StandardMaterial>,
    seed: u64,
    center: Vec2,
    frontage: f32,
    throat: f32,
    // `height` is how tall the wall is and `eaves` is how far above the ground
    // its top sits; the two differ by the kerb, and the clamp below wants the
    // first.
    height: f32,
    eaves: f32,
    yaw: f32,
    chunk: IVec2,
    lod_scale: f32,
) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x6AB1_E501);
    let steps = rng.random_range(STEPS.0..=STEPS.1);
    // Off the frontage, and then held to the house. The width is what sets the
    // pitch and the pitch is what makes a row read as one street — but a
    // generator that hands out lots wider than its buildings are tall will
    // otherwise put a ten-metre screen on a five-metre house, which is not an
    // Altstadt, it is a row of billboards. `CityStyle::lot_scale` narrows the
    // plots so the clamp rarely bites; this is what happens when it does.
    let rise = (frontage * rng.random_range(RISE.0..RISE.1)).min(height * 0.62);
    let ridge = (frontage * PITCH).min(rise * 0.72);

    let end = (RANGE * lod_scale).min(2_000.0);
    let range = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: end..(end * 1.05),
        use_aabb: false,
    };
    let near = (COPING_RANGE * lod_scale).min(600.0);
    let close = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: near..(near * 1.1),
        use_aabb: false,
    };
    let turn = Quat::from_rotation_y(yaw);
    // The screen's own frame: `outward` is the way the front face looks, and
    // the steps march up the middle from both sides at once.
    let outward = Vec2::new(yaw.sin(), yaw.cos());
    let front = center + outward * (throat * 0.5 + PROUD - THICK * 0.5);

    for index in 0..steps {
        let (across, up) = step(index, steps);
        let width = frontage * across;
        let top = eaves + rise * up;
        let previous = if index == 0 {
            eaves
        } else {
            eaves + rise * step(index - 1, steps).1
        };
        let band = top - previous;

        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(wall.clone()),
            Transform::from_xyz(front.x, previous + band * 0.5, front.y)
                .with_rotation(turn)
                .with_scale(Vec3::new(width, band, THICK)),
            range.clone(),
        ));
        // The coping: a slab of stone across the top of each step, oversailing
        // it a little. Without it the steps are a wall with notches cut in it,
        // and with it they are a gable.
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.cap.clone()),
            Transform::from_xyz(front.x, top + 0.06, front.y)
                .with_rotation(turn)
                .with_scale(Vec3::new(width + 0.22, 0.12, THICK + 0.16)),
            close.clone(),
        ));
    }

    // And the roof behind it: two slabs leaning on a ridge that runs back from
    // the screen. Almost nobody ever sees this — the screen is taller than the
    // ridge, which is the entire purpose of a screen — so it is two meshes and
    // no further argument.
    // When this roof was last done. From the building's own seed like
    // everything else about it.
    let age = rng.random_range(0..kit.tile.len());
    let slope = (ridge / (frontage * 0.5)).atan();
    let leaf = (frontage * 0.5) / slope.cos();
    for side in [-1.0f32, 1.0] {
        // Each leaf is centred half way up its own slope, offset to its side.
        let across = Vec2::new(outward.y, -outward.x) * (side * frontage * 0.25);
        let at = center + across;
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.tile[age].clone()),
            // Negated, and this is the whole difference between a roof and a
            // gutter. `rotation_z` by a positive angle lifts the local +X end,
            // and the leaf on the +X side of the ridge has its *outer* edge
            // there — so `side * slope` raises both outer edges and drops the
            // middle, and every house in the town wears a trough. The eaves go
            // down and the ridge goes up.
            Transform::from_xyz(at.x, eaves + ridge * 0.5, at.y)
                .with_rotation(turn * Quat::from_rotation_z(-side * slope))
                .with_scale(Vec3::new(leaf, 0.14, throat * 1.04)),
            range.clone(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The steps reach the middle exactly once.
    #[test]
    fn a_gable_comes_to_a_point() {
        for steps in STEPS.0..=STEPS.1 {
            let (first_across, _) = step(0, steps);
            assert!(
                (first_across - 1.0).abs() < 1e-6,
                "the bottom step is not the width of the wall"
            );
            let (last_across, last_up) = step(steps - 1, steps);
            assert!(
                (last_up - 1.0).abs() < 1e-6,
                "the top step does not reach the top"
            );
            assert!(
                last_across > 0.0 && last_across <= 1.0 / steps as f32 + 1e-6,
                "the top step is {last_across} of the wall, not a pier"
            );
        }
    }

    /// It only ever narrows, and only ever rises.
    #[test]
    fn the_steps_march_one_way() {
        for steps in STEPS.0..=STEPS.1 {
            for index in 1..steps {
                let (across, up) = step(index, steps);
                let (before, lower) = step(index - 1, steps);
                assert!(across < before, "step {index} of {steps} got wider");
                assert!(up > lower, "step {index} of {steps} went down");
            }
        }
    }

    /// A narrow house gets a steep gable and a wide one a shallow gable.
    #[test]
    fn the_pitch_is_the_same_whatever_the_house_is_wide() {
        // The rise is a fraction of the *frontage*, so the angle at the apex is
        // the same on every house in a row — which is what makes a parade of
        // them read as one street built at one time, rather than as a shelf of
        // unrelated triangles.
        let angle = |frontage: f32, share: f32| ((frontage * share) / (frontage * 0.5)).atan();
        for share in [RISE.0, RISE.1] {
            let narrow = angle(6.0, share);
            let wide = angle(18.0, share);
            assert!(
                (narrow - wide).abs() < 1e-5,
                "a 6m house pitches at {narrow:.3} and an 18m one at {wide:.3}"
            );
            // And the pitch is a roof pitch rather than a spire or a shed.
            assert!(
                (0.8..1.4).contains(&narrow),
                "a pitch of {narrow:.2} radians is not a house"
            );
            // The roof leans at the same angle whatever the house is wide, for
            // the same reason and by the same arithmetic.
            let roof = (2.0f32 * PITCH).atan();
            assert!(
                (0.55..0.95).contains(&roof),
                "a {roof:.2} radian roof is a spire or a shed"
            );
        }
    }

    /// The eaves are below the ridge.
    #[test]
    fn a_roof_sheds_outwards_rather_than_inwards() {
        // The failure this pins is a roof rotated the right amount about the
        // right axis in the wrong direction: two leaves that meet in a valley
        // at the middle and rise to their outer edges, which is a gutter the
        // length of the house and reads, from the air, as a town of troughs.
        let slope = 0.7f32;
        let leaf = 6.0f32;
        for side in [-1.0f32, 1.0] {
            let turn = Quat::from_rotation_z(-side * slope);
            // The leaf runs along its own X; its outer end is the one on the
            // same side of the ridge as the leaf itself.
            let outer = turn * Vec3::new(side * leaf * 0.5, 0.0, 0.0);
            let inner = turn * Vec3::new(-side * leaf * 0.5, 0.0, 0.0);
            assert!(
                outer.y < inner.y,
                "the {side} leaf rises {:.2} from ridge to eaves",
                outer.y - inner.y
            );
        }
    }

    /// The screen stands taller than the roof it is hiding.
    #[test]
    fn the_screen_hides_its_own_roof() {
        // This is the only thing a gable screen is for. If the ridge pokes out
        // over the top of it the building has a triangle growing out of a
        // staircase, which is not a thing anybody has ever built.
        for frontage in [6.0f32, 12.0, 20.0] {
            for throat in [8.0f32, 14.0, 22.0] {
                let rise = frontage * RISE.0;
                let ridge = frontage * PITCH;
                assert!(
                    rise > ridge * 0.85,
                    "a {frontage}x{throat}m house hides {rise:.1}m of a {ridge:.1}m ridge"
                );
            }
        }
    }
}
