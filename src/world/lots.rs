//! Furnishing the vacant lots.
//!
//! `citygen` decides what a vacant lot is *for*; this module is what that
//! looks like. Everything here is spawned per chunk by `buildings::spawn_block`
//! and carries `ChunkOf`, and everything is a pure function of the lot's own
//! rectangle — chunks regenerate on re-entry, so a bay line that moved between
//! visits would be a bug the player can stand on.
//!
//! The one exception is the cars standing in the parking bays: vehicles are
//! spawned once at startup and never streamed, so `vehicle::spawn` places them
//! into the same [`bays`] this module paints. The bay grid is the shared
//! contract; only the paint lives here.

use avian3d::prelude::*;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::citygen::{Block, Rect, VacantLot, VacantUse};
use super::mayhem::Breakaway;
use super::signage::SignKit;
use super::texture::{byte, fbm, painted_rect};

/// A parking bay, matching the smaller end of the real-world range — this is
/// a European city, and a comedy one at that.
pub const BAY_WIDTH: f32 = 2.7;
pub const BAY_DEPTH: f32 = 5.0;
/// Clearance between two facing rows of bays, enough to drive down.
const AISLE: f32 = 6.0;
/// Paint floats this far over the kerb slab's walking surface.
const PAINT_LIFT: f32 = SIDEWALK_HEIGHT + 0.006;

/// Canopy over a filling station's pumps.
const CANOPY_HEIGHT: f32 = 4.5;
const CANOPY_THICKNESS: f32 = 0.32;
/// Speed at which a car takes a pump off its bolts. The stump throws water —
/// nothing in this city burns, including, apparently, the petrol.
const PUMP_SHEARS_AT: f32 = 7.0;

#[derive(Resource)]
pub struct LotKit {
    /// A unit cube for slabs, scaled per use.
    cube: Handle<Mesh>,
    /// A unit plane, scaled into every painted line.
    line: Handle<Mesh>,
    paint: Handle<StandardMaterial>,
    post: Handle<Mesh>,
    steel: Handle<StandardMaterial>,
    canopy: Handle<StandardMaterial>,
    pump: Handle<Mesh>,
    pump_body: Handle<StandardMaterial>,
    hoop_post: Handle<Mesh>,
    backboard: Handle<Mesh>,
    board_white: Handle<StandardMaterial>,
    ring: Handle<Mesh>,
    ring_red: Handle<StandardMaterial>,
}

/// Worn white paint, the same reasoning as the road markings: a flat white
/// quad reads as a decal laid on the world, paint that has been parked on
/// does not.
fn paint_texture() -> bevy::image::Image {
    painted_rect(64, 64, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let value = (0.68 + fbm(u, v, 13, 2, 37) * 0.45).clamp(0.0, 1.0);
        [byte(value), byte(value * 0.98), byte(value * 0.93), 255]
    })
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<bevy::image::Image>,
) -> LotKit {
    LotKit {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        line: meshes.add(Plane3d::default().mesh().size(1.0, 1.0)),
        paint: materials.add(StandardMaterial {
            base_color_texture: Some(images.add(paint_texture())),
            perceptual_roughness: 0.75,
            ..default()
        }),
        post: meshes.add(Cuboid::new(0.22, CANOPY_HEIGHT, 0.22)),
        steel: materials.add(StandardMaterial {
            base_color: Color::srgb(0.52, 0.54, 0.56),
            perceptual_roughness: 0.45,
            metallic: 0.6,
            ..default()
        }),
        canopy: materials.add(StandardMaterial {
            base_color: Color::srgb(0.82, 0.83, 0.84),
            perceptual_roughness: 0.6,
            ..default()
        }),
        pump: meshes.add(Cuboid::new(0.55, 1.15, 0.42)),
        pump_body: materials.add(StandardMaterial {
            base_color: Color::srgb(0.72, 0.14, 0.12),
            perceptual_roughness: 0.5,
            ..default()
        }),
        hoop_post: meshes.add(Cylinder::new(0.07, 3.05)),
        backboard: meshes.add(Cuboid::new(1.35, 0.9, 0.05)),
        board_white: materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.9, 0.88),
            perceptual_roughness: 0.7,
            ..default()
        }),
        ring: meshes.add(Torus::new(0.21, 0.25)),
        ring_red: materials.add(StandardMaterial {
            base_color: Color::srgb(0.78, 0.28, 0.12),
            perceptual_roughness: 0.4,
            metallic: 0.4,
            ..default()
        }),
    }
}

/// One row of parking bays.
struct Row {
    /// Centre of the first separator line on this row's edge.
    origin: Vec2,
    /// Unit vector along the row.
    along: Vec2,
    /// Unit vector from the row's edge towards the aisle.
    across: Vec2,
    count: usize,
    step: f32,
}

/// The rows a lot's rectangle supports: one along each long edge if there is
/// room for two nose-in rows and an aisle, one along a single edge otherwise,
/// none if a car would not fit at all.
fn rows(rect: &Rect) -> Vec<Row> {
    let inner = rect.inset(0.8);
    if !inner.is_valid() {
        return Vec::new();
    }
    let size = inner.size();
    let along_x = size.x >= size.y;
    let (long, short) = if along_x {
        (size.x, size.y)
    } else {
        (size.y, size.x)
    };
    if short < BAY_DEPTH + 0.4 || long < BAY_WIDTH {
        return Vec::new();
    }
    let count = (long / BAY_WIDTH).floor() as usize;
    let step = long / count as f32;
    let two_rows = short >= BAY_DEPTH * 2.0 + AISLE;

    let (along, across) = if along_x {
        (Vec2::X, Vec2::Y)
    } else {
        (Vec2::Y, Vec2::X)
    };
    let mut out = vec![Row {
        origin: inner.min,
        along,
        across,
        count,
        step,
    }];
    if two_rows {
        // The second row hugs the opposite edge and faces back across the
        // aisle.
        let far = if along_x {
            Vec2::new(inner.min.x, inner.max.y)
        } else {
            Vec2::new(inner.max.x, inner.min.y)
        };
        out.push(Row {
            origin: far,
            along,
            across: -across,
            count,
            step,
        });
    }
    out
}

/// Centre and nose-out yaw of every bay on the lot — the contract shared with
/// `vehicle::spawn`, which parks real cars into them.
pub fn bays(rect: &Rect) -> Vec<(Vec2, f32)> {
    rows(rect)
        .iter()
        .flat_map(|row| {
            (0..row.count).map(move |i| {
                let centre = row.origin
                    + row.along * ((i as f32 + 0.5) * row.step)
                    + row.across * (BAY_DEPTH * 0.5);
                // Nose towards the edge, tail to the aisle, the way anybody
                // parks when nobody is watching them reverse.
                let nose = -row.across;
                (centre, crate::vehicle::spawn::heading_towards(nose))
            })
        })
        .collect()
}

/// Everything one vacant lot stands up.
pub fn spawn_lot(
    commands: &mut Commands,
    kit: &LotKit,
    signs: &SignKit,
    block: &Block,
    lot: &VacantLot,
    chunk: IVec2,
) {
    match lot.purpose {
        VacantUse::ParkingLot => spawn_parking(commands, kit, &lot.rect, chunk),
        VacantUse::GasStation => spawn_gas_station(commands, kit, signs, block, &lot.rect, chunk),
        VacantUse::Court => spawn_court(commands, kit, &lot.rect, chunk),
        VacantUse::Yard => {}
    }
}

fn painted_line(
    commands: &mut Commands,
    kit: &LotKit,
    chunk: IVec2,
    at: Vec2,
    yaw: f32,
    width: f32,
    length: f32,
) {
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.line.clone()),
        MeshMaterial3d(kit.paint.clone()),
        Transform::from_xyz(at.x, PAINT_LIFT, at.y)
            .with_rotation(Quat::from_rotation_y(yaw))
            .with_scale(Vec3::new(width, 1.0, length)),
        NotShadowCaster,
    ));
}

fn spawn_parking(commands: &mut Commands, kit: &LotKit, rect: &Rect, chunk: IVec2) {
    for row in rows(rect) {
        let yaw = row.across.x.atan2(row.across.y);
        // A separator either side of every bay: count + 1 lines.
        for i in 0..=row.count {
            let at =
                row.origin + row.along * (i as f32 * row.step) + row.across * (BAY_DEPTH * 0.5);
            painted_line(commands, kit, chunk, at, yaw, 0.11, BAY_DEPTH);
        }
    }
}

fn spawn_gas_station(
    commands: &mut Commands,
    kit: &LotKit,
    signs: &SignKit,
    block: &Block,
    rect: &Rect,
    chunk: IVec2,
) {
    let centre = rect.center();
    let size = rect.size();
    let along_x = size.x >= size.y;
    let width = if along_x { size.x } else { size.y }.min(11.0);
    let depth = if along_x { size.y } else { size.x }.min(7.5);
    let yaw = if along_x {
        0.0
    } else {
        std::f32::consts::FRAC_PI_2
    };
    let along = if along_x { Vec2::X } else { Vec2::Y };
    let across = if along_x { Vec2::Y } else { Vec2::X };

    // The canopy: four posts and a slab. The posts hold the roof, so unlike
    // most of the street furniture they do not break away.
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            let foot =
                centre + along * (sx * (width * 0.5 - 0.7)) + across * (sz * (depth * 0.5 - 0.7));
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.post.clone()),
                MeshMaterial3d(kit.steel.clone()),
                Transform::from_xyz(foot.x, SIDEWALK_HEIGHT + CANOPY_HEIGHT * 0.5, foot.y),
                RigidBody::Static,
                Collider::cuboid(0.22, CANOPY_HEIGHT, 0.22),
            ));
        }
    }
    // The slab, a scaled unit cube. Solid: a car launched off a wreck can
    // land on a canopy, and a roof you fall through is a bug, not a joke.
    let (ex, ez) = if along_x {
        (width, depth)
    } else {
        (depth, width)
    };
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.cube.clone()),
        MeshMaterial3d(kit.canopy.clone()),
        Transform::from_xyz(
            centre.x,
            SIDEWALK_HEIGHT + CANOPY_HEIGHT + CANOPY_THICKNESS * 0.5,
            centre.y,
        )
        .with_scale(Vec3::new(ex, CANOPY_THICKNESS, ez)),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
    ));

    // The pumps, in a row under the canopy. Bolted, but not forever: a car
    // arriving fast enough shears one off, and the stump throws water.
    // Nothing in this city burns — including, apparently, the petrol.
    let pumps = if width > 9.0 { 3 } else { 2 };
    for i in 0..pumps {
        let a = (i as f32 - (pumps - 1) as f32 * 0.5) * 3.4;
        let at = centre + along * a;
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.pump.clone()),
            MeshMaterial3d(kit.pump_body.clone()),
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT + 1.15 * 0.5, at.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            RigidBody::Static,
            Collider::cuboid(0.55, 1.15, 0.42),
            Breakaway {
                at: PUMP_SHEARS_AT,
                mass: 90.0,
                geyser: true,
            },
        ));
    }

    // TANKSTELLE, on the canopy fascia facing the lot's front — the side
    // nearest the block perimeter, which is the side with the arterial past
    // it (citygen only zones a filling station onto such a lot).
    let gaps = [
        rect.min.x - block.area.min.x,
        block.area.max.x - rect.max.x,
        rect.min.y - block.area.min.y,
        block.area.max.y - rect.max.y,
    ];
    let front = gaps
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map(|(side, _)| side)
        .unwrap_or(3);
    use std::f32::consts::{FRAC_PI_2, PI};
    let (board_mesh, board_material, board) = signs.tankstelle();
    let (at, board_yaw) = match front {
        0 => (centre + Vec2::new(-(ex * 0.5 + 0.05), 0.0), -FRAC_PI_2),
        1 => (centre + Vec2::new(ex * 0.5 + 0.05, 0.0), FRAC_PI_2),
        2 => (centre + Vec2::new(0.0, -(ez * 0.5 + 0.05)), PI),
        _ => (centre + Vec2::new(0.0, ez * 0.5 + 0.05), 0.0),
    };
    let fit = (width * 0.8 / board.x).min(1.0);
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(board_mesh.clone()),
        MeshMaterial3d(board_material.clone()),
        Transform::from_xyz(
            at.x,
            SIDEWALK_HEIGHT + CANOPY_HEIGHT + CANOPY_THICKNESS * 0.5,
            at.y,
        )
        .with_rotation(Quat::from_rotation_y(board_yaw))
        .with_scale(Vec3::splat(fit)),
        NotShadowCaster,
    ));
}

fn spawn_court(commands: &mut Commands, kit: &LotKit, rect: &Rect, chunk: IVec2) {
    let inner = rect.inset(1.2);
    if !inner.is_valid() {
        return;
    }
    let centre = inner.center();
    let size = inner.size();

    // The boundary, four painted lines.
    for (at, yaw, length) in [
        (Vec2::new(centre.x, inner.min.y), 0.0, size.x),
        (Vec2::new(centre.x, inner.max.y), 0.0, size.x),
        (
            Vec2::new(inner.min.x, centre.y),
            std::f32::consts::FRAC_PI_2,
            size.y,
        ),
        (
            Vec2::new(inner.max.x, centre.y),
            std::f32::consts::FRAC_PI_2,
            size.y,
        ),
    ] {
        painted_line(commands, kit, chunk, at, yaw, 0.1, length);
    }
    // Halfway line, across the short axis.
    let along_x = size.x >= size.y;
    if along_x {
        painted_line(
            commands,
            kit,
            chunk,
            centre,
            std::f32::consts::FRAC_PI_2,
            0.1,
            size.y,
        );
    } else {
        painted_line(commands, kit, chunk, centre, 0.0, 0.1, size.x);
    }

    // A hoop at each end of the long axis, facing back down the court.
    let (along, extent) = if along_x {
        (Vec2::X, size.x)
    } else {
        (Vec2::Y, size.y)
    };
    for end in [-1.0f32, 1.0] {
        let foot = centre + along * (end * (extent * 0.5 - 0.3));
        let inward = -along * end;
        let yaw = inward.x.atan2(inward.y);
        commands
            .spawn((
                ChunkOf(chunk),
                Mesh3d(kit.hoop_post.clone()),
                MeshMaterial3d(kit.steel.clone()),
                Transform::from_xyz(foot.x, SIDEWALK_HEIGHT + 3.05 * 0.5, foot.y)
                    .with_rotation(Quat::from_rotation_y(yaw)),
                RigidBody::Static,
                Collider::cuboid(0.14, 3.05, 0.14),
                // A car can take a hoop clean off its footing. No water in
                // this one, just regret.
                Breakaway {
                    at: 6.0,
                    mass: 45.0,
                    geyser: false,
                },
            ))
            .with_children(|post| {
                // Backboard near the top, hanging over the court side.
                post.spawn((
                    Mesh3d(kit.backboard.clone()),
                    MeshMaterial3d(kit.board_white.clone()),
                    Transform::from_xyz(0.0, 1.25, 0.4),
                ));
                // The ring, flat, in front of the board.
                post.spawn((
                    Mesh3d(kit.ring.clone()),
                    MeshMaterial3d(kit.ring_red.clone()),
                    Transform::from_xyz(0.0, 1.0, 0.68),
                ));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lot(w: f32, d: f32) -> Rect {
        Rect::new(Vec2::ZERO, Vec2::new(w, d))
    }

    #[test]
    fn bays_stay_inside_the_lot() {
        let rect = lot(24.0, 18.0);
        for (at, _) in bays(&rect) {
            assert!(
                at.x > rect.min.x && at.x < rect.max.x && at.y > rect.min.y && at.y < rect.max.y,
                "bay at {at:?} escaped the lot"
            );
        }
    }

    #[test]
    fn a_deep_lot_parks_two_rows_and_a_shallow_one_parks_one() {
        let deep = bays(&lot(20.0, 18.0));
        let shallow = bays(&lot(20.0, 8.0));
        assert!(!shallow.is_empty());
        assert_eq!(deep.len(), shallow.len() * 2);
    }

    #[test]
    fn a_lot_too_small_for_a_car_parks_nobody() {
        assert!(bays(&lot(3.0, 3.0)).is_empty());
    }

    #[test]
    fn bays_do_not_overlap() {
        for (i, (a, _)) in bays(&lot(26.0, 19.0)).iter().enumerate() {
            for (b, _) in bays(&lot(26.0, 19.0)).iter().skip(i + 1) {
                assert!(
                    a.distance(*b) > BAY_WIDTH - 0.1,
                    "bays at {a:?} and {b:?} share paint"
                );
            }
        }
    }
}
