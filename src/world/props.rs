//! Street furniture.
//!
//! Bins, bollards, hydrants, post boxes, signs. None of it is interactive and
//! none of it is in the way — it exists because a street without any of it does
//! not read as a street. A kerb with nothing standing on it for sixty metres is
//! the single clearest tell that a city was generated rather than built.
//!
//! Everything shares one mesh per kind and one material per kind, so the whole
//! city's furniture is a handful of draw calls however much of it is up. Props
//! are placed from the chunk's own RNG stream, so a chunk regenerates
//! identically however many times the player walks in and out of it — the same
//! rule the buildings follow.
//!
//! None of it used to have a collider: walking through a bin was a smaller lie
//! than several hundred more bodies for the solver to consider. That trade is
//! off now, because in a city made of rubber the furniture is what you bounce
//! off. It is split in two by whether the real thing is bolted down — a bollard
//! is a post set in concrete and a bin is a bin — so a car mounting the kerb
//! scatters the bins and stops dead at the bollard, which is the right joke
//! both times. Since the world-damage milestone the bolted half is graded
//! rather than absolute: everything bolted except the bollard shears off its
//! footing above some speed (`world::mayhem`), and the bollard alone is
//! genuinely forever.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::mayhem::Breakaway;
use super::roadgraph::RoadEdge;

/// What a car has to be doing, in metres per second, to take a hydrant off its
/// footing.
///
/// Public because it is the benchmark: the hydrant is the sturdiest fixture on
/// an ordinary street, so anything elsewhere in the world that claims to be
/// harder to shift than street furniture is measured against this rather than
/// against a number somebody typed twice.
pub const HYDRANT_SHEARS_AT: f32 = 5.5;

/// How many sides a drawn cylinder gets, from the radius it is drawn at.
///
/// Everything round in the street kit used to take Bevy's default resolution of
/// 32, which is 124 triangles whatever the thing is — a cylinder costs
/// `4 * sides - 4` here, two per side for the wall and a fan of `sides - 2` per
/// cap. On the 45 mm sign post that bought a silhouette whose facets are 9 mm
/// wide, which is a fraction of a pixel from anywhere anyone stands, while the
/// building behind it was a 12-triangle box with knife-sharp corners. The
/// budget was being spent on the one object in frame that could not possibly
/// show it.
///
/// What the eye reads is facet *width*, not side count, so the count follows
/// the radius. The flats that come out run from 4.5 cm on a sign post to
/// 18 cm on a bin, and 47 cm on the one object big enough to reach the top
/// band:
///
/// * under 8 cm — the 45 mm sign post, the 55 mm lamp arm, the 75 mm lamp
///   column and signal mast, the 60 mm rooftop aerial: **6 sides**, 20 tris.
/// * up to 20 cm — the 110 mm bollard, the 160 mm hydrant, the 85 mm signal
///   lens: **10 sides**, 36 tris.
/// * up to 60 cm — the 260 mm bin, the 330 mm memorial bollard, the rooftop
///   vent stacks: **12 sides**, 44 tris.
/// * larger — rooftop water tanks, anything a person could stand inside:
///   **16 sides**, 60 tris.
///
/// Sixteen is a deliberate ceiling rather than the end of the ramp. The only
/// things up there are water tanks, which live behind a parapet and stop being
/// drawn at 420 m (`rooftop::CLUTTER_RANGE`), so their 47 cm flats are read
/// from at least twenty metres away and through a silhouette that is mostly
/// parapet. Twenty-four sides would halve that and cost 92 triangles apiece on
/// the one piece of clutter nobody stands next to.
///
/// Rejected: scaling continuously with the radius. It reads no better, and it
/// makes the mesh count unbounded in a module whose whole bargain is one mesh
/// per kind shared by the entire city.
///
/// Lives here rather than in `world::mesh` because street furniture is where
/// the problem was measured; `statues`, `streetlights` and `rooftop` call in.
pub fn cylinder_sides(radius: f32) -> u32 {
    if radius < 0.08 {
        6
    } else if radius <= 0.20 {
        10
    } else if radius <= 0.60 {
        12
    } else {
        16
    }
}

/// A cylinder at a resolution its radius can actually show.
///
/// The one place `Cylinder::mesh()` is allowed to be called in the street kit,
/// so that no future addition can quietly take the default 32 again.
pub fn cylinder(radius: f32, height: f32) -> Mesh {
    Cylinder::new(radius, height)
        .mesh()
        .resolution(cylinder_sides(radius))
        .build()
}

/// Metres between chances to place something on a kerb.
const SPACING: f32 = 14.0;
/// How many of those chances actually produce a prop.
const DENSITY: f32 = 0.42;
/// How far inside the kerb line furniture stands, in metres.
const SET_BACK: f32 = 0.75;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Prop {
    Bin,
    Bollard,
    Hydrant,
    PostBox,
    Sign,
    /// A parking meter. Small, and there are more of them on a real street
    /// than of anything else here.
    Meter,
    Bench,
    /// A newspaper vending box, in a colour nobody chose for it.
    NewsBox,
    /// A planter. The one piece of furniture that is here to look like it is
    /// not furniture.
    Planter,
    /// A phone box, kept for exactly the reason real ones are: nobody has got
    /// round to taking it away.
    PhoneBox,
}

impl Prop {
    /// Weighted the way a street is: bollards, meters and bins everywhere, a
    /// hydrant here and there, a phone box rarely.
    const TABLE: [(Prop, u32); 10] = [
        (Prop::Bollard, 9),
        (Prop::Meter, 7),
        (Prop::Bin, 6),
        (Prop::Sign, 5),
        (Prop::Hydrant, 3),
        (Prop::Bench, 3),
        (Prop::NewsBox, 2),
        (Prop::Planter, 2),
        (Prop::PostBox, 1),
        (Prop::PhoneBox, 1),
    ];

    /// Whether a piece of street furniture is bolted down, and if not, what it
    /// weighs.
    ///
    /// The split is the one a street actually makes. A bollard is a post set in
    /// concrete and a bin is a bin; in a city where everything else bounces,
    /// which of the two a car sends cartwheeling down the pavement is the whole
    /// joke, and it has to be the right one or the street stops making sense.
    ///
    /// Bolted is no longer forever, though: everything bolted except the
    /// bollard carries a [`Breakaway`], so a car arriving fast enough shears
    /// it off its footing and it flies after all — see `world::mayhem`. The
    /// bollard alone has no shear speed, because its entire comedic function
    /// is being the one thing on the street that always wins.
    fn footing(self) -> Footing {
        match self {
            Prop::Bollard => Footing::Bolted(None),
            Prop::Sign => Footing::Bolted(Some(Breakaway {
                at: 3.5,
                mass: 26.0,
                geyser: false,
            })),
            Prop::Meter => Footing::Bolted(Some(Breakaway {
                at: 3.5,
                mass: 30.0,
                geyser: false,
            })),
            // The hydrant is the whole reason shearing exists.
            Prop::Hydrant => Footing::Bolted(Some(Breakaway {
                at: HYDRANT_SHEARS_AT,
                mass: 55.0,
                geyser: true,
            })),
            Prop::PhoneBox => Footing::Bolted(Some(Breakaway {
                at: 7.5,
                mass: 320.0,
                geyser: false,
            })),
            // Heavy enough that a person bouncing off one loses the argument,
            // light enough that a car does not.
            Prop::PostBox => Footing::Loose(120.0),
            Prop::Planter => Footing::Loose(140.0),
            Prop::Bench => Footing::Loose(85.0),
            Prop::NewsBox => Footing::Loose(38.0),
            Prop::Bin => Footing::Loose(14.0),
        }
    }

    /// The collider for one kind, in the same dimensions its mesh is built at.
    ///
    /// Written out a second time rather than derived from the mesh, because a
    /// convex hull of a cylinder is a worse cylinder and Avian has the real
    /// thing. The duplication is held honest by a test that walks every kind
    /// and checks the collider against the standing height the meshes are
    /// placed by.
    fn collider(self) -> Collider {
        match self {
            Prop::Bin => Collider::cylinder(0.26, 0.95),
            Prop::Bollard => Collider::cylinder(0.11, 0.95),
            Prop::Hydrant => Collider::cylinder(0.16, 0.72),
            Prop::PostBox => Collider::cuboid(0.52, 1.25, 0.46),
            Prop::Sign => Collider::cylinder(0.045, 2.35),
            Prop::Meter => Collider::cuboid(0.14, 1.22, 0.12),
            Prop::Bench => Collider::cuboid(1.75, 0.46, 0.55),
            Prop::NewsBox => Collider::cuboid(0.44, 1.08, 0.40),
            Prop::Planter => Collider::cuboid(0.86, 0.58, 0.86),
            Prop::PhoneBox => Collider::cuboid(0.94, 2.42, 0.94),
        }
    }

    fn pick(rng: &mut ChaCha8Rng) -> Prop {
        let total: u32 = Self::TABLE.iter().map(|(_, weight)| weight).sum();
        let mut ticket = rng.random_range(0..total);
        for (prop, weight) in Self::TABLE {
            if ticket < weight {
                return prop;
            }
            ticket -= weight;
        }
        Prop::Bollard
    }
}

/// How firmly a prop is attached to the pavement.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Footing {
    /// Set in concrete. Things bounce off it; it does not bounce off them —
    /// unless it carries a [`Breakaway`], in which case a fast enough car
    /// takes it off its footing after all.
    Bolted(Option<Breakaway>),
    /// Free to be sent down the street, at this mass in kilograms.
    Loose(f32),
}

#[derive(Resource)]
pub struct PropAssets {
    bin: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    bollard: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    hydrant: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    post_box: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    sign_post: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    sign_plate: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    meter: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    bench: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    news_box: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    planter: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    /// What grows in the planter. Without it the trough was a grey block at
    /// the kerb — the one prop a player circled on a screenshot and asked what
    /// it was, which is a fair question about a concrete cube with nothing in
    /// it.
    planter_top: (Handle<Mesh>, Handle<StandardMaterial>),
    phone_box: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    /// The signal head, its pole, and the lens plate that faces the traffic.
    signal_post: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    signal_head: (Handle<Mesh>, Handle<StandardMaterial>, f32),
    signal_lens: [(Handle<Mesh>, Handle<StandardMaterial>); 3],
}

impl PropAssets {
    /// The mesh, material and standing height for one kind.
    fn parts(&self, prop: Prop) -> &(Handle<Mesh>, Handle<StandardMaterial>, f32) {
        match prop {
            Prop::Bin => &self.bin,
            Prop::Bollard => &self.bollard,
            Prop::Hydrant => &self.hydrant,
            Prop::PostBox => &self.post_box,
            Prop::Sign => &self.sign_post,
            Prop::Meter => &self.meter,
            Prop::Bench => &self.bench,
            Prop::NewsBox => &self.news_box,
            Prop::Planter => &self.planter,
            Prop::PhoneBox => &self.phone_box,
        }
    }
}

fn painted_metal(color: Color, roughness: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        metallic: 0.55,
        ..default()
    }
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> PropAssets {
    PropAssets {
        bin: (
            meshes.add(cylinder(0.26, 0.95)),
            materials.add(painted_metal(Color::srgb(0.17, 0.20, 0.18), 0.72)),
            0.95,
        ),
        bollard: (
            meshes.add(cylinder(0.11, 0.95)),
            materials.add(painted_metal(Color::srgb(0.12, 0.13, 0.15), 0.55)),
            0.95,
        ),
        hydrant: (
            meshes.add(cylinder(0.16, 0.72)),
            materials.add(painted_metal(Color::srgb(0.60, 0.10, 0.09), 0.62)),
            0.72,
        ),
        post_box: (
            meshes.add(Cuboid::new(0.52, 1.25, 0.46)),
            materials.add(painted_metal(Color::srgb(0.42, 0.10, 0.11), 0.50)),
            1.25,
        ),
        sign_post: (
            meshes.add(cylinder(0.045, 2.35)),
            materials.add(painted_metal(Color::srgb(0.55, 0.56, 0.58), 0.45)),
            2.35,
        ),
        sign_plate: (
            meshes.add(Cuboid::new(0.46, 0.46, 0.03)),
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.80, 0.81, 0.83),
                perceptual_roughness: 0.34,
                metallic: 0.15,
                ..default()
            }),
            0.46,
        ),
        meter: (
            meshes.add(Cuboid::new(0.14, 1.22, 0.12)),
            materials.add(painted_metal(Color::srgb(0.24, 0.26, 0.28), 0.58)),
            1.22,
        ),
        bench: (
            meshes.add(Cuboid::new(1.75, 0.46, 0.55)),
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.36, 0.26, 0.17),
                perceptual_roughness: 0.88,
                ..default()
            }),
            0.46,
        ),
        news_box: (
            meshes.add(Cuboid::new(0.44, 1.08, 0.40)),
            materials.add(painted_metal(Color::srgb(0.16, 0.34, 0.46), 0.52)),
            1.08,
        ),
        planter: (
            meshes.add(Cuboid::new(0.86, 0.58, 0.86)),
            // Pale cast concrete, weathered. It was a darker grey-brown, and a
            // dark cube at a kerb reads as a bin or a junction box; a trough is
            // pale because it is made of the same mix as a kerbstone.
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.58, 0.56, 0.52),
                perceptual_roughness: 0.97,
                ..default()
            }),
            0.58,
        ),
        planter_top: (
            // A clipped box shrub, a little wider than tall and lower than a
            // head: what a council actually puts in a trough.
            meshes.add(Sphere::new(0.46).mesh().uv(16, 10)),
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.22, 0.33, 0.16),
                perceptual_roughness: 0.96,
                ..default()
            }),
        ),
        phone_box: (
            meshes.add(Cuboid::new(0.94, 2.42, 0.94)),
            materials.add(painted_metal(Color::srgb(0.34, 0.12, 0.12), 0.42)),
            2.42,
        ),
        signal_post: (
            meshes.add(cylinder(0.075, SIGNAL_HEIGHT)),
            materials.add(painted_metal(Color::srgb(0.17, 0.18, 0.19), 0.50)),
            SIGNAL_HEIGHT,
        ),
        signal_head: (
            meshes.add(Cuboid::new(0.32, 0.86, 0.26)),
            materials.add(painted_metal(Color::srgb(0.13, 0.14, 0.15), 0.55)),
            0.86,
        ),
        // Unlit. Nothing in this game drives the signals yet, and a signal
        // showing green down every approach of a crossroads at once would be a
        // clearer lie than one showing nothing.
        signal_lens: [
            Color::srgb(0.34, 0.06, 0.05),
            Color::srgb(0.36, 0.26, 0.05),
            Color::srgb(0.06, 0.30, 0.12),
        ]
        .map(|color| {
            (
                meshes.add(cylinder(0.085, 0.045)),
                materials.add(StandardMaterial {
                    base_color: color,
                    perceptual_roughness: 0.28,
                    ..default()
                }),
            )
        }),
    }
}

/// How high a signal head hangs above the pavement.
const SIGNAL_HEIGHT: f32 = 3.1;
/// Where the three lenses sit on the head, measured from its middle.
const SIGNAL_LENSES: [f32; 3] = [0.26, 0.0, -0.26];
/// How far back from the junction's centre a signal stands, as a multiple of
/// the widest road meeting there.
const SIGNAL_SET_BACK: f32 = 0.62;

/// Where a signal for one approach stands, and which way it looks.
///
/// It stands on the near kerb, on the right of a driver coming down this arm,
/// and it looks back up the arm at them. Both halves are easy to get a quarter
/// turn or a whole side out, and neither shows in a still — which is why they
/// are a function with a test rather than four lines inside a spawn.
fn signal_pose(at: Vec2, towards: Vec2, widest: f32) -> Option<(Vec2, f32)> {
    let direction = Dir2::new(towards - at).ok()?;
    // The driver is coming *down* the arm, so their heading is the other way
    // and their right hand is the other way with it.
    let heading = -*direction;
    let right = Vec2::new(-heading.y, heading.x);
    let kerb = if crate::ai::steering::RIGHT_HAND_TRAFFIC {
        right
    } else {
        -right
    };

    // Both terms off the widest arm. The set-back already was; the lateral
    // offset was off this arm's own half-width, and at a junction where the
    // arms leave at anything but a right angle that put a fifth of the town's
    // signal masts in the crossing carriageway.
    let foot = at + *direction * (widest * SIGNAL_SET_BACK) + kerb * (widest * 0.5 + 0.8);
    // The lenses look down the head's local +Z, and a yaw of theta sends +Z to
    // (sin, cos) — which has to come out as the direction the traffic arrives
    // *from*, or the signal shows its back to the only people it is for.
    Some((foot, direction.x.atan2(direction.y)))
}

/// Puts a signal on each approach to one junction.
///
/// Only where it would actually be: three or more arms, at least one of them an
/// arterial road. A signalled crossroads on a back street is as clear a tell
/// that a city was generated as an unsignalled one on a main road.
pub fn spawn_junction(
    commands: &mut Commands,
    assets: &PropAssets,
    at: Vec2,
    approaches: &[(Vec2, f32)],
    arterial: bool,
    chunk: IVec2,
) {
    if approaches.len() < 3 || !arterial {
        return;
    }
    let widest = approaches
        .iter()
        .map(|(_, width)| *width)
        .fold(0.0f32, f32::max);

    for (towards, _) in approaches {
        let Some((foot, yaw)) = signal_pose(at, *towards, widest) else {
            continue;
        };

        let (post, steel, height) = &assets.signal_post;
        let (head, casing, head_height) = &assets.signal_head;
        commands.spawn((
            ChunkOf(chunk),
            // A signal is a mast in the pavement. It used to be immovable on
            // the grounds that nothing here was funny enough to be worth a
            // set of lights bouncing down a street — and then the game became
            // a comedy, and a set of lights bouncing down a street is now
            // precisely the register it plays in. Heavy and firmly footed all
            // the same: shearing one takes real speed.
            RigidBody::Static,
            Breakaway {
                at: 6.5,
                mass: 90.0,
                geyser: false,
            },
            Collider::cylinder(0.075, SIGNAL_HEIGHT),
            Mesh3d(post.clone()),
            MeshMaterial3d(steel.clone()),
            Transform::from_xyz(foot.x, SIDEWALK_HEIGHT + height * 0.5, foot.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            children![(
                Mesh3d(head.clone()),
                MeshMaterial3d(casing.clone()),
                Transform::from_xyz(0.0, height * 0.5 - head_height * 0.5, 0.0),
                Children::spawn(SpawnIter(
                    assets
                        .signal_lens
                        .clone()
                        .into_iter()
                        .zip(SIGNAL_LENSES)
                        .map(|((lens, glass), y)| {
                            (
                                Mesh3d(lens),
                                MeshMaterial3d(glass),
                                // Cylinders stand up; a lens looks out.
                                Transform::from_xyz(0.0, y, 0.15).with_rotation(
                                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                                ),
                            )
                        })
                        .collect::<Vec<_>>()
                        .into_iter()
                )),
            )],
        ));
    }
}

/// Scatters furniture along both kerbs of one street.
pub fn spawn_edge(
    commands: &mut Commands,
    assets: &PropAssets,
    corridors: &super::streetside::Corridors,
    rng: &mut ChaCha8Rng,
    edge: &RoadEdge,
    from: Vec2,
    to: Vec2,
    chunk: IVec2,
) {
    let Ok(direction) = Dir2::new(to - from) else {
        return;
    };
    let normal = Vec2::new(-direction.y, direction.x);
    let offset = edge.width * 0.5 + SET_BACK;

    // Walked along the segment rather than counted in whole slots of it. The
    // third copy of this bug: `length / SPACING` floored and iterated from one
    // gives nothing at all on any edge under twice the spacing, and Landshut's
    // median segment is 13.7 m against a 14 m spacing — so most of the town had
    // no bin, no bollard, no meter and no bench, and nothing said so.
    // `vegetation` was fixed for this and neither this file nor `streetlights`
    // was.
    let mut along = (edge.length.min(SPACING) * 0.5).max(1.5);
    while along < edge.length - 1.0 {
        let here = along;
        along += SPACING;
        for side in [-1.0f32, 1.0] {
            if rng.random_range(0.0..1.0) > DENSITY {
                continue;
            }
            let jitter = rng.random_range(-2.5..2.5);
            let at = from + *direction * (here + jitter) + normal * offset * side;
            // Not on somebody else's tarmac. The jitter alone can walk a bin
            // into a crossing street, and at a real town's angles the street
            // behind this one is at no angle in particular.
            if corridors.in_the_road(at, 0.5) {
                continue;
            }

            let prop = Prop::pick(rng);
            let (mesh, material, height) = assets.parts(prop);
            // Furniture stands on the pavement, and the meshes are centred, so
            // everything is lifted by half its own height plus the kerb.
            let base = SIDEWALK_HEIGHT + height * 0.5;
            let yaw = rng.random_range(0.0..std::f32::consts::TAU);

            let mut entity = commands.spawn((
                ChunkOf(chunk),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(at.x, base, at.y).with_rotation(Quat::from_rotation_y(yaw)),
                prop.collider(),
            ));
            match prop.footing() {
                Footing::Bolted(breakaway) => {
                    entity.insert(RigidBody::Static);
                    if let Some(breakaway) = breakaway {
                        entity.insert(breakaway);
                    }
                }
                Footing::Loose(mass) => {
                    entity.insert((RigidBody::Dynamic, Mass(mass)));
                }
            }

            if prop == Prop::Planter {
                // The shrub sits in the trough, its underside inside the
                // concrete so it reads as planted rather than balanced. The
                // planter's mesh is centred, so the rim is half its height up.
                let (top_mesh, top_material) = &assets.planter_top;
                entity.with_child((
                    Mesh3d(top_mesh.clone()),
                    MeshMaterial3d(top_material.clone()),
                    Transform::from_xyz(0.0, height * 0.5 + 0.20, 0.0)
                        .with_scale(Vec3::new(1.0, 0.72, 1.0)),
                ));
            }
            if prop == Prop::Sign {
                // The plate rides near the top of its post, facing along the
                // street rather than at a random angle — a sign nobody can read
                // from the road is not a sign.
                let (plate_mesh, plate_material, _) = &assets.sign_plate;
                entity.with_child((
                    Mesh3d(plate_mesh.clone()),
                    MeshMaterial3d(plate_material.clone()),
                    Transform::from_xyz(0.0, height * 0.34, 0.0),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::rng::{stream, stream_for};

    /// The table is sampled in proportion to its weights.
    ///
    /// Checked against the weights themselves rather than against the table's
    /// order: several kinds now share a weight, and which of two equally common
    /// props happens to come up more often over a finite number of draws is
    /// noise, not a property worth asserting.
    #[test]
    fn the_table_is_sampled_in_proportion_to_its_weights() {
        const DRAWS: usize = 40_000;
        let mut rng = stream_for(4, stream::PROPS);
        let mut counts = [0usize; Prop::TABLE.len()];
        for _ in 0..DRAWS {
            let picked = Prop::pick(&mut rng);
            let slot = Prop::TABLE.iter().position(|(p, _)| *p == picked).unwrap();
            counts[slot] += 1;
        }

        let total: u32 = Prop::TABLE.iter().map(|(_, weight)| weight).sum();
        for (slot, (prop, weight)) in Prop::TABLE.iter().enumerate() {
            let expected = DRAWS as f32 * *weight as f32 / total as f32;
            // Generous: the rarest entry expects about twelve hundred draws,
            // so this is still several standard deviations wide.
            assert!(
                (counts[slot] as f32 - expected).abs() < expected * 0.15,
                "{prop:?} came up {} times against an expected {expected:.0}",
                counts[slot]
            );
        }
    }

    #[test]
    fn every_kind_turns_up_eventually() {
        // An off-by-one in the weighted pick makes the last kind unreachable,
        // which nobody notices until they go looking for a post box.
        let mut rng = stream_for(11, stream::PROPS);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..3000 {
            seen.insert(Prop::pick(&mut rng));
        }
        assert_eq!(seen.len(), Prop::TABLE.len());
    }

    #[test]
    fn furniture_stands_on_the_pavement_and_not_in_it() {
        // Meshes are centred on their origin, so a prop placed at pavement
        // height would be buried to its waist.
        for height in [0.72, 0.95, 1.25, 2.35] {
            let base = SIDEWALK_HEIGHT + height * 0.5;
            let foot = base - height * 0.5;
            assert!(
                (foot - SIDEWALK_HEIGHT).abs() < 1e-6,
                "a {height}m prop's foot landed at {foot}, not on the kerb"
            );
        }
    }

    /// A signal that faces the wrong way is still a signal in a screenshot.
    #[test]
    fn a_signal_looks_back_at_the_traffic_it_is_for() {
        // An arm running east out of the origin: traffic arrives heading west.
        let (foot, yaw) =
            signal_pose(Vec2::ZERO, Vec2::new(60.0, 0.0), 17.0).expect("an arm with a length");

        assert!(
            foot.x > 0.0,
            "the signal stands at {foot}, on the far side of the junction"
        );
        let facing = Vec2::new(yaw.sin(), yaw.cos());
        assert!(
            facing.dot(Vec2::X) > 0.999,
            "the signal looks {facing} rather than back up its own arm"
        );
        // A driver heading west has -Z on their right.
        assert_eq!(
            foot.y < 0.0,
            crate::ai::steering::RIGHT_HAND_TRAFFIC,
            "the signal at {foot} is on the wrong kerb for this side of the road"
        );
        // Clear of the widest carriageway meeting here, not standing in it —
        // in both directions. The set-back along the arm always came off the
        // widest arm; the offset across it came off this arm's own half-width,
        // so at a junction of a narrow arm and a wide one the mast stood in the
        // wide one. A fifth of the town's signals did.
        assert!(
            foot.x > 17.0 * 0.5,
            "the signal at {foot} is inside the junction"
        );
        assert!(
            foot.y.abs() > 17.0 * 0.5,
            "the signal at {foot} is in the crossing carriageway, \
             which is 17m wide and not the 12m arm it stands on"
        );

        assert!(signal_pose(Vec2::ZERO, Vec2::ZERO, 17.0).is_none());
    }

    #[test]
    fn furniture_clears_the_carriageway() {
        // Set back from the kerb line, or traffic drives through it.
        let width = 12.0f32;
        let offset = width * 0.5 + SET_BACK;
        assert!(
            offset > width * 0.5,
            "furniture at {offset} is inside a {width}m road"
        );
    }

    #[test]
    fn every_collider_is_the_size_of_the_thing_it_stands_in_for() {
        // The dimensions are written twice — once for the mesh, once for the
        // collider — so this walks every kind and checks the second copy
        // against the standing height the first one is placed by. A mismatch
        // is furniture floating a hand's breadth off the pavement, or sunk
        // into it, and it is invisible until somebody bounces off the gap.
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let assets = build_assets(&mut meshes, &mut materials);

        for (prop, _) in Prop::TABLE {
            let (_, _, height) = assets.parts(prop);
            let aabb = prop.collider().aabb(Vec3::ZERO, Quat::IDENTITY);
            let collider_height = aabb.max.y - aabb.min.y;
            assert!(
                (collider_height - height).abs() < 1e-3,
                "{prop:?} is drawn {height:.3}m tall and collides {collider_height:.3}m tall"
            );
        }
    }

    #[test]
    fn the_things_that_should_not_move_do_not() {
        // A bollard that a car can knock over is not a bollard, and a street
        // whose signs walk about on their own stops reading as a street. But
        // bolted is no longer forever: everything bolted except the bollard
        // shears at some speed, because a comedy city where nothing ever gives
        // way is only half a comedy city.
        assert_eq!(Prop::Bollard.footing(), Footing::Bolted(None));
        for prop in [Prop::Sign, Prop::Meter, Prop::Hydrant, Prop::PhoneBox] {
            assert!(
                matches!(prop.footing(), Footing::Bolted(Some(_))),
                "{prop:?} should be bolted but shearable"
            );
        }
        assert!(matches!(Prop::Bin.footing(), Footing::Loose(_)));
    }

    #[test]
    fn only_the_hydrant_makes_water() {
        for (prop, _) in Prop::TABLE {
            let Footing::Bolted(Some(breakaway)) = prop.footing() else {
                continue;
            };
            assert_eq!(
                breakaway.geyser,
                prop == Prop::Hydrant,
                "{prop:?} has the wrong idea about plumbing"
            );
        }
    }

    #[test]
    fn shearing_the_street_takes_more_speed_the_sturdier_the_fixture() {
        // A sign goes before a hydrant, a hydrant before a phone box. If this
        // ladder flattens, either everything shears at a touch — the street
        // dissolves at parking speed — or nothing does and the milestone is
        // quietly dead.
        let at = |prop: Prop| match prop.footing() {
            Footing::Bolted(Some(breakaway)) => breakaway.at,
            _ => panic!("{prop:?} is not shearable"),
        };
        assert!(at(Prop::Sign) < at(Prop::Hydrant));
        assert!(at(Prop::Hydrant) < at(Prop::PhoneBox));
        // And nothing shears at walking-into-it speed: shearing is a crash,
        // not a bump.
        assert!(at(Prop::Sign) > 2.5);
    }

    #[test]
    fn a_bin_gives_way_more_easily_than_a_planter() {
        let mass = |prop: Prop| match prop.footing() {
            Footing::Loose(mass) => mass,
            Footing::Bolted(_) => f32::INFINITY,
        };
        assert!(mass(Prop::Bin) < mass(Prop::NewsBox));
        assert!(mass(Prop::NewsBox) < mass(Prop::Bench));
        assert!(mass(Prop::Bench) < mass(Prop::Planter));
    }

    /// Triangles in a mesh, however it happens to be indexed.
    fn tris(mesh: &Mesh) -> usize {
        match mesh.indices() {
            Some(bevy::mesh::Indices::U16(i)) => i.len() / 3,
            Some(bevy::mesh::Indices::U32(i)) => i.len() / 3,
            None => mesh.count_vertices() / 3,
        }
    }

    #[test]
    fn a_post_is_never_drawn_finer_than_it_can_show() {
        // The rule the whole rebalance rests on: side count follows radius, so
        // the facet a viewer actually sees stays roughly the same width from
        // the 45 mm sign post to the 1.2 m water tank. If this inverts, the
        // budget goes back where the audit found it — 124 triangles on a mast
        // and 12 on the building behind it.
        assert!(cylinder_sides(0.045) < cylinder_sides(0.11));
        assert!(cylinder_sides(0.11) <= cylinder_sides(0.16));
        assert!(cylinder_sides(0.16) < cylinder_sides(0.26));
        assert!(cylinder_sides(0.26) < cylinder_sides(1.2));
        // Nothing coarser than a hexagon: a pentagon post reads as a mistake
        // rather than as a post.
        assert!(cylinder_sides(0.001) >= 6);
        // And nothing anywhere near Bevy's default of 32, which is where the
        // budget was going.
        assert!(cylinder_sides(9.0) <= 16);

        // Chord width, which is the thing the rule is actually about: a
        // 32-sided 45 mm post has 9 mm flats and an eight-sided 1.2 m tank
        // would have 92 cm ones. Both are wrong. The band table keeps every
        // piece of street furniture between 4.5 and 18 cm, and only the water
        // tank — behind a parapet, gone by 420 m — runs out to the ceiling.
        for radius in [0.045f32, 0.055, 0.075, 0.085, 0.11, 0.16, 0.26, 0.33, 0.35] {
            let sides = cylinder_sides(radius) as f32;
            let chord = 2.0 * radius * (std::f32::consts::PI / sides).sin();
            assert!(
                (0.03..0.20).contains(&chord),
                "a {radius} m cylinder at {sides} sides has {chord:.3} m flats"
            );
        }
        let tank = 2.0 * 1.2 * (std::f32::consts::PI / cylinder_sides(1.2) as f32).sin();
        assert!(tank < 0.50, "even the water tank has {tank:.3} m flats");
    }

    #[test]
    fn the_street_kit_costs_less_than_it_did_at_the_default_resolution() {
        // Six round pieces, all of them previously 124 triangles because
        // `Cylinder::mesh()` defaults to a resolution of 32. A cylinder is
        // `4 * sides - 4` triangles here, so this is also a check that the
        // helper is actually being reached from `build_assets` rather than
        // sitting unused next to six literal `Cylinder::new`s.
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let assets = build_assets(&mut meshes, &mut materials);

        let round = [
            &assets.bin.0,
            &assets.bollard.0,
            &assets.hydrant.0,
            &assets.sign_post.0,
            &assets.signal_post.0,
            &assets.signal_lens[0].0,
        ];
        let total: usize = round
            .iter()
            .map(|handle| tris(meshes.get(*handle).expect("a built mesh")))
            .sum();
        assert!(
            total < 6 * 124,
            "the round street kit is {total} triangles against {} at the default",
            6 * 124
        );
        for handle in round {
            let count = tris(meshes.get(handle).expect("a built mesh"));
            assert!(count <= 44, "one piece of street furniture costs {count}");
        }
    }
}
