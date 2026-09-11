//! Public monuments.
//!
//! Every city keeps a few of its jokes in stone, and a park without a statue
//! is a lawn. There are three monuments on rotation and the bronze plaque is
//! the punchline of each: the unknown flummi (a granite ball — in this city
//! that *is* a figure at rest), the inventor of the bollard (a granite
//! bollard three times life size, honouring the one thing here that always
//! wins), and the monument to patience, which is an empty plinth that has
//! been awaiting its statue since 1874.
//!
//! Placement is a pure function of the world seed and the park's rectangle,
//! like everything the streaming respawns. The unknown flummi shears off his
//! plinth at speed — being launched is the most honest tribute this city
//! pays anybody — but the bollard memorial does not, because a breakable
//! monument to unbreakability would miss its own point.

use avian3d::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::citygen::Block;
use super::mayhem::Breakaway;
use super::texture::{encode, painted_rect, text_band};

/// How far a monument draws. Statues are landmarks; they read further than
/// street furniture but need not survive the horizon.
const RANGE: f32 = 420.0;

/// The plinth, in metres. One size for all three monuments: the city bought
/// them as a job lot.
const PLINTH: Vec3 = Vec3::new(1.5, 1.3, 1.5);

/// The bronze plaque on the plinth's face, in metres.
const PLAQUE: Vec2 = Vec2::new(1.14, 0.44);

/// Which monument stands in a given park, if any.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Monument {
    /// A granite sphere: the unknown flummi, at rest at last.
    Flummi,
    /// A granite bollard, three times life size.
    Bollard,
    /// Nothing. The statue has been in progress since 1874.
    Vacant,
    /// A pair of granite shoes and nothing above them. The city does not
    /// say what happened to the rest and the plaque is confident it is
    /// coming back, which in a town where everything bounces is not even
    /// unreasonable.
    Shoes,
    /// Four granite spheres in a row, not quite straight: the monument to
    /// the queue. The one piece of civic art in the city that its citizens
    /// re-enact daily outside every bakery in town — see `ai::queue`.
    Line,
}

impl Monument {
    const ALL: [Monument; 5] = [
        Monument::Flummi,
        Monument::Bollard,
        Monument::Vacant,
        Monument::Shoes,
        Monument::Line,
    ];

    /// The plaque's two lines. Deadpan municipal, like the signs.
    fn inscription(self) -> (&'static str, &'static str) {
        match self {
            Monument::Flummi => ("DER UNBEKANNTE FLUMMI", "ER PRALLTE FÜR UNS ALLE AB"),
            Monument::Bollard => ("DEM ERFINDER DES POLLERS", "ER GEWINNT. IMMER."),
            Monument::Vacant => ("DENKMAL DER GEDULD", "SEIT 1874 IN ARBEIT"),
            Monument::Shoes => ("DIE SCHUHE DES STIFTERS", "DER REST FOLGT"),
            Monument::Line => ("DENKMAL DER SCHLANGE", "SIE BEWEGT SICH NOCH"),
        }
    }
}

/// Paints one plaque: bronze field, raised bronze rim, the inscription in a
/// lighter alloy. Two bands, title over subline, the board painter's layout
/// at memorial proportions.
fn plaque_texture(monument: Monument) -> Image {
    let (title, subline) = monument.inscription();
    let title = encode(title);
    let subline = encode(subline);
    painted_rect(456, 176, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        if !(0.02..=0.98).contains(&u) || !(0.05..=0.95).contains(&v) {
            return [64, 46, 28, 255];
        }
        let lit =
            text_band(&title, u, (v - 0.14) / 0.42) || text_band(&subline, u, (v - 0.62) / 0.26);
        if lit {
            [226, 202, 152, 255]
        } else {
            [110, 82, 48, 255]
        }
    })
}

#[derive(Resource)]
pub struct StatueKit {
    plinth: (Handle<Mesh>, Handle<StandardMaterial>),
    sphere: (Handle<Mesh>, Handle<StandardMaterial>),
    bollard: (Handle<Mesh>, Handle<StandardMaterial>),
    /// One granite shoe, used twice, and one granite pebble, used four
    /// times. Both take the plinth's own weathered stone: a monument and
    /// its pedestal are quarried together and it shows.
    shoe: Handle<Mesh>,
    pebble: Handle<Mesh>,
    plaque: Handle<Mesh>,
    inscriptions: [(Monument, Handle<StandardMaterial>); Monument::ALL.len()],
}

/// The granite sphere's radius. Life size, as it happens.
const SPHERE_RADIUS: f32 = 0.85;
/// How finely the unknown flummi is carved.
///
/// An icosphere costs `20 * (subdivisions + 1)^2` triangles, so Bevy's default
/// of five is 720 for a 1.7 m ball that is only ever seen from across a park.
/// Three is 320 and its facets are about 11 cm across — a hand's breadth on a
/// granite sphere, which is what a chisel leaves anyway. Two (180) was tried
/// and rejected: at 17 cm the facets catch the sun individually and the
/// monument reads as a d20 rather than as a ball, which is precisely the
/// low-poly tell this pass exists to remove.
const SPHERE_SUBDIVISIONS: u32 = 3;
/// The memorial bollard: the street one is 0.11 by 0.95, this is three of it.
const BOLLARD_RADIUS: f32 = 0.33;
const BOLLARD_HEIGHT: f32 = 2.85;
/// The stifter's shoes, in metres. A figure's shoe is 0.245 long; these are
/// a civic pair, which is to say slightly larger than anybody's feet.
const SHOE: Vec3 = Vec3::new(0.16, 0.13, 0.38);
const SHOES_APART: f32 = 0.21;
/// The queue on its plinth: four of these, this far apart, each nudged a
/// little out of line. A queue carved dead straight is a colonnade.
const PEBBLE_RADIUS: f32 = 0.17;
const QUEUE_LENGTH: usize = 4;
const QUEUE_PITCH: f32 = 0.33;
const QUEUE_WANDER: f32 = 0.07;

// Both new monuments have to stand on the plinth the city bought as a job
// lot: a queue that runs off the granite is a queue standing in the grass,
// and two shoes closer together than one shoe is one shoe.
const _: () = assert!((QUEUE_LENGTH as f32 - 1.0) * QUEUE_PITCH + PEBBLE_RADIUS * 2.0 < PLINTH.x);
const _: () = assert!(QUEUE_WANDER * 2.0 + PEBBLE_RADIUS * 2.0 < PLINTH.z);
const _: () = assert!(SHOES_APART * 2.0 + SHOE.x < PLINTH.x);
const _: () = assert!(SHOES_APART > SHOE.x * 0.5);

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> StatueKit {
    let granite = materials.add(StandardMaterial {
        base_color: Color::srgb(0.52, 0.53, 0.55),
        perceptual_roughness: 0.92,
        ..default()
    });
    let weathered = materials.add(StandardMaterial {
        base_color: Color::srgb(0.44, 0.46, 0.49),
        perceptual_roughness: 0.96,
        ..default()
    });
    StatueKit {
        plinth: (
            meshes.add(Cuboid::new(PLINTH.x, PLINTH.y, PLINTH.z)),
            weathered,
        ),
        sphere: (
            meshes.add(
                Sphere::new(SPHERE_RADIUS)
                    .mesh()
                    .ico(SPHERE_SUBDIVISIONS)
                    .expect("an icosphere at three subdivisions"),
            ),
            granite.clone(),
        ),
        // A 33 cm column: `props::cylinder` gives it twelve sides rather than
        // the default thirty-two, which is 44 triangles instead of 124 for a
        // silhouette nobody can tell apart at the ten metres a park puts
        // between you and it.
        bollard: (
            meshes.add(super::props::cylinder(BOLLARD_RADIUS, BOLLARD_HEIGHT)),
            granite,
        ),
        shoe: meshes.add(Cuboid::new(SHOE.x, SHOE.y, SHOE.z)),
        pebble: meshes.add(
            Sphere::new(PEBBLE_RADIUS)
                .mesh()
                .ico(2)
                .expect("an icosphere at two subdivisions"),
        ),
        plaque: meshes.add(Rectangle::new(PLAQUE.x, PLAQUE.y)),
        inscriptions: Monument::ALL.map(|monument| {
            (
                monument,
                materials.add(StandardMaterial {
                    base_color_texture: Some(images.add(plaque_texture(monument))),
                    perceptual_roughness: 0.55,
                    metallic: 0.35,
                    ..default()
                }),
            )
        }),
    }
}

/// Which monument a park keeps, from the world seed and the park's own
/// rectangle — `None` for the parks that are just lawns. Pure, so the chunk
/// can respawn it forever.
fn monument_for(seed: u64, area: super::citygen::Rect) -> Option<(Monument, f32)> {
    let roll = super::rooftop::seed_for(seed, area);
    // A quarter of parks stay lawns, so a statue stays a find.
    if roll & 0b11 == 0 {
        return None;
    }
    let monument = Monument::ALL[((roll >> 2) % Monument::ALL.len() as u64) as usize];
    // Facing one of the four compass points, which is how municipal art is
    // actually installed: square to something, never to the sun.
    let yaw = std::f32::consts::FRAC_PI_2 * ((roll >> 4) & 0b11) as f32;
    Some((monument, yaw))
}

/// Raises the park's monument, if this park has one.
pub fn spawn(commands: &mut Commands, kit: &StatueKit, seed: u64, block: &Block, chunk: IVec2) {
    let Some((monument, yaw)) = monument_for(seed, block.area) else {
        return;
    };
    let centre = block.area.center();
    let facing = Quat::from_rotation_y(yaw);
    let range = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: (RANGE * 0.9)..RANGE,
        use_aabb: false,
    };

    // The plinth. Static forever: a monument you can nudge is street
    // furniture with pretensions.
    let (plinth_mesh, plinth_material) = &kit.plinth;
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(plinth_mesh.clone()),
        MeshMaterial3d(plinth_material.clone()),
        Transform::from_xyz(centre.x, SIDEWALK_HEIGHT + PLINTH.y * 0.5, centre.y)
            .with_rotation(facing),
        RigidBody::Static,
        Collider::cuboid(PLINTH.x, PLINTH.y, PLINTH.z),
        range.clone(),
    ));

    // The plaque, on the face the plinth was turned towards.
    let forward = facing * Vec3::Z;
    let plaque_at = Vec3::new(centre.x, SIDEWALK_HEIGHT + PLINTH.y * 0.55, centre.y)
        + forward * (PLINTH.z * 0.5 + 0.02);
    let (_, plaque_material) = kit
        .inscriptions
        .iter()
        .find(|(m, _)| *m == monument)
        .expect("every monument has its plaque");
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.plaque.clone()),
        MeshMaterial3d(plaque_material.clone()),
        Transform::from_translation(plaque_at).with_rotation(facing),
        range.clone(),
        NotShadowCaster,
    ));

    // The statue itself, for the two monuments that have got round to one.
    let top = SIDEWALK_HEIGHT + PLINTH.y;
    match monument {
        Monument::Flummi => {
            let (mesh, material) = &kit.sphere;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(centre.x, top + SPHERE_RADIUS, centre.y),
                RigidBody::Static,
                Collider::sphere(SPHERE_RADIUS),
                // Launchable, at real speed. A granite sphere bounding down
                // a park is the city's highest civic honour.
                Breakaway {
                    at: 8.5,
                    mass: 480.0,
                    geyser: false,
                },
                range,
            ));
        }
        Monument::Bollard => {
            let (mesh, material) = &kit.bollard;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(centre.x, top + BOLLARD_HEIGHT * 0.5, centre.y),
                RigidBody::Static,
                // No Breakaway, and none ever: see the module doc.
                Collider::cylinder(BOLLARD_RADIUS, BOLLARD_HEIGHT),
                range,
            ));
        }
        Monument::Shoes => {
            // Side by side, pointing the way the plinth faces, and nothing
            // above them. The city has not taken the shoes away because it
            // is expecting the rest of him back.
            let across = facing * Vec3::X;
            for side in [-1.0f32, 1.0] {
                let at = Vec3::new(centre.x, top + SHOE.y * 0.5, centre.y)
                    + across * (side * SHOES_APART);
                commands.spawn((
                    ChunkOf(chunk),
                    Mesh3d(kit.shoe.clone()),
                    MeshMaterial3d(kit.plinth.1.clone()),
                    Transform::from_translation(at).with_rotation(facing),
                    range.clone(),
                ));
            }
        }
        Monument::Line => {
            // Four of them, across the plaque face so the queue reads as a
            // queue from where anybody stands to read about it, each pushed
            // a little out of line — the wander is what stops it being a
            // colonnade, and it is the only honest thing about a queue.
            let across = facing * Vec3::X;
            let forward = facing * Vec3::Z;
            for index in 0..QUEUE_LENGTH {
                let along = (index as f32 - (QUEUE_LENGTH as f32 - 1.0) * 0.5) * QUEUE_PITCH;
                // Alternating rather than drawn: a monument is respawned
                // with its chunk and has to come back the same shape.
                let wander = if index % 2 == 0 {
                    QUEUE_WANDER
                } else {
                    -QUEUE_WANDER
                };
                let at = Vec3::new(centre.x, top + PEBBLE_RADIUS, centre.y)
                    + across * along
                    + forward * wander;
                commands.spawn((
                    ChunkOf(chunk),
                    Mesh3d(kit.pebble.clone()),
                    MeshMaterial3d(kit.plinth.1.clone()),
                    Transform::from_translation(at).with_rotation(facing),
                    range.clone(),
                ));
            }
        }
        Monument::Vacant => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::texture::glyph;

    #[test]
    fn every_inscription_fits_the_font() {
        for monument in Monument::ALL {
            let (title, subline) = monument.inscription();
            for text in [title, subline] {
                for code in encode(text) {
                    assert!(
                        code == b' ' || glyph(code) != [0; 7],
                        "{monument:?} says {text:?} and the font cannot draw {:?}",
                        code as char
                    );
                }
            }
        }
    }

    #[test]
    fn a_plaque_is_mostly_bronze_with_an_inscription_on_it() {
        for monument in Monument::ALL {
            let image = plaque_texture(monument);
            let data = image.data.as_ref().expect("the plaque was not painted");
            let ink = data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[..3] == [226, 202, 152])
                .count() as f32
                / (data.len() / 4) as f32;
            assert!(
                (0.02..0.40).contains(&ink),
                "{monument:?}: {ink:.3} of the plaque is inscription"
            );
        }
    }

    #[test]
    fn some_parks_are_lawns_and_the_rest_share_the_monuments_out() {
        // Sweep a grid of plausible park rectangles and check the rotation
        // actually rotates: all three monuments turn up, and so does the
        // occasional bare lawn. A modulus slip here shows every park the
        // same statue, which nobody notices until they visit two parks.
        let mut seen = std::collections::HashSet::new();
        let mut lawns = 0;
        for i in 0..64 {
            let at = Vec2::new(i as f32 * 97.0 - 800.0, i as f32 * 53.0 - 600.0);
            let area = crate::world::citygen::Rect::new(at, at + Vec2::new(60.0, 70.0));
            match monument_for(0xA17E_5EED, area) {
                Some((monument, yaw)) => {
                    seen.insert(monument);
                    assert!(
                        (0.0..std::f32::consts::TAU).contains(&yaw),
                        "a monument turned {yaw} radians"
                    );
                }
                None => lawns += 1,
            }
        }
        assert_eq!(
            seen.len(),
            Monument::ALL.len(),
            "only {seen:?} ever get built"
        );
        assert!(lawns > 0, "every single park has a statue");
        assert!(lawns < 40, "almost every park is a lawn");
    }
}
