//! Streetworks.
//!
//! A city that is never being dug up is a city nobody lives in. Roadworks are
//! the one piece of street furniture that says something is *happening* while
//! standing perfectly still: a barriered-off square of pavement with the slabs
//! lifted off it, a heap of spoil, three lengths of concrete pipe waiting to go
//! in, and a lamp on the corner blinking at nobody.
//!
//! They are also the best obstacle this game has. Everything on a site is
//! light, loose and stackable, the barriers are hoardings on two feet, and a
//! flummi arriving at speed puts the entire works across the street — which is
//! precisely the comedy the whole game is for.
//!
//! ## On the pavement, not in the road
//!
//! A site in the carriageway would be more dramatic and is the wrong call: the
//! traffic AI drives edge to edge and does not know about it, so within thirty
//! seconds a queue of cars has bulldozed the works into the next junction and
//! the street reads as broken rather than as busy. On the pavement it is the
//! player, the crowd and anybody who mounts the kerb who find it — which is the
//! right set of people, and the kerb-mounting is a decision rather than a
//! commute.
//!
//! Two cones go out into the gutter anyway, because that is what actually
//! happens and because the traffic knocking *those* about is funny.

use avian3d::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::roadgraph::RoadEdge;
use super::texture::{encode, painted_rect, text_band};
use crate::core::schedule::GameSet;

/// How far a site is drawn. Further than the small stuff: a red-and-white
/// hoarding is the most legible object on a street and it reads from a junction
/// away, which is the whole point of painting it red and white.
pub const RANGE: f32 = 120.0;

/// Chance any one street has works on it. Rare on purpose — a city being dug
/// up everywhere at once is not a busy city, it is a broken one.
const CHANCE: f32 = 0.045;

/// The dug patch, in metres.
const PIT: Vec2 = Vec2::new(2.6, 1.9);
/// How high the slabs stacked on edge round the dug patch stand. Low: they are
/// paving, not a fence.
const RIM: f32 = 0.10;

/// Barrier proportions.
const PANEL: Vec3 = Vec3::new(1.85, 0.44, 0.055);
/// How high the board's middle rides.
const PANEL_HEIGHT: f32 = 0.72;

/// Blinks per second on the corner lamp.
const BLINK: f32 = 0.9;

/// What the loose things on a site weigh, in kilograms, and what it takes to
/// shift the one thing that is not loose.
///
/// This ladder is the whole reason a worksite is worth building. A hoarding is
/// two trestles with a board on them and goes when a person walks into it; a
/// cone goes if you look at it; a concrete pipe needs a car at thirty
/// kilometres an hour, which is a decision rather than an accident.
const BARRIER_MASS: f32 = 11.0;
const CONE_MASS: f32 = 4.0;
const PIPE_MASS: f32 = 240.0;
/// Metres per second a car needs to move a pipe. Above the hydrant's, which is
/// the sturdiest thing on an ordinary street.
const PIPE_SHEARS_AT: f32 = 8.5;

// The rim is a lip and not a wall: high enough to throw a shadow across the
// bare earth, low enough to step over without noticing.
const _: () = assert!(RIM < SIDEWALK_HEIGHT);
const _: () = assert!(CONE_MASS < BARRIER_MASS);
const _: () = assert!(BARRIER_MASS * 10.0 < PIPE_MASS);
// And a pipe outlasts the sturdiest fixture on an ordinary street, read off
// `props` rather than typed in again — so that softening the street's furniture
// cannot silently leave this the wrong way round.
const _: () = assert!(PIPE_SHEARS_AT > super::props::HYDRANT_SHEARS_AT);

// --------------------------------------------------------------- notice ----

/// The board the city puts up to explain itself.
///
/// Every hole in every German pavement has one of these beside it, and it is
/// the single most reliably deadpan object in municipal life: a department
/// nobody can name, a completion date nobody believes, and a line at the
/// bottom disclaiming the whole thing. The site was already the best joke on
/// the street — a barriered square of nothing with a lamp blinking at it —
/// and it has been standing there for a year without a caption.
///
/// Written in the register of the institution telling it, which is the rule
/// every painted word in this town is held to: the sign is never in on the
/// joke, and that is the joke.
struct Notice {
    /// The authority line, small, in the blue band across the top.
    head: &'static str,
    /// What is happening here. Or what is not.
    title: &'static str,
    /// The small print, which is where the sign gives itself away.
    foot: &'static str,
}

const NOTICES: [Notice; 6] = [
    Notice {
        head: "TIEFBAUAMT",
        title: "BAUSTELLE",
        foot: "FERTIGSTELLUNG: 2031",
    },
    Notice {
        head: "STADT",
        title: "WIR BAUEN FÜR SIE",
        foot: "SIE WARTEN FÜR UNS",
    },
    Notice {
        head: "AMT FÜR ORDNUNG",
        title: "BETRETEN VERBOTEN",
        foot: "HÜPFEN ERST RECHT",
    },
    Notice {
        head: "TIEFBAUAMT",
        title: "ARBEITEN RUHEN",
        foot: "SEIT MÄRZ",
    },
    Notice {
        head: "STADT",
        title: "HIER ENTSTEHT ETWAS",
        foot: "DETAILS FOLGEN",
    },
    Notice {
        head: "UMLEITUNG",
        title: "DEM SCHILD FOLGEN",
        foot: "WELCHEM SCHILD?",
    },
];

/// The board, in metres, and how high its middle rides. Chest-to-eye height
/// on two legs, which is where a notice nobody reads is always put.
const NOTICE: Vec2 = Vec2::new(0.95, 0.68);
const NOTICE_HEIGHT: f32 = 1.24;
/// How far along the street from the middle of the site it stands, in the
/// site's own frame: past the end of the hoardings, clear of the trench,
/// and nowhere near the gutter the cones are in.
const NOTICE_ALONG: f32 = 2.95;
const NOTICE_ACROSS: f32 = 0.9;
/// How far apart the legs stand and how thick they are.
const NOTICE_LEGS: f32 = 0.34;
const NOTICE_LEG: f32 = 0.035;
/// Two faces, back to back: a site is walked past from both ends of a street
/// and a sign that is only there from one of them is half a sign.
const NOTICE_LEAF: f32 = 0.014;
/// How deep the blue authority band runs down the top of the sheet.
const NOTICE_BAND: f32 = 0.26;

// It is read from the pavement, so it must stand clear of the trench and
// outside the pen, and it must be a sign at eye level rather than a gantry.
const _: () = assert!(NOTICE_ALONG > PIT.x * 0.5 + 0.6);
const _: () = assert!(NOTICE_HEIGHT - NOTICE.y * 0.5 > 0.8);
const _: () = assert!(NOTICE_HEIGHT + NOTICE.y * 0.5 < 1.9);
const _: () = assert!(NOTICE_LEGS * 2.0 < NOTICE.x);

/// Width of one glyph cell relative to the height of its letters — the same
/// proportion as every other painted word in this city.
const CELL_ASPECT: f32 = 0.95;

/// One line of notice text: where its band sits, how tall its letters are
/// and how wide it comes out, clamped so the longest line stays on the sheet.
fn notice_line(text: &str, centre_v: f32, tallest_v: f32) -> (Vec3, Vec<u8>) {
    let cells = (text.chars().count() + 2) as f32;
    let letter_v = tallest_v.min(0.90 / (cells * CELL_ASPECT * (NOTICE.y / NOTICE.x)));
    let width_u = cells * letter_v * CELL_ASPECT * (NOTICE.y / NOTICE.x);
    (Vec3::new(centre_v, letter_v, width_u), encode(text))
}

/// Paints one board: white sheet, blue authority band, dark print.
fn notice_texture(notice: &Notice) -> Image {
    let head = notice_line(notice.head, NOTICE_BAND * 0.5, 0.11);
    let lines = [
        notice_line(notice.title, 0.52, 0.20),
        notice_line(notice.foot, 0.81, 0.11),
    ];
    const SHEET: [u8; 4] = [238, 238, 234, 255];
    const BAND: [u8; 4] = [26, 62, 124, 255];
    const PRINT: [u8; 4] = [30, 32, 36, 255];
    const PAPER: [u8; 4] = [206, 206, 200, 255];

    painted_rect(384, 274, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        let band = v < NOTICE_BAND;
        let field = if band { BAND } else { SHEET };
        // A pressed steel rim, which is what is left of this sign at the
        // distance where the print has stopped resolving.
        if !(0.015..=0.985).contains(&u) || !(0.02..=0.98).contains(&v) {
            return PAPER;
        }
        let paint = |(geometry, text): &(Vec3, Vec<u8>)| {
            let (centre_v, letter_v, width_u) = (geometry.x, geometry.y, geometry.z);
            let band_u = (u - (0.5 - width_u * 0.5)) / width_u;
            let band_v = (v - (centre_v - letter_v * 0.5)) / letter_v;
            (0.0..1.0).contains(&band_u) && text_band(text, band_u, band_v)
        };
        if band {
            return if paint(&head) { SHEET } else { BAND };
        }
        if lines.iter().any(paint) {
            return PRINT;
        }
        field
    })
}

#[derive(Resource)]
pub struct WorksiteKit {
    cube: Handle<Mesh>,
    cone: Handle<Mesh>,
    tube: Handle<Mesh>,
    heap: Handle<Mesh>,
    /// The hoarding, red and white on both faces — and the same board after
    /// somebody has been at it. A site hoarding is the single most reliably
    /// tagged surface in any city, and it is the one surface in this world
    /// that is a flat panel I own end to end: no window reveal, no shopfront
    /// glass, nothing to work around.
    hoarding: [Handle<StandardMaterial>; 2],
    steel: Handle<StandardMaterial>,
    /// Traffic cone orange, and the reflective band round it.
    plastic: Handle<StandardMaterial>,
    band: Handle<StandardMaterial>,
    concrete: Handle<StandardMaterial>,
    spoil: Handle<StandardMaterial>,
    /// The sheet the city explains itself on, and the run of things it has
    /// to say. One quad shared by every site in the town; only the paint
    /// differs, the same economy the shop signs run on.
    sheet: Handle<Mesh>,
    notices: Vec<Handle<StandardMaterial>>,
    /// The paving slabs, stacked on edge round the dug patch.
    slab: Handle<StandardMaterial>,
    /// The bare earth under the lifted slabs. Not black — a dark patch painted
    /// black is a sticker.
    dark: Handle<StandardMaterial>,
    lamp: Handle<StandardMaterial>,
}

/// The corner lamp, which is the only thing on a site that moves.
#[derive(Component)]
pub struct WarningLamp;

/// A tag across the middle of a board, as colour and coverage.
///
/// The board is five times wider than it is tall, so the tag's own square image
/// is stretched into the middle third of it — which is where somebody standing
/// on a pavement can actually reach.
fn scrawl(u: f32, v: f32) -> ([u8; 4], f32) {
    let across = (u - 0.10) / 0.80;
    let up = (v - 0.16) / 0.68;
    if !(0.0..1.0).contains(&across) || !(0.0..1.0).contains(&up) {
        return ([0; 4], 0.0);
    }
    let texel = super::texture::graffiti_at(across, up, 1);
    (texel, texel[3] as f32 / 255.0)
}

/// Red and white diagonals, which is the most legible pattern anybody has ever
/// painted on anything.
///
/// Diagonal rather than vertical: a vertical bar pattern on a board seen at a
/// glancing angle collapses into one colour, and the whole reason this thing is
/// striped is to be unmistakable from up the street.
fn hoarding_stripes(tagged: bool) -> Image {
    const SIZE: u32 = 256;
    super::texture::painted(SIZE, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        // The board is five times wider than it is tall, so `u` is squashed to
        // put the stripes at forty-five degrees on the finished panel rather
        // than at the texture's own aspect.
        // The tag, worked out first so the stripes underneath can be skipped
        // where it covers them. Painted rather than laid over as a second quad:
        // the board is already a texture and a decal on a movable dynamic body
        // would have to be parented and scaled with it for nothing.
        if tagged {
            let (ink, alpha) = scrawl(u, v);
            if alpha > 0.5 {
                return [ink[0], ink[1], ink[2], 255];
            }
        }
        let diagonal = (u * 5.0 + v).fract();
        let red = diagonal < 0.5;
        // Weathered: a hoarding that has stood in a street for a fortnight is
        // not the colour it left the depot.
        let grime = super::texture::fbm(u, v, 7, 3, 0x3f0b) * 0.18;
        if red {
            [
                super::texture::byte(0.66 - grime),
                super::texture::byte(0.10),
                super::texture::byte(0.09),
                255,
            ]
        } else {
            [
                super::texture::byte(0.86 - grime),
                super::texture::byte(0.85 - grime),
                super::texture::byte(0.82 - grime),
                255,
            ]
        }
    })
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> WorksiteKit {
    WorksiteKit {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        cone: meshes.add(Cone::new(1.0, 1.0).mesh().resolution(10).build()),
        tube: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(10).build()),
        heap: meshes.add(
            Sphere::new(1.0)
                .mesh()
                .ico(2)
                .expect("an icosphere at two subdivisions"),
        ),
        hoarding: [false, true].map(|tagged| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(images.add(hoarding_stripes(tagged))),
                perceptual_roughness: 0.85,
                ..default()
            })
        }),
        steel: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.56, 0.58),
            perceptual_roughness: 0.5,
            metallic: 0.7,
            ..default()
        }),
        plastic: materials.add(StandardMaterial {
            base_color: Color::srgb(0.88, 0.32, 0.06),
            perceptual_roughness: 0.68,
            ..default()
        }),
        band: materials.add(StandardMaterial {
            base_color: Color::srgb(0.90, 0.90, 0.88),
            perceptual_roughness: 0.35,
            ..default()
        }),
        concrete: materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.61, 0.58),
            perceptual_roughness: 0.95,
            ..default()
        }),
        spoil: materials.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.24, 0.17),
            perceptual_roughness: 1.0,
            ..default()
        }),
        sheet: meshes.add(Rectangle::new(NOTICE.x, NOTICE.y)),
        notices: NOTICES
            .iter()
            .map(|notice| {
                materials.add(StandardMaterial {
                    base_color_texture: Some(images.add(notice_texture(notice))),
                    perceptual_roughness: 0.72,
                    ..default()
                })
            })
            .collect(),
        slab: materials.add(StandardMaterial {
            base_color: Color::srgb(0.52, 0.51, 0.49),
            perceptual_roughness: 0.97,
            ..default()
        }),
        // Very dark brown rather than black: a hole is earth in shadow, and a
        // black one reads as a decal somebody stuck on the pavement.
        dark: materials.add(StandardMaterial {
            base_color: Color::srgb(0.09, 0.075, 0.06),
            perceptual_roughness: 1.0,
            ..default()
        }),
        lamp: materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.55, 0.10),
            emissive: LinearRgba::rgb(3.0, 1.3, 0.1),
            perceptual_roughness: 0.35,
            ..default()
        }),
    }
}

/// Digs up one street, if this street is being dug up.
#[allow(clippy::too_many_arguments)]
pub fn spawn_edge(
    commands: &mut Commands,
    kit: &WorksiteKit,
    rng: &mut ChaCha8Rng,
    edge: &RoadEdge,
    from: Vec2,
    to: Vec2,
    chunk: IVec2,
    range: f32,
) {
    if rng.random_range(0.0..1.0) > CHANCE {
        return;
    }
    let Ok(direction) = Dir2::new(to - from) else {
        return;
    };
    // A site needs a run of pavement to stand on, and a short back street does
    // not have one.
    if edge.length < 26.0 {
        return;
    }
    let normal = Vec2::new(-direction.y, direction.x);
    let side = if rng.random_range(0.0..1.0) < 0.5 {
        1.0
    } else {
        -1.0
    };
    // Out on the pavement, clear of the kerb — see the module note on why this
    // is not in the road.
    let out = edge.width * 0.5 + 1.7;
    let along = rng.random_range(0.25..0.75) * edge.length;
    let middle = from + *direction * along + normal * (out * side);

    // The site's own frame: `across` runs along the street, and `depth` runs
    // across the pavement *towards* the kerb — so a positive depth is towards
    // the road and a negative one is towards the wall. Worth stating, because
    // the first arrangement had it the other way round and the whole site came
    // out inside out: the spoil heaped in the gutter and the hoardings lined up
    // against the shopfront, protecting the wall from the hole.
    let across = *direction;
    let depth = -normal * side;
    let at = |a: f32, d: f32| middle + across * a + depth * d;
    // The yaw that turns a mesh's +Z to face the road, which is what a hoarding
    // does — so a barrier with no extra turn stands *along* the street and one
    // turned a quarter closes an end. Derived from `depth` and not from
    // `across`: taking it off the street's own direction stands every board in
    // the city broadside to the pavement, which is a fence across the footway
    // rather than a pen around a hole.
    let facing = depth.x.atan2(depth.y);

    let visibility = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: range..(range * 1.1),
        use_aabb: false,
    };
    let ground = SIDEWALK_HEIGHT;

    // The trench.
    //
    // Not a hole. The pavement is one solid slab per block and nothing can cut
    // a shaft into it, so a box sunk below the paving is a box nobody can see —
    // which is exactly what the first attempt was. What a dug-up footway
    // actually looks like from three metres is the paving *lifted*: bare earth
    // where the slabs were, with the slabs themselves stacked on edge round the
    // rim holding it. That reads as an excavation and needs no hole at all.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.cube.clone()),
        MeshMaterial3d(kit.dark.clone()),
        Transform::from_xyz(middle.x, ground + 0.008, middle.y)
            .with_rotation(Quat::from_rotation_y(facing))
            .with_scale(Vec3::new(PIT.x, 0.016, PIT.y)),
        visibility.clone(),
    ));
    for (a, d, turn) in [
        (0.0f32, PIT.y * 0.5, 0.0f32),
        (0.0, -PIT.y * 0.5, 0.0),
        (PIT.x * 0.5, 0.0, std::f32::consts::FRAC_PI_2),
        (-PIT.x * 0.5, 0.0, std::f32::consts::FRAC_PI_2),
    ] {
        let spot = at(a, d);
        let run = if turn == 0.0 { PIT.x } else { PIT.y };
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.slab.clone()),
            Transform::from_xyz(spot.x, ground + RIM * 0.5, spot.y)
                .with_rotation(Quat::from_rotation_y(facing + turn))
                .with_scale(Vec3::new(run + 0.09, RIM, 0.045)),
            visibility.clone(),
        ));
    }

    // The spoil, heaped behind the dug patch — between it and the wall, which
    // is the side nobody walks down.
    for i in 0..3 {
        let a = (i as f32 - 1.0) * 0.62;
        let spot = at(a, -1.35);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.heap.clone()),
            MeshMaterial3d(kit.spoil.clone()),
            Transform::from_xyz(spot.x, ground + 0.10, spot.y)
                .with_scale(Vec3::new(0.62, 0.30, 0.55)),
            visibility.clone(),
        ));
    }

    // Three lengths of pipe, stacked two and one, waiting to go in the hole.
    for (a, d, lift) in [
        (-0.42f32, -1.1f32, 0.0f32),
        (0.42, -1.1, 0.0),
        (0.0, -1.1, 0.62),
    ] {
        let spot = at(a - 2.9, d);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.tube.clone()),
            MeshMaterial3d(kit.concrete.clone()),
            // Cylinders stand up; a pipe on a pallet lies along the street.
            Transform::from_xyz(spot.x, ground + 0.31 + lift, spot.y)
                .with_rotation(
                    Quat::from_rotation_y(facing)
                        * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                )
                .with_scale(Vec3::new(0.31, 1.5, 0.31)),
            visibility.clone(),
            // Heavy, and the one thing on the site that stays put. A concrete
            // pipe a car can scatter is a concrete pipe made of polystyrene.
            RigidBody::Static,
            Collider::cylinder(0.31, 1.5),
            // Except at real speed, when it goes like everything else.
            super::mayhem::Breakaway {
                at: PIPE_SHEARS_AT,
                mass: PIPE_MASS,
                geyser: false,
            },
        ));
    }

    // The hoardings: a three-sided pen round the hole, closed towards the
    // street and open towards the wall, so somebody could plausibly be working
    // in it and anybody coming down the pavement meets the board rather than
    // the trench.
    let pen = [
        (0.0f32, 1.5f32, 0.0f32),
        (-1.7, 0.35, std::f32::consts::FRAC_PI_2),
        (1.7, 0.35, std::f32::consts::FRAC_PI_2),
    ];
    for (i, (a, d, turn)) in pen.into_iter().enumerate() {
        let spot = at(a, d);
        let yaw = facing + turn;
        // The middle board carries the lamp, and every other board in the city
        // has been tagged.
        barrier(
            commands,
            kit,
            spot,
            ground,
            yaw,
            chunk,
            &visibility,
            i == 0,
            rng.random_range(0.0..1.0) < 0.45,
        );
    }

    // The notice, standing past the end of the pen and turned across the
    // pavement, so somebody walking down the footway meets it face on rather
    // than edge on. The one thing on a site that is meant to be read, and
    // therefore the only thing not painted red and white.
    //
    // Static and unbreakable: a sign a flummi can put across the street is a
    // sign nobody ever finishes reading, and this one has a punchline.
    let notice = &kit.notices[rng.random_range(0..kit.notices.len())];
    // Turned a quarter from the hoardings, so its width runs across the
    // pavement — which is to say along `depth`, which is why both legs are
    // written in the site's own frame rather than back out of the yaw.
    let notice_yaw = facing + std::f32::consts::FRAC_PI_2;
    let post = at(NOTICE_ALONG, NOTICE_ACROSS);
    for side in [-1.0f32, 1.0] {
        let leg = at(NOTICE_ALONG, NOTICE_ACROSS + side * NOTICE_LEGS);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.steel.clone()),
            Transform::from_xyz(leg.x, ground + NOTICE_HEIGHT * 0.5, leg.y)
                .with_rotation(Quat::from_rotation_y(notice_yaw))
                .with_scale(Vec3::new(NOTICE_LEG, NOTICE_HEIGHT, NOTICE_LEG)),
            visibility.clone(),
        ));
    }
    // Both faces carry the same print: a `Rectangle` looks down its own +Z,
    // so the second leaf is the first turned half a turn and nudged back
    // along the sheet's own normal.
    for turn in [0.0f32, std::f32::consts::PI] {
        let spin = Quat::from_rotation_y(notice_yaw + turn);
        let out = (spin * Vec3::Z * NOTICE_LEAF).xz();
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.sheet.clone()),
            MeshMaterial3d(notice.clone()),
            Transform::from_xyz(post.x + out.x, ground + NOTICE_HEIGHT, post.y + out.y)
                .with_rotation(spin),
            visibility.clone(),
        ));
    }

    // And two cones out in the gutter, which is both what actually happens and
    // the half of the site the traffic gets to knock about.
    for i in 0..2 {
        let spot = at((i as f32 - 0.5) * 3.4, 2.9);
        commands
            .spawn((
                ChunkOf(chunk),
                Transform::from_xyz(spot.x, 0.36, spot.y),
                Visibility::default(),
                RigidBody::Dynamic,
                Collider::cone(0.24, 0.72),
                Mass(CONE_MASS),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Mesh3d(kit.cone.clone()),
                    MeshMaterial3d(kit.plastic.clone()),
                    Transform::from_scale(Vec3::new(0.24, 0.72, 0.24)),
                    visibility.clone(),
                ));
                parent.spawn((
                    Mesh3d(kit.tube.clone()),
                    MeshMaterial3d(kit.band.clone()),
                    Transform::from_xyz(0.0, 0.04, 0.0).with_scale(Vec3::new(0.155, 0.10, 0.155)),
                    visibility.clone(),
                ));
            });
    }
}

/// One hoarding on its two feet, and the lamp if this is the one that carries
/// it.
#[allow(clippy::too_many_arguments)]
fn barrier(
    commands: &mut Commands,
    kit: &WorksiteKit,
    at: Vec2,
    ground: f32,
    yaw: f32,
    chunk: IVec2,
    range: &VisibilityRange,
    lamped: bool,
    tagged: bool,
) {
    commands
        .spawn((
            ChunkOf(chunk),
            Transform::from_xyz(at.x, ground + PANEL_HEIGHT * 0.5, at.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            // Dynamic and light. A hoarding is two trestles with a board on
            // them; anybody who has ever cycled into one knows exactly how
            // little it weighs.
            RigidBody::Dynamic,
            Collider::cuboid(PANEL.x, PANEL_HEIGHT, 0.30),
            Mass(BARRIER_MASS),
        ))
        .with_children(|parent| {
            // The board, near the top of the frame.
            parent.spawn((
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(kit.hoarding[usize::from(tagged)].clone()),
                Transform::from_xyz(0.0, PANEL_HEIGHT * 0.5 - PANEL.y * 0.5, 0.0).with_scale(PANEL),
                range.clone(),
            ));
            // Two legs, splayed the way a trestle is.
            for side in [-1.0f32, 1.0] {
                parent.spawn((
                    Mesh3d(kit.cube.clone()),
                    MeshMaterial3d(kit.steel.clone()),
                    Transform::from_xyz(side * PANEL.x * 0.42, -PANEL_HEIGHT * 0.16, 0.0)
                        .with_rotation(Quat::from_rotation_z(side * 0.14))
                        .with_scale(Vec3::new(0.045, PANEL_HEIGHT * 0.68, 0.28)),
                    range.clone(),
                ));
            }
            if lamped {
                parent.spawn((
                    WarningLamp,
                    Mesh3d(kit.cube.clone()),
                    MeshMaterial3d(kit.lamp.clone()),
                    Transform::from_xyz(PANEL.x * 0.44, PANEL_HEIGHT * 0.5 + 0.07, 0.0)
                        .with_scale(Vec3::new(0.13, 0.14, 0.11)),
                    range.clone(),
                ));
            }
        });
}

pub struct WorksitePlugin;

impl Plugin for WorksitePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, blink.in_set(GameSet::Simulation));
    }
}

/// The corner lamp.
///
/// One material for every lamp in the city, so this is one write a frame and
/// every site in sight blinks together. That is not a shortcut being excused:
/// real ones are all running off the same kind of timer and they do drift into
/// step, and the alternative — a material per lamp — is the parked-car mistake
/// again in miniature.
fn blink(
    time: Res<Time>,
    kit: Option<Res<WorksiteKit>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(kit) = kit else { return };
    // Square-ish rather than a sine: a warning lamp is on or it is off, and a
    // gentle throb reads as a pilot light.
    let on = (time.elapsed_secs() * BLINK * std::f32::consts::TAU).sin() > -0.25;
    if let Some(mut material) = materials.get_mut(&kit.lamp) {
        let wanted = if on {
            LinearRgba::rgb(3.0, 1.3, 0.1)
        } else {
            LinearRgba::rgb(0.10, 0.045, 0.005)
        };
        if material.emissive != wanted {
            material.emissive = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The site fits on the pavement it is standing on.
    #[test]
    fn every_notice_fits_the_font_and_the_sheet() {
        use crate::world::texture::glyph;
        for notice in &NOTICES {
            for (band, text) in [
                (0.11f32, notice.head),
                (0.20, notice.title),
                (0.11, notice.foot),
            ] {
                for code in encode(text) {
                    assert!(
                        code == b' ' || glyph(code) != [0; 7],
                        "a site notice says {text:?} and the font cannot draw {:?}",
                        code as char
                    );
                }
                let (geometry, _) = notice_line(text, 0.5, band);
                assert!(
                    geometry.z <= 0.901,
                    "{text:?} runs {:.2} of the sheet wide",
                    geometry.z
                );
                assert!(geometry.y > 0.02, "{text:?} is printed too small to read");
            }
        }
    }

    #[test]
    fn a_notice_is_a_sheet_with_print_on_it() {
        // The same band-arithmetic slip the plaques and the posters can
        // hide: a sign painted all print or no print still stands beside
        // the hole, it just stops saying anything.
        for notice in &NOTICES {
            let image = notice_texture(notice);
            let data = image.data.as_ref().expect("the notice was not painted");
            let dark = data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] < 80 && pixel[1] < 90)
                .count() as f32
                / (data.len() / 4) as f32;
            assert!(
                (0.03..0.55).contains(&dark),
                "{:?}: {dark:.3} of the sheet is ink and band",
                notice.title
            );
        }
    }

    #[test]
    fn a_worksite_stays_on_its_own_pavement() {
        // The pavement is `citygen::SIDEWALK_WIDTH` deep, the site is centred
        // 1.7m out from the kerb, and the furthest thing from that centre is
        // the spoil heap at 1.35m back. Between them they must not reach past
        // the building line — a heap of earth inside a shop is not a joke, it
        // is a hole in the world.
        let pavement = super::super::citygen::SIDEWALK_WIDTH;
        let centre_from_kerb = 1.7;
        let spoil_from_centre = 1.35 + 0.30;
        assert!(
            centre_from_kerb + spoil_from_centre <= pavement + 0.4,
            "the spoil heap lands {:.2}m from the kerb on a {pavement}m pavement",
            centre_from_kerb + spoil_from_centre
        );
        // And the cones, which go the other way, land in the road rather than
        // on the kerb line.
        assert!(2.9 - centre_from_kerb > 1.0, "the cones are on the kerb");
    }

    /// The city is being dug up in a few places, not everywhere.
    #[test]
    fn the_city_is_not_one_continuous_building_site() {
        // Roughly one street in twenty-two. Over the whole road graph that is
        // a few dozen sites in a two-kilometre city, which is a place with
        // work going on; at one in five it is a place that has been evacuated.
        assert!((0.02..0.08).contains(&CHANCE));
    }

    #[test]
    fn the_lamp_is_on_about_half_the_time() {
        // Sampled over several cycles: a warning lamp with a duty cycle near
        // nought is broken and one near one is a light.
        let lit = (0..600)
            .filter(|step| {
                let t = *step as f32 / 100.0;
                (t * BLINK * std::f32::consts::TAU).sin() > -0.25
            })
            .count() as f32
            / 600.0;
        assert!(
            (0.4..0.75).contains(&lit),
            "the lamp is lit {lit:.2} of the time"
        );
    }
}
