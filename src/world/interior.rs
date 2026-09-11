//! What is behind the door.
//!
//! An enterable building's ground floor is a real room: floor, walls, an
//! emissive ceiling, furnishing to its kind, and one member of staff with a
//! full set of feelings. Everything above the room stays the facade illusion
//! it always was — the upper storeys are `glaze()` parallax forever, and that
//! illusion is the budget that makes the ground floor affordable.
//!
//! ## The frame
//!
//! Everything here is laid out in the building's *door frame*: `x` runs
//! across the front wall, `z` towards the street, the origin is the middle of
//! the footprint at pavement level. `world::buildings` chooses which compass
//! face is the front (the same choice the sign makes) and hands in the yaw;
//! this module never learns which way the building actually points, which is
//! what keeps the layout code readable and the door always in the wall it
//! was carved into.
//!
//! ## Light without lights
//!
//! No point lights per room — a city of interiors would be a city of shadow
//! maps. The room's surfaces are emissive instead, at a *fixed* level on the
//! window-glow scale (`emissive` here is exposure-independent — the same scale
//! `timeofday::WINDOW_GLOW` is on, which is multiples of the frame's white
//! point rather than the nits that constant's own comment used to claim; the
//! deferred g-buffer drops the alpha Bevy would have applied the exposure
//! through, and this module was the one place that had it right).
//! Fixed is not a shortcut,
//! it is the behaviour wanted: against a sunlit street the room reads as
//! shade, and after dark the same numbers read as a shop with its lights
//! on, without a system touching a thing. A first draft drove these values
//! up a daylight curve to ~300 and every interior rendered as a furnace —
//! the exposure never sees emissive, so the curve was multiplying against
//! nothing.

use avian3d::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::citygen::BuildingKind;
use super::shell;
use super::texture::FacadeClass;
use crate::bounce::controller::Bouncer;
use crate::mood::face::FaceLevel;
use crate::mood::feeling::{Mood, Tempers};
use crate::mood::provoke::Provoker;
use crate::mood::voice::Voicebox;

/// Structural wall thickness, in metres — the compound collider's plates.
/// `world::buildings` builds the collider from this; the visual wall linings
/// here sit flush against its inner surface.
pub const WALL: f32 = 0.30;

/// How far away an interior is drawn at all, before `lod_scale`. Just past
/// the full-detail shell handover: once the doored shell has given way to
/// the coarse one there is no opening to see it through — and while it has
/// one, an unfurnished room is a hole you can see the far side of the town
/// through. That coupling is held at compile time at the foot of this file,
/// which is why this number moved when `shell::NEAR` did.
pub const RANGE: f32 = 200.0;

/// The lining's own thickness. Visual only.
const LINING: f32 = 0.10;

/// The same capsule the crowd wears — the staff are citizens.
const RADIUS: f32 = 0.32;
const HEIGHT: f32 = 1.05;
const STAND_HEIGHT: f32 = HEIGHT * 0.5 + RADIUS;

/// One member of staff, behind their counter. A marker so later milestones
/// can find them; today it only names what they are.
#[derive(Component)]
pub struct Staff;

/// The cast resources an interior needs to put a person in the room.
/// Bundled so `BlockContext` carries one optional thing rather than three.
pub struct CastContext<'a> {
    pub figures: &'a crate::ai::figure::FigureAssets,
    pub tempers: &'a Tempers,
}

/// Shared meshes and materials for every interior in the city.
#[derive(Resource)]
pub struct InteriorKit {
    cube: Handle<Mesh>,
    floor: Handle<StandardMaterial>,
    wall: Handle<StandardMaterial>,
    ceiling: Handle<StandardMaterial>,
    shelf: Handle<StandardMaterial>,
    counter: Handle<StandardMaterial>,
    table: Handle<StandardMaterial>,
    sofa: Handle<StandardMaterial>,
    /// A work coat per enterable kind, in `coat_for` order.
    coats: Vec<Handle<StandardMaterial>>,
}

impl InteriorKit {
    fn coat_for(&self, kind: BuildingKind) -> Handle<StandardMaterial> {
        let slot = match kind {
            BuildingKind::Supermarket => 0,
            BuildingKind::Restaurant => 1,
            BuildingKind::Hotel => 2,
            _ => 3,
        };
        self.coats[slot].clone()
    }
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> InteriorKit {
    // Every interior surface glows a little instead of being lit — see the
    // module docs. `strength` is its share of the room's light on the
    // window-glow scale: the ceiling is the lamp, everything else is what
    // the lamp falls on.
    let surface =
        |materials: &mut Assets<StandardMaterial>, color: Color, roughness: f32, strength: f32| {
            materials.add(StandardMaterial {
                base_color: color,
                emissive: color.to_linear() * strength,
                perceptual_roughness: roughness,
                ..default()
            })
        };

    let floor = surface(materials, Color::srgb(0.62, 0.58, 0.50), 0.7, 0.6);
    let wall = surface(materials, Color::srgb(0.87, 0.83, 0.74), 0.95, 1.0);
    let ceiling = surface(materials, Color::srgb(0.97, 0.96, 0.92), 0.9, 2.4);
    let shelf = surface(materials, Color::srgb(0.72, 0.30, 0.24), 0.8, 0.6);
    let counter = surface(materials, Color::srgb(0.35, 0.27, 0.20), 0.6, 0.5);
    let table = surface(materials, Color::srgb(0.80, 0.78, 0.72), 0.8, 0.65);
    let sofa = surface(materials, Color::srgb(0.28, 0.38, 0.46), 1.0, 0.5);

    // Work coats are lit like the room they stand in, or the clerk reads as
    // a cutout pasted into their own shop.
    let coats = [
        Color::srgb(0.24, 0.52, 0.30), // the supermarket apron
        Color::srgb(0.16, 0.16, 0.18), // the waiter's black
        Color::srgb(0.22, 0.30, 0.52), // the concierge blue
        Color::srgb(0.45, 0.45, 0.48), // office grey
    ]
    .into_iter()
    .map(|color| surface(materials, color, 0.85, 0.55))
    .collect();

    InteriorKit {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        floor,
        wall,
        ceiling,
        shelf,
        counter,
        table,
        sofa,
        coats,
    }
}

/// How high above the pavement a shopfront's light hangs.
///
/// Head height rather than window height. The source is a whole shop window
/// two or three metres wide, and a point light standing in for one has to sit
/// where its *average* is or the pavement gets a hard little disc instead of a
/// wash.
pub const SPILL_HEIGHT: f32 = 2.3;

/// Marks the patch of pavement a lit shopfront throws its light onto.
///
/// A marker with a position and nothing else: the light itself belongs to
/// `world::streetlights`, which keeps a pool of them and moves the pool to
/// whichever fixtures are nearest. Spawning a real `PointLight` per shop would
/// be a few hundred lights in a district, and almost all of them behind the
/// camera.
///
/// Spawned by `world::buildings::spawn_building` rather than here, because a
/// shopfront is a thing every building above house height has and a walk-in
/// interior is a thing about one in twenty of them has.
#[derive(Component)]
pub struct Shopfront {
    /// The way the front faces on the ground plane: out through the door,
    /// across the pavement, towards the road.
    ///
    /// A position alone was enough while the only thing anybody did with a
    /// front was walk at it. A queue has to know which way is *along* the
    /// wall — a line that runs out from the door stands in the carriageway —
    /// and the only honest source for that is the yaw the building was
    /// stamped at, so the spawner hands it over rather than letting the
    /// crowd guess from which side the first customer arrived.
    pub outward: Vec2,
}

/// Stable destination identity for a front. The mesh is streamed, this value
/// is not: errands can remember a shop while its chunk is out of range.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlaceId(pub u64);

impl PlaceId {
    /// Quantising to decimetres makes the identity independent of f32 noise
    /// while keeping two neighbouring doors distinct in the whole town.
    pub fn at(position: Vec3) -> Self {
        let x = (position.x * 10.0).round() as i64 as u64;
        let z = (position.z * 10.0).round() as i64 as u64;
        Self(x.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ z.rotate_left(23))
    }
}

/// Everything `spawn` needs to know about where the room stands.
pub struct Doorframe {
    /// World position of the footprint's centre, at pavement level.
    pub center: Vec2,
    /// Rotation taking the door frame's `+z` onto the front face's outward
    /// normal — the same yaw the sign hangs at.
    pub yaw: f32,
    /// Footprint extent across the front and towards the back, in the door
    /// frame's axes.
    pub width: f32,
    pub depth: f32,
    pub height: f32,
    pub class: FacadeClass,
}

/// Spawns one building's room, furnishing and staff.
pub fn spawn(
    commands: &mut Commands,
    kit: &InteriorKit,
    cast: Option<&CastContext>,
    kind: BuildingKind,
    frame: &Doorframe,
    seed: u64,
    chunk: IVec2,
    lod_scale: f32,
) {
    let spin = Quat::from_rotation_y(frame.yaw);
    let head = shell::door_head(frame.class, frame.height);
    let range = (RANGE * lod_scale).max(1.0);

    // The room inside the structural walls.
    let (w, d) = (frame.width - 2.0 * WALL, frame.depth - 2.0 * WALL);

    // A local box: centre and size in the door frame, rotated out into the
    // world. Everything visible in here is `NotShadowCaster` — the room is
    // inside a building; the sun was never getting in.
    let piece = |commands: &mut Commands,
                 material: &Handle<StandardMaterial>,
                 at: Vec3,
                 size: Vec3,
                 solid: bool| {
        let world = spin * at + Vec3::new(frame.center.x, SIDEWALK_HEIGHT, frame.center.y);
        let mut piece = commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(world)
                .with_rotation(spin)
                .with_scale(size),
            VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin: (range * 0.9)..range,
                use_aabb: false,
            },
            NotShadowCaster,
        ));
        if solid {
            piece.insert((RigidBody::Static, Collider::cuboid(1.0, 1.0, 1.0)));
        }
    };

    // Floor, ceiling, and the wall linings. The door gap in the front lining
    // matches the shell's carved opening and the collider's plates — all
    // three read the same `shell::door_*` contract.
    piece(
        commands,
        &kit.floor,
        Vec3::new(0.0, -0.03, 0.0),
        Vec3::new(w, LINING, d),
        false,
    );
    piece(
        commands,
        &kit.ceiling,
        Vec3::new(0.0, head + LINING * 0.5, 0.0),
        Vec3::new(w, LINING, d),
        false,
    );
    let lining = |offset: f32| offset - WALL - LINING * 0.5;
    for side in [-1.0f32, 1.0] {
        piece(
            commands,
            &kit.wall,
            Vec3::new(side * lining(frame.width * 0.5), head * 0.5, 0.0),
            Vec3::new(LINING, head, d),
            false,
        );
    }
    piece(
        commands,
        &kit.wall,
        Vec3::new(0.0, head * 0.5, -lining(frame.depth * 0.5)),
        Vec3::new(w, head, LINING),
        false,
    );
    let door = shell::door_width(frame.class, frame.width);
    let flank = (w - door) * 0.5;
    for side in [-1.0f32, 1.0] {
        piece(
            commands,
            &kit.wall,
            Vec3::new(
                side * (door + flank) * 0.5,
                head * 0.5,
                lining(frame.depth * 0.5),
            ),
            Vec3::new(flank, head, LINING),
            false,
        );
    }

    // Furnishing, to the kind. Laid out from the building's own seed so the
    // shop shelves stand where they stood the last time the chunk streamed
    // in. Everything with a collider is `solid` — a shelf you can walk
    // through is worse than no shelf.
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x1D00_2BAD_F00D_5EED);
    match kind {
        BuildingKind::Supermarket => {
            let aisles = (((w - 3.0) / 2.4) as usize).clamp(2, 6);
            let pitch = (w - 2.0) / aisles as f32;
            for aisle in 0..aisles {
                let x = -((aisles - 1) as f32) * 0.5 * pitch + aisle as f32 * pitch;
                piece(
                    commands,
                    &kit.shelf,
                    Vec3::new(x, 0.85, -d * 0.08),
                    Vec3::new(0.6, 1.7, d * 0.5),
                    true,
                );
            }
            // The till, beside the door.
            piece(
                commands,
                &kit.counter,
                Vec3::new(-w * 0.28, 0.5, d * 0.32),
                Vec3::new(w * 0.3, 1.0, 0.7),
                true,
            );
        }
        BuildingKind::Restaurant => {
            let across = ((w / 2.6) as usize).clamp(2, 5);
            let deep = ((d / 3.0) as usize).clamp(1, 4);
            for i in 0..across {
                for j in 0..deep {
                    // Not every table made it through last night.
                    if rng.random_range(0.0..1.0) < 0.18 {
                        continue;
                    }
                    let x = (i as f32 - (across - 1) as f32 * 0.5) * (w / across as f32);
                    let z = (j as f32 - (deep - 1) as f32 * 0.5) * (d * 0.6 / deep as f32);
                    piece(
                        commands,
                        &kit.table,
                        Vec3::new(x, 0.38, z),
                        Vec3::new(0.9, 0.76, 0.9),
                        true,
                    );
                }
            }
            piece(
                commands,
                &kit.counter,
                Vec3::new(0.0, 0.55, -d * 0.42),
                Vec3::new(w * 0.5, 1.1, 0.7),
                true,
            );
        }
        // The two lobbies share a plan; the materials tell them apart.
        _ => {
            piece(
                commands,
                &kit.counter,
                Vec3::new(0.0, 0.55, -d * 0.38),
                Vec3::new(w * 0.42, 1.1, 0.8),
                true,
            );
            for side in [-1.0f32, 1.0] {
                piece(
                    commands,
                    &kit.sofa,
                    Vec3::new(side * w * 0.3, 0.28, d * 0.22),
                    Vec3::new(1.8, 0.56, 0.8),
                    true,
                );
            }
        }
    }

    // The member of staff, behind the counter, facing the door, feeling
    // however they feel. Their temperament and wardrobe come off the
    // building's seed — the same clerk in the same shop every visit, which
    // matters once a grudge is involved.
    let Some(cast) = cast else { return };
    let temper = cast.tempers.draw(&mut rng);
    let mood = temper.baseline;
    let level = crate::mood::face::level_of(mood);
    // Behind their own counter: the supermarket's till stands beside the
    // door, everybody else's counter guards the back wall.
    let post = match kind {
        BuildingKind::Supermarket => Vec3::new(-w * 0.28, 0.0, d * 0.32 - 1.1),
        _ => Vec3::new(0.0, 0.0, -d * 0.42 + 1.0),
    };
    let at = spin * post
        + Vec3::new(
            frame.center.x,
            SIDEWALK_HEIGHT + STAND_HEIGHT,
            frame.center.y,
        );
    let mut clerk = commands.spawn((
        Name::new("Staff"),
        Staff,
        ChunkOf(chunk),
        Transform::from_translation(at)
            // The figure's shoes point down -Z, so facing the door is a
            // half-turn on top of the frame's yaw.
            .with_rotation(Quat::from_rotation_y(frame.yaw + std::f32::consts::PI)),
        RigidBody::Dynamic,
        Collider::capsule(RADIUS, HEIGHT),
        LockedAxes::ROTATION_LOCKED,
        Bouncer::new(STAND_HEIGHT),
        temper,
        Mood::new(mood),
        FaceLevel(level),
        Voicebox::new(rng.random_range(0.85..1.22)),
        Provoker::default(),
        crate::ai::archetype::Archetype::Everyday,
        Visibility::default(),
    ));
    crate::ai::figure::dress(
        &mut clerk,
        cast.figures,
        kit.coat_for(kind),
        level,
        crate::ai::archetype::Archetype::Everyday,
        &mut rng,
    );
}

// Two facts held at compile time, the way the figure's proportions are.
// The visual lining must sit within the collider's plate, or a wall you can
// lean into shows its own back; and the interior must still be drawn while
// the full-detail shell — the only one with an actual opening — is on
// screen, with slack for the crossfade band.
const _: () = assert!(LINING < WALL);
const _: () = assert!(RANGE > shell::NEAR * 1.1);

#[cfg(test)]
mod place_tests {
    use super::PlaceId;

    #[test]
    fn place_ids_are_stable_under_repeated_generation() {
        let position = bevy::prelude::Vec3::new(12.34, 2.3, -56.78);
        assert_eq!(PlaceId::at(position), PlaceId::at(position));
        assert_eq!(
            PlaceId::at(position),
            PlaceId::at(position + bevy::prelude::Vec3::Y * 9.0)
        );
    }

    #[test]
    fn neighbouring_fronts_get_different_ids() {
        assert_ne!(
            PlaceId::at(bevy::prelude::Vec3::new(0.0, 2.3, 0.0)),
            PlaceId::at(bevy::prelude::Vec3::new(0.2, 2.3, 0.0))
        );
    }
}
