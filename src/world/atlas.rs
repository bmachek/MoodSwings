//! A real town's street plan, read off a baked OpenStreetMap extract.
//!
//! Everything else in `world` builds a city out of a seed. This builds one out
//! of a *file*, and it is the only thing in the game that does.
//!
//! ## What this reverses, and why
//!
//! `CityStyle` was written down as "a postcard, not a map", on the honest
//! grounds that a street plan matching the real Landshut needs curved blocks
//! the pipeline could not hold. Half of that is still true — the blocks are
//! still the thing that cannot be done — and the other half turned out to be
//! wrong: the *road graph* has been able to hold a real street plan all along.
//! It is a list of nodes with positions and edges with widths, its one
//! grid-shaped field is read by nothing outside the generator that fills it,
//! and the ground is already one asphalt plane with the streets carved out of
//! it rather than a mesh per road. So the graph took Landshut unchanged.
//!
//! What the blocks could not do, `frontage_lots` sidesteps rather than solves:
//! see there.
//!
//! ## Determinism
//!
//! The layout is now a pure function of `(seed, style, atlas)` rather than of
//! `(seed, style)`, and the atlas is a file that ships with the game. Chunks
//! still respawn identically, which is what the rule was protecting. What has
//! genuinely changed is that a city can now differ between *versions* — rerun
//! `tools/fetch-city.sh` a year from now and Landshut will have a new bypass —
//! which is why the baked file is committed rather than fetched at startup.
//!
//! ## Licence
//!
//! The data is © OpenStreetMap contributors, ODbL 1.0. It is the only asset in
//! this repository that is not CC0, and the attribution is in the baked file,
//! in `CREDITS.md`, and in the log line this module prints on load.

use bevy::math::Vec2;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use serde::Deserialize;

use super::citygen::{Block, Building, BuildingKind, CityLayout, Rect};
use super::roadgraph::RoadGraph;
use crate::core::config::CityStyle;

/// One town, as baked by `tools/bake-city.py`.
#[derive(Debug, Clone, Deserialize)]
pub struct Atlas {
    pub name: String,
    /// The latitude and longitude the metres are measured from. Carried so a
    /// future feature — a compass, a real sun angle — has it, and so the file
    /// says where it is of.
    pub centre: (f64, f64),
    pub streets: Vec<Street>,
    /// The town's real buildings, where they really stand.
    ///
    /// Defaulted like [`Street::surface`], so an atlas baked before any of this
    /// existed still loads as the street plan it was.
    #[serde(default)]
    pub buildings: Vec<Footprint>,
    /// Its rivers and streams.
    #[serde(default)]
    pub waters: Vec<Water>,
    /// And the ground that is not built on.
    #[serde(default)]
    pub grounds: Vec<Ground>,
    /// The shape of the ground, read off a digital elevation model.
    ///
    /// Defaulted like the rest: an atlas baked before the relief existed is
    /// still the flat town it was, with the generator's noise past its edge.
    #[serde(default)]
    pub relief: Option<Relief>,
}

/// The ground's height above the town's datum, as two grids of metres.
///
/// Baked by `tools/bake-city.py` from the Copernicus GLO-30 elevation model:
/// a fine grid over the town and a coarse one over the landscape round it,
/// both relative to `datum` — the median height of the valley floor under the
/// streets — so the town itself sits at about zero and the Hofberg south of
/// the Altstadt reads as the seventy-odd metres it is.
///
/// Values may be negative: the Isar plain north of the town is a few metres
/// below the Altstadt. What the runtime does with a negative inside the town
/// is `world::terrain`'s decision, not the file's.
#[derive(Debug, Clone, Deserialize)]
pub struct Relief {
    /// Metres above sea level the grids are measured from.
    pub datum: f32,
    /// Fine, over the town.
    pub near: Grid,
    /// Coarse, out to the horizon.
    pub far: Grid,
}

/// A regular grid of heights, row-major with rows running along +Z.
#[derive(Debug, Clone, Deserialize)]
pub struct Grid {
    /// World position of the centre of cell `(0, 0)`.
    pub origin: (f32, f32),
    /// Metres between cell centres.
    pub step: f32,
    pub cols: u32,
    pub rows: u32,
    pub values: Vec<f32>,
}

impl Grid {
    /// The height at a point, bilinear, or `None` outside the grid.
    pub fn sample(&self, at: Vec2) -> Option<f32> {
        if self.cols < 2 || self.rows < 2 || self.step <= 0.0 {
            return None;
        }
        let u = (at.x - self.origin.0) / self.step;
        let v = (at.y - self.origin.1) / self.step;
        let (last_u, last_v) = (self.cols as f32 - 1.0, self.rows as f32 - 1.0);
        if u < 0.0 || v < 0.0 || u > last_u || v > last_v {
            return None;
        }
        let (u0, v0) = (u.floor().min(last_u - 1.0), v.floor().min(last_v - 1.0));
        let (fu, fv) = (u - u0, v - v0);
        let (c, r) = (u0 as usize, v0 as usize);
        let cols = self.cols as usize;
        let cell = |r: usize, c: usize| self.values.get(r * cols + c).copied().unwrap_or(0.0);
        let top = cell(r, c) * (1.0 - fu) + cell(r, c + 1) * fu;
        let bottom = cell(r + 1, c) * (1.0 - fu) + cell(r + 1, c + 1) * fu;
        Some(top * (1.0 - fv) + bottom * fv)
    }

    /// The same, clamped to the border rather than refusing.
    pub fn sample_clamped(&self, at: Vec2) -> f32 {
        let last = Vec2::new(
            self.origin.0 + (self.cols.max(1) - 1) as f32 * self.step,
            self.origin.1 + (self.rows.max(1) - 1) as f32 * self.step,
        );
        let inside = at.clamp(Vec2::new(self.origin.0, self.origin.1), last);
        self.sample(inside).unwrap_or(0.0)
    }

    /// Whether the grid is the size it says it is.
    pub fn is_sound(&self) -> bool {
        self.values.len() == (self.cols as usize) * (self.rows as usize)
            && self.cols >= 2
            && self.rows >= 2
            && self.step > 0.0
    }
}

impl Relief {
    /// Height above the datum at a point: the fine grid where it reaches, the
    /// coarse one beyond, and the coarse grid's edge beyond that.
    pub fn at(&self, at: Vec2) -> f32 {
        self.near
            .sample(at)
            .unwrap_or_else(|| self.far.sample_clamped(at))
    }

    /// Whether a point is up on the hill rather than down on the valley
    /// floor the town is built on — see [`HILL`].
    pub fn is_hill(&self, at: Vec2) -> bool {
        self.at(at) > HILL
    }
}

/// Metres above the datum at which ground stops being the town.
///
/// The one place the relief and the town's oldest rule meet. `world::terrain`
/// holds the ground at exactly zero wherever a street runs, because thirty
/// spawners write a y off that; so a street the map puts sixty metres up the
/// Hofberg cannot be built there — it would be a flat trench with a cliff
/// either side. Above this line the street is left out and the hill takes its
/// place, wooded, which is what the north face of the Hofberg looks like from
/// the Altstadt anyway. Below it the ground under a street is flattened to the
/// floor and nobody can tell: the valley is flat to within a few metres over a
/// kilometre.
///
/// Twelve, because Landshut's floor is bimodal. Measured along the streets of
/// the extract, 85 percent lie within eight metres of the datum and the rest
/// are thirty to ninety-six above it; nothing much lives in between.
pub const HILL: f32 = 12.0;

/// One building, as the smallest rotated rectangle that contains it.
///
/// A rectangle rather than the polygon it came from, and that is not a
/// concession — it is the shape [`crate::world::citygen::Building`] already is.
/// A footprint there is read as `frontage x depth` in the building's own frame
/// with a `facing` yaw, precisely because a town read off a map has no blocks
/// and no four sides to choose between. So the bake reduces each OSM way to its
/// minimum-area enclosing rectangle and the whole of it drops into the existing
/// mesh path: no new geometry, no polygon extrusion, and every shell, gable,
/// sign, doorway and chimney follows the same yaw it always did.
///
/// What that gives up is the courtyard in a ring-shaped block. The bake throws
/// away any footprint that fills less than about half its own box for that
/// reason: a rectangle stamped over a courtyard block is a solid lump where a
/// courtyard should be, and an invented terrace is better than a wrong solid.
#[derive(Debug, Clone, Deserialize)]
pub struct Footprint {
    /// What the town calls it, where the town calls it anything.
    ///
    /// Only landmarks carry one — the two thousand houses that make up the
    /// street wall are anonymous, and a town where every building announces
    /// itself is an airport. Empty for those.
    #[serde(default)]
    pub name: String,
    pub centre: (f32, f32),
    /// Which way the frontage runs, in the same convention `Building::facing`
    /// uses: `+Z` out across the pavement.
    pub yaw: f32,
    pub frontage: f32,
    pub depth: f32,
    /// What the mappers measured or counted, in metres. `None` where they did
    /// neither, and then the city style decides as it always has — four fifths
    /// of this town is `None`.
    pub height: Option<f32>,
    /// What the tags say it is for, where they say anything the game draws
    /// differently.
    pub kind: Option<crate::world::citygen::BuildingKind>,
    /// Which building this is a part of.
    ///
    /// A building is no longer one box. The bake cuts each polygon into the
    /// rectangles that cover it — an L is two, a courtyard block four — and
    /// emits each as its own footprint carrying the same group; the runtime
    /// gathers them back into one block sharing a height, a palette and a
    /// kind. `None` is an atlas baked before parts existed, where every
    /// footprint is its own building.
    #[serde(default)]
    pub group: Option<u32>,
    /// The true area of the polygon the parts were cut from, in square
    /// metres, repeated on every part. What the survey measures coverage
    /// against; zero where the bake did not know.
    #[serde(default)]
    pub area: f32,
    /// What the map says the roof is, where it says anything.
    #[serde(default)]
    pub roof: Option<crate::world::citygen::RoofShape>,
    /// Storeys the mappers counted, where they counted them.
    #[serde(default)]
    pub levels: Option<u8>,
}

/// A river, a stream or a mill race.
#[derive(Debug, Clone, Deserialize)]
pub struct Water {
    pub name: String,
    pub width: f32,
    pub points: Vec<(f32, f32)>,
}

/// A piece of ground that is not built on and is not a street.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum GroundKind {
    Grass,
    Park,
    Cemetery,
    Trees,
    Allotments,
    Field,
    Pitch,
    Playground,
    Parking,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ground {
    pub kind: GroundKind,
    pub points: Vec<(f32, f32)>,
}

/// What a street is paved with.
///
/// A fifth of Landshut is not asphalt, and the bake used to throw that away.
/// The Altstadt is `sett` — the dressed granite Kopfsteinpflaster a Bavarian
/// market street has been laid in since it was a market — and the Neustadt is
/// sawn `paving_stones`; between them that is a hundred and thirty-six ways of
/// the extract, and they are the two streets the town is known for.
///
/// Four values rather than OSM's forty. The game draws a carriageway, and the
/// question a carriageway asks is which of four materials, not which of the
/// nine words a mapper might have used for gravel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum Surface {
    #[default]
    Asphalt,
    /// Kopfsteinpflaster: setts, laid in fans.
    Sett,
    /// Sawn rectangular slabs.
    Slabs,
    /// A lane that was never surfaced.
    Gravel,
}

impl Surface {
    pub const ALL: [Self; 4] = [Self::Asphalt, Self::Sett, Self::Slabs, Self::Gravel];

    /// Its slot in the tables that are indexed by surface. Written out rather
    /// than derived from the discriminant, so reordering the enum cannot
    /// silently repave the town.
    pub fn index(self) -> usize {
        match self {
            Self::Asphalt => 0,
            Self::Sett => 1,
            Self::Slabs => 2,
            Self::Gravel => 3,
        }
    }
}

/// One way out of the extract: a polyline in metres, with a carriageway width.
#[derive(Debug, Clone, Deserialize)]
pub struct Street {
    pub name: String,
    pub width: f32,
    pub arterial: bool,
    /// Defaulted, so an atlas baked before surfaces existed still loads as the
    /// asphalt town it was.
    #[serde(default)]
    pub surface: Surface,
    pub points: Vec<(f32, f32)>,
    /// Whether the bake folded several parallel ways into this one — the
    /// Altstadt's carriageway, parking lanes and pedestrian halves as one
    /// street as wide as the band they covered. The game does not read it;
    /// it is carried so a re-bake from this file knows which streets to
    /// measure wall to wall rather than to the nearest kerb.
    #[serde(default)]
    pub band: bool,
}

/// How close two points have to be to be the same junction, in metres.
///
/// Small: the baker emits a decimetre of precision and a shared OSM node comes
/// out of the projection bit-for-bit identical in both ways that use it, so
/// this only has to survive the rounding. Anything larger starts welding
/// genuinely separate kerbs on a dual carriageway into one line.
const WELD: f32 = 0.35;

/// Shortest edge worth keeping, in metres.
///
/// Under this the two ends weld to the same node anyway, and what is left is a
/// self-loop that every downstream consumer — traffic, the minimap, the
/// furniture spawners — has to be careful about.
const SHORTEST: f32 = 1.2;

/// Reads a town off disk.
///
/// Returns `None` rather than panicking on anything: a missing or unreadable
/// atlas is a city that falls back to the generator with a warning, which is
/// the same discipline `audio::files` applies to a missing recording. A
/// half-built town is worse than a generated one.
pub fn load(name: &str) -> Option<Atlas> {
    let path = crate::core::assets::root()
        .join("cities")
        .join(format!("{name}.ron"));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            warn!("no atlas at {}: {error}", path.display());
            return None;
        }
    };
    match ron::from_str::<Atlas>(&text) {
        Ok(atlas) => Some(atlas),
        Err(error) => {
            warn!("{} is not an atlas: {error}", path.display());
            None
        }
    }
}

/// The names of a town's streets, and which edge wears which.
///
/// Kept beside the layout rather than inside `RoadEdge`, because a name is a
/// fact about a *street* and an edge is one segment of one: a curved road is a
/// dozen edges and one name. The generator has no names at all and gets an
/// empty one of these.
#[derive(Resource, Default)]
pub struct Signposts {
    /// Distinct names, in the order the plates are painted in.
    pub names: Vec<String>,
    /// Which name each `EdgeId` wears, if it wears one.
    pub per_edge: Vec<Option<usize>>,
}

/// Builds a layout out of a town.
///
/// The blocks come out empty and that is not an oversight — see
/// `frontage_lots`, which is what fills a real city instead.
/// The ground the map says is not built on.
///
/// Kept whole rather than clipped: a park is a ring and half a ring is not a
/// smaller park, it is an open curve that a point-in-polygon test reads as
/// nonsense. Anything whose whole outline is outside the square is dropped and
/// the rest is left as it is — the planting inside it is clipped to the chunk
/// instead, which is where the clipping belongs.
fn open_ground(atlas: &Atlas, half_extent: f32) -> Vec<super::citygen::OpenGround> {
    let mut out = Vec::new();
    for ground in &atlas.grounds {
        let points: Vec<Vec2> = ground
            .points
            .iter()
            .map(|&(x, z)| Vec2::new(x, z))
            .collect();
        if points.len() < 4 {
            continue;
        }
        if !points
            .iter()
            .any(|at| at.x.abs() <= half_extent && at.y.abs() <= half_extent)
        {
            continue;
        }
        let (mut low, mut high) = (points[0], points[0]);
        for at in &points {
            low = low.min(*at);
            high = high.max(*at);
        }
        out.push(super::citygen::OpenGround {
            kind: ground.kind,
            points,
            bounds: Rect::new(low, high),
        });
    }
    out
}

/// Turns a footprint to face the street it stands on.
///
/// Returns the yaw and the frontage and depth that go with it: fronting the
/// short side of a box means the two swap, because `Building::footprint` is
/// read as `frontage x depth` in the building's own frame.
///
/// The convention `buildings::site_in` reads is that a yaw of theta sends the
/// building's local `+Z` outward, across the pavement — so the front is the
/// side whose outward normal has the largest dot with the direction of the
/// nearest road.
fn facing_the_street(graph: &RoadGraph, centre: Vec2, plot: &Footprint) -> (f32, f32, f32) {
    let Some(towards) = nearest_street(graph, centre) else {
        return (plot.yaw, plot.frontage, plot.depth);
    };
    let quarter = std::f32::consts::FRAC_PI_2;
    let mut best = (plot.yaw, plot.frontage, plot.depth);
    let mut score = f32::MIN;
    for turn in 0..4 {
        let yaw = plot.yaw + quarter * turn as f32;
        let outward = Vec2::new(yaw.sin(), yaw.cos());
        let facing = outward.dot(towards);
        if facing > score {
            score = facing;
            // A quarter or three quarters of a turn swaps which side is the
            // frontage.
            best = if turn % 2 == 0 {
                (yaw, plot.frontage, plot.depth)
            } else {
                (yaw, plot.depth, plot.frontage)
            };
        }
    }
    best
}

/// The direction from `at` to the nearest carriageway, if there is one near.
fn nearest_street(graph: &RoadGraph, at: Vec2) -> Option<Vec2> {
    // How far to look. Past this a building is not on a street in any sense
    // that would decide which way its front door is.
    const REACH: f32 = 60.0;
    street_within(graph, at, REACH).map(|(_, towards)| towards)
}

/// The nearest carriageway within `reach` of `at`: how far to its edge, and
/// which way it lies.
fn street_within(graph: &RoadGraph, at: Vec2, reach: f32) -> Option<(f32, Vec2)> {
    let mut best = (reach, Vec2::ZERO);
    for edge in graph.edges() {
        let (a, b) = (graph.node(edge.a).pos, graph.node(edge.b).pos);
        let span = b - a;
        let length = span.length_squared();
        if length < 1e-6 {
            continue;
        }
        let t = ((at - a).dot(span) / length).clamp(0.0, 1.0);
        let foot = a + span * t;
        // To the kerb, not the centreline: what the terrain holds level
        // runs from the pavement's edge outward, and so should this.
        let away = (foot - at).length() - edge.width * 0.5 - super::citygen::SIDEWALK_WIDTH;
        if away < best.0 {
            best = (away, foot - at);
        }
    }
    (best.0 < reach).then(|| (best.0.max(0.0), best.1.normalize_or_zero()))
}

/// The town's water, clipped to the square the game builds.
///
/// Clipped the way a street is — an arm that leaves and comes back is two
/// runs, not one bridged across the outside — because the alternative is a
/// river drawn straight through a kilometre of town it never touches.
fn waters(atlas: &Atlas, half_extent: f32) -> Vec<super::citygen::Waterway> {
    let mut out = Vec::new();
    for water in &atlas.waters {
        let mut run: Vec<Vec2> = Vec::new();
        for &(x, z) in &water.points {
            if x.abs() > half_extent || z.abs() > half_extent {
                if run.len() >= 2 {
                    out.push(super::citygen::Waterway {
                        name: water.name.clone(),
                        width: water.width,
                        points: std::mem::take(&mut run),
                    });
                } else {
                    run.clear();
                }
                continue;
            }
            run.push(Vec2::new(x, z));
        }
        if run.len() >= 2 {
            out.push(super::citygen::Waterway {
                name: water.name.clone(),
                width: water.width,
                points: run,
            });
        }
    }
    out
}

/// The town's real buildings, turned into the layout's own blocks.
///
/// One block per building, which is what `streetside` already produces for an
/// atlas city: a real block is not a rectangle and nothing downstream wants one
/// to be. A building is one or more parts — the rectangles the bake cut its
/// polygon into, gathered back together by their `group` — and the block holds
/// all of them, sharing one height, one palette and one kind, each part
/// turned to face the street nearest *it*: an L-shaped house on a corner
/// fronts both its streets, which is what an L-shaped house on a corner does.
///
/// Anything whose middle is outside the square the game builds is dropped, the
/// same rule the streets take; so is anything up on the hill — see [`HILL`] —
/// unless it is a landmark, which is kept and stood on the relief.
pub fn footprints(
    atlas: &Atlas,
    graph: &RoadGraph,
    seed: u64,
    half_extent: f32,
    style: CityStyle,
) -> Vec<Block> {
    use crate::core::rng::{stream, stream_for};
    use rand::RngExt;

    // Its own stream, drawn after nothing and before nothing: a palette or a
    // storey height taken here must not move a single invented terrace, and the
    // terraces draw from `stream::BUILDINGS`. One draw of each per *building*,
    // not per part, so cutting a polygon finer does not re-palette the town.
    let mut rng = stream_for(seed, stream::ATLAS);
    let relief = atlas.relief.as_ref();
    let mut blocks = Vec::new();
    let mut uphill = 0usize;

    // Gather the parts of each building. They are emitted together, largest
    // first, so a group is a run of consecutive footprints; an atlas from
    // before parts existed carries no group and every footprint is its own.
    let mut index = 0usize;
    while index < atlas.buildings.len() {
        let first = &atlas.buildings[index];
        let mut last = index + 1;
        if first.group.is_some() {
            while last < atlas.buildings.len() && atlas.buildings[last].group == first.group {
                last += 1;
            }
        }
        let parts = &atlas.buildings[index..last];
        index = last;

        // The building stands where its largest part does: that is the part
        // that decides which side of the square, which district and which
        // height band it belongs to.
        let centre = Vec2::new(first.centre.0, first.centre.1);
        if centre.x.abs() > half_extent || centre.y.abs() > half_extent {
            continue;
        }
        let landmark = !first.name.is_empty();
        // Up on the hill the town is not built; a landmark is, at the height
        // the hill puts it. The ground under the whole building is one
        // height — the highest of its parts' — so a castle wing does not step
        // down its own slope; the terrain raises a plateau to meet it.
        //
        // Unless a street still holds the ground under it. The terrain keeps
        // everything within `terrain::LEVEL_REACH` of a pavement at exactly
        // zero and fades to the hill over `LEVEL_FADE` past that, and a
        // landmark any part of which stands inside that reach is on the
        // street's ground, not the hill's: the hill has been cut away round
        // the street, and the building goes with the street. So the test is
        // from the building's farthest corner, not its middle.
        let ground = match relief {
            Some(relief) if relief.is_hill(centre) => {
                if !landmark {
                    uphill += 1;
                    continue;
                }
                let spread = parts
                    .iter()
                    .map(|part| {
                        let at = Vec2::new(part.centre.0, part.centre.1);
                        at.distance(centre) + Vec2::new(part.frontage, part.depth).length() * 0.5
                    })
                    .fold(0.0f32, f32::max);
                let held = super::terrain::LEVEL_REACH + super::terrain::LEVEL_FADE + spread;
                if street_within(graph, centre, held).is_some() {
                    0.0
                } else {
                    parts
                        .iter()
                        .map(|part| relief.at(Vec2::new(part.centre.0, part.centre.1)))
                        .fold(0.0f32, f32::max)
                }
            }
            _ => 0.0,
        };

        let district = super::streetside::district_at(centre, half_extent, false);
        // What the mappers counted, where they counted it. Where they did not —
        // four buildings in five — the style decides, exactly as it does for an
        // invented one, so a Landshut townhouse is a Landshut townhouse whether
        // or not somebody typed its storeys into OSM.
        let (low, high) = style.heights(district.height_range());
        // A landmark's measured height is believed as it stands. Everything
        // else is clamped into the style's band, because an OSM `height` on an
        // ordinary house is as often the ridge as the eaves and as often a typo
        // as either — but St. Martin really is a hundred and thirty metres, and
        // clamping that to a Landshut townhouse is how a town loses its tower.
        let height = first
            .height
            .map(|metres| {
                if landmark {
                    metres
                } else {
                    metres.clamp(low.min(high), high.max(low) * 1.6)
                }
            })
            .unwrap_or_else(|| match first.kind {
                // A town-wall tower is not a thin house. The map gives its
                // footprint and almost never its height, and a masonry tower
                // runs about four times its own base: Landshut's are four to
                // seven metres square and fifteen to twenty-five tall. Six
                // times, which this said first, stood the Hungerturm up on
                // the Hofberg like a thirty-metre office block.
                Some(BuildingKind::Tower) => {
                    (first.frontage.min(first.depth) * 4.0).clamp(12.0, 26.0)
                }
                // A gate carries a room over the arch and crenellations over
                // that. The Ländtor is the one the map measured, at ten.
                Some(BuildingKind::Gate) => 14.0,
                _ => rng.random_range(low..high),
            });
        let palette = rng.random_range(0..super::citygen::PALETTE_SIZE);
        let kind = first.kind.unwrap_or(BuildingKind::Apartments);

        let mut buildings = Vec::with_capacity(parts.len());
        let (mut low_corner, mut high_corner) = (Vec2::MAX, Vec2::MIN);
        for part in parts {
            let centre = Vec2::new(part.centre.0, part.centre.1);
            // Which way it faces, which the bake cannot know and this can.
            //
            // A rectangle has two axes and the baker puts the frontage on the
            // longer one, because a plot is usually wider on the street than
            // it is deep. Usually is not always, and the sign is arbitrary
            // either way — so a quarter of the town wore its front on its
            // flank and another quarter on its back. What that draws is a
            // Giebelhaus with its stepped screen standing off to one side of
            // its own roof, which is what a wrong Dachfront is.
            //
            // The street knows. Take the four sides the box can present, and
            // front the one whose outward normal best points at the nearest
            // carriageway — per part, because a back wing has a back street.
            let (yaw, frontage, depth) = facing_the_street(graph, centre, part);
            let half = Vec2::new(frontage, depth) * 0.5;
            let reach = Vec2::splat(half.length());
            low_corner = low_corner.min(centre - reach);
            high_corner = high_corner.max(centre + reach);
            buildings.push(Building {
                footprint: Rect::new(centre - half, centre + half),
                facing: Some(yaw),
                height,
                palette,
                kind,
                roof: part.roof,
                ground,
            });
        }
        blocks.push(Block {
            // Only ever read for filing this into a chunk and for the minimap,
            // both of which want a world box round the whole building.
            area: Rect::new(low_corner, high_corner),
            paved: false,
            district,
            buildings,
            vacants: Vec::new(),
            arterial: [false; 4],
            quarter: None,
        });
    }
    if uphill > 0 {
        info!("{uphill} of the town's buildings stand up on the hill and are left to it");
    }
    blocks
}

pub fn layout(atlas: &Atlas, seed: u64, half_extent: f32) -> (CityLayout, Signposts) {
    let mut graph = RoadGraph::default();
    // A relief that is not the size it claims is a bake gone wrong, and a
    // wrong grid sampled as heights is a town on a cliff. Left out, with a
    // warning, the town is the flat one it was.
    let relief = atlas.relief.as_ref().filter(|relief| {
        let sound = relief.near.is_sound() && relief.far.is_sound();
        if !sound {
            warn!(
                "{}: the relief grids are not the size they say; ignored",
                atlas.name
            );
        }
        sound
    });
    // Junction welding: two ways that share an OSM node project to the same
    // metre, so quantising to a decimetre and looking up is enough to turn five
    // hundred loose polylines into one connected network.
    let mut welded: HashMap<(i32, i32), super::roadgraph::NodeId> = HashMap::default();
    let key = |at: Vec2| ((at.x / WELD).round() as i32, (at.y / WELD).round() as i32);

    let mut signs = Signposts::default();
    // Distinct names, so a hundred and sixty-three plates are painted for a
    // town with two thousand streets in it.
    let mut named: HashMap<&str, usize> = HashMap::default();

    let mut clipped = 0usize;
    let mut uphill = 0usize;
    for street in &atlas.streets {
        let name = (!street.name.is_empty()).then(|| {
            *named.entry(street.name.as_str()).or_insert_with(|| {
                signs.names.push(street.name.clone());
                signs.names.len() - 1
            })
        });
        let mut previous: Option<(Vec2, super::roadgraph::NodeId)> = None;
        for &(x, z) in &street.points {
            let at = Vec2::new(x, z);
            // Outside the square the game builds, the street simply stops. A
            // way that leaves and comes back — a ring road clipping a corner —
            // is left as two runs rather than joined across the outside, which
            // is what `previous = None` does here.
            if at.x.abs() > half_extent || at.y.abs() > half_extent {
                if previous.is_some() {
                    clipped += 1;
                }
                previous = None;
                continue;
            }
            // And where the map takes it up the hill, the street stops the
            // same way. The rule is written down at [`HILL`]: the ground under
            // a street is held dead level, so a street sixty metres up the
            // Hofberg is one the game cannot build without cutting the hill
            // away round it. What the game builds instead is the hill.
            if relief.is_some_and(|relief| relief.is_hill(at)) {
                uphill += 1;
                previous = None;
                continue;
            }

            let node = *welded.entry(key(at)).or_insert_with(|| {
                // The grid key is what the generator uses to find the junction
                // at a crossing of two of its own street lists. A real town has
                // no such lists, so every node here gets its own key and
                // `node_at_grid` simply never finds anything — which is fine,
                // because only the generator ever asks.
                let index = graph.node_count() as u32;
                graph.add_node(at, ((index >> 16) as u16, index as u16))
            });

            if let Some((before, from)) = previous
                && from != node
                && before.distance(at) >= SHORTEST
            {
                graph.connect(from, node, street.width, street.arterial, street.surface);
                signs.per_edge.push(name);
            }
            previous = Some((at, node));
        }
    }

    info!(
        "{}: {} streets under {} names, {} junctions, {} roads \
         ({clipped} runs clipped at the edge, {uphill} points left to the hill) \
         — map data (c) OpenStreetMap contributors, ODbL 1.0",
        atlas.name,
        atlas.streets.len(),
        signs.names.len(),
        graph.node_count(),
        graph.edge_count(),
    );

    (
        CityLayout {
            seed,
            half_extent,
            // A real town has no street lists: those are the generator's two axes
            // of its own grid, and nothing outside it reads them.
            x_streets: Vec::new(),
            z_streets: Vec::new(),
            blocks: Vec::new(),
            graph,
            // A canal here is one street of a grid surrendered to water:
            // straight, axis-aligned, and found by walking two street lists a
            // town read off a map does not have. The Isar is a braided river
            // that goes where it goes, so it is not one of these and never
            // could have been — it is a `waters` entry, below, and the reason
            // this said `None` with an apology attached for as long as it did.
            canal: None,
            grounds: open_ground(atlas, half_extent),
            waters: waters(atlas, half_extent),
            relief: relief.cloned(),
        },
        signs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn town(streets: Vec<Street>) -> Atlas {
        Atlas {
            name: "Test".into(),
            centre: (0.0, 0.0),
            streets,
            buildings: Vec::new(),
            waters: Vec::new(),
            grounds: Vec::new(),
            relief: None,
        }
    }

    fn street(points: &[(f32, f32)]) -> Street {
        Street {
            name: "Teststraße".into(),
            width: 8.0,
            arterial: false,
            surface: Surface::Asphalt,
            points: points.to_vec(),
            band: false,
        }
    }

    /// The committed extract is the one asset in the repository that is data
    /// rather than code, and every field of it is a promise to a match arm
    /// somewhere. Nothing else in the test suite would notice if a re-bake
    /// changed the shape of the file, or if `surface` quietly went back to
    /// defaulting because the baker stopped writing it.
    /// The committed extract, or nothing: a checkout without the file is not
    /// a failure, but a file that is there and does not parse is — `load`
    /// answers `None` to both, and for as long as every test here returned
    /// early on `None`, a bake that wrote a field the runtime could not read
    /// passed every one of them while the game fell back to the generator.
    fn committed() -> Option<Atlas> {
        let path = crate::core::assets::root().join("cities/landshut.ron");
        let town = load("landshut");
        assert!(
            town.is_some() || !path.exists(),
            "{} is on disk and does not load as an atlas",
            path.display()
        );
        town
    }

    #[test]
    fn the_committed_landshut_still_says_what_it_is_paved_with() {
        let Some(town) = committed() else {
            // A checkout without the extract is not a failure; the game falls
            // back to the generator and says so.
            return;
        };
        assert!(town.streets.len() > 300, "{} streets", town.streets.len());

        // One street, not the four ways the mappers drew it as. The Altstadt is
        // twenty-two ways in the raw extract -- carriageway, parking lanes and
        // pedestrian halves each mapped separately -- and the runtime draws
        // every one of them as a full street with two pavements. The bake folds
        // ways of one name that run alongside each other into one as wide as
        // the band they cover, so what arrives here is a market square rather
        // than nine parallel stripes of paving with kerbs marooned between
        // them.
        let altstadt: Vec<&Street> = town
            .streets
            .iter()
            .filter(|street| street.name == "Altstadt")
            .collect();
        assert!(
            altstadt.len() < 12,
            "the Altstadt is still {} separate ways",
            altstadt.len()
        );
        let broadest = altstadt
            .iter()
            .map(|street| street.width)
            .fold(0.0f32, f32::max);
        // The literature gives it as about thirty metres wide, which is what
        // makes it a Platz rather than a road — wall to wall. The bake takes
        // the pavement off each side of that, because the width here is the
        // carriageway and the game lays its own pavements beside it.
        assert!(
            (13.0..=34.0).contains(&broadest),
            "the Altstadt's widest way is {broadest} m; the town says thirty"
        );

        let widths: Vec<f32> = town.streets.iter().map(|street| street.width).collect();
        let narrowest = widths.iter().copied().fold(f32::MAX, f32::min);
        let widest = widths.iter().copied().fold(0.0, f32::max);
        // Real widths, off `width` and `lanes` tags. Before those were read the
        // whole town came off a table of six class defaults, and three streets
        // in five were the identical 7.5m.
        assert!(
            widest - narrowest > 6.0,
            "every street is about {narrowest}m wide"
        );

        let setts = town
            .streets
            .iter()
            .filter(|street| street.surface == Surface::Sett)
            .count();
        assert!(setts > 50, "only {setts} cobbled streets in Landshut");
        assert!(
            town.streets
                .iter()
                .any(|street| street.name == "Altstadt" && street.surface == Surface::Sett),
            "the Altstadt is not cobbled"
        );
    }

    /// The extract now carries more than its streets, and every one of those
    /// fields is a promise to a match arm somewhere. Nothing else in the suite
    /// would notice a re-bake that quietly stopped emitting buildings, or one
    /// that started emitting a retail shed the length of a street.
    #[test]
    fn the_committed_landshut_stands_its_own_buildings_on_its_own_ground() {
        let Some(town) = committed() else {
            return;
        };
        assert!(
            town.buildings.len() > 2000,
            "only {} of Landshut's buildings came off the map",
            town.buildings.len()
        );

        // A townhouse, not a shed and not a shopping centre. The median plot in
        // this town is about 17 m by 11; the bake drops anything under 24 m² as
        // a bin store, and cuts anything longer than sixty metres into parts
        // no longer than forty, because the game draws a part as one box with
        // one roof and a box the length of a street is a wall.
        //
        // A *landmark* is exempt from that, and has to be: St. Martin is
        // ninety-one metres long and the Stadtresidenz fifty-nine deep, so the
        // limits that keep a retail shed from being drawn as a box the length
        // of a street were throwing away exactly the buildings the town is
        // known for.
        for plot in &town.buildings {
            // A church, a gate or a tower is the one box its module raises its
            // own anatomy on, cut for nothing; everything else is a part.
            use crate::world::citygen::BuildingKind as Kind;
            let whole = matches!(
                plot.kind,
                Some(
                    Kind::Church
                        | Kind::Cathedral
                        | Kind::Gate
                        | Kind::Tower
                        | Kind::Stadium
                        | Kind::ParkingGarage
                )
            );
            let (longest, deepest) = if plot.name.is_empty() && !whole {
                (60.5, 60.5)
            } else {
                (130.0, 70.0)
            };
            assert!(
                plot.frontage >= 1.0 && plot.frontage <= longest,
                "a {} m frontage at {:?} ({})",
                plot.frontage,
                plot.centre,
                plot.name
            );
            assert!(
                plot.depth >= 1.0 && plot.depth <= deepest,
                "{} m deep",
                plot.depth
            );
            assert!(plot.frontage >= plot.depth, "a plot deeper than it is wide");
            if let Some(height) = plot.height {
                assert!((2.0..=140.0).contains(&height), "{height} m tall");
            }
        }

        // The buildings come in parts now, and the parts of one building
        // arrive together and say the same thing about what they are part of.
        let groups = town
            .buildings
            .iter()
            .filter_map(|plot| plot.group)
            .collect::<std::collections::HashSet<_>>();
        assert!(
            groups.len() > 2_000,
            "only {} buildings in {} parts",
            groups.len(),
            town.buildings.len()
        );
        assert!(
            town.buildings.len() > groups.len() + 400,
            "{} parts for {} buildings: nothing was cut into an L or a courtyard",
            town.buildings.len(),
            groups.len()
        );
        for pair in town.buildings.windows(2) {
            if pair[0].group.is_some() && pair[0].group == pair[1].group {
                assert_eq!(pair[0].name, pair[1].name);
                assert_eq!(pair[0].height, pair[1].height);
                assert_eq!(pair[0].kind, pair[1].kind);
                assert_eq!(pair[0].area, pair[1].area);
                // Largest first: the first part is the one that stands for
                // the building.
                assert!(
                    pair[0].frontage * pair[0].depth >= pair[1].frontage * pair[1].depth * 0.999,
                    "a group whose first part is not its largest"
                );
            }
        }
        for plot in &town.buildings {
            assert!(plot.area > 0.0, "a part with no polygon behind it");
        }
        // And the map's roofs came too: Landshut is a gabled town where
        // anybody bothered to say.
        let gabled = town
            .buildings
            .iter()
            .filter(|plot| plot.roof == Some(crate::world::citygen::RoofShape::Gabled))
            .count();
        assert!(gabled > 100, "only {gabled} roofs the map called gabled");

        // The town is a skyline before it is a street plan, and the skyline is
        // one building: the tallest brick tower in the world, at the south end
        // of the Altstadt. Losing it to a size limit is how Landshut stops
        // being Landshut.
        let martin = town
            .buildings
            .iter()
            .find(|plot| plot.name == "Basilika Sankt Martin")
            .expect("St. Martin is not in the atlas");
        assert_eq!(martin.height, Some(130.6));
        assert!(
            martin.frontage > 85.0,
            "St. Martin is only {} m long; the literature says 92",
            martin.frontage
        );
        assert_eq!(
            martin.kind,
            Some(crate::world::citygen::BuildingKind::Church)
        );

        // And the rest of what a person walks across town to look at. Only a
        // landmark is named — the five hundred listed townhouses that make up
        // an Altstadt street wall are not, because a name here means the
        // height is believed as it stands.
        let named = town.buildings.iter().filter(|plot| !plot.name.is_empty());
        let mut kinds = std::collections::HashMap::new();
        for plot in named {
            *kinds.entry(plot.kind).or_insert(0usize) += 1;
        }
        use crate::world::citygen::BuildingKind::{Church, Gate, Tower};
        for (kind, least) in [(Church, 12), (Tower, 5), (Gate, 3)] {
            let count = kinds.get(&Some(kind)).copied().unwrap_or(0);
            assert!(count >= least, "only {count} of {kind:?} in Landshut");
        }
        let total: usize = kinds.values().sum();
        assert!(
            (40..400).contains(&total),
            "{total} named landmarks, which is either a town with none or one \
             where every listed house counts as one"
        );

        // Landshut is a town on a braided river and the atlas has to know it.
        // Three arms, each better than three kilometres inside the square.
        for arm in ["Isar", "Große Isar", "Kleine Isar"] {
            let run: f32 = town
                .waters
                .iter()
                .filter(|water| water.name == arm)
                .flat_map(|water| {
                    water
                        .points
                        .windows(2)
                        .map(|p| Vec2::new(p[0].0, p[0].1).distance(Vec2::new(p[1].0, p[1].1)))
                        .collect::<Vec<_>>()
                })
                .sum();
            assert!(run > 2_000.0, "the {arm} is only {run} m long");
        }
        // And a river is drawn as a band, not as a canal: the bake densifies to
        // twelve metres and smooths, so no run may be a long straight.
        for water in &town.waters {
            for pair in water.points.windows(2) {
                let step =
                    Vec2::new(pair[0].0, pair[0].1).distance(Vec2::new(pair[1].0, pair[1].1));
                assert!(step < 20.0, "a {step} m straight in the {}", water.name);
            }
        }

        assert!(
            town.grounds.len() > 40,
            "only {} pieces of open ground",
            town.grounds.len()
        );
    }

    /// The extract carries the shape of the ground, and the shape is
    /// Landshut's: a flat valley floor with the Hofberg south of the Altstadt.
    #[test]
    fn the_committed_landshut_stands_in_its_valley() {
        let Some(town) = committed() else {
            return;
        };
        let relief = town.relief.as_ref().expect("no relief in the atlas");
        assert!(relief.near.is_sound() && relief.far.is_sound());
        // The datum is the valley floor, about three hundred and ninety-five
        // metres above the sea.
        assert!(
            (380.0..=410.0).contains(&relief.datum),
            "datum {} m",
            relief.datum
        );
        // The Altstadt is on the floor.
        let altstadt = relief.at(Vec2::new(50.0, 150.0));
        assert!(
            altstadt.abs() < 6.0,
            "the Altstadt is {altstadt} m off the floor"
        );
        assert!(!relief.is_hill(Vec2::new(50.0, 150.0)));
        // The castle hill is not.
        let trausnitz = relief.at(Vec2::new(150.0, 680.0));
        assert!(
            trausnitz > 25.0,
            "the Hofberg under Trausnitz is only {trausnitz} m up"
        );
        assert!(relief.is_hill(Vec2::new(150.0, 680.0)));
        // Most of the town is floor.
        let mut floor = 0usize;
        let mut total = 0usize;
        for street in &town.streets {
            for &(x, z) in &street.points {
                if x.abs() > 1000.0 || z.abs() > 1000.0 {
                    continue;
                }
                total += 1;
                if !relief.is_hill(Vec2::new(x, z)) {
                    floor += 1;
                }
            }
        }
        let share = floor as f32 / total.max(1) as f32;
        assert!(
            (0.75..=0.95).contains(&share),
            "{:.0}% of the street plan is on the floor",
            share * 100.0
        );
        // And the far grid reaches the horizon rather than stopping at the
        // town, with the hill still on it.
        assert!(relief.far.sample(Vec2::new(5_000.0, -5_000.0)).is_some());
        assert!(relief.at(Vec2::new(300.0, 900.0)) > 20.0);
    }

    /// A grid is sampled bilinearly and clamped at its border.
    #[test]
    fn a_relief_grid_is_read_between_its_cells() {
        let grid = Grid {
            origin: (-10.0, -10.0),
            step: 10.0,
            cols: 3,
            rows: 3,
            values: vec![0.0, 0.0, 0.0, 0.0, 10.0, 20.0, 0.0, 30.0, 40.0],
        };
        assert!(grid.is_sound());
        assert_eq!(grid.sample(Vec2::new(0.0, 0.0)), Some(10.0));
        assert_eq!(grid.sample(Vec2::new(10.0, 0.0)), Some(20.0));
        assert_eq!(grid.sample(Vec2::new(5.0, 0.0)), Some(15.0));
        assert_eq!(grid.sample(Vec2::new(5.0, 5.0)), Some(25.0));
        assert_eq!(grid.sample(Vec2::new(10.0, 10.0)), Some(40.0));
        assert_eq!(grid.sample(Vec2::new(11.0, 0.0)), None);
        assert_eq!(grid.sample_clamped(Vec2::new(11.0, 0.0)), 20.0);
        assert_eq!(grid.sample_clamped(Vec2::new(100.0, 100.0)), 40.0);
        let short = Grid {
            values: vec![0.0; 4],
            ..grid.clone()
        };
        assert!(!short.is_sound());
    }

    /// Where a street climbs the hill it stops, and the hill is left standing.
    #[test]
    fn a_street_up_the_hill_is_left_to_it() {
        // A relief that is floor north of the origin and forty metres of hill
        // south of it, with a soft edge one cell wide.
        let mut hill = town(vec![street(&[
            (0.0, -300.0),
            (0.0, -100.0),
            (0.0, 100.0),
            (0.0, 300.0),
        ])]);
        let grid = |step: f32, half: f32| {
            let cells = (2.0 * half / step) as u32 + 1;
            let mut values = Vec::with_capacity((cells * cells) as usize);
            for row in 0..cells {
                let z = -half + row as f32 * step;
                for _ in 0..cells {
                    values.push(if z > 0.0 { 40.0 } else { 0.0 });
                }
            }
            Grid {
                origin: (-half, -half),
                step,
                cols: cells,
                rows: cells,
                values,
            }
        };
        hill.relief = Some(Relief {
            datum: 400.0,
            near: grid(50.0, 500.0),
            far: grid(200.0, 2000.0),
        });
        let (layout, _) = layout(&hill, 1, 1000.0);
        // The two points on the floor are joined; the two up the hill are
        // gone, and no edge reaches up to them.
        assert_eq!(layout.graph.node_count(), 2, "the hill was built on");
        assert_eq!(layout.graph.edge_count(), 1);
        for edge in layout.graph.edges() {
            for end in [edge.a, edge.b] {
                assert!(layout.graph.node(end).pos.y < 0.0);
            }
        }
        assert!(layout.relief.is_some(), "the layout lost the relief");

        // A house up there is left out; a landmark is kept, and stood on the
        // hill at the hill's height.
        let part = |name: &str, z: f32| Footprint {
            name: name.into(),
            centre: (30.0, z),
            yaw: 0.0,
            frontage: 12.0,
            depth: 9.0,
            height: Some(9.0),
            kind: None,
            group: None,
            area: 108.0,
            roof: None,
            levels: None,
        };
        hill.buildings = vec![part("", 200.0), part("Burg", 200.0), part("", -200.0)];
        let blocks = footprints(&hill, &layout.graph, 1, 1000.0, CityStyle::Landshuepf);
        assert_eq!(blocks.len(), 2, "a house was built on the hill");
        let castle = blocks
            .iter()
            .find(|block| block.buildings[0].footprint.center().y > 0.0)
            .expect("the castle was left out");
        assert!((castle.buildings[0].ground - 40.0).abs() < 0.5);
        let house = blocks
            .iter()
            .find(|block| block.buildings[0].footprint.center().y < 0.0)
            .expect("the house on the floor was left out");
        assert_eq!(house.buildings[0].ground, 0.0);
    }

    /// The parts of one building come back as one block, sharing what a
    /// building shares.
    #[test]
    fn a_building_in_parts_is_one_block() {
        let mut corner = town(vec![street(&[(-100.0, 0.0), (100.0, 0.0)])]);
        let part = |group: u32, x: f32, z: f32, frontage: f32| Footprint {
            name: String::new(),
            centre: (x, z),
            yaw: 0.0,
            frontage,
            depth: 8.0,
            height: None,
            kind: None,
            group: Some(group),
            area: 300.0,
            roof: Some(crate::world::citygen::RoofShape::Hipped),
            levels: None,
        };
        // An L: a front wing on the street and a back wing behind it, then a
        // plain house next door.
        corner.buildings = vec![
            part(0, 0.0, 12.0, 20.0),
            part(0, 6.0, 22.0, 8.0),
            part(1, 40.0, 12.0, 14.0),
        ];
        let (layout, _) = layout(&corner, 1, 1000.0);
        let blocks = footprints(&corner, &layout.graph, 1, 1000.0, CityStyle::Landshuepf);
        assert_eq!(blocks.len(), 2);
        let l = &blocks[0];
        assert_eq!(l.buildings.len(), 2);
        assert_eq!(l.buildings[0].height, l.buildings[1].height);
        assert_eq!(l.buildings[0].palette, l.buildings[1].palette);
        assert_eq!(l.buildings[0].kind, l.buildings[1].kind);
        assert_eq!(
            l.buildings[1].roof,
            Some(crate::world::citygen::RoofShape::Hipped)
        );
        // The block's box holds both wings.
        for building in &l.buildings {
            let footprint = building.footprint;
            assert!(l.area.min.x <= footprint.min.x && l.area.max.x >= footprint.max.x);
            assert!(l.area.min.y <= footprint.min.y && l.area.max.y >= footprint.max.y);
        }
        assert_eq!(blocks[1].buildings.len(), 1);
        // And a file from before parts existed still reads one box as one
        // building.
        corner.buildings = vec![
            Footprint {
                group: None,
                ..part(0, 0.0, 12.0, 20.0)
            },
            Footprint {
                group: None,
                ..part(0, 40.0, 12.0, 14.0)
            },
        ];
        let blocks = footprints(&corner, &layout.graph, 1, 1000.0, CityStyle::Landshuepf);
        assert_eq!(blocks.len(), 2);
    }

    /// A building faces the street, not whichever way its bounding box came
    /// out.
    ///
    /// This is what a wrong Dachfront was. A minimum-area box has two axes and
    /// the bake picks the longer one; the sign is arbitrary and the longer side
    /// is not always the street side, so a quarter of the town wore its front
    /// on its flank and another quarter on its back. On a Giebelhaus that draws
    /// the stepped screen standing off to one side of its own roof, which is
    /// what you see from the street and cannot see from above.
    #[test]
    fn every_building_turns_its_front_to_the_road() {
        let Some(town) = committed() else {
            return;
        };
        let (layout, _) = layout(&town, 1, 1_000.0);
        let blocks = footprints(&town, &layout.graph, 1, 1_000.0, CityStyle::Landshuepf);
        assert!(blocks.len() > 2_000, "{} buildings", blocks.len());

        let mut faced = 0usize;
        let mut counted = 0usize;
        for block in &blocks {
            let building = &block.buildings[0];
            let centre = building.footprint.center();
            let Some(towards) = nearest_street(&layout.graph, centre) else {
                continue;
            };
            counted += 1;
            let yaw = building.facing.expect("an atlas building faces somewhere");
            // The convention `buildings::site_in` reads: a yaw of theta sends
            // the building's local +Z outward, across the pavement.
            let outward = Vec2::new(yaw.sin(), yaw.cos());
            if outward.dot(towards) > 0.55 {
                faced += 1;
            }
        }
        let share = faced as f32 / counted.max(1) as f32;
        assert!(
            share > 0.9,
            "only {faced} of {counted} buildings ({:.0}%) face the street they \
             stand on",
            share * 100.0
        );
    }

    /// Two ways that meet share a junction rather than passing through one
    /// another.
    #[test]
    fn streets_that_meet_are_welded() {
        // A cross: one way north to south, one east to west, sharing the
        // middle point exactly the way two OSM ways share a node.
        let atlas = town(vec![
            street(&[(0.0, -50.0), (0.0, 0.0), (0.0, 50.0)]),
            street(&[(-50.0, 0.0), (0.0, 0.0), (50.0, 0.0)]),
        ]);
        let (layout, _) = layout(&atlas, 1, 1000.0);
        assert_eq!(layout.graph.node_count(), 5, "the middle was not shared");
        assert_eq!(layout.graph.edge_count(), 4);

        // And the junction really has four arms, which is the thing traffic
        // and the signals both read.
        let middle = layout
            .graph
            .nearest_node(Vec2::ZERO)
            .expect("a node at the crossing");
        assert_eq!(layout.graph.node(middle).edges.len(), 4);
    }

    /// A street that leaves the square is cut rather than stretched across it.
    #[test]
    fn a_street_stops_at_the_edge_of_the_world() {
        let atlas = town(vec![street(&[
            (-100.0, 0.0),
            (0.0, 0.0),
            (900.0, 0.0),
            (2_000.0, 0.0),
            (2_100.0, 0.0),
        ])]);
        let (layout, _) = layout(&atlas, 1, 1000.0);
        // The three points inside stay and are joined; the two outside are
        // dropped, and no edge reaches out to them.
        assert_eq!(layout.graph.node_count(), 3);
        assert_eq!(layout.graph.edge_count(), 2);
        for edge in layout.graph.edges() {
            for end in [edge.a, edge.b] {
                let at = layout.graph.node(end).pos;
                assert!(at.x.abs() <= 1000.0 && at.y.abs() <= 1000.0);
            }
        }
    }

    /// A way that leaves the square and comes back is two runs, not one.
    #[test]
    fn a_street_that_leaves_and_returns_is_not_bridged() {
        // The failure this stops is a ring road clipping a corner: joined
        // across the gap it lays a kilometre of asphalt straight through the
        // middle of the town.
        let atlas = town(vec![street(&[
            (-500.0, 0.0),
            (-400.0, 0.0),
            (-5_000.0, 0.0),
            (400.0, 0.0),
            (500.0, 0.0),
        ])]);
        let (layout, _) = layout(&atlas, 1, 1000.0);
        assert_eq!(layout.graph.edge_count(), 2, "the gap was bridged");
        for edge in layout.graph.edges() {
            assert!(edge.length < 200.0, "an edge crossed the whole town");
        }
    }

    /// Points too close together do not become an edge.
    #[test]
    fn a_kerb_wobble_is_not_a_street() {
        let atlas = town(vec![street(&[(0.0, 0.0), (0.05, 0.0), (60.0, 0.0)])]);
        let (layout, _) = layout(&atlas, 1, 1000.0);
        // The wobble welds into the first node, so there are two nodes and one
        // road rather than three and two.
        assert_eq!(layout.graph.node_count(), 2);
        assert_eq!(layout.graph.edge_count(), 1);
    }

    /// A real town has no grid, and asking for one finds nothing.
    #[test]
    fn an_atlas_city_has_no_street_lists() {
        let atlas = town(vec![street(&[(0.0, 0.0), (40.0, 0.0)])]);
        let (layout, _) = layout(&atlas, 7, 1000.0);
        assert!(layout.x_streets.is_empty() && layout.z_streets.is_empty());
        assert!(layout.canal.is_none());
        assert_eq!(layout.seed, 7);
    }
}
