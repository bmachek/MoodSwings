//! The church, as the one building that is not a box.
//!
//! Every skyline this game wants to parody has its spires, and a spire is
//! the single strongest silhouette a city block can buy: a tower reading
//! over the rooflines says "somewhere" where a grid of slabs says
//! "anywhere". So the zoning pass stamps three `BuildingKind::Church`es and
//! this module replaces each stamped box with a nave, a front tower, a
//! four-sided spire and a cross — the same arrangement as `world::garage`,
//! which proved that a civic kind can own its whole structure.
//!
//! Same door-frame convention as the garage and the interiors: `x` across
//! the front, `+z` towards the street, origin at the footprint's centre on
//! the pavement; `world::buildings` picks the front and hands in the yaw.
//!
//! The zoned height is treated as *presence* rather than as a wall height:
//! the nave stays low and the tower takes the rest, because a church that
//! is all nave is a warehouse and one that is all tower is a chimney.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::buildings::{ChunkOf, CityAssets, SIDEWALK_HEIGHT};

/// The tower's footprint side, clamped so a small lot still carries one.
const TOWER_SIDE_MAX: f32 = 5.5;
/// The spire's height as a share of the tower's.
const SPIRE_SHARE: f32 = 0.55;

/// Spawns one church: nave, pitched roof, tower, spire, cross.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    assets: &CityAssets,
    center: Vec2,
    width: f32,
    depth: f32,
    height: f32,
    yaw: f32,
    cathedral: bool,
    chunk: IVec2,
) {
    let spin = Quat::from_rotation_y(yaw);
    let place = |at: Vec3| spin * at + Vec3::new(center.x, SIDEWALK_HEIGHT, center.y);

    let nave_h = (height * 0.55).clamp(6.0, 12.0);
    // A parish tower is one and a half claims tall. The cathedral's is not
    // a multiple anybody signed off on — it is the tallest brick tower in
    // the world, the plaque says so, and the skyline has to back it up.
    let tower_h = height * if cathedral { 5.0 } else { 1.5 };
    // The zoning pass claims *large* buildings, and a church the full width
    // of a department-store lot grows a roof that swallows its own tower —
    // the pitched roof's height is tied to the nave's width by the 45°
    // geometry. So the nave keeps church proportions and cedes the rest of
    // the lot to its own forecourt.
    let width = width.min(18.0);
    let tower_side = (width * 0.32).min(TOWER_SIDE_MAX);
    // The nave sits behind the tower; the tower stands on the street front.
    let nave_depth = depth - tower_side;
    let nave_z = -tower_side * 0.5;
    let tower_z = (depth - tower_side) * 0.5;

    let solid = |commands: &mut Commands,
                 material: Handle<StandardMaterial>,
                 at: Vec3,
                 size: Vec3,
                 spun: Quat| {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(assets.unit_cube.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(place(at))
                .with_rotation(spin * spun)
                .with_scale(size),
            RigidBody::Static,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));
    };

    // The nave, in the kerb's stone — a church is masonry or it is a hall.
    solid(
        commands,
        assets.concrete(),
        Vec3::new(0.0, nave_h * 0.5, nave_z),
        Vec3::new(width, nave_h, nave_depth),
        Quat::IDENTITY,
    );
    // The pitched roof: a cube turned 45° about the ridge line reads as a
    // gable from every angle that matters. Its lower half is buried in the
    // nave, which is what keeps the eaves from floating.
    let half = width * 0.72;
    solid(
        commands,
        assets.roof_material(half.max(nave_depth)),
        Vec3::new(0.0, nave_h + 0.2, nave_z),
        Vec3::new(half, half, nave_depth * 0.98),
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
    );

    // The tower, centred on the front face.
    solid(
        commands,
        assets.concrete(),
        Vec3::new(0.0, tower_h * 0.5, tower_z),
        Vec3::new(tower_side, tower_h, tower_side),
        Quat::IDENTITY,
    );
    // The dark doorway in the tower's foot: two jambs and a lintel of
    // shadow, painted in geometry because the tower wears no facade.
    solid(
        commands,
        // The doorway is a slab of shadow rather than roofing; the tiling only
        // has to be small enough not to smear.
        assets.roof_material(2.0),
        Vec3::new(0.0, 1.6, tower_z + tower_side * 0.5 + 0.03),
        Vec3::new(1.9, 3.2, 0.08),
        Quat::IDENTITY,
    );

    // The spire: a four-sided cone, i.e. a pyramid. Visual only — nothing
    // lands on a spire on purpose, and anything arriving by accident can
    // meet the tower's box below it.
    let spire_h = tower_h * SPIRE_SHARE;
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(assets.spire()),
        MeshMaterial3d(assets.roof_material(tower_side.max(spire_h))),
        Transform::from_translation(place(Vec3::new(0.0, tower_h + spire_h * 0.5, tower_z)))
            // A four-sided cone stands with an edge forward; an eighth turn
            // puts a *face* forward, which is how a spire sits on a tower.
            .with_rotation(spin * Quat::from_rotation_y(std::f32::consts::FRAC_PI_4))
            .with_scale(Vec3::new(tower_side * 0.72, spire_h, tower_side * 0.72)),
    ));

    // The cross: two slim bars, proud of the spire's tip — scaled up on
    // the cathedral, where a parish cross would vanish at that altitude.
    let cross_base = tower_h + spire_h;
    let c = if cathedral { 2.2 } else { 1.0 };
    for (at, size) in [
        (
            Vec3::new(0.0, cross_base + 0.9 * c, tower_z),
            Vec3::new(0.12 * c, 1.8 * c, 0.12 * c),
        ),
        (
            Vec3::new(0.0, cross_base + 1.15 * c, tower_z),
            Vec3::new(0.85 * c, 0.12 * c, 0.12 * c),
        ),
    ] {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(assets.unit_cube.clone()),
            MeshMaterial3d(assets.concrete()),
            Transform::from_translation(place(at))
                .with_rotation(spin)
                .with_scale(size),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tower_out_ranks_the_nave() {
        // The proportions are the whole silhouette: for any zoned height the
        // claim hands over (10..14), the nave stays squat and the tower with
        // its spire clearly overtops it — otherwise the skyline reads
        // "warehouse with a chimney" instead of "church".
        for height in [10.0f32, 12.0, 14.0] {
            let nave = (height * 0.55f32).clamp(6.0, 10.0);
            let tower = height * 1.5;
            assert!(nave < tower * 0.6, "at {height}m the nave swallows it");
            assert!(
                tower * (1.0 + SPIRE_SHARE) > height * 1.8,
                "at {height}m the spire fails to read over the block"
            );
        }
    }
}
