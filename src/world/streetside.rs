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

/// One quad of carriageway per street, and one per junction.
///
/// A generated city needs none of this: its ground *is* the asphalt and its
/// block slabs carve the streets out of it as negative space, which is one quad
/// for the whole world. That trick only works when the blocks tile the ground,
/// and a real town's do not — so it runs the other way round here. The ground
/// is grass, and the roads are laid on top of it.
///
/// The meshes are built once, at startup, and never again. They cannot be
/// shared — a ribbon's UVs carry its own length and width so the asphalt tiles
/// at the right size however long the street is — and building them in the
/// streaming path would add two thousand meshes to `Assets<Mesh>` every time a
/// chunk came back, for ever.
#[derive(Resource)]
pub struct Ribbons {
    /// Indexed by `EdgeId`.
    roads: Vec<Handle<Mesh>>,
    /// One shared square for a crossing. Small enough that a fixed tiling is
    /// right whatever it is stretched over.
    junction: Handle<Mesh>,
    /// One material per [`super::atlas::Surface`], indexed by its own `index`.
    paving: [Handle<super::road::RoadMaterial>; 4],
}

impl Ribbons {
    fn material(&self, surface: super::atlas::Surface) -> Handle<super::road::RoadMaterial> {
        self.paving[surface.index()].clone()
    }
}

/// Metres of road one repeat of the asphalt covers. The same number the one
/// big ground quad uses, so a ribbon and a generated city's road are the same
/// asphalt at the same size.
const TILE: f32 = super::ASPHALT_TILE;

/// A flat quad `width` by `length`, lying in XZ, with UVs that tile the asphalt
/// at its true size.
fn ribbon(width: f32, length: f32) -> Mesh {
    let (hw, hl) = (width * 0.5, length * 0.5);
    let (u, v) = (width / TILE, length / TILE);
    Mesh::new(
        bevy::render::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-hw, 0.0, -hl],
            [hw, 0.0, -hl],
            [hw, 0.0, hl],
            [-hw, 0.0, hl],
        ],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [u, 0.0], [u, v], [0.0, v]],
    )
    // Wound anticlockwise seen from above, which is the way round that faces
    // *up*. The obvious order — 0,1,2 then 0,2,3 across those four corners —
    // is clockwise from above, so every ribbon in the city was back-facing and
    // culled: two thousand invisible roads, and a town that looked like it had
    // grass where its carriageways should be.
    .with_inserted_indices(bevy::render::mesh::Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}

/// Builds the carriageway of a whole town, once.
pub fn build_ribbons(
    layout: &CityLayout,
    meshes: &mut Assets<Mesh>,
    paving: [Handle<super::road::RoadMaterial>; 4],
) -> Ribbons {
    let roads = layout
        .graph
        .edges()
        .map(|edge| {
            // Wider than the carriageway by a pavement either side, and longer
            // than the street by its own width.
            //
            // The length is so that ribbons overlap at every junction; without
            // it a crossing shows four green wedges where they stop. The width
            // is so the asphalt runs *under* the kerb, which is where a road
            // bed actually goes: the pavement slab is an opaque box sitting on
            // top of it, so none of the extra is ever seen, and any daylight
            // between the two — from a width that rounds differently, from two
            // streets a metre out of parallel — comes out as more road instead
            // of as a green stripe down the gutter.
            meshes.add(super::buildings::with_tangents(ribbon(
                edge.width + SIDEWALK_WIDTH * 2.0,
                edge.length + edge.width,
            )))
        })
        .collect();
    Ribbons {
        roads,
        junction: meshes.add(super::buildings::with_tangents(ribbon(1.0, 1.0))),
        paving,
    }
}

/// How far short of one of its nodes a pavement has to stop.
///
/// A pavement runs beside its own carriageway, and where another street crosses
/// it that puts the strip out in the middle of *that* street's tarmac — for
/// half the crossing width, at both ends, on both sides. It is a kerb-height
/// slab lying across the road at every junction in the town, and it is exactly
/// what it looked like: pavements running into the carriageway. So the strip
/// stops where the crossing carriageway begins.
///
/// A node where only two edges meet is not a crossing at all, it is a kink in
/// one street, and there the two strips have to *overlap* or the bend shows a
/// notch — hence the negative return: half a pavement's width past the node,
/// which is the join these strips have always had.
///
/// `widest_other` is the widest street at the node that is not this one, so a
/// back lane meeting a dual carriageway is held back by the dual carriageway
/// and not by itself.
pub fn pavement_trim(widest_other: f32, arms: usize) -> f32 {
    match arms >= 3 {
        true => widest_other * 0.5,
        false => -SIDEWALK_WIDTH * 0.5,
    }
}

/// Lays the two pavements of one street.
///
/// A strip either side rather than a slab round a block, because a block on a
/// real map is not a rectangle. `trim` is what [`pavement_trim`] says to cut
/// off each end: an overlap at a bend, and a real setback at a crossing.
#[allow(clippy::too_many_arguments)]
pub fn spawn_edge(
    commands: &mut Commands,
    kit: &StreetsideKit,
    ribbons: &Ribbons,
    kerb: &Handle<StandardMaterial>,
    id: super::roadgraph::EdgeId,
    edge: &RoadEdge,
    from: Vec2,
    to: Vec2,
    trim: (f32, f32),
    chunk: IVec2,
    range: f32,
) {
    let Ok(direction) = Dir2::new(to - from) else {
        return;
    };
    let normal = Vec2::new(-direction.y, direction.x);
    let yaw = direction.x.atan2(direction.y);
    let middle = from.midpoint(to);

    // The carriageway. Just off the ground so it wins the depth test against
    // the grass without z-fighting it.
    if let Some(mesh) = ribbons.roads.get(id.0 as usize) {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(ribbons.material(edge.surface)),
            Transform::from_xyz(middle.x, 0.012, middle.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
        ));
    }
    let visibility = bevy::camera::visibility::VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: range..(range * 1.05),
        use_aabb: false,
    };

    // What is left of the street once both ends have been given back to
    // whatever crosses them. A trim is negative at a bend, where the strips
    // are meant to overlap, and the arithmetic is the same either way.
    let paved = edge.length - trim.0 - trim.1;
    if paved < 0.5 {
        return;
    }
    // Cut unevenly at the two ends, so the middle of the strip is no longer the
    // middle of the street.
    let along = middle + *direction * ((trim.0 - trim.1) * 0.5);

    for side in [-1.0f32, 1.0] {
        let at = along + normal * (side * (edge.width * 0.5 + SIDEWALK_WIDTH * 0.5));
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.slab.clone()),
            MeshMaterial3d(kerb.clone()),
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT * 0.5, at.y)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::new(SIDEWALK_WIDTH, SIDEWALK_HEIGHT, paved)),
            visibility.clone(),
        ));
    }

    // The kerb the player steps up onto, as two boxes that stop *short* of the
    // slabs above.
    //
    // The trimming is not tidiness. Laid at the slabs' own length these
    // overlap every neighbour at every corner, and four and a half thousand
    // long overlapping static boxes with two and a half thousand parked cars
    // sitting among them took a settled frame from twenty-five milliseconds to
    // three hundred and seventeen. The picture is unchanged either way: what is
    // trimmed away is the metre of kerb under a crossing, where there is no
    // kerb.
    let stub = (paved - SIDEWALK_WIDTH * 2.0).max(0.0);
    if stub < 1.0 {
        return;
    }
    for side in [-1.0f32, 1.0] {
        let at = along + normal * (side * (edge.width * 0.5 + SIDEWALK_WIDTH * 0.5));
        commands.spawn((
            ChunkOf(chunk),
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT * 0.5, at.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            avian3d::prelude::RigidBody::Static,
            avian3d::prelude::Collider::cuboid(SIDEWALK_WIDTH, SIDEWALK_HEIGHT, stub),
        ));
    }
}

/// Paves a crossing, so the ribbons meeting there do not leave a hole.
pub fn spawn_junction(
    commands: &mut Commands,
    ribbons: &Ribbons,
    at: Vec2,
    widest: f32,
    surface: super::atlas::Surface,
    chunk: IVec2,
) {
    // A square the size of the widest street meeting here, plus its pavements
    // for the same reason the ribbons carry theirs. Square rather than
    // fitted to the arms, because a junction is covered by the ribbons of its
    // own arms except for the diamond in the very middle, and a square covers
    // that whatever angle the arms arrive at.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(ribbons.junction.clone()),
        // Paved as the widest arm is. A junction between a cobbled square and
        // a tarmac street is one or the other, and the bigger road is the one
        // whose surfacing gang got there.
        MeshMaterial3d(ribbons.material(surface)),
        Transform::from_xyz(at.x, 0.010, at.y).with_scale(Vec3::new(
            widest + SIDEWALK_WIDTH * 2.0,
            1.0,
            widest + SIDEWALK_WIDTH * 2.0,
        )),
    ));
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

/// A rectangle with a direction: a building's plot, or a road's corridor.
///
/// Both are the same shape and neither is axis-aligned, which is the whole
/// reason this exists. A generated city could ask "is this lot inside that
/// block?" because its blocks were rectangles on the map; a real town has
/// streets meeting at every angle there is, and a plot that faces one of them
/// squarely sits at some arbitrary angle to the next.
#[derive(Clone, Copy)]
struct Oblong {
    centre: Vec2,
    /// Unit vector along the local X — the frontage of a plot, the run of a
    /// road.
    axis: Vec2,
    /// Half the extent along that axis and across it.
    half: Vec2,
}

impl Oblong {
    fn across(&self) -> Vec2 {
        Vec2::new(-self.axis.y, self.axis.x)
    }

    /// How far this reaches along `n`, from its own middle.
    fn reach(&self, n: Vec2) -> f32 {
        self.half.x * self.axis.dot(n).abs() + self.half.y * self.across().dot(n).abs()
    }

    /// Do the two overlap by more than `margin` in every direction?
    ///
    /// The separating-axis test: two convex shapes miss each other if there is
    /// one direction they do not overlap in, and for two rectangles the only
    /// directions worth trying are their four sides. `margin` is what lets a
    /// building stand with its front wall exactly on the edge of the pavement,
    /// which is where a building goes, without that counting as standing in the
    /// road.
    fn clashes_with(&self, other: &Oblong, margin: f32) -> bool {
        let between = other.centre - self.centre;
        ![self.axis, self.across(), other.axis, other.across()]
            .into_iter()
            .any(|n| between.dot(n).abs() + margin >= self.reach(n) + other.reach(n))
    }
}

/// How deep into the tarmac a corner has to reach before it is in the road.
///
/// Not zero, and it cannot be: a plot is placed with its front wall exactly on
/// the corridor's edge, so at nought every single building in the town would
/// reject itself against the street it faces. A hand's width of slack passes
/// that and still catches the failure this is here for — the corner of a
/// house standing out in a *second* street that happens to run past the back
/// of it, which nothing checked at all before, because the only clash test
/// there was compared buildings against other buildings.
const IN_THE_ROAD: f32 = 0.20;

/// Every road, filed by cell, so a candidate plot only tests the handful of
/// streets that could possibly be under it.
///
/// Without this the check is every plot against every road: six thousand times
/// two thousand for Landshut, which is thirteen million tests to place a town.
fn corridors(layout: &CityLayout) -> HashMap<(i32, i32), Vec<Oblong>> {
    let mut filed: HashMap<(i32, i32), Vec<Oblong>> = HashMap::default();
    for edge in layout.graph.edges() {
        let a = layout.graph.node(edge.a).pos;
        let b = layout.graph.node(edge.b).pos;
        let Ok(direction) = Dir2::new(b - a) else {
            continue;
        };
        // The corridor is the carriageway and the pavement either side of it:
        // the ground a building may not stand on.
        let road = Oblong {
            centre: a.midpoint(b),
            axis: *direction,
            half: Vec2::new(edge.length * 0.5, edge.width * 0.5 + SIDEWALK_WIDTH),
        };
        // Filed into every cell its bounding box touches, which for a long
        // street is a lot of cells and for a stub is one.
        let extent = Vec2::new(road.reach(Vec2::X), road.reach(Vec2::Y));
        let low = ((road.centre - extent) / CELL).floor().as_ivec2();
        let high = ((road.centre + extent) / CELL).floor().as_ivec2();
        for x in low.x..=high.x {
            for z in low.y..=high.y {
                filed.entry((x, z)).or_default().push(road);
            }
        }
    }
    filed
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
    let roads = corridors(layout);
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
                // And is it standing in a road? Not the one it faces — that one
                // it is placed against on purpose — but any *other* street that
                // happens to run behind or beside it. An OSM town is full of
                // them: a lane ending a few metres off a main road, two streets
                // meeting at thirty degrees, a footway threading a block. The
                // frontage of a plot squares up to its own street and therefore
                // sits at some arbitrary angle to every other one, which is why
                // this is a rotated-rectangle test and not a box overlap.
                let plot = Oblong {
                    centre,
                    axis: *direction,
                    half: Vec2::new(frontage, depth) * 0.5,
                };
                let paved = (-1..=1).any(|dx| {
                    (-1..=1).any(|dz| {
                        roads.get(&(cell.0 + dx, cell.1 + dz)).is_some_and(|near| {
                            near.iter().any(|road| plot.clashes_with(road, IN_THE_ROAD))
                        })
                    })
                });
                if paved {
                    continue;
                }
                taken.entry(cell).or_default().push((centre, radius));

                let district = district_at(centre, layout.half_extent, edge.arterial);
                let (low, high) = style.heights(district.height_range());
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

    /// A pavement stops where the crossing carriageway starts.
    #[test]
    fn a_pavement_gives_way_to_the_street_that_crosses_it() {
        // A crossing: the strip has to be clear of the other street's tarmac,
        // which reaches half its width out from the node.
        for width in [6.0f32, 9.0, 15.0] {
            let trim = pavement_trim(width, 4);
            assert!(
                trim >= width * 0.5 - 1e-6,
                "a {width}m street is crossed by a pavement stopping {trim}m short"
            );
        }
        // A kink in one street is not a crossing, and there the strips have to
        // meet — which means running *past* the node, not short of it.
        assert!(
            pavement_trim(9.0, 2) < 0.0,
            "a bend leaves a notch of bare asphalt between its two pavements"
        );
    }

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

    /// A ribbon faces the sky.
    #[test]
    fn a_road_is_not_laid_upside_down() {
        // The one thing about this mesh that fails silently and completely.
        // A quad wound the wrong way is culled, so the road is not dark or
        // striped or in the wrong place — it is simply not there, and what is
        // underneath it looks like the answer.
        let mesh = ribbon(8.0, 40.0);
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::render::mesh::VertexAttributeValues::Float32x3(v)) => v.clone(),
            _ => panic!("a ribbon has no positions"),
        };
        let indices: Vec<u32> = match mesh.indices() {
            Some(bevy::render::mesh::Indices::U32(i)) => i.clone(),
            _ => panic!("a ribbon has no indices"),
        };
        assert_eq!(indices.len(), 6, "a quad is two triangles");
        for triangle in indices.chunks(3) {
            let corner = |i: u32| Vec3::from_array(positions[i as usize]);
            let (a, b, c) = (
                corner(triangle[0]),
                corner(triangle[1]),
                corner(triangle[2]),
            );
            let facing = (b - a).cross(c - a);
            assert!(
                facing.y > 0.0,
                "a triangle faces {facing:?}, which is into the ground"
            );
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

    /// Two rectangles at an angle to one another are told apart correctly.
    #[test]
    fn an_oblong_knows_what_it_is_standing_in() {
        let road = Oblong {
            centre: Vec2::ZERO,
            axis: Vec2::X,
            half: Vec2::new(50.0, 6.0),
        };
        // Squarely alongside, its front wall exactly on the kerb: this is where
        // every building in the town is put, and it must not read as a clash.
        let beside = Oblong {
            centre: Vec2::new(0.0, 6.0 + 7.0),
            axis: Vec2::X,
            half: Vec2::new(9.0, 7.0),
        };
        assert!(!beside.clashes_with(&road, IN_THE_ROAD));

        // Turned forty-five degrees on the same spot, so a corner swings into
        // the carriageway. This is the failure: a plot squared up to one street
        // and slanted across another.
        let slanted = Oblong {
            axis: Vec2::new(1.0, 1.0).normalize(),
            ..beside
        };
        assert!(
            slanted.clashes_with(&road, IN_THE_ROAD),
            "a corner in the road was not noticed"
        );

        // Well clear is well clear, whatever the angle.
        let away = Oblong {
            centre: Vec2::new(0.0, 40.0),
            ..slanted
        };
        assert!(!away.clashes_with(&road, IN_THE_ROAD));
    }

    /// Nothing is built on the tarmac of a street it does not face.
    #[test]
    fn no_house_stands_in_a_crossing_street() {
        // A T: a long road east to west, and a lane running south off the
        // middle of it. The lane's own buildings are placed square to the lane
        // and reach back towards the main road, and before the corridor test
        // the ones near the top of the lane stood in it.
        let mut graph = crate::world::roadgraph::RoadGraph::default();
        let west = graph.add_node(Vec2::new(-200.0, 0.0), (0, 0));
        let middle = graph.add_node(Vec2::new(0.0, 0.0), (0, 1));
        let east = graph.add_node(Vec2::new(200.0, 0.0), (0, 2));
        let south = graph.add_node(Vec2::new(0.0, 200.0), (0, 3));
        graph.connect(
            west,
            middle,
            14.0,
            true,
            crate::world::atlas::Surface::Asphalt,
        );
        graph.connect(middle, east, 14.0, true, crate::world::atlas::Surface::Sett);
        graph.connect(
            middle,
            south,
            7.5,
            false,
            crate::world::atlas::Surface::Asphalt,
        );

        let layout = CityLayout {
            seed: 1,
            half_extent: 400.0,
            x_streets: Vec::new(),
            z_streets: Vec::new(),
            blocks: Vec::new(),
            graph,
            canal: None,
        };
        let blocks = lots(&layout, 1, CityStyle::Landshuepf);
        assert!(!blocks.is_empty(), "nothing was built at all");

        let roads = corridors(&layout);
        for block in &blocks {
            let building = &block.buildings[0];
            let yaw = building.facing.expect("a lot faces its street");
            let plot = Oblong {
                centre: building.footprint.center(),
                // `facing` turns +Z outwards, so local +X — the frontage — is
                // the way the street runs.
                axis: Vec2::new(yaw.cos(), -yaw.sin()),
                half: building.footprint.size() * 0.5,
            };
            for near in roads.values() {
                for road in near {
                    assert!(
                        !plot.clashes_with(road, IN_THE_ROAD),
                        "a house at {} is standing in the road",
                        plot.centre
                    );
                }
            }
        }
    }
}
