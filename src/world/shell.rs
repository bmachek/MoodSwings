//! The wall, as geometry rather than as a picture of one.
//!
//! Every building in this city was a scaled cube with a facade painted onto it.
//! That carries a surprising distance — the normal map bevels the panes, the
//! grain shader gives it material — but it fails in exactly one situation, and
//! it is the situation the player is in most of the time: standing on the
//! pavement looking along a street. At a glancing angle a painted reveal has no
//! parallax and no silhouette. Every window in the street lies in one plane,
//! and the eye reads the whole row as a poster.
//!
//! So the near wall is a real shell: panes set back behind the plane of the
//! wall with jambs around them, string courses at the floor lines, a cornice at
//! the top, a sign board over the shopfronts, and balconies where the class of
//! building would have them.
//!
//! ## Everything here is a fraction of the window grid
//!
//! There is no metre in this file. A reveal is a fraction of a *bay*, a course
//! is a fraction of a *storey*, and both come out of [`FacadeClass::grid`] —
//! the same grid `world::texture` paints the windows on.
//!
//! That is not tidiness, it is the only thing that makes one mesh serve four
//! thousand buildings. The shell is built in unit-cube space and scaled by the
//! building's transform exactly like the box it replaces, so a length baked
//! into the mesh comes out multiplied by whatever that building happens to
//! measure — a twelve-centimetre reveal on a house would be half a metre on a
//! tower. Expressed as a fraction of a bay it survives the scaling, because a
//! bay is scaled by the same number.
//!
//! The one place it does not survive exactly: a wall's depth axis is scaled by
//! the footprint's *other* side. A building twice as deep as it is wide gets
//! reveals twice as deep on its short faces. Lots are subdivided by always
//! splitting the longer side, so they stay roughly square and the error stays
//! under a factor of two — on a two-hundred-millimetre reveal, nothing anybody
//! will ever see.
//!
//! ## Two levels, and what separates them
//!
//! [`Detail::Full`] has all of it. [`Detail::Coarse`] keeps only the horizontal
//! courses — because those are what still read at two hundred metres, where a
//! reveal is under a pixel and all it contributes is aliasing. Below that the
//! plain box takes over, and the three of them are the level-of-detail chain in
//! `world::buildings`.
//!
//! Both levels put the wall surface in the same plane with the same UVs, so a
//! crossfade between them changes the relief and nothing else.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};

use super::buildings::with_tangents;
use super::texture::{self, FacadeClass, Pane};

/// How thick a window frame is, as a fraction of the opening.
///
/// A sash is about eighty millimetres on a window a metre and a half across,
/// which is a twentieth. Rather more here, because the thing this is for is a
/// *silhouette* at pavement distance and a twentieth of an opening is under a
/// pixel at thirty metres — the shell's whole argument about reveals, applied
/// to the reveal's contents.
const FRAME: f32 = 0.075;

/// And how far in front of the glass it sits, as a fraction of the reveal.
const FRAME_STAND: f32 = 0.45;

/// Where the bar across a window goes, up the opening.
///
/// Two thirds, not a half. A German casement is a tall light over a short one
/// and the transom sits high; centred it reads as a shop door.
const TRANSOM: f32 = 0.62;

/// How far a pane is set back behind the wall, as a fraction of one bay.
const REVEAL: f32 = 0.045;

/// How far a *shopfront* is set back, which is further.
///
/// A shop window is a recess you can stand in out of the rain, with a doorway
/// somewhere in it and usually a step. At street level it is the deepest thing
/// on the facade and the only one the player walks past close enough to read.
const REVEAL_SHOP: f32 = 0.13;

/// How far a string course stands proud of the wall, as a fraction of a bay.
const PROUD: f32 = 0.055;

/// Thickness of a string course, as a fraction of one storey.
const COURSE: f32 = 0.055;

/// The cornice: the one course that has to read from across a junction, so it
/// is both thicker and deeper than the storey courses under it.
const CORNICE: f32 = 0.20;
const CORNICE_PROUD: f32 = 0.11;

/// The sign board over a shopfront, as a fraction of a bay.
const FASCIA_PROUD: f32 = 0.075;

/// A window sill: shallow, and the only reason it exists is the shadow it
/// throws down the wall under it when the sun is anywhere but overhead.
const SILL_PROUD: f32 = 0.045;
const SILL: f32 = 0.022;

/// A balcony slab and the parapet standing on it, as fractions of a bay and a
/// storey respectively.
const BALCONY_OUT: f32 = 0.22;
const BALCONY_SLAB: f32 = 0.035;
const BALCONY_RAIL: f32 = 0.26;
/// How far the parapet stands in from the edge of its own slab.
const BALCONY_INSET: f32 = 0.035;

/// An awning over a shop window, as fractions of a bay and a storey.
const AWNING_OUT: f32 = 0.30;
const AWNING_DROP: f32 = 0.16;
/// How far in from the sides of the bay it stops.
const AWNING_SIDES: f32 = 0.06;
/// Its thickness, as a fraction of a storey.
const AWNING_THICK: f32 = 0.022;

/// How many balcony and awning patterns exist per class.
///
/// Two rather than one because a residential street is a row of buildings of
/// the same class, and one pattern would put every balcony in the city on the
/// same bay of the same floor. Two rather than five because the patterns are
/// baked into shared meshes, and every one of them is a mesh per class.
pub const VARIANTS: u32 = 2;

// ----------------------------------------------------------------- doors ----

/// The doorway, as fractions of the middle ground-storey bay of `Face::PosZ`.
///
/// One description, three readers, exactly like the window grid above it: the
/// shell carves this opening, the compound collider in `world::buildings`
/// leaves the same gap, and `world::interior` builds its room to the same
/// head height. They agree to the millimetre or the player either walks
/// through a painted wall or bumps into an open door — the two failures this
/// function exists to make impossible to write separately.
///
/// Per class rather than one constant, because a bay narrows as the grid
/// densifies: a tower's bay on the same wall is half a lowrise's, and the
/// same fraction of it is a letterbox. A wider fraction of a narrower bay
/// keeps the metric width a door — held by the tests.
pub fn door_span(class: FacadeClass) -> (f32, f32) {
    match class {
        FacadeClass::House | FacadeClass::Lowrise => (0.32, 0.68),
        FacadeClass::Midrise => (0.22, 0.78),
        FacadeClass::Tower => (0.15, 0.85),
    }
}

/// Which column of the grid carries the door: the middle bay. Every
/// shopfront class has an odd column count, so the middle bay is centred on
/// the wall and the door lands on the building's axis — asserted in the
/// tests, because the collider below assumes it.
pub fn door_column(class: FacadeClass) -> u32 {
    (class.grid().0 as u32) / 2
}

/// The doorway's width in metres on a wall `width` metres across.
pub fn door_width(class: FacadeClass, width: f32) -> f32 {
    let span = door_span(class);
    (span.1 - span.0) * width / class.grid().0
}

/// Metres from the pavement to the door head on a building `height` tall:
/// the top of the shopfront glass, which is where the opening stops.
pub fn door_head(class: FacadeClass, height: f32) -> f32 {
    height / class.grid().1 * class.pane(0).v1
}

/// How much of the shell survives to this distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// Reveals, sills, balconies, awnings and every course.
    Full,
    /// The horizontal courses only.
    Coarse,
}

impl Detail {
    const ALL: [Detail; 2] = [Detail::Full, Detail::Coarse];

    fn index(self) -> usize {
        match self {
            Detail::Full => 0,
            Detail::Coarse => 1,
        }
    }
}

// ----------------------------------------------------------------- faces ----

/// One of the four walls, as a frame turning facade coordinates into positions
/// on the unit cube.
///
/// `u` runs along the wall from 0 to 1, `v` up it, and `depth` *into* it — so a
/// reveal is a positive depth and a cornice is a negative one. The mapping is
/// the one `buildings::unit_cube_mesh` uses, and it has to stay that way: the
/// whole argument for this module is that the facade texture lands on the shell
/// in exactly the place it lands on the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Face {
    PosZ,
    NegZ,
    PosX,
    NegX,
}

impl Face {
    const ALL: [Face; 4] = [Face::PosZ, Face::NegZ, Face::PosX, Face::NegX];

    fn at(self, u: f32, v: f32, depth: f32) -> Vec3 {
        match self {
            Face::PosZ => Vec3::new(u - 0.5, v - 0.5, 0.5 - depth),
            Face::NegZ => Vec3::new(0.5 - u, v - 0.5, depth - 0.5),
            Face::PosX => Vec3::new(0.5 - depth, v - 0.5, 0.5 - u),
            Face::NegX => Vec3::new(depth - 0.5, v - 0.5, u - 0.5),
        }
    }

    /// The outward normal, derived rather than written down, so it cannot
    /// disagree with [`Face::at`].
    fn out(self) -> Vec3 {
        self.at(0.0, 0.0, 0.0) - self.at(0.0, 0.0, 1.0)
    }

    /// The direction `u` runs in.
    fn along(self) -> Vec3 {
        self.at(1.0, 0.0, 0.0) - self.at(0.0, 0.0, 0.0)
    }

    /// Whether this face's share of a course's flat top and bottom runs out
    /// past the corners, or stops at them.
    ///
    /// Those two surfaces are one annulus split four ways, so each corner
    /// square belongs to exactly one face or two coplanar quads fight for the
    /// depth buffer. The pair running along Z take theirs; the pair running
    /// along X stop at the wall line. (The *outer* faces of a course are four
    /// separate planes and all four run the full width — see `course`.)
    fn owns_corners(self) -> bool {
        matches!(self, Face::PosZ | Face::NegZ)
    }
}

// --------------------------------------------------------------- building ----

/// A mesh under construction.
#[derive(Default)]
struct Shell {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// A rectangle in facade coordinates: `(from, to)` on each axis.
type Span = (f32, f32);

fn uv(u: f32, v: f32) -> Vec2 {
    Vec2::new(u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
}

impl Shell {
    /// One quad, wound so that it faces `normal` whichever order its corners
    /// arrive in.
    ///
    /// A back-to-front winding is invisible until the wall it is on happens to
    /// face away from the camera, at which point a building has a hole in it.
    /// Checking here costs a cross product per quad at startup and removes the
    /// possibility from fifty call sites.
    fn quad(&mut self, corners: [Vec3; 4], uvs: [Vec2; 4], normal: Vec3) {
        let facing = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        let order = if facing.dot(normal) < 0.0 {
            [3, 2, 1, 0]
        } else {
            [0, 1, 2, 3]
        };

        let base = self.positions.len() as u32;
        for i in order {
            self.positions.push(corners[i].to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uvs[i].to_array());
        }
        self.indices
            .extend([base, base + 1, base + 2, base + 2, base + 3, base]);
    }

    /// The frame in one opening: a border, a transom and a mullion.
    ///
    /// Five quads, all of them at one depth in the reveal, and all of them
    /// coloured from a band of wall — see [`Shell::band`]. Not a box: the frame
    /// stands two centimetres in front of the glass, and the edge of that is a
    /// thing nobody has ever seen on a building from a pavement.
    fn window_frame(&mut self, face: Face, u: Span, v: Span, reveal: f32, masonry: Span) {
        let depth = reveal * FRAME_STAND;
        let (wide, tall) = (u.1 - u.0, v.1 - v.0);
        let (bar_u, bar_v) = (wide * FRAME, tall * FRAME);
        // The border.
        self.band(face, (u.0, u.0 + bar_u), v, depth, masonry);
        self.band(face, (u.1 - bar_u, u.1), v, depth, masonry);
        self.band(face, u, (v.0, v.0 + bar_v), depth, masonry);
        self.band(face, u, (v.1 - bar_v, v.1), depth, masonry);
        // The transom, and the mullion under it. Together they make the pane a
        // window with three lights in it rather than one sheet of glass, which
        // is what every window on a street like this actually is.
        let bar = v.0 + tall * TRANSOM;
        self.band(
            face,
            u,
            (bar - bar_v * 0.5, bar + bar_v * 0.5),
            depth,
            masonry,
        );
        let middle = (u.0 + u.1) * 0.5;
        self.band(
            face,
            (middle - bar_u * 0.5, middle + bar_u * 0.5),
            (v.0, bar),
            depth,
            masonry,
        );
    }

    /// A rectangle parallel to the wall, `depth` in from it, taking its colour
    /// from a band of facade that need not be the one it stands in front of.
    fn band(&mut self, face: Face, u: Span, v: Span, depth: f32, texture_v: Span) {
        self.quad(
            [
                face.at(u.0, v.0, depth),
                face.at(u.1, v.0, depth),
                face.at(u.1, v.1, depth),
                face.at(u.0, v.1, depth),
            ],
            [
                uv(u.0, texture_v.0),
                uv(u.1, texture_v.0),
                uv(u.1, texture_v.1),
                uv(u.0, texture_v.1),
            ],
            face.out(),
        );
    }

    /// A rectangle lying in the wall, coloured by where it lies.
    fn panel(&mut self, face: Face, u: Span, v: Span, depth: f32) {
        self.band(face, u, v, depth, v);
    }

    /// A vertical strip at one `u`, running from one depth to another: the side
    /// of a reveal, or the end of a balcony.
    ///
    /// `texture_u` is where on the wall it takes its colour from, which is
    /// never where it stands — a jamb is wall, and the wall beside it is the
    /// only place on the facade guaranteed not to be glass.
    fn jamb(&mut self, face: Face, u: f32, v: Span, depth: Span, texture_u: Span, out: f32) {
        let normal = face.along() * out;
        self.quad(
            [
                face.at(u, v.0, depth.0),
                face.at(u, v.0, depth.1),
                face.at(u, v.1, depth.1),
                face.at(u, v.1, depth.0),
            ],
            [
                uv(texture_u.0, v.0),
                uv(texture_u.1, v.0),
                uv(texture_u.1, v.1),
                uv(texture_u.0, v.1),
            ],
            normal,
        );
    }

    /// A horizontal strip at one `v`: the head or the sill of a reveal, the top
    /// or the underside of a course.
    fn soffit(&mut self, face: Face, u: Span, v: f32, depth: Span, texture_v: Span, up: f32) {
        self.quad(
            [
                face.at(u.0, v, depth.0),
                face.at(u.1, v, depth.0),
                face.at(u.1, v, depth.1),
                face.at(u.0, v, depth.1),
            ],
            [
                uv(u.0, texture_v.0),
                uv(u.1, texture_v.0),
                uv(u.1, texture_v.1),
                uv(u.0, texture_v.1),
            ],
            Vec3::Y * up,
        );
    }

    /// A box standing proud of one wall: a balcony slab, its parapet, a sill.
    /// Five faces; the sixth is against the wall and is never seen.
    ///
    /// Every one of them is coloured from `masonry` — a rectangle of facade
    /// that is known to be wall — rather than from where it happens to stand.
    /// A balcony parapet hangs in front of the glass, and sampled by position
    /// it comes out painted with a window: a second pane, floating in front of
    /// the real one and displaced from it by its own parallax, which looks
    /// exactly as strange as it sounds.
    ///
    /// It also settles the other half of the problem for nothing. One texture
    /// column is a zero-area rectangle in UV space, and a zero-area UV triangle
    /// is where mikktspace hands back a tangent basis of nothing and the normal
    /// map goes black; a rectangle of wall has area on both axes by definition.
    fn proud(&mut self, face: Face, u: Span, v: Span, out: f32, masonry: (Span, Span)) {
        let ((tu0, tu1), (tv0, tv1)) = masonry;
        let colour = [uv(tu0, tv0), uv(tu1, tv0), uv(tu1, tv1), uv(tu0, tv1)];

        self.quad(
            [
                face.at(u.0, v.0, -out),
                face.at(u.1, v.0, -out),
                face.at(u.1, v.1, -out),
                face.at(u.0, v.1, -out),
            ],
            colour,
            face.out(),
        );
        for (at, up) in [(v.1, 1.0f32), (v.0, -1.0)] {
            self.quad(
                [
                    face.at(u.0, at, -out),
                    face.at(u.1, at, -out),
                    face.at(u.1, at, 0.0),
                    face.at(u.0, at, 0.0),
                ],
                colour,
                Vec3::Y * up,
            );
        }
        for (at, side) in [(u.0, -1.0f32), (u.1, 1.0)] {
            self.quad(
                [
                    face.at(at, v.0, -out),
                    face.at(at, v.0, 0.0),
                    face.at(at, v.1, 0.0),
                    face.at(at, v.1, -out),
                ],
                colour,
                face.along() * side,
            );
        }
    }

    /// A course wrapping the whole building at height `v`, standing `out`
    /// proud.
    ///
    /// Not four `proud` boxes. A course is a rectangular ring, and its two
    /// halves have to be split differently:
    ///
    /// * Its four **outer faces** lie in four distinct planes, so every one of
    ///   them runs the full width of the ring, corners included. Stopping one
    ///   at the wall line leaves a notch you can see daylight through.
    /// * Its **top and bottom** are one flat annulus in a single plane, so they
    ///   have to be split the way a picture frame is — two sides long, two
    ///   short — or the corners are covered twice and fight for the depth
    ///   buffer.
    ///
    /// `masonry` is the band of facade it is coloured from. A storey course sits
    /// on a floor line and can take the wall it stands on; a cornice is deeper
    /// than the gap above the top row of windows and would otherwise come out
    /// with a strip of glass along its underside.
    fn course(&mut self, v: Span, out: f32, masonry: Span) {
        let ring = (-out, 1.0 + out);
        for face in Face::ALL {
            self.band(face, ring, v, -out, masonry);

            let over = if face.owns_corners() { out } else { 0.0 };
            let frame = (-over, 1.0 + over);
            self.soffit(face, frame, v.1, (-out, 0.0), masonry, 1.0);
            self.soffit(face, frame, v.0, (-out, 0.0), masonry, -1.0);
        }
    }

    /// The flat top and bottom of the box, with the unit cube's own UVs so the
    /// roof of a shell and the roof of a box look the same from above.
    fn caps(&mut self) {
        self.quad(
            [
                Vec3::new(-0.5, 0.5, 0.5),
                Vec3::new(0.5, 0.5, 0.5),
                Vec3::new(0.5, 0.5, -0.5),
                Vec3::new(-0.5, 0.5, -0.5),
            ],
            [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)],
            Vec3::Y,
        );
        self.quad(
            [
                Vec3::new(-0.5, -0.5, -0.5),
                Vec3::new(0.5, -0.5, -0.5),
                Vec3::new(0.5, -0.5, 0.5),
                Vec3::new(-0.5, -0.5, 0.5),
            ],
            [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)],
            Vec3::NEG_Y,
        );
    }

    fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

// ------------------------------------------------------------- the recipe ----

/// How often a class carries a string course, in floors. `None` for a curtain
/// wall, whose horizontals are mullions and belong to the texture.
fn course_every(class: FacadeClass) -> Option<u32> {
    match class {
        // A terraced house has one band, at first-floor level.
        FacadeClass::House => Some(1),
        FacadeClass::Lowrise => Some(1),
        FacadeClass::Midrise => Some(3),
        // Not every floor of a tower, but the plant floors are real and they
        // are what break a hundred metres of glass into something with a scale.
        FacadeClass::Tower => Some(6),
    }
}

/// Whether this kind of building hangs balconies off its facade.
///
/// A terraced house has a back garden instead, and a curtain-walled tower
/// cannot open a window at all.
fn has_balconies(class: FacadeClass) -> bool {
    matches!(class, FacadeClass::Lowrise | FacadeClass::Midrise)
}

/// Which cells carry a balcony, given the building's variant.
fn balcony_at(class: FacadeClass, column: u32, row: u32, variant: u32) -> bool {
    has_balconies(class) && row >= 1 && (column + row + variant).is_multiple_of(3)
}

/// Which shopfronts have an awning out.
fn awning_at(class: FacadeClass, column: u32, variant: u32) -> bool {
    class.has_shopfronts() && (column + variant).is_multiple_of(2)
}

/// The doorway cell: the middle ground bay with an actual hole in it.
///
/// The cell keeps its shopfront look — the wall either side of the opening
/// still lies where the texture paints glass, which is what a shop door in
/// the middle of a shop window is — but between the jambs there is nothing:
/// no pane, no stallriser, an opening from the pavement to the head. What
/// shows through it is `world::interior`'s room.
fn doorway(
    mesh: &mut Shell,
    class: FacadeClass,
    face: Face,
    cell: Span,
    size: (f32, f32),
    pane: &Pane,
) {
    let (cell_u, cell_v) = size;
    let span = door_span(class);
    let width = cell.1 - cell.0;
    let u0 = cell.0 + span.0 * width;
    let u1 = cell.0 + span.1 * width;
    let v1 = pane.v1 * cell_v;
    let reveal = REVEAL_SHOP * cell_u;

    // The wall over the head and either side of the opening, tiling the rest
    // of the cell exactly — any gap beyond the doorway is a hole through the
    // building that was not asked for.
    mesh.panel(face, cell, (v1, cell_v), 0.0);
    mesh.panel(face, (cell.0, u0), (0.0, v1), 0.0);
    mesh.panel(face, (u1, cell.1), (0.0, v1), 0.0);

    // Jambs and head, coloured from the wall beside them like every reveal.
    let inset = (span.0 * width).min(u0 - cell.0) * 0.5;
    mesh.jamb(face, u0, (0.0, v1), (0.0, reveal), (u0, u0 - inset), 1.0);
    mesh.jamb(face, u1, (0.0, v1), (0.0, reveal), (u1, u1 + inset), -1.0);
    mesh.soffit(
        face,
        (u0, u1),
        v1,
        (0.0, reveal),
        (v1, v1 + (1.0 - pane.v1) * cell_v * 0.5),
        -1.0,
    );
}

/// Builds one shell.
fn shell(class: FacadeClass, detail: Detail, variant: u32, doored: bool) -> Mesh {
    let (columns, rows) = class.grid();
    let (columns, rows) = (columns as u32, rows as u32);
    let (cell_u, cell_v) = (1.0 / columns as f32, 1.0 / rows as f32);

    // Everything horizontal is a fraction of a bay; everything vertical is a
    // fraction of a storey. See the module docs — this is the whole trick.
    let proud = PROUD * cell_u;

    let mut mesh = Shell::default();
    mesh.caps();

    for face in Face::ALL {
        if detail == Detail::Coarse {
            // One flat wall, in the same plane and with the same UVs as the
            // panes-and-webbing version, so the crossfade only swaps relief.
            mesh.panel(face, (0.0, 1.0), (0.0, 1.0), 0.0);
            continue;
        }

        for row in 0..rows {
            let pane = class.pane(row);
            let (cv0, cv1) = (row as f32 * cell_v, (row as f32 + 1.0) * cell_v);
            let v0 = cv0 + pane.v0 * cell_v;
            let v1 = cv0 + pane.v1 * cell_v;

            let reveal = if pane.ground { REVEAL_SHOP } else { REVEAL } * cell_u;

            for column in 0..columns {
                let (cu0, cu1) = (column as f32 * cell_u, (column as f32 + 1.0) * cell_u);

                // The door bay, on the front face only. `continue` rather
                // than a flag: nothing of the ordinary cell survives here.
                if doored && face == Face::PosZ && pane.ground && column == door_column(class) {
                    doorway(&mut mesh, class, face, (cu0, cu1), (cell_u, cell_v), &pane);
                    if awning_at(class, column, variant) {
                        awning(&mut mesh, face, (cu0, cu1), cv0, (cell_u, cell_v));
                    }
                    continue;
                }

                let u0 = cu0 + pane.u0 * cell_u;
                let u1 = cu0 + pane.u1 * cell_u;

                // The wall around the opening, as four strips that tile the
                // cell exactly. Any gap here is a hole through the building.
                mesh.panel(face, (cu0, cu1), (cv0, v0), 0.0);
                mesh.panel(face, (cu0, cu1), (v1, cv1), 0.0);
                mesh.panel(face, (cu0, u0), (v0, v1), 0.0);
                mesh.panel(face, (u1, cu1), (v0, v1), 0.0);

                // The opening. Jambs take their colour from halfway across the
                // wall beside them, which is inside the cell by construction —
                // see `there_is_wall_left_over_for_a_reveal_to_sit_in`.
                let inset = pane_inset(&pane, cell_u, cell_v);
                mesh.jamb(face, u0, (v0, v1), (0.0, reveal), (u0, u0 - inset.0), 1.0);
                mesh.jamb(face, u1, (v0, v1), (0.0, reveal), (u1, u1 + inset.1), -1.0);
                mesh.soffit(face, (u0, u1), v0, (0.0, reveal), (v0, v0 - inset.2), 1.0);
                mesh.soffit(face, (u0, u1), v1, (0.0, reveal), (v1, v1 + inset.3), -1.0);
                mesh.panel(face, (u0, u1), (v0, v1), reveal);

                // The spandrel under this window: the nearest patch of wall
                // that is guaranteed not to be glass, and what everything
                // standing proud of the facade here is coloured from.
                let masonry = ((cu0, cu1), (cv0, v0));

                // The frame, standing in the reveal in front of the glass.
                //
                // Until this, an opening was a hole with a painted colour in
                // the back of it: reveal, sill, and then flat glass edge to
                // edge. Real glazing is a *border* — a sash, a frame, a
                // transom, a bar down the middle — and it is the single
                // loudest thing missing from a facade seen from a pavement,
                // because it is what turns a dark rectangle into a window.
                //
                // Coloured from the spandrel rather than from where it stands,
                // the same trick `proud` uses and for the same reason: a strip
                // sampled over the glass comes back painted with the glass,
                // and a frame the colour of its own pane is not a frame.
                if !pane.ground {
                    mesh.window_frame(face, (u0, u1), (v0, v1), reveal, masonry.1);
                }

                if balcony_at(class, column, row, variant) {
                    let width = (u0 - proud, u1 + proud);
                    let slab = (v0 - BALCONY_SLAB * cell_v, v0);
                    mesh.proud(face, width, slab, BALCONY_OUT * cell_u, masonry);

                    let rail = (slab.1, slab.1 + BALCONY_RAIL * cell_v);
                    let inset = BALCONY_INSET * cell_u;
                    mesh.proud(
                        face,
                        (width.0 + inset, width.1 - inset),
                        rail,
                        (BALCONY_OUT - BALCONY_INSET) * cell_u,
                        masonry,
                    );
                } else if !pane.ground {
                    // A sill, which is here for its shadow and nothing else.
                    mesh.proud(
                        face,
                        (u0 - proud, u1 + proud),
                        (v0 - SILL * cell_v, v0),
                        SILL_PROUD * cell_u,
                        masonry,
                    );
                }

                if pane.ground && awning_at(class, column, variant) {
                    awning(&mut mesh, face, (cu0, cu1), cv0, (cell_u, cell_v));
                }
            }
        }
    }

    // The horizontal courses, which both levels of detail carry. A course sits
    // on a floor line, which is spandrel either side of it, so it can be
    // coloured by where it stands.
    if let Some(every) = course_every(class) {
        for row in 1..rows {
            if row % every == 0 {
                let line = row as f32 * cell_v;
                let band = (line - COURSE * cell_v * 0.5, line + COURSE * cell_v * 0.5);
                mesh.course(band, proud, band);
            }
        }
    }

    // The sign board over the shops, standing proud of the wall the way a board
    // screwed to one does — and coloured, rightly, as the board the texture
    // already paints there.
    if class.has_shopfronts() {
        let fascia = (texture::FASCIA.0 * cell_v, texture::FASCIA.1 * cell_v);
        mesh.course(fascia, FASCIA_PROUD * cell_u, fascia);
    }

    // And the cornice, which is where a wall stops rather than where it is
    // interrupted, so it runs to the very top. Deep enough to reach down past
    // the head of the top row of windows, so it is coloured from the spandrel
    // under them instead of from the glass it hangs in front of.
    mesh.course(
        (1.0 - CORNICE * cell_v, 1.0),
        CORNICE_PROUD * cell_u,
        top_masonry(class, cell_v),
    );

    with_tangents(mesh.build())
}

/// How far into the wall beside each edge of a pane its jamb should take its
/// colour from: half the wall that is actually there, on all four sides.
fn pane_inset(pane: &Pane, cell_u: f32, cell_v: f32) -> (f32, f32, f32, f32) {
    let [left, right, below, above] = pane.margins();
    (
        left * cell_u * 0.5,
        right * cell_u * 0.5,
        below * cell_v * 0.5,
        above * cell_v * 0.5,
    )
}

/// A shop awning: a sloping board hanging from under the sign board.
///
/// A board with a thickness rather than one quad shown from both sides. Two
/// coplanar quads in the same place is not a two-sided surface, it is a depth
/// fight, and the side it loses on changes with the camera.
fn awning(mesh: &mut Shell, face: Face, cell: Span, cell_v0: f32, size: (f32, f32)) {
    let (cell_u, cell_v) = size;
    let u = (
        cell.0 + AWNING_SIDES * cell_u,
        cell.1 - AWNING_SIDES * cell_u,
    );
    // Hangs from the bottom edge of the sign board and drops as it comes out.
    let high = cell_v0 + texture::FASCIA.0 * cell_v;
    let low = high - AWNING_DROP * cell_v;
    let out = -AWNING_OUT * cell_u;
    let thick = AWNING_THICK * cell_v;

    // Two rails: the upper surface, and the same slope a board's thickness
    // below it.
    let rail = |drop: f32| {
        [
            face.at(u.0, high - drop, 0.0),
            face.at(u.1, high - drop, 0.0),
            face.at(u.1, low - drop, out),
            face.at(u.0, low - drop, out),
        ]
    };
    // Coloured from the sign board above it rather than from where it hangs,
    // which is across the top of the shop window: an awning painted with a pane
    // of glass is the same mistake a balcony parapet makes, one storey down.
    // It is also what a real awning is — the shopfront's own livery.
    let board = (
        cell_v0 + texture::FASCIA.0 * cell_v,
        cell_v0 + texture::FASCIA.1 * cell_v,
    );
    let coords = |_: f32| {
        [
            uv(u.0, board.0),
            uv(u.1, board.0),
            uv(u.1, board.1),
            uv(u.0, board.1),
        ]
    };
    let (top, under) = (rail(0.0), rail(thick));

    // The slope's true normal rather than a guess at one, so the board catches
    // the sun the way its own angle says it should.
    let up = (top[3] - top[0]).cross(top[1] - top[0]).normalize();
    let up = if up.y < 0.0 { -up } else { up };

    mesh.quad(top, coords(0.0), up);
    mesh.quad(under, coords(thick), -up);
    // The front lip and the two ends.
    for (corners, normal) in [
        (
            [top[3], top[2], under[2], under[3]],
            (face.out() - Vec3::Y).normalize(),
        ),
        ([top[0], top[3], under[3], under[0]], -face.along()),
        ([top[1], top[2], under[2], under[1]], face.along()),
    ] {
        mesh.quad(corners, coords(0.0), normal);
    }
}

/// A band of the top storey that is wall rather than window, for the cornice
/// hanging over it to take its colour from.
fn top_masonry(class: FacadeClass, cell_v: f32) -> (f32, f32) {
    let rows = class.grid().1;
    let top = rows - 1.0;
    let pane = class.pane(top as u32);
    ((top * cell_v), (top + pane.v0) * cell_v)
}

// ------------------------------------------------------------------ kit ----

/// Every shell mesh, built once.
///
/// Sixteen of them: four classes, two levels of detail, two variants. That
/// count is the entire budget argument for this module — the geometry is per
/// *kind* of building, not per building, so a city of four thousand costs
/// sixteen meshes and the transforms it already had.
#[derive(Resource)]
pub struct ShellKit {
    meshes: Vec<Handle<Mesh>>,
    /// The doored full-detail shells, one per shopfront class and variant.
    /// `None` for a class with no shopfronts — a house's front door is a
    /// painted one, and a house is never enterable.
    doors: Vec<Option<Handle<Mesh>>>,
}

impl ShellKit {
    pub fn get(&self, class: FacadeClass, detail: Detail, variant: u32) -> Handle<Mesh> {
        self.meshes[index(class, detail, variant)].clone()
    }

    /// The full-detail shell with the doorway carved into `Face::PosZ`, for
    /// buildings the player can walk into. Only the full level is doored:
    /// past the coarse handover the interior is culled anyway, and a hole
    /// into an unlit nothing reads worse than a pane of glass.
    pub fn door(&self, class: FacadeClass, variant: u32) -> Option<Handle<Mesh>> {
        self.doors[class.index() * VARIANTS as usize + (variant % VARIANTS) as usize].clone()
    }
}

fn index(class: FacadeClass, detail: Detail, variant: u32) -> usize {
    (class.index() * VARIANTS as usize + (variant % VARIANTS) as usize) * Detail::ALL.len()
        + detail.index()
}

pub fn build_assets(meshes: &mut Assets<Mesh>) -> ShellKit {
    let started = std::time::Instant::now();

    let mut built = vec![Handle::default(); FacadeClass::ALL.len() * VARIANTS as usize * 2];
    let mut doors = vec![None; FacadeClass::ALL.len() * VARIANTS as usize];
    let mut triangles = 0usize;
    for class in FacadeClass::ALL {
        for variant in 0..VARIANTS {
            for detail in Detail::ALL {
                let mesh = shell(class, detail, variant, false);
                triangles += mesh.indices().map_or(0, |i| i.len()) / 3;
                built[index(class, detail, variant)] = meshes.add(mesh);
            }
            if class.has_shopfronts() {
                let mesh = shell(class, Detail::Full, variant, true);
                triangles += mesh.indices().map_or(0, |i| i.len()) / 3;
                doors[class.index() * VARIANTS as usize + variant as usize] =
                    Some(meshes.add(mesh));
            }
        }
    }

    info!(
        "facade shells built in {:.0}ms: {} meshes, {} triangles",
        started.elapsed().as_secs_f32() * 1000.0,
        built.len() + doors.iter().flatten().count(),
        triangles,
    );
    ShellKit {
        meshes: built,
        doors,
    }
}

// ------------------------------------------------------------------ lod ----

/// Where the full shell gives way to the courses alone, before `lod_scale`.
///
/// Eighty metres was where this started, and eighty metres is inside the
/// street you are standing in. A Landshut canyon runs two hundred metres and
/// the view across the Isar six hundred, so past the first few houses the town
/// *was* the coarse shell — which is four wall quads, two caps and a couple of
/// course rings: sixty triangles for a house, a hundred and thirty for a
/// three-storey block. Everything in this file — the reveals, the sills, the
/// frames, the cornice — was being spent on the two buildings nearest the
/// camera and thrown away on the four hundred behind them. That is the whole
/// of "everything looks like boxes", and it was a constant rather than a mesh.
///
/// A full House shell is 1012 triangles and a Lowrise 3204, and Landshut is
/// House and Lowrise only (`height_cap` 23 m). Four hundred of them resident
/// at full detail is under a million triangles, which is what a hundred and
/// forty pedestrians used to cost between them before their capsules were cut
/// down. It is affordable; it was simply never asked for.
pub const NEAR: f32 = 170.0;
/// Where the courses give way to the plain box.
pub const FAR: f32 = 460.0;

/// The distance past which no amount of quality setting draws a reveal.
///
/// Not a frame budget — a resolution. At 1440p across a sixty-degree field a
/// pixel subtends about four ten-thousandths of a radian, so a two-hundred-
/// millimetre reveal is one pixel wide at five hundred metres and less than one
/// beyond it. Photo mode is entitled to never drop detail it could see; this is
/// where that stops being the same sentence.
const CEILING: f32 = 600.0;

/// The two distances at which a building changes level of detail.
///
/// Returns finite numbers for every preset including Photo, whose `lod_scale`
/// is infinite — a `VisibilityRange` with an infinite start is a mesh that is
/// never drawn at all, which is the opposite of what Photo asked for.
pub fn ranges(lod_scale: f32) -> (f32, f32) {
    let near = (NEAR * lod_scale).min(CEILING);
    let far = (FAR * lod_scale).min(CEILING * FAR / NEAR);
    (near, far.max(near))
}

/// How much of a level-of-detail band is spent crossfading into the next.
const FADE: f32 = 0.88;

/// The range over which `at` fades out and the level behind it fades in.
///
/// One function for both, because Bevy crossfades two levels only if the one
/// in front hands over across precisely the band the one behind takes over on;
/// a millimetre of disagreement is a frame where the building is transparent.
pub fn handover(at: f32) -> std::ops::Range<f32> {
    (at * FADE)..at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::quality::QualityPreset;

    fn triangles(mesh: &Mesh) -> Vec<[Vec3; 3]> {
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            bevy::render::mesh::VertexAttributeValues::Float32x3(p) => p,
            _ => panic!("unexpected position format"),
        };
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("expected u32 indices")
        };
        indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                [
                    Vec3::from(positions[t[0] as usize]),
                    Vec3::from(positions[t[1] as usize]),
                    Vec3::from(positions[t[2] as usize]),
                ]
            })
            .collect()
    }

    /// The reveal is cut into the wall beside the glass, and its jambs take
    /// their colour from that wall. No wall left over and there is nowhere for
    /// either to come from — and the jamb's texture rectangle collapses to a
    /// line, which is the failure `nothing_is_flattened_to_a_line_in_texture_space`
    /// then catches on every building in the city at once.
    #[test]
    fn there_is_wall_left_over_for_a_reveal_to_sit_in() {
        for class in FacadeClass::ALL {
            for row in 0..2 {
                let pane = class.pane(row);
                for margin in pane.margins() {
                    assert!(
                        margin > 0.0,
                        "{class:?} row {row} has glass running into the edge of its cell"
                    );
                }
                // And a reveal on one wall must not meet the one cut into the
                // wall opposite, however narrow the building is.
                assert!(
                    REVEAL.max(REVEAL_SHOP) < 0.5,
                    "a reveal is deeper than half a bay"
                );
            }
        }
    }

    /// Every triangle's stored normal has to agree with the way it is wound, or
    /// back-face culling opens a hole in a wall the moment it turns away.
    #[test]
    fn every_face_is_wound_the_way_it_claims_to_point() {
        for class in FacadeClass::ALL {
            for (detail, doored) in [
                (Detail::Full, false),
                (Detail::Coarse, false),
                (Detail::Full, true),
            ] {
                let mesh = shell(class, detail, 0, doored);
                let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap() {
                    bevy::render::mesh::VertexAttributeValues::Float32x3(n) => n.clone(),
                    _ => panic!("unexpected normal format"),
                };
                let Some(Indices::U32(indices)) = mesh.indices() else {
                    panic!("expected u32 indices")
                };
                for (i, corners) in triangles(&mesh).into_iter().enumerate() {
                    let geometric = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
                    if geometric.length_squared() < 1e-18 {
                        panic!("{class:?}/{detail:?} triangle {i} is degenerate");
                    }
                    let stored = Vec3::from(normals[indices[i * 3] as usize]);
                    assert!(
                        geometric.dot(stored) > 0.0,
                        "{class:?}/{detail:?} triangle {i} is wound against its own normal"
                    );
                }
            }
        }
    }

    /// The shell replaces a unit cube and is scaled by the same transform, so
    /// nothing may reach further out than the courses are meant to project —
    /// a balcony poking through the neighbouring building would be the tell.
    #[test]
    fn nothing_reaches_further_out_than_a_balcony() {
        // The deepest thing on the wall, in bays; the cell is at most half a
        // unit wide, so this is a generous bound rather than a tight one.
        let deepest = BALCONY_OUT.max(AWNING_OUT).max(CORNICE_PROUD);
        for class in FacadeClass::ALL {
            let (columns, _) = class.grid();
            let limit = 0.5 + deepest / columns + 1e-4;
            for detail in Detail::ALL {
                for variant in 0..VARIANTS {
                    for corners in triangles(&shell(class, detail, variant, false)) {
                        for corner in corners {
                            assert!(
                                corner.x.abs() <= limit
                                    && corner.z.abs() <= limit
                                    && corner.y.abs() <= 0.5 + 1e-4,
                                "{class:?}/{detail:?} reaches {corner} past {limit}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Does a ray from `from` along `direction` hit anything?
    ///
    /// Möller-Trumbore, because the only honest way to ask whether a ring of
    /// geometry has a gap in it is to try to shoot through the gap.
    fn hits(mesh: &Mesh, from: Vec3, direction: Vec3) -> bool {
        triangles(mesh).into_iter().any(|[a, b, c]| {
            let (edge_1, edge_2) = (b - a, c - a);
            let across = direction.cross(edge_2);
            let determinant = edge_1.dot(across);
            if determinant.abs() < 1e-12 {
                return false;
            }
            let to_a = from - a;
            let u = to_a.dot(across) / determinant;
            if !(0.0..=1.0).contains(&u) {
                return false;
            }
            let along = to_a.cross(edge_1);
            let v = direction.dot(along) / determinant;
            if v < 0.0 || u + v > 1.0 {
                return false;
            }
            edge_2.dot(along) / determinant > 0.0
        })
    }

    /// A course wraps the building, so its four corners have to be closed.
    ///
    /// The natural way to write one — four boxes, each stopping at the wall
    /// line — leaves a notch at every corner facing two of the four ways, and
    /// it is invisible in a vertex-count check, a bounds check and a winding
    /// check alike. So this stands inside each corner of the cornice and tries
    /// to shoot its way out.
    #[test]
    fn a_course_has_no_gap_at_its_corners() {
        for class in FacadeClass::ALL {
            let (columns, rows) = class.grid();
            let out = CORNICE_PROUD / columns;
            let mesh = shell(class, Detail::Coarse, 0, false);
            // Inside the ring: past the wall, short of the outer face, and
            // halfway up the cornice.
            let corner = 0.5 + out * 0.5;
            let y = 0.5 - CORNICE / rows * 0.5;

            for x in [-1.0f32, 1.0] {
                for z in [-1.0f32, 1.0] {
                    let from = Vec3::new(x * corner, y, z * corner);
                    for direction in [Vec3::X * x, Vec3::Z * z] {
                        assert!(
                            hits(&mesh, from, direction),
                            "{class:?}: a ray escapes the cornice at {from} along {direction}"
                        );
                    }
                }
            }
        }
    }

    /// Nothing that stands proud of the wall may be painted with a window.
    ///
    /// The failure this catches is not subtle once you have seen it and is
    /// invisible until you do: a balcony parapet hangs across the lower half of
    /// its own window, so sampled by position it comes out glazed — a second
    /// pane floating half a metre in front of the real one and displaced from
    /// it by its own parallax. The awning over a shopfront and the underside of
    /// the cornice both reach across glass the same way.
    #[test]
    fn nothing_standing_proud_of_the_wall_is_painted_with_a_window() {
        for class in FacadeClass::ALL {
            let (columns, rows) = class.grid();
            for variant in 0..VARIANTS {
                let mesh = shell(class, Detail::Full, variant, false);
                let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap() {
                    bevy::render::mesh::VertexAttributeValues::Float32x2(uv) => uv.clone(),
                    _ => panic!("unexpected uv format"),
                };
                let Some(Indices::U32(indices)) = mesh.indices() else {
                    panic!("expected u32 indices")
                };

                for (corner, positions) in indices.as_chunks::<3>().0.iter().zip(triangles(&mesh)) {
                    // Anything reaching outside the unit cube stands proud of
                    // the wall rather than lying in it or behind it.
                    let out = positions
                        .iter()
                        .any(|p| p.x.abs() > 0.5 + 1e-4 || p.z.abs() > 0.5 + 1e-4);
                    if !out {
                        continue;
                    }

                    let centre = corner
                        .iter()
                        .map(|&v| Vec2::from(uvs[v as usize]))
                        .sum::<Vec2>()
                        / 3.0;
                    let (su, sv) = (centre.x * columns, centre.y * rows);
                    let (column, row) = (su.floor(), sv.floor());
                    let pane = class.pane(row.max(0.0) as u32);
                    let (fu, fv) = (su - column, sv - row);

                    assert!(
                        fu <= pane.u0 || fu >= pane.u1 || fv <= pane.v0 || fv >= pane.v1,
                        "{class:?}/{variant}: something standing proud of the wall takes \
                         its colour from {centre}, which is inside the glass"
                    );
                }
            }
        }
    }

    /// No triangle may be degenerate in UV space.
    ///
    /// Not an aesthetic point. Mikktspace builds the tangent basis from the UV
    /// derivatives, and a zero-area UV triangle has none — so the tangent comes
    /// back as zero, the normal map is applied against a basis of nothing, and
    /// that face of the building goes black. It is easy to write, because the
    /// natural UV for the side of a twelve-centimetre sill is one texture
    /// column.
    #[test]
    fn nothing_is_flattened_to_a_line_in_texture_space() {
        for class in FacadeClass::ALL {
            for (detail, doored) in [
                (Detail::Full, false),
                (Detail::Coarse, false),
                (Detail::Full, true),
            ] {
                for variant in 0..VARIANTS {
                    let mesh = shell(class, detail, variant, doored);
                    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap() {
                        bevy::render::mesh::VertexAttributeValues::Float32x2(uv) => uv.clone(),
                        _ => panic!("unexpected uv format"),
                    };
                    let Some(Indices::U32(indices)) = mesh.indices() else {
                        panic!("expected u32 indices")
                    };
                    for (i, corner) in indices.as_chunks::<3>().0.iter().enumerate() {
                        let [a, b, c] = corner.map(|v| Vec2::from(uvs[v as usize]));
                        let area = (b - a).perp_dot(c - a).abs() * 0.5;
                        assert!(
                            area > 1e-9,
                            "{class:?}/{detail:?}/{variant} triangle {i} is a line in UV space"
                        );
                    }
                }
            }
        }
    }

    /// The point of the coarse level is that it is cheap. If it ever stops
    /// being an order of magnitude lighter it has stopped being a level.
    #[test]
    fn the_coarse_shell_is_a_fraction_of_the_full_one() {
        for class in FacadeClass::ALL {
            let full = triangles(&shell(class, Detail::Full, 0, false)).len();
            let coarse = triangles(&shell(class, Detail::Coarse, 0, false)).len();
            assert!(
                coarse * 8 < full,
                "{class:?}: {coarse} coarse triangles against {full} full ones is not a saving"
            );
        }
    }

    /// Two variants that produce the same mesh are two slots wasted on one
    /// building, and a street of identical balconies.
    #[test]
    fn the_variants_actually_differ_where_they_are_meant_to() {
        for class in FacadeClass::ALL {
            let a = triangles(&shell(class, Detail::Full, 0, false));
            let b = triangles(&shell(class, Detail::Full, 1, false));
            let differs = a.len() != b.len() || a.iter().zip(&b).any(|(x, y)| x != y);
            assert_eq!(
                differs,
                has_balconies(class) || class.has_shopfronts(),
                "{class:?} variants disagree with what it is supposed to carry"
            );
        }
    }

    /// Every kit slot is filled by exactly one combination, or a tower quietly
    /// gets a house's windows.
    #[test]
    fn every_slot_in_the_kit_is_addressed_once() {
        let mut seen = std::collections::HashSet::new();
        for class in FacadeClass::ALL {
            for detail in Detail::ALL {
                for variant in 0..VARIANTS {
                    assert!(
                        seen.insert(index(class, detail, variant)),
                        "index collision at {class:?}/{detail:?}/{variant}"
                    );
                }
            }
        }
        assert_eq!(seen.len(), FacadeClass::ALL.len() * 2 * VARIANTS as usize);
        assert_eq!(seen.iter().copied().max(), Some(seen.len() - 1));
    }

    /// Out-of-range variants have to land somewhere rather than panic: they
    /// come from a hash of the building's position.
    #[test]
    fn a_variant_from_anywhere_still_addresses_the_kit() {
        let slots = FacadeClass::ALL.len() * 2 * VARIANTS as usize;
        for variant in [0, 1, 2, 7, u32::MAX] {
            assert!(index(FacadeClass::Tower, Detail::Full, variant) < slots);
        }
    }

    /// Every preset has to produce a usable chain, including the one whose LOD
    /// scale is infinite.
    #[test]
    fn the_levels_are_ordered_and_finite_at_every_preset() {
        for preset in QualityPreset::ALL {
            let (near, far) = ranges(preset.settings().lod_scale);
            assert!(
                near.is_finite() && far.is_finite(),
                "{} produced a level of detail at infinity",
                preset.name()
            );
            assert!(
                near > 0.0 && far >= near,
                "{}: {near} then {far}",
                preset.name()
            );
            assert!(
                near <= CEILING,
                "{} draws reveals past the ceiling",
                preset.name()
            );
        }
    }

    /// The doored shell's opening is open, and only the doored shell's.
    ///
    /// The same honest question the cornice test asks, pointed the other way:
    /// stand where the player will stand — in the middle of the doorway, at
    /// waist height — and shoot out through the front wall. Through the
    /// doored shell the ray escapes; through the plain one it must not,
    /// because that hole would be a window pane somebody forgot to draw.
    #[test]
    fn the_doorway_is_actually_open() {
        for class in FacadeClass::ALL {
            if !class.has_shopfronts() {
                continue;
            }
            let rows = class.grid().1;
            let waist = class.pane(0).v1 / rows * 0.5 - 0.5;
            let from = Vec3::new(0.0, waist, 0.0);

            let doored = shell(class, Detail::Full, 0, true);
            assert!(
                !hits(&doored, from, Vec3::Z),
                "{class:?}: the doorway is painted shut"
            );
            let plain = shell(class, Detail::Full, 0, false);
            assert!(
                hits(&plain, from, Vec3::Z),
                "{class:?}: the plain shell has a hole in its front wall"
            );
        }
    }

    /// The door contract the collider and the interior build against.
    #[test]
    fn the_door_contract_stays_inside_the_shopfront() {
        for class in FacadeClass::ALL {
            if !class.has_shopfronts() {
                continue;
            }
            let span = door_span(class);
            assert!(span.0 > 0.0 && span.1 < 1.0 && span.0 < span.1, "{class:?}");
            let pane = class.pane(0);
            // The opening sits inside the glass span, so the wall strips
            // beside it are painted shopfront rather than half a window.
            assert!(span.0 > pane.u0 && span.1 < pane.u1, "{class:?}");
            // The head stops under the fascia band the sign hangs on.
            assert!(pane.v1 < texture::FASCIA.0, "{class:?}");
            // An odd column count centres the middle bay — and with it the
            // door — on the building's axis, which the compound collider in
            // `world::buildings` takes as given.
            assert_eq!(class.grid().0 as u32 % 2, 1, "{class:?}");
            // And a citizen actually fits through, on the narrowest building
            // the city plats.
            assert!(
                door_width(class, 14.0) > 0.75,
                "{class:?}: {} m is not a door, it is a letterbox",
                door_width(class, 14.0)
            );
        }
    }

    /// A crossfade only works if the level in front hands over across exactly
    /// the band the level behind takes over on.
    #[test]
    fn the_levels_hand_over_across_the_same_band() {
        let (near, far) = ranges(1.0);
        assert_eq!(handover(near), handover(near));
        assert!(handover(near).start < handover(near).end);
        assert!(handover(near).end <= handover(far).start);
    }
}
