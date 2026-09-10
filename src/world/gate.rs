//! The city gates, and the hole through them.
//!
//! A gate is the one building in this town that is *supposed* to stand in the
//! road. That is what a gate is: the street runs under it. Every other
//! landmark is kept out of a carriageway by `streetside::lots`, and a gate had
//! to be left out of the town altogether for as long as it would have been
//! drawn as what every other building is drawn as — a solid box — because a
//! solid box across a carriageway is a wall the traffic piles up behind and the
//! patrol walks into.
//!
//! So the whole module is about the hole. Everything else follows from the
//! photographs: Landshut's are fired brick, they carry a crenellated parapet
//! rather than a roof, and the two that survive on the Isar bank are twin
//! towers with a pointed arch slung between them.
//!
//! Two shapes, decided by how wide the map says the gate is.
//!
//! * **Twin towers.** The Ländtor is sixteen and a half metres across and five
//!   deep — two towers standing clear of each other with the street between
//!   them, which is why it reads as a gate from a hundred metres and a single
//!   arch does not.
//! * **A gate tower.** The other three are seven to twelve metres across,
//!   which is one tower with the road bored through it.
//!
//! ## The hole is a fact about the collider, not about the mesh
//!
//! A pointed arch here is three boxes: two jambs and a head made of two more
//! leaning together. What matters is that *none of them has a collider inside
//! the opening*. The jambs are solid, the arch head is spawned above the
//! clearance and carries a collider that starts there, and the span over the
//! top carries one too — so a lorry drives under it and a launched car does
//! not pass through it. `world::gate` is therefore the only module here where
//! the collider layout is the design and the mesh is the decoration.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::buildings::{ChunkOf, CityAssets, SIDEWALK_HEIGHT};

/// How much headroom the arch leaves, in metres.
///
/// Four and a half: a lorry is four, and the game's tallest vehicle plus the
/// hop a rubber one takes off a kerb wants the rest. Below this the gate stops
/// being a gate and becomes a low bridge, which is a different joke.
const CLEAR: f32 = 4.5;

/// How wide a gate has to be before it is two towers rather than one.
const TWIN: f32 = 13.0;

/// How much of a twin gate's width each tower takes.
const TOWER_SHARE: f32 = 0.31;

/// How wide the opening is through a single gate tower, as a share of its
/// frontage.
const BORE: f32 = 0.46;

/// The parapet: how tall a merlon stands and how many go along a side.
const MERLON: f32 = 1.15;
const MERLONS: usize = 5;

/// Spawns one gate.
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
    let place = |at: Vec3| spin * at + Vec3::new(center.x, SIDEWALK_HEIGHT, center.y);
    let brick = assets.brick();

    // A gate that cannot clear a lorry is not one. The map gives the Ländtor
    // ten metres, which is the parapet; the others get what the atlas guessed.
    let height = height.max(CLEAR + MERLON + 1.5);
    let depth = depth.max(2.4);

    // Solid, with a collider. Used for everything a vehicle must not pass
    // through, which is everything except the opening.
    let solid = |commands: &mut Commands,
                 material: Handle<StandardMaterial>,
                 at: Vec3,
                 size: Vec3,
                 spun: Quat| {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(assets.stone_cube.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(place(at))
                .with_rotation(spin * spun)
                .with_scale(size),
            RigidBody::Static,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));
    };
    // And the same without one, for the arch head: it leans into the opening,
    // and a collider on it would be a collider in the hole.
    let hollow = |commands: &mut Commands, at: Vec3, size: Vec3, spun: Quat| {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(assets.stone_cube.clone()),
            MeshMaterial3d(brick.clone()),
            Transform::from_translation(place(at))
                .with_rotation(spin * spun)
                .with_scale(size),
        ));
    };

    // The two jambs and how far apart they stand. A twin gate's towers are
    // deeper than its span; a bored one is one mass with a slot in it.
    let (jamb, opening, tower_depth) = if width >= TWIN {
        let jamb = width * TOWER_SHARE;
        (jamb, width - jamb * 2.0, depth.max(jamb * 0.9))
    } else {
        let opening = width * BORE;
        ((width - opening) * 0.5, opening, depth)
    };

    for side in [-1.0f32, 1.0] {
        let at = side * (opening + jamb) * 0.5;
        solid(
            commands,
            brick.clone(),
            Vec3::new(at, height * 0.5, 0.0),
            Vec3::new(jamb, height, tower_depth),
            Quat::IDENTITY,
        );
        parapet(commands, assets, &place, spin, chunk, at, height, jamb, tower_depth);
    }

    // The span over the opening: solid, and starting at the clearance, so what
    // is under it is air.
    let span_h = (height - CLEAR).max(0.5);
    solid(
        commands,
        brick.clone(),
        Vec3::new(0.0, CLEAR + span_h * 0.5, 0.0),
        Vec3::new(opening, span_h, depth),
        Quat::IDENTITY,
    );

    // The pointed head. Two slabs springing from the jambs and meeting at the
    // apex, which is what makes the opening an arch rather than a doorway —
    // and the shape the photographs of the Ländtor are of.
    //
    // Purely mesh. They lean *into* the opening, so a collider on them would
    // be a collider in the hole; what stops anything getting through is the
    // span above, whose underside is the clearance.
    let springing = CLEAR * 0.60;
    let rise = CLEAR - springing;
    let half_run = opening * 0.5;
    let leaf = (half_run * half_run + rise * rise).sqrt();
    for side in [-1.0f32, 1.0] {
        // From the springing point on this jamb, up and inward to the apex.
        let lean = rise.atan2(-side * half_run);
        hollow(
            commands,
            Vec3::new(side * half_run * 0.5, springing + rise * 0.5, 0.0),
            Vec3::new(leaf, opening * 0.13, depth * 1.02),
            Quat::from_rotation_z(lean),
        );
    }

    // And the parapet across the span, so the whole thing is crowned rather
    // than the towers alone.
    if width >= TWIN {
        parapet(
            commands, assets, &place, spin, chunk, 0.0, height, opening, depth,
        );
    }
}

/// The crenellations: merlons with the gaps between them left as gaps.
#[allow(clippy::too_many_arguments)]
fn parapet(
    commands: &mut Commands,
    assets: &CityAssets,
    place: &impl Fn(Vec3) -> Vec3,
    spin: Quat,
    chunk: IVec2,
    at: f32,
    height: f32,
    across: f32,
    depth: f32,
) {
    // Merlon, gap, merlon: an odd count so a run starts and ends on stone,
    // which is what a battlement does and what makes the silhouette read.
    let pitch = across / (MERLONS as f32 * 2.0 - 1.0);
    for i in 0..MERLONS {
        let x = at - across * 0.5 + pitch * (i as f32 * 2.0 + 0.5);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(assets.stone_cube.clone()),
            MeshMaterial3d(assets.brick()),
            Transform::from_translation(place(Vec3::new(x, height + MERLON * 0.5, 0.0)))
                .with_rotation(spin)
                .with_scale(Vec3::new(pitch, MERLON, depth * 1.04)),
        ));
    }
}
