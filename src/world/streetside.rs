//! Filling a real town: pavements laid along its streets, and buildings lined
//! up on them.
//!
//! `world::atlas` gives the game Landshut's road network and nothing else. A
//! network is not a city — without pavements the whole world is carriageway,
//! and without buildings there is nothing to walk between. This is what puts
//! them there.
//!
//! ## Why not blocks
//!
//! The generator works the other way round: it lays out rectangular blocks and
//! subdivides each into lots. That needs the *faces* of the street network —
//! the closed loops of road that enclose a block — and finding those in an
//! arbitrary planar graph is a half-edge traversal with every degenerate case
//! a real map contains: dead ends, dual carriageways, footpaths that cross
//! without a junction, ways that leave the square. It is a fortnight of work
//! and it is the wrong fortnight, because a town was never built that way.
//!
//! A town was built *along its streets*. Somebody bought a frontage and put a
//! house on it, and the block in the middle is whatever was left. So that is
//! what happens here: march down each side of each street, hand out frontages,
//! and let the middle be the middle. It needs no faces, no polygons and no
//! subdivision, and it produces the thing the faces were only ever a means to.
//!
//! ## What keeps two streets from building into each other
//!
//! Nothing in the marching does — two streets thirty metres apart will both
//! want the ground between them. So every building is checked against the ones
//! already placed, through a coarse spatial grid: a candidate whose middle is
//! nearer an existing middle than the two of them can both fit is dropped, and
//! the frontage carries on past the gap. The gaps are not a defect. A back
//! street that is built up on one side and open on the other is what the inside
//! of a real block looks like.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::citygen::{
    Block, Building, BuildingKind, CityLayout, District, PALETTE_SIZE, Rect, SIDEWALK_WIDTH,
};
use super::roadgraph::RoadEdge;
use crate::core::config::CityStyle;

/// How far a pavement strip is drawn. The same as a building's far shell: a
/// kerb is what tells you where the road stops, and a street without one reads
/// as a runway from any distance.
pub const RANGE: f32 = 900.0;

/// Clear of a junction, so nothing is built across a crossing.
const JUNCTION_CLEAR: f32 = 9.0;

/// And clear of a plain bend, which is almost nothing.
///
/// The difference between these two is worth more than any other number in
/// this module. An edge here is one *segment* of a polyline, not a whole
/// street: a curved road arrives as a dozen edges of twenty-odd metres each,
/// and holding nine metres clear at both ends of every one of them leaves
/// three metres to build on and skips the rest outright. Landshut came out at
/// thirteen hundred buildings with the whole middle of every block bare.
///
/// A junction is a node three or more edges meet at. A node where exactly two
/// meet is a kink in one street, and a terrace runs straight through it.
const BEND_CLEAR: f32 = 0.6;

/// Frontage handed to one building, before the style's own lot scale.
const FRONTAGE: (f32, f32) = (9.0, 21.0);
/// And how far back it goes.
const DEPTH: (f32, f32) = (11.0, 19.0);

/// Chance the marcher leaves a hole rather than building the next frontage:
/// an entry to a yard, a gap somebody never filled, the corner of a block that
/// belongs to the street round the corner.
const HOLE: f32 = 0.26;
/// And how wide one is.
const HOLE_WIDTH: (f32, f32) = (5.0, 16.0);

/// Cell of the occupancy grid, in metres. About the size of one building, so a
/// candidate only ever has to look at nine cells.
const CELL: f32 = 20.0;

/// How much of the sum of two buildings' circumradii has to separate their
/// middles before they are allowed to stand.
///
/// A circumradius is the circle a rectangle fits inside, so it overstates a
/// building's half-width by up to a factor of root two — which is why this is
/// well under one. At 0.80, which is where it started, two ordinary
/// fourteen-metre houses had to stand sixteen metres apart to be allowed:
/// a terrace was rejected as a clash, and the whole of Landshut came out at a
/// thousand buildings with holes everywhere. Below about 0.45 the check stops
/// rejecting real overlaps.
const CLEARANCE: f32 = 0.62;

// Nothing is built across a junction: the clearance at each end of a street has
// to be wide enough for the widest carriageway the baker emits — a fifteen-metre
// motorway — to cross without the corner building standing in it. And a street
// shorter than two clearances plus a frontage is skipped rather than half
// built, so that has to stay under the length of a back street.
const _: () = assert!(JUNCTION_CLEAR > 15.0 * 0.5);
const _: () = assert!(JUNCTION_CLEAR * 2.0 + FRONTAGE.0 < 30.0);

/// Where the town stops being a town, as a fraction of the half-extent.
///
/// A real extract has no districts in it — nobody tags a street "downtown" —
/// so the one thing the game can honestly read off the geometry is *where the
/// middle is*. Everything else is the road class: a shop belongs on a main
/// road and a house belongs on a back street, which is true of every town
/// anybody has ever walked through.
const CORE: f32 = 0.22;
const INNER: f32 = 0.55;

/// Meshes and materials for the pavement strips.
#[derive(Resource)]
pub struct StreetsideKit {
    slab: Handle<Mesh>,
}

pub fn build_assets(meshes: &mut Assets<Mesh>) -> StreetsideKit {
    StreetsideKit {
        slab: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
    }
}

/// Lays the two pavements of one street.
///
/// A strip either side rather than a slab round a block, because a block on a
/// real map is not a rectangle. Strips overlap a little at every junction,
/// which costs nothing: they are the same height, the same material, and the
/// overlap is under the crossing.
#[allow(clippy::too_many_arguments)]
pub fn spawn_edge(
    commands: &mut Commands,
    kit: &StreetsideKit,
    kerb: &Handle<StandardMaterial>,
    edge: &RoadEdge,
    from: Vec2,
    to: Vec2,
    chunk: IVec2,
    range: f32,
) {
    let Ok(direction) = Dir2::new(to - from) else {
        return;
    };
    let normal = Vec2::new(-direction.y, direction.x);
    let yaw = direction.x.atan2(direction.y);
    let middle = from.midpoint(to);
    let visibility = bevy::camera::visibility::VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: range..(range * 1.05),
        use_aabb: false,
    };

    for side in [-1.0f32, 1.0] {
        // Overlapping its own junctions at both ends, so a crossing is paved
        // rather than showing four notches of asphalt where the strips stop.
        let at = middle + normal * (side * (edge.width * 0.5 + SIDEWALK_WIDTH * 0.5));
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.slab.clone()),
            MeshMaterial3d(kerb.clone()),
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT * 0.5, at.y)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::new(
                    SIDEWALK_WIDTH,
                    SIDEWALK_HEIGHT,
                    edge.length + SIDEWALK_WIDTH,
                )),
            visibility.clone(),
        ));
    }

    // The kerb the player steps up onto, as two boxes that stop *short* of the
    // junctions the slabs above run through.
    //
    // The trimming is not tidiness. Laid at the slabs' own length these
    // overlap every neighbour at every corner, and four and a half thousand
    // long overlapping static boxes with two and a half thousand parked cars
    // sitting among them took a settled frame from twenty-five milliseconds to
    // three hundred and seventeen. The picture is unchanged either way: what is
    // trimmed away is the metre of kerb under a crossing, where there is no
    // kerb.
    let stub = (edge.length - SIDEWALK_WIDTH * 2.0).max(0.0);
    if stub < 1.0 {
        return;
    }
    for side in [-1.0f32, 1.0] {
        let at = middle + normal * (side * (edge.width * 0.5 + SIDEWALK_WIDTH * 0.5));
        commands.spawn((
            ChunkOf(chunk),
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT * 0.5, at.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            avian3d::prelude::RigidBody::Static,
            avian3d::prelude::Collider::cuboid(SIDEWALK_WIDTH, SIDEWALK_HEIGHT, stub),
        ));
    }
}

/// Which district a point of a real town behaves like.
///
/// Read off the two things an extract actually knows: how far from the middle
/// it is, and whether the street is a main road.
fn district_at(at: Vec2, half_extent: f32, arterial: bool) -> District {
    let out = at.length() / half_extent.max(1.0);
    match out {
        r if r < CORE && arterial => District::Downtown,
        r if r < CORE => District::Midtown,
        r if r < INNER && arterial => District::Midtown,
        r if r < INNER => District::Residential,
        _ if arterial => District::Residential,
        _ => District::Industrial,
    }
}

/// Everything a real town gets built along its streets.
///
/// Returns one [`Block`] per building — unpaved, because the pavement is laid
/// by [`spawn_edge`] instead — which is what lets every downstream spawner,
/// from the facade shells to the geraniums, take a real town without knowing
/// there is one.
pub fn lots(layout: &CityLayout, seed: u64, style: CityStyle) -> Vec<Block> {
    let mut rng = crate::core::rng::stream_for(seed, crate::core::rng::stream::BUILDINGS);
    let mut blocks = Vec::new();
    // Middles of everything placed so far, bucketed by cell. A candidate only
    // looks at its own cell and the eight around it.
    let mut taken: HashMap<(i32, i32), Vec<(Vec2, f32)>> = HashMap::default();
    let scale = style.lot_scale();

    for edge in layout.graph.edges() {
        let a = layout.graph.node(edge.a).pos;
        let b = layout.graph.node(edge.b).pos;
        let Ok(direction) = Dir2::new(b - a) else {
            continue;
        };
        let normal = Vec2::new(-direction.y, direction.x);
        // How much of this segment is buildable: everything except what the
        // nodes at its ends need kept clear, which is a lot at a crossing and
        // nothing at a bend.
        let clear_at = |node| {
            if layout.graph.node(node).edges.len() >= 3 {
                JUNCTION_CLEAR
            } else {
                BEND_CLEAR
            }
        };
        let (head, tail) = (clear_at(edge.a), clear_at(edge.b));
        let run = edge.length - head - tail;
        if run < FRONTAGE.0 * scale {
            continue;
        }

        for side in [-1.0f32, 1.0] {
            // The building line: past the carriageway and past the pavement.
            let line = edge.width * 0.5 + SIDEWALK_WIDTH;
            // A mesh's +Z faces the street it stands on, which is the way back
            // across the pavement.
            let yaw = (-normal.x * side).atan2(-normal.y * side);
            let mut along = head;

            while along < run + head {
                if rng.random_range(0.0..1.0) < HOLE {
                    along += rng.random_range(HOLE_WIDTH.0..HOLE_WIDTH.1);
                    continue;
                }
                let frontage = rng.random_range(FRONTAGE.0..FRONTAGE.1) * scale;
                if along + frontage > run + head {
                    break;
                }
                let depth = rng.random_range(DEPTH.0..DEPTH.1);
                let centre = a
                    + *direction * (along + frontage * 0.5)
                    + normal * (side * (line + depth * 0.5));
                along += frontage + 0.4;

                // Would it stand in something already built? The two radii are
                // the circles the buildings fit inside, and eight tenths of
                // their sum is close enough to touching to call it a clash —
                // exactly touching is what a terrace is, and a terrace is
                // wanted.
                let radius = Vec2::new(frontage, depth).length() * 0.5;
                let cell = (
                    (centre.x / CELL).floor() as i32,
                    (centre.y / CELL).floor() as i32,
                );
                let clash = (-1..=1).any(|dx| {
                    (-1..=1).any(|dz| {
                        taken
                            .get(&(cell.0 + dx, cell.1 + dz))
                            .is_some_and(|others| {
                                others.iter().any(|(other, other_radius)| {
                                    centre.distance(*other) < (radius + other_radius) * CLEARANCE
                                })
                            })
                    })
                });
                if clash {
                    continue;
                }
                taken.entry(cell).or_default().push((centre, radius));

                let district = district_at(centre, layout.half_extent, edge.arterial);
                let (low, high) = district.height_range();
                let (low, high) = (
                    (low * style.height_scale()).max(4.0),
                    (high * style.height_scale()).max(5.0),
                );
                let height = rng.random_range(low..high);
                // The footprint is read in the site's own frame — frontage
                // across, depth back — because `Building::facing` is set. It is
                // never a rectangle on the map, and nothing treats it as one.
                let half = Vec2::new(frontage, depth) * 0.5;
                let footprint = Rect::new(centre - half, centre + half);
                let building = Building {
                    footprint,
                    facing: Some(yaw),
                    height,
                    palette: rng.random_range(0..PALETTE_SIZE),
                    kind: kind_for(&mut rng, district, edge.arterial),
                };
                blocks.push(Block {
                    // Only ever read for filing this into a chunk and for the
                    // minimap, both of which want a world box round the thing.
                    area: Rect::new(centre - Vec2::splat(radius), centre + Vec2::splat(radius)),
                    paved: false,
                    district,
                    buildings: vec![building],
                    vacants: Vec::new(),
                    arterial: [edge.arterial; 4],
                    quarter: None,
                });
            }
        }
    }

    blocks
}

/// What gets built on a given street.
///
/// The one piece of urban logic an extract can support: shops and offices face
/// a main road, flats face a back street, and the civic kinds are left to the
/// zoning pass that already exists.
fn kind_for(rng: &mut ChaCha8Rng, district: District, arterial: bool) -> BuildingKind {
    let roll = rng.random_range(0.0..1.0);
    match (district, arterial) {
        (District::Downtown, _) if roll < 0.45 => BuildingKind::Offices,
        (District::Downtown, _) if roll < 0.62 => BuildingKind::Hotel,
        (_, true) if roll < 0.22 => BuildingKind::Supermarket,
        (_, true) if roll < 0.44 => BuildingKind::Restaurant,
        (_, true) if roll < 0.58 => BuildingKind::Offices,
        _ => BuildingKind::Apartments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A building placed along a street looks at it.
    #[test]
    fn a_frontage_faces_its_own_street() {
        // The yaw has to turn a mesh's +Z back across the pavement towards the
        // carriageway, on both sides of a street running any direction. Get the
        // sign wrong and half the town has its back to the road — with its
        // doorway, its sign and its geraniums on the wall nobody can see.
        for angle in [0.0f32, 0.7, 2.4, -1.9] {
            let direction = Vec2::new(angle.sin(), angle.cos());
            let normal = Vec2::new(-direction.y, direction.x);
            for side in [-1.0f32, 1.0] {
                let yaw = (-normal.x * side).atan2(-normal.y * side);
                let looks = Vec2::new(yaw.sin(), yaw.cos());
                // The building stands on `+normal * side`, so it must look the
                // other way.
                assert!(
                    looks.dot(normal * side) < -0.999,
                    "a building on side {side} of a {angle} street looks {looks:?}"
                );
            }
        }
    }

    /// The middle of a town is denser than its edge.
    #[test]
    fn a_real_town_has_a_middle() {
        assert_eq!(
            district_at(Vec2::ZERO, 1000.0, true),
            District::Downtown,
            "the main road through the middle is not downtown"
        );
        assert_eq!(district_at(Vec2::ZERO, 1000.0, false), District::Midtown);
        assert_eq!(
            district_at(Vec2::new(900.0, 0.0), 1000.0, false),
            District::Industrial
        );
        // And a main road keeps some life in it all the way out.
        assert_ne!(
            district_at(Vec2::new(900.0, 0.0), 1000.0, true),
            District::Industrial
        );
        // Nothing is ever a park: a park has no frontage to build on, and
        // pretending a street is one would put a lawn across it.
        for radius in [0.0f32, 300.0, 600.0, 1_200.0] {
            for arterial in [false, true] {
                assert_ne!(
                    district_at(Vec2::new(radius, 0.0), 1000.0, arterial),
                    District::Park
                );
            }
        }
    }

    /// Two streets close together do not build into each other.
    #[test]
    fn a_clash_is_rejected_before_it_is_placed() {
        // The rule is that two middles closer than `CLEARANCE` of the sum of
        // their circumradii clash. Two 14x14m buildings share a wall at 14m
        // apart and overlap below that, and the check has to reject the
        // overlap while letting a terrace stand.
        let radius = Vec2::new(14.0, 14.0).length() * 0.5;
        let apart = (radius + radius) * CLEARANCE;
        assert!(apart < 14.0, "a terrace would be rejected as a clash");
        assert!(apart > 9.0, "two buildings could stand on the same ground");
    }
}
