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

/// How deep the ground a hole in the frontage puts on show runs before the
/// middle of the block takes over.
///
/// A little under the shallowest plot the marcher hands out, so a forecourt
/// laid across a gap never reaches past the back wall of the houses either
/// side of it. What is beyond that is the inside of the block, and that is the
/// ground shader's job — see `world::urban_mask`.
const FORECOURT: f32 = 9.5;

/// Anything narrower than this is a passage between two houses, not a yard,
/// and putting a gate across it would only draw attention to a hole three
/// metres wide.
const FORECOURT_MIN: f32 = 3.6;

/// A boundary's proportions: how tall a wall or a hedge across a gap stands,
/// and how thick it is.
///
/// Chest height at the top of the range. Higher and a street of gaps becomes a
/// street of walls, which is a different town from the one on the map; lower
/// and it stops hiding the ground it is there to hide.
const BOUNDARY_HEIGHT: (f32, f32) = (0.85, 1.45);
const BOUNDARY_THICK: f32 = 0.34;

/// How often a gap is left as a plain opening — a driveway, an entry, the way
/// through to somebody's yard.
///
/// A third, because that is roughly how often a hole in a real terrace is one.
/// Nought would fence the whole town off from itself, which is worse than the
/// bare meadow this replaced: at least meadow admits you can walk through it.
const OPENING: f32 = 0.32;

/// How far behind the building in front the back land starts, and how much
/// space is left between one outbuilding and the next.
const BACKYARD: (f32, f32) = (4.5, 13.0);

/// How many the marcher will try to put behind one frontage, and how often it
/// carries on to the next.
///
/// Three, falling off: a deep block gets a workshop and a garage behind the
/// house and then stops, which is what the back of a town looks like. Carrying
/// on further would build a second street with no street on it.
const OUTBUILDINGS: usize = 3;
const BACK_BUILT: f32 = 0.62;

/// An outbuilding's size against the frontage it stands behind, its own depth,
/// and how tall it is.
///
/// Small and low, and both matter. A back building the size of the house in
/// front of it is a second house, and a town of them reads as a housing estate
/// rather than as an old town with sheds behind it.
const BACK_SPAN: (f32, f32) = (0.45, 1.05);
const BACK_DEPTH: (f32, f32) = (4.0, 11.0);
const BACK_HEIGHT: (f32, f32) = (3.0, 7.5);

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

/// Meshes shared by every pavement in the town.
/// A stretch of street frontage the marcher handed to nobody.
///
/// The holes are not a defect — see the note at the top of this module about
/// what the inside of a real block looks like — but for as long as the marcher
/// recorded nothing, a hole meant the world plain ran straight up to the kerb,
/// which is the one thing a hole in a real town is never. It is a wall, a gate,
/// a yard entrance or a hardstanding. So the hole is now a thing rather than a
/// `continue`, and this is what it carries.
#[derive(Clone, Copy)]
pub struct Gap {
    /// Middle of the run, on the building line — the back edge of the pavement.
    centre: Vec2,
    /// The way a building here would have faced: local +Z back across the
    /// pavement, local +X along the street.
    yaw: f32,
    /// Away from the street, in the ground plane.
    outward: Vec2,
    /// How wide it runs along the street.
    span: f32,
    /// What stands on the boundary, if anything.
    boundary: Option<Boundary>,
    /// How tall that is.
    height: f32,
}

/// What somebody put across the gap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Boundary {
    /// Rendered blockwork, the same grey the pavement is.
    Wall,
    /// Clipped, and the same leaves the parks are — see `world::vegetation`.
    Hedge,
}

/// Every gap in the town's frontage, filed by `EdgeId`.
///
/// A resource because it is decided in `generate_city` and spent in
/// `setup_ground`, which are two systems with a sync point between them; the
/// meshes it turns into live in [`Ribbons`] with everything else that is built
/// once and streamed many times.
#[derive(Resource, Default, Clone)]
pub struct Frontage {
    gaps: Vec<Vec<Gap>>,
}

impl Frontage {
    /// How many holes the marcher left, over the whole town. Log line and test
    /// hook; nothing in the game asks.
    pub fn len(&self) -> usize {
        self.gaps.iter().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.gaps.iter().all(Vec::is_empty)
    }
}

#[derive(Resource)]
pub struct StreetsideKit {
    /// One upright unit face: a metre wide, a metre tall, standing on `y = 0`
    /// and looking down its own local +Z.
    ///
    /// A kerb used to be a unit *cube* scaled to the length of its street, and
    /// a cube has square ends — which is exactly what could not be squared with
    /// a junction that is not square. It is two of these instead: the face the
    /// carriageway sees and the face the gardens see, each scaled to its own
    /// length, because a mitred pavement is longer along its back than along
    /// its kerb. Everything between them is under the walking surface and has
    /// never been seen from anywhere.
    ///
    /// Shared, and that is the point: four upright faces per street batch into
    /// one draw where four thousand bespoke prisms would not.
    face: Handle<Mesh>,
}

pub fn build_assets(meshes: &mut Assets<Mesh>) -> StreetsideKit {
    StreetsideKit {
        face: meshes.add(super::buildings::with_tangents(upright_face())),
    }
}

/// A unit quad standing on the ground, facing local +Z.
///
/// `Rectangle` would very nearly do, but it is centred on its own middle, and a
/// kerb is placed by the ground it stands on rather than by its waist. Half a
/// kerb height of offset in every transform is the sort of thing that is right
/// until the first time somebody scales one.
fn upright_face() -> Mesh {
    Mesh::new(
        bevy::render::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, 0.0, 0.0],
            [0.5, 0.0, 0.0],
            [0.5, 1.0, 0.0],
            [-0.5, 1.0, 0.0],
        ],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    )
    .with_inserted_indices(bevy::render::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]))
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
    /// The two pavements of each street, indexed by `EdgeId` then by side —
    /// 0 for the clockwise side of the run from `a` to `b`, 1 for the
    /// anticlockwise one. `None` where the street is too short to carry one
    /// once both its ends have been given back to the crossings.
    strips: Vec<[Option<Strip>; 2]>,
    slabs: Handle<StandardMaterial>,
    /// What fills the holes in the frontage, filed by `EdgeId`.
    ///
    /// One mesh each rather than one shared shape scaled to fit, for the reason
    /// the road ribbons are: a mesh carries its own size in its UVs, and a
    /// gravel yard sixteen metres wide drawn with a one-repeat quad is not
    /// gravel, it is a photograph of gravel stretched over a yard. The hedge
    /// version of that is worse — the repeat comes out eight times wider than
    /// it is tall, and through an alpha mask a hedge becomes a venetian blind.
    /// They are built at startup, in [`build_ribbons`], and never in the
    /// streaming path.
    courts: Vec<Vec<Court>>,
    court: Handle<StandardMaterial>,
    /// What a boundary is made of, built and planted.
    wall: Handle<StandardMaterial>,
    hedge: Handle<StandardMaterial>,
}

/// The meshes one hole in the frontage turns into.
struct Court {
    /// The forecourt quad, sized to the hole.
    ground: Handle<Mesh>,
    /// The box across the back of the pavement, sized and tiled for whichever
    /// kind of boundary it is. `None` where the gap is an opening.
    boundary: Option<Handle<Mesh>>,
    gap: Gap,
}

/// One pavement, as the two meshes it is drawn with and the box it is felt as.
///
/// The meshes are per street *and per side*, which they have to be: the two
/// pavements of one street are cut differently at both ends, because the
/// street they give way to is a different street on each side. A shared mesh
/// scaled to length — which is what the kerbs used to be — cannot express a
/// mitre, and the mitre is the whole of what was wrong.
#[derive(Clone)]
struct Strip {
    /// The walking surface, on a quad that carries its own size in its UVs and
    /// its own mitre at each end.
    ///
    /// Per street *and per side*, which it has to be: the two pavements of one
    /// street give way to a different street at each of their four ends, so no
    /// two of them are the same shape. It used to be one mesh per street,
    /// shared by both sides and squeezed along its length, which is why a
    /// mitre was not expressible.
    footway: Handle<Mesh>,
    /// The kerb face the carriageway sees: how far its middle sits along the
    /// street from the street's middle, and how long it is.
    kerb: (f32, f32),
    /// And the face the gardens see, which at a mitred corner is the longer of
    /// the two.
    back: (f32, f32),
    /// Where the collider goes, in the same terms.
    ///
    /// Deliberately the *conservative* box inside the mitred outline rather
    /// than the outline itself: a convex hull per pavement is nine thousand
    /// hulls, and what a box gives up is a wedge at an acute corner that
    /// nothing can reach without first driving over the kerb.
    body: (f32, f32),
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

/// Metres of pavement one repeat of the slabs covers.
const FOOTWAY_TILE: f32 = 1.45;

/// A flat quad `width` by `length`, lying in XZ, with UVs that tile a paving
/// of size `tile` at its true size.
fn ribbon(width: f32, length: f32, tile: f32) -> Mesh {
    let (hw, hl) = (width * 0.5, length * 0.5);
    let (u, v) = (width / tile, length / tile);
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

/// How shallow a crossing has to be before the mitre stops being believed.
///
/// Two kerb lines meeting at nothing intersect a hundred metres away, and the
/// pavement that would honour it is a spike down the middle of a street. Under
/// this the two are treated as one line: they butt at the node and whatever
/// width they do not share shows as a jog in the kerb, which is what a real
/// kerb does where a street widens.
const NEARLY_STRAIGHT: f32 = 0.035;

/// How far a mitre may run *past* its own node, against the two half-widths.
///
/// Only the negative side is bounded, and only because of one shape: a street
/// that doubles back on itself. Two arms ten degrees apart have outer kerbs
/// that are nearly parallel and meet a long way behind the node, so honouring
/// the mitre would run the pavement sixty metres up the other side of the
/// hairpin. A right-angled bend — the sharpest turn a street really makes —
/// wants half of this, so nothing that is a corner is affected.
///
/// The positive side is deliberately *not* capped. A fork at twenty degrees
/// genuinely does want a thirty-metre wedge of pavement, because that is what
/// the tip of the block between two diverging streets is, and capping it was
/// putting the pavement back in the road at exactly the corners this was
/// written to fix. Where the wedge will not fit, [`strips`] drops the pavement
/// rather than laying a short one across the crossing.
const MITRE_REACH: f32 = 1.0;

/// Where one arm's kerb line crosses the kerb line of the arm beside it,
/// measured along this arm from the node they share.
///
/// This is the whole of what was wrong with the pavements of a town read off a
/// map, so it is worth being precise about. Take two streets leaving a node,
/// `mine` and `theirs` half a carriageway wide each, with `gap` the angle
/// swept from one to the other on the side the pavement is on. Their kerb
/// lines — each parallel to its own street, each offset by its own half-width
/// — cross at
///
/// ```text
///     t = (theirs + mine · cos gap) / sin gap
/// ```
///
/// along mine. At a right angle that is `theirs`, which is the answer the old
/// code had: stop where the crossing carriageway starts. Everywhere else it is
/// not. At forty degrees it is nearly twice as far, and the difference is the
/// slab of pavement that used to be left lying in the road. Past a right angle
/// it goes *negative*, and that is right too: the outside of a bend has to run
/// past the node, or the corner shows a notch.
///
/// The same formula with both offsets pushed out by a pavement's width gives
/// where the *backs* of the two pavements cross. The segment between the two
/// crossings is the joint, and cutting both pavements along it is what makes
/// the band turn the corner without either half straying into the other's
/// street. Everything else here follows from that one line.
pub fn mitre(mine: f32, theirs: f32, gap: f32) -> f32 {
    let (sin, cos) = gap.sin_cos();
    if sin.abs() < NEARLY_STRAIGHT {
        // Straight on, or laid along itself. Butting at the node is right for
        // the first — a street that widens shows the change as a jog in the
        // kerb, which is what a real kerb does. The second is two ways of the
        // extract on top of each other, and the only honest answer is a number
        // no street is long enough to satisfy, so no pavement is laid at all.
        return if cos > 0.0 { UNBUILDABLE } else { 0.0 };
    }
    ((theirs + mine * cos) / sin).max(-(mine + theirs) * MITRE_REACH)
}

/// A mitre no street can honour. Whatever asks for it goes unpaved.
const UNBUILDABLE: f32 = 1.0e6;

/// One street leaving a junction: which way, how wide, and which edge it is.
struct Arm {
    bearing: f32,
    half: f32,
    edge: super::roadgraph::EdgeId,
}

/// The arms of every junction, sorted anticlockwise.
///
/// Built once. Every street asks both of its nodes about both of its sides, so
/// sorting where it is needed would sort every junction four times over.
fn fans(layout: &CityLayout) -> Vec<Vec<Arm>> {
    let graph = &layout.graph;
    graph
        .nodes()
        .map(|(id, node)| {
            let mut arms: Vec<Arm> = node
                .edges
                .iter()
                .filter_map(|&edge| {
                    let e = graph.edge(edge);
                    let far = if e.a == id { e.b } else { e.a };
                    let out = graph.node(far).pos - node.pos;
                    Dir2::new(out).ok().map(|d| Arm {
                        bearing: d.y.atan2(d.x),
                        half: e.width * 0.5,
                        edge,
                    })
                })
                .collect();
            arms.sort_by(|a, b| a.bearing.total_cmp(&b.bearing));
            arms
        })
        .collect()
}

/// How far along an arm, from its node, one of its pavements starts.
///
/// Returns the cut on the kerb line and the cut on the back of the pavement:
/// the two ends of the joint this pavement is mitred against. `left` picks the
/// side — anticlockwise of the direction the arm leaves the node, which is the
/// side its own `+normal` is on.
fn joint(fan: &[Arm], edge: super::roadgraph::EdgeId, left: bool) -> (f32, f32) {
    let Some(index) = fan.iter().position(|arm| arm.edge == edge) else {
        return (0.0, 0.0);
    };
    let mine = fan[index].half;
    if fan.len() < 2 {
        // A dead end has no neighbour to give way to. The pavement runs to the
        // node and stops square, which is the one place in the town a square
        // end is the right answer.
        return (0.0, 0.0);
    }
    let count = fan.len();
    let (other, gap) = if left {
        let next = &fan[(index + 1) % count];
        (next.half, wrap(next.bearing - fan[index].bearing))
    } else {
        let previous = &fan[(index + count - 1) % count];
        (previous.half, wrap(fan[index].bearing - previous.bearing))
    };
    (
        mitre(mine, other, gap),
        mitre(mine + SIDEWALK_WIDTH, other + SIDEWALK_WIDTH, gap),
    )
}

/// An angle brought into the turn anticlockwise from one arm to the next.
fn wrap(angle: f32) -> f32 {
    let turn = angle % std::f32::consts::TAU;
    if turn <= 0.0 {
        turn + std::f32::consts::TAU
    } else {
        turn
    }
}

/// The two pavements of one street, cut to the joints at both of its ends.
///
/// `None` where what is left after both crossings have been given their room
/// is not a pavement any more. A street with no pavement is a street with no
/// pavement; a street with a pavement lying across the crossing at the end of
/// it is what this whole module exists to stop.
fn strips(
    layout: &CityLayout,
    fans: &[Vec<Arm>],
    edge_id: super::roadgraph::EdgeId,
    meshes: &mut Assets<Mesh>,
) -> [Option<Strip>; 2] {
    let graph = &layout.graph;
    let edge = graph.edge(edge_id);
    let length = edge.length;
    let half = edge.width * 0.5;

    // At `a` the street leaves along `a -> b`, so the pavement on the world
    // `+normal` side is the anticlockwise one. At `b` it leaves the other way,
    // and the same pavement is on the *clockwise* side of that. Getting this
    // round the wrong way mitres every pavement against the street on the far
    // side of the road, which looks very nearly right and is wrong at every
    // junction that is not symmetrical.
    let mut out = [None, None];
    for (index, side) in [(0usize, -1.0f32), (1, 1.0)] {
        let left = side > 0.0;
        let (a_kerb, a_back) = joint(&fans[edge.a.0 as usize], edge_id, left);
        let (b_kerb, b_back) = joint(&fans[edge.b.0 as usize], edge_id, !left);

        // Both cuts have to leave something between them, on both lines.
        let kerb_run = length - a_kerb - b_kerb;
        let back_run = length - a_back - b_back;
        if kerb_run < 0.6 || back_run < 0.6 {
            continue;
        }

        let across = side * half;
        let back_across = side * (half + SIDEWALK_WIDTH);
        // The four corners, as (across, along) from the middle of the street.
        let corners = [
            Vec2::new(across, a_kerb - length * 0.5),
            Vec2::new(across, length * 0.5 - b_kerb),
            Vec2::new(back_across, length * 0.5 - b_back),
            Vec2::new(back_across, a_back - length * 0.5),
        ];
        // UVs run across the pavement and along the street, in true metres, so
        // a slab is a slab whatever the street does.
        let uvs = [
            Vec2::new(0.0, a_kerb),
            Vec2::new(0.0, length - b_kerb),
            Vec2::new(SIDEWALK_WIDTH, length - b_back),
            Vec2::new(SIDEWALK_WIDTH, a_back),
        ]
        .map(|uv| uv / FOOTWAY_TILE);

        // What a car may not drive through: the box that fits inside the
        // mitred outline, and never past either node, so two pavements meeting
        // at a corner cannot overlap. Overlapping static boxes are what took a
        // settled frame to three hundred milliseconds once already.
        let body_a = a_kerb.max(a_back).max(0.0) + KERB_BODY_INSET;
        let body_b = b_kerb.max(b_back).max(0.0) + KERB_BODY_INSET;
        let body_run = length - body_a - body_b;

        out[index] = Some(Strip {
            footway: meshes.add(super::buildings::with_tangents(plan(&corners, &uvs))),
            kerb: ((a_kerb - b_kerb) * 0.5, kerb_run),
            back: ((a_back - b_back) * 0.5, back_run),
            body: ((body_a - body_b) * 0.5, body_run.max(0.0)),
        });
    }
    out
}

/// How far short of the mitre the collider stops, at each end.
///
/// Butting two static boxes exactly is still a contact pair every tick. A hand's
/// width of daylight is not something anybody walks through and is the whole of
/// what stops the solver having an opinion about it.
const KERB_BODY_INSET: f32 = 0.25;

/// A flat convex polygon lying in XZ, given as `(across, along)` corners.
///
/// Wound from whatever order the corners arrive in rather than trusting it: a
/// mitred pavement can be cut either way round depending on which side of the
/// street it is, and a quad wound the wrong way is not dark or striped — it is
/// simply not there, with the ground showing through where the pavement was.
fn plan(corners: &[Vec2; 4], uvs: &[Vec2; 4]) -> Mesh {
    let positions: Vec<[f32; 3]> = corners.iter().map(|c| [c.x, 0.0, c.y]).collect();
    // Twice the signed area. Positive means the corners run one way round;
    // which way that is depends on the handedness of XZ, so it is measured
    // rather than assumed.
    let area: f32 = (0..4)
        .map(|i| {
            let (p, q) = (corners[i], corners[(i + 1) % 4]);
            p.x * q.y - q.x * p.y
        })
        .sum();
    let indices = if area < 0.0 {
        vec![0, 1, 2, 0, 2, 3]
    } else {
        vec![0, 2, 1, 0, 3, 2]
    };
    Mesh::new(
        bevy::render::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        uvs.iter().map(|uv| [uv.x, uv.y]).collect::<Vec<_>>(),
    )
    .with_inserted_indices(bevy::render::mesh::Indices::U32(indices))
}

/// Builds the carriageway of a whole town, once.
#[allow(clippy::too_many_arguments)]
pub fn build_ribbons(
    layout: &CityLayout,
    frontage: &Frontage,
    meshes: &mut Assets<Mesh>,
    paving: [Handle<super::road::RoadMaterial>; 4],
    slabs: Handle<StandardMaterial>,
    court: Handle<StandardMaterial>,
    wall: Handle<StandardMaterial>,
    hedge: Handle<StandardMaterial>,
) -> Ribbons {
    let fans = fans(layout);
    let strips = (0..layout.graph.edge_count())
        .map(|i| strips(layout, &fans, super::roadgraph::EdgeId(i as u32), meshes))
        .collect();
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
                TILE,
            )))
        })
        .collect();
    // One quad per hole, sized to the hole. Landshut leaves about three
    // thousand of them, which is the same order as the pavement strips this
    // function already builds and a twentieth of what a chunk spawns.
    let courts = (0..layout.graph.edge_count())
        .map(|i| {
            frontage
                .gaps
                .get(i)
                .map(|gaps| {
                    gaps.iter()
                        .map(|gap| Court {
                            ground: meshes.add(super::buildings::with_tangents(ribbon(
                                gap.span, FORECOURT, GRIT_TILE,
                            ))),
                            boundary: gap.boundary.map(|kind| {
                                meshes.add(boundary_box(
                                    Vec3::new(gap.span, gap.height, BOUNDARY_THICK),
                                    match kind {
                                        // Blockwork somebody rendered
                                        // themselves, and a hedge's own clumps,
                                        // are not the same size and never look
                                        // right at the same repeat.
                                        Boundary::Wall => 1.2,
                                        Boundary::Hedge => 0.45,
                                    },
                                ))
                            }),
                            gap: *gap,
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    Ribbons {
        roads,
        junction: meshes.add(super::buildings::with_tangents(ribbon(1.0, 1.0, TILE))),
        paving,
        strips,
        slabs,
        courts,
        court,
        wall,
        hedge,
    }
}

/// A box whose UVs are its own size in metres, divided by `tile`.
///
/// `Cuboid`'s own mesh gives every face one repeat, which is right for a cube
/// and wrong for everything else: stretched to eight metres by one, the repeat
/// comes out eight times wider than it is tall. Rewriting the UVs off the
/// positions costs nothing — the normal says which two axes the face lies in,
/// and there are twenty-four vertices.
fn boundary_box(size: Vec3, tile: f32) -> Mesh {
    let mut mesh = Cuboid::from_size(size).mesh().build();
    let positions: Vec<[f32; 3]> = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|values| values.as_float3())
        .map(|slice| slice.to_vec())
        .unwrap_or_default();
    let normals: Vec<[f32; 3]> = mesh
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .and_then(|values| values.as_float3())
        .map(|slice| slice.to_vec())
        .unwrap_or_default();
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .zip(normals.iter())
        .map(|(at, normal)| {
            let (u, v) = if normal[1].abs() > 0.5 {
                (at[0], at[2])
            } else if normal[0].abs() > 0.5 {
                (at[2], at[1])
            } else {
                (at[0], at[1])
            };
            [u / tile, v / tile]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    super::buildings::with_tangents(mesh)
}

/// Metres of yard one repeat of the grit covers. The same number the gravelled
/// carriageways use, so a forecourt and a gravel lane are the same gravel.
const GRIT_TILE: f32 = super::GRIT_TILE;

/// The lowest a carriageway is laid, in metres above the ground, and how much
/// height the widest street in the town is allowed to claim over the narrowest.
const ROAD_BED: f32 = 0.008;
const ROAD_RANK: f32 = 0.006;
/// The width, in metres, at which a street has claimed all of it.
const WIDEST_STREET: f32 = 18.0;

/// How high one street's carriageway is laid.
///
/// Every ribbon used to sit at the same twelve millimetres, and every junction
/// in a real town is two or three of them overlapping — a rectangle each, at
/// arbitrary angles, all coplanar. What that gives is z-fighting where they
/// cross and, where the depth test happens to settle, a hard straight edge of
/// one street's surface cut across another's.
///
/// So width decides. The wider street's carriageway is laid over the narrower
/// one's, which is not a trick to break the tie — it is what a resurfacing gang
/// actually does: the main road runs through and the side road stops at it. The
/// last fraction of a millimetre is the edge's own index, so two streets of
/// exactly the same width still cannot fight.
fn carriageway_height(width: f32, id: super::roadgraph::EdgeId) -> f32 {
    let rank = (width / WIDEST_STREET).clamp(0.0, 1.0);
    ROAD_BED + rank * ROAD_RANK + (id.0 % 8) as f32 * 0.00005
}

/// Lays the two pavements of one street.
///
/// A strip either side rather than a slab round a block, because a block on a
/// real map is not a rectangle. Where each of the four ends stops was decided
/// once, at startup, by [`mitre`]: a pavement is cut along the joint it shares
/// with the pavement of the street beside it, so the band turns every corner in
/// the town without either half straying into the other's carriageway.
///
/// Three draws a side. The walking surface is a mesh of its own because it is
/// the one shape here that is not a rectangle; the two upright kerb faces are
/// the shared unit face scaled, so however many streets are resident they cost
/// one batch between them.
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
            Transform::from_xyz(middle.x, carriageway_height(edge.width, id), middle.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
        ));
    }
    let Some(strips) = ribbons.strips.get(id.0 as usize) else {
        return;
    };
    let visibility = bevy::camera::visibility::VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: range..(range * 1.05),
        use_aabb: false,
    };

    let half = edge.width * 0.5;
    for (index, side) in [(0usize, -1.0f32), (1, 1.0)] {
        let Some(strip) = strips[index].as_ref() else {
            continue;
        };

        // The walking surface. Its mesh is already cut to shape and already
        // sits in the street's own frame, so all it wants is the middle of the
        // street and the way the street runs.
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(strip.footway.clone()),
            MeshMaterial3d(ribbons.slabs.clone()),
            // A few millimetres proud of the kerb, which settles the depth test
            // without being visible from standing height.
            Transform::from_xyz(middle.x, SIDEWALK_HEIGHT + 0.004, middle.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            visibility.clone(),
        ));

        // The two upright faces. The one the carriageway sees looks back across
        // the road; the one behind looks out at whatever the pavement was cut
        // out of. Nothing between them has ever been seen: the walking surface
        // is the lid and the ground is the floor.
        for (offset, run, outward) in [
            (side * half, strip.kerb, -side),
            (side * (half + SIDEWALK_WIDTH), strip.back, side),
        ] {
            if run.1 < 0.4 {
                continue;
            }
            let at = middle + *direction * run.0 + normal * offset;
            // A face looks along its own local +Z. Turned to look across the
            // pavement, its local +X — the axis the length is scaled on — ends
            // up along the street, which is what the scale below assumes.
            let facing = normal * outward;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.face.clone()),
                MeshMaterial3d(kerb.clone()),
                Transform::from_xyz(at.x, 0.0, at.y)
                    .with_rotation(Quat::from_rotation_y(facing.x.atan2(facing.y)))
                    .with_scale(Vec3::new(run.1, SIDEWALK_HEIGHT, 1.0)),
                visibility.clone(),
            ));
        }

        // And the step the player climbs. Inside the mitred outline rather than
        // on it — see [`Strip::body`].
        if strip.body.1 < 1.0 {
            continue;
        }
        let at =
            middle + *direction * strip.body.0 + normal * (side * (half + SIDEWALK_WIDTH * 0.5));
        commands.spawn((
            ChunkOf(chunk),
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT * 0.5, at.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            avian3d::prelude::RigidBody::Static,
            avian3d::prelude::Collider::cuboid(SIDEWALK_WIDTH, SIDEWALK_HEIGHT, strip.body.1),
        ));
    }

    fill_gaps(commands, ribbons, id, chunk, &visibility);
}

/// How high a forecourt is laid.
///
/// *Under* the road bed, not over it. A gap is held nine metres clear of a
/// crossing but only half a metre clear of a bend, and a ribbon runs half its
/// own width past every node it ends at — so a forecourt beside a kink in a
/// street can find itself under the neighbouring carriageway. Laid lower, the
/// carriageway simply wins the depth test and the forecourt is not there;
/// laid higher, the two would fight, and a shimmering rectangle of gravel over
/// a road is a great deal more obvious than a missing yard.
const COURT_HEIGHT: f32 = 0.004;

/// Fills the holes the marcher left in one street's frontage.
///
/// A gravelled forecourt, and across the back of the pavement a wall, a hedge
/// or nothing. Everything here is a mesh built at startup — the forecourt quads
/// in [`build_ribbons`], the wall and the hedge as one shared unit cube each —
/// so however many times a chunk comes back, nothing is added to
/// `Assets<Mesh>`.
fn fill_gaps(
    commands: &mut Commands,
    ribbons: &Ribbons,
    id: super::roadgraph::EdgeId,
    chunk: IVec2,
    visibility: &bevy::camera::visibility::VisibilityRange,
) {
    let Some(courts) = ribbons.courts.get(id.0 as usize) else {
        return;
    };
    for court in courts {
        let gap = &court.gap;
        let turn = Quat::from_rotation_y(gap.yaw);
        // The quad runs back from the building line, away from the street. Its
        // local +Z faces the street, so half its depth *against* +Z is the
        // middle of it.
        let middle = gap.centre + gap.outward * (FORECOURT * 0.5);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(court.ground.clone()),
            MeshMaterial3d(ribbons.court.clone()),
            Transform::from_xyz(middle.x, COURT_HEIGHT, middle.y).with_rotation(turn),
            visibility.clone(),
        ));

        let (Some(mesh), Some(kind)) = (court.boundary.as_ref(), gap.boundary) else {
            continue;
        };
        let material = match kind {
            Boundary::Wall => &ribbons.wall,
            Boundary::Hedge => &ribbons.hedge,
        };
        // On the line itself, so it reads as the back of the pavement rather
        // than as a fence somebody put up in a field. The box is already the
        // right size and is centred on its own middle in all three axes, hence
        // the half-height and the half thickness here and no scale at all on
        // the transform — which also keeps Avian out of the argument about
        // whether a collider follows one.
        let at = gap.centre + gap.outward * (BOUNDARY_THICK * 0.5);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(at.x, gap.height * 0.5, at.y).with_rotation(turn),
            // Solid, because a wall you walk through is worse than no wall.
            avian3d::prelude::RigidBody::Static,
            avian3d::prelude::Collider::cuboid(gap.span, gap.height, BOUNDARY_THICK),
            visibility.clone(),
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
        // Under every arm's own ribbon, so what shows in the middle of a
        // junction is the ribbons themselves and this is only what fills the
        // diamond none of them covers.
        Transform::from_xyz(at.x, ROAD_BED - 0.002, at.y).with_scale(Vec3::new(
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
pub fn lots(layout: &CityLayout, seed: u64, style: CityStyle) -> (Vec<Block>, Frontage) {
    let mut rng = crate::core::rng::stream_for(seed, crate::core::rng::stream::BUILDINGS);
    // A second stream, and it has to be second. What goes across a gap is
    // decided *inside* the walk down each street, so drawing it from the
    // marcher's own stream would shift every frontage after it — the exact
    // failure `core::rng` opens with, and it would rebuild the whole town from
    // an unchanged seed.
    let mut yards = crate::core::rng::stream_for(seed, crate::core::rng::stream::YARDS);
    let mut blocks = Vec::new();
    // Named for what it holds rather than for the type, because a few lines
    // down `frontage` is already the width of one plot.
    let mut holes = Frontage {
        gaps: vec![Vec::new(); layout.graph.edge_count()],
    };
    // Middles of everything placed so far, bucketed by cell. A candidate only
    // looks at its own cell and the eight around it.
    let mut taken: HashMap<(i32, i32), Vec<(Vec2, f32)>> = HashMap::default();
    let roads = corridors(layout);
    let scale = style.lot_scale();

    for (index, edge) in layout.graph.edges().enumerate() {
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
                    let span = rng.random_range(HOLE_WIDTH.0..HOLE_WIDTH.1);
                    // The hole is a place, not a skipped iteration. Recorded
                    // here and surfaced by `spawn_edge`, because a gap in a
                    // real terrace is a gate, a yard or a hardstanding and the
                    // one thing it is never is mown meadow abutting a kerb.
                    let centre = a + *direction * (along + span * 0.5) + normal * (side * line);
                    let gap = Gap {
                        centre,
                        yaw,
                        outward: normal * side,
                        span,
                        boundary: if yards.random_range(0.0..1.0) < OPENING {
                            None
                        } else if yards.random_range(0.0..1.0) < 0.45 {
                            Some(Boundary::Hedge)
                        } else {
                            Some(Boundary::Wall)
                        },
                        height: yards.random_range(BOUNDARY_HEIGHT.0..BOUNDARY_HEIGHT.1),
                    };
                    along += span;
                    // Wide enough to be a yard, and not standing in a street
                    // that happens to run behind this one. The corridor test is
                    // the same one a building takes, for the same reason: a
                    // plot at a real town's angles sits at some arbitrary angle
                    // to every road but its own.
                    let yard = Oblong {
                        centre: centre + gap.outward * (FORECOURT * 0.5),
                        axis: *direction,
                        half: Vec2::new(span, FORECOURT) * 0.5,
                    };
                    if span >= FORECOURT_MIN && !in_a_road(&roads, &yard) {
                        holes.gaps[index].push(gap);
                    }
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
                // And is it standing in a road? See [`in_a_road`] — not the
                // street it faces, which it is placed against on purpose, but
                // any other one that happens to run behind or beside it.
                let plot = Oblong {
                    centre,
                    axis: *direction,
                    half: Vec2::new(frontage, depth) * 0.5,
                };
                if in_a_road(&roads, &plot) {
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

                // And what is behind it.
                //
                // A town built only along its frontages is a town with a hole
                // in the middle of every block, and the holes are enormous: the
                // gap between two streets in Landshut is about a hundred and
                // fifty metres and a plot is twenty deep, so four fifths of the
                // ground inside a block had nothing on it at all. The shading
                // makes it read as a yard rather than as a meadow now, but a
                // yard a hundred metres across with nothing standing on it is
                // still not a town — it is a car park nobody painted.
                //
                // What is actually back there is the back of the town: a
                // workshop, a coach house, a garage block, a lock-up, an
                // extension somebody built in the sixties. Low, small, turned
                // to the same street as the house in front, and placed by
                // exactly the machinery the frontage is — the same clash grid
                // and the same corridor test — so a back building can no more
                // stand in a road than a front one can.
                let mut back = line + depth + yards.random_range(BACKYARD.0..BACKYARD.1);
                for _ in 0..OUTBUILDINGS {
                    if yards.random_range(0.0..1.0) > BACK_BUILT {
                        break;
                    }
                    let across = frontage * yards.random_range(BACK_SPAN.0..BACK_SPAN.1);
                    let deep = yards.random_range(BACK_DEPTH.0..BACK_DEPTH.1);
                    let at = a
                        + *direction
                            * (along - frontage * 0.5
                                + across * 0.5
                                + yards.random_range(-2.0..2.0))
                        + normal * (side * (back + deep * 0.5));
                    let reach = Vec2::new(across, deep).length() * 0.5;
                    let cell = ((at.x / CELL).floor() as i32, (at.y / CELL).floor() as i32);
                    let clash = (-1..=1).any(|dx| {
                        (-1..=1).any(|dz| {
                            taken
                                .get(&(cell.0 + dx, cell.1 + dz))
                                .is_some_and(|others| {
                                    others.iter().any(|(other, other_radius)| {
                                        at.distance(*other) < (reach + other_radius) * CLEARANCE
                                    })
                                })
                        })
                    });
                    let shape = Oblong {
                        centre: at,
                        axis: *direction,
                        half: Vec2::new(across, deep) * 0.5,
                    };
                    back += deep + yards.random_range(BACKYARD.0..BACKYARD.1);
                    if clash || in_a_road(&roads, &shape) {
                        continue;
                    }
                    taken.entry(cell).or_default().push((at, reach));

                    let half = Vec2::new(across, deep) * 0.5;
                    blocks.push(Block {
                        area: Rect::new(at - Vec2::splat(reach), at + Vec2::splat(reach)),
                        paved: false,
                        district,
                        buildings: vec![Building {
                            footprint: Rect::new(at - half, at + half),
                            facing: Some(yaw),
                            // One or two storeys, whatever the street in front
                            // is: this is a shed, not a second house.
                            height: yards.random_range(BACK_HEIGHT.0..BACK_HEIGHT.1),
                            palette: yards.random_range(0..PALETTE_SIZE),
                            kind: BuildingKind::Apartments,
                        }],
                        vacants: Vec::new(),
                        arterial: [false; 4],
                        quarter: None,
                    });
                }
            }
        }
    }

    (blocks, holes)
}

/// Is this rectangle standing in a street?
///
/// Not the one it faces — that one it is placed against on purpose — but any
/// *other* street that happens to run behind or beside it. An OSM town is full
/// of them: a lane ending a few metres off a main road, two streets meeting at
/// thirty degrees, a footway threading a block. The frontage of a plot squares
/// up to its own street and therefore sits at some arbitrary angle to every
/// other one, which is why this is a rotated-rectangle test and not a box
/// overlap.
fn in_a_road(roads: &HashMap<(i32, i32), Vec<Oblong>>, shape: &Oblong) -> bool {
    let cell = (
        (shape.centre.x / CELL).floor() as i32,
        (shape.centre.y / CELL).floor() as i32,
    );
    (-1..=1).any(|dx| {
        (-1..=1).any(|dz| {
            roads.get(&(cell.0 + dx, cell.1 + dz)).is_some_and(|near| {
                near.iter()
                    .any(|road| shape.clashes_with(road, IN_THE_ROAD))
            })
        })
    })
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

    /// Where two kerbs cross, at the one angle everybody agrees about.
    #[test]
    fn a_square_crossing_stops_the_pavement_at_the_kerb_it_meets() {
        // Two streets at a right angle: this pavement stops exactly where the
        // crossing carriageway starts, which is half its width out from the
        // node — whatever this street's own width happens to be.
        for mine in [3.0f32, 4.5, 7.5] {
            for theirs in [3.0f32, 4.5, 7.5] {
                let t = mitre(mine, theirs, std::f32::consts::FRAC_PI_2);
                assert!(
                    (t - theirs).abs() < 1e-4,
                    "a {mine}m/{theirs}m square corner mitred at {t}m"
                );
            }
        }
    }

    /// The half a grid never needed. Two streets meeting square give up half a
    /// carriageway; two meeting at forty degrees give up a good deal more,
    /// because "where the crossing carriageway starts", measured along *this*
    /// street, is further the shallower the crossing is. A town read off a map
    /// is mostly the second case, and this is the slab of pavement that used to
    /// be left lying in the road at every oblique corner in Landshut.
    #[test]
    fn an_oblique_corner_is_given_more_room_than_a_square_one() {
        let square = mitre(4.5, 4.5, std::f32::consts::FRAC_PI_2);
        let oblique = mitre(4.5, 4.5, (40.0f32).to_radians());
        assert!(
            oblique > square * 1.4,
            "a forty-degree corner mitred at {oblique}m against {square}m square"
        );
        // And a fork that is barely a fork asks for more pavement than any
        // street in the town is long, which is how `strips` knows to lay none.
        let grazing = mitre(4.5, 4.5, (2.0f32).to_radians());
        assert!(grazing > 200.0, "a grazing fork asked for only {grazing}m");
    }

    /// Past a right angle the mitre goes negative, and it has to.
    #[test]
    fn the_outside_of_a_bend_runs_past_its_node() {
        // A kink in one street: nearly straight on, turning a few degrees. The
        // pavement on the outside of the turn has to run *past* the node or the
        // bend shows a notch of bare road between two strips.
        let outside = mitre(4.5, 4.5, std::f32::consts::PI + 0.25);
        assert!(
            outside < 0.0,
            "the outside of a bend stopped {outside}m short"
        );
        // And the inside of the same bend stops just short of it.
        let inside = mitre(4.5, 4.5, std::f32::consts::PI - 0.25);
        assert!(
            inside > 0.0 && inside < 1.0,
            "the inside of a bend was trimmed {inside}m"
        );
    }

    /// The two halves of a joint meet, whatever the angle.
    ///
    /// This is the property the whole module rests on: the point where this
    /// pavement's kerb line is cut is the same point where the next one's is,
    /// so the band turns the corner with no overlap and no hole. Checked as
    /// geometry rather than as arithmetic — two arms are built, both mitres are
    /// taken, and the two answers have to land on the same spot.
    #[test]
    fn two_pavements_are_cut_at_the_same_point() {
        for degrees in [25.0f32, 55.0, 90.0, 120.0, 155.0] {
            let gap = degrees.to_radians();
            let (mine, theirs) = (4.5f32, 6.0f32);
            let a = Vec2::X;
            let b = Vec2::new(gap.cos(), gap.sin());
            // Mine is cut `t` along itself, offset onto its anticlockwise side;
            // theirs is cut `s` along itself, offset onto its clockwise side.
            let t = mitre(mine, theirs, gap);
            let s = mitre(theirs, mine, gap);
            let from_mine = a * t + Vec2::new(-a.y, a.x) * mine;
            let from_theirs = b * s + Vec2::new(b.y, -b.x) * theirs;
            assert!(
                from_mine.distance(from_theirs) < 1e-3,
                "at {degrees}° the two halves of the joint are at {from_mine:?} and {from_theirs:?}"
            );
        }
    }

    /// Nothing that is cut to a joint is left standing in the crossing street.
    ///
    /// The failure this replaces was not the kerb line — that one the old trim
    /// got right at a right angle — it was the *back* corner of the pavement,
    /// which at an acute corner swings a pavement's width further into the
    /// street the pavement is giving way to. Both corners are checked here, on
    /// both lines, at every angle a real junction comes in at.
    #[test]
    fn no_corner_of_a_pavement_lies_in_the_street_it_gives_way_to() {
        for degrees in [20.0f32, 35.0, 50.0, 70.0, 90.0, 110.0, 140.0, 170.0] {
            let gap = degrees.to_radians();
            let (mine, theirs) = (4.5f32, 6.0f32);
            let a = Vec2::X;
            let b = Vec2::new(gap.cos(), gap.sin());
            // The crossing street's carriageway: everything within `theirs` of
            // its centre line. Its near edge, seen from this side, is at
            // `+theirs` along the clockwise normal.
            let across = Vec2::new(b.y, -b.x);
            let kerb = mitre(mine, theirs, gap);
            let back = mitre(mine + SIDEWALK_WIDTH, theirs + SIDEWALK_WIDTH, gap);
            let normal = Vec2::new(-a.y, a.x);
            for (along, offset) in [(kerb, mine), (back, mine + SIDEWALK_WIDTH)] {
                let corner = a * along + normal * offset;
                assert!(
                    corner.dot(across) >= theirs - 1e-3,
                    "at {degrees}° a pavement corner sits {:.2}m inside a carriageway that starts at {theirs}m",
                    corner.dot(across)
                );
            }
        }
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
        let mesh = ribbon(8.0, 40.0, TILE);
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
        let (blocks, holes) = lots(&layout, 1, CityStyle::Landshuepf);
        assert!(!blocks.is_empty(), "nothing was built at all");
        assert!(!holes.is_empty(), "a town with no gaps in its frontage");

        let roads = corridors(&layout);
        // The forecourts laid across those gaps are held to exactly the same
        // rule the houses are: a yard in the carriageway is worse than a green
        // wedge, because a green wedge does not have a kerb through it.
        for gaps in &holes.gaps {
            for gap in gaps {
                let yard = Oblong {
                    centre: gap.centre + gap.outward * (FORECOURT * 0.5),
                    // `yaw` turns +Z back across the pavement, so local +X is
                    // the way the street runs.
                    axis: Vec2::new(gap.yaw.cos(), -gap.yaw.sin()),
                    half: Vec2::new(gap.span, FORECOURT) * 0.5,
                };
                for near in roads.values() {
                    for road in near {
                        assert!(
                            !yard.clashes_with(road, IN_THE_ROAD),
                            "a forecourt at {} is standing in the road",
                            yard.centre
                        );
                    }
                }
                assert!(gap.span >= FORECOURT_MIN, "a yard narrower than a passage");
            }
        }

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
