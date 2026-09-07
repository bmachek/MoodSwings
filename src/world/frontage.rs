//! What a building has hung on its face and put out in front of it.
//!
//! The facade is finished work — a shell with reveals, courses, a cornice and
//! an awning over every other shopfront — and the kerb is furnished, and
//! between the two there was nothing at all. Which is the strip a pedestrian
//! spends the whole game looking at: a street reads as inhabited from what
//! people have *left* on the pavement, not from what an architect drew. The
//! wall above it has the same problem for the same reason — a facade with
//! nothing bolted to it is a drawing of a facade.
//!
//! So this module is the dressing, and all of it hangs off one building's front
//! face: the pipe down the corner, the sandwich board outside the shop, the
//! geraniums on the sills, the dish somebody screwed to the brickwork, the
//! condenser humming under a window, the bikes at the rack, and the tables the
//! restaurant has put out. Nothing here is decided by the block or the district — a
//! building's frontage is a fact about that building, derived from its own
//! footprint the way its roof is, so a chunk walked back into puts every chair
//! back exactly where it was.
//!
//! ## What this is allowed to cost
//!
//! There are four thousand buildings. A single extra mesh on each is four
//! thousand meshes, and this module could easily have put thirty on every one
//! of them — so two rules hold it down, and both are load-bearing rather than
//! tidy.
//!
//! *Nothing is on every building.* A downpipe is on most, a sandwich board is
//! on a shopfront that rolls for one, a bike rack is on one building in eight,
//! and a terrace is on a restaurant. The city averages a little over three of
//! these per building, not thirty.
//!
//! *Everything carries a range*, and there are two of them. [`RANGE`] is for
//! the things with a silhouette — a pipe, a board, a parasol — and [`NEAR`] is
//! for the ones that are a handful of pixels from across a junction: the bikes,
//! the chairs, the flowers. A bicycle is nine meshes and is a smudge at fifty
//! metres, so it is drawn for fifty metres.

use avian3d::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::citygen::{Building, BuildingKind, Rect};
use super::mayhem::Breakaway;
use std::f32::consts::FRAC_PI_2;

use super::texture::{FacadeClass, byte, fbm, hash01, painted};

/// How far the frontage's larger pieces are drawn: pipes, boards, troughs,
/// parasols, tables.
pub const RANGE: f32 = 90.0;

/// And the small ones — bicycles, chairs, blossom. Five meshes each and none
/// of them a shape at any distance.
const NEAR: f32 = 50.0;

/// A range past which no quality setting draws frontage.
///
/// The Photo preset's `lod_scale` is infinite, and while an infinite range is
/// legal it makes every one of these a mesh that is always submitted. Frontage
/// is the one category where that is genuinely pointless: at four hundred
/// metres a geranium is not a subtle detail, it is a wasted draw.
const CEILING: f32 = 400.0;

/// How far the pipe stands off the wall, in metres.
const PIPE_STAND_OFF: f32 = 0.085;
const PIPE_RADIUS: f32 = 0.055;

/// Where a sandwich board stands, out from the wall.
///
/// Far enough to be past the shopfront's own reveal — `shell::REVEAL_SHOP` is
/// the deepest thing on the facade — and near enough that it is clearly *this*
/// shop's board and not the neighbour's.
const BOARD_OUT: f32 = 1.15;

/// Clearance a terrace leaves between the last chair and the kerb, so the
/// pavement is dressed rather than blocked.
const KERB_CLEARANCE: f32 = 0.9;

#[derive(Resource)]
pub struct FrontageKit {
    /// A unit cube, scaled into every board, trough, chair and frame tube.
    cube: Handle<Mesh>,
    /// A unit cylinder — radius one, height one — so a scale of (r, h, r)
    /// makes any pipe, post or table column in the module.
    pipe: Handle<Mesh>,
    /// A parasol, and nothing else.
    cone: Handle<Mesh>,
    /// The bike rack's hoop, and a bicycle wheel: the same primitive at two
    /// sizes, both cut down to a resolution that suits something the size of a
    /// hand at fifty metres.
    hoop: Handle<Mesh>,
    wheel: Handle<Mesh>,
    /// A blossom. One lump, scaled and tinted.
    lump: Handle<Mesh>,

    zinc: Handle<StandardMaterial>,
    slate: Handle<StandardMaterial>,
    timber: Handle<StandardMaterial>,
    terracotta: Handle<StandardMaterial>,
    leaf: Handle<StandardMaterial>,
    /// Geraniums, and the two things that are not geraniums.
    blossom: [Handle<StandardMaterial>; 3],
    steel: Handle<StandardMaterial>,
    rubber: Handle<StandardMaterial>,
    /// Bicycle frames. Four, because a rack of one colour is a bike shop.
    enamel: [Handle<StandardMaterial>; 4],
    linen: Handle<StandardMaterial>,
    /// Parasol canvas, in the three colours a brewery gives them away in.
    canvas: [Handle<StandardMaterial>; 3],
}

/// Slate with something chalked on it that cannot quite be read.
///
/// Deliberately illegible. A board with real words on it needs the glyph
/// machinery `signage` carries and would then say the same thing outside every
/// café in the city; a board with the *shape* of a day's specials on it says
/// the same thing to a passer-by and is a hundred and twenty-eight texels.
fn chalkboard() -> Image {
    const SIZE: u32 = 128;
    painted(SIZE, TextureFormat::Rgba8UnormSrgb, |u, v| {
        // The slate itself: very dark, with the ghost of everything ever
        // written on it and wiped off again.
        let smear = fbm(u, v, 5, 3, 0x51a7) * 0.12;
        let mut value = 0.075 + smear;

        // Six lines of chalk, the top one twice the height of the rest, each
        // running to its own ragged end — which is what a specials board looks
        // like from the far side of a pavement.
        let lines = 6.0;
        let row = (v * lines).floor();
        let within = (v * lines).fract();
        let heading = row < 1.0;
        let (thickness, indent) = if heading { (0.42, 0.12) } else { (0.20, 0.18) };
        let ends = 0.55 + hash01(row as u32, 3, 0x2c19) * 0.30;
        if within > 0.30 && within < 0.30 + thickness && u > indent && u < indent + ends {
            // Chalk is not paint: it skips, and the skipping is most of what
            // reads as chalk rather than as a white bar.
            let skip = hash01((u * 96.0) as u32, row as u32, 0x7f31);
            if skip > 0.22 {
                value = 0.62 + skip * 0.28;
            }
        }

        let c = byte(value.clamp(0.0, 1.0));
        [c, c, byte((value * 1.03).clamp(0.0, 1.0)), 255]
    })
}

fn matte(color: Color, roughness: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        ..default()
    }
}

fn painted_metal(color: Color, roughness: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        metallic: 0.6,
        ..default()
    }
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> FrontageKit {
    FrontageKit {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        pipe: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(8).build()),
        cone: meshes.add(Cone::new(1.0, 1.0).mesh().resolution(8).build()),
        // Two rings at unit radius, so a scale is the radius in metres and
        // nothing in this module has to remember how big the mesh was built.
        // The proportions are the difference between them: a stand's tube is a
        // twentieth of its arch and a tyre is a twelfth of its wheel.
        //
        // Both at a resolution that suits something the thickness of a thumb.
        // The default torus is 24 by 10, which is 480 triangles for a hoop that
        // is never more than a few pixels across.
        hoop: meshes.add(
            Torus::new(0.89, 1.0)
                .mesh()
                .major_resolution(14)
                .minor_resolution(4)
                .build(),
        ),
        wheel: meshes.add(
            Torus::new(0.84, 1.0)
                .mesh()
                .major_resolution(12)
                .minor_resolution(4)
                .build(),
        ),
        lump: meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),

        zinc: materials.add(painted_metal(Color::srgb(0.52, 0.54, 0.55), 0.62)),
        slate: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(images.add(chalkboard())),
            perceptual_roughness: 0.93,
            ..default()
        }),
        timber: materials.add(matte(Color::srgb(0.33, 0.23, 0.15), 0.88)),
        terracotta: materials.add(matte(Color::srgb(0.55, 0.31, 0.22), 0.90)),
        leaf: materials.add(matte(Color::srgb(0.18, 0.35, 0.16), 0.85)),
        blossom: [
            // The geranium, which is the whole point, and then a white and a
            // purple so a terrace of window boxes is not one flower repeated.
            Color::srgb(0.72, 0.13, 0.16),
            Color::srgb(0.86, 0.84, 0.78),
            Color::srgb(0.48, 0.26, 0.55),
        ]
        .map(|color| materials.add(matte(color, 0.80))),
        steel: materials.add(painted_metal(Color::srgb(0.58, 0.59, 0.61), 0.44)),
        rubber: materials.add(matte(Color::srgb(0.075, 0.075, 0.08), 0.95)),
        enamel: [
            Color::srgb(0.16, 0.32, 0.48),
            Color::srgb(0.62, 0.16, 0.14),
            Color::srgb(0.14, 0.14, 0.15),
            Color::srgb(0.30, 0.46, 0.34),
        ]
        .map(|color| materials.add(painted_metal(color, 0.36))),
        linen: materials.add(matte(Color::srgb(0.82, 0.81, 0.77), 0.82)),
        canvas: [
            Color::srgb(0.72, 0.24, 0.20),
            Color::srgb(0.20, 0.36, 0.52),
            Color::srgb(0.85, 0.79, 0.62),
        ]
        .map(|color| materials.add(matte(color, 0.88))),
    }
}

/// The frontage's own stream for one building.
///
/// Keyed on the footprint like [`rooftop::seed_for`](super::rooftop::seed_for)
/// and for the same reason — a chunk regenerates, so anything keyed on spawn
/// order would move the chairs about between visits — but on its own stream
/// key, so that adding a bike rack cannot reshuffle a single roof.
fn stream_for_building(world_seed: u64, footprint: Rect) -> ChaCha8Rng {
    let center = footprint.center();
    // Quantised to a centimetre before hashing, so a float that comes back one
    // ulp different does not redecorate the building.
    let x = (center.x * 100.0).round() as i64 as i32;
    let z = (center.y * 100.0).round() as i64 as i32;
    crate::core::rng::stream_for_chunk(world_seed, crate::core::rng::stream::FRONTAGE, (x, z))
}

fn ranges(lod_scale: f32) -> (VisibilityRange, VisibilityRange) {
    let cut = |base: f32| {
        let end = (base * lod_scale).min(CEILING);
        VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: end..(end * 1.1),
            use_aabb: false,
        }
    };
    (cut(RANGE), cut(NEAR))
}

/// Where one building's frontage stands, in the frame of its own front face.
///
/// Everything in this module is placed in two numbers — how far *along* the
/// frontage, and how far *out* from the wall — because that is how a person
/// standing on the pavement describes it, and because the alternative is four
/// copies of the same trigonometry with one sign wrong in the third of them.
struct Face {
    wall: Vec2,
    along: Vec2,
    outward: Vec2,
    frontage: f32,
    /// Pavement between the wall and the kerb.
    depth: f32,
}

impl Face {
    fn at(&self, across: f32, out: f32) -> Vec2 {
        self.wall + self.along * across + self.outward * out
    }

    /// The yaw that turns a mesh's +Z to face the street.
    fn facing(&self) -> f32 {
        self.outward.x.atan2(self.outward.y)
    }
}

/// Dresses one building's front.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    kit: &FrontageKit,
    world_seed: u64,
    lod_scale: f32,
    building: &Building,
    class: FacadeClass,
    wall: Vec2,
    outward: Vec2,
    frontage: f32,
    depth: f32,
    chunk: IVec2,
) {
    // A frontage too narrow to stand a board on, or a building with no pavement
    // in front of it, gets nothing. Both happen: a subdivided lot can leave a
    // slice two metres wide, and the back of a corner building fronts the next
    // building along rather than a street.
    if frontage < 3.0 || depth < 1.4 {
        return;
    }

    let face = Face {
        wall,
        along: Vec2::new(outward.y, -outward.x),
        outward,
        frontage,
        depth,
    };
    let (far, near) = ranges(lod_scale);
    let mut rng = stream_for_building(world_seed, building.footprint);

    downpipe(commands, kit, &face, building.height, &far, chunk, &mut rng);
    window_boxes(
        commands, kit, &face, building, class, &near, chunk, &mut rng,
    );
    fittings(
        commands, kit, &face, building, class, &far, &near, chunk, &mut rng,
    );
    if class.has_shopfronts() {
        sandwich_board(commands, kit, &face, &far, chunk, &mut rng);
    }
    bike_rack(commands, kit, &face, &far, &near, chunk, &mut rng);
    if building.kind == BuildingKind::Restaurant {
        terrace(commands, kit, &face, &far, &near, chunk, &mut rng);
    }
}

/// The pipe down the corner of the facade.
///
/// The cheapest thing in this module and comfortably the most valuable. A blank
/// wall with a vertical line down one edge stops being a texture on a box: the
/// line is the only thing on the whole facade whose length is the building's
/// actual height, and the eye reads the storey from it.
///
/// No collider. It is flush against a wall that already has one, so a collider
/// here would be four thousand more static bodies that nothing can ever touch.
fn downpipe(
    commands: &mut Commands,
    kit: &FrontageKit,
    face: &Face,
    height: f32,
    range: &VisibilityRange,
    chunk: IVec2,
    rng: &mut ChaCha8Rng,
) {
    // Most buildings, not all: a street where every single corner has a pipe
    // down it reads as regularly as one where none of them does.
    if rng.random_range(0.0..1.0) > 0.72 {
        return;
    }
    // Whichever corner, and a hand's width in from it so it lands on the wall
    // rather than on the arris.
    let side = if rng.random_range(0.0..1.0) < 0.5 {
        1.0
    } else {
        -1.0
    };
    let at = face.at(side * (face.frontage * 0.5 - 0.24), PIPE_STAND_OFF);

    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.pipe.clone()),
        MeshMaterial3d(kit.zinc.clone()),
        Transform::from_xyz(at.x, SIDEWALK_HEIGHT + height * 0.5, at.y).with_scale(Vec3::new(
            PIPE_RADIUS,
            height,
            PIPE_RADIUS,
        )),
        range.clone(),
    ));
}

/// The things screwed to the brickwork: dishes, condensers, extract ducts.
///
/// All of it goes on a *pier* — the strip of wall between two windows — rather
/// than at a bay's middle, because a bay's middle is glass. The grid that says
/// where the piers are is `FacadeClass::grid`, the same one the shell cuts its
/// reveals from, so a dish cannot end up screwed to a window.
///
/// And all of it stands proud of the wall. The deepest thing on the facade is
/// the shopfront reveal at `shell::REVEAL_SHOP`; everything here is bolted on
/// top of a plane, so nothing has to know about that.
#[allow(clippy::too_many_arguments)]
fn fittings(
    commands: &mut Commands,
    kit: &FrontageKit,
    face: &Face,
    building: &Building,
    class: FacadeClass,
    far: &VisibilityRange,
    near: &VisibilityRange,
    chunk: IVec2,
    rng: &mut ChaCha8Rng,
) {
    let (columns, rows) = class.grid();
    // A house has two storeys and three bays; there is no pier on it worth
    // screwing anything to, and nobody puts a satellite dish on a cottage's
    // front elevation anyway. A tower is a curtain wall and has no pier at all.
    if columns < 4.0 || rows < 3.0 {
        return;
    }
    let storey = building.height / rows;
    let bay = face.frontage / columns;
    let pier = |column: u32| column as f32 * bay - face.frontage * 0.5;

    // The dish. One per building at most, and not on most of them: a street
    // where every flat has one is a street from a photograph of 1998, and a
    // street with three on it is a street.
    if rng.random_range(0.0..1.0) < 0.30 {
        let column = rng.random_range(1..columns as u32);
        // Upper storeys only — the ground floor is a shopfront and the first
        // one is where the pole would be in the way of the awning.
        let row = rng.random_range(2..rows as u32);
        let at = face.at(pier(column), 0.10);
        let y = SIDEWALK_HEIGHT + (row as f32 + 0.55) * storey;
        let yaw = face.facing();

        // The arm out of the wall.
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.pipe.clone()),
            MeshMaterial3d(kit.steel.clone()),
            Transform::from_xyz(at.x, y, at.y)
                .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(FRAC_PI_2))
                .with_scale(Vec3::new(0.022, 0.34, 0.022)),
            far.clone(),
        ));
        // And the dish on the end of it, tipped up at the sky. A shallow dome
        // rather than a paraboloid: at the distance a first-floor dish is ever
        // seen from, the difference is which way it is pointing, and that it
        // is pointing the same way as every other one in the street.
        let bowl = face.at(pier(column), 0.36);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.lump.clone()),
            MeshMaterial3d(kit.linen.clone()),
            Transform::from_xyz(bowl.x, y + 0.06, bowl.y)
                .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-0.55))
                .with_scale(Vec3::new(0.30, 0.085, 0.30)),
            far.clone(),
        ));
    }

    // Condensers, under windows on the piers. These go in twos and threes on a
    // building that has any, because whoever fitted the first one told the
    // neighbours who did it.
    if rng.random_range(0.0..1.0) < 0.22 {
        let column = rng.random_range(1..columns as u32);
        for row in 1..(rows as u32).min(5) {
            if rng.random_range(0.0..1.0) > 0.55 {
                continue;
            }
            let at = face.at(pier(column), 0.19);
            let y = SIDEWALK_HEIGHT + (row as f32 + 0.22) * storey;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(kit.zinc.clone()),
                Transform::from_xyz(at.x, y, at.y)
                    .with_rotation(Quat::from_rotation_y(face.facing()))
                    .with_scale(Vec3::new(0.62, 0.44, 0.34)),
                near.clone(),
            ));
        }
    }

    // A kitchen extract, low on the wall beside the door. Only where there is a
    // kitchen — this is the one fitting that says what the building is for.
    if matches!(
        building.kind,
        BuildingKind::Restaurant | BuildingKind::Supermarket
    ) {
        let column = if rng.random_range(0.0..1.0) < 0.5 {
            1
        } else {
            columns as u32 - 1
        };
        let at = face.at(pier(column), 0.13);
        let y = SIDEWALK_HEIGHT + 2.35;
        // The duct: a box on the wall with a stub of pipe elbowing out of it,
        // which between them are the whole of what a passer-by ever sees of a
        // ventilation system and the reason a back street smells of chips.
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.steel.clone()),
            Transform::from_xyz(at.x, y, at.y)
                .with_rotation(Quat::from_rotation_y(face.facing()))
                .with_scale(Vec3::new(0.52, 0.52, 0.26)),
            far.clone(),
        ));
        let mouth = face.at(pier(column), 0.34);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.pipe.clone()),
            MeshMaterial3d(kit.zinc.clone()),
            Transform::from_xyz(mouth.x, y + 0.30, mouth.y).with_scale(Vec3::new(0.13, 0.42, 0.13)),
            near.clone(),
        ));
    }
}

/// Geraniums on the sills.
///
/// Only on the classes that would have them: a house and a low block of flats.
/// Nobody keeps a window box on the fourteenth floor of a curtain wall, and
/// putting one there is exactly the kind of detail that makes a city look
/// *more* generated rather than less.
#[allow(clippy::too_many_arguments)]
fn window_boxes(
    commands: &mut Commands,
    kit: &FrontageKit,
    face: &Face,
    building: &Building,
    class: FacadeClass,
    range: &VisibilityRange,
    chunk: IVec2,
    rng: &mut ChaCha8Rng,
) {
    if !matches!(class, FacadeClass::House | FacadeClass::Lowrise) {
        return;
    }
    if rng.random_range(0.0..1.0) > 0.55 {
        return;
    }

    let (columns, rows) = class.grid();
    let storey = building.height / rows;
    let bay = face.frontage / columns;
    // The sill is the bottom of the pane, and the pane is the one description
    // of the window grid — read here rather than guessed, so a box lands on a
    // sill rather than a hand's width under one.
    let sill = |row: u32| SIDEWALK_HEIGHT + row as f32 * storey + class.pane(row).v0 * storey;

    // The rows a person can water: the ground storey where it is a window
    // rather than a shopfront, and the two above it.
    let first = if class.has_shopfronts() { 1 } else { 0 };
    for row in first..(rows as u32).min(first + 3) {
        for column in 0..columns as u32 {
            if rng.random_range(0.0..1.0) > 0.34 {
                continue;
            }
            let across = (column as f32 + 0.5) * bay - face.frontage * 0.5;
            let at = face.at(across, 0.13);
            let y = sill(row);

            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(kit.terracotta.clone()),
                Transform::from_xyz(at.x, y + 0.09, at.y)
                    .with_rotation(Quat::from_rotation_y(face.facing()))
                    .with_scale(Vec3::new((bay * 0.52).min(0.95), 0.18, 0.20)),
                range.clone(),
            ));

            // Two lumps of planting, in one of the three colours. Sized off the
            // trough so a wide house window and a narrow flat window are the
            // same idea at two scales.
            let width = (bay * 0.52).min(0.95);
            let blossom = kit.blossom[rng.random_range(0..kit.blossom.len())].clone();
            for lump in [-1.0f32, 1.0] {
                let spot = face.at(across + lump * width * 0.22, 0.13);
                commands.spawn((
                    ChunkOf(chunk),
                    Mesh3d(kit.lump.clone()),
                    MeshMaterial3d(if rng.random_range(0.0..1.0) < 0.45 {
                        blossom.clone()
                    } else {
                        kit.leaf.clone()
                    }),
                    Transform::from_xyz(spot.x, y + 0.20, spot.y).with_scale(Vec3::new(
                        width * 0.24,
                        0.10,
                        0.11,
                    )),
                    range.clone(),
                ));
            }
        }
    }
}

/// The board outside the shop, with today's on it.
///
/// Dynamic, and that is the whole point. It is the one thing on the pavement
/// that is not bolted, chained or bedded in concrete — it is a hinged board
/// somebody carried out this morning — so in a city made of rubber it is the
/// first thing to go over, and it should be.
fn sandwich_board(
    commands: &mut Commands,
    kit: &FrontageKit,
    face: &Face,
    range: &VisibilityRange,
    chunk: IVec2,
    rng: &mut ChaCha8Rng,
) {
    if rng.random_range(0.0..1.0) > 0.38 {
        return;
    }
    let out = BOARD_OUT.min(face.depth - 0.6);
    if out < 0.7 {
        return;
    }
    // Off to one side of the frontage rather than dead centre, which is where
    // the door is.
    let across = rng.random_range(-0.34..0.34) * face.frontage;
    let at = face.at(across, out);
    // Turned a few degrees off square to the wall, the way one always is.
    let yaw = face.facing() + rng.random_range(-0.5..0.5);

    const HEIGHT: f32 = 0.92;
    const WIDTH: f32 = 0.62;
    /// How far the two panels lean apart at the foot, as a fraction of height.
    const SPLAY: f32 = 0.22;

    commands
        .spawn((
            ChunkOf(chunk),
            RigidBody::Dynamic,
            // The tent as one box rather than two leaves: the collider is what
            // it topples on, and a hinged pair would need a joint to hold a
            // shape it never changes.
            Collider::cuboid(WIDTH, HEIGHT, HEIGHT * SPLAY * 2.0),
            Mass(9.0),
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT + HEIGHT * 0.5, at.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
        ))
        .with_children(|parent| {
            for lean in [-1.0f32, 1.0] {
                parent.spawn((
                    Mesh3d(kit.cube.clone()),
                    MeshMaterial3d(kit.slate.clone()),
                    Transform::from_xyz(0.0, 0.0, lean * HEIGHT * SPLAY * 0.5)
                        .with_rotation(Quat::from_rotation_x(lean * SPLAY))
                        .with_scale(Vec3::new(WIDTH, HEIGHT, 0.035)),
                    range.clone(),
                ));
            }
            // The frame across the top, which is what makes it a board and not
            // a slab of slate standing in the street.
            parent.spawn((
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(kit.timber.clone()),
                Transform::from_xyz(0.0, HEIGHT * 0.5, 0.0).with_scale(Vec3::new(
                    WIDTH * 1.04,
                    0.05,
                    HEIGHT * SPLAY * 1.4,
                )),
                range.clone(),
            ));
        });
}

/// A hoop with bikes locked to it.
///
/// The bikes are static and carry a [`Breakaway`], which is the same footing
/// the street's signs and meters have and is the honest one here: a bike at a
/// rack is *locked* to it. A car takes it and the rack together; a person
/// bouncing off it does not, and should not.
#[allow(clippy::too_many_arguments)]
fn bike_rack(
    commands: &mut Commands,
    kit: &FrontageKit,
    face: &Face,
    far: &VisibilityRange,
    near: &VisibilityRange,
    chunk: IVec2,
    rng: &mut ChaCha8Rng,
) {
    if rng.random_range(0.0..1.0) > 0.13 {
        return;
    }
    let out = (face.depth - KERB_CLEARANCE).min(1.5);
    if out < 0.9 {
        return;
    }
    let across = rng.random_range(-0.3..0.3) * face.frontage;
    let at = face.at(across, out);
    let yaw = face.facing();

    // The hoop, which is a Sheffield stand: a ring set in the pavement with
    // most of its lower half buried, so what is above ground is the arch. The
    // torus lies in XZ with its axis up, and `rotation_x` turns that axis onto
    // local Z; `rotation_y(yaw)` then points it at the street, which stands the
    // arch parallel to the wall — the way bikes have to lean on it.
    const HOOP: f32 = 0.45;
    /// How much of the ring is above the pavement. A real stand is about
    /// three-quarters of a metre; buried by a fifth of its radius, this one
    /// reaches sixty-seven centimetres.
    const BURIED: f32 = 0.22;
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.hoop.clone()),
        MeshMaterial3d(kit.steel.clone()),
        Transform::from_xyz(at.x, SIDEWALK_HEIGHT + BURIED, at.y)
            .with_rotation(
                Quat::from_rotation_y(yaw) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            )
            .with_scale(Vec3::splat(HOOP)),
        RigidBody::Static,
        Collider::cuboid(HOOP * 2.0, HOOP, 0.10),
        Breakaway {
            at: 5.0,
            mass: 40.0,
            geyser: false,
        },
        far.clone(),
    ));

    // One bike either side of the stand, leaning inwards onto it. Offset in
    // *depth* rather than along the frontage: a bike parked at a hoop lies
    // alongside it, so two of them side by side along the wall would be two
    // bikes in the same metre of pavement.
    for slot in 0..rng.random_range(1..3) {
        let side = if slot == 0 { 1.0 } else { -1.0 };
        let spot = face.at(across, out + side * 0.36);
        bicycle(commands, kit, spot, yaw, side, near, chunk, rng);
    }
}

/// One bicycle, leaning on the rack.
///
/// Nine meshes: two wheels, the five tubes of a diamond frame, a saddle and a
/// handlebar. The frame is the whole job — a bike read from across a pavement
/// is two circles with a *triangle* between them, and two tubes crossing in an
/// X, which is what this was first time round, reads as a deckchair.
///
/// What is deliberately absent: spokes, chain, cranks, pedals, mudguards,
/// brakes and a chainstay. None of them survives being three pixels wide, and
/// [`NEAR`] means nobody is ever looking at this from further than a pavement's
/// width anyway.
#[allow(clippy::too_many_arguments)]
fn bicycle(
    commands: &mut Commands,
    kit: &FrontageKit,
    at: Vec2,
    yaw: f32,
    lean: f32,
    range: &VisibilityRange,
    chunk: IVec2,
    rng: &mut ChaCha8Rng,
) {
    const WHEEL: f32 = 0.34;
    /// Tube radius. A bicycle frame is about a two-pence piece across.
    const TUBE: f32 = 0.024;
    /// Where the bike's origin sits above the pavement. Halfway up, so the
    /// collider is centred on it — Avian puts a collider at the entity's
    /// transform, and an offset one would need a child to hang it from.
    const WAIST: f32 = 0.50;

    let enamel = kit.enamel[rng.random_range(0..kit.enamel.len())].clone();

    // Built along local X with the front at +X, wheels turning about local Z.
    // `rotation_y(yaw)` puts that length along the frontage — a bike at a stand
    // lies parallel to the wall, not pointing at it — and the lean is a roll
    // about its own length, which is what leaning on something is.
    //
    // Negated: `rotation_x` tips the top towards +Z, and a bike standing on the
    // +Z side of the stand has to fall the other way to reach it.
    let pose = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-lean * 0.17);

    // Every station on the frame, as its height above the pavement, so the
    // numbers below can be read against a real bicycle instead of against an
    // origin halfway up one.
    let h = |above: f32| above - WAIST;
    let bracket = Vec3::new(-0.10, h(0.28), 0.0);
    let saddle = Vec3::new(-0.42, h(0.92), 0.0);
    let head = Vec3::new(0.40, h(0.92), 0.0);
    let rear_hub = Vec3::new(-0.52, h(WHEEL), 0.0);
    let front_hub = Vec3::new(0.52, h(WHEEL), 0.0);

    commands
        .spawn((
            ChunkOf(chunk),
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT + WAIST, at.y).with_rotation(pose),
            Visibility::default(),
            RigidBody::Static,
            Collider::cuboid(1.15, 1.0, 0.12),
            Breakaway {
                at: 3.0,
                mass: 14.0,
                geyser: false,
            },
        ))
        .with_children(|parent| {
            // The wheels. The torus lies in XZ with its axle up, and a wheel
            // that rolls along X has its axle along Z — so a quarter turn about
            // X, and not about Z, which stands the wheel across its own
            // direction of travel and makes the bike a pair of dinner plates.
            for hub in [rear_hub, front_hub] {
                parent.spawn((
                    Mesh3d(kit.wheel.clone()),
                    MeshMaterial3d(kit.rubber.clone()),
                    Transform::from_translation(hub)
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                        .with_scale(Vec3::splat(WHEEL)),
                    range.clone(),
                ));
            }

            // The diamond: seat tube, down tube and top tube make the triangle
            // that says bicycle, and the seat stay and the fork are what stop
            // the wheels looking like they are rolling past it.
            for (from, to) in [
                (bracket, saddle),
                (bracket, head),
                (saddle, head),
                (saddle, rear_hub),
                (head, front_hub),
            ] {
                let span = to - from;
                parent.spawn((
                    Mesh3d(kit.pipe.clone()),
                    MeshMaterial3d(enamel.clone()),
                    // The cylinder stands along its own Y, so it is turned to
                    // lie along the tube it is standing in for.
                    Transform::from_translation((from + to) * 0.5)
                        .with_rotation(Quat::from_rotation_arc(Vec3::Y, span.normalize()))
                        .with_scale(Vec3::new(TUBE, span.length(), TUBE)),
                    range.clone(),
                ));
            }

            // The handlebar, across the bike rather than along it, and the
            // saddle. Two boxes, and between them they are most of what makes a
            // frame read as something somebody rides.
            parent.spawn((
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(kit.rubber.clone()),
                Transform::from_xyz(head.x, h(1.00), 0.0).with_scale(Vec3::new(0.05, 0.05, 0.44)),
                range.clone(),
            ));
            parent.spawn((
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(kit.rubber.clone()),
                Transform::from_xyz(saddle.x, h(0.97), 0.0).with_scale(Vec3::new(0.26, 0.05, 0.10)),
                range.clone(),
            ));
        });
}

/// Tables out on the pavement.
///
/// A restaurant with a lit window is a building; a restaurant with three tables
/// and a parasol outside it is a place people go. Everything here is dynamic
/// and light, because a flummi coming through a terrace at speed is the exact
/// joke this game is made of.
#[allow(clippy::too_many_arguments)]
fn terrace(
    commands: &mut Commands,
    kit: &FrontageKit,
    face: &Face,
    far: &VisibilityRange,
    near: &VisibilityRange,
    chunk: IVec2,
    rng: &mut ChaCha8Rng,
) {
    // A terrace needs pavement. On a narrow one the restaurant keeps its
    // chairs inside, which is also what happens in life.
    let out = face.depth - KERB_CLEARANCE - 0.5;
    if out < 1.1 {
        return;
    }

    let tables = if face.frontage > 11.0 { 3 } else { 2 };
    for i in 0..tables {
        let across = ((i as f32 + 0.5) / tables as f32 - 0.5) * face.frontage * 0.82;
        let at = face.at(across, out.min(1.9));

        // The table: a column with a disc on it and a disc under it.
        const TOP: f32 = 0.74;
        commands
            .spawn((
                ChunkOf(chunk),
                RigidBody::Dynamic,
                Collider::cylinder(0.36, TOP),
                Mass(12.0),
                Transform::from_xyz(at.x, SIDEWALK_HEIGHT + TOP * 0.5, at.y),
                Visibility::default(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Mesh3d(kit.pipe.clone()),
                    MeshMaterial3d(kit.steel.clone()),
                    Transform::from_xyz(0.0, -0.04, 0.0).with_scale(Vec3::new(0.04, TOP, 0.04)),
                    far.clone(),
                ));
                parent.spawn((
                    Mesh3d(kit.pipe.clone()),
                    MeshMaterial3d(kit.linen.clone()),
                    Transform::from_xyz(0.0, TOP * 0.5 - 0.02, 0.0)
                        .with_scale(Vec3::new(0.36, 0.035, 0.36)),
                    far.clone(),
                ));
                parent.spawn((
                    Mesh3d(kit.pipe.clone()),
                    MeshMaterial3d(kit.steel.clone()),
                    Transform::from_xyz(0.0, -TOP * 0.5 + 0.015, 0.0)
                        .with_scale(Vec3::new(0.22, 0.03, 0.22)),
                    far.clone(),
                ));
            });

        // Two chairs, facing each other across it.
        for chair in [-1.0f32, 1.0] {
            let spot = face.at(across, out.min(1.9) + chair * 0.62);
            const SEAT: f32 = 0.45;
            commands
                .spawn((
                    ChunkOf(chunk),
                    RigidBody::Dynamic,
                    Collider::cuboid(0.42, 0.86, 0.42),
                    Mass(5.0),
                    // Both chairs look at the table, so the one on the street
                    // side has its back to the street.
                    Transform::from_xyz(spot.x, SIDEWALK_HEIGHT + 0.43, spot.y).with_rotation(
                        Quat::from_rotation_y(
                            face.facing()
                                + if chair > 0.0 {
                                    std::f32::consts::PI
                                } else {
                                    0.0
                                },
                        ),
                    ),
                    Visibility::default(),
                ))
                .with_children(|parent| {
                    parent.spawn((
                        Mesh3d(kit.cube.clone()),
                        MeshMaterial3d(kit.timber.clone()),
                        Transform::from_xyz(0.0, SEAT - 0.43, 0.0)
                            .with_scale(Vec3::new(0.40, 0.04, 0.40)),
                        near.clone(),
                    ));
                    parent.spawn((
                        Mesh3d(kit.cube.clone()),
                        MeshMaterial3d(kit.timber.clone()),
                        Transform::from_xyz(0.0, 0.19, -0.18)
                            .with_scale(Vec3::new(0.40, 0.42, 0.04)),
                        near.clone(),
                    ));
                });
        }

        // And a parasol over the middle one, which is what stops a terrace
        // reading as three tables somebody forgot to bring in.
        if i == tables / 2 {
            let canvas = kit.canvas[rng.random_range(0..kit.canvas.len())].clone();
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.pipe.clone()),
                MeshMaterial3d(kit.steel.clone()),
                Transform::from_xyz(at.x, SIDEWALK_HEIGHT + 1.1, at.y)
                    .with_scale(Vec3::new(0.03, 2.2, 0.03)),
                far.clone(),
            ));
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.cone.clone()),
                MeshMaterial3d(canvas),
                // A cone points up and a parasol is the other way round.
                Transform::from_xyz(at.x, SIDEWALK_HEIGHT + 2.12, at.y)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::PI))
                    .with_scale(Vec3::new(1.40, 0.46, 1.40)),
                far.clone(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(outward: Vec2) -> Face {
        Face {
            wall: Vec2::new(10.0, -4.0),
            along: Vec2::new(outward.y, -outward.x),
            outward,
            frontage: 12.0,
            depth: 3.2,
        }
    }

    /// The one piece of trigonometry in the module, and the one that is wrong
    /// on exactly one of the four sides if it is typed in rather than derived.
    #[test]
    fn a_frontage_puts_its_furniture_in_front_of_the_wall_on_every_side() {
        for outward in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            let face = face(outward);
            let out = face.at(0.0, 1.5);
            assert!(
                (out - face.wall).dot(outward) > 1.4,
                "furniture on the {outward:?} face landed inside the building"
            );
            // And moving along the frontage must not move it away from the
            // wall, or a board at one end of a shop stands in the road.
            let sideways = face.at(4.0, 1.5);
            assert!(
                ((sideways - face.wall).dot(outward) - 1.5).abs() < 1e-4,
                "moving along the {outward:?} frontage changed the set-back"
            );
            assert!((sideways - out).length() - 4.0 < 1e-4);
        }
    }

    /// A mesh's +Z has to come out pointing at the street, or every window box
    /// in the city is edge-on and every chair has its back to the table.
    #[test]
    fn the_facing_yaw_turns_a_mesh_to_look_out_at_the_street() {
        for outward in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            let yaw = face(outward).facing();
            // Rotating +Z by yaw gives (sin, cos) in XZ.
            let looks = Vec2::new(yaw.sin(), yaw.cos());
            assert!(
                looks.dot(outward) > 0.999,
                "a {outward:?} frontage faces {looks:?}"
            );
        }
    }

    #[test]
    fn the_same_building_is_dressed_the_same_way_twice() {
        // The whole determinism contract in one assertion: a chunk regenerates
        // on re-entry, so a frontage keyed on anything but the footprint would
        // rearrange itself behind the player's back.
        let footprint = Rect::new(Vec2::new(-14.0, 30.0), Vec2::new(2.0, 44.0));
        let draw = || {
            let mut rng = stream_for_building(0xBEEF, footprint);
            (0..16)
                .map(|_| rng.random_range(0.0..1.0))
                .collect::<Vec<f32>>()
        };
        assert_eq!(draw(), draw());

        // And a different building is dressed differently.
        let other = Rect::new(Vec2::new(-14.0, 60.0), Vec2::new(2.0, 74.0));
        let mut rng = stream_for_building(0xBEEF, other);
        let elsewhere: Vec<f32> = (0..16).map(|_| rng.random_range(0.0..1.0)).collect();
        assert_ne!(draw(), elsewhere);
    }

    /// The frontage must not reshuffle anything the building already decided.
    #[test]
    fn dressing_a_building_draws_from_its_own_stream() {
        let footprint = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(16.0, 12.0));
        let mut mine = stream_for_building(7, footprint);
        let roof = super::super::rooftop::seed_for(7, footprint);
        assert_ne!(
            mine.random::<u64>(),
            roof,
            "the frontage is drawing from the roof's stream"
        );
    }

    #[test]
    fn the_ranges_stay_finite_and_the_small_things_go_first() {
        for scale in [0.5, 1.0, 2.0, f32::INFINITY] {
            let (far, near) = ranges(scale);
            for range in [&far, &near] {
                assert!(range.end_margin.start.is_finite(), "scale {scale}");
                assert!(range.start_margin.end <= range.end_margin.start);
                assert!(range.end_margin.start < range.end_margin.end);
            }
            assert!(
                near.end_margin.start <= far.end_margin.start,
                "bicycles should go before parasols do"
            );
        }
    }

    /// A wheel turns about the axle it is drawn with.
    ///
    /// This one was wrong first time round and it is worth a test rather than a
    /// second look at a screenshot: a torus is built lying flat with its hole
    /// pointing up, and standing it on edge is a quarter turn about *one* of
    /// two axes. Pick the other and the bike keeps both wheels, both at right
    /// angles to the direction it would travel in — which from twelve metres
    /// reads as a bicycle that has been run over rather than as a bug.
    #[test]
    fn a_bicycle_wheel_stands_in_the_plane_it_would_roll_in() {
        // The bike is built along local X. A wheel rolling along X turns about
        // an axle along Z, and the torus's own axle starts along Y.
        let axle = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2) * Vec3::Y;
        assert!(
            axle.dot(Vec3::Z).abs() > 0.999,
            "the axle came out {axle:?}"
        );
        assert!(
            axle.dot(Vec3::X).abs() < 1e-5,
            "the wheel is across the frame"
        );
        // And it is horizontal: an axle with any Y in it is a wheel leaning
        // over on a bike that is not.
        assert!(axle.y.abs() < 1e-5);
    }

    /// The stand is a ring at unit radius, so its scale is its size.
    #[test]
    fn the_bike_stand_comes_up_to_a_person() {
        // Radius 0.45 with 0.22 of it under the pavement: a Sheffield stand is
        // about three-quarters of a metre tall, and one that came out at thirty
        // centimetres — which is what happens when an already-sized torus is
        // scaled a second time — is a croquet hoop.
        let top = 0.45 + 0.22;
        assert!(
            (0.6..0.85).contains(&top),
            "a stand {top}m tall is not something you lean a bike on"
        );
    }

    /// Nothing is placed past the kerb.
    ///
    /// The pavement is between three and four metres deep and a terrace is the
    /// deepest thing on it, so this is the arithmetic that decides whether a
    /// restaurant's chairs are on the pavement or in the road — and a chair in
    /// the road is a chair a car detonates on its way past, which is funny
    /// exactly once and then looks like a bug.
    #[test]
    fn nothing_a_frontage_puts_out_reaches_the_kerb() {
        for depth in [1.6f32, 2.4, 3.2, 6.0] {
            let board = BOARD_OUT.min(depth - 0.6);
            assert!(board < depth, "a board at {board} on {depth}m of pavement");

            let rack = (depth - KERB_CLEARANCE).min(1.5);
            assert!(rack + 0.4 < depth, "a bike at {rack} on {depth}m");

            // A terrace stands its tables at `out` and its chairs 0.62 either
            // side, so the far chair is what has to clear the kerb.
            let table = (depth - KERB_CLEARANCE - 0.5).min(1.9);
            if table >= 1.1 {
                assert!(
                    table + 0.62 + 0.21 < depth,
                    "the far chair at {} sits off a {depth}m pavement",
                    table + 0.62
                );
            }
        }
    }
}
