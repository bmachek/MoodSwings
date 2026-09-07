//! The parking garage, as a building with no walls.
//!
//! Every other building in the city is a facade with an inside implied; the
//! garage inverts that — it is *all* inside. Open decks on columns, a low
//! parapet around each, a wide mouth at street level, and switchback ramps
//! climbing the sides. The point of the whole structure is the ramp: a
//! bouncing car on a parking ramp is why the zoning pass bothered placing
//! four of these.
//!
//! Same door-frame convention as `world::interior`: `x` across the front,
//! `+z` towards the street, origin at the footprint's centre on the
//! pavement. `world::buildings` picks the front and hands in the yaw.
//!
//! No materials of its own: the decks wear the block paving and the
//! structure wears the kerb concrete, because a garage *is* pavement,
//! stacked. No visibility ranges either — a garage replaces its building's
//! whole silhouette, there are four in the city, and an open deck that
//! blinks out at distance would leave a hole in the skyline.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::buildings::{ChunkOf, CityAssets, SIDEWALK_HEIGHT};

/// Metres from one deck floor to the next.
const DECK: f32 = 3.1;
/// A deck slab's thickness.
const SLAB: f32 = 0.3;
/// The parapet a deck trusts to keep a car on it. Trusts.
const PARAPET: f32 = 0.95;
const PARAPET_THICK: f32 = 0.25;
/// The ramp's driveable width.
const RAMP_W: f32 = 5.0;
/// The street-level mouth a car drives in through.
const MOUTH: f32 = 8.0;
const COLUMN: f32 = 0.45;

/// Spawns one garage: decks, parapets, columns, ramps, mouth.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    assets: &CityAssets,
    center: Vec2,
    width: f32,
    depth: f32,
    height: f32,
    yaw: f32,
    chunk: IVec2,
) {
    let spin = Quat::from_rotation_y(yaw);
    let decks = ((height / DECK) as usize).clamp(2, 5);

    let piece = |commands: &mut Commands,
                 material: Handle<StandardMaterial>,
                 at: Vec3,
                 size: Vec3,
                 pitch: f32| {
        let world = spin * at + Vec3::new(center.x, SIDEWALK_HEIGHT, center.y);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(assets.unit_cube.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(world)
                .with_rotation(spin * Quat::from_rotation_x(pitch))
                .with_scale(size),
            RigidBody::Static,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));
    };
    let concrete = || assets.concrete();

    // Which side the ramp up from deck `k` climbs: alternating, so the climb
    // through the building is a switchback.
    let ramp_side = |k: usize| if k.is_multiple_of(2) { -1.0f32 } else { 1.0 };
    let ramp_run = depth - 2.0 * PARAPET_THICK;
    let pitch = (DECK / ramp_run).atan();

    for deck in 0..decks {
        let floor = deck as f32 * DECK;

        if deck > 0 {
            // The slab, minus the strip the arriving ramp needs open. The
            // strip sits on the side the ramp below climbs.
            let open = ramp_side(deck - 1);
            let solid = width - RAMP_W;
            piece(
                commands,
                assets.paving_for(width),
                Vec3::new(-open * RAMP_W * 0.5, floor - SLAB * 0.5, 0.0),
                Vec3::new(solid, SLAB, depth),
                0.0,
            );
        }

        // The parapet ring. At street level the front leaves the mouth open;
        // everywhere else the ring closes, and what the parapet cannot hold
        // back was always going to fly.
        let rail = floor + PARAPET * 0.5;
        for side in [-1.0f32, 1.0] {
            piece(
                commands,
                concrete(),
                Vec3::new(side * (width - PARAPET_THICK) * 0.5, rail, 0.0),
                Vec3::new(PARAPET_THICK, PARAPET, depth),
                0.0,
            );
        }
        piece(
            commands,
            concrete(),
            Vec3::new(0.0, rail, -(depth - PARAPET_THICK) * 0.5),
            Vec3::new(width - 2.0 * PARAPET_THICK, PARAPET, PARAPET_THICK),
            0.0,
        );
        if deck == 0 {
            let jamb = (width - MOUTH) * 0.5 - PARAPET_THICK;
            for side in [-1.0f32, 1.0] {
                piece(
                    commands,
                    concrete(),
                    Vec3::new(
                        side * (MOUTH + jamb) * 0.5,
                        rail,
                        (depth - PARAPET_THICK) * 0.5,
                    ),
                    Vec3::new(jamb, PARAPET, PARAPET_THICK),
                    0.0,
                );
            }
        } else {
            piece(
                commands,
                concrete(),
                Vec3::new(0.0, rail, (depth - PARAPET_THICK) * 0.5),
                Vec3::new(width - 2.0 * PARAPET_THICK, PARAPET, PARAPET_THICK),
                0.0,
            );
        }

        // The ramp up from this deck, unless this is the top one.
        if deck + 1 < decks {
            let side = ramp_side(deck);
            let x = side * (width - RAMP_W) * 0.5 - side * PARAPET_THICK;
            // A slab whose centre sits halfway up the climb. Slightly longer
            // than the run so its ends tuck under the decks instead of
            // leaving a lip to catch a bumper on.
            let length = ramp_run / pitch.cos() + 0.6;
            // Climbing towards the back on even decks, towards the front on
            // odd ones — the direction of the switchback.
            let grade = if deck.is_multiple_of(2) {
                pitch
            } else {
                -pitch
            };
            piece(
                commands,
                assets.paving_for(RAMP_W),
                Vec3::new(x, floor + DECK * 0.5 - SLAB * 0.5, 0.0),
                Vec3::new(RAMP_W, SLAB, length),
                grade,
            );
        }
    }

    // The columns holding the whole optimistic arrangement up, full height,
    // set in from the corners.
    let top = (decks - 1) as f32 * DECK;
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            piece(
                commands,
                concrete(),
                Vec3::new(
                    sx * (width * 0.5 - COLUMN * 1.5),
                    top * 0.5,
                    sz * (depth * 0.5 - COLUMN * 1.5),
                ),
                Vec3::new(COLUMN, top.max(DECK), COLUMN),
                0.0,
            );
        }
    }
}

// The widest vehicle spec is well under three metres; the mouth takes one
// with room to miss the jambs, mostly. Compile-time facts, like the rest of
// the city's proportions.
const _: () = assert!(MOUTH > 6.0);
const _: () = assert!(RAMP_W > 3.5);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramp_is_driveable_on_the_shortest_footprint() {
        // The grade on the shortest plausible garage footprint stays under
        // twenty degrees — arcade suspension climbs that without drama.
        let steepest = (DECK / (16.0 - 2.0 * PARAPET_THICK)).atan();
        assert!(
            steepest < 20.0f32.to_radians(),
            "the ramp is a wall: {steepest}"
        );
    }
}
