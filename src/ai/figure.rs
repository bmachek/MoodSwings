//! The shape of a person, and the way one walks.
//!
//! A pedestrian was a capsule. A capsule reads as a person at fifty metres and
//! as a bollard at five, and the difference matters because the whole point of
//! pedestrians is that the player gets close to them — witnesses, victims, the
//! crowd that scatters when a car mounts the kerb.
//!
//! So the figure is assembled from parts hung off the same entity the capsule
//! collider is still on. Nothing here is physical: the collider is unchanged,
//! and the limbs are decoration that follows it. That keeps every raycast, every
//! line-of-sight check and every piece of pursuit logic exactly as it was.
//!
//! Limbs pivot at their joint rather than their centre, which is why each one
//! is an entity at the shoulder or hip with the mesh hung *below* it. Rotating a
//! centred capsule swings it about its middle, and a leg that does that is not
//! walking, it is being stirred.
//!
//! Every part also carries its [`Rest`] pose, which is what makes the whole
//! figure squash and stretch as one. The alternative — scaling the body entity
//! — is not available: Avian scales a collider by its transform, so a figure
//! flattening at the bottom of a hop would flatten its own collider with it and
//! sink through the pavement.

use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use crate::bounce::controller::Bouncer;

/// Metres walked per full stride cycle — one step of each foot.
const STRIDE: f32 = 1.45;
/// How far a leg swings at a walk, in radians.
const SWING: f32 = 0.62;
/// Arms swing less than legs, and opposite them.
const ARM_SWING: f32 = 0.44;

/// Where a limb hangs from.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Limb {
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

impl Limb {
    /// Legs lead, arms follow half a cycle behind, and the two sides oppose.
    fn phase_offset(self) -> f32 {
        use std::f32::consts::PI;
        match self {
            Limb::LeftLeg | Limb::RightArm => 0.0,
            Limb::RightLeg | Limb::LeftArm => PI,
        }
    }

    fn amplitude(self) -> f32 {
        match self {
            Limb::LeftLeg | Limb::RightLeg => SWING,
            Limb::LeftArm | Limb::RightArm => ARM_SWING,
        }
    }
}

/// The head. Marked because it is the one part of a figure that anything else
/// wants to find: it is where the face goes.
#[derive(Component)]
pub struct Head;

/// Skin, of which a flummi has exactly two patches: its hands.
///
/// Marked for the same reason the head is. A flummi's complexion is its mood —
/// it goes red with the face — and skin-toned hands under an emoji head read as
/// a bug rather than as a person.
#[derive(Component)]
pub struct Bare;

/// How a part of a figure sits when the body is at its natural height.
///
/// Held per part rather than looked up from the part's kind, so that posing a
/// figure is one query over its children instead of a match with an arm for
/// every piece of anatomy.
///
/// A scale as well as a position, because one part is not a unit shape: the
/// hair is a flattened sphere, and the squash multiplies this rather than
/// replacing it. Without that the cap is round again at the bottom of every
/// hop, which is a head growing a bubble sixty times a second.
#[derive(Component, Clone, Copy)]
pub struct Rest {
    pub at: Vec3,
    pub scale: Vec3,
}

impl Rest {
    pub fn at(at: Vec3) -> Self {
        Self {
            at,
            scale: Vec3::ONE,
        }
    }

    pub fn posed(at: Vec3, scale: Vec3) -> Self {
        Self { at, scale }
    }
}

/// How tall this figure stands, relative to the adult every part was
/// proportioned for.
///
/// Read by [`animate`], which multiplies it into the squash pose — so the
/// scale reaches every `Rest`-carrying part, and the limbs' children inherit
/// it through their parents' transforms. The body entity is never scaled
/// (Avian would shrink the collider with it); whoever spawns a small figure
/// scales the capsule and the stand height by the same number instead.
#[derive(Component, Clone, Copy)]
pub struct Stature(pub f32);

/// A pose that overrides the walk swing while it is worn.
///
/// The walk cycle owns the limbs by default; a figure doing something *with*
/// its arms — grabbing a collar, being shaken by one — wears a posture over
/// it, and [`animate`] hands the limbs to the posture instead for as long as
/// the component is on the body. Time-parameterised rather than distance-
/// parameterised, because everything a posture animates happens standing
/// still, where a distance-paced clock has stopped.
#[derive(Component, Clone, Copy, Debug)]
pub enum Posture {
    /// Both arms out in front at collar height, feet planted: the grabber.
    Grabbing,
    /// Arms windmilling, legs pedalling air: the one being shaken.
    Flailing,
}

impl Posture {
    pub fn limb_angle(self, limb: Limb, seconds: f32) -> f32 {
        // Positive rotation about X tips a hanging limb towards -Z, which is
        // the way a figure faces — so positive is "forward" throughout.
        match (self, limb) {
            // Locked straight out: the grip is the whole statement.
            (Posture::Grabbing, Limb::LeftArm | Limb::RightArm) => 1.35,
            // Braced: one foot ahead of the other, holding the ground.
            (Posture::Grabbing, Limb::LeftLeg) => 0.28,
            (Posture::Grabbing, Limb::RightLeg) => -0.28,
            // Windmilling, the two arms out of phase so it reads as panic
            // rather than as a callisthenics routine.
            (Posture::Flailing, Limb::LeftArm) => 1.0 + (seconds * 13.0).sin() * 0.75,
            (Posture::Flailing, Limb::RightArm) => {
                1.0 + (seconds * 13.0 + std::f32::consts::PI).sin() * 0.75
            }
            (Posture::Flailing, Limb::LeftLeg) => (seconds * 13.0).sin() * 0.35,
            (Posture::Flailing, Limb::RightLeg) => {
                (seconds * 13.0 + std::f32::consts::PI).sin() * 0.35
            }
        }
    }
}

/// Something worth looking at, and how long it stays worth looking at.
///
/// A head used to be welded to a chest. `animate` wrote a rotation only for
/// the children carrying a [`Limb`], so a citizen could not look at the person
/// they were talking to, at the busker they had stopped for, at the accident
/// they were rubbernecking — the rubbernecking turned the *whole body*, which
/// is what somebody does when they are about to walk over, not when they have
/// glanced round — or at the player standing in front of them.
///
/// This is the cheapest intelligence in the game. A figure that turns its head
/// towards what it is dealing with reads as having noticed it; four sine-driven
/// limbs and a fixed stare read as a machine walking a route, which is exactly
/// what the crowd was.
///
/// `until` is on the game clock, so an interest expires on its own and the head
/// falls back to looking where the body is going.
#[derive(Component, Clone, Copy, Debug)]
pub struct Attention {
    pub at: Vec3,
    pub until: f32,
}

impl Attention {
    /// Look at a point for `seconds`, from now.
    pub fn to(at: Vec3, now: f32, seconds: f32) -> Self {
        Self {
            at,
            until: now + seconds,
        }
    }
}

/// How far a neck turns, and how fast.
///
/// Seventy degrees of yaw is about what a person manages without moving their
/// shoulders; past it they turn their body, which several systems here already
/// do for their own reasons. The pitch is smaller because a head that tips
/// right back to look at a first-floor window reads as a faint rather than as
/// curiosity. The slew is what makes it a glance instead of a snap.
const NECK_YAW: f32 = 1.22;
const NECK_PITCH: f32 = 0.44;
const NECK_SLEW: f32 = 7.0;

/// A figure that is sitting down, and therefore has no stride.
///
/// Deliberately not a [`Posture`]: a posture is something a figure is doing
/// for a moment and is taken off again when it stops, and `mood::scuffle`
/// and `mood::grudge` both do exactly that. Sitting is not a moment, it is
/// who this citizen is — so it lives underneath, and a posture still wins
/// for as long as one is on the body. A wheelchair user being shaken by the
/// collar flails; when they are let go they are sitting down again.
#[derive(Component)]
pub struct Seated;

/// Where a seated figure holds a limb.
///
/// The legs are swung forward off the hip, since there is no knee to bend;
/// the arms hang, which puts the hands on the wheel rims because that is
/// where `ARM_LENGTH` below `SHOULDER` happens to land.
pub fn seated_angle(limb: Limb) -> f32 {
    match limb {
        Limb::LeftLeg | Limb::RightLeg => body::SEATED_LEG,
        Limb::LeftArm | Limb::RightArm => 0.0,
    }
}

/// Where a figure stops being worth its trimmings, in metres.
///
/// A citizen is nineteen meshes: torso, head, hair, four limb joints with a
/// limb and a hand or a shoe hanging off each, and whatever the archetype is
/// carrying. At a hundred and sixty of them that is three thousand entities,
/// and most of them are somewhere down the street where a hand is under two
/// pixels across.
///
/// So the trimmings carry a range and the silhouette does not — the same trade
/// `vehicle::spawn` makes with a car's fittings, and for the same reason: what
/// survives to any distance is the shape, and the shape is all that is left of
/// a person at forty metres anyway. Hands, shoes, hair and the archetype's
/// props go; the torso, the head and the four limbs stay, because a figure
/// without them is not a figure.
const TRIMMINGS: f32 = 42.0;

/// The range every trimming carries.
///
/// A tenth of the distance is the crossfade, which at this range is a couple
/// of frames of dither on something a few pixels wide.
fn trimmings_range() -> bevy::camera::visibility::VisibilityRange {
    bevy::camera::visibility::VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: TRIMMINGS..(TRIMMINGS * 1.12),
        use_aabb: false,
    }
}

/// How far through a stride this figure is, and how fast it is covering ground.
///
/// The speed is written by whoever owns the figure — the pedestrian AI for a
/// crowd, the character controller for the player — so the animation itself
/// does not need to know which it is looking at.
#[derive(Component, Default)]
pub struct WalkCycle {
    pub phase: f32,
    pub speed: f32,
}

#[derive(Resource)]
pub struct FigureAssets {
    torso: Handle<Mesh>,
    head: Handle<Mesh>,
    arm: Handle<Mesh>,
    leg: Handle<Mesh>,
    hand: Handle<Mesh>,
    hair: Handle<Mesh>,
    shoe: Handle<Mesh>,
    trousers: Vec<Handle<StandardMaterial>>,
    hair_colours: Vec<Handle<StandardMaterial>>,
    leather: Handle<StandardMaterial>,
    // The archetype wardrobe. Small, shared, and deliberately silly.
    crest: Handle<Mesh>,
    crest_colours: Vec<Handle<StandardMaterial>>,
    cup: Handle<Mesh>,
    phones_bar: Handle<Mesh>,
    plastic: Handle<StandardMaterial>,
    paper_cup: Handle<Mesh>,
    paper: Handle<StandardMaterial>,
    board: Handle<Mesh>,
    wood: Handle<StandardMaterial>,
    cane: Handle<Mesh>,
    wheel: Handle<Mesh>,
    castor: Handle<Mesh>,
    seat: Handle<Mesh>,
    seat_back: Handle<Mesh>,
    guitar_body: Handle<Mesh>,
    guitar_neck: Handle<Mesh>,
    camera_body: Handle<Mesh>,
    camera_lens: Handle<Mesh>,
    tray: Handle<Mesh>,
    ware: Handle<Mesh>,
}

/// Proportions, in metres, measured from the middle of the collider capsule.
///
/// The capsule is unchanged, so these have to fit inside it: feet on its bottom
/// cap, head under its top. A figure that pokes out of its own collider is one
/// that can be shot through the head without being hit.
/// The figure's own dimensions, in metres from the body's origin.
///
/// Public because anything that hangs something *on* a figure needs them —
/// an umbrella has to clear a head, and a head is a number that lives here.
pub mod body {
    pub const FEET: f32 = -0.845;
    pub const HIP: f32 = -0.09;
    pub const SHOULDER: f32 = 0.34;
    pub const SHOULDER_X: f32 = 0.19;
    pub const TORSO_CENTRE: f32 = 0.16;
    pub const TORSO_HEIGHT: f32 = 0.52;
    pub const HEAD_CENTRE: f32 = 0.62;
    pub const HEAD_RADIUS: f32 = 0.13;
    pub const LEG_LENGTH: f32 = HIP - FEET;
    pub const ARM_LENGTH: f32 = 0.60;
    pub const HAND_RADIUS: f32 = 0.062;
    /// A cap rather than a hairstyle: at the distance a pedestrian is normally
    /// seen, the only thing hair does is stop the head reading as a bare ball.
    pub const HAIR_RADIUS: f32 = 0.134;
    pub const HAIR_FLATTEN: f32 = 0.74;
    pub const HAIR_RISE: f32 = 0.026;
    pub const SHOE_HEIGHT: f32 = 0.062;
    pub const SHOE_LENGTH: f32 = 0.245;
    /// A punk's crest, standing on the crown.
    pub const CREST_CENTRE: f32 = 0.785;
    pub const CREST_HEIGHT: f32 = 0.10;
    /// The headphone bridge, lying over the crown, and the cups over the ears.
    pub const PHONES_BAR: f32 = 0.757;
    pub const PHONES_BAR_HEIGHT: f32 = 0.022;
    pub const CUP_RADIUS: f32 = 0.048;
    /// A skateboard under the feet, flush with the capsule's bottom cap.
    pub const BOARD_CENTRE: f32 = -0.825;
    pub const BOARD_THICKNESS: f32 = 0.04;
    /// A cane at the side, and the wheels a wheelchair rolls on.
    pub const CANE_CENTRE: f32 = -0.45;
    pub const CANE_HALF: f32 = 0.39;
    pub const WHEEL_CENTRE: f32 = -0.575;
    pub const WHEEL_RADIUS: f32 = 0.27;
    /// The rest of the chair. The seat sits directly under the hips rather
    /// than a third of a metre below them, because a seat a body is not
    /// touching is a footstool being carried around.
    pub const SEAT_CENTRE: f32 = HIP - 0.07;
    pub const SEAT_THICKNESS: f32 = 0.06;
    pub const SEAT_HALF_WIDTH: f32 = 0.20;
    /// A back to lean on, which is most of what tells a wheelchair apart
    /// from a pair of wheels at this distance.
    pub const BACK_CENTRE: f32 = 0.16;
    pub const BACK_HEIGHT: f32 = 0.46;
    pub const BACK_BEHIND: f32 = 0.19;
    /// The little front castors, whose bottoms rest on the same ground the
    /// big wheels do.
    pub const CASTOR_RADIUS: f32 = 0.085;
    pub const CASTOR_CENTRE: f32 = FEET + CASTOR_RADIUS;
    /// Far enough forward that the sitter's feet come down behind them
    /// rather than out past them — see the test.
    pub const CASTOR_AHEAD: f32 = -0.64;
    /// How far forward a seated figure's legs are swung, in radians off
    /// vertical. One segment per leg and no knee, so this is the compromise
    /// between a thigh (horizontal) and a shin (vertical); anything more
    /// puts the feet out past the castors.
    pub const SEATED_LEG: f32 = 0.92;
}

/// Half the collider capsule's height, which is what the figure has to fit in.
const CAPSULE_HALF: f32 = 0.845;

// Checked at compile time rather than in a test: these are all constants, so
// there is nothing to run — and a figure that pokes out of its own collider is
// one whose head can be shot at without being hit.
const _: () = {
    assert!(
        body::FEET >= -CAPSULE_HALF,
        "the feet hang below the capsule"
    );
    assert!(
        body::HEAD_CENTRE + body::HEAD_RADIUS <= CAPSULE_HALF,
        "the head stands above the capsule"
    );
    assert!(
        body::TORSO_CENTRE + body::TORSO_HEIGHT * 0.5 < body::HEAD_CENTRE - body::HEAD_RADIUS,
        "the head is inside the chest"
    );
    assert!(
        body::LEG_LENGTH > 0.0 && body::ARM_LENGTH > 0.0,
        "a limb has no length"
    );
    assert!(
        body::HEAD_CENTRE + body::HAIR_RISE + body::HAIR_RADIUS * body::HAIR_FLATTEN
            <= CAPSULE_HALF,
        "the hair stands above the capsule"
    );
    assert!(
        body::FEET + body::SHOE_HEIGHT * 0.5 >= -CAPSULE_HALF,
        "the shoes sink through the capsule"
    );
    // A hairline below the chin is a balaclava and one above the crown is a
    // beret. Neither fails, and neither is something anybody would report as a
    // bug — they would just say the pedestrians look odd.
    assert!(
        body::HEAD_CENTRE + body::HAIR_RISE - body::HAIR_RADIUS * body::HAIR_FLATTEN
            > body::HEAD_CENTRE - body::HEAD_RADIUS,
        "the hair has swallowed the face"
    );
    assert!(
        body::HEAD_CENTRE + body::HAIR_RISE - body::HAIR_RADIUS * body::HAIR_FLATTEN
            < body::HEAD_CENTRE + body::HEAD_RADIUS,
        "the hair is floating above the head"
    );
    // And wide enough to wrap it, or it sits on top like a lid.
    assert!(
        body::HAIR_RADIUS > body::HEAD_RADIUS,
        "the hair is narrower than the head it is on"
    );
    // The archetype furniture is the newest thing to test the ceiling: a
    // mohawk is exactly the shape that wants to stand out of the collider.
    assert!(
        body::CREST_CENTRE + body::CREST_HEIGHT * 0.5 <= CAPSULE_HALF,
        "the crest stands above the capsule"
    );
    assert!(
        body::CREST_CENTRE - body::CREST_HEIGHT * 0.5
            >= body::HEAD_CENTRE + body::HEAD_RADIUS * 0.3,
        "the crest grows out of the forehead"
    );
    assert!(
        body::PHONES_BAR + body::PHONES_BAR_HEIGHT * 0.5 <= CAPSULE_HALF,
        "the headphone bridge stands above the capsule"
    );
    assert!(
        body::BOARD_CENTRE - body::BOARD_THICKNESS * 0.5 >= -CAPSULE_HALF,
        "the skateboard hangs below the capsule"
    );
    assert!(
        body::CANE_CENTRE - body::CANE_HALF >= -CAPSULE_HALF,
        "the cane pokes through the pavement"
    );
    assert!(
        body::WHEEL_CENTRE - body::WHEEL_RADIUS >= -CAPSULE_HALF,
        "the wheels sink below the capsule"
    );
    assert!(
        body::CASTOR_CENTRE - body::CASTOR_RADIUS >= -CAPSULE_HALF,
        "the castors sink below the capsule"
    );
    assert!(
        body::SEAT_CENTRE + body::SEAT_THICKNESS * 0.5 <= body::HIP,
        "the seat is inside the sitter"
    );
    assert!(
        body::BACK_CENTRE + body::BACK_HEIGHT * 0.5 <= body::HEAD_CENTRE - body::HEAD_RADIUS,
        "the backrest reaches over the head"
    );
};

/// How square a rounded body is: 2 is a sphere, and large is a box.
///
/// Four is the shape a party balloon takes when you press it between two
/// hands, which is what a flummi's torso should be — the cast are made of the
/// same rubber as their own heads and were built out of a hard-edged cuboid
/// and two hard-edged bricks for feet. Nothing about a figure at forty pixels
/// tall reads except its outline, so the outline is the only place worth
/// spending anything.
const ROUNDNESS: f32 = 4.0;

/// A box with the edges inflated out of it.
///
/// A superellipsoid: every direction on a sphere is pushed out to where the
/// surface `|x|ⁿ + |y|ⁿ + |z|ⁿ = 1` is, then scaled to the size wanted. At n = 2
/// that is exactly the sphere it started as and at n = ∞ it is the cuboid it
/// replaces; everything interesting is in between.
///
/// Built off a UV sphere rather than an icosphere because the poles need to
/// land on the flat top and bottom, which is where a torso's shoulders are.
fn rounded_box(size: Vec3) -> Mesh {
    let mut mesh = Sphere::new(0.5).mesh().uv(16, 12);
    let half = size * 0.5;
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for position in positions.iter_mut() {
            let unit = Vec3::from(*position).normalize_or_zero();
            let power = unit.abs().powf(ROUNDNESS);
            let scale = (power.x + power.y + power.z)
                .max(1e-6)
                .powf(-1.0 / ROUNDNESS);
            *position = (unit * scale * half).to_array();
        }
    }
    // The normals came off the sphere and this is not one any more.
    mesh.compute_smooth_normals();
    mesh
}

// --------------------------------------------------- how round is round ----
//
// Bevy's primitive defaults are authored for a single hero object sitting in
// the middle of a scene, and every one of them was left alone here. That put
// the cast's whole triangle budget into its smoothest, least interesting
// surfaces: a plain citizen was 8400 triangles, of which 4096 — nearly half —
// were four featureless tubes, and a further 2160 were three balls under 14 cm
// across. The buildings behind them are 60-triangle boxes.
//
// So each shape below now carries a named resolution with the arithmetic that
// picked it. The measure throughout is the *sagitta*: how far a flat facet
// sags away from the true curve, `r · (1 − cos(π/n))` for an n-sided ring of
// radius r. A millimetre or two on a body part is well under a pixel at the
// distance a pedestrian is seen, and it is not recovered by the shading
// either — every one of these meshes is smooth-normalled, so what a coarser
// ring costs is the silhouette and nothing else.

/// Sides round a limb, and rings over each of its rounded ends.
///
/// `Capsule3d`'s default meshing is 32 longitudes by 16 latitudes = 1024
/// triangles, and four limbs at that rate were 4096 triangles on a figure that
/// is 40 pixels tall in the shots it actually appears in. At ten sides the
/// sagitta is 2.8 mm on a 58 mm arm and 3.8 mm on a 78 mm leg; twelve sides
/// would buy a millimetre of that back for another 24 triangles a limb, and
/// eight would give up 2 mm more on the leg — the outside of a thigh is the
/// one place on a figure with a long unbroken highlight down it, so that is
/// the wrong side to save on. Ten by six is 120 triangles, so the four limbs
/// cost 480 between them.
const LIMB_SIDES: u32 = 10;
/// Rings over a limb's cap. The caps are hemispheres nobody looks at: a
/// shoulder is inside the coat and a hip is inside the torso, and only the
/// wrist and the ankle are ever in the open, each with a hand or a shoe
/// parked on top of it.
const LIMB_RINGS: u32 = 6;

/// Subdivisions on the round odds and ends — the hands and the cap of hair.
///
/// `Sphere::mesh()` defaults to an icosphere at five subdivisions, which is
/// 720 triangles for a 62 mm hand. Two subdivisions is 180, and its facets
/// span about 23°, so the sagitta is 1.2 mm on a hand and 2.7 mm on the
/// 134 mm cap of hair. One subdivision (80 triangles) is where it stops being
/// free: 6 mm of sag on the hair, and the hair is nothing *but* silhouette —
/// it is the dark shape that stops a head fading into what is behind it.
///
/// The hands are also on [`trimmings_range`], so past 42 m they cost nothing
/// at all.
const BLOB_SUBDIVISIONS: u32 = 2;

/// Sides on a wheelchair's drive wheels.
///
/// The one round prop in the wardrobe big enough to need them: at 270 mm
/// radius, sixteen sides sag 5.2 mm, which on a 54 cm wheel is a hair over
/// 1%. `Cylinder`'s default 32 is 124 triangles and sixteen is 60, and a chair
/// carries two.
const WHEEL_SIDES: u32 = 16;

/// Sides on the small round props: ear cups, a paper cup, a camera lens, a
/// castor. Nothing here is over 85 mm in radius, where eight sides sag 6.5 mm
/// — and that worst case is a castor sitting 8 cm off the pavement under
/// somebody who is sitting down. 28 triangles apiece instead of 124.
const PROP_SIDES: u32 = 8;

/// Sides on the cane. An 18 mm stick: six sides sag 2.3 mm, and there is no
/// viewing distance at which a walking stick is not a line.
const STICK_SIDES: u32 = 6;

/// One limb: a capsule long enough to reach from its joint to its end, meshed
/// at [`LIMB_SIDES`] by [`LIMB_RINGS`] rather than at Bevy's default.
///
/// `length` is the whole limb, joint to tip, so the caps are subtracted out of
/// it — a capsule's `half_length` is the straight part only, and a leg built
/// without the subtraction is 78 mm too long and stands its owner on tiptoe.
fn limb_mesh(radius: f32, length: f32) -> Mesh {
    Capsule3d {
        radius,
        half_length: length * 0.5 - radius,
    }
    .mesh()
    .longitudes(LIMB_SIDES)
    .latitudes(LIMB_RINGS)
    .build()
}

/// A hand, or a cap of hair: an icosphere at [`BLOB_SUBDIVISIONS`].
///
/// An icosphere rather than a UV sphere because neither of these carries a
/// texture — both are flat colour — so there is no reason to pay for the
/// pole-to-pole layout the head needs, and an icosphere spends its triangles
/// evenly instead of crowding them into two points nobody sees.
fn blob_mesh(radius: f32) -> Mesh {
    Sphere::new(radius)
        .mesh()
        .ico(BLOB_SUBDIVISIONS)
        .expect("an icosphere at two subdivisions is well inside Bevy's limit")
}

/// One round thing the cast carries, at a stated number of sides.
///
/// The sides are passed rather than taken from the radius, because what
/// decides them is how close the prop is ever looked at and not how big it is:
/// a wheelchair's wheel is at the player's knee and its castors are on the
/// floor behind it.
fn prop_mesh(radius: f32, height: f32, sides: u32) -> Mesh {
    Cylinder::new(radius, height)
        .mesh()
        .resolution(sides)
        .build()
}

/// Repeats of the weave across one garment.
///
/// A cuboid's faces and a capsule's shell both carry UVs from zero to one, so
/// this is a count rather than a size — and the garments are all within a
/// factor of two of each other, which is what lets one number serve. At
/// fourteen a torso's threads come out around two millimetres, which is cloth.
pub const WEAVE_TILE: f32 = 14.0;

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> FigureAssets {
    // One weave, worn by everybody. Every citizen was dressed in flat colour at
    // a wool roughness, which is a material that does not exist: cloth is a
    // *surface*, and what says so is the shading in the valleys between its
    // threads. It costs two textures for the whole cast.
    let weave = images.add(crate::world::texture::fabric());
    let weave_relief = images.add(crate::world::texture::fabric_normal());
    let cloth = |color: Color| StandardMaterial {
        base_color: color,
        base_color_texture: Some(weave.clone()),
        normal_map_texture: Some(weave_relief.clone()),
        uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(WEAVE_TILE)),
        perceptual_roughness: 0.88,
        ..default()
    };
    // Skin is not cloth — at a wool coat's roughness a face takes no highlight
    // at all and reads as a mannequin — but no skin is mixed here any more.
    // Every uncovered part of a flummi is its complexion, which is its mood,
    // and both the head and the hands take their material from
    // `crate::mood::face` for that reason.

    FigureAssets {
        torso: meshes.add(rounded_box(Vec3::new(0.36, body::TORSO_HEIGHT, 0.22))),
        // A UV sphere rather than the default icosphere: the face is painted
        // into a texture, and an icosphere's seams run wherever they like.
        // See `crate::mood::face::head_mesh` for why it is turned on its side.
        head: meshes.add(crate::mood::face::head_mesh(body::HEAD_RADIUS)),
        arm: meshes.add(limb_mesh(0.058, body::ARM_LENGTH)),
        leg: meshes.add(limb_mesh(0.078, body::LEG_LENGTH)),
        hand: meshes.add(blob_mesh(body::HAND_RADIUS)),
        hair: meshes.add(blob_mesh(body::HAIR_RADIUS)),
        shoe: meshes.add(rounded_box(Vec3::new(
            0.105,
            body::SHOE_HEIGHT,
            body::SHOE_LENGTH,
        ))),
        hair_colours: [
            Color::srgb(0.07, 0.06, 0.06),
            Color::srgb(0.19, 0.13, 0.09),
            Color::srgb(0.35, 0.26, 0.16),
            Color::srgb(0.52, 0.49, 0.47),
        ]
        .into_iter()
        // Hair is matte and dark, and it is the darkness that does the work:
        // what a cap of it buys is a head that ends in a shape instead of
        // fading into whatever is behind it.
        .map(|color| materials.add(cloth(color)))
        .collect(),
        leather: materials.add(StandardMaterial {
            base_color: Color::srgb(0.09, 0.08, 0.08),
            perceptual_roughness: 0.64,
            ..default()
        }),
        trousers: [
            Color::srgb(0.16, 0.18, 0.24),
            Color::srgb(0.22, 0.20, 0.18),
            Color::srgb(0.12, 0.13, 0.14),
        ]
        .into_iter()
        .map(|color| materials.add(cloth(color)))
        .collect(),
        // A blade of hair rather than a fan of spikes: at pavement distance
        // the crest is a silhouette, and a box is the silhouette.
        crest: meshes.add(Cuboid::new(0.035, body::CREST_HEIGHT, 0.24)),
        crest_colours: [Color::srgb(0.08, 0.72, 0.32), Color::srgb(0.85, 0.16, 0.55)]
            .into_iter()
            .map(|color| materials.add(cloth(color)))
            .collect(),
        cup: meshes.add(prop_mesh(body::CUP_RADIUS, 0.035, PROP_SIDES)),
        phones_bar: meshes.add(Cuboid::new(
            (body::HEAD_RADIUS + 0.02) * 2.0,
            body::PHONES_BAR_HEIGHT,
            0.032,
        )),
        plastic: materials.add(StandardMaterial {
            base_color: Color::srgb(0.11, 0.11, 0.13),
            perceptual_roughness: 0.35,
            ..default()
        }),
        paper_cup: meshes.add(prop_mesh(0.045, 0.095, PROP_SIDES)),
        paper: materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.80, 0.70),
            perceptual_roughness: 0.9,
            ..default()
        }),
        board: meshes.add(Cuboid::new(0.22, body::BOARD_THICKNESS, 0.56)),
        wood: materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.28, 0.16),
            perceptual_roughness: 0.7,
            ..default()
        }),
        cane: meshes.add(prop_mesh(0.018, body::CANE_HALF * 2.0, STICK_SIDES)),
        wheel: meshes.add(prop_mesh(body::WHEEL_RADIUS, 0.03, WHEEL_SIDES)),
        castor: meshes.add(prop_mesh(body::CASTOR_RADIUS, 0.025, PROP_SIDES)),
        seat: meshes.add(Cuboid::new(
            body::SEAT_HALF_WIDTH * 2.0,
            body::SEAT_THICKNESS,
            0.40,
        )),
        seat_back: meshes.add(Cuboid::new(
            body::SEAT_HALF_WIDTH * 2.0,
            body::BACK_HEIGHT,
            0.05,
        )),
        // The performers' tools. Boxes and cylinders, like everything the
        // cast owns: at pedestrian distance a silhouette does all the work —
        // and the cylinders are cut to as many sides as their silhouette
        // actually needs, which for a 35 mm camera lens is eight.
        guitar_body: meshes.add(Cuboid::new(0.26, 0.34, 0.09)),
        guitar_neck: meshes.add(Cuboid::new(0.05, 0.38, 0.04)),
        camera_body: meshes.add(Cuboid::new(0.17, 0.11, 0.09)),
        camera_lens: meshes.add(prop_mesh(0.035, 0.07, PROP_SIDES)),
        tray: meshes.add(Cuboid::new(0.48, 0.05, 0.30)),
        ware: meshes.add(Cuboid::new(0.09, 0.07, 0.09)),
    }
}

/// Hangs a figure off an entity that already has its collider and behaviour.
///
/// The face and the complexion arrive already chosen, because which ones they
/// are depends on how the figure feels — see [`crate::mood::face::Worn`].
/// Nobody in this city has a skin tone; every head is an emoji.
///
/// The archetype decides the extras — a crest, a pompadour, headphones, a
/// paper cup — and every one of them is a child with a [`Rest`], because a
/// child without one is silently skipped by [`animate`] and pops off the
/// figure at the first squash.
pub fn dress(
    entity: &mut EntityCommands,
    assets: &FigureAssets,
    coat: Handle<StandardMaterial>,
    worn: &crate::mood::face::Worn,
    archetype: crate::ai::archetype::Archetype,
    rng: &mut ChaCha8Rng,
) {
    use crate::ai::archetype::Archetype;

    let trousers = assets.trousers[rng.random_range(0..assets.trousers.len())].clone();
    // Always drawn, even for the bald and the crested: the wardrobe stream
    // must consume the same draws whoever is being dressed, or retuning the
    // cast would reshuffle every trouser leg after it.
    let hair = assets.hair_colours[rng.random_range(0..assets.hair_colours.len())].clone();
    let crest = assets.crest_colours[rng.random_range(0..assets.crest_colours.len())].clone();

    entity.insert(WalkCycle::default());
    // Nobody in a wheelchair is taking a step. Without this the stride runs
    // anyway — the walk cycle is paced by ground covered, and a chair covers
    // ground — and a seated figure that keeps striding reads as somebody
    // standing up, taking one step and sitting down again, about twice a
    // second.
    if archetype == Archetype::Wheelchair {
        entity.insert(Seated);
    }
    entity.with_children(|parent| {
        let torso = Vec3::new(0.0, body::TORSO_CENTRE, 0.0);
        parent.spawn((
            Rest::at(torso),
            Mesh3d(assets.torso.clone()),
            MeshMaterial3d(coat.clone()),
            Transform::from_translation(torso),
        ));
        let head = Vec3::new(0.0, body::HEAD_CENTRE, 0.0);
        parent.spawn((
            Head,
            Rest::at(head),
            Mesh3d(assets.head.clone()),
            MeshMaterial3d(worn.face.clone()),
            Transform::from_translation(head),
        ));
        // A shade wider than the head and sat a little high and a little back,
        // so the crown is covered and the face below it is not. The hairline
        // this puts on a figure is low — the cap cannot rise much further and
        // still fit inside the collider — but at the distance a pedestrian is
        // seen it is the dark top to the silhouette that does the work, not
        // where exactly it starts.
        //
        // A missionary goes without: the shaved head is most of the costume.
        if !archetype.bald() {
            let (cap, cap_scale, cap_hair) = if archetype == Archetype::Elvis {
                // The pompadour: the same cap, always black, worn taller and
                // pushed forward until it is a hairstyle rather than a hat.
                (
                    Vec3::new(0.0, body::HEAD_CENTRE + body::HAIR_RISE, -0.016),
                    Vec3::new(0.96, body::HAIR_FLATTEN * 1.28, 1.08),
                    assets.hair_colours[0].clone(),
                )
            } else {
                (
                    Vec3::new(0.0, body::HEAD_CENTRE + body::HAIR_RISE, 0.018),
                    Vec3::new(1.0, body::HAIR_FLATTEN, 1.0),
                    hair.clone(),
                )
            };
            parent.spawn((
                Rest::posed(cap, cap_scale),
                Mesh3d(assets.hair.clone()),
                MeshMaterial3d(cap_hair),
                Transform::from_translation(cap).with_scale(cap_scale),
            ));
        }

        match archetype {
            Archetype::Punk => {
                let at = Vec3::new(0.0, body::CREST_CENTRE, 0.0);
                parent.spawn((
                    Rest::at(at),
                    Mesh3d(assets.crest.clone()),
                    MeshMaterial3d(crest),
                    Transform::from_translation(at),
                ));
            }
            Archetype::Headphones => {
                let bar = Vec3::new(0.0, body::PHONES_BAR, 0.0);
                parent.spawn((
                    Rest::at(bar),
                    Mesh3d(assets.phones_bar.clone()),
                    MeshMaterial3d(assets.plastic.clone()),
                    Transform::from_translation(bar),
                ));
                for side in [-1.0f32, 1.0] {
                    let cup = Vec3::new(side * (body::HEAD_RADIUS + 0.012), body::HEAD_CENTRE, 0.0);
                    parent.spawn((
                        Rest::at(cup),
                        Mesh3d(assets.cup.clone()),
                        MeshMaterial3d(assets.plastic.clone()),
                        Transform::from_translation(cup)
                            // A cylinder stands on Y; an ear cup lies on X.
                            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                    ));
                }
            }
            Archetype::Beggar => {
                // The cup, held out in front. It travels with the figure,
                // which is not how begging works and exactly how comedy does.
                let at = Vec3::new(0.0, -0.26, 0.22);
                parent.spawn((
                    Rest::at(at),
                    Mesh3d(assets.paper_cup.clone()),
                    MeshMaterial3d(assets.paper.clone()),
                    Transform::from_translation(at),
                ));
            }
            Archetype::Skater => {
                // The board rides under the feet, flush with the capsule's
                // bottom — the legs still pump above it, which is the joke.
                let at = Vec3::new(0.0, body::BOARD_CENTRE, 0.0);
                parent.spawn((
                    Rest::at(at),
                    Mesh3d(assets.board.clone()),
                    MeshMaterial3d(assets.wood.clone()),
                    Transform::from_translation(at),
                ));
            }
            Archetype::CaneUser => {
                let at = Vec3::new(0.26, body::CANE_CENTRE, 0.05);
                parent.spawn((
                    Rest::at(at),
                    Mesh3d(assets.cane.clone()),
                    MeshMaterial3d(assets.wood.clone()),
                    Transform::from_translation(at),
                ));
            }
            Archetype::Busker => {
                // The guitar, slung across the chest with the neck rising
                // past the left shoulder. The figure faces -Z, the way the
                // castors say, so the instrument hangs on the minus side.
                let strings = Vec3::new(0.03, body::TORSO_CENTRE - 0.06, -0.19);
                parent.spawn((
                    Rest::at(strings),
                    Mesh3d(assets.guitar_body.clone()),
                    MeshMaterial3d(assets.wood.clone()),
                    Transform::from_translation(strings)
                        .with_rotation(Quat::from_rotation_z(-0.35)),
                ));
                let neck = Vec3::new(-0.17, body::TORSO_CENTRE + 0.16, -0.19);
                parent.spawn((
                    Rest::at(neck),
                    Mesh3d(assets.guitar_neck.clone()),
                    MeshMaterial3d(assets.leather.clone()),
                    Transform::from_translation(neck).with_rotation(Quat::from_rotation_z(-0.55)),
                ));
            }
            Archetype::Photographer => {
                // The camera lives at the face, permanently raised: the
                // figure *is* mid-shot, whatever else it is doing.
                let camera = Vec3::new(0.0, body::HEAD_CENTRE - 0.04, -(body::HEAD_RADIUS + 0.10));
                parent.spawn((
                    Rest::at(camera),
                    Mesh3d(assets.camera_body.clone()),
                    MeshMaterial3d(assets.plastic.clone()),
                    Transform::from_translation(camera),
                ));
                // A cylinder stands on Y; a lens looks where the figure does.
                let lens = Vec3::new(0.0, body::HEAD_CENTRE - 0.04, -(body::HEAD_RADIUS + 0.17));
                parent.spawn((
                    Rest::at(lens),
                    Mesh3d(assets.camera_lens.clone()),
                    MeshMaterial3d(assets.leather.clone()),
                    Transform::from_translation(lens)
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
            }
            Archetype::Vendor => {
                // The tray, carried in front like the beggar's cup grown a
                // business plan, with three unspecified wares on it in
                // whatever colours the wardrobe was already holding.
                let tray = Vec3::new(0.0, body::TORSO_CENTRE - 0.20, -0.26);
                parent.spawn((
                    Rest::at(tray),
                    Mesh3d(assets.tray.clone()),
                    MeshMaterial3d(assets.wood.clone()),
                    Transform::from_translation(tray),
                ));
                for (slot, material) in [
                    (-0.14, assets.paper.clone()),
                    (0.0, crest.clone()),
                    (0.14, assets.plastic.clone()),
                ] {
                    let ware = Vec3::new(slot, body::TORSO_CENTRE - 0.14, -0.26);
                    parent.spawn((
                        Rest::at(ware),
                        Mesh3d(assets.ware.clone()),
                        MeshMaterial3d(material),
                        Transform::from_translation(ware),
                    ));
                }
            }
            Archetype::Wheelchair => {
                // A cylinder stands on Y, so every wheel here is laid onto an
                // axle across the chair — a rotation about Z. About X the
                // axle would point the way the chair travels and the wheels
                // would be held out in front and behind like paddles.
                let onto_the_axle = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);

                let seat = Vec3::new(0.0, body::SEAT_CENTRE, 0.0);
                parent.spawn((
                    Rest::at(seat),
                    Mesh3d(assets.seat.clone()),
                    MeshMaterial3d(assets.plastic.clone()),
                    Transform::from_translation(seat),
                ));
                // Behind, so +Z: the figure faces -Z, the way the shoes point.
                let back = Vec3::new(0.0, body::BACK_CENTRE, body::BACK_BEHIND);
                parent.spawn((
                    Rest::at(back),
                    Mesh3d(assets.seat_back.clone()),
                    MeshMaterial3d(assets.plastic.clone()),
                    Transform::from_translation(back),
                ));
                for side in [-1.0f32, 1.0] {
                    let hub = Vec3::new(side * 0.24, body::WHEEL_CENTRE, 0.0);
                    parent.spawn((
                        Rest::at(hub),
                        Mesh3d(assets.wheel.clone()),
                        MeshMaterial3d(assets.plastic.clone()),
                        Transform::from_translation(hub).with_rotation(onto_the_axle),
                    ));
                    let castor = Vec3::new(side * 0.17, body::CASTOR_CENTRE, body::CASTOR_AHEAD);
                    parent.spawn((
                        Rest::at(castor),
                        Mesh3d(assets.castor.clone()),
                        MeshMaterial3d(assets.leather.clone()),
                        Transform::from_translation(castor).with_rotation(onto_the_axle),
                    ));
                }
            }
            _ => {}
        }

        for (limb, side) in [(Limb::LeftArm, -1.0f32), (Limb::RightArm, 1.0)] {
            let joint = Vec3::new(side * body::SHOULDER_X, body::SHOULDER, 0.0);
            parent
                .spawn((
                    limb,
                    Rest::at(joint),
                    Transform::from_translation(joint),
                    Visibility::default(),
                ))
                // Hung below the joint, so the parent's rotation swings it from
                // the shoulder rather than spinning it about its own middle.
                .with_children(|joint| {
                    joint.spawn((
                        Mesh3d(assets.arm.clone()),
                        MeshMaterial3d(coat.clone()),
                        Transform::from_xyz(0.0, -body::ARM_LENGTH * 0.5, 0.0),
                    ));
                    // A sleeve that ends in nothing is the other half of why
                    // a figure reads as a shop dummy. The hand is one sphere
                    // and it swings with the arm because it hangs off the
                    // same joint.
                    joint.spawn((
                        Bare,
                        Mesh3d(assets.hand.clone()),
                        MeshMaterial3d(worn.bare.clone()),
                        Transform::from_xyz(0.0, -body::ARM_LENGTH, 0.0),
                        trimmings_range(),
                    ));
                });
        }

        for (limb, side) in [(Limb::LeftLeg, -1.0f32), (Limb::RightLeg, 1.0)] {
            let joint = Vec3::new(side * 0.10, body::HIP, 0.0);
            parent
                .spawn((
                    limb,
                    Rest::at(joint),
                    Transform::from_translation(joint),
                    Visibility::default(),
                ))
                .with_children(|joint| {
                    joint.spawn((
                        Mesh3d(assets.leg.clone()),
                        MeshMaterial3d(trousers.clone()),
                        Transform::from_xyz(0.0, -body::LEG_LENGTH * 0.5, 0.0),
                    ));
                    // Toes forward, and the sole exactly on the capsule's
                    // bottom cap — which is where the ground check puts the
                    // figure, so this is the one part that has to be right or
                    // everybody walks on their ankles.
                    joint.spawn((
                        Mesh3d(assets.shoe.clone()),
                        MeshMaterial3d(assets.leather.clone()),
                        Transform::from_xyz(
                            0.0,
                            -body::LEG_LENGTH + body::SHOE_HEIGHT * 0.5,
                            -body::SHOE_LENGTH * 0.22,
                        ),
                        trimmings_range(),
                    ));
                });
        }
    });
}

/// Angle a limb is swung to, at a given point in the stride.
pub fn limb_angle(limb: Limb, phase: f32) -> f32 {
    (phase + limb.phase_offset()).sin() * limb.amplitude()
}

/// Advances every figure's stride, poses its limbs, and squashes it into
/// whatever part of its hop it is in.
///
/// The stride and the hop are independent on purpose. The legs are paced by
/// ground covered and the squash by the bounce, so a flummi sailing across a
/// junction is still running in mid-air — which is both what a cartoon does and
/// what the walk cycle would do anyway if asked.
pub fn animate(
    time: Res<Time>,
    config: Res<crate::core::config::GameConfig>,
    figures: Query<(
        &mut WalkCycle,
        Option<&Bouncer>,
        Option<&Stature>,
        Option<&Posture>,
        Option<&Seated>,
        Option<&Attention>,
        &GlobalTransform,
        &Children,
    )>,
    mut parts: Query<(&mut Transform, &Rest, Option<&Limb>, Option<&Head>)>,
) {
    let dt = time.delta_secs();
    let elapsed = time.elapsed_secs();
    for (mut cycle, bouncer, stature, posture, seated, attention, placed, children) in figures {
        // Driven by distance covered, not by time: someone running has to take
        // faster steps, not longer ones, or they moonwalk.
        cycle.phase = (cycle.phase + cycle.speed / STRIDE * TAU_F32 * dt) % TAU_F32;
        // And a figure that has stopped brings its feet together instead of
        // freezing mid-stride. Eased rather than snapped, so somebody who
        // stops for a word arrives at standing over about a third of a
        // second — which is what stopping looks like.
        if cycle.speed < STANDING_STILL {
            let settle = (SETTLE_RATE * dt).min(1.0);
            cycle.phase += shortest_turn(cycle.phase, 0.0) * settle;
            cycle.phase = cycle.phase.rem_euclid(TAU_F32);
        }

        let (vertical, horizontal) = match bouncer {
            // The gait gates the squash along with the hop: a walking body is
            // permanently at phase zero as far as the bouncer can tell, and
            // holding the landing squash forever reads as a rendering bug.
            Some(bouncer) => crate::bounce::squash::stretch(
                bouncer.hop_phase(),
                config.gait.squash(config.bounce.squash),
            ),
            None => (1.0, 1.0),
        };
        // The stature multiplies straight into the pose: a child is an adult
        // squashed evenly, every frame, which is also exactly what a rubber
        // city would say about children.
        let size = stature.map_or(1.0, |stature| stature.0);
        let pose = Vec3::new(horizontal, vertical, horizontal) * size;

        // Where the neck wants to be, in the body's own frame. `None` while
        // there is nothing worth looking at, which is when the head goes back
        // to facing the way the body does.
        let looking = attention
            .filter(|attention| attention.until > elapsed)
            .and_then(|attention| neck(placed, attention.at));

        for &child in children {
            let Ok((mut transform, rest, limb, head)) = parts.get_mut(child) else {
                continue;
            };
            // The rest pose scaled by the squash, so parts stay attached to one
            // another as the body flattens instead of pulling apart at the neck.
            transform.translation = rest.at * pose;
            transform.scale = rest.scale * pose;
            if let Some(limb) = limb {
                let angle = match (posture, seated) {
                    // A posture is the loudest thing on the body and wins
                    // over both of the others.
                    (Some(posture), _) => posture.limb_angle(*limb, elapsed),
                    (None, Some(_)) => seated_angle(*limb),
                    (None, None) => limb_angle(*limb, cycle.phase),
                };
                transform.rotation = Quat::from_rotation_x(angle);
            } else if head.is_some() {
                // Slewed rather than set: a head that snaps onto its target is
                // a turret. Six or seven radians a second is a glance.
                let (yaw, pitch) = looking.unwrap_or((0.0, 0.0));
                let wanted = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
                transform.rotation = transform
                    .rotation
                    .slerp(wanted, (NECK_SLEW * dt).clamp(0.0, 1.0));
            }
        }
    }
}

const TAU_F32: f32 = std::f32::consts::TAU;

/// Yaw and pitch that turn a figure's head towards a world point.
///
/// In the body's own frame, and clamped to what a neck does: past the limit the
/// head simply stops turning rather than following something round behind the
/// figure, which is the one failure mode that reads as possession rather than
/// as interest. `None` when the target is too close to be looked at, where the
/// direction is noise.
fn neck(body: &GlobalTransform, at: Vec3) -> Option<(f32, f32)> {
    let to = at - body.translation();
    if to.length_squared() < 0.16 {
        return None;
    }
    // Into the body's frame. The head hangs off the body, so what matters is
    // the body's rotation and nothing else.
    let local = body.rotation().inverse() * to;
    // A figure faces -Z.
    let yaw = (-local.x).atan2(-local.z);
    if yaw.abs() > NECK_YAW {
        return None;
    }
    let flat = local.xz().length().max(1e-3);
    let pitch = (local.y / flat).atan().clamp(-NECK_PITCH, NECK_PITCH);
    // Positive rotation about X tips a head *down* here, the same convention
    // the limbs use, so looking up is negative.
    Some((yaw, -pitch))
}

/// Under this, in metres per second, a figure is standing rather than walking.
///
/// Well under a stroll and well over the drift a dynamic body has while it is
/// being leaned on by the crowd around it.
const STANDING_STILL: f32 = 0.22;
/// How fast a stopped figure brings its feet together, in 1/s.
const SETTLE_RATE: f32 = 7.0;

/// The shorter way round from one phase to another.
fn shortest_turn(from: f32, to: f32) -> f32 {
    let delta = (to - from).rem_euclid(TAU_F32);
    if delta > TAU_F32 * 0.5 {
        delta - TAU_F32
    } else {
        delta
    }
}

/// Paces the crowd's figures from how fast they are actually moving.
///
/// Off the body, not off the pedestrian AI's intent, and for the same reason
/// the player's is: half the interesting things that happen to a citizen are
/// *overrides*. Stopping for a chat, staring into a window, holding a grudge,
/// being knocked flying — every one of them writes over the walking intent
/// somewhere in `ai::social` or `mood`, and none of them thought to write the
/// walk speed back down. What that looked like was a city where everybody who
/// stopped to talk carried on striding on the spot.
pub fn pace_pedestrians(
    mut walkers: Query<
        (&avian3d::prelude::LinearVelocity, &mut WalkCycle),
        With<super::pedestrian::Pedestrian>,
    >,
) {
    for (velocity, mut cycle) in &mut walkers {
        cycle.speed = velocity.0.xz().length();
    }
}

/// And the player's, from how fast they are actually moving.
///
/// Read off the body rather than off the input, so being shoved by a car or
/// sliding down a kerb moves the legs too.
pub fn pace_player(
    mut player: Query<
        (&avian3d::prelude::LinearVelocity, &mut WalkCycle),
        With<crate::player::on_foot::Player>,
    >,
) {
    for (velocity, mut cycle) in &mut player {
        cycle.speed = velocity.0.xz().length();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_legs_are_always_out_of_step_with_each_other() {
        // Both legs forward at once is a hop, not a walk.
        for step in 0..64 {
            let phase = TAU_F32 * step as f32 / 64.0;
            let left = limb_angle(Limb::LeftLeg, phase);
            let right = limb_angle(Limb::RightLeg, phase);
            assert!(
                (left + right).abs() < 1e-5,
                "at phase {phase:.2} the legs are at {left:.2} and {right:.2}"
            );
        }
    }

    #[test]
    fn an_arm_swings_opposite_the_leg_on_the_same_side() {
        // Same-side arm and leg swinging together is the walk of someone
        // thinking very hard about walking.
        for step in 0..64 {
            let phase = TAU_F32 * step as f32 / 64.0;
            let arm = limb_angle(Limb::LeftArm, phase);
            let leg = limb_angle(Limb::LeftLeg, phase);
            assert!(
                arm * leg <= 1e-6,
                "at phase {phase:.2} the left arm and leg both went {arm:.2}/{leg:.2}"
            );
        }
    }

    #[test]
    fn arms_swing_less_than_legs() {
        let peak = |limb: Limb| {
            (0..256)
                .map(|i| limb_angle(limb, TAU_F32 * i as f32 / 256.0).abs())
                .fold(0.0, f32::max)
        };
        assert!(peak(Limb::LeftArm) < peak(Limb::LeftLeg));
    }

    #[test]
    fn a_seated_figure_holds_both_legs_still_and_forward() {
        // The bug this pins: a wheelchair user striding. Both legs at the
        // same angle is the whole difference between sitting and walking —
        // the walk cycle's own test asserts the opposite, that the two legs
        // are never together.
        let left = seated_angle(Limb::LeftLeg);
        let right = seated_angle(Limb::RightLeg);
        assert_eq!(left, right, "one leg is taking a step");
        assert!(left > 0.0, "the legs should be swung forward, not back");
    }

    #[test]
    fn a_seated_figures_feet_stay_off_the_pavement() {
        // Legs swung off the hip with no knee to bend: too far forward and
        // the feet stick out past the castors, not far enough and they drag.
        let angle = seated_angle(Limb::LeftLeg);
        let foot_y = body::HIP - body::LEG_LENGTH * angle.cos();
        let foot_z = -body::LEG_LENGTH * angle.sin();
        assert!(
            foot_y > body::FEET,
            "the feet are dragging along the ground at {foot_y:.2}"
        );
        assert!(
            foot_z > body::CASTOR_AHEAD,
            "the feet reach out past the castors to {foot_z:.2}"
        );
    }

    /// Triangles in a built mesh: what a part actually costs the GPU, rather
    /// than what the primitive's name suggests it costs.
    fn triangles(mesh: &Mesh) -> usize {
        match mesh.indices() {
            Some(indices) => indices.len() / 3,
            None => mesh.count_vertices() / 3,
        }
    }

    #[test]
    fn a_plain_citizen_costs_a_quarter_of_what_it_used_to() {
        // 8400 triangles, of which 4096 were four limbs at `Capsule3d`'s
        // default 32x16 meshing, 1440 two hands at `Sphere`'s default
        // icosphere, and 1088 one head at Bevy's default UV sphere. The
        // buildings behind them are 60-triangle boxes, which is most of why
        // the city read as angular: nearly everything round in the frame was
        // a person, and the budget had gone into smoothing four featureless
        // tubes that are 40 pixels tall on screen.
        //
        // Pinned rather than bounded, so that a part added at a primitive's
        // default resolution shows up here as a failing test instead of
        // showing up in a month as a frame rate.
        let torso = triangles(&rounded_box(Vec3::new(0.36, body::TORSO_HEIGHT, 0.22)));
        let head = triangles(&crate::mood::face::head_mesh(body::HEAD_RADIUS));
        let hair = triangles(&blob_mesh(body::HAIR_RADIUS));
        let hand = triangles(&blob_mesh(body::HAND_RADIUS));
        let arm = triangles(&limb_mesh(0.058, body::ARM_LENGTH));
        let leg = triangles(&limb_mesh(0.078, body::LEG_LENGTH));
        let shoe = triangles(&rounded_box(Vec3::new(
            0.105,
            body::SHOE_HEIGHT,
            body::SHOE_LENGTH,
        )));

        // The torso and the shoes are the one part of the figure that was
        // already right: a superellipsoid off `uv(16, 12)`, 352 triangles,
        // spent on the only silhouette a figure has at any distance.
        assert_eq!((torso, shoe), (352, 352));
        assert_eq!((head, hair, hand), (440, 180, 180));
        assert_eq!((arm, leg), (120, 120));

        let citizen = torso + head + hair + 2 * (arm + leg + hand + shoe);
        assert_eq!(citizen, 2516, "the cast has put weight back on");
    }

    #[test]
    fn nothing_round_in_the_wardrobe_is_still_meshed_at_thirty_two_sides() {
        // `Cylinder`'s default is 124 triangles whatever its radius, and a
        // wheelchair paid it four times over — 496 triangles of furniture on
        // a figure whose entire body is 2516 — while a 36 mm cane paid it for
        // a stick that is a line at every distance it is seen from.
        assert_eq!(
            triangles(&prop_mesh(body::WHEEL_RADIUS, 0.03, WHEEL_SIDES)),
            60
        );
        assert_eq!(
            triangles(&prop_mesh(body::CASTOR_RADIUS, 0.025, PROP_SIDES)),
            28
        );
        assert_eq!(
            triangles(&prop_mesh(0.018, body::CANE_HALF * 2.0, STICK_SIDES)),
            20
        );
    }

    #[test]
    fn the_head_rides_above_the_chest_at_every_squash() {
        // The parts are posed by scaling one rest pose, so they can only come
        // apart if the scale is applied to one of them and not the other.
        for step in 0..16 {
            let (vertical, _) = crate::bounce::squash::stretch(step as f32 / 16.0, 0.35);
            let head = body::HEAD_CENTRE * vertical;
            let chest = body::TORSO_CENTRE * vertical;
            assert!(
                head > chest,
                "the head sank into the chest at {vertical:.2}"
            );
        }
    }
}
