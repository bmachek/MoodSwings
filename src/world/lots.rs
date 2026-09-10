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
/// Parking-bay paint, over the pavement it is laid on — see `world::layer`.
const PAINT_LIFT: f32 = super::layer::FOOTWAY + super::layer::STEP * 6.0;

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
    ball: Handle<Mesh>,
    ball_orange: Handle<StandardMaterial>,
    stall_post: Handle<Mesh>,
    stall_counter: Handle<Mesh>,
    stall_canopy: Handle<Mesh>,
    stall_wood: Handle<StandardMaterial>,
    /// Three awning colours, dealt round the stalls in rotation.
    awnings: [Handle<StandardMaterial>; 3],
    crate_box: Handle<Mesh>,
    crate_wood: Handle<StandardMaterial>,
}

/// A market stall's proportions.
const STALL_W: f32 = 2.6;
const STALL_D: f32 = 1.8;
const STALL_GAP: f32 = 1.3;
const STALL_CANOPY_H: f32 = 2.2;

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
        ball: meshes.add(Sphere::new(0.24)),
        ball_orange: materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.44, 0.14),
            perceptual_roughness: 0.85,
            ..default()
        }),
        stall_post: meshes.add(Cuboid::new(0.1, STALL_CANOPY_H, 0.1)),
        stall_counter: meshes.add(Cuboid::new(STALL_W * 0.85, 0.9, STALL_D * 0.7)),
        stall_canopy: meshes.add(Cuboid::new(STALL_W, 0.06, STALL_D)),
        stall_wood: materials.add(StandardMaterial {
            base_color: Color::srgb(0.48, 0.36, 0.22),
            perceptual_roughness: 0.9,
            ..default()
        }),
        awnings: [
            Color::srgb(0.70, 0.22, 0.18),
            Color::srgb(0.22, 0.48, 0.28),
            Color::srgb(0.86, 0.68, 0.20),
        ]
        .map(|color| {
            materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.85,
                ..default()
            })
        }),
        crate_box: meshes.add(Cuboid::new(0.52, 0.4, 0.52)),
        crate_wood: materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.50, 0.32),
            perceptual_roughness: 0.95,
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
        VacantUse::Market => spawn_market(commands, kit, signs, &lot.rect, chunk),
        VacantUse::Yard => {}
    }
}

/// A market: two rows of stalls facing a central lane, an awning each, and
/// loose crates by the counters — dynamic on purpose, because a car through
/// a market that does not scatter crates is a missed appointment.
fn spawn_market(commands: &mut Commands, kit: &LotKit, signs: &SignKit, rect: &Rect, chunk: IVec2) {
    let inner = rect.inset(1.2);
    if !inner.is_valid() {
        return;
    }
    let size = inner.size();
    let along_x = size.x >= size.y;
    let (long, short) = if along_x {
        (size.x, size.y)
    } else {
        (size.y, size.x)
    };
    let (along, across) = if along_x {
        (Vec2::X, Vec2::Y)
    } else {
        (Vec2::Y, Vec2::X)
    };
    let count = (long / (STALL_W + STALL_GAP)).floor() as usize;
    if count == 0 {
        return;
    }
    let step = long / count as f32;
    let centre = inner.center();
    // Two rows if a lane fits between them, one down the middle otherwise.
    let offsets: &[f32] = if short >= STALL_D * 2.0 + 3.0 {
        &[-1.0, 1.0]
    } else {
        &[0.0]
    };

    for (row, &side) in offsets.iter().enumerate() {
        let edge = centre + across * (side * (short * 0.5 - STALL_D * 0.5));
        for i in 0..count {
            let at = edge + along * ((i as f32 + 0.5) * step - long * 0.5);
            // The counter, solid: the one piece of a stall you bounce off.
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.stall_counter.clone()),
                MeshMaterial3d(kit.stall_wood.clone()),
                Transform::from_xyz(at.x, SIDEWALK_HEIGHT + 0.45, at.y),
                RigidBody::Static,
                Collider::cuboid(STALL_W * 0.85, 0.9, STALL_D * 0.7),
            ));
            // Two posts and the awning. Visual only — the stall's body is
            // the collider, and a canopy you clip on a hop is comedy tax.
            for end in [-1.0f32, 1.0] {
                let foot = at + along * (end * STALL_W * 0.45);
                commands.spawn((
                    ChunkOf(chunk),
                    Mesh3d(kit.stall_post.clone()),
                    MeshMaterial3d(kit.stall_wood.clone()),
                    Transform::from_xyz(foot.x, SIDEWALK_HEIGHT + STALL_CANOPY_H * 0.5, foot.y),
                ));
            }
            let yaw = if along_x {
                0.0
            } else {
                std::f32::consts::FRAC_PI_2
            };
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.stall_canopy.clone()),
                MeshMaterial3d(kit.awnings[(i + row * 2) % kit.awnings.len()].clone()),
                Transform::from_xyz(at.x, SIDEWALK_HEIGHT + STALL_CANOPY_H, at.y)
                    .with_rotation(Quat::from_rotation_y(yaw)),
                NotShadowCaster,
            ));
            // A crate or two beside the counter, loose. Every other stall
            // keeps its stock packed away, so the ground stays walkable.
            if i % 2 == 0 {
                let spot = at + across * (STALL_D * 0.85) + along * (STALL_W * 0.2);
                commands.spawn((
                    ChunkOf(chunk),
                    Mesh3d(kit.crate_box.clone()),
                    MeshMaterial3d(kit.crate_wood.clone()),
                    Transform::from_xyz(spot.x, SIDEWALK_HEIGHT + 0.2, spot.y),
                    RigidBody::Dynamic,
                    Collider::cuboid(0.52, 0.4, 0.52),
                    Mass(8.0),
                ));
            }
        }
    }

    // The board on its pole at the lot's centre front, over the lane.
    let (mesh, material, board) = signs.markt();
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.hoop_post.clone()),
        MeshMaterial3d(kit.steel.clone()),
        Transform::from_xyz(centre.x, SIDEWALK_HEIGHT + 1.5, centre.y),
        RigidBody::Static,
        Collider::cylinder(0.07, 3.05),
    ));
    let lift = SIDEWALK_HEIGHT + 3.05 + board.y * 0.5;
    let yaw = if along_x {
        0.0
    } else {
        std::f32::consts::FRAC_PI_2
    };
    for flip in [0.0, std::f32::consts::PI] {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(centre.x, lift, centre.y)
                .with_rotation(Quat::from_rotation_y(yaw + flip)),
            NotShadowCaster,
        ));
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

/// A real basketball court's proportions, in metres, as one block of numbers.
///
/// A lot is whatever the subdivision left over, so nothing here is used at
/// face value: [`court_fit`] shrinks the whole court until it fits the lot,
/// and every measurement below is multiplied by that one factor. The court
/// is therefore always *a court* — 28 by 15, the key half as wide again as
/// the circle, the arc where an arc goes — just sometimes a small one.
mod court {
    pub const LENGTH: f32 = 28.0;
    pub const WIDTH: f32 = 15.0;
    /// Centre of the ring, measured in from the baseline.
    pub const BASKET_INSET: f32 = 1.575;
    /// The key: how far it reaches in from the baseline, and half how wide.
    pub const LANE_LENGTH: f32 = 5.80;
    pub const LANE_HALF: f32 = 2.45;
    /// Centre circle, free-throw circle: the same radius on a real court.
    pub const CIRCLE: f32 = 1.80;
    /// The three-point arc, and how far from the centre line its two
    /// straight sections run.
    pub const THREE: f32 = 6.75;
    pub const THREE_CORNER: f32 = 6.60;
}

// Checked at compile time rather than in a test, the same way the figure's
// proportions are: these are all constants, so there is nothing to run.
const _: () = {
    assert!(
        court::LANE_HALF < court::THREE_CORNER,
        "the key is wider than the three-point line"
    );
    assert!(
        court::LANE_LENGTH + court::CIRCLE < court::LENGTH * 0.5,
        "the free-throw circle crosses the halfway line"
    );
    // The free-throw circle is centred on the free-throw line, so half of it
    // stands inside the key and half outside.
    assert!(
        court::CIRCLE > court::LANE_HALF * 0.5,
        "the free-throw circle fits inside the key it is drawn across"
    );
    assert!(
        court::BASKET_INSET < court::LANE_LENGTH,
        "the basket stands outside its own key"
    );
};

/// The court that fits a lot: half its length, half its width, and the unit
/// vector its length runs along.
///
/// One scale factor for both axes — a court stretched to the lot is not a
/// court, it is a car park with a hoop — and the long axis of the court laid
/// along the long axis of the lot.
fn court_fit(inner: &Rect) -> (f32, f32, Vec2) {
    let size = inner.size();
    let (long, short, along) = if size.x >= size.y {
        (size.x, size.y, Vec2::X)
    } else {
        (size.y, size.x, Vec2::Y)
    };
    let scale = (long / court::LENGTH).min(short / court::WIDTH);
    (
        court::LENGTH * 0.5 * scale,
        court::WIDTH * 0.5 * scale,
        along,
    )
}

/// The scale one lot's court came out at, which every measurement in
/// [`court`] is multiplied by.
fn court_scale(half_length: f32) -> f32 {
    half_length / (court::LENGTH * 0.5)
}

/// One court's paint pot: everything a painted line needs that is not its own
/// position, so that the layout below can read as a layout.
struct Brush<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    kit: &'a LotKit,
    chunk: IVec2,
    width: f32,
}

impl Brush<'_, '_, '_> {
    /// A line of `length` through `p`, running along `dir`.
    ///
    /// The same yaw convention as `spawn_parking`: the mesh's +Z is turned
    /// onto `dir`, so the length runs the way `dir` points. Getting this
    /// pair the wrong way round is exactly what was wrong with the court —
    /// every boundary line was drawn at right angles to the edge it marked,
    /// with the length of the edge it did not.
    fn stripe(&mut self, p: Vec2, dir: Vec2, length: f32) {
        painted_line(
            self.commands,
            self.kit,
            self.chunk,
            p,
            dir.x.atan2(dir.y),
            self.width,
            length,
        );
    }

    /// An arc as short straight segments: `sweep` radians either side of
    /// `facing`, at `radius` about `hub`. Sixteen segments to the half turn
    /// is where the corners stop being visible from the pavement.
    fn arc(&mut self, hub: Vec2, radius: f32, facing: f32, sweep: f32) {
        let steps = ((sweep / std::f32::consts::PI) * 16.0).ceil().max(4.0) as usize;
        let step = sweep * 2.0 / steps as f32;
        let point = |angle: f32| hub + Vec2::new(angle.cos(), angle.sin()) * radius;
        for i in 0..steps {
            let from = point(facing - sweep + step * i as f32);
            let to = point(facing - sweep + step * (i + 1) as f32);
            let span = to - from;
            let Ok(dir) = Dir2::new(span) else { continue };
            // Overlapped by a line width, or every joint in the arc shows as
            // a notch where the two segments are turned against each other.
            self.stripe(from.midpoint(to), *dir, span.length() + self.width);
        }
    }

    fn circle(&mut self, hub: Vec2, radius: f32) {
        self.arc(hub, radius, 0.0, std::f32::consts::PI);
    }
}

fn spawn_court(commands: &mut Commands, kit: &LotKit, rect: &Rect, chunk: IVec2) {
    let inner = rect.inset(1.2);
    if !inner.is_valid() {
        return;
    }
    let centre = inner.center();
    let (half_length, half_width, along) = court_fit(&inner);
    let scale = court_scale(half_length);
    // Below about a third scale the whole layout is thinner than its own
    // paint and reads as a smear. A bare yard is a better joke than a
    // diagram of one.
    if scale < 0.3 {
        return;
    }
    let across = Vec2::new(-along.y, along.x);
    // Real paint is five centimetres, which from standing height on a
    // shrunken court is nothing at all — so it thins with the court, but
    // never past what a decal can carry.
    let width = (0.09 * scale).clamp(0.07, 0.12);

    // Court coordinates: `u` along the length from the middle, `v` across.
    let at = |u: f32, v: f32| centre + along * u + across * v;

    {
        let mut brush = Brush {
            commands,
            kit,
            chunk,
            width,
        };

        // The boundary. Sidelines run the length of the court, baselines
        // across it.
        for side in [-1.0f32, 1.0] {
            brush.stripe(at(0.0, side * half_width), along, half_length * 2.0);
            brush.stripe(at(side * half_length, 0.0), across, half_width * 2.0);
        }
        // Halfway line and centre circle.
        brush.stripe(centre, across, half_width * 2.0);
        brush.circle(centre, court::CIRCLE * scale);

        // Where the three-point arc reaches its widest allowed point, and so
        // where its two straight sections have to start.
        let reach =
            (court::THREE * court::THREE - court::THREE_CORNER * court::THREE_CORNER).sqrt();

        // The two ends. `end` is +1 for the half in the +`along` direction.
        for end in [-1.0f32, 1.0] {
            let baseline = end * half_length;

            // The key, and the free-throw line closing it.
            let lane_end = baseline - end * court::LANE_LENGTH * scale;
            for side in [-1.0f32, 1.0] {
                brush.stripe(
                    at((baseline + lane_end) * 0.5, side * court::LANE_HALF * scale),
                    along,
                    court::LANE_LENGTH * scale,
                );
            }
            brush.stripe(at(lane_end, 0.0), across, court::LANE_HALF * 2.0 * scale);
            // The free-throw circle sits on that line, centred on it.
            brush.circle(at(lane_end, 0.0), court::CIRCLE * scale);

            // The three-point line: two straights parallel to the sidelines,
            // then the arc that joins them round the basket.
            let hub_u = baseline - end * court::BASKET_INSET * scale;
            let straight_to = hub_u - end * reach * scale;
            for side in [-1.0f32, 1.0] {
                brush.stripe(
                    at(
                        (baseline + straight_to) * 0.5,
                        side * court::THREE_CORNER * scale,
                    ),
                    along,
                    (baseline - straight_to).abs(),
                );
            }
            // Swept about the basket, facing in from the baseline, until the
            // arc is as wide as the straights it has to meet.
            let inward = -along * end;
            brush.arc(
                at(hub_u, 0.0),
                court::THREE * scale,
                inward.y.atan2(inward.x),
                std::f32::consts::FRAC_PI_2 + (reach / court::THREE).asin(),
            );
        }
    }

    // The ball. A flummi that is only a flummi: a pure bounce body with no
    // mood, no face and no opinions — taunting it does nothing, which any
    // citizen could have told you. Anybody who walks into it kicks it, and
    // it respawns centre-court with the chunk, which is more than most
    // courts can say for their ball.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.ball.clone()),
        MeshMaterial3d(kit.ball_orange.clone()),
        Transform::from_xyz(centre.x, SIDEWALK_HEIGHT + 1.2, centre.y),
        RigidBody::Dynamic,
        Collider::sphere(0.24),
        // Bouncier than the city's baseline: it is the one object here whose
        // entire job is the bounce.
        Restitution::new(0.88).with_combine_rule(CoefficientCombine::Max),
        Mass(0.6),
    ));

    // A hoop at each end, standing just off the baseline and reaching in far
    // enough that the ring hangs over the middle of its own arc.
    for end in [-1.0f32, 1.0] {
        const STAND: f32 = 0.5;
        let foot = centre + along * (end * (half_length + STAND));
        let inward = -along * end;
        let yaw = inward.x.atan2(inward.y);
        let ring_out = STAND + court::BASKET_INSET * scale;
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
                    Transform::from_xyz(0.0, 1.25, ring_out - 0.28),
                ));
                // The ring, flat, in front of the board and directly over
                // the point its own three-point arc is drawn about.
                post.spawn((
                    Mesh3d(kit.ring.clone()),
                    MeshMaterial3d(kit.ring_red.clone()),
                    Transform::from_xyz(0.0, 1.0, ring_out),
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
    fn a_court_keeps_a_real_courts_proportions_whatever_the_lot() {
        // Stretching the court to the lot is the failure this guards: the
        // markings only read as basketball if the key, the circles and the
        // arc keep their sizes relative to one another and to the boundary.
        for (w, d) in [(60.0, 40.0), (40.0, 60.0), (30.0, 28.0), (18.0, 40.0)] {
            let inner = lot(w, d).inset(1.2);
            let (half_length, half_width, along) = court_fit(&inner);
            let ratio = half_length / half_width;
            assert!(
                (ratio - court::LENGTH / court::WIDTH).abs() < 1e-4,
                "a {w}x{d} lot gave a court of ratio {ratio}"
            );
            // And it fits, on both axes, whichever way round it was laid.
            let size = inner.size();
            let (long, short) = if along == Vec2::X {
                (size.x, size.y)
            } else {
                (size.y, size.x)
            };
            assert!(half_length * 2.0 <= long + 1e-3, "the court is too long");
            assert!(half_width * 2.0 <= short + 1e-3, "the court is too wide");
        }
    }

    #[test]
    fn a_court_lies_along_the_long_side_of_its_lot() {
        assert_eq!(court_fit(&lot(60.0, 40.0)).2, Vec2::X);
        assert_eq!(court_fit(&lot(40.0, 60.0)).2, Vec2::Y);
    }

    #[test]
    fn the_three_point_arc_meets_its_own_straight_sections() {
        // The straights run at THREE_CORNER from the centre line and the arc
        // is struck at THREE from the basket. If the two do not meet, the
        // line has a step in it — which is the tell that the numbers were
        // eyeballed rather than measured.
        let reach =
            (court::THREE * court::THREE - court::THREE_CORNER * court::THREE_CORNER).sqrt();
        let sweep = std::f32::consts::FRAC_PI_2 + (reach / court::THREE).asin();
        // The arc's own endpoint, measured out from the basket.
        let across = (sweep.sin() * court::THREE).abs();
        assert!(
            (across - court::THREE_CORNER).abs() < 1e-3,
            "the arc ends {across} out where the straight runs at {}",
            court::THREE_CORNER
        );
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
