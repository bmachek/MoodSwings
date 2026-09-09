//! Things that grow.
//!
//! There was not a single tree in this city. A park was a green rectangle and a
//! residential street was two rows of boxes with a bin between them, and no
//! amount of work on the renderer was going to fix either — a street reads as a
//! street partly because something on it is *soft*, and every surface here was
//! flat, hard and man-made.
//!
//! Three decisions carry the whole module:
//!
//! * **Two entities per tree.** A trunk whose origin is at its foot, and one
//!   crown as its child. The crown is several blobs merged into a single mesh
//!   at build time rather than several children at spawn time, because the
//!   silhouette is what distinguishes a plane from a poplar and merging costs
//!   nothing once, per species, at startup.
//! * **The trunk pivots.** Its mesh is translated so that the origin sits on
//!   the ground, which means the wind can lean the whole tree by writing a
//!   rotation and nothing else — no per-vertex animation, no custom material,
//!   and the crown comes with it because it is a child.
//! * **Only what is on screen sways.** The sway system skips anything the
//!   camera cannot see. Writing `Transform` marks an entity for transform
//!   propagation and a fresh upload of its instance data, so animating every
//!   tree in a nine-hundred-metre radius would cost far more than drawing them.
//!
//! Placement follows the same rule as everything else streamed: it is drawn
//! from the chunk's own RNG stream, so walking out of a chunk and back into it
//! regrows exactly the same trees.

use bevy::camera::visibility::{ViewVisibility, VisibilityRange};
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::citygen::{Block, District};
use super::roadgraph::RoadEdge;
use super::weather::Weather;

/// Metres between street trees along a kerb.
const SPACING: f32 = 17.0;
/// How far in from the kerb line a trunk stands.
const SET_BACK: f32 = 1.15;

/// How much room a trunk needs to not be standing in a road, in metres.
const TRUNK_ROOM: f32 = 0.55;
/// Fraction of streets that are planted at all.
///
/// Not all of them, and not at random per tree: a street either is an avenue or
/// it is not, and half a row of trees reads as trees that have died.
const AVENUE: f32 = 0.42;
/// How much likelier an arterial road is to be an avenue.
const AVENUE_ARTERIAL: f32 = 1.7;

/// Is this street an avenue?
///
/// Decided from the street's *name* where the extract gave one, and only from
/// the segment where it did not. The distinction is the difference between a
/// row of trees and a row of gaps: a curved Altstadt street arrives as a dozen
/// segments, and rolling per segment plants trees down four of them and leaves
/// eight bare, which reads as an avenue somebody has been cutting down. One
/// council plants one street.
fn avenue(edge: &RoadEdge, street: Option<usize>, rng: &mut ChaCha8Rng) -> bool {
    let chance = AVENUE * if edge.arterial { AVENUE_ARTERIAL } else { 1.0 };
    match street {
        // Hashed rather than drawn: every segment of one street has to reach
        // the same answer, and they are visited in whatever order the chunks
        // arrive in. The draw is still taken, because the streams downstream
        // of it must not shift depending on which streets have names.
        Some(name) => {
            let _ = rng.random_range(0.0..1.0);
            let mut key = (name as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            key ^= key >> 29;
            (key % 1000) as f32 / 1000.0 < chance
        }
        None => rng.random_range(0.0..1.0) < chance,
    }
}

/// Metres between trees in a park, before jitter.
const PARK_SPACING: f32 = 11.0;
/// How far inside a park's edge planting starts.
const PARK_MARGIN: f32 = 4.5;
/// Chance a park grid cell has a tree in it.
const PARK_DENSITY: f32 = 0.55;

/// How far away foliage stops being drawn, before `lod_scale`.
///
/// Further than street furniture: a plane tree is six metres across and reads
/// as a shape on the street long after a bollard has stopped being a pixel. But
/// no longer five hundred metres, and the hundred that came off paid for the
/// trunk and the crown getting rounder.
///
/// Trees scatter over the ground, so how many are drawn goes as the square of
/// this: the ring between four hundred metres and five holds 36% of every tree
/// in view. Each of those is about twenty pixels tall on a 1080-line frame, and
/// each is drawn at full mesh resolution, because a tree has no second LOD —
/// there is one trunk mesh and one crown mesh per species and that is what gets
/// instanced at every distance. Losing the ring is 0.64 of the trees; the
/// rounder trunk and the finer crown are 1.62 of the triangles; 0.64 × 1.62 is
/// 1.04, so the whole change is very nearly triangle-neutral and every triangle
/// it moved went from something nobody can resolve to something the player
/// walks past at arm's length.
pub const RANGE: f32 = 400.0;

/// Wind speed, in metres per second, at which a tree leans as far as it is
/// going to. Above this it thrashes rather than leans, and thrashing is not
/// something a rigid rotation can portray honestly.
const GALE: f32 = 12.0;
/// How far the strongest wind lays the most flexible species over, in radians.
const LEAN: f32 = 0.085;
/// Rotation below which a tree counts as not having moved, in radians.
///
/// A thousandth of a degree. It exists to make still air free rather than to
/// quantise the movement.
const STILL: f32 = 3.0e-5;

/// What a tree is, which is mostly a statement about its silhouette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Species {
    /// Broad, high-crowned, and the standard street tree of half the cities in
    /// the world for the good reason that it tolerates being one.
    Plane,
    /// Rounded and domestic. The residential street's tree.
    Lime,
    /// Columnar. Narrow enough to line a road that has no room for a tree.
    Poplar,
    /// Small and ornamental, for a forecourt or a park path.
    Cherry,
}

impl Species {
    pub const ALL: [Species; 4] = [
        Species::Plane,
        Species::Lime,
        Species::Poplar,
        Species::Cherry,
    ];

    fn index(self) -> usize {
        match self {
            Species::Plane => 0,
            Species::Lime => 1,
            Species::Poplar => 2,
            Species::Cherry => 3,
        }
    }

    /// Trunk radius and clear height to the underside of the crown.
    fn trunk(self) -> (f32, f32) {
        match self {
            Species::Plane => (0.24, 3.6),
            Species::Lime => (0.19, 2.6),
            Species::Poplar => (0.17, 2.4),
            Species::Cherry => (0.13, 1.9),
        }
    }

    /// The blobs the crown is made of: centre, then radius, both relative to
    /// the top of the trunk.
    ///
    /// Three offset blobs for a plane tree, a close-packed spindle for a
    /// poplar. This is the entire difference between the species as far as
    /// anyone standing on the pavement can tell.
    fn crown(self) -> &'static [(Vec3, f32)] {
        // Six rather than three, and the three extra are all small and all at
        // the edge. A crown is read at fifty metres by its *outline*, and an
        // outline made of three big arcs is three big arcs however lumpy each
        // of them is — the noise in `ball` works on a scale of a metre and the
        // thing that was wrong was on a scale of five. What breaks a silhouette
        // is a limb sticking out where the next one does not.
        const PLANE: [(Vec3, f32); 6] = [
            (Vec3::new(0.0, 2.0, 0.0), 2.4),
            (Vec3::new(-1.7, 1.3, 0.6), 1.8),
            (Vec3::new(1.5, 1.5, -0.9), 1.9),
            (Vec3::new(0.4, 2.9, 1.3), 1.35),
            (Vec3::new(-1.1, 2.6, -1.4), 1.25),
            (Vec3::new(2.2, 0.7, 0.9), 1.10),
        ];
        const LIME: [(Vec3, f32); 5] = [
            (Vec3::new(0.0, 1.8, 0.0), 2.0),
            (Vec3::new(0.6, 0.7, 0.5), 1.5),
            (Vec3::new(-0.9, 1.2, -0.6), 1.4),
            (Vec3::new(0.2, 2.7, -0.5), 1.15),
            (Vec3::new(-1.4, 2.0, 0.9), 1.00),
        ];
        // The columnar one needs more blobs than the rest and closer
        // together, because a stack is only read as one crown while the necks
        // between its blobs stay wide. Widest a third of the way up, tapering
        // to a point: the shape of a Lombardy poplar, and nothing like the
        // three balls that stood here and read as a snowman.
        const POPLAR: [(Vec3, f32); 5] = [
            (Vec3::new(0.0, 0.75, 0.0), 1.05),
            (Vec3::new(0.0, 1.70, 0.0), 1.25),
            (Vec3::new(0.0, 2.65, 0.0), 1.15),
            (Vec3::new(0.0, 3.55, 0.0), 0.95),
            (Vec3::new(0.0, 4.40, 0.0), 0.72),
        ];
        const CHERRY: [(Vec3, f32); 4] = [
            (Vec3::new(0.0, 1.1, 0.0), 1.35),
            (Vec3::new(-0.7, 0.55, 0.35), 1.05),
            (Vec3::new(0.8, 0.8, -0.4), 0.95),
            (Vec3::new(0.1, 1.8, 0.3), 0.80),
        ];

        match self {
            Species::Plane => &PLANE,
            Species::Lime => &LIME,
            Species::Poplar => &POPLAR,
            Species::Cherry => &CHERRY,
        }
    }

    /// Whether this crown is a stack rather than a cluster.
    ///
    /// The one thing that decides how much waist a crown may have between its
    /// blobs — see `a_crown_is_one_shape_rather_than_a_stack_of_balls`.
    fn columnar(self) -> bool {
        matches!(self, Species::Poplar)
    }

    /// How far a full gale lays it over, as a fraction of [`LEAN`].
    ///
    /// A plane tree with a trunk a quarter of a metre thick barely moves; a
    /// young cherry moves a lot. Getting this the wrong way round is the sort
    /// of thing that reads as wrong without anybody being able to say why.
    fn give(self) -> f32 {
        match self {
            Species::Plane => 0.45,
            Species::Lime => 0.7,
            Species::Poplar => 1.0,
            Species::Cherry => 0.9,
        }
    }

    /// Overall height, ground to the top of the crown.
    fn height(self) -> f32 {
        let (_, clear) = self.trunk();
        clear
            + self
                .crown()
                .iter()
                .map(|(centre, radius)| centre.y + radius)
                .fold(0.0, f32::max)
    }

    /// How stiff it is, up to a constant nobody needs.
    ///
    /// A tree in wind is a cantilever with the load at the top, and a
    /// cantilever's tip deflection goes as its length cubed over the second
    /// moment of its section — which for a round trunk is the fourth power of
    /// its radius. So height matters more than thickness does, which is why a
    /// poplar bends further than a cherry despite the thicker trunk.
    fn stiffness(self) -> f32 {
        let (radius, _) = self.trunk();
        radius.powi(4) / self.height().powi(3)
    }

    fn foliage(self) -> Color {
        match self {
            Species::Plane => Color::srgb(0.24, 0.35, 0.16),
            Species::Lime => Color::srgb(0.29, 0.40, 0.17),
            Species::Poplar => Color::srgb(0.26, 0.37, 0.20),
            Species::Cherry => Color::srgb(0.34, 0.40, 0.22),
        }
    }

    /// Which species line a street, and how often. Poplars where a plane tree
    /// would not fit, and no cherries: they belong in gardens.
    fn on_streets() -> [(Species, u32); 3] {
        [
            (Species::Plane, 5),
            (Species::Lime, 4),
            (Species::Poplar, 2),
        ]
    }

    fn in_parks() -> [(Species, u32); 4] {
        [
            (Species::Plane, 4),
            (Species::Lime, 5),
            (Species::Cherry, 3),
            (Species::Poplar, 2),
        ]
    }

    fn pick(table: &[(Species, u32)], rng: &mut ChaCha8Rng) -> Species {
        let total: u32 = table.iter().map(|(_, weight)| weight).sum();
        let mut ticket = rng.random_range(0..total);
        for &(species, weight) in table {
            if ticket < weight {
                return species;
            }
            ticket -= weight;
        }
        table[0].0
    }
}

/// A tree that the wind can get at. Sits on the trunk, whose origin is its foot.
#[derive(Component)]
pub struct Sways {
    /// Which way it was planted, kept so the sway can be composed on top of it.
    yaw: f32,
    /// How far this individual leans in a full gale, in radians.
    give: f32,
    /// Where in its own cycle it is, so a row of trees does not move as one.
    phase: f32,
}

#[derive(Resource)]
pub struct FoliageKit {
    /// Trunk mesh and bark material, per species. The mesh's origin is its foot.
    trunk: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    /// The whole crown as one mesh, and how high above the foot it hangs.
    crown: Vec<(Handle<Mesh>, Handle<StandardMaterial>, f32)>,
    hedge: (Handle<Mesh>, Handle<StandardMaterial>),
    /// The leaf maps, kept so a second hedge can be cut at a different repeat
    /// — see [`FoliageKit::hedge_leaves`].
    leaf: (Handle<Image>, Handle<Image>),
}

/// What a clipped hedge is, in one colour. Shared by the park boundary and by
/// the walls of a real town's back yards, because a town does not grow two
/// kinds of privet.
const HEDGE_TINT: Color = Color::srgb(0.20, 0.31, 0.15);

impl FoliageKit {
    /// A hedge's leaves, cut at a given repeat.
    ///
    /// Public because a park is no longer the only thing in the city with a
    /// boundary: `world::streetside` plants one across every gap in a real
    /// town's frontage. It cannot use the park's own material, and the reason
    /// is worth writing down — the park hedge is one shared unit cube scaled
    /// by its transform, so its UVs are one repeat per face however long the
    /// run is, and the material multiplies that by nine. Scaled to eight
    /// metres by one, that is nine repeats across eight metres and nine across
    /// one, an aspect ratio of eight to one; through an alpha mask it comes out
    /// as a venetian blind. A boundary whose mesh carries its own size in its
    /// UVs wants the repeat at one instead.
    pub fn hedge_leaves(
        &self,
        tile: f32,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        materials.add(leaves(HEDGE_TINT, &self.leaf.0, &self.leaf.1, tile))
    }
}

/// Metres of canopy one repeat of the leaf texture covers.
///
/// Small. A crown's UVs come off the sphere it was built from, so this is a
/// count of repeats around a blob rather than a size in metres, and what it has
/// to land on is a clump about a metre across on a crown four or five metres
/// wide. Fewer and the holes are craters; more and the cut goes below the size a
/// pixel can resolve and the silhouette turns back into a fizzing grey ball.
const LEAF_TILE: f32 = 4.5;

/// How a crown is lit and cut out. Shared by every species; only the tint
/// differs, which is the whole reason the leaf texture is nearly white.
fn leaves(
    tint: Color,
    color: &Handle<Image>,
    normal: &Handle<Image>,
    tile: f32,
) -> StandardMaterial {
    StandardMaterial {
        base_color: tint,
        base_color_texture: Some(color.clone()),
        normal_map_texture: Some(normal.clone()),
        uv_transform: Affine2::from_scale(Vec2::splat(tile)),
        // The cut. Masked rather than blended, because blended foliage has to be
        // sorted against itself and a street of trees is the worst case there
        // is; and because a leaf's edge is genuinely a hard edge, not a fade.
        alpha_mode: AlphaMode::Mask(0.5),
        // With holes in it, a crown is seen from the inside as much as the
        // outside: through its own gaps, and from under it. Culling the back
        // faces would leave those views looking into an empty shell.
        double_sided: true,
        cull_mode: None,
        // A leaf is a tenth of a millimetre of green sandwiched between two
        // waxy surfaces, and most of what makes a tree read as alive is that
        // sunlight comes *through* it. Without this, the shaded half of a crown
        // is as black as the shaded half of a boulder, which is what every tree
        // in this city looked like.
        //
        // Note what this quietly does to the pipeline: transmission has nowhere
        // to live in the g-buffer, so Bevy reports any material asking for it as
        // forward-shaded whatever the default renderer method says. Every tree
        // in the city therefore leaves the deferred path. That is still the
        // right trade, but not for the reason first written here: a crown is
        // 1280 to 1920 triangles now, not "a few hundred", and forward shading
        // is paid per fragment and per light rather than per triangle. What
        // makes it affordable is that there are not many lights on a tree.
        diffuse_transmission: 0.80,
        thickness: 0.35,
        // Not matte. A leaf has a cuticle on it and a canopy in low sun has a
        // sheen across the top that is most of what says "waxy" rather than
        // "felt".
        perceptual_roughness: 0.72,
        ..default()
    }
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> FoliageKit {
    let bark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.38, 0.33),
        base_color_texture: Some(images.add(super::texture::bark())),
        normal_map_texture: Some(images.add(super::texture::bark_normal())),
        perceptual_roughness: 0.94,
        ..default()
    });
    let leaf_color = images.add(super::texture::foliage());
    let leaf_normal = images.add(super::texture::foliage_normal());

    let mut trunk = Vec::with_capacity(Species::ALL.len());
    let mut crown = Vec::with_capacity(Species::ALL.len());
    for species in Species::ALL {
        let (radius, clear) = species.trunk();
        // The trunk runs a little into the crown so there is no gap to see
        // daylight through, and is translated so the entity's origin is the
        // point it grows out of — which is what lets the wind rotate it.
        let height = clear + 0.8;
        trunk.push((
            meshes.add(super::buildings::with_tangents(timber(
                species, radius, height,
            ))),
            bark.clone(),
        ));

        crown.push((
            meshes.add(super::buildings::with_tangents(crown_mesh(species))),
            materials.add(leaves(
                species.foliage(),
                &leaf_color,
                &leaf_normal,
                LEAF_TILE,
            )),
            clear,
        ));
    }

    FoliageKit {
        trunk,
        crown,
        hedge: (
            meshes.add(super::buildings::with_tangents(
                Cuboid::new(1.0, 1.0, 1.0).mesh().build(),
            )),
            // A hedge is clipped, so its edge is the one bit of greenery in the
            // city that really is a straight line — but its face is still leaves
            // and still lets the low sun through. Tiled tighter than a crown,
            // because a box's UVs are one repeat per face however big it is and
            // a hedge is several metres long.
            materials.add(leaves(
                HEDGE_TINT,
                &leaf_color,
                &leaf_normal,
                LEAF_TILE * 2.0,
            )),
        ),
        leaf: (leaf_color, leaf_normal),
    }
}

/// How thick a bough is where it leaves the trunk, against the trunk's own
/// radius, and how far out along its blob it reaches before the leaves take
/// over.
const BOUGH_THICK: f32 = 0.40;
const BOUGH_REACH: f32 = 0.78;

/// Sides on the prism a trunk is drawn as.
///
/// Every limb on the tree used to be five, and the justification written here
/// was that a branch is three pixels wide at the distance anybody looks at it.
/// That is true of a branch and it is false of a trunk. A street tree stands
/// 1.15 m in from the kerb of a pavement the player walks down, and a 0.24 m
/// pentagon seen from two metres shows three flat facets and two hard vertical
/// creases running its whole height, seventy-two degrees apart, immediately
/// under a crown that is smooth-shaded. That contrast — a faceted post holding
/// up a round thing — is most of what "eckig" meant.
///
/// Ten puts the creases thirty-six degrees apart and the flat of a facet 12 mm
/// inside the circle it stands in for, under half a degree of silhouette wobble
/// at two metres, which is finer than the bark texture's own grain. Nine would
/// have done and ten is an even number of facets, so the trunk has a facet
/// facing the viewer rather than a crease whichever way it was planted. It
/// costs 20 triangles per tree over the pentagon, on the one part of a tree
/// that is ever looked at closely.
const TRUNK_SIDES: u32 = 10;
/// Sides on a bough.
///
/// A bough leaves the trunk at 0.40 of its radius — 96 mm on a plane tree — at
/// three and a half metres up, and runs into the leaves before it ends. Six is
/// enough that it is not a visible prism at head height, and there are up to
/// six boughs to one trunk, so this is the number that multiplies.
const BOUGH_SIDES: u32 = 6;
/// Sides on a sub-branch.
///
/// Still five, because the original argument was always about these: 53 mm
/// thick, thrown out sideways, and swallowed by the crown along most of their
/// length. There are as many of them as there are boughs.
const BRANCH_SIDES: u32 = 5;

/// The ordering is the point of having three numbers, so it is checked where a
/// tidy-up would meet it rather than in a test that has to be run. A trunk is
/// walked past at arm's length, a bough is at head height with leaves round it,
/// a sub-branch is inside them; collapse the three back to one and the
/// pentagonal trunk is back on the pavement. The even count is so that the
/// trunk turns a facet towards the viewer rather than a crease, whichever way
/// the tree happened to be planted.
const _: () = assert!(
    TRUNK_SIDES > BOUGH_SIDES && BOUGH_SIDES > BRANCH_SIDES && TRUNK_SIDES.is_multiple_of(2),
    "a trunk has to be rounder, and evenly so, than the branches it carries"
);

/// A tapered limb between two points: the one primitive a tree is made of.
///
/// Two stacked cylinders rather than one, because a branch that does not get
/// thinner is a pipe. Bevy has no frustum in its shape kit and one is not worth
/// hand-rolling when the taper can be had by stacking two prisms of the same
/// `sides`.
fn limb(from: Vec3, to: Vec3, radius: f32, sides: u32) -> Mesh {
    let axis = to - from;
    let length = axis.length().max(1e-3);
    let along = axis / length;
    let turn = Quat::from_rotation_arc(Vec3::Y, along);

    let piece = |at: f32, span: f32, radius: f32| {
        Cylinder::new(radius, length * span)
            .mesh()
            .resolution(sides)
            .build()
            .rotated_by(turn)
            .translated_by(from + axis * (at + span * 0.5))
    };

    let mut mesh = piece(0.0, 0.55, radius);
    if let Err(error) = mesh.merge(&piece(0.5, 0.5, radius * 0.6)) {
        warn!("a branch came out in one piece: {error}");
    }
    mesh
}

/// Trunk and boughs, as one mesh.
///
/// One mesh and therefore one draw, and — because the boughs are part of the
/// trunk rather than of the crown — they lean with the wind for free: the trunk
/// is the entity the sway rotates, and everything welded to it comes along.
///
/// Boughs are new, and they are new because the crown became see-through. While
/// a canopy was an opaque ball there was nothing to look into and a bare trunk
/// running up to it was fine; with the leaves cut away you can see through the
/// gaps, and what you saw through them was nothing at all. A tree read as a
/// lollipop from underneath, which is the angle a street tree is most often
/// seen from.
fn timber(species: Species, radius: f32, height: f32) -> Mesh {
    let mut mesh = limb(Vec3::ZERO, Vec3::Y * height, radius, TRUNK_SIDES);

    // Out of the top of the trunk, one to each blob of crown, stopping well
    // short of its middle so the leaves swallow the end.
    let fork = Vec3::Y * (height - 0.45);
    for (centre, blob) in species.crown() {
        let (_, clear) = species.trunk();
        let target = Vec3::new(centre.x, clear + centre.y, centre.z);
        let out = fork + (target - fork) * BOUGH_REACH;
        if let Err(error) = mesh.merge(&limb(fork, out, radius * BOUGH_THICK, BOUGH_SIDES)) {
            warn!("a {species:?} lost a bough: {error}");
        }
        // And one sub-branch off it, thrown to the side, which is what stops a
        // crown looking like an umbrella frame.
        let aside = out
            + (target - fork).normalize_or_zero() * (blob * 0.35)
            + Vec3::new(centre.z, blob * 0.30, -centre.x) * 0.28;
        if let Err(error) = mesh.merge(&limb(out, aside, radius * BOUGH_THICK * 0.55, BRANCH_SIDES))
        {
            warn!("a {species:?} lost a branch: {error}");
        }
    }
    mesh
}

/// How far a blob's surface is pushed in and out, as a fraction of its radius.
///
/// The leaf texture cuts holes in a crown and that fixed the *inside* of it; the
/// outline stayed a circular arc, because underneath the holes the geometry
/// really was a sphere. A tree is read at fifty metres almost entirely by its
/// outline, so the sphere has to stop being one — and a sixth of the radius,
/// pushed around by noise, is about the lumpiness of a real crown against the
/// sky. Much more and a plane tree turns into a cauliflower.
const CROWN_LUMP: f32 = 0.17;

/// The quickest ripple in a crown blob, in radians per unit of the unit sphere.
///
/// The six sine terms in [`ball`] run from 2.7 to 8.7. This is the fastest of
/// them, and therefore the one that decides how finely a blob has to be
/// subdivided before the displacement means anything at all — hoisted out of
/// `ball` so that [`CROWN_SUBDIVISIONS`] and the test that guards it can name
/// the number they are reasoning about instead of copying it.
const CROWN_FINEST: f32 = 8.7;

/// How finely a crown blob is subdivided.
///
/// This was two, on the grounds that a crown is read as a silhouette and as a
/// shadow, that smoothing it buys a rounder edge nobody looks at, and that a
/// deliberately lumpy silhouette does not want one anyway. The last two thirds
/// of that still stand. The mistake was treating the subdivision as a smoothness
/// dial when it is the *sampling rate of the lumps*: [`ball`] displaces
/// vertices, so a facet the noise never gets a vertex inside is a facet the
/// noise cannot bend.
///
/// Bevy's icosphere is `20 * (n + 1)^2` triangles, so `ico(2)` is 180 facets
/// over a whole sphere — about twenty-three degrees between neighbours — while
/// the quickest ripple in `ball` runs at [`CROWN_FINEST`], 8.7 radians per unit,
/// which turns over every twenty-one degrees. The mesh was sampling below
/// Nyquist: what came out was not a lumpy crown at `CROWN_LUMP` amplitude, it
/// was a sphere with a handful of vertices pulled off it at whatever phase they
/// happened to land on, which is why raising `CROWN_LUMP` had never made the
/// outline read as foliage. `ico(3)` is 320 facets, seventeen degrees apart,
/// which is two and a half samples per ripple and the least that makes the
/// shape the amplitude was tuned for.
///
/// 140 triangles a blob: 560 more on a cherry, 840 on a plane. Paid for by
/// `RANGE`.
const CROWN_SUBDIVISIONS: u32 = 3;

/// Merges a species' blobs into one mesh.
fn crown_mesh(species: Species) -> Mesh {
    let mut blobs = species.crown().iter().enumerate();
    let (_, (first, radius)) = blobs.next().expect("every species has a crown");
    let mut mesh = ball(*radius, 0).translated_by(*first);

    for (index, (centre, radius)) in blobs {
        if let Err(error) = mesh.merge(&ball(*radius, index as u32).translated_by(*centre)) {
            warn!("a {species:?} lost part of its crown: {error}");
        }
    }
    mesh
}

/// A crown blob: a sphere with the roundness knocked off it.
///
/// The lumps come from three sine waves crossed on the surface direction rather
/// than from an RNG. Two reasons, both about not having to think about it again:
/// it is the same tree every time the chunk respawns without joining the
/// determinism scheme, and it is continuous over the sphere, so the seam an
/// icosphere's UVs have does not become a seam in the shape as well.
fn ball(radius: f32, variant: u32) -> Mesh {
    // The fallback is matched to the icosphere rather than left at the coarse
    // `uv(11, 8)` it was: a UV sphere is `2 * longitudes * (latitudes - 1)`
    // triangles, so 14 by 12 is 308 against `ico(3)`'s 320. It has never fired —
    // Bevy only refuses an icosphere above 80 subdivisions — but a fallback that
    // silently halves the sampling rate is not a fallback, it is a bug waiting
    // for the day the limit moves.
    let mut mesh = Sphere::new(radius)
        .mesh()
        .ico(CROWN_SUBDIVISIONS)
        .unwrap_or_else(|_| Sphere::new(radius).mesh().uv(14, 12));

    let turn = variant as f32 * 1.7;
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for position in positions.iter_mut() {
            let unit = Vec3::from(*position) / radius.max(1e-4);
            let broad = (unit.x * 3.1 + turn).sin()
                * (unit.y * 2.7 - turn).sin()
                * (unit.z * 3.5 + turn).sin();
            let fine = (unit.x * 7.3 - turn).sin()
                * (unit.y * 6.1 + turn).sin()
                * (unit.z * CROWN_FINEST - turn).sin();
            let lump = 1.0 + (broad * 0.72 + fine * 0.28) * CROWN_LUMP;
            *position = (unit * radius * lump).to_array();
        }
    }
    // The normals came off the sphere and the surface is no longer one, so
    // without this the lumps are a silhouette and nothing else — lit exactly as
    // flat as the ball they were carved out of.
    mesh.compute_smooth_normals();
    mesh
}

/// Plants one tree, and hands back the trunk so a caller can add to it.
fn plant(
    commands: &mut Commands,
    kit: &FoliageKit,
    chunk: IVec2,
    at: Vec2,
    ground: f32,
    species: Species,
    rng: &mut ChaCha8Rng,
    range: f32,
) {
    let (trunk, bark) = &kit.trunk[species.index()];
    let (crown, leaves, clear) = &kit.crown[species.index()];

    // One scale for the whole tree, so proportions stay the species' own.
    let size = rng.random_range(0.82..1.24);
    let yaw = rng.random_range(0.0..std::f32::consts::TAU);
    // A visibility range is *not* inherited: a crown without one of its own
    // would go on hanging in the air after its trunk stopped being drawn.
    let draw = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: (range.max(1.0) * 0.9)..range.max(1.0),
        use_aabb: false,
    };

    commands.spawn((
        ChunkOf(chunk),
        Sways {
            yaw,
            give: species.give(),
            phase: rng.random_range(0.0..std::f32::consts::TAU),
        },
        Mesh3d(trunk.clone()),
        MeshMaterial3d(bark.clone()),
        Transform::from_xyz(at.x, ground, at.y)
            .with_rotation(Quat::from_rotation_y(yaw))
            .with_scale(Vec3::splat(size)),
        draw.clone(),
        children![(
            Mesh3d(crown.clone()),
            MeshMaterial3d(leaves.clone()),
            Transform::from_xyz(0.0, *clear, 0.0),
            draw,
        )],
    ));
}

/// Plants both kerbs of one street, if this street is an avenue at all.
#[allow(clippy::too_many_arguments)]
pub fn spawn_edge(
    commands: &mut Commands,
    kit: &FoliageKit,
    corridors: &super::streetside::Corridors,
    rng: &mut ChaCha8Rng,
    edge: &RoadEdge,
    // Which named street this segment belongs to, if the extract said. A whole
    // street is an avenue or it is not — see [`avenue`].
    street: Option<usize>,
    from: Vec2,
    to: Vec2,
    chunk: IVec2,
    range: f32,
) {
    if !avenue(edge, street, rng) {
        return;
    }

    let Ok(direction) = Dir2::new(to - from) else {
        return;
    };
    let normal = Vec2::new(-direction.y, direction.x);
    let offset = edge.width * 0.5 + SET_BACK;

    // One species for the street. A row of trees is planted at once, by one
    // council, from one nursery; mixing them per tree is the tell.
    let species = Species::pick(&Species::on_streets(), rng);

    // Walked along the segment rather than counted in whole slots of it.
    //
    // `length / SPACING` floored, iterated from one, is what this was, and on a
    // grid — sixty to a hundred metres between junctions — it plants three or
    // four trees. A town read off a map is a *polyline*: Landshut's median
    // segment is thirteen and a half metres against a seventeen-metre spacing,
    // so the count came out at zero, the loop `1..0` ran not at all, and four
    // streets in ten were avenues with no trees on them. The whole Altstadt was
    // bare and nothing said so.
    let mut along = (edge.length.min(SPACING) * 0.5).max(2.0);
    while along < edge.length - 1.5 {
        for side in [-1.0f32, 1.0] {
            // A gap where a crossing or a driveway would be.
            if rng.random_range(0.0..1.0) > 0.86 {
                continue;
            }
            let jitter = rng.random_range(-1.1..1.1);
            let at = from + *direction * (along + jitter) + normal * offset * side;
            // The density was fixed here and the ground was not: simulated over
            // the committed extract, 536 of 7350 trunks stood in another
            // street's carriageway. A town read off a map has streets behind
            // streets at every angle, and a metre of jitter is enough to walk
            // a plane tree into one.
            if corridors.in_the_road(at, TRUNK_ROOM) {
                continue;
            }
            plant(
                commands,
                kit,
                chunk,
                at,
                SIDEWALK_HEIGHT,
                species,
                rng,
                range,
            );
        }
        along += SPACING;
    }
}

/// Plants a park, which is the only block in the city that is not paved.
///
/// A park was a green rectangle. What makes it a park rather than a lawn is
/// that it has things standing in it at different distances, so that walking
/// through it changes what you can see — hence a scattered canopy rather than
/// an avenue, and a hedge along the edge to give it a boundary.
pub fn spawn_park(
    commands: &mut Commands,
    kit: &FoliageKit,
    rng: &mut ChaCha8Rng,
    block: &Block,
    chunk: IVec2,
    range: f32,
) {
    if block.district != District::Park {
        return;
    }
    let planted = block.area.inset(PARK_MARGIN);
    if !planted.is_valid() {
        return;
    }

    let size = planted.size();
    let (across, down) = (
        (size.x / PARK_SPACING).floor() as i32,
        (size.y / PARK_SPACING).floor() as i32,
    );
    // A grid with jitter inside each cell, for the same reason the roof kit
    // uses one: it cannot put two trees in the same place however the dice
    // fall, and it finishes in a fixed number of steps.
    for row in 0..down {
        for column in 0..across {
            if rng.random_range(0.0..1.0) > PARK_DENSITY {
                continue;
            }
            let cell = Vec2::new(
                planted.min.x + (column as f32 + 0.5) * PARK_SPACING,
                planted.min.y + (row as f32 + 0.5) * PARK_SPACING,
            );
            let jitter = Vec2::new(
                rng.random_range(-PARK_SPACING * 0.35..PARK_SPACING * 0.35),
                rng.random_range(-PARK_SPACING * 0.35..PARK_SPACING * 0.35),
            );
            let species = Species::pick(&Species::in_parks(), rng);
            plant(
                commands,
                kit,
                chunk,
                cell + jitter,
                SIDEWALK_HEIGHT,
                species,
                rng,
                range,
            );
        }
    }

    hedge(commands, kit, block, chunk, range);
}

/// A clipped hedge along each side of a park, in segments with gaps for the
/// ways in.
fn hedge(commands: &mut Commands, kit: &FoliageKit, block: &Block, chunk: IVec2, range: f32) {
    const HEIGHT: f32 = 0.95;
    const DEPTH: f32 = 0.7;
    /// How far in from the kerb the hedge is planted.
    const INSET: f32 = 1.6;
    /// The width of the way in at the middle of each side.
    const GATE: f32 = 6.0;

    let (mesh, leaves) = &kit.hedge;
    let area = block.area.inset(INSET);
    if !area.is_valid() {
        return;
    }
    let size = area.size();
    let centre = area.center();
    let draw = range.max(1.0);

    // Four sides, each in two runs with a gap between them.
    for (along_x, sign) in [(true, -1.0f32), (true, 1.0), (false, -1.0), (false, 1.0)] {
        let span = if along_x { size.x } else { size.y };
        let run = (span - GATE) * 0.5;
        if run <= 0.5 {
            continue;
        }
        for end in [-1.0f32, 1.0] {
            let offset = end * (GATE * 0.5 + run * 0.5);
            let (at, scale) = if along_x {
                (
                    Vec2::new(centre.x + offset, centre.y + sign * size.y * 0.5),
                    Vec3::new(run, HEIGHT, DEPTH),
                )
            } else {
                (
                    Vec2::new(centre.x + sign * size.x * 0.5, centre.y + offset),
                    Vec3::new(DEPTH, HEIGHT, run),
                )
            };
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(leaves.clone()),
                Transform::from_xyz(at.x, SIDEWALK_HEIGHT + HEIGHT * 0.5, at.y).with_scale(scale),
                VisibilityRange {
                    start_margin: 0.0..0.0,
                    end_margin: (draw * 0.9)..draw,
                    use_aabb: false,
                },
                // Knee-high and up against a kerb. Its own shadow is a line on
                // the ground that the ambient occlusion already draws.
                NotShadowCaster,
            ));
        }
    }
}

// ------------------------------------------------------------------ wind ----

/// The axis a tree turns about to lean downwind.
///
/// Rotating by a small angle about this axis moves the crown along the wind:
/// the tilt direction of `Y` under `axis` is `axis × Y`, which for an axis in
/// the ground plane comes out perpendicular to the axis — so the axis itself
/// has to be perpendicular to the wind, not along it. Getting this a quarter
/// turn out is invisible on a still day and unmissable in a gale.
pub fn lean_axis(wind: Vec2) -> Option<Vec3> {
    Dir2::new(wind)
        .ok()
        .map(|wind| Vec3::new(wind.y, 0.0, -wind.x))
}

/// How far a tree of unit give leans, in radians, at this wind speed.
pub fn lean(speed: f32) -> f32 {
    LEAN * (speed / GALE).clamp(0.0, 1.0)
}

/// Leans everything the camera can see, and nothing it cannot.
///
/// The visibility check is not an optimisation of the maths — the maths is four
/// sines. It is an optimisation of the *write*: touching `Transform` queues an
/// entity for transform propagation and for a fresh upload of its instance
/// data, and there are thousands of these standing in chunks nobody is looking
/// at.
fn sway(
    time: Res<Time>,
    weather: Res<Weather>,
    mut trees: Query<(&mut Transform, &Sways, &ViewVisibility)>,
) {
    let Some(axis) = lean_axis(weather.wind) else {
        return;
    };
    let reach = lean(weather.wind_speed());
    let now = time.elapsed_secs();

    for (mut transform, tree, visible) in &mut trees {
        if !visible.get() {
            continue;
        }
        // Two frequencies rather than one: a single sine is a metronome, and a
        // street of metronomes in step is worse than no movement at all.
        let gust =
            (now * 0.9 + tree.phase).sin() * 0.6 + (now * 2.3 + tree.phase * 1.7).sin() * 0.4;
        // Always downwind, never upwind — a gust adds to a lean, it does not
        // reverse it — so the cycle runs from a third of the lean to all of it.
        let angle = reach * tree.give * (0.66 + 0.34 * gust);
        let wanted = Quat::from_axis_angle(axis, angle) * Quat::from_rotation_y(tree.yaw);

        // `Mut` marks a component changed the instant it is dereferenced
        // mutably, and a changed `Transform` is a propagation and an instance
        // upload whether or not the value differs. So the current rotation is
        // read past the bypass and written only when the tree has actually
        // moved — which on a still day is never.
        if transform
            .bypass_change_detection()
            .rotation
            .angle_between(wanted)
            > STILL
        {
            transform.rotation = wanted;
        }
    }
}

pub struct VegetationPlugin;

impl Plugin for VegetationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sway);
    }
}

// -------------------------------------------------------- the commons ----

/// How far apart the planting grid is over a mapped piece of open ground.
const COMMONS_SPACING: f32 = 11.0;
/// And how far in from its edge nothing is planted, so a wood does not spill
/// over the path round it.
const COMMONS_MARGIN: f32 = 3.0;

/// Plants a piece of ground the map says is open.
///
/// This is what fills what was, until the extract grew, simply bare: eighty-five
/// parks, woods, pitches, allotments and a cemetery, each a real polygon rather
/// than a rectangle. Clipped to the chunk here rather than at load, because a
/// park is a ring and half a ring is not a smaller park.
pub fn spawn_ground(
    commands: &mut Commands,
    kit: &FoliageKit,
    rng: &mut ChaCha8Rng,
    ground: &super::citygen::OpenGround,
    chunk: IVec2,
    range: f32,
) {
    use super::atlas::GroundKind;

    // How thickly, and out of which species. A pitch and a car park are marked
    // out here so that nothing is *built* on them — see `streetside::lots` —
    // and get no trees at all; a lawn gets the odd specimen; a wood is a wood.
    let (density, palette) = match ground.kind {
        GroundKind::Trees => (0.85, Species::in_parks()),
        GroundKind::Park => (0.34, Species::in_parks()),
        GroundKind::Cemetery => (0.30, Species::in_parks()),
        GroundKind::Allotments => (0.22, Species::in_parks()),
        GroundKind::Grass => (0.07, Species::in_parks()),
        GroundKind::Field | GroundKind::Pitch | GroundKind::Playground | GroundKind::Parking => {
            return;
        }
    };

    let cell = super::streaming::chunk_center(chunk);
    let half = super::streaming::CHUNK_SIZE * 0.5;
    // The part of this polygon that is this chunk's business.
    let low = ground.bounds.min.max(cell - Vec2::splat(half));
    let high = ground.bounds.max.min(cell + Vec2::splat(half));
    if low.x >= high.x || low.y >= high.y {
        return;
    }

    // A jittered grid, like `spawn_park`: it cannot put two trees in the same
    // place however the dice fall, and it finishes in a fixed number of steps.
    // Anchored to the world rather than to the chunk, so a tree does not move
    // when the chunk it happens to be in changes.
    let first = (low / COMMONS_SPACING).ceil() * COMMONS_SPACING;
    let mut at = first;
    while at.y < high.y {
        at.x = first.x;
        while at.x < high.x {
            let here = at;
            at.x += COMMONS_SPACING;
            if rng.random_range(0.0..1.0) > density {
                continue;
            }
            let jitter = Vec2::new(
                rng.random_range(-COMMONS_SPACING * 0.4..COMMONS_SPACING * 0.4),
                rng.random_range(-COMMONS_SPACING * 0.4..COMMONS_SPACING * 0.4),
            );
            let point = here + jitter;
            if !inside(&ground.points, point, COMMONS_MARGIN) {
                continue;
            }
            let species = Species::pick(&palette, rng);
            plant(
                commands,
                kit,
                chunk,
                point,
                SIDEWALK_HEIGHT,
                species,
                rng,
                range,
            );
        }
        at.y += COMMONS_SPACING;
    }
}

/// Is this point inside the ring, and at least `margin` in from its edge?
///
/// Ray casting for the inside test and a distance-to-segment sweep for the
/// margin. Both are O(edges) and a ring here is a few dozen points, run a few
/// hundred times per chunk — which is nothing next to spawning one tree.
fn inside(ring: &[Vec2], at: Vec2, margin: f32) -> bool {
    let mut within = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        // A horizontal ray to +X. The half-open rule on y is what stops a
        // vertex exactly on the ray being counted twice.
        if (a.y > at.y) != (b.y > at.y) {
            let cut = a.x + (at.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if cut > at.x {
                within = !within;
            }
        }
        if margin > 0.0 {
            let span = b - a;
            let length = span.length_squared();
            let t = if length < 1e-6 {
                0.0
            } else {
                ((at - a).dot(span) / length).clamp(0.0, 1.0)
            };
            if (a + span * t).distance(at) < margin {
                return false;
            }
        }
    }
    within
}


#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn every_species_has_a_trunk_to_hang_its_crown_on() {
        for species in Species::ALL {
            let (radius, clear) = species.trunk();
            assert!(radius > 0.0 && clear > 1.5, "{species:?} is not a tree");
            assert!(
                !species.crown().is_empty(),
                "{species:?} has nothing on top of it"
            );
            // The lowest blob has to reach down to the top of the trunk, or
            // there is a gap you can see the sky through.
            let lowest = species
                .crown()
                .iter()
                .map(|(centre, radius)| centre.y - radius)
                .fold(f32::MAX, f32::min);
            assert!(
                lowest < 0.8,
                "{species:?}'s crown floats {lowest}m above its trunk"
            );
        }
    }

    /// A stiffer tree moves less. The only property of the wind model anybody
    /// would notice being wrong — and the reason it is checked against the
    /// cantilever stiffness rather than against trunk thickness is that a
    /// poplar has a thicker trunk than a cherry and still bends further,
    /// because it is twice the height.
    /// How wide the union of two blobs is where they meet, as a fraction of
    /// the smaller one. One means no waist at all — the smaller blob's centre
    /// lies inside the larger, so it reads as a bulge. Zero means they do not
    /// touch and are two separate balls.
    ///
    /// This is the measurement the eye actually makes. Overlap alone does not
    /// answer it: two blobs can overlap by a third of their radii and still
    /// join through a neck half their width, which is exactly what a snowman
    /// is.
    fn waist(a: (Vec3, f32), b: (Vec3, f32)) -> f32 {
        let ((centre, radius), (other, other_radius)) = (a, b);
        let gap = centre.distance(other);
        if gap >= radius + other_radius {
            return 0.0;
        }
        if gap + radius.min(other_radius) <= radius.max(other_radius) {
            return 1.0;
        }
        // Distance from `centre` to the plane the two spheres cross in, and
        // from that the radius of the circle they cross in.
        let along = (gap * gap + radius * radius - other_radius * other_radius) / (2.0 * gap);
        let neck = (radius * radius - along * along).max(0.0).sqrt();
        neck / radius.min(other_radius)
    }

    /// A crown is one merged mesh, and two blobs joined by a thin neck are two
    /// blobs however much they overlap.
    ///
    /// How thin a neck may be is a question about the *shape of the species*,
    /// which is why the threshold is one. A columnar tree's crown is a stack,
    /// and every waist in a stack is a waist you can see straight through — get
    /// it wrong and the tree reads as a snowman, which is what the poplar did.
    /// A broadleaf's crown is a cluster of limbs and is *supposed* to be lobed:
    /// the outline is what a tree is read by at fifty metres, and an outline
    /// made of three big arcs is three big arcs however lumpy each of them is.
    /// A limb that sticks out far enough to break the arc necessarily narrows
    /// where it leaves the crown.
    #[test]
    fn a_crown_is_one_shape_rather_than_a_stack_of_balls() {
        for species in Species::ALL {
            let blobs = species.crown();
            // A stack has to hold together; a cluster is allowed its lobes.
            let floor = if species.columnar() { 0.9 } else { 0.6 };
            for (i, &blob) in blobs.iter().enumerate().skip(1) {
                // Every blob has to join at least one of the ones before it
                // through a neck that is not a stalk.
                let widest = blobs[..i]
                    .iter()
                    .map(|&other| waist(blob, other))
                    .fold(0.0, f32::max);
                assert!(
                    widest > floor,
                    "{species:?} blob {i} at {} joins the rest of its crown \
                     through a neck {:.0}% of its width",
                    blob.0,
                    widest * 100.0
                );
            }
        }
    }

    /// The ordering of the three side counts is a `const` assertion above;
    /// what needs a test is that ten sides is actually enough for the trunk
    /// radius every species is given, because those are four separate numbers
    /// and a new species would come with a fifth.
    #[test]
    fn a_trunk_is_round_enough_to_stand_next_to() {
        for species in Species::ALL {
            let (radius, _) = species.trunk();
            // Sagitta: how far the flat of one facet falls inside the circle it
            // stands in for, which is the silhouette error at the edge of the
            // trunk. Fifteen millimetres on a trunk seen from two metres is
            // under half a degree, finer than the grain of the bark texture
            // drawn over it.
            let flat = radius * (1.0 - (std::f32::consts::PI / TRUNK_SIDES as f32).cos());
            assert!(
                flat < 0.015,
                "{species:?}'s trunk is {:.0} mm off round at every facet",
                flat * 1000.0
            );
        }
    }

    /// The lumps in a crown are displaced *vertices*, so the subdivision is the
    /// rate at which the noise gets sampled and not a smoothness dial. Below
    /// two samples to a ripple the mesh cannot make the shape at all, whatever
    /// `CROWN_LUMP` is set to — which is the state `ico(2)` was in.
    #[test]
    fn a_crown_is_subdivided_finely_enough_to_show_its_lumps() {
        // Bevy's icosphere is `20 * (n + 1)^2` triangles. Spread over the 4π
        // steradians of a sphere and taken as equilateral, that gives the angle
        // between neighbouring vertices.
        let facets = 20.0 * (CROWN_SUBDIVISIONS as f32 + 1.0).powi(2);
        let solid = 4.0 * std::f32::consts::PI / facets;
        let spacing = (4.0 * solid / 3.0f32.sqrt()).sqrt();
        // Crest to trough of the quickest ripple, in the same radians.
        let feature = std::f32::consts::PI / CROWN_FINEST;
        assert!(
            spacing < feature,
            "a crown samples every {:.0}° and its lumps turn over every {:.0}°",
            spacing.to_degrees(),
            feature.to_degrees()
        );
    }

    #[test]
    fn the_stiffer_tree_is_the_one_that_gives_less() {
        let mut by_stiffness = Species::ALL;
        by_stiffness.sort_by(|a, b| b.stiffness().total_cmp(&a.stiffness()));
        for pair in by_stiffness.windows(2) {
            assert!(
                pair[0].give() <= pair[1].give(),
                "{:?} is stiffer than {:?} but gives more ({} against {})",
                pair[0],
                pair[1],
                pair[0].give(),
                pair[1].give()
            );
        }
        // And the ordering has to be a real one rather than four equal values.
        assert!(
            by_stiffness[0].give() < by_stiffness[Species::ALL.len() - 1].give(),
            "every species gives the same amount"
        );
    }

    /// The lean has to go *with* the wind. A quarter turn out is a rotation
    /// that still looks like movement, which is why it needs a test and not an
    /// eye.
    #[test]
    fn trees_lean_downwind() {
        for wind in [
            Vec2::new(6.0, 0.0),
            Vec2::new(0.0, -4.0),
            Vec2::new(-3.0, 3.0),
        ] {
            let axis = lean_axis(wind).expect("a wind with a direction");
            let leaned = Quat::from_axis_angle(axis, lean(wind.length())) * Vec3::Y;
            let drift = Vec2::new(leaned.x, leaned.z);
            assert!(drift.length() > 1e-4, "no lean at all in a {wind} wind");
            assert!(
                drift.normalize().dot(wind.normalize()) > 0.999,
                "a {wind} wind leant the tree towards {drift}"
            );
        }
        assert!(
            lean_axis(Vec2::ZERO).is_none(),
            "still air has no direction"
        );
    }

    #[test]
    fn the_lean_saturates_rather_than_running_away() {
        assert_eq!(lean(0.0), 0.0);
        assert!(lean(GALE * 0.5) < lean(GALE));
        assert_eq!(lean(GALE), lean(GALE * 10.0));
        assert!(lean(GALE) <= LEAN);
    }

    #[test]
    fn street_planting_avoids_the_ornamental_and_park_planting_does_not() {
        let streets: Vec<_> = Species::on_streets().iter().map(|(s, _)| *s).collect();
        assert!(!streets.contains(&Species::Cherry));
        let parks: Vec<_> = Species::in_parks().iter().map(|(s, _)| *s).collect();
        for species in Species::ALL {
            assert!(parks.contains(&species), "{species:?} grows nowhere");
        }
    }

    #[test]
    fn the_weighted_pick_can_reach_every_entry() {
        let mut rng = ChaCha8Rng::seed_from_u64(9);
        let table = Species::in_parks();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..3000 {
            seen.insert(Species::pick(&table, &mut rng));
        }
        assert_eq!(seen.len(), table.len());
    }

    /// Two chunks must not plant the same trees, and one chunk must plant the
    /// same trees every time it is walked back into.
    #[test]
    fn planting_is_per_chunk_and_repeatable() {
        let draw = |chunk: (i32, i32)| {
            let mut rng = crate::core::rng::stream_for_chunk(
                0xBEEF,
                crate::core::rng::stream::VEGETATION,
                chunk,
            );
            (0..8)
                .map(|_| rng.random_range(0.0..1.0f32))
                .collect::<Vec<_>>()
        };
        assert_eq!(draw((3, -4)), draw((3, -4)));
        assert_ne!(draw((3, -4)), draw((-4, 3)));
        assert_ne!(draw((0, 0)), draw((0, 1)));
    }
}
