//! Plain pitched roofs: the carpet of tile a Bavarian town is from the air.
//!
//! Every building in this city was a box with a slab on it, and for exactly
//! one postcard the slab was replaced — on the screened Altstadt houses,
//! whose pitch `world::gable` hid behind a `Vorschussmauer`. That left the
//! rest of Landshut flat: a suburb of grey decks with air handling on them,
//! standing round an old town of red roofs. Photographed from the Hofberg the
//! real town is red to the edge — a Satteldach or a Walmdach at forty-five to
//! fifty degrees on *every* house, dormers in most of them, a chimney near
//! every ridge — and the flat roofs are the supermarket and the car park.
//!
//! ## What is shared with the screen
//!
//! The roof behind a screen wall and the roof on a suburban house are the
//! same roof: two slopes, an overhang with a fascia and a gutter, a half-round
//! along the ridge, dormers on the flanks. So the roof half of what `gable`
//! did is here, drawn from one [`Pitch`] whichever way the building came by
//! it, and the screen keeps only the screen. A screened house still draws
//! its steps and its oriel and its tile age from its own stream in its own
//! order, so a chunk walked back into wears the same skyline it did.
//!
//! ## What the shape is
//!
//! Three unit meshes, scaled per building. A prism — ridge along local X at
//! `y = 1`, eaves at `y = 0`, span across Z — carries the two tile slopes; a
//! pair of triangles closes its ends in the building's own wall paint; and a
//! half-pyramid closes one end in tile instead, so a hipped roof is the prism
//! cut short between two of those, and a pyramid is two of them meeting. The
//! meshes are thin, which is what lets one unit mesh be every roof in the
//! town at every size: a slab with a thickness cannot be scaled to a span
//! without its thickness scaling with it. The tile material is drawn from
//! both sides for that reason — the underside of the overhang is the soffit,
//! and a single-sided quad seen from under the eaves is a strip of sky.
//!
//! ## Which way the ridge runs
//!
//! Along the frontage when the house is clearly wider than it is deep — a
//! `Traufhaus`, eaves to the street — and back from the street otherwise, a
//! `Giebelhaus` with its gable on the pavement. The bias towards the gable is
//! deliberate: it is what a Bavarian street mostly is, and a row of them at
//! their own heights is what a roofline is.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::{ChaCha8Rng, rand_core::SeedableRng};

use super::buildings::ChunkOf;
use super::citygen::{BuildingKind, RoofShape};
use super::gable::GableKit;
use super::texture::{self, FacadeClass};
use crate::core::config::RoofDials;

/// How far a roof is drawn. As far as the building it caps: a roofline is a
/// silhouette, and a silhouette is the last thing to stop mattering.
pub const RANGE: f32 = 900.0;

/// And how far its trim is — fascia, gutter, bargeboard, the coping on a
/// screen. All of it is a hand's width thick, and past a few hundred metres
/// it is under a pixel and all it contributes is a lighter edge on a skyline
/// that already has one. On a town where every roof carries it, it is the
/// obvious half to drop first.
///
/// Two hundred and thirty was tuned against a shell that turned into a plain
/// box at two hundred and fifty, so the trim outlived the wall it sat on by
/// twenty metres and no further. The wall now keeps its courses to four
/// hundred and sixty, and a skyline without its copings and gutters is a row
/// of plain triangles.
pub const TRIM_RANGE: f32 = 380.0;

/// How far the ridge stands above the eaves, as a fraction of the span.
///
/// Fifty-five hundredths is a shade under forty-eight degrees, which is what
/// a Biberschwanz roof is laid at: steep enough to shed snow, and the reason
/// a Landshut roof is half the height of the house under it. The first pass
/// at the screened houses ran at forty-two, and forty degrees on a house
/// whose neighbours stand at fifty reads as the one roof in the row that has
/// slumped.
pub const PITCH: f32 = 0.55;

/// The widest span pitched at the full [`PITCH`].
///
/// A roof wider than this is pitched as though it were this wide, so its
/// rise stops growing with the building. Forty-eight degrees over a
/// thirty-metre block is a fifteen-metre attic — a barn on top of a
/// department store — and what a big Gründerzeit block actually carries is
/// the same rise as the houses beside it, spread over more roof and
/// therefore shallower.
const WIDEST_STEEP: f32 = 16.0;

/// And no rise past this share of the wall's own height: a roof that is
/// taller than nine tenths of its house is a tent.
const TALLEST: f32 = 0.9;

/// A house at least this much wider than it is deep runs its ridge along the
/// street; anything squarer keeps the gable on the pavement.
const TRAUF: f32 = 1.15;

/// How far the tiles carry on past the wall they are resting on.
///
/// The single most load-bearing number here, and for a long time it was
/// zero. A roof whose covering stops dead on the plane of the wall is not a
/// roof — it is a lid — and a street of lids is a street of extruded boxes,
/// which is exactly what an aerial of this town looked like. What an eaves
/// overhang buys is a *shadow*: a dark line the width of a hand under the
/// tiles, running the whole length of every house, which is the thing that
/// separates the roof from the wall at any distance at all.
///
/// Forty-five centimetres at the eaves, which is a modest Bavarian one, and
/// twenty-five at the verge, where a bargeboard closes it.
pub const EAVES_OVERHANG: f32 = 0.45;
pub const VERGE_OVERHANG: f32 = 0.25;

/// The board closing the ends of the rafters, and the gutter hung on it.
///
/// A fascia is what you actually see of an overhang from underneath: a pale
/// vertical strip under the tile edge. The gutter in front of it is the one
/// horizontal line on a house that is neither wall nor roof, and it catches
/// the sun along its whole length, which is why it reads from a street away.
const FASCIA_DROP: f32 = 0.20;
const FASCIA_THICK: f32 = 0.05;
const GUTTER_RADIUS: f32 = 0.065;

/// The board up the verge of a gable, under the tile edge. Without it the
/// slope is a sheet of paper seen edge-on, because that is what a thin mesh
/// is.
const BARGE_DROP: f32 = 0.18;
const BARGE_THICK: f32 = 0.05;

/// The capping along the ridge and down the hips. A half-round in life, and
/// a half-round here.
const RIDGE_RADIUS: f32 = 0.11;

/// A dormer's proportions, in metres: how far along the roof it stands, how
/// wide across the slope it is, and how far it rises out of the tiles.
///
/// It is the defining feature of a Landshut roofscape and there was not one
/// in the game. They sit on the flanks, which on a `Giebelhaus` is the side
/// facing the neighbour rather than the street — so they are worth almost
/// nothing from a pavement and a great deal from anywhere above one, which
/// is where a roofscape is looked at.
const DORMER: (f32, f32, f32) = (1.35, 1.05, 0.95);

/// How far up the slope from the eaves a dormer sits, as a fraction of the
/// horizontal run. Low: a dormer high on a slope is a skylight.
const DORMER_UP: f32 = 0.34;

/// Roofs shallower than this in their run carry no dormers — there is
/// nothing to put one in.
const DORMER_MIN_RUN: f32 = 4.2;

/// How much ridge one dormer wants to itself, and the length of roof that
/// earns a second row of them.
const DORMER_SPACING: f32 = DORMER.0 * 2.4;
const DORMER_PER: f32 = 10.0;

/// And how far they are drawn. Nearer than the roof itself: a dormer is a
/// metre across and four of them on a skyline are four pixels.
const DORMER_RANGE: f32 = 320.0;

/// Metres of roof one repeat of the tile image covers: up the slope first,
/// then along the ridge.
///
/// A Biberschwanz is eighteen centimetres wide and shows sixteen of its
/// length per course; the image carries nine courses by six tiles, so the
/// true repeat is about a metre and a half by a metre. A shade over that,
/// because a tile that is under a pixel from the far pavement is noise.
pub const GRAIN: Vec2 = Vec2::new(1.5, 1.2);

/// Repeat counts a tile material may be drawn at, up the slope and along the
/// ridge — the same bucket machinery the flat roofs use, because the same
/// thing was wrong: a fixed repeat count over whatever the leaf was scaled to
/// put hand-sized tiles on a house and doormat-sized ones on a block. Two
/// tables rather than one, because a leaf is not square: a house's slope is
/// five metres up and twelve along, a block's is twelve up and forty along.
const UP_BUCKETS: [f32; 3] = [3.0, 6.0, 12.0];
const ALONG_BUCKETS: [f32; 4] = [5.0, 10.0, 20.0, 36.0];

/// One material per bucket pair per age — see [`AGES`].
pub const BUCKETS: usize = UP_BUCKETS.len() * ALONG_BUCKETS.len();

/// Three roofs rather than one, because a real old town has not been
/// re-tiled all at once: a house done last summer sits between one that was
/// done in the seventies and one nobody has touched since the moss took it.
/// Picked from the building's own seed, so a roof keeps its age across a
/// chunk respawn.
pub const AGES: usize = 3;

/// What `world::buildings` puts on top of a box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Roof {
    /// A slab with a parapet and whatever accumulated on it — `rooftop`.
    Flat,
    /// The front wall carried on up past a pitch — `gable`.
    Screened,
    /// A plain pitch, the ends closed by triangles of wall.
    Gabled,
    /// A pitch with its ends sloped in tile too; a pyramid when the plan is
    /// square.
    Hipped,
}

impl Roof {
    /// Whether there is a pitch rather than a deck up there — which decides
    /// the parapet slab, the clutter, and how often a chimney is lit.
    pub fn is_pitched(self) -> bool {
        !matches!(self, Roof::Flat)
    }
}

/// The three rolls a roof is decided on, each in `0..1`.
///
/// Separate rolls rather than one, so that the share of screens can be
/// retuned without moving which of the unscreened houses are hipped — and
/// so that the screen roll is the exact expression it always was, which
/// keeps every screened house in the town where it stood.
#[derive(Clone, Copy, Debug)]
pub struct Rolls {
    pub screen: f32,
    pub pitch: f32,
    pub hip: f32,
    /// Which of the [`AGES`] a plain roof's tiles are. Drawn last, so a
    /// retuned share cannot re-tile the town. A screened house draws its own
    /// from its own stream and ignores this one.
    pub age: usize,
}

/// Salt on the building's own seed for everything a plain roof draws: the
/// pitch and hip rolls, and the age of the tiles. Not the screen's salt and
/// not the dormers', so none of the three can move the others.
const SALT: u64 = 0x5200_F5D1;

impl Rolls {
    pub fn from_seed(seed: u64) -> Self {
        // The screen roll is the expression `buildings` has always used, so
        // the screened houses stay bit-for-bit where they were.
        let screen = (seed >> 31) as f32 / u32::MAX as f32 % 1.0;
        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ SALT);
        Rolls {
            screen,
            pitch: rng.random_range(0.0..1.0),
            hip: rng.random_range(0.0..1.0),
            age: rng.random_range(0..AGES),
        }
    }
}

/// What this building's roof is, from what the style wants, what the
/// building is, and what the map may already know.
///
/// The map first: `roof:shape` is the one fact about a roofline a style
/// cannot guess. Then the kinds that are not houses keep the roof they had —
/// a supermarket in the suburbs is a flat box, and an office block in the
/// old town wears whatever the screen roll gave it before this existed. Then
/// the low classes: screened at the style's share in the core ring, pitched
/// at its share everywhere, hipped at its share of those. The middle class
/// hips or stays flat, and a tower is a tower.
pub fn decide(
    dials: RoofDials,
    class: FacadeClass,
    kind: BuildingKind,
    core: bool,
    map: Option<RoofShape>,
    rolls: Rolls,
) -> Roof {
    let low = matches!(class, FacadeClass::House | FacadeClass::Lowrise);
    let screened = low && core && rolls.screen < dials.screened;
    if let Some(shape) = map {
        return match shape {
            RoofShape::Flat => Roof::Flat,
            RoofShape::Hipped | RoofShape::HalfHipped | RoofShape::Pyramidal => Roof::Hipped,
            // A gable tag on an Altstadt house is the roof *behind* the
            // screen — nobody tags the screen — so the screen roll still
            // decides there. Outside the core it is what it says.
            RoofShape::Gabled if screened => Roof::Screened,
            RoofShape::Gabled => Roof::Gabled,
            // One slope over a box needs a wall shape this kit does not have;
            // two of them in the whole of Landshut are not worth a mesh.
            RoofShape::Skillion => Roof::Flat,
        };
    }
    // The kinds that were never houses: the rule they had before the
    // suburbs were roofed, which is a screen at the old share on the low
    // classes and a slab on everything else.
    if matches!(
        kind,
        BuildingKind::Offices
            | BuildingKind::Supermarket
            | BuildingKind::ParkingGarage
            | BuildingKind::Tower
            | BuildingKind::Stadium
            | BuildingKind::Barracks
            | BuildingKind::FireStation
    ) {
        return if low && rolls.screen < dials.screened {
            Roof::Screened
        } else {
            Roof::Flat
        };
    }
    match class {
        _ if screened => Roof::Screened,
        FacadeClass::House | FacadeClass::Lowrise => {
            if rolls.pitch >= dials.pitched {
                Roof::Flat
            } else if rolls.hip < dials.hipped {
                Roof::Hipped
            } else {
                Roof::Gabled
            }
        }
        FacadeClass::Midrise if rolls.pitch < dials.midrise_hipped => Roof::Hipped,
        FacadeClass::Midrise | FacadeClass::Tower => Roof::Flat,
    }
}

/// Which way the ridge runs, in the building's own frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ridge {
    /// Along the street face: eaves to the pavement. A `Traufhaus`.
    AlongFrontage,
    /// Back from the street: the gable on the pavement. A `Giebelhaus`.
    AlongDepth,
}

/// One pitched roof over one box, in the box's own frame — `x` along the
/// frontage, `z` towards the street, `y` up from the eaves.
///
/// Everything a roof is, short of where it stands: the spawner turns this
/// into meshes, and `plume` asks it where the tiles are under a chimney.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pitch {
    pub ridge: Ridge,
    /// How far the ridge stands above the eaves.
    pub rise: f32,
    /// The horizontal distance the two slopes cross, wall to wall.
    pub span: f32,
    /// The length of the roof along the ridge, wall to wall.
    pub along: f32,
    /// Whether the ends are sloped in tile rather than closed in wall.
    pub hipped: bool,
    /// How far the tiles carry on past the two end walls, at the end the
    /// ridge axis points away from and the end it points to. The verge on a
    /// gable, the eaves on a hip — and negative where the roof has to stop
    /// short of the wall, which is what a screen in front of it needs.
    pub ends: (f32, f32),
}

impl Pitch {
    /// The roof a box with nothing in front of it gets.
    pub fn plain(hipped: bool, frontage: f32, depth: f32, height: f32) -> Self {
        let ridge = if frontage >= depth * TRAUF {
            Ridge::AlongFrontage
        } else {
            Ridge::AlongDepth
        };
        let (span, along) = match ridge {
            Ridge::AlongFrontage => (depth, frontage),
            Ridge::AlongDepth => (frontage, depth),
        };
        let end = if hipped {
            EAVES_OVERHANG
        } else {
            VERGE_OVERHANG
        };
        Pitch {
            ridge,
            rise: Self::rise_for(span, height),
            span,
            along,
            hipped,
            ends: (end, end),
        }
    }

    /// The roof behind a screen: a `Giebelhaus` whose front end stops `stop`
    /// metres inside the front wall so nothing of it comes through the
    /// screen, and whose ridge is held under the screen's own top.
    pub fn screened(frontage: f32, depth: f32, height: f32, screen_rise: f32, stop: f32) -> Self {
        Pitch {
            ridge: Ridge::AlongDepth,
            // Under the screen, and this is the only thing a screen is for.
            // If the ridge pokes out over the top of it the house has a
            // triangle growing out of a staircase, which nobody has built.
            // The neighbours' screens are another matter: a taller house's
            // roof standing up behind a lower screen is what the photographs
            // show, and the steeper pitch is what puts it there.
            rise: Self::rise_for(frontage, height).min(screen_rise * 0.92),
            span: frontage,
            along: depth,
            hipped: false,
            ends: (VERGE_OVERHANG, -stop),
        }
    }

    /// The ridge rise for a span: [`PITCH`], capped by the widest span that
    /// is pitched in full and by the wall under it.
    fn rise_for(span: f32, height: f32) -> f32 {
        (span.min(WIDEST_STEEP) * PITCH).min(height * TALLEST)
    }

    /// The angle a slope leans at, from the horizontal.
    pub fn slope(&self) -> f32 {
        (self.rise / (self.span * 0.5).max(0.1)).atan()
    }

    /// How far a hip end runs in from the end wall to where the ridge stops.
    /// Half the span when the plan allows it — equal pitch on all four
    /// faces — and half the length when it does not, which is a pyramid.
    pub fn hip_run(&self) -> f32 {
        if self.hipped {
            (self.span * 0.5).min(self.along * 0.5)
        } else {
            0.0
        }
    }

    /// How long the ridge is between the walls.
    pub fn ridge_length(&self) -> f32 {
        (self.along - 2.0 * self.hip_run()).max(0.0)
    }

    /// Unit vectors along the ridge and across it, in the building's `(x, z)`.
    fn axes(&self) -> (Vec2, Vec2) {
        match self.ridge {
            Ridge::AlongFrontage => (Vec2::X, Vec2::Y),
            Ridge::AlongDepth => (Vec2::Y, Vec2::X),
        }
    }

    /// The turn that puts the unit prism's ridge along this roof's, relative
    /// to the building's own yaw. A quarter turn the negative way puts the
    /// prism's `+X` on the building's `+Z` — towards the street — so the
    /// second of [`Pitch::ends`] is the street end either way.
    pub fn rotation(&self) -> Quat {
        match self.ridge {
            Ridge::AlongFrontage => Quat::IDENTITY,
            Ridge::AlongDepth => Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
        }
    }

    /// How high the tiles are above the eaves at a point in the building's
    /// frame; nought beyond the walls.
    pub fn surface(&self, x: f32, z: f32) -> f32 {
        let (axis, across) = self.axes();
        let at = Vec2::new(x, z);
        let a = at.dot(axis).abs();
        let c = at.dot(across).abs();
        let main = self.rise * (1.0 - c / (self.span * 0.5).max(0.1));
        let hip = if self.hipped {
            self.rise * (self.along * 0.5 - a) / self.hip_run().max(0.1)
        } else {
            self.rise
        };
        main.min(hip).clamp(0.0, self.rise)
    }

    /// Where a chimney comes up, from two unit draws: one near the ridge
    /// across the roof, one along it. In the building's frame.
    ///
    /// The draws are the two `plume` has always made, in the order it made
    /// them, and on a `Giebelhaus` they map exactly as they did — so a fire
    /// lit on a screened house is lit where it was. On a `Traufhaus` the
    /// same two numbers are turned round: the first is still the one that
    /// keeps the stack near the ridge, whichever way the ridge runs.
    pub fn chimney_spot(&self, near: f32, along: f32) -> Vec2 {
        // On a hip the ridge stops short of the walls, and a stack past the
        // end of it is a stack halfway down a hip.
        let reach = (self.ridge_length() * 0.5 - 0.5).max(0.0);
        match self.ridge {
            Ridge::AlongDepth => {
                Vec2::new(near * self.span, (along * self.along).clamp(-reach, reach))
            }
            Ridge::AlongFrontage => Vec2::new(
                ((along - 0.21) * 2.0 * self.along).clamp(-reach, reach),
                near * self.span,
            ),
        }
    }
}

/// One mesh of a roof, in the prism's frame: `x` along the ridge, `z` across
/// it, `y` up from the eaves, all relative to the middle of the building.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Piece {
    pub(crate) part: Part,
    pub(crate) at: Vec3,
    pub(crate) rotation: Quat,
    pub(crate) scale: Vec3,
}

/// What a piece is made of, which decides its mesh, its material and how far
/// it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// The two slopes: the unit prism, in tile.
    Slopes,
    /// One hipped end: the unit half-pyramid, in tile.
    HipEnd,
    /// The two triangles closing a gable, in wall.
    GableEnds,
    Fascia,
    Gutter,
    /// A half-round along the ridge or down a hip, in tile.
    Capping,
    Barge,
    DormerCheek,
    DormerGlass,
    DormerLid,
}

impl Part {
    /// The corners of the unit shape this part is scaled from — every point
    /// the piece can reach, for anything that wants to know where it ends.
    fn unit_corners(self) -> &'static [Vec3] {
        const PRISM: [Vec3; 6] = [
            Vec3::new(-0.5, 0.0, -0.5),
            Vec3::new(0.5, 0.0, -0.5),
            Vec3::new(-0.5, 0.0, 0.5),
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(-0.5, 1.0, 0.0),
            Vec3::new(0.5, 1.0, 0.0),
        ];
        const HIP: [Vec3; 5] = [
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, -0.5),
            Vec3::new(0.0, 0.0, 0.5),
            Vec3::new(1.0, 0.0, -0.5),
            Vec3::new(1.0, 0.0, 0.5),
        ];
        const CUBE: [Vec3; 8] = [
            Vec3::new(-0.5, -0.5, -0.5),
            Vec3::new(0.5, -0.5, -0.5),
            Vec3::new(-0.5, 0.5, -0.5),
            Vec3::new(0.5, 0.5, -0.5),
            Vec3::new(-0.5, -0.5, 0.5),
            Vec3::new(0.5, -0.5, 0.5),
            Vec3::new(-0.5, 0.5, 0.5),
            Vec3::new(0.5, 0.5, 0.5),
        ];
        // A unit cylinder is a radius of one and a height of one on its Y.
        const ROUND: [Vec3; 8] = [
            Vec3::new(-1.0, -0.5, -1.0),
            Vec3::new(1.0, -0.5, -1.0),
            Vec3::new(-1.0, 0.5, -1.0),
            Vec3::new(1.0, 0.5, -1.0),
            Vec3::new(-1.0, -0.5, 1.0),
            Vec3::new(1.0, -0.5, 1.0),
            Vec3::new(-1.0, 0.5, 1.0),
            Vec3::new(1.0, 0.5, 1.0),
        ];
        match self {
            Part::Slopes | Part::GableEnds => &PRISM,
            Part::HipEnd => &HIP,
            Part::Gutter | Part::Capping => &ROUND,
            _ => &CUBE,
        }
    }
}

impl Piece {
    /// Every corner of this piece, in the prism's frame.
    pub(crate) fn corners(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.part
            .unit_corners()
            .iter()
            .map(move |&corner| self.at + self.rotation * (corner * self.scale))
    }
}

/// Every piece of a roof, short of its dormers, in the prism's frame.
///
/// Pure, and that is the point of it: the spawner only turns these into
/// entities, so what a roof *is* — where its overhang ends, where its ridge
/// stops, that its hips meet its slopes at the corners — can be asked
/// without a world.
pub(crate) fn pieces(pitch: &Pitch) -> Vec<Piece> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let e = EAVES_OVERHANG;
    let (e0, e1) = pitch.ends;
    let slope = pitch.slope();
    // The tiles carry on past the wall. Everything below is measured off the
    // tile edge — out past the wall by the overhang and down by what the
    // pitch does over that distance — so the overhang moves the eaves, the
    // fascia, the gutter and the dormers together and cannot be added to one
    // of them and forgotten on the next.
    let drop = e * slope.tan();
    let top = pitch.rise + drop;
    let w = pitch.span * 0.5 + e;
    let leaf = (w * w + top * top).sqrt();
    let len = pitch.along + e0 + e1;
    // Where the roof's own midpoint sits along the ridge, relative to the
    // building's: nought unless one end has been cut short.
    let shift = (e1 - e0) * 0.5;
    let ridge = pitch.ridge_length();
    let hip = pitch.hip_run() + e;
    let lip = -drop;

    let mut out = Vec::with_capacity(24);
    let along_x = Quat::from_rotation_z(FRAC_PI_2);
    let along_z = Quat::from_rotation_x(FRAC_PI_2);

    if pitch.hipped {
        // The ridge stretch of the prism between the two hip ends, if the
        // plan leaves one; a pyramid has none and is the two hips meeting.
        if ridge > 0.05 {
            out.push(Piece {
                part: Part::Slopes,
                at: Vec3::new(0.0, lip, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(ridge, top, 2.0 * w),
            });
            out.push(Piece {
                part: Part::Capping,
                at: Vec3::new(0.0, pitch.rise + RIDGE_RADIUS * 0.4, 0.0),
                rotation: along_x,
                scale: Vec3::new(RIDGE_RADIUS, ridge, RIDGE_RADIUS),
            });
        }
        for s in [-1.0f32, 1.0] {
            // The unit hip points along +X from its apex; the far end is
            // turned about.
            let apex = Vec3::new(s * ridge * 0.5, pitch.rise, 0.0);
            out.push(Piece {
                part: Part::HipEnd,
                at: Vec3::new(apex.x, lip, 0.0),
                rotation: if s > 0.0 {
                    Quat::IDENTITY
                } else {
                    Quat::from_rotation_y(PI)
                },
                scale: Vec3::new(hip, top, 2.0 * w),
            });
            // The fascia and gutter across the end, which a hip has and a
            // gable does not.
            let end = s * (pitch.along * 0.5 + e);
            out.push(Piece {
                part: Part::Fascia,
                at: Vec3::new(end, lip - FASCIA_DROP * 0.5, 0.0),
                rotation: Quat::from_rotation_y(FRAC_PI_2),
                scale: Vec3::new(2.0 * w, FASCIA_DROP, FASCIA_THICK),
            });
            out.push(Piece {
                part: Part::Gutter,
                at: Vec3::new(end + s * GUTTER_RADIUS * 0.6, lip - FASCIA_DROP * 0.55, 0.0),
                rotation: along_z,
                scale: Vec3::new(GUTTER_RADIUS, 2.0 * w, GUTTER_RADIUS),
            });
            // And the capping down each hip, from the apex to the corner —
            // which is what makes a hip read as a hip rather than as a
            // gable seen from the wrong end.
            for t in [-1.0f32, 1.0] {
                let corner = Vec3::new(end, lip, t * w);
                let run = corner - apex;
                out.push(Piece {
                    part: Part::Capping,
                    at: (apex + corner) * 0.5 + Vec3::Y * (RIDGE_RADIUS * 0.4),
                    rotation: Quat::from_rotation_arc(Vec3::Y, run.normalize()),
                    scale: Vec3::new(RIDGE_RADIUS, run.length(), RIDGE_RADIUS),
                });
            }
        }
    } else {
        out.push(Piece {
            part: Part::Slopes,
            at: Vec3::new(shift, lip, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(len, top, 2.0 * w),
        });
        // The ends in wall paint, on the wall planes — or wherever a cut
        // short end has moved the roof to.
        let inset = (e0.min(0.0), e1.min(0.0));
        out.push(Piece {
            part: Part::GableEnds,
            at: Vec3::new((inset.1 - inset.0) * 0.5, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(pitch.along + inset.0 + inset.1, pitch.rise, pitch.span),
        });
        out.push(Piece {
            part: Part::Capping,
            at: Vec3::new(shift, pitch.rise + RIDGE_RADIUS * 0.4, 0.0),
            rotation: along_x,
            scale: Vec3::new(RIDGE_RADIUS, len, RIDGE_RADIUS),
        });
        // A bargeboard up each slope at each end that actually overhangs —
        // set just under the tile plane, so the tiles oversail it the way
        // they do in life and it does not read as a rail laid on the roof.
        for (s, overhang) in [(-1.0f32, e0), (1.0f32, e1)] {
            if overhang <= 0.0 {
                continue;
            }
            let end = s * (pitch.along * 0.5 + overhang - BARGE_THICK * 0.6);
            for t in [-1.0f32, 1.0] {
                let middle = Vec3::new(end, (pitch.rise + lip) * 0.5, t * w * 0.5);
                let normal = Vec3::new(0.0, w, t * top) / leaf;
                out.push(Piece {
                    part: Part::Barge,
                    at: middle - normal * (BARGE_DROP * 0.5 + 0.02),
                    rotation: Quat::from_rotation_x(t * slope),
                    scale: Vec3::new(BARGE_THICK, BARGE_DROP, leaf),
                });
            }
        }
    }

    // The fascia closing the rafter ends along both eaves, and the gutter
    // hung on it. Both sit at the tile edge and both run the full length of
    // the roof, which is what makes them a *line* rather than a detail.
    for t in [-1.0f32, 1.0] {
        out.push(Piece {
            part: Part::Fascia,
            at: Vec3::new(shift, lip - FASCIA_DROP * 0.5, t * w),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(len, FASCIA_DROP, FASCIA_THICK),
        });
        out.push(Piece {
            part: Part::Gutter,
            at: Vec3::new(
                shift,
                lip - FASCIA_DROP * 0.55,
                t * (w + GUTTER_RADIUS * 0.6),
            ),
            rotation: along_x,
            scale: Vec3::new(GUTTER_RADIUS, len, GUTTER_RADIUS),
        });
    }
    out
}

/// The dormers on one slope of a roof, in the prism's frame.
///
/// Three boxes each: the cheeks, the glazing set into the outward face of
/// them, and a small flat lid with a zinc edge. Not a pitched dormer roof,
/// which would be four more meshes for a shape that is one pixel tall from
/// anywhere the dormer is visible at all — what reads is the *interruption*
/// in the slope and the dark rectangle in it.
pub(crate) fn dormers(seed: u64, pitch: &Pitch, side: f32) -> Vec<Piece> {
    let e = EAVES_OVERHANG;
    let run = pitch.span * 0.5 + e;
    if run < DORMER_MIN_RUN {
        return Vec::new();
    }
    // Its own stream off the building's seed, mixed differently from the one
    // the screen draws from: how many dormers a house has must not move which
    // roof it wears or how many steps its gable has.
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x0D0F_3E12 ^ if side > 0.0 { 1 } else { 0 });
    let count = match rng.random_range(0.0..1.0) {
        roll if roll < 0.34 => 0,
        roll if roll < 0.80 => 1,
        _ => 2,
    };
    if count == 0 {
        return Vec::new();
    }
    // Only along the ridge: a dormer past the end of it is a dormer on a
    // hip, and one hard against a verge is a dormer with no cheek.
    let usable = if pitch.hipped {
        pitch.ridge_length()
    } else {
        pitch.along
    } - DORMER.0
        - 0.6;
    if usable < 0.0 {
        return Vec::new();
    }
    // A long roof earns a row of them rather than a pair in the middle of
    // forty metres of tile; and however many were drawn, no more than fit.
    let per = (pitch.along / DORMER_PER).floor().max(1.0) as usize;
    let count = (count * per).min(1 + (usable / DORMER_SPACING) as usize);
    let (e0, e1) = pitch.ends;
    let shift = if pitch.hipped { 0.0 } else { (e1 - e0) * 0.5 };

    // Where on the slope: `DORMER_UP` of the way from the tile edge to the
    // ridge, measured horizontally. The roof surface at that point is what
    // the dormer stands on.
    let across = run * (1.0 - DORMER_UP);
    let base = pitch.rise - across * pitch.slope().tan();
    let c = side * across;
    let spacing = if count > 1 {
        (usable / (count - 1) as f32).min(DORMER_SPACING)
    } else {
        0.0
    };

    let mut out = Vec::with_capacity(count * 3);
    for index in 0..count {
        let a = shift + (index as f32 - (count as f32 - 1.0) * 0.5) * spacing;
        // The cheeks. Set *into* the slope by a little, so the box does not
        // float on top of the tiles at the low end of it.
        out.push(Piece {
            part: Part::DormerCheek,
            at: Vec3::new(a, base + DORMER.2 * 0.5 - 0.12, c),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(DORMER.0, DORMER.2, DORMER.1),
        });
        // The glazing, standing a few centimetres proud of the outward cheek.
        out.push(Piece {
            part: Part::DormerGlass,
            at: Vec3::new(
                a,
                base + DORMER.2 * 0.52 - 0.06,
                c + side * (DORMER.1 * 0.5 + 0.02),
            ),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(DORMER.0 * 0.72, DORMER.2 * 0.55, 0.05),
        });
        // And the lid, oversailing on all four sides with a zinc edge.
        out.push(Piece {
            part: Part::DormerLid,
            at: Vec3::new(a, base + DORMER.2 - 0.06, c),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(DORMER.0 + 0.22, 0.08, DORMER.1 + 0.22),
        });
    }
    out
}

/// Which tile material a leaf `up` metres up the slope and `along` metres
/// along the ridge is drawn with: the bucket pair nearest its true size.
pub fn bucket(up: f32, along: f32) -> usize {
    // Nearest in ratio rather than in difference: a repeat that is twice
    // too big and one that is half too big are the same mistake.
    let nearest = |table: &[f32], wanted: f32| {
        let wanted = wanted.max(0.5).ln();
        table
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (a.ln() - wanted).abs().total_cmp(&(b.ln() - wanted).abs()))
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let u = nearest(&UP_BUCKETS, up / GRAIN.x);
    let v = nearest(&ALONG_BUCKETS, along / GRAIN.y);
    u * ALONG_BUCKETS.len() + v
}

/// How many times the tile image repeats over a leaf drawn with `bucket`:
/// up the slope, then along the ridge.
pub fn lap(bucket: usize) -> Vec2 {
    let u = (bucket / ALONG_BUCKETS.len()).min(UP_BUCKETS.len() - 1);
    let v = bucket % ALONG_BUCKETS.len();
    Vec2::new(UP_BUCKETS[u], ALONG_BUCKETS[v])
}

/// The clay, at every bucket and every age — [`BUCKETS`] × [`AGES`] handles,
/// indexed `bucket * AGES + age`.
pub fn build_tiles(
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Vec<Handle<StandardMaterial>> {
    let clay = images.add(texture::tiles());
    let relief = images.add(texture::tiles_normal());
    // Occlusion in red, roughness in green, off the same height field as the
    // other two — see `texture::tiles_surface`.
    let surface = images.add(texture::tiles_surface());
    // New, weathered, and one the moss has had. All three are the same fired
    // clay underneath — what changes with age is that it goes browner, greyer
    // and less even, not that it goes a different colour.
    // Fired clay is a good deal more orange than memory says — the first pass
    // at these was a third darker and every roof in the town read as slate.
    let ages = [
        Color::srgb(0.78, 0.36, 0.22),
        Color::srgb(0.63, 0.33, 0.23),
        Color::srgb(0.50, 0.34, 0.26),
    ];
    let mut out = Vec::with_capacity(BUCKETS * AGES);
    for bucket in 0..BUCKETS {
        for age in ages {
            out.push(materials.add(StandardMaterial {
                // Old clay, not the grey felt the flat roofs are covered in.
                base_color: age,
                base_color_texture: Some(clay.clone()),
                normal_map_texture: Some(relief.clone()),
                // The same image twice: occlusion takes its red channel and
                // the metallic/roughness slot takes its green and blue.
                occlusion_texture: Some(surface.clone()),
                metallic_roughness_texture: Some(surface.clone()),
                uv_transform: Affine2::from_scale(lap(bucket)),
                // One, because it multiplies the map. Anything else throws the
                // green channel away — see `material::ScannedSet::apply`.
                perceptual_roughness: 1.0,
                metallic: 1.0,
                // From both sides: the slopes are one face thick, and the
                // underside of an overhang seen from the pavement is the
                // soffit, not a window onto the sky.
                double_sided: true,
                cull_mode: None,
                ..default()
            }));
        }
    }
    out
}

/// The unit prism: ridge along X at `y = 1`, eaves at `y = 0`, span across Z
/// from `-0.5` to `0.5`. Two slopes and nothing else — the ends are their
/// own meshes, because they are wall on a gable and tile on a hip.
///
/// `u` runs up the slope and `v` along the ridge, which is the way the tile
/// image is painted: courses stack up the slope.
pub fn prism() -> Mesh {
    let mut build = MeshBuild::default();
    build.quad(
        [
            Vec3::new(-0.5, 0.0, 0.5),
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(0.5, 1.0, 0.0),
            Vec3::new(-0.5, 1.0, 0.0),
        ],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
    );
    build.quad(
        [
            Vec3::new(0.5, 0.0, -0.5),
            Vec3::new(-0.5, 0.0, -0.5),
            Vec3::new(-0.5, 1.0, 0.0),
            Vec3::new(0.5, 1.0, 0.0),
        ],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
    );
    build.finish()
}

/// The two triangles closing the prism's ends, at `x = ±0.5`, facing out.
pub fn gable_ends() -> Mesh {
    let mut build = MeshBuild::default();
    build.triangle(
        [
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(0.5, 0.0, -0.5),
            Vec3::new(0.5, 1.0, 0.0),
        ],
        [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
    );
    build.triangle(
        [
            Vec3::new(-0.5, 0.0, -0.5),
            Vec3::new(-0.5, 0.0, 0.5),
            Vec3::new(-0.5, 1.0, 0.0),
        ],
        [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
    );
    build.finish()
}

/// The unit hip end: the apex at the origin's `y = 1`, the end eaves at
/// `x = 1` from `z = -0.5` to `0.5`. Three tile faces — the two slopes
/// carrying on as triangles to the corners, and the hip between them.
pub fn hip_end() -> Mesh {
    let mut build = MeshBuild::default();
    let apex = Vec3::new(0.0, 1.0, 0.0);
    build.triangle(
        [Vec3::new(0.0, 0.0, 0.5), Vec3::new(1.0, 0.0, 0.5), apex],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 0.0]],
    );
    build.triangle(
        [Vec3::new(1.0, 0.0, -0.5), Vec3::new(0.0, 0.0, -0.5), apex],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 0.0]],
    );
    build.triangle(
        [Vec3::new(1.0, 0.0, 0.5), Vec3::new(1.0, 0.0, -0.5), apex],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 0.5]],
    );
    build.finish()
}

/// Flat-shaded triangles, wound anticlockwise seen from outside, with the
/// normal each face actually has.
#[derive(Default)]
struct MeshBuild {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl MeshBuild {
    fn triangle(&mut self, corners: [Vec3; 3], uvs: [[f32; 2]; 3]) {
        let normal = (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .normalize();
        let first = self.positions.len() as u32;
        for (corner, uv) in corners.iter().zip(uvs) {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv);
        }
        self.indices.extend([first, first + 1, first + 2]);
    }

    fn quad(&mut self, corners: [Vec3; 4], uvs: [[f32; 2]; 4]) {
        let normal = (corners[1] - corners[0])
            .cross(corners[3] - corners[0])
            .normalize();
        let first = self.positions.len() as u32;
        for (corner, uv) in corners.iter().zip(uvs) {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv);
        }
        self.indices
            .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }

    fn finish(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// Raises one pitched roof on a box, with everything that hangs off it.
///
/// `eaves` is how far above the ground the top of the wall is; `age` is
/// which of the three tilings this roof was last done in, drawn by the
/// caller because a screened house draws it from its own stream in its own
/// order.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    kit: &GableKit,
    wall: &Handle<StandardMaterial>,
    seed: u64,
    age: usize,
    center: Vec2,
    yaw: f32,
    eaves: f32,
    pitch: &Pitch,
    chunk: IVec2,
    lod_scale: f32,
) {
    let far = (RANGE * lod_scale).min(2_000.0);
    let range = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: far..(far * 1.05),
        use_aabb: false,
    };
    let near = (TRIM_RANGE * lod_scale).min(600.0);
    let close = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: near..(near * 1.1),
        use_aabb: false,
    };
    let end = (DORMER_RANGE * lod_scale).min(700.0);
    let dormer = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: end..(end * 1.1),
        use_aabb: false,
    };

    // One tiling for the whole roof, off the slope's own size: a hip end
    // drawn coarser than the slope it meets would show the seam.
    let slope = pitch.slope();
    let w = pitch.span * 0.5 + EAVES_OVERHANG;
    let leaf = w / slope.cos();
    let tile = kit.tile(age, leaf, pitch.along + EAVES_OVERHANG * 2.0);

    let turn = Quat::from_rotation_y(yaw) * pitch.rotation();
    let origin = Vec3::new(center.x, eaves, center.y);
    let mut place = |piece: Piece| {
        let (mesh, material, range) = match piece.part {
            Part::Slopes => (&kit.prism, &tile, &range),
            Part::HipEnd => (&kit.hip, &tile, &range),
            Part::GableEnds => (&kit.ends, wall, &range),
            Part::Capping => (&kit.round, &tile, &range),
            Part::Fascia | Part::Barge => (&kit.cube, &kit.cap, &close),
            Part::Gutter => (&kit.round, &kit.metal, &close),
            Part::DormerCheek => (&kit.cube, &kit.dormer, &dormer),
            Part::DormerGlass => (&kit.cube, &kit.glass, &dormer),
            Part::DormerLid => (&kit.cube, &kit.metal, &dormer),
        };
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(origin + turn * piece.at)
                .with_rotation(turn * piece.rotation)
                .with_scale(piece.scale),
            range.clone(),
        ));
    };
    for piece in pieces(pitch) {
        place(piece);
    }
    for side in [-1.0f32, 1.0] {
        for piece in dormers(seed, pitch, side) {
            place(piece);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::CityStyle;

    fn rolls(screen: f32, pitch: f32, hip: f32) -> Rolls {
        Rolls {
            screen,
            pitch,
            hip,
            age: 0,
        }
    }

    /// A wide house runs its ridge along the street and a deep one runs it
    /// back — with the tie going to the gable, because that is what a
    /// Bavarian street is mostly made of.
    #[test]
    fn the_ridge_runs_along_the_street_only_on_a_clearly_wide_house() {
        assert_eq!(
            Pitch::plain(false, 14.0, 8.0, 9.0).ridge,
            Ridge::AlongFrontage
        );
        assert_eq!(Pitch::plain(false, 8.0, 14.0, 9.0).ridge, Ridge::AlongDepth);
        assert_eq!(
            Pitch::plain(false, 10.0, 10.0, 9.0).ridge,
            Ridge::AlongDepth
        );
        assert_eq!(
            Pitch::plain(false, 11.0, 10.0, 9.0).ridge,
            Ridge::AlongDepth
        );
        assert_eq!(
            Pitch::plain(false, 11.6, 10.0, 9.0).ridge,
            Ridge::AlongFrontage
        );
        // And the span is always the side the ridge does not run along.
        let wide = Pitch::plain(false, 14.0, 8.0, 9.0);
        assert_eq!((wide.span, wide.along), (8.0, 14.0));
        let deep = Pitch::plain(true, 8.0, 14.0, 9.0);
        assert_eq!((deep.span, deep.along), (8.0, 14.0));
    }

    /// Every house roof leans between forty and fifty degrees, and no roof
    /// is taller than its house.
    #[test]
    fn a_roof_pitches_like_a_roof() {
        for frontage in [5.0f32, 7.0, 9.0, 12.0, 16.0] {
            for depth in [6.0f32, 9.0, 14.0] {
                for height in [7.0f32, 9.0, 12.0, 20.0] {
                    let pitch = Pitch::plain(false, frontage, depth, height);
                    let degrees = pitch.slope().to_degrees();
                    assert!(
                        (38.0..52.0).contains(&degrees) || pitch.rise >= height * TALLEST - 1e-3,
                        "a {frontage}x{depth}x{height} house leans at {degrees:.1} degrees"
                    );
                    assert!(pitch.rise <= height * TALLEST + 1e-4);
                }
            }
        }
        // A big block is not a barn: the rise stops growing with the span.
        let block = Pitch::plain(true, 40.0, 28.0, 30.0);
        assert!(block.rise <= WIDEST_STEEP * PITCH + 1e-4);
        assert!(block.slope().to_degrees() < 35.0);
    }

    /// Nothing of a hipped roof reaches past the footprint and its overhang,
    /// and its hips meet its slopes at the corners.
    #[test]
    fn a_hipped_roofs_pieces_stay_inside_the_footprint_and_its_overhang() {
        for (frontage, depth) in [(10.0f32, 10.0f32), (14.0, 9.0), (8.0, 20.0), (30.0, 18.0)] {
            let pitch = Pitch::plain(true, frontage, depth, 12.0);
            let reach = Vec2::new(
                pitch.along * 0.5 + EAVES_OVERHANG + GUTTER_RADIUS * 1.6,
                pitch.span * 0.5 + EAVES_OVERHANG + GUTTER_RADIUS * 1.6,
            );
            let drop = EAVES_OVERHANG * pitch.slope().tan();
            for piece in pieces(&pitch) {
                for corner in piece.corners() {
                    assert!(
                        corner.x.abs() <= reach.x + 1e-3 && corner.z.abs() <= reach.y + 1e-3,
                        "{:?} of a {frontage}x{depth} roof reaches {corner:?}, past {reach:?}",
                        piece.part
                    );
                    assert!(
                        corner.y >= -drop - FASCIA_DROP - GUTTER_RADIUS - 1e-3
                            && corner.y <= pitch.rise + RIDGE_RADIUS * 1.4 + 1e-3,
                        "{:?} of a {frontage}x{depth} roof is at {corner:?}",
                        piece.part
                    );
                }
            }
            // The hip ends' apexes sit on the ridge's ends.
            let ridge = pitch.ridge_length();
            let hips: Vec<_> = pieces(&pitch)
                .into_iter()
                .filter(|piece| piece.part == Part::HipEnd)
                .collect();
            assert_eq!(hips.len(), 2);
            for hip in &hips {
                let apex = hip.corners().next().expect("a hip has an apex");
                assert!((apex.x.abs() - ridge * 0.5).abs() < 1e-3);
                assert!((apex.y - pitch.rise).abs() < 1e-3);
            }
            // And nothing sticks out of a gable's ends past its verge either.
            let gable = Pitch::plain(false, frontage, depth, 12.0);
            for piece in pieces(&gable) {
                for corner in piece.corners() {
                    assert!(
                        corner.x.abs() <= gable.along * 0.5 + VERGE_OVERHANG + 1e-3,
                        "{:?} of a gable reaches {corner:?}",
                        piece.part
                    );
                }
            }
        }
        // A square plan is a pyramid: no ridge, the two hips meeting.
        let pyramid = Pitch::plain(true, 10.0, 10.0, 9.0);
        assert!(pyramid.ridge_length() < 1e-4);
        assert!(
            !pieces(&pyramid)
                .iter()
                .any(|piece| piece.part == Part::Slopes)
        );
    }

    /// A screened house's roof stops inside its screen and never rises over
    /// it.
    #[test]
    fn the_roof_behind_a_screen_stays_behind_it() {
        let pitch = Pitch::screened(9.0, 12.0, 14.0, 4.5, 0.23);
        assert_eq!(pitch.ridge, Ridge::AlongDepth);
        assert!(pitch.rise <= 4.5 * 0.92 + 1e-4);
        for piece in pieces(&pitch) {
            for corner in piece.corners() {
                // The street end is `+x` in the prism's frame: nothing may
                // reach the front wall plane, let alone through it.
                assert!(
                    corner.x <= 12.0 * 0.5 - 0.23 + 1e-3,
                    "{:?} pokes through the screen at {corner:?}",
                    piece.part
                );
            }
        }
        // But the roof still overhangs the back.
        let slopes = pieces(&pitch)
            .into_iter()
            .find(|piece| piece.part == Part::Slopes)
            .expect("a roof has slopes");
        assert!(
            slopes
                .corners()
                .any(|corner| corner.x < -6.0 - VERGE_OVERHANG * 0.9)
        );
    }

    /// Every face of the three unit meshes points away from the roof it is
    /// part of, and the normal stored on it agrees with its winding.
    #[test]
    fn the_unit_meshes_face_outward() {
        for (name, mesh, inside) in [
            ("prism", prism(), Vec3::new(0.0, 0.3, 0.0)),
            ("gable ends", gable_ends(), Vec3::new(0.0, 0.3, 0.0)),
            ("hip end", hip_end(), Vec3::new(0.3, 0.3, 0.0)),
        ] {
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("the {name} lost its positions");
            };
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) =
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
            else {
                panic!("the {name} lost its normals");
            };
            let Some(Indices::U32(indices)) = mesh.indices() else {
                panic!("the {name} lost its indices");
            };
            assert!(!indices.is_empty());
            for triangle in indices.chunks(3) {
                let [a, b, c] = [
                    Vec3::from(positions[triangle[0] as usize]),
                    Vec3::from(positions[triangle[1] as usize]),
                    Vec3::from(positions[triangle[2] as usize]),
                ];
                let wound = (b - a).cross(c - a).normalize();
                let stored = Vec3::from(normals[triangle[0] as usize]);
                assert!(
                    wound.dot(stored) > 0.99,
                    "a {name} face is wound against its normal: {wound:?} vs {stored:?}"
                );
                let centroid = (a + b + c) / 3.0;
                assert!(
                    wound.dot(centroid - inside) > 0.0,
                    "a {name} face at {centroid:?} faces in"
                );
            }
        }
    }

    /// A chimney stands on the tiles wherever the roof puts it.
    #[test]
    fn the_surface_is_the_ridge_in_the_middle_and_the_eaves_at_the_edge() {
        let gable = Pitch::plain(false, 10.0, 14.0, 9.0);
        assert!((gable.surface(0.0, 0.0) - gable.rise).abs() < 1e-4);
        assert!((gable.surface(0.0, 6.0) - gable.rise).abs() < 1e-4);
        assert!(gable.surface(5.0, 0.0).abs() < 1e-4);
        assert!((gable.surface(2.5, 3.0) - gable.rise * 0.5).abs() < 1e-4);

        let hip = Pitch::plain(true, 10.0, 14.0, 9.0);
        assert!((hip.surface(0.0, 0.0) - hip.rise).abs() < 1e-4);
        // Past the end of the ridge the hip falls away.
        assert!(hip.surface(0.0, 6.0) < hip.rise * 0.3);
        assert!(hip.surface(0.0, 7.0).abs() < 1e-4);
        assert!(hip.surface(5.0, 0.0).abs() < 1e-4);

        // A stack drawn near the ridge lands near the ridge, whichever way
        // the ridge runs — and on a hip, on the ridge and not down a hip.
        for pitch in [
            Pitch::plain(false, 14.0, 8.0, 9.0),
            Pitch::plain(false, 8.0, 14.0, 9.0),
            Pitch::plain(true, 14.0, 8.0, 9.0),
            Pitch::plain(true, 8.0, 14.0, 9.0),
        ] {
            for (near, along) in [(-0.16f32, 0.08f32), (0.16, 0.34), (0.0, 0.2)] {
                let spot = pitch.chimney_spot(near, along);
                assert!(
                    pitch.surface(spot.x, spot.y) > pitch.rise * 0.6,
                    "a stack on a {pitch:?} at {spot:?} is down the slope"
                );
            }
        }
        // And on a `Giebelhaus` it is exactly where `plume` always put it.
        let old = Pitch::plain(false, 8.0, 14.0, 9.0);
        assert_eq!(old.chimney_spot(0.1, 0.2), Vec2::new(0.8, 2.8));
    }

    /// A tile stays about a hand across on a shed and on a block.
    #[test]
    fn tiles_stay_hand_sized_at_every_roof_size() {
        for up in [4.0f32, 6.0, 9.0, 13.0, 19.0] {
            for along in [6.0f32, 9.0, 14.0, 22.0, 34.0, 48.0] {
                let repeats = lap(bucket(up, along));
                let size = Vec2::new(up, along) / repeats;
                for (axis, got, want) in [("up", size.x, GRAIN.x), ("along", size.y, GRAIN.y)] {
                    assert!(
                        got > want * 0.66 && got < want * 1.5,
                        "a {up}x{along} leaf repeats every {got:.2}m {axis}, wanted {want}"
                    );
                }
            }
        }
        assert!(bucket(4.0, 6.0) < bucket(19.0, 48.0));
        assert_eq!(build_lap_count(), BUCKETS);
    }

    fn build_lap_count() -> usize {
        (0..BUCKETS).map(lap).collect::<Vec<_>>().len()
    }

    /// Dormers stay on the ridge stretch, and a long roof gets a row.
    #[test]
    fn dormers_stay_on_the_ridge() {
        let mut seen = 0;
        for seed in 0..64u64 {
            for pitch in [
                Pitch::plain(true, 12.0, 9.0, 9.0),
                Pitch::plain(false, 9.0, 12.0, 9.0),
                Pitch::plain(false, 36.0, 12.0, 14.0),
            ] {
                for side in [-1.0f32, 1.0] {
                    let pieces = dormers(seed, &pitch, side);
                    seen += pieces.len();
                    let reach = if pitch.hipped {
                        pitch.ridge_length() * 0.5
                    } else {
                        pitch.along * 0.5
                    };
                    for piece in &pieces {
                        assert!(
                            piece.at.x.abs() + DORMER.0 * 0.5 <= reach + 1e-3,
                            "a dormer at {} on a {pitch:?}",
                            piece.at.x
                        );
                        // On the right slope, below the ridge.
                        assert!(piece.at.z * side > 0.0);
                        assert!(piece.at.y < pitch.rise);
                    }
                }
            }
        }
        assert!(seen > 0, "no roof in sixty-four had a dormer");
        // The long roof gets more of them than the house.
        let long = (0..64u64)
            .map(|seed| dormers(seed, &Pitch::plain(false, 36.0, 12.0, 14.0), 1.0).len())
            .max()
            .unwrap_or(0);
        let house = (0..64u64)
            .map(|seed| dormers(seed, &Pitch::plain(false, 9.0, 12.0, 9.0), 1.0).len())
            .max()
            .unwrap_or(0);
        assert!(
            long > house,
            "a 36m roof got {long} dormer pieces and a house {house}"
        );
    }

    /// The whole decision table, style by style and class by class.
    #[test]
    fn the_style_decides_the_roof() {
        use BuildingKind::Apartments;
        let landshut = CityStyle::Landshuepf.roofs();
        let low = rolls(0.5, 0.5, 0.5);

        // The old town keeps its screens at the old share, and what the
        // screens leave is roofed rather than tarred.
        assert_eq!(
            decide(landshut, FacadeClass::House, Apartments, true, None, low),
            Roof::Screened
        );
        assert_eq!(
            decide(
                landshut,
                FacadeClass::Lowrise,
                Apartments,
                true,
                None,
                rolls(0.95, 0.5, 0.5)
            ),
            Roof::Gabled
        );
        assert_eq!(
            decide(
                landshut,
                FacadeClass::Lowrise,
                Apartments,
                true,
                None,
                rolls(0.95, 0.5, 0.1)
            ),
            Roof::Hipped
        );
        // The suburbs are pitched, hipped a quarter of the time, and never
        // screened.
        assert_eq!(
            decide(landshut, FacadeClass::House, Apartments, false, None, low),
            Roof::Gabled
        );
        assert_eq!(
            decide(
                landshut,
                FacadeClass::House,
                Apartments,
                false,
                None,
                rolls(0.1, 0.5, 0.2)
            ),
            Roof::Hipped
        );
        assert_eq!(
            decide(
                landshut,
                FacadeClass::House,
                Apartments,
                false,
                None,
                rolls(0.1, 0.95, 0.2)
            ),
            Roof::Flat
        );
        // The middle class hips or stays flat; a tower is flat.
        assert_eq!(
            decide(
                landshut,
                FacadeClass::Midrise,
                Apartments,
                false,
                None,
                rolls(0.1, 0.5, 0.5)
            ),
            Roof::Hipped
        );
        assert_eq!(
            decide(
                landshut,
                FacadeClass::Midrise,
                Apartments,
                true,
                None,
                rolls(0.1, 0.7, 0.5)
            ),
            Roof::Flat
        );
        assert_eq!(
            decide(
                landshut,
                FacadeClass::Tower,
                Apartments,
                true,
                None,
                rolls(0.1, 0.1, 0.1)
            ),
            Roof::Flat
        );
        // The kinds that are not houses keep the roof they had: a screen at
        // the old share on a low one, a slab otherwise, anywhere.
        for kind in [
            BuildingKind::Offices,
            BuildingKind::Supermarket,
            BuildingKind::Tower,
        ] {
            assert_eq!(
                decide(landshut, FacadeClass::Lowrise, kind, false, None, low),
                Roof::Screened
            );
            assert_eq!(
                decide(
                    landshut,
                    FacadeClass::Lowrise,
                    kind,
                    false,
                    None,
                    rolls(0.95, 0.1, 0.1)
                ),
                Roof::Flat
            );
            assert_eq!(
                decide(
                    landshut,
                    FacadeClass::Midrise,
                    kind,
                    true,
                    None,
                    rolls(0.1, 0.1, 0.1)
                ),
                Roof::Flat
            );
        }
        // The map overrides where it speaks — except that a gable tag on an
        // old-town house is the roof behind its screen.
        for (shape, want) in [
            (RoofShape::Flat, Roof::Flat),
            (RoofShape::Hipped, Roof::Hipped),
            (RoofShape::HalfHipped, Roof::Hipped),
            (RoofShape::Pyramidal, Roof::Hipped),
            (RoofShape::Gabled, Roof::Gabled),
            (RoofShape::Skillion, Roof::Flat),
        ] {
            assert_eq!(
                decide(
                    landshut,
                    FacadeClass::House,
                    Apartments,
                    false,
                    Some(shape),
                    low
                ),
                want,
                "{shape:?} in the suburbs"
            );
        }
        assert_eq!(
            decide(
                landshut,
                FacadeClass::House,
                Apartments,
                true,
                Some(RoofShape::Gabled),
                low
            ),
            Roof::Screened
        );
        assert_eq!(
            decide(
                landshut,
                FacadeClass::House,
                Apartments,
                true,
                Some(RoofShape::Hipped),
                low
            ),
            Roof::Hipped
        );

        // Minga is pitched too, with a few screens in its middle.
        let minga = CityStyle::Minga.roofs();
        assert_eq!(
            decide(minga, FacadeClass::House, Apartments, false, None, low),
            Roof::Gabled
        );
        assert_eq!(
            decide(
                minga,
                FacadeClass::House,
                Apartments,
                true,
                None,
                rolls(0.1, 0.5, 0.5)
            ),
            Roof::Screened
        );
        assert_eq!(
            decide(
                minga,
                FacadeClass::House,
                Apartments,
                false,
                None,
                rolls(0.1, 0.5, 0.5)
            ),
            Roof::Gabled
        );

        // Everywhere else is the flat city it was, whatever the rolls.
        for style in [
            CityStyle::Generisch,
            CityStyle::NewDork,
            CityStyle::Londoof,
            CityStyle::Paree,
        ] {
            for class in FacadeClass::ALL {
                for core in [false, true] {
                    assert_eq!(
                        decide(
                            style.roofs(),
                            class,
                            Apartments,
                            core,
                            None,
                            rolls(0.0, 0.0, 0.0)
                        ),
                        Roof::Flat,
                        "{style:?} pitched a {class:?}"
                    );
                }
            }
        }
    }

    /// The screen roll is the expression it always was, and the other two
    /// come from somewhere else.
    #[test]
    fn the_rolls_are_independent_and_the_screen_one_is_unchanged() {
        for seed in [0u64, 1, 0xDEAD_BEEF, u64::MAX / 3] {
            let rolls = Rolls::from_seed(seed);
            assert_eq!(rolls.screen, (seed >> 31) as f32 / u32::MAX as f32 % 1.0);
            assert!((0.0..1.0).contains(&rolls.pitch));
            assert!((0.0..1.0).contains(&rolls.hip));
            assert!(rolls.age < AGES);
            assert_ne!(rolls.pitch, rolls.hip);
        }
    }
}
