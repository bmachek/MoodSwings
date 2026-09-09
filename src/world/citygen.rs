//! Procedural city layout.
//!
//! A perturbed axis-aligned grid rather than L-systems or tensor fields. Curved
//! organic road networks look better in screenshots but produce a messy lane
//! graph, and the lane graph is what traffic, pursuit and the minimap all
//! depend on. An irregular grid keeps every block a rectangle — which makes
//! footprints, sidewalks and colliders trivial — while jittered spacing and
//! per-district massing keep it from reading as graph paper.
//!
//! Generation is pure and deterministic: same seed, same city, every run.

use bevy::math::Vec2;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::roadgraph::RoadGraph;
use crate::core::config::CityStyle;
use crate::core::rng::{key_for, stream, stream_for};

const ARTERIAL_WIDTH: f32 = 17.0;
const MINOR_WIDTH: f32 = 9.5;
/// Pavement between the kerb and the buildable area.
pub const SIDEWALK_WIDTH: f32 = 3.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum District {
    Downtown,
    Midtown,
    Residential,
    Industrial,
    Park,
}

impl District {
    /// (min height, max height) in metres.
    ///
    /// Public because a town read off a map has no districts of its own: it
    /// works out which district a frontage behaves like and then wants the same
    /// heights the generator would have given it.
    pub fn height_range(self) -> (f32, f32) {
        match self {
            District::Downtown => (38.0, 135.0),
            District::Midtown => (16.0, 46.0),
            District::Residential => (6.5, 15.0),
            District::Industrial => (5.5, 13.0),
            District::Park => (0.0, 0.0),
        }
    }

    /// Smallest lot side before subdivision stops. Bigger = fewer, bulkier
    /// buildings, which is what reads as downtown.
    fn min_lot(self) -> f32 {
        match self {
            District::Downtown => 25.0,
            District::Midtown => 18.0,
            District::Residential => 11.0,
            District::Industrial => 23.0,
            District::Park => f32::MAX,
        }
    }

    /// Chance a lot is left empty (car park, yard, vacant plot).
    fn vacancy(self) -> f32 {
        match self {
            District::Downtown => 0.05,
            District::Midtown => 0.08,
            District::Residential => 0.10,
            District::Industrial => 0.18,
            District::Park => 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }
    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }
    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }
    pub fn inset(&self, d: f32) -> Self {
        Self {
            min: self.min + Vec2::splat(d),
            max: self.max - Vec2::splat(d),
        }
    }
    pub fn is_valid(&self) -> bool {
        self.max.x > self.min.x && self.max.y > self.min.y
    }
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.min.x < other.max.x
            && other.min.x < self.max.x
            && self.min.y < other.max.y
            && other.min.y < self.max.y
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Street {
    /// Centreline position on the perpendicular axis.
    pub center: f32,
    pub width: f32,
    pub arterial: bool,
}

/// What a building is for.
///
/// Two regimes share the enum. The common kinds are a pure function of
/// (seed, footprint) — see [`common_kind`] — so assigning them draws nothing
/// from any stream and cannot move a single lot, height or palette that
/// `stream::BUILDINGS` already decided. The civic kinds are stamped on by
/// [`zone_civics`], a pass over the *finished* layout with its own
/// `stream::ZONING`, for the same reason: a new civic building must never
/// reshuffle the city around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum BuildingKind {
    Apartments,
    Offices,
    Supermarket,
    Restaurant,
    Hotel,
    // Civic, from here on: one-offs the zoning pass places.
    TownHall,
    FireStation,
    /// A city with no police still has the building. The sign explains:
    /// „wegen anhaltender Freundlichkeit geschlossen".
    PoliceStation,
    Barracks,
    ParkingGarage,
    /// Free entry, uncertain exit. Sealed for now; its jokes hang outside.
    Museum,
    /// Where the city's children learn to hop quietly.
    School,
    /// The one building that is not a box: `world::church` raises a nave,
    /// a tower and a spire where the stamped building's box would stand.
    Church,
    /// The bowl on the edge of town: pitch, stands, floodlights, and a
    /// crowd doing the wave — `world::stadium` owns the whole structure.
    Stadium,
    /// The church, but the one the postcards are of: a style that claims a
    /// cathedral gets exactly one, with a tower the city cannot justify.
    Cathedral,
}

impl BuildingKind {
    /// Whether the player can walk in through the front door.
    ///
    /// The ground floors that earn an interior: the shop, the dining room,
    /// the two kinds of lobby. Civic buildings keep their doors shut for now
    /// (the town hall is a venue on the events roadmap), and apartments stay
    /// private — this is a comedy, not a burglary simulator.
    pub fn enterable(self) -> bool {
        matches!(
            self,
            BuildingKind::Supermarket
                | BuildingKind::Restaurant
                | BuildingKind::Hotel
                | BuildingKind::Offices
        )
    }

    /// Placed by the zoning pass rather than the common draw.
    pub fn is_civic(self) -> bool {
        matches!(
            self,
            BuildingKind::TownHall
                | BuildingKind::FireStation
                | BuildingKind::PoliceStation
                | BuildingKind::Barracks
                | BuildingKind::ParkingGarage
                | BuildingKind::Museum
                | BuildingKind::School
                | BuildingKind::Church
                | BuildingKind::Stadium
                | BuildingKind::Cathedral
        )
    }
}

/// What a lot the vacancy roll left empty is used for.
///
/// Vacant lots used to be nothing at all — the roll simply dropped them, and
/// the city was pocked with bare paving. The roll is unchanged (it draws from
/// `stream::BUILDINGS`, and moving it would reshuffle every lot downstream);
/// what changed is that the lot is now *recorded*, with a purpose derived from
/// its own footprint hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VacantUse {
    ParkingLot,
    /// Only on lots fronting an arterial: a filling station with no traffic
    /// past its door is a lemonade stand.
    GasStation,
    /// A basketball court or kickabout cage — the street-sports venue.
    Court,
    /// A row of market stalls: canopies, counters, and crates that a car
    /// can send everywhere, which is the whole reason to drive there.
    Market,
    /// Genuinely nothing. A city needs a few.
    Yard,
}

#[derive(Debug, Clone, Copy)]
pub struct VacantLot {
    pub rect: Rect,
    pub purpose: VacantUse,
}

/// A cultural quarter, laid over the radial districts as a second axis of
/// identity. A district says how tall and how dense; a quarter says whose
/// kitchen the street smells of. Two wedges of the middle ring carry one
/// each, placed by the seed via [`quarter_for`] — sampled, never drawn, so
/// a quarter can move between seeds but never moves a lot within one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quarter {
    /// Klein-Neapel: terracotta walls, pizzerias, a mandolin on the air.
    Italia,
    /// The Fernost-Viertel: red and gold, woks, a guzheng over the street.
    Fernost,
}

/// Which quarter a point of the city belongs to, if any.
///
/// The two wedges sit in the middle ring (inside it downtown is too proud
/// to be themed, outside it the industrial belt would waste the paint),
/// roughly opposite each other so one walk cannot cross both.
pub fn quarter_for(seed: u64, center: Vec2) -> Option<Quarter> {
    use std::f32::consts::{PI, TAU};
    let r = center.length();
    if !(230.0..=720.0).contains(&r) {
        return None;
    }
    let key = key_for(seed, stream::QUARTERS);
    let first = (key & 0xFFFF) as f32 / 65536.0 * TAU;
    let second = first + PI * (0.75 + ((key >> 16) & 0xFF) as f32 / 256.0 * 0.5);
    let angle = center.y.atan2(center.x);
    let within = |wedge: f32| {
        let mut apart = (angle - wedge).rem_euclid(TAU);
        if apart > PI {
            apart = TAU - apart;
        }
        // A 48° wedge: wide enough to walk through, narrow enough that the
        // city stays mostly itself.
        apart < 0.42
    };
    if within(first) {
        Some(Quarter::Italia)
    } else if within(second) {
        Some(Quarter::Fernost)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Building {
    pub footprint: Rect,
    /// The way this building faces, if it was placed along a street rather than
    /// inside a block.
    ///
    /// `None` is the generator's own answer: a building in a rectangular block
    /// fronts whichever of its four sides is nearest the block's perimeter, and
    /// `buildings::site_in` works that out. `Some(yaw)` is a real town read off
    /// `world::atlas`, where there are no blocks and no four sides to choose
    /// between — the building faces the street it was placed along, and the
    /// footprint is read as `frontage x depth` in that street's own frame
    /// rather than as a rectangle on the map.
    pub facing: Option<f32>,
    pub height: f32,
    /// Index into the district's material palette.
    pub palette: u8,
    pub kind: BuildingKind,
}

#[derive(Debug, Clone)]
pub struct Block {
    /// Kerb-to-kerb extent, sidewalk included.
    pub area: Rect,
    /// Whether this block lays its own kerb slab.
    ///
    /// True for a generated block, whose rectangle *is* the pavement — the
    /// ground is asphalt and the slab carves the street grid out of it as
    /// negative space. False for the one-building blocks a real town is filled
    /// with, where the pavement is a strip laid along each street by
    /// `world::streetside` instead, because an axis-aligned slab round a
    /// building on a curved street is a paving stone at the wrong angle.
    pub paved: bool,
    pub district: District,
    pub buildings: Vec<Building>,
    /// Lots the vacancy roll left empty, now put to use.
    pub vacants: Vec<VacantLot>,
    /// Whether each bounding street is an arterial: -x, +x, -z, +z.
    pub arterial: [bool; 4],
    /// The cultural quarter this block sits in, if any.
    pub quarter: Option<Quarter>,
}

#[derive(Debug, Clone)]
pub struct CityLayout {
    pub seed: u64,
    pub half_extent: f32,
    /// Streets running along Z, indexed by their X centreline.
    pub x_streets: Vec<Street>,
    /// Streets running along X, indexed by their Z centreline.
    pub z_streets: Vec<Street>,
    pub blocks: Vec<Block>,
    pub graph: RoadGraph,
    /// The one street surrendered to water, if the grid had a spare.
    pub canal: Option<Canal>,
    /// Ground a real town does not build on: its parks, its pitches, its
    /// allotments, its cemetery. Empty for the generator, which decides all of
    /// that from its own districts.
    pub grounds: Vec<OpenGround>,
    /// Real water, read off a map: a river with arms, a mill race, a stream.
    ///
    /// Beside `canal` rather than instead of it, because the two are different
    /// things and the comment that used to sit on Landshut's `canal: None` said
    /// so — a canal here is one street of a grid surrendered to water, straight
    /// and axis-aligned, and the Isar is a braided river that goes where it
    /// goes. The generator keeps its canal; a town read off a map gets these.
    pub waters: Vec<Waterway>,
}

/// A piece of ground the map says is not built on.
#[derive(Debug, Clone)]
pub struct OpenGround {
    pub kind: super::atlas::GroundKind,
    /// The outline, anticlockwise or clockwise — the point-in-polygon test
    /// this feeds does not care which.
    pub points: Vec<Vec2>,
    /// Its bounding box, so the chunk index can file it without walking the
    /// ring again.
    pub bounds: Rect,
}

/// One arm of a river, as a polyline with a width.
#[derive(Debug, Clone)]
pub struct Waterway {
    pub name: String,
    pub width: f32,
    pub points: Vec<Vec2>,
}

impl CityLayout {
    pub fn building_count(&self) -> usize {
        self.blocks.iter().map(|b| b.buildings.len()).sum()
    }

    /// Order-independent digest, for asserting a seed reproduces a city.
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: f32| {
            // Quantise so bit-level float noise cannot flip the digest.
            let q = (v * 64.0).round() as i64 as u64;
            h ^= q;
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        for s in self.x_streets.iter().chain(self.z_streets.iter()) {
            mix(s.center);
            mix(s.width);
        }
        for b in &self.blocks {
            mix(b.area.min.x);
            mix(b.area.min.y);
            for building in &b.buildings {
                mix(building.footprint.min.x);
                mix(building.footprint.min.y);
                mix(building.height);
                // Deliberately in the digest: a zoning change is a different
                // city, and the determinism tests should say so.
                mix(building.kind as u8 as f32);
            }
            for vacant in &b.vacants {
                mix(vacant.rect.min.x);
                mix(vacant.purpose as u8 as f32);
            }
            // A quarter moving is a different city the same way a zoning
            // change is.
            mix(match b.quarter {
                None => 0.0,
                Some(Quarter::Italia) => 1.0,
                Some(Quarter::Fernost) => 2.0,
            });
        }
        h
    }
}

pub fn generate(seed: u64, half_extent: f32, style: CityStyle) -> CityLayout {
    let mut road_rng = stream_for(seed, stream::ROADS);
    let x_streets = streets(&mut road_rng, half_extent);
    let z_streets = streets(&mut road_rng, half_extent);

    let canal = canal_for(seed, &x_streets, &z_streets);
    let graph = build_graph(&x_streets, &z_streets, canal);
    let mut blocks = build_blocks(seed, &x_streets, &z_streets, style);
    zone_civics(seed, &mut blocks, style);

    CityLayout {
        grounds: Vec::new(),
        waters: Vec::new(),
        seed,
        half_extent,
        x_streets,
        z_streets,
        blocks,
        graph,
        canal,
    }
}

/// The canal: one minor street of the grid surrendered to water.
///
/// The elegant part is what it does *not* touch. The street's line stays in
/// the layout, so every block keeps its shape and every lot its hash — but
/// the road graph gets no edges *along* it, so traffic, parking, props,
/// markings and lamps all leave it alone without knowing why. The water is
/// cut a little wider than the carriageway, which drowns the bottom of the
/// neighbouring kerb slabs and turns them into quay walls for free. Every
/// crossing street keeps its edges and becomes a bridge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Canal {
    /// Which axis' street list the canal came from, and its index there:
    /// `true` means an `x_streets` entry (water running along Z).
    pub along_z: bool,
    pub index: usize,
    /// Centreline on the perpendicular axis, and the water's width.
    pub center: f32,
    pub width: f32,
}

/// How far past the carriageway the water reaches, drowning the kerb feet.
const CANAL_OVERHANG: f32 = 2.0;

fn canal_for(seed: u64, x_streets: &[Street], z_streets: &[Street]) -> Option<Canal> {
    let key = key_for(seed, stream::RIVER);
    let along_z = key & 1 == 0;
    let list = if along_z { x_streets } else { z_streets };
    // The minor streets only: an arterial carries the traffic the grid
    // cannot spare, and a canal down the main drag is a different game.
    let target = ((key >> 8) & 0xFFFF) as f32 / 65536.0 * 1200.0 - 600.0;
    let (index, street) = list
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.arterial)
        .min_by(|a, b| {
            (a.1.center - target)
                .abs()
                .total_cmp(&(b.1.center - target).abs())
        })?;
    Some(Canal {
        along_z,
        index,
        center: street.center,
        width: street.width + CANAL_OVERHANG * 2.0,
    })
}

/// Walks one axis laying down streets, alternating arterials with a run of
/// minor streets and jittering the gap between them.
fn streets(rng: &mut ChaCha8Rng, half_extent: f32) -> Vec<Street> {
    let mut out = Vec::new();
    let mut edge = -half_extent;
    let mut until_arterial = 0u32;

    loop {
        let arterial = until_arterial == 0;
        let width = if arterial {
            ARTERIAL_WIDTH
        } else {
            MINOR_WIDTH
        };
        if edge + width > half_extent {
            break;
        }

        out.push(Street {
            center: edge + width * 0.5,
            width,
            arterial,
        });

        until_arterial = if arterial {
            rng.random_range(3..=4)
        } else {
            until_arterial - 1
        };

        // Arterials front deeper blocks, which is what puts the tall stuff on
        // the main roads instead of scattering it.
        let depth: f32 = if arterial {
            rng.random_range(62.0..92.0)
        } else {
            rng.random_range(48.0..78.0)
        };
        edge += width + depth;
    }

    out
}

fn build_graph(x_streets: &[Street], z_streets: &[Street], canal: Option<Canal>) -> RoadGraph {
    let mut graph = RoadGraph::default();

    // A node at every crossing.
    for (xi, xs) in x_streets.iter().enumerate() {
        for (zi, zs) in z_streets.iter().enumerate() {
            graph.add_node(Vec2::new(xs.center, zs.center), (xi as u16, zi as u16));
        }
    }

    // No edges *along* the canal: nothing drives, parks or lights a street
    // that is water. Its nodes stay — they are the bridge ends, and every
    // crossing street still runs through them.
    let drowned = |along_z: bool, index: usize| {
        canal.is_some_and(|c| c.along_z == along_z && c.index == index)
    };

    // Link along each street to its immediate neighbour.
    for (xi, xs) in x_streets.iter().enumerate() {
        if drowned(true, xi) {
            continue;
        }
        for zi in 0..z_streets.len().saturating_sub(1) {
            let a = graph.node_at_grid((xi as u16, zi as u16));
            let b = graph.node_at_grid((xi as u16, zi as u16 + 1));
            if let (Some(a), Some(b)) = (a, b) {
                graph.connect(a, b, xs.width, xs.arterial, super::atlas::Surface::Asphalt);
            }
        }
    }
    for (zi, zs) in z_streets.iter().enumerate() {
        if drowned(false, zi) {
            continue;
        }
        for xi in 0..x_streets.len().saturating_sub(1) {
            let a = graph.node_at_grid((xi as u16, zi as u16));
            let b = graph.node_at_grid((xi as u16 + 1, zi as u16));
            if let (Some(a), Some(b)) = (a, b) {
                graph.connect(a, b, zs.width, zs.arterial, super::atlas::Surface::Asphalt);
            }
        }
    }

    graph
}

fn build_blocks(
    seed: u64,
    x_streets: &[Street],
    z_streets: &[Street],
    style: CityStyle,
) -> Vec<Block> {
    let mut rng = stream_for(seed, stream::BLOCKS);
    let mut building_rng = stream_for(seed, stream::BUILDINGS);
    let mut blocks = Vec::new();

    for xi in 0..x_streets.len().saturating_sub(1) {
        for zi in 0..z_streets.len().saturating_sub(1) {
            let (left, right) = (x_streets[xi], x_streets[xi + 1]);
            let (near, far) = (z_streets[zi], z_streets[zi + 1]);

            let area = Rect::new(
                Vec2::new(
                    left.center + left.width * 0.5,
                    near.center + near.width * 0.5,
                ),
                Vec2::new(
                    right.center - right.width * 0.5,
                    far.center - far.width * 0.5,
                ),
            );
            if !area.is_valid() {
                continue;
            }

            let district = district_for(area.center(), &mut rng);
            let arterial = [left.arterial, right.arterial, near.arterial, far.arterial];
            let (buildings, vacants) =
                lay_out_buildings(seed, area, district, arterial, style, &mut building_rng);
            blocks.push(Block {
                paved: true,
                area,
                district,
                buildings,
                vacants,
                arterial,
                quarter: quarter_for(seed, area.center()),
            });
        }
    }

    blocks
}

fn district_for(center: Vec2, rng: &mut ChaCha8Rng) -> District {
    // A few parks anywhere keep the skyline from being uniform.
    if rng.random_range(0.0..1.0) < 0.04 {
        return District::Park;
    }
    let r = center.length();
    match r {
        _ if r < 300.0 => District::Downtown,
        _ if r < 600.0 => District::Midtown,
        _ if r < 850.0 => District::Residential,
        _ => District::Industrial,
    }
}

fn lay_out_buildings(
    seed: u64,
    area: Rect,
    district: District,
    arterial: [bool; 4],
    style: CityStyle,
    rng: &mut ChaCha8Rng,
) -> (Vec<Building>, Vec<VacantLot>) {
    if district == District::Park {
        return (Vec::new(), Vec::new());
    }

    let buildable = area.inset(SIDEWALK_WIDTH);
    if !buildable.is_valid() {
        return (Vec::new(), Vec::new());
    }

    let mut lots = Vec::new();
    // The style's second lever, and the one that decides how *wide* a house is
    // rather than how tall: a postcard of burgage plots subdivides further than
    // one of city blocks. Applied here rather than inside `min_lot`, because
    // `min_lot` is a fact about a district and this is a fact about a city.
    subdivide(
        buildable,
        district.min_lot() * style.lot_scale(),
        rng,
        0,
        &mut lots,
    );

    // The style's one big lever: the same draws, a different skyline.
    let (min_h, max_h) = district.height_range();
    let (min_h, max_h) = style.heights((min_h, max_h));
    let min_h = min_h.max(4.0);
    let max_h = max_h.max(min_h + 1.0);
    let vacancy = district.vacancy();

    let mut buildings = Vec::new();
    let mut vacants = Vec::new();
    for lot in lots {
        // The vacancy roll is unchanged and stays in this stream: moving or
        // skipping it would reshuffle every lot, height and palette after it.
        if rng.random_range(0.0..1.0) < vacancy {
            vacants.push(VacantLot {
                rect: lot,
                purpose: vacant_purpose(seed, &lot, &buildable, arterial, style),
            });
            continue;
        }
        // The setback is applied only to the sides that face a street.
        //
        // It used to be applied on all four, which left every pair of
        // neighbours one to four metres apart. At that width the two
        // buildings read as one frontage from the pavement — and then
        // daylight comes through the seam, because there is a slot there
        // after all. Wo kein Spalt, da kein Lichthof: a block's inner lot
        // lines are party walls now, and the only gaps left in a frontage
        // are the ones a vacant lot actually opens.
        //
        // Still one draw, and still in this position in the stream: moving
        // or dropping it would reshuffle every height and palette after it.
        let setback = rng.random_range(0.6..2.2);
        let footprint = street_setback(lot, &buildable, setback);
        if !footprint.is_valid() {
            continue;
        }
        buildings.push(Building {
            facing: None,
            footprint,
            height: rng.random_range(min_h..max_h),
            palette: rng.random_range(0..PALETTE_SIZE),
            kind: common_kind(seed, &footprint, district),
        });
    }
    (buildings, vacants)
}

/// One lot's footprint: set back from the block's outer edge, flush with its
/// neighbours everywhere else.
///
/// The lots partition the buildable rectangle exactly, so a side that does not
/// lie on that rectangle's boundary is shared with another lot. Insetting such
/// a side opens a slot between two buildings that is too narrow to be a street
/// and too wide to be a joint — see the caller.
fn street_setback(lot: Rect, buildable: &Rect, setback: f32) -> Rect {
    // A lot line is "on the boundary" within a millimetre: the subdivision is
    // float arithmetic on the same endpoints, so the shared edges come back
    // bit-identical, but the outer ones travel through `inset` first.
    let on = |a: f32, b: f32| (a - b).abs() < 1e-3;
    Rect::new(
        Vec2::new(
            lot.min.x
                + if on(lot.min.x, buildable.min.x) {
                    setback
                } else {
                    0.0
                },
            lot.min.y
                + if on(lot.min.y, buildable.min.y) {
                    setback
                } else {
                    0.0
                },
        ),
        Vec2::new(
            lot.max.x
                - if on(lot.max.x, buildable.max.x) {
                    setback
                } else {
                    0.0
                },
            lot.max.y
                - if on(lot.max.y, buildable.max.y) {
                    setback
                } else {
                    0.0
                },
        ),
    )
}

/// A deterministic roll in 0..1 for one footprint, salted per question.
///
/// The `rooftop::seed_for` idea: quantise the centre to a centimetre so a
/// float that comes back from generation one ulp different cannot flip the
/// answer, then hash. Salted so that "what kind of building" and "what is
/// this vacant lot for" are independent questions about the same rectangle.
fn footprint_roll(seed: u64, rect: &Rect, salt: u64) -> f32 {
    let center = rect.center();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for v in [
        (center.x * 100.0).round() as i64 as u64,
        (center.y * 100.0).round() as i64 as u64,
    ] {
        h ^= v;
        h = h.wrapping_mul(0x0100_0000_01b3);
        h ^= h >> 29;
    }
    // The top bits are the well-mixed ones.
    (h >> 40) as f32 / (1u64 << 24) as f32
}

mod salt {
    pub const KIND: u64 = 1;
    pub const VACANT: u64 = 2;
}

/// The everyday mix of a district, as (kind, share) with shares summing to 1.
///
/// Downtown is offices with hotels among them; the further out, the more of
/// the city is simply lived in. Corner shops and restaurants are seasoned
/// through every district rather than zoned into one, because a supermarket
/// you can never stumble across is a supermarket that may as well not exist.
fn common_kinds(district: District) -> &'static [(BuildingKind, f32)] {
    use BuildingKind::*;
    match district {
        District::Downtown => &[
            (Offices, 0.50),
            (Hotel, 0.15),
            (Apartments, 0.15),
            (Restaurant, 0.10),
            (Supermarket, 0.10),
        ],
        District::Midtown => &[
            (Apartments, 0.38),
            (Offices, 0.30),
            (Restaurant, 0.12),
            (Supermarket, 0.12),
            (Hotel, 0.08),
        ],
        District::Residential => &[
            (Apartments, 0.68),
            (Supermarket, 0.12),
            (Restaurant, 0.10),
            (Offices, 0.06),
            (Hotel, 0.04),
        ],
        District::Industrial => &[
            (Offices, 0.45),
            (Supermarket, 0.20),
            (Apartments, 0.20),
            (Restaurant, 0.10),
            (Hotel, 0.05),
        ],
        District::Park => &[(Apartments, 1.0)],
    }
}

/// The common kind of one building: a pure function of (seed, footprint).
pub fn common_kind(seed: u64, footprint: &Rect, district: District) -> BuildingKind {
    let mut roll = footprint_roll(seed, footprint, salt::KIND);
    for (kind, share) in common_kinds(district) {
        if roll < *share {
            return *kind;
        }
        roll -= share;
    }
    BuildingKind::Apartments
}

/// What one vacant lot is for: a pure function of (seed, lot).
fn vacant_purpose(
    seed: u64,
    lot: &Rect,
    buildable: &Rect,
    arterial: [bool; 4],
    style: CityStyle,
) -> VacantUse {
    let size = lot.size();
    let roll = footprint_roll(seed, lot, salt::VACANT);

    // Fronting means the lot's own edge lies on the buildable perimeter on a
    // side whose street is an arterial — subdivision cuts lots flush to the
    // perimeter, so touching it is fronting it.
    let eps = 0.1;
    let fronts_arterial = (arterial[0] && lot.min.x < buildable.min.x + eps)
        || (arterial[1] && lot.max.x > buildable.max.x - eps)
        || (arterial[2] && lot.min.y < buildable.min.y + eps)
        || (arterial[3] && lot.max.y > buildable.max.y - eps);

    if fronts_arterial && size.min_element() > 12.0 && roll < 0.30 {
        return VacantUse::GasStation;
    }
    if size.min_element() > 14.0 && roll > 0.72 {
        return VacantUse::Court;
    }
    // Carved out of the parking band rather than appended past it, so the
    // roll keeps meaning the same thing for the uses that already existed.
    // The band is deliberately narrow: a market you can find is a treat, a
    // market on every second block is a supermarket with weather.
    if size.min_element() > 14.0 && style.market_band().contains(&roll) {
        return VacantUse::Market;
    }
    if size.min_element() > 8.0 && roll > 0.30 {
        return VacantUse::ParkingLot;
    }
    VacantUse::Yard
}

/// Stamps the civic one-offs onto the finished layout.
///
/// Runs after every lot, height and palette is already drawn, with its own
/// `stream::ZONING` — so adding a civic kind, or retuning how many there are,
/// changes *which buildings get a new sign* and nothing else about the city.
/// Heights are rewritten for the claimed buildings, because a fire station
/// drawn as a 40 m slab is a fire station nobody recognises.
fn zone_civics(seed: u64, blocks: &mut [Block], style: CityStyle) {
    use BuildingKind::*;
    let mut rng = stream_for(seed, stream::ZONING);

    // Every candidate, largest footprint first. Ties broken by position so
    // the order — and with it every claim below — is fully deterministic.
    let mut candidates: Vec<(usize, usize)> = blocks
        .iter()
        .enumerate()
        .flat_map(|(bi, block)| (0..block.buildings.len()).map(move |bj| (bi, bj)))
        .collect();
    let area_of = |blocks: &[Block], (bi, bj): (usize, usize)| {
        let s = blocks[bi].buildings[bj].footprint.size();
        s.x * s.y
    };
    candidates.sort_by(|a, b| {
        let (aa, ab) = (area_of(blocks, *a), area_of(blocks, *b));
        ab.total_cmp(&aa).then_with(|| {
            let (fa, fb) = (
                blocks[a.0].buildings[a.1].footprint.min,
                blocks[b.0].buildings[b.1].footprint.min,
            );
            fa.x.total_cmp(&fb.x).then(fa.y.total_cmp(&fb.y))
        })
    });

    let mut claimed: Vec<(usize, usize)> = Vec::new();
    let claim = |blocks: &mut [Block],
                 claimed: &mut Vec<(usize, usize)>,
                 rng: &mut ChaCha8Rng,
                 kind: BuildingKind,
                 count: usize,
                 districts: &[District],
                 heights: std::ops::Range<f32>,
                 spread: f32| {
        let mut placed: Vec<Vec2> = Vec::new();
        for &(bi, bj) in &candidates {
            if placed.len() >= count {
                break;
            }
            if claimed.contains(&(bi, bj)) || !districts.contains(&blocks[bi].district) {
                continue;
            }
            let at = blocks[bi].buildings[bj].footprint.center();
            if placed.iter().any(|p| p.distance(at) < spread) {
                continue;
            }
            let building = &mut blocks[bi].buildings[bj];
            building.kind = kind;
            building.height = rng.random_range(heights.clone());
            claimed.push((bi, bj));
            placed.push(at);
        }
    };

    use District::*;
    // The town hall is the largest thing downtown; everything civic after it
    // works down the same size-ordered list, so prominence follows purpose.
    claim(
        blocks,
        &mut claimed,
        &mut rng,
        TownHall,
        1,
        &[Downtown],
        20.0..28.0,
        0.0,
    );
    claim(
        blocks,
        &mut claimed,
        &mut rng,
        PoliceStation,
        1,
        &[Downtown, Midtown],
        9.0..13.0,
        0.0,
    );
    // Three fire stations, spread out — a single one would leave most of the
    // city to burn, if anything here could burn.
    claim(
        blocks,
        &mut claimed,
        &mut rng,
        FireStation,
        3,
        &[Midtown, Residential, Industrial],
        8.0..11.0,
        350.0,
    );
    claim(
        blocks,
        &mut claimed,
        &mut rng,
        Barracks,
        1,
        &[Industrial],
        6.0..9.0,
        0.0,
    );
    claim(
        blocks,
        &mut claimed,
        &mut rng,
        ParkingGarage,
        4,
        &[Downtown, Midtown],
        14.0..20.0,
        250.0,
    );
    claim(
        blocks,
        &mut claimed,
        &mut rng,
        Museum,
        1,
        &[Downtown, Midtown],
        11.0..15.0,
        0.0,
    );
    // Two schools, far apart — every catchment area deserves its own bell.
    claim(
        blocks,
        &mut claimed,
        &mut rng,
        School,
        2,
        &[Residential, Midtown],
        7.0..10.0,
        500.0,
    );
    // Churches, spread like the fire stations: a skyline needs its spires
    // the way a street needs its hydrants. How many is the style's call —
    // Landshüpf keeps five, New Dork barely keeps one. The height claimed
    // here is the *presence* — `world::church` builds a lower nave and a
    // taller tower out of it, so the box the layout stores never appears.
    let (churches, cathedral) = style.churches();
    claim(
        blocks,
        &mut claimed,
        &mut rng,
        Church,
        churches,
        &[Downtown, Midtown, Residential],
        10.0..14.0,
        350.0,
    );
    if cathedral {
        claim(
            blocks,
            &mut claimed,
            &mut rng,
            Cathedral,
            1,
            &[Downtown, Midtown],
            13.0..16.0,
            0.0,
        );
    }
    // One stadium, out where the land is cheap — which is where they
    // actually get built. The claimed height is only the stands' presence;
    // `world::stadium` builds tiers and floodlights out of the footprint.
    claim(
        blocks,
        &mut claimed,
        &mut rng,
        Stadium,
        1,
        &[Industrial, Residential],
        9.0..11.0,
        0.0,
    );
}

/// Number of material variants per district.
///
/// Six rather than four since the Landshut pass: an old town's whole look is
/// that no two houses in a row are the same colour, and with four tones a
/// terrace of eight repeats itself twice however the draws fall. The cost is
/// two more shared materials per district — every building still draws from
/// the same table, so this is a handful of materials, not a handful per house.
pub const PALETTE_SIZE: u8 = 6;

/// Recursively halves a block into lots, always splitting the longer side so
/// lots stay roughly square rather than degenerating into slivers.
fn subdivide(rect: Rect, min_lot: f32, rng: &mut ChaCha8Rng, depth: u32, out: &mut Vec<Rect>) {
    const MAX_DEPTH: u32 = 6;
    let size = rect.size();
    let can_split_x = size.x > min_lot * 2.0;
    let can_split_z = size.y > min_lot * 2.0;

    if depth >= MAX_DEPTH || (!can_split_x && !can_split_z) {
        out.push(rect);
        return;
    }

    let split_x = if can_split_x && can_split_z {
        size.x > size.y
    } else {
        can_split_x
    };
    let t: f32 = rng.random_range(0.4..0.6);

    let (a, b) = if split_x {
        let x = rect.min.x + size.x * t;
        (
            Rect::new(rect.min, Vec2::new(x, rect.max.y)),
            Rect::new(Vec2::new(x, rect.min.y), rect.max),
        )
    } else {
        let z = rect.min.y + size.y * t;
        (
            Rect::new(rect.min, Vec2::new(rect.max.x, z)),
            Rect::new(Vec2::new(rect.min.x, z), rect.max),
        )
    };

    subdivide(a, min_lot, rng, depth + 1, out);
    subdivide(b, min_lot, rng, depth + 1, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> CityLayout {
        generate(
            0xA17E_5EED,
            1000.0,
            crate::core::config::CityStyle::Generisch,
        )
    }

    #[test]
    fn same_seed_rebuilds_the_same_city() {
        assert_eq!(
            generate(7, 800.0, crate::core::config::CityStyle::Generisch).digest(),
            generate(7, 800.0, crate::core::config::CityStyle::Generisch).digest()
        );
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(
            generate(7, 800.0, crate::core::config::CityStyle::Generisch).digest(),
            generate(8, 800.0, crate::core::config::CityStyle::Generisch).digest()
        );
    }

    #[test]
    fn produces_a_substantial_city() {
        let city = layout();
        assert!(city.blocks.len() > 100, "blocks: {}", city.blocks.len());
        assert!(
            city.building_count() > 500,
            "buildings: {}",
            city.building_count()
        );
    }

    #[test]
    fn buildings_stay_inside_their_block() {
        for block in &layout().blocks {
            let buildable = block.area.inset(SIDEWALK_WIDTH);
            for b in &block.buildings {
                assert!(
                    b.footprint.min.x >= buildable.min.x - 1e-3
                        && b.footprint.min.y >= buildable.min.y - 1e-3
                        && b.footprint.max.x <= buildable.max.x + 1e-3
                        && b.footprint.max.y <= buildable.max.y + 1e-3,
                    "building escaped its block: {:?} vs {:?}",
                    b.footprint,
                    buildable
                );
            }
        }
    }

    #[test]
    fn buildings_never_overlap_each_other() {
        for block in &layout().blocks {
            for (i, a) in block.buildings.iter().enumerate() {
                for b in &block.buildings[i + 1..] {
                    assert!(
                        !a.footprint.overlaps(&b.footprint),
                        "overlapping footprints {:?} / {:?}",
                        a.footprint,
                        b.footprint
                    );
                }
            }
        }
    }

    #[test]
    fn blocks_never_overlap_the_roads() {
        let city = layout();
        for block in &city.blocks {
            for street in &city.x_streets {
                let (lo, hi) = (
                    street.center - street.width * 0.5,
                    street.center + street.width * 0.5,
                );
                assert!(
                    block.area.max.x <= lo + 1e-3 || block.area.min.x >= hi - 1e-3,
                    "block {:?} overlaps street at x={}",
                    block.area,
                    street.center
                );
            }
        }
    }

    #[test]
    fn the_same_seed_zones_the_same_city() {
        let (a, b) = (
            generate(7, 800.0, crate::core::config::CityStyle::Generisch),
            generate(7, 800.0, crate::core::config::CityStyle::Generisch),
        );
        let kinds = |city: &CityLayout| -> Vec<BuildingKind> {
            city.blocks
                .iter()
                .flat_map(|block| block.buildings.iter().map(|b| b.kind))
                .collect()
        };
        let purposes = |city: &CityLayout| -> Vec<VacantUse> {
            city.blocks
                .iter()
                .flat_map(|block| block.vacants.iter().map(|v| v.purpose))
                .collect()
        };
        assert_eq!(kinds(&a), kinds(&b));
        assert_eq!(purposes(&a), purposes(&b));
    }

    #[test]
    fn there_is_one_town_hall_and_it_stands_downtown() {
        let city = layout();
        let halls: Vec<_> = city
            .blocks
            .iter()
            .filter(|block| {
                block
                    .buildings
                    .iter()
                    .any(|b| b.kind == BuildingKind::TownHall)
            })
            .collect();
        assert_eq!(halls.len(), 1, "a city has exactly one Rathaus");
        assert_eq!(halls[0].district, District::Downtown);
    }

    #[test]
    fn the_civic_register_is_complete() {
        let city = layout();
        let count = |kind: BuildingKind| -> usize {
            city.blocks
                .iter()
                .flat_map(|block| &block.buildings)
                .filter(|b| b.kind == kind)
                .count()
        };
        assert_eq!(count(BuildingKind::TownHall), 1);
        assert_eq!(count(BuildingKind::PoliceStation), 1);
        assert_eq!(count(BuildingKind::FireStation), 3);
        assert_eq!(count(BuildingKind::Barracks), 1);
        assert_eq!(count(BuildingKind::ParkingGarage), 4);
    }

    #[test]
    fn a_gas_station_only_stands_on_an_arterial_lot() {
        let mut stations = 0;
        for block in &layout().blocks {
            let buildable = block.area.inset(SIDEWALK_WIDTH);
            for vacant in &block.vacants {
                if vacant.purpose != VacantUse::GasStation {
                    continue;
                }
                stations += 1;
                let lot = vacant.rect;
                let eps = 0.1;
                let fronts = (block.arterial[0] && lot.min.x < buildable.min.x + eps)
                    || (block.arterial[1] && lot.max.x > buildable.max.x - eps)
                    || (block.arterial[2] && lot.min.y < buildable.min.y + eps)
                    || (block.arterial[3] && lot.max.y > buildable.max.y - eps);
                assert!(fronts, "gas station on a back lot: {lot:?}");
            }
        }
        assert!(
            stations > 0,
            "the default seed should manage one filling station"
        );
    }

    #[test]
    fn common_kinds_are_a_pure_function_of_the_footprint() {
        let rect = Rect::new(Vec2::new(10.0, 20.0), Vec2::new(30.0, 44.0));
        assert_eq!(
            common_kind(7, &rect, District::Midtown),
            common_kind(7, &rect, District::Midtown),
        );
        // And every district's shares cover the whole roll, so no roll can
        // fall off the end of the table into the fallback.
        for district in [
            District::Downtown,
            District::Midtown,
            District::Residential,
            District::Industrial,
        ] {
            let total: f32 = common_kinds(district).iter().map(|(_, share)| share).sum();
            assert!(
                (total - 1.0).abs() < 1e-5,
                "{district:?} shares sum to {total}"
            );
        }
    }

    #[test]
    fn vacant_lots_have_lives_now() {
        let city = layout();
        let vacants: usize = city.blocks.iter().map(|b| b.vacants.len()).sum();
        assert!(vacants > 20, "vacancy rolls should leave lots: {vacants}");
        let parking = city
            .blocks
            .iter()
            .flat_map(|b| &b.vacants)
            .filter(|v| v.purpose == VacantUse::ParkingLot)
            .count();
        assert!(parking > 5, "most vacants should be parking: {parking}");
    }

    #[test]
    fn every_intersection_is_reachable() {
        let city = layout();
        let graph = &city.graph;
        assert!(graph.node_count() > 100);

        // A grid should be fully connected; a path from the first node to the
        // last is a cheap proxy that also exercises A*.
        let first = super::super::roadgraph::NodeId(0);
        let last = super::super::roadgraph::NodeId(graph.node_count() as u32 - 1);
        let path = graph.path(first, last).expect("no route across the city");
        assert!(path.len() > 2);
        assert_eq!(path[0], first);
        assert_eq!(*path.last().unwrap(), last);

        // Consecutive nodes in the path must actually share an edge.
        for pair in path.windows(2) {
            assert!(
                graph.neighbors(pair[0]).any(|(n, _)| n == pair[1]),
                "path jumps between unconnected nodes"
            );
        }
    }
}
