//! Judging a town by numbers, without a window.
//!
//! `tools/shoot.sh` is the visual half of checking Landshut: shoot the same
//! framings before and after and look. This is the numeric half. A change to
//! the bake or to the marcher moves things a screenshot cannot count — how
//! much of the mapped footprint survived, how much of a street has a wall
//! along it, whether the ground under a corridor is still exactly zero — and
//! rendering a frame to find out costs a minute of GPU for one framing of one
//! quarter of the town.
//!
//! ```sh
//! cargo run -- --survey                       # the persisted city and seed
//! cargo run -- --survey --city landshuepf     # a named style
//! cargo run -- --survey --city landshuepf --seed 7
//! ```
//!
//! builds exactly what `world::generate_city` builds — the atlas, the layout,
//! the town's own footprints, the marcher's terraces, the terrain — prints a
//! scorecard and exits, without starting Bevy at all. The layout is a pure
//! function of `(seed, style, atlas)` and none of it needs an `App`, which is
//! what makes this a page rather than a second capture harness; it is the
//! same shape `audio::audition` has, for the same reason.
//!
//! What it measures is written down beside each section below. The point of
//! measuring it is the tests at the bottom: the committed Landshut has to hold
//! a bar on every figure, so a re-bake that quietly threw away a tenth of the
//! Altstadt, or a marcher change that opened the street wall, fails a test
//! before anybody renders anything.

use std::path::Path;
use std::time::Instant;

use bevy::math::Vec2;
use bevy::platform::collections::HashMap;

use crate::core::config::{CityStyle, GameConfig};
use crate::world::atlas::{self, Atlas, HILL, Signposts, Surface};
use crate::world::citygen::{self, Building, CityLayout, RoofShape, SIDEWALK_WIDTH};
use crate::world::roadgraph::EdgeId;
use crate::world::streetside;
use crate::world::terrain::Terrain;

/// How far out from the middle the core of a real town runs, as a share of
/// the half extent. The same ring `streetside` calls downtown; measured here
/// so the Altstadt can be judged apart from the suburbs it is graded with.
const CORE: f32 = 0.22;
/// And how far out the street wall is measured at all. Past this a Landshut
/// street is a suburb of detached houses, and a suburb has no wall.
const INNER: f32 = 0.55;

/// How far apart the kerb is sampled when the street wall is measured, and
/// how far out from the kerb a sample looks for a building. Two metres is
/// finer than any frontage; eighteen covers the pavement and a fifteen-metre
/// setback behind it, which is as far back as anything still on the street
/// stands.
const KERB_STEP: f32 = 2.0;
const WALL_REACH: f32 = 18.0;

/// How much of each end of a street is not sampled. The same nine metres
/// `streetside` keeps clear of a crossing: a junction has no wall by design,
/// and counted, every crossing in the town would be the longest gap on both
/// streets meeting there.
const JUNCTION_CLEAR: f32 = 9.0;

/// Metres of height per storey, for the histogram. A Landshut townhouse floor.
const STOREY: f32 = 3.0;

/// Spacing of the grids the relief and the gradient are read on.
const RELIEF_GRID: f32 = 20.0;

/// How far past the played square `world::setup_ground` rasterises the level
/// field. `ENVELOPE` is private to `world`, so its value is repeated here;
/// the reach has to be the same one or the corridor test below measures a
/// different field from the one the game plays on.
const ENVELOPE: f32 = 140.0;

/// The spatial grid the buildings are filed in for the ray casts, in metres.
const CELL: f32 = 20.0;

/// How many of the longest unbuilt stretches to report.
const GAPS_REPORTED: usize = 10;

/// What a run was asked to survey.
#[derive(Debug, Clone, Copy)]
pub struct SurveyRequest {
    pub city: CityStyle,
    pub seed: u64,
    pub half_extent: f32,
}

/// The survey a run was asked for, if it was.
///
/// The defaults are the *persisted* ones from `saves/options.ron`, for the
/// reason `core::capture` uses them: a survey of the code-default seed would
/// be a survey of a different city from the one on screen. `--city` and
/// `--seed` override, so a style the player has not selected can be measured
/// without editing anybody's options file.
pub fn requested() -> Option<SurveyRequest> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--survey")?;
    let value_of = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let persisted = persisted();
    let city = match value_of("--city") {
        Some(name) => style_named(&name).unwrap_or_else(|| panic!("--city {name}: no such style")),
        None => persisted.city,
    };
    let seed = match value_of("--seed") {
        Some(text) => parse_seed(&text).unwrap_or_else(|| panic!("--seed {text}: not a number")),
        None => persisted.world_seed,
    };
    Some(SurveyRequest {
        city,
        seed,
        half_extent: persisted.world.half_extent,
    })
}

/// Runs the survey and prints the scorecard.
pub fn run(request: &SurveyRequest) {
    print!("{}", survey(request).render());
}

/// What `saves/options.ron` says, or the defaults.
///
/// Read here rather than through `core::settings`, whose loader fills a
/// `KeyBindings` too and is written for the app; this wants one field of the
/// same file. Serde ignores the rest.
fn persisted() -> GameConfig {
    #[derive(serde::Deserialize)]
    struct Persisted {
        config: GameConfig,
    }
    std::fs::read_to_string(Path::new("saves").join("options.ron"))
        .ok()
        .and_then(|text| ron::from_str::<Persisted>(&text).ok())
        .map(|options| options.config)
        .unwrap_or_default()
}

/// The style a flag names: the enum name or the label, either case, the way
/// `core::capture` reads `--city`.
fn style_named(name: &str) -> Option<CityStyle> {
    CityStyle::ALL.into_iter().find(|style| {
        style.label().eq_ignore_ascii_case(name) || format!("{style:?}").eq_ignore_ascii_case(name)
    })
}

/// A seed as typed: decimal, or hex with a `0x`, which is how the default one
/// is written in `GameConfig`.
fn parse_seed(text: &str) -> Option<u64> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

// ----------------------------------------------------------------- the card --

/// Everything the survey measured. Numbers rather than text, so a test can
/// hold a bar on one of them without parsing the table.
#[derive(Debug, Clone)]
pub struct Survey {
    pub city: CityStyle,
    pub seed: u64,
    pub half_extent: f32,
    pub streets: Streets,
    pub buildings: Buildings,
    /// `None` for a generated city, which has no map to be measured against.
    pub coverage: Option<Coverage>,
    pub closure: Closure,
    pub relief: ReliefReport,
    /// Each stage and how long it took, in milliseconds.
    pub timing: Vec<(&'static str, f32)>,
}

#[derive(Debug, Clone, Default)]
pub struct Streets {
    pub edges: usize,
    pub km: f32,
    pub nodes: usize,
    /// Nodes with three or more arms.
    pub junctions: usize,
    /// Runs of the extract's polylines cut at the edge of the square, and
    /// runs left out for standing above [`HILL`] — recomputed from the atlas
    /// by the rule `atlas::layout` applies.
    pub clipped: usize,
    pub uphill: usize,
    /// Edges and kilometres per width bin: up to 4 m, 4–6, 6–9, 9–14, over 14.
    pub widths: [(usize, f32); 5],
    pub widest: f32,
    /// Kilometres per [`Surface`], by `Surface::index`.
    pub surfaces: [f32; 4],
}

#[derive(Debug, Clone, Default)]
pub struct Buildings {
    /// The map's own buildings whose middle is inside the square, and the
    /// parts they were cut into.
    pub mapped: usize,
    pub mapped_parts: usize,
    /// Of those, what stands in the town after `lots` has had its say.
    pub real: usize,
    pub real_parts: usize,
    /// Parts `lots` dropped for standing on a carriageway.
    pub in_a_road: usize,
    /// Buildings left to the hill by `atlas::footprints`.
    pub uphill: usize,
    /// What the marcher invented: houses in terraces, and sheds behind them.
    pub houses: usize,
    pub outbuildings: usize,
    /// What the generator laid out in blocks, for a city with no map: the
    /// one kind of building that is neither mapped nor marched.
    pub generated: usize,
    pub generated_blocks: usize,
    /// Buildings by storeys, one to six and seven-plus.
    pub storeys: [usize; 7],
    /// Real parts by the roof the map gave them, in [`ROOFS`] order, and the
    /// parts the map said nothing about.
    pub roofs: [usize; 6],
    pub roof_unsaid: usize,
    /// The style's own answer where the map is silent: the share of low
    /// houses that get a gable.
    pub gables: f32,
    /// Landmarks kept up on the hill, standing on their own ground.
    pub on_the_hill: usize,
}

/// How much of the mapped footprint the town keeps.
///
/// The denominator is the true polygon area the bake recorded per building;
/// `bake` counts the rectangles it cut the polygon into, `built` only those
/// that survived the hill and the carriageway test. Each overall and inside
/// the core ring.
#[derive(Debug, Clone, Copy, Default)]
pub struct Coverage {
    pub bake: f32,
    pub bake_core: f32,
    pub built: f32,
    pub built_core: f32,
}

/// Kerb samples that found a building within [`WALL_REACH`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Share {
    pub hits: usize,
    pub samples: usize,
}

impl Share {
    pub fn share(&self) -> f32 {
        if self.samples == 0 {
            0.0
        } else {
            self.hits as f32 / self.samples as f32
        }
    }

    fn count(&mut self, hit: bool) {
        self.samples += 1;
        self.hits += usize::from(hit);
    }
}

/// How closed the street wall is.
#[derive(Debug, Clone, Default)]
pub struct Closure {
    /// Against every building, real or invented: what the player sees.
    pub all: Share,
    pub all_core: Share,
    /// Against the map's own buildings only: what the map says is there.
    pub real: Share,
    pub real_core: Share,
    /// The longest stretches of kerb with nothing behind them, longest first.
    pub gaps: Vec<Gap>,
}

#[derive(Debug, Clone)]
pub struct Gap {
    pub metres: f32,
    pub edge: EdgeId,
    pub name: Option<String>,
    /// The middle of the stretch, on the kerb.
    pub at: Vec2,
}

#[derive(Debug, Clone, Default)]
pub struct ReliefReport {
    /// Metres above sea level the relief is measured from; `None` without a
    /// relief.
    pub datum: Option<f32>,
    pub min: f32,
    pub max: f32,
    pub mean: f32,
    /// Share of the square standing above [`HILL`].
    pub hill_share: f32,
    /// The worst |height| the terrain returns under any street corridor and
    /// the plots along it. The rule at the top of `world::terrain` says this
    /// is exactly zero.
    pub worst_under_a_street: f32,
    /// The steepest slope on a grid inside 1.2 × the half extent, and where.
    pub steepest: f32,
    pub steepest_at: Vec2,
}

/// The roof shapes in the order the card lists them.
const ROOFS: [(RoofShape, &str); 6] = [
    (RoofShape::Gabled, "gabled"),
    (RoofShape::Hipped, "hipped"),
    (RoofShape::HalfHipped, "half-hipped"),
    (RoofShape::Flat, "flat"),
    (RoofShape::Pyramidal, "pyramidal"),
    (RoofShape::Skillion, "skillion"),
];

fn surface_name(surface: Surface) -> &'static str {
    match surface {
        Surface::Asphalt => "asphalt",
        Surface::Sett => "sett",
        Surface::Slabs => "slabs",
        Surface::Gravel => "gravel",
    }
}

/// Which width bin a carriageway falls in.
fn width_bin(width: f32) -> usize {
    match width {
        w if w <= 4.0 => 0,
        w if w <= 6.0 => 1,
        w if w <= 9.0 => 2,
        w if w <= 14.0 => 3,
        _ => 4,
    }
}

// --------------------------------------------------------------- building --

/// Builds the town the way `world::generate_city` does and measures it.
pub fn survey(request: &SurveyRequest) -> Survey {
    let SurveyRequest {
        city,
        seed,
        half_extent,
    } = *request;
    let mut timing: Vec<(&'static str, f32)> = Vec::new();
    let mut clock = Instant::now();
    let mut lap = |name: &'static str, clock: &mut Instant| {
        timing.push((name, clock.elapsed().as_secs_f32() * 1000.0));
        *clock = Instant::now();
    };

    // The same three steps, in the same order, as `generate_city`; a survey
    // of a different town from the one on screen is no use to anybody.
    let town = city.atlas().and_then(atlas::load);
    lap("load", &mut clock);
    let (mut layout, signs) = town
        .as_ref()
        .map(|town| atlas::layout(town, seed, half_extent))
        .unwrap_or_else(|| {
            (
                citygen::generate(seed, half_extent, city),
                Signposts::default(),
            )
        });
    lap("layout", &mut clock);

    // The map's own parts as `footprints` hands them to the marcher, keyed
    // so they can be found again in what the marcher hands back.
    let mut mapped_parts: HashMap<PartKey, usize> = HashMap::default();
    let mut mapped = 0usize;
    let marched = layout.blocks.is_empty();
    if marched {
        let real = town
            .as_ref()
            .map(|atlas| atlas::footprints(atlas, &layout.graph, seed, half_extent, city))
            .unwrap_or_default();
        lap("footprints", &mut clock);
        mapped = real.len();
        for (group, block) in real.iter().enumerate() {
            for building in &block.buildings {
                mapped_parts.insert(PartKey::of(building), group);
            }
        }
        let (blocks, _holes) = streetside::lots(&layout, seed, city, real);
        lap("lots", &mut clock);
        layout.blocks = blocks;
    }

    let gentle = layout.blocks.first().is_some_and(|block| !block.paved);
    let reach = half_extent + ENVELOPE + crate::world::BACKLAND + 60.0;
    let terrain = Terrain::new(&layout, reach, gentle, seed);
    lap("terrain", &mut clock);

    let streets = measure_streets(&layout, town.as_ref());
    let (buildings, coverage) = measure_buildings(
        &layout,
        town.as_ref(),
        &mapped_parts,
        mapped,
        marched,
        city,
        half_extent,
    );
    let closure = measure_closure(&layout, &signs, &mapped_parts, half_extent);
    let relief = measure_relief(&layout, &terrain, half_extent);
    lap("survey", &mut clock);

    Survey {
        city,
        seed,
        half_extent,
        streets,
        buildings,
        coverage,
        closure,
        relief,
        timing,
    }
}

/// A part of a real building, as something that can be looked up after the
/// marcher has rebuilt the block round it.
///
/// Its middle and its size, to the decimetre. `lots` keeps a part's
/// footprint untouched or drops it, so a part that comes back is found by
/// the key it went in under, and an invented house cannot share one — it was
/// tested against the part's footprint before it was placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PartKey(i32, i32, i32, i32);

impl PartKey {
    fn of(building: &Building) -> Self {
        let centre = building.footprint.center();
        let size = building.footprint.size();
        let dm = |v: f32| (v * 10.0).round() as i32;
        Self(dm(centre.x), dm(centre.y), dm(size.x), dm(size.y))
    }
}

fn measure_streets(layout: &CityLayout, town: Option<&Atlas>) -> Streets {
    let graph = &layout.graph;
    let mut streets = Streets {
        edges: graph.edge_count(),
        nodes: graph.node_count(),
        junctions: graph
            .nodes()
            .filter(|(_, node)| node.edges.len() >= 3)
            .count(),
        ..Default::default()
    };
    for edge in graph.edges() {
        let km = edge.length / 1000.0;
        streets.km += km;
        let bin = &mut streets.widths[width_bin(edge.width)];
        bin.0 += 1;
        bin.1 += km;
        streets.widest = streets.widest.max(edge.width);
        streets.surfaces[edge.surface.index()] += km;
    }

    // What the layout threw away, counted again by its own rule: a run of
    // points ends where one leaves the square, and where one stands up on
    // the hill. `atlas::layout` logs the two numbers and keeps neither.
    if let Some(town) = town {
        let relief = town.relief.as_ref();
        for street in &town.streets {
            let mut inside = false;
            for &(x, z) in &street.points {
                let at = Vec2::new(x, z);
                if at.x.abs() > layout.half_extent || at.y.abs() > layout.half_extent {
                    streets.clipped += usize::from(inside);
                    inside = false;
                } else if relief.is_some_and(|relief| relief.is_hill(at)) {
                    streets.uphill += usize::from(inside);
                    inside = false;
                } else {
                    inside = true;
                }
            }
        }
    }
    streets
}

fn measure_buildings(
    layout: &CityLayout,
    town: Option<&Atlas>,
    mapped_parts: &HashMap<PartKey, usize>,
    mapped: usize,
    // Whether `streetside::lots` built these blocks. A generated block holds
    // a dozen buildings that are neither mapped nor invented by the marcher,
    // and read as the marcher's they would all count as terrace houses.
    marched: bool,
    city: CityStyle,
    half_extent: f32,
) -> (Buildings, Option<Coverage>) {
    let mut buildings = Buildings {
        mapped,
        mapped_parts: mapped_parts.len(),
        gables: city.gables(),
        ..Default::default()
    };

    // Which groups came back, and with how many parts.
    let mut kept_groups: HashMap<usize, usize> = HashMap::default();
    for block in &layout.blocks {
        let Some(first) = block.buildings.first() else {
            continue;
        };
        if !marched {
            buildings.generated_blocks += 1;
            for building in &block.buildings {
                let storeys = ((building.height / STOREY).round() as usize).clamp(1, 7);
                buildings.storeys[storeys - 1] += 1;
                buildings.generated += 1;
            }
            continue;
        }
        let storeys = ((first.height / STOREY).round() as usize).clamp(1, 7);
        buildings.storeys[storeys - 1] += 1;
        if first.ground > 0.0 {
            buildings.on_the_hill += 1;
        }
        match mapped_parts.get(&PartKey::of(first)) {
            Some(group) => {
                buildings.real += 1;
                *kept_groups.entry(*group).or_default() += block.buildings.len();
                for building in &block.buildings {
                    buildings.real_parts += 1;
                    match building.roof {
                        Some(roof) => {
                            if let Some(slot) = ROOFS.iter().position(|(shape, _)| *shape == roof) {
                                buildings.roofs[slot] += 1;
                            }
                        }
                        None => buildings.roof_unsaid += 1,
                    }
                }
            }
            None => {
                // The marcher files a terrace house with a world box round its
                // rotated footprint and a shed with its footprint as its box;
                // nothing else tells the two apart once they are blocks.
                if block.area == first.footprint {
                    buildings.outbuildings += 1;
                } else {
                    buildings.houses += 1;
                }
            }
        }
    }
    buildings.in_a_road = buildings.mapped_parts - buildings.real_parts;

    let Some(town) = town else {
        return (buildings, None);
    };

    // The mapped footprint, group by group, the way `footprints` gathers
    // them: a group is a run of consecutive parts, largest first.
    let relief = town.relief.as_ref();
    let mut coverage = Coverage::default();
    let (mut area, mut area_core) = (0.0f32, 0.0f32);
    let (mut bake, mut bake_core) = (0.0f32, 0.0f32);
    let (mut built, mut built_core) = (0.0f32, 0.0f32);
    let mut group = 0usize;
    let mut index = 0usize;
    while index < town.buildings.len() {
        let first = &town.buildings[index];
        let mut last = index + 1;
        if first.group.is_some() {
            while last < town.buildings.len() && town.buildings[last].group == first.group {
                last += 1;
            }
        }
        let parts = &town.buildings[index..last];
        index = last;
        let centre = Vec2::new(first.centre.0, first.centre.1);
        if centre.x.abs() > half_extent || centre.y.abs() > half_extent {
            continue;
        }
        let this = group;
        group += 1;
        if first.name.is_empty() && relief.is_some_and(|relief| relief.is_hill(centre)) {
            buildings.uphill += 1;
        }
        if first.area <= 0.0 {
            continue;
        }
        let core = centre.length() < CORE * half_extent;
        let cut: f32 = parts.iter().map(|part| part.frontage * part.depth).sum();
        // What came back of it: the parts `lots` kept, by their own boxes.
        let kept: f32 = layout
            .blocks
            .iter()
            .filter(|block| {
                block
                    .buildings
                    .first()
                    .and_then(|b| mapped_parts.get(&PartKey::of(b)))
                    == Some(&this)
            })
            .flat_map(|block| block.buildings.iter())
            .map(|building| {
                let size = building.footprint.size();
                size.x * size.y
            })
            .sum();
        area += first.area;
        bake += cut;
        built += kept;
        if core {
            area_core += first.area;
            bake_core += cut;
            built_core += kept;
        }
    }
    let ratio = |over: f32, under: f32| if under > 0.0 { over / under } else { 0.0 };
    coverage.bake = ratio(bake, area);
    coverage.bake_core = ratio(bake_core, area_core);
    coverage.built = ratio(built, area);
    coverage.built_core = ratio(built_core, area_core);
    (buildings, Some(coverage))
}

// ---------------------------------------------------------- street wall --

/// A building as something a ray can hit: a rotated rectangle.
///
/// The same shape `streetside::Oblong` is, written again here because that
/// one is private to the marcher and this one answers a different question —
/// not "do these two overlap" but "does a line from the kerb reach this".
#[derive(Debug, Clone, Copy)]
struct Wall {
    centre: Vec2,
    /// Unit vector along the frontage.
    axis: Vec2,
    half: Vec2,
    real: bool,
}

impl Wall {
    fn of(building: &Building, real: bool) -> Self {
        // `facing` turns +Z outwards, so local +X — the frontage — is the
        // way the street runs. A generated building faces nothing and its
        // footprint is a box on the map.
        let axis = match building.facing {
            Some(yaw) => Vec2::new(yaw.cos(), -yaw.sin()),
            None => Vec2::X,
        };
        Self {
            centre: building.footprint.center(),
            axis,
            half: building.footprint.size() * 0.5,
            real,
        }
    }

    fn across(&self) -> Vec2 {
        Vec2::new(-self.axis.y, self.axis.x)
    }

    /// Does a ray `length` long from `from` along the unit `dir` enter this?
    ///
    /// The slab test in the rectangle's own frame: the ray is clipped to the
    /// span between the two long sides and then to the span between the two
    /// short ones, and it hits if anything of it is left.
    fn struck_by(&self, from: Vec2, dir: Vec2, length: f32) -> bool {
        let q = from - self.centre;
        let origin = Vec2::new(q.dot(self.axis), q.dot(self.across()));
        let d = Vec2::new(dir.dot(self.axis), dir.dot(self.across()));
        let (mut t0, mut t1) = (0.0f32, length);
        for k in 0..2 {
            let (o, v, h) = (origin[k], d[k], self.half[k]);
            if v.abs() < 1.0e-6 {
                if o.abs() > h {
                    return false;
                }
            } else {
                let (mut a, mut b) = ((-h - o) / v, (h - o) / v);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                t0 = t0.max(a);
                t1 = t1.min(b);
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
    }
}

/// Every wall in the town, filed by cell so a ray tests a handful of them.
struct Walls(HashMap<(i32, i32), Vec<Wall>>);

fn cell_of(at: Vec2) -> (i32, i32) {
    ((at.x / CELL).floor() as i32, (at.y / CELL).floor() as i32)
}

impl Walls {
    fn build(layout: &CityLayout, mapped_parts: &HashMap<PartKey, usize>) -> Self {
        let mut filed: HashMap<(i32, i32), Vec<Wall>> = HashMap::default();
        for block in &layout.blocks {
            for building in &block.buildings {
                let real = mapped_parts.contains_key(&PartKey::of(building));
                let wall = Wall::of(building, real);
                let reach = Vec2::splat(wall.half.x + wall.half.y);
                let (low, high) = (cell_of(wall.centre - reach), cell_of(wall.centre + reach));
                for x in low.0..=high.0 {
                    for z in low.1..=high.1 {
                        filed.entry((x, z)).or_default().push(wall);
                    }
                }
            }
        }
        Self(filed)
    }

    /// What a ray from the kerb finds: `(anything, something real)`.
    fn cast(&self, from: Vec2, dir: Vec2, length: f32) -> (bool, bool) {
        let to = from + dir * length;
        let (low, high) = (cell_of(from.min(to)), cell_of(from.max(to)));
        let (mut any, mut real) = (false, false);
        for x in low.0..=high.0 {
            for z in low.1..=high.1 {
                let Some(near) = self.0.get(&(x, z)) else {
                    continue;
                };
                for wall in near {
                    if (!any || (wall.real && !real)) && wall.struck_by(from, dir, length) {
                        any = true;
                        real |= wall.real;
                    }
                }
            }
        }
        (any, real)
    }
}

fn measure_closure(
    layout: &CityLayout,
    signs: &Signposts,
    mapped_parts: &HashMap<PartKey, usize>,
    half_extent: f32,
) -> Closure {
    let graph = &layout.graph;
    let walls = Walls::build(layout, mapped_parts);
    let mut closure = Closure::default();
    let mut gaps: Vec<Gap> = Vec::new();

    // Along each street rather than each segment, because a hole in the
    // frontage does not stop at a segment end and cut there the longest one
    // in the town reads as twenty metres.
    for (points, edges) in streetside::street_runs(layout) {
        if points.len() < 2 || edges.is_empty() {
            continue;
        }
        let mut reach = Vec::with_capacity(points.len());
        let mut running = 0.0f32;
        reach.push(0.0);
        for pair in points.windows(2) {
            running += pair[0].distance(pair[1]);
            reach.push(running);
        }
        let total = running;
        if total <= 0.0 {
            continue;
        }
        // The nodes at the two ends, found from the edges: the points are
        // the nodes' own positions, so the match is exact.
        let end_is_junction = |edge: EdgeId, at: Vec2| {
            let e = graph.edge(edge);
            let node = if graph.node(e.a).pos == at { e.a } else { e.b };
            graph.node(node).edges.len() >= 3
        };
        let head = end_is_junction(edges[0], points[0]);
        let tail = end_is_junction(*edges.last().unwrap(), *points.last().unwrap());

        // Per segment: its edge, the ring it is in, its direction.
        let segments: Vec<(EdgeId, f32, Vec2)> = edges
            .iter()
            .enumerate()
            .map(|(i, edge)| {
                let out = points[i].midpoint(points[i + 1]).length() / half_extent.max(1.0);
                let axis = (points[i + 1] - points[i]).normalize_or_zero();
                (*edge, out, axis)
            })
            .collect();
        let locate = |along: f32| -> (usize, Vec2) {
            let i = match reach.binary_search_by(|r| r.total_cmp(&along)) {
                Ok(i) => i.min(segments.len() - 1),
                Err(i) => i.saturating_sub(1).min(segments.len() - 1),
            };
            (i, points[i] + segments[i].2 * (along - reach[i]))
        };

        for side in [-1.0f32, 1.0] {
            // An open stretch: where it started and how many samples long.
            let mut open: Option<(f32, usize)> = None;
            let close = |open: &mut Option<(f32, usize)>, gaps: &mut Vec<Gap>| {
                if let Some((from, count)) = open.take() {
                    let metres = count as f32 * KERB_STEP;
                    let middle = from + metres * 0.5;
                    let (i, at) = locate(middle);
                    let edge = segments[i].0;
                    let width = graph.edge(edge).width;
                    let across = Vec2::new(-segments[i].2.y, segments[i].2.x);
                    gaps.push(Gap {
                        metres,
                        edge,
                        name: signs
                            .per_edge
                            .get(edge.0 as usize)
                            .copied()
                            .flatten()
                            .map(|i| signs.names[i].clone()),
                        at: at + across * (side * width * 0.5),
                    });
                }
            };
            let mut along = 0.0f32;
            while along <= total {
                let (i, point) = locate(along);
                let (edge, out, axis) = segments[i];
                let clear = (head && along < JUNCTION_CLEAR)
                    || (tail && total - along < JUNCTION_CLEAR)
                    || out >= INNER;
                if clear {
                    close(&mut open, &mut gaps);
                    along += KERB_STEP;
                    continue;
                }
                let width = graph.edge(edge).width;
                let outward = Vec2::new(-axis.y, axis.x) * side;
                let kerb = point + outward * (width * 0.5);
                let (any, real) = walls.cast(kerb, outward, WALL_REACH);
                closure.all.count(any);
                closure.real.count(real);
                if out < CORE {
                    closure.all_core.count(any);
                    closure.real_core.count(real);
                }
                if any {
                    close(&mut open, &mut gaps);
                } else {
                    match &mut open {
                        Some((_, count)) => *count += 1,
                        None => open = Some((along, 1)),
                    }
                }
                along += KERB_STEP;
            }
            close(&mut open, &mut gaps);
        }
    }

    gaps.sort_by(|a, b| b.metres.total_cmp(&a.metres));
    gaps.truncate(GAPS_REPORTED);
    closure.gaps = gaps;
    closure
}

// ---------------------------------------------------------------- relief --

fn measure_relief(layout: &CityLayout, terrain: &Terrain, half_extent: f32) -> ReliefReport {
    let mut report = ReliefReport {
        datum: layout.relief.as_ref().map(|relief| relief.datum),
        ..Default::default()
    };

    if let Some(relief) = &layout.relief {
        let steps = (half_extent * 2.0 / RELIEF_GRID).ceil() as i32;
        let (mut low, mut high, mut sum, mut hill, mut count) =
            (f32::MAX, f32::MIN, 0.0f64, 0usize, 0usize);
        for iz in 0..=steps {
            for ix in 0..=steps {
                let at = Vec2::new(
                    -half_extent + ix as f32 * RELIEF_GRID,
                    -half_extent + iz as f32 * RELIEF_GRID,
                );
                let h = relief.at(at);
                low = low.min(h);
                high = high.max(h);
                sum += h as f64;
                hill += usize::from(h > HILL);
                count += 1;
            }
        }
        report.min = low;
        report.max = high;
        report.mean = (sum / count.max(1) as f64) as f32;
        report.hill_share = hill as f32 / count.max(1) as f32;
    }

    // The rule, checked the way `world::terrain`'s own test checks it: along
    // every edge, on the centreline, at the back of the pavement, and at the
    // back of the deepest plot the marcher hands out.
    let graph = &layout.graph;
    for edge in graph.edges() {
        let a = graph.node(edge.a).pos;
        let b = graph.node(edge.b).pos;
        let normal = (b - a).perp().normalize_or_zero();
        let corridor = edge.width * 0.5 + SIDEWALK_WIDTH;
        let steps = (edge.length / 4.0).ceil().max(1.0) as usize;
        for step in 0..=steps {
            let along = a.lerp(b, step as f32 / steps as f32);
            for out in [0.0, corridor, corridor + 19.0] {
                for side in [-1.0f32, 1.0] {
                    let h = terrain.height(along + normal * (out * side)).abs();
                    report.worst_under_a_street = report.worst_under_a_street.max(h);
                }
            }
        }
    }

    // And the steepest slope anywhere the player could walk to, by central
    // difference over half a grid step.
    let span = 1.2 * half_extent;
    let steps = (span * 2.0 / RELIEF_GRID).ceil() as i32;
    let h = RELIEF_GRID * 0.5;
    for iz in 0..=steps {
        for ix in 0..=steps {
            let at = Vec2::new(
                -span + ix as f32 * RELIEF_GRID,
                -span + iz as f32 * RELIEF_GRID,
            );
            let dx =
                terrain.height(at + Vec2::new(h, 0.0)) - terrain.height(at - Vec2::new(h, 0.0));
            let dz =
                terrain.height(at + Vec2::new(0.0, h)) - terrain.height(at - Vec2::new(0.0, h));
            let slope = Vec2::new(dx, dz).length() / RELIEF_GRID;
            if slope > report.steepest {
                report.steepest = slope;
                report.steepest_at = at;
            }
        }
    }
    report
}

// ------------------------------------------------------------- the table --

impl Survey {
    /// The scorecard, as one screen of aligned text.
    pub fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let pct = |share: f32| format!("{:5.1}%", share * 100.0);

        let _ = writeln!(
            out,
            "survey     {} ({:?})   seed 0x{:X}   square ±{:.0} m",
            self.city.label(),
            self.city,
            self.seed,
            self.half_extent
        );
        let _ = writeln!(out, "{}", "-".repeat(96));

        let s = &self.streets;
        let _ = writeln!(
            out,
            "streets    {:>5} edges   {:>7.1} km   {:>5} nodes   {:>4} junctions   \
             {:>3} runs clipped at the edge   {:>3} left to the hill",
            s.edges, s.km, s.nodes, s.junctions, s.clipped, s.uphill
        );
        let _ = writeln!(
            out,
            "           width      {:>8} {:>8} {:>8} {:>8} {:>8}   widest {:.1} m",
            "<=4 m", "4-6 m", "6-9 m", "9-14 m", ">14 m", s.widest
        );
        let _ = writeln!(
            out,
            "           edges      {:>8} {:>8} {:>8} {:>8} {:>8}",
            s.widths[0].0, s.widths[1].0, s.widths[2].0, s.widths[3].0, s.widths[4].0
        );
        let _ = writeln!(
            out,
            "           km         {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>8.1}",
            s.widths[0].1, s.widths[1].1, s.widths[2].1, s.widths[3].1, s.widths[4].1
        );
        let mut line = String::from("           surface   ");
        for surface in Surface::ALL {
            let km = s.surfaces[surface.index()];
            let _ = write!(
                line,
                " {} {}",
                surface_name(surface),
                pct(if s.km > 0.0 { km / s.km } else { 0.0 })
            );
        }
        let _ = writeln!(out, "{line}   (by length)");

        let b = &self.buildings;
        if b.generated > 0 {
            let _ = writeln!(
                out,
                "buildings  generated  {:>5} buildings in {:>5} blocks   (no map under this city)",
                b.generated, b.generated_blocks
            );
        } else {
            let _ = writeln!(
                out,
                "buildings  mapped     {:>5} buildings in {:>5} parts   \
                 {:>4} buildings left to the hill   {:>4} parts in a road",
                b.mapped, b.mapped_parts, b.uphill, b.in_a_road
            );
            let _ = writeln!(
                out,
                "           real       {:>5} buildings in {:>5} parts   \
                 {:>4} landmarks stand on the hill",
                b.real, b.real_parts, b.on_the_hill
            );
            let _ = writeln!(
                out,
                "           invented   {:>5} houses      {:>5} outbuildings",
                b.houses, b.outbuildings
            );
        }
        let mut line = String::from("           storeys   ");
        for (i, count) in b.storeys.iter().enumerate() {
            let _ = write!(
                line,
                " {}{}: {:>5}",
                i + 1,
                if i + 1 == b.storeys.len() { "+" } else { "" },
                count
            );
        }
        let _ = writeln!(out, "{line}   ({STOREY:.0} m a storey)");
        let mut line = String::from("           roof      ");
        for (i, (_, name)) in ROOFS.iter().enumerate() {
            let _ = write!(line, " {name} {}", b.roofs[i]);
        }
        let _ = writeln!(
            out,
            "{line}   unsaid {}   (style: {:.0}% of low houses gabled)",
            b.roof_unsaid,
            b.gables * 100.0
        );

        match &self.coverage {
            Some(c) => {
                let _ = writeln!(
                    out,
                    "coverage   bake       {} overall   {} core       built      {} overall   {} core",
                    pct(c.bake),
                    pct(c.bake_core),
                    pct(c.built),
                    pct(c.built_core)
                );
                let _ = writeln!(
                    out,
                    "           (the cut rectangles over the mapped polygons' own area; \
                     built = what survived the hill and the road)"
                );
            }
            None => {
                let _ = writeln!(out, "coverage   n/a        (no map under this city)");
            }
        }

        let c = &self.closure;
        let _ = writeln!(
            out,
            "closure    all        {} overall   {} core       real only  {} overall   {} core",
            pct(c.all.share()),
            pct(c.all_core.share()),
            pct(c.real.share()),
            pct(c.real_core.share()),
        );
        let _ = writeln!(
            out,
            "           ({} kerb points every {KERB_STEP:.0} m inside ±{:.0} m, cast {WALL_REACH:.0} m out, \
             {JUNCTION_CLEAR:.0} m clear of a junction)",
            c.all.samples,
            INNER * self.half_extent
        );
        for (i, gap) in c.gaps.iter().enumerate() {
            let _ = writeln!(
                out,
                "           {:<10} {:>5.0} m   {:<28} edge {:<5} at ({:.0}, {:.0})",
                if i == 0 { "unbuilt" } else { "" },
                gap.metres,
                gap.name.as_deref().unwrap_or("(unnamed)"),
                gap.edge.0,
                gap.at.x,
                gap.at.y
            );
        }

        let r = &self.relief;
        match r.datum {
            Some(datum) => {
                let _ = writeln!(
                    out,
                    "relief     datum {datum:.1} m   min {:+.1}   max {:+.1}   mean {:+.1}   above {HILL:.0} m (HILL) {} of the square",
                    r.min,
                    r.max,
                    r.mean,
                    pct(r.hill_share)
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "relief     n/a        (no relief under this city; the landscape is noise past the edge)"
                );
            }
        }
        let _ = writeln!(
            out,
            "           corridor   worst |height| under a street {:.3} m",
            r.worst_under_a_street
        );
        let _ = writeln!(
            out,
            "           gradient   steepest {:.3} at ({:.0}, {:.0})   \
             ({RELIEF_GRID:.0} m grid inside ±{:.0} m)",
            r.steepest,
            r.steepest_at.x,
            r.steepest_at.y,
            1.2 * self.half_extent
        );

        let mut line = String::from("timing    ");
        let mut total = 0.0;
        for (name, ms) in &self.timing {
            let _ = write!(line, " {name} {ms:.0} ms  ");
            total += ms;
        }
        let _ = writeln!(out, "{line} total {total:.0} ms");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed Landshut, at the code-default seed. `None` only when the
    /// checkout has no extract; a file that is there and does not load is a
    /// failure, the way `world::terrain`'s test treats it.
    fn landshut() -> Option<Survey> {
        if atlas::load("landshut").is_none() {
            assert!(
                !crate::core::assets::root()
                    .join("cities/landshut.ron")
                    .exists(),
                "the committed Landshut does not load as an atlas"
            );
            return None;
        }
        Some(survey(&SurveyRequest {
            city: CityStyle::Landshuepf,
            seed: GameConfig::default().world_seed,
            half_extent: GameConfig::default().world.half_extent,
        }))
    }

    /// The bar the committed Landshut holds, and the bar the next one has to.
    ///
    /// Each number here was measured first and set a little under what it
    /// measured, so a change that costs the town a tenth of its footprint or
    /// five points of street wall fails here rather than in a screenshot:
    ///
    /// - the bake keeps at least nine tenths of the mapped footprint (it
    ///   keeps 99.0 percent, and a little over that in the core, where the
    ///   rectangles overhang the irregular polygons they were cut from);
    /// - the ground under every street corridor is exactly zero, which is
    ///   the rule the whole of `world::terrain` rests on;
    /// - the street wall in the core is at least [`CORE_CLOSURE`] closed
    ///   against everything standing. Measured at 78.7 percent when the bar
    ///   was set, and the bar is that less five points, so a marcher change
    ///   may cost a house here and there but not a street;
    /// - no street is wider than the Altstadt, which the literature gives as
    ///   about thirty metres and the bake as at most thirty-four;
    /// - between a tenth and three tenths of the square is up on the hill
    ///   (18.1 percent measured): less and the Hofberg has gone, more and
    ///   the town has.
    const CORE_CLOSURE: f32 = 0.73;

    #[test]
    fn the_committed_landshut_holds_the_survey_bar() {
        let Some(survey) = landshut() else {
            return;
        };
        let card = survey.render();
        println!("{card}");

        let coverage = survey.coverage.expect("a mapped town has a coverage");
        assert!(
            coverage.bake >= 0.90,
            "the bake keeps only {:.1}% of the mapped footprint",
            coverage.bake * 100.0
        );
        assert!(
            survey.relief.worst_under_a_street < 1.0e-3,
            "the ground under a street corridor moves by {} m",
            survey.relief.worst_under_a_street
        );
        assert!(
            survey.closure.all_core.share() >= CORE_CLOSURE,
            "the core street wall is only {:.1}% closed",
            survey.closure.all_core.share() * 100.0
        );
        assert!(
            survey.streets.widest <= 34.0,
            "a street {} m wide",
            survey.streets.widest
        );
        assert!(
            (0.10..=0.30).contains(&survey.relief.hill_share),
            "{:.1}% of the square is hill",
            survey.relief.hill_share * 100.0
        );
        assert!(
            survey.buildings.on_the_hill > 0,
            "nothing stands on the Hofberg"
        );
        assert!(
            card.contains("worst |height| under a street 0.000 m"),
            "the card does not print the corridor as flat:\n{card}"
        );
    }

    /// A city with no map under it surveys too: nothing here may assume an
    /// atlas, a relief or a name for anything.
    #[test]
    fn the_survey_runs_on_a_generated_city() {
        let survey = survey(&SurveyRequest {
            city: CityStyle::Generisch,
            seed: 1,
            half_extent: 1_000.0,
        });
        assert!(survey.streets.edges > 0);
        assert!(survey.buildings.generated > 0);
        assert_eq!(survey.buildings.houses + survey.buildings.real, 0);
        assert!(survey.coverage.is_none());
        assert!(survey.relief.datum.is_none());
        assert!(survey.closure.all.samples > 0);
        assert!(
            survey.relief.worst_under_a_street < 1.0e-3,
            "the generator's streets are not flat: {} m",
            survey.relief.worst_under_a_street
        );
        let card = survey.render();
        assert!(card.contains("coverage   n/a"));
        assert!(card.contains("relief     n/a"));
    }

    /// The ray cast the street wall is measured with, on a rectangle that is
    /// not axis-aligned: the whole reason it is a slab test in the wall's own
    /// frame rather than a box overlap.
    #[test]
    fn a_ray_from_the_kerb_finds_a_turned_wall() {
        let wall = Wall {
            centre: Vec2::new(10.0, 10.0),
            axis: Vec2::new(0.6, 0.8),
            half: Vec2::new(6.0, 3.0),
            real: true,
        };
        // Straight at the middle from outside: hit. Parallel to its long side
        // and clear of it: miss. Too short to reach: miss.
        assert!(wall.struck_by(Vec2::new(-10.0, 10.0), Vec2::X, 30.0));
        assert!(!wall.struck_by(
            Vec2::new(10.0, 10.0) + wall.across() * 5.0 - wall.axis * 20.0,
            wall.axis,
            40.0
        ));
        assert!(!wall.struck_by(Vec2::new(-10.0, 10.0), Vec2::X, 5.0));
        // Starting inside counts as a hit: a kerb sample under a building
        // that stands in the road is a wall, not a hole.
        assert!(wall.struck_by(Vec2::new(10.0, 10.0), Vec2::Y, 1.0));
    }

    #[test]
    fn a_seed_is_read_the_way_the_config_writes_it() {
        assert_eq!(parse_seed("0xA17E5EED"), Some(0xA17E_5EED));
        assert_eq!(parse_seed("42"), Some(42));
        assert_eq!(parse_seed("seven"), None);
        assert_eq!(style_named("landshuepf"), Some(CityStyle::Landshuepf));
        assert_eq!(style_named("Landshüpf"), Some(CityStyle::Landshuepf));
        assert_eq!(style_named("atlantis"), None);
    }
}
