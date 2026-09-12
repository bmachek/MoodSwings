//! The face on the front of every head, painted rather than typed.
//!
//! An emoji font would have been a morning's work and the wrong answer twice
//! over. It would be the only piece of third-party art in a project that paints
//! its own bricks, and — worse — a font can only hand back the moods somebody
//! else drew. A mood here is a continuum, and the face has to be able to sit
//! anywhere on it, including the places between 😠 and 😐 where most of the
//! comedy lives.
//!
//! So the face is a function of one number. Every feature — the complexion, the
//! tilt of the brows, how far the eyes are screwed up, which way the mouth
//! bends — is interpolated from that number, and the whole thing is painted per
//! texel by [`shade`].
//!
//! ## Why the head is a UV sphere, rotated
//!
//! Bevy's UV sphere is built with its poles on **±Z**, not ±Y: the stack angle
//! drives the third component of each vertex. Left alone that would put a pole
//! singularity — where every texel of one texture row collapses to a point —
//! exactly where the face goes, since a figure faces its local −Z. So the mesh
//! is rotated a quarter turn about X, which stands the poles up on ±Y where a
//! head's poles belong and brings the equator's u = 0.25 round to the front.
//! Hence [`FACE_U`]. It is worth knowing that this is derived rather than
//! guessed: the u of a vertex is `sector / sectors` and its sector angle runs
//! anticlockwise from +X, so a quarter of the way round is +Z before the
//! rotation and −Z after it.
//!
//! ## Why thirteen faces and not one
//!
//! Repainting a 256² texture whenever a mood moves would be a texture upload
//! per flummi per frame. Instead the mood is quantised to thirteen levels, each
//! with its own texture and material baked at startup, and a figure whose level
//! changes swaps a material handle. Thirteen is enough that the steps are not
//! visible on a face 13 cm across, and the whole set costs about 4 MB.

use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::world::texture::{byte, painted, smoothstep01};

/// Where the middle of the face sits along the head's texture. See the module
/// note: derived from the UV sphere's winding, not tuned by eye.
pub const FACE_U: f32 = 0.25;

/// Distinct faces baked at startup. Odd, so that one of them is exactly
/// neutral.
pub const LEVELS: usize = 13;

/// The HUD portrait shows only the face, so it needs far less.
const PORTRAIT_SIZE: u32 = 96;

/// How the head sphere is divided: meridians round it, and rows of quads from
/// pole to pole.
///
/// It was left at Bevy's `uv(32, 18)` default, which is 1088 triangles — the
/// single most expensive mesh on a citizen, and more than the four limbs now
/// cost between them. A head is 13 cm across: at twenty meridians a facet
/// spans 18° and sags 1.6 mm off the true sphere, at twelve stacks 15° and
/// 1.1 mm, and the clearcoat over the top is what sells the roundness anyway.
/// `uv(20, 12)` is 440 triangles.
///
/// The count is otherwise free to move, and deliberately so: [`FACE_U`] is a
/// *fraction* of the way round, derived from the sphere's winding rather than
/// from any particular vertex, and [`wrapped`] paints the face into the
/// texture, not into the mesh. The two divisibility rules below are the whole
/// of what the face asks of the tessellation, and they are asserted rather
/// than remembered.
const SECTORS: u32 = 20;
const STACKS: u32 = 12;

// A multiple of four puts a meridian exactly on FACE_U = 0.25, so the face is
// tessellated symmetrically about its own centre line rather than with one eye
// straddling a seam and the other not; an even number of stacks puts a ring of
// vertices on the equator, which is where the face's y = 0 lands. Neither is
// load-bearing for correctness — the texture would still map — but both are
// free, and getting them wrong is the sort of asymmetry that reads as a
// wonky face without ever looking like a bug.
const _: () = {
    assert!(
        SECTORS.is_multiple_of(4),
        "the face's meridian no longer lands on a vertex column"
    );
    assert!(
        STACKS.is_multiple_of(2),
        "the face's equator no longer lands on a vertex ring"
    );
};

/// How wide a feature edge is ramped, in face units. Below about a texel this
/// starts to alias; well above it the face turns to fog.
const SOFT: f32 = 0.05;

/// The box the face lives in. Outside it a texel is the back of a head, and
/// the check is the first thing [`shade`] does — the head texture wraps a whole
/// sphere, so most of it is not face and should not be paying for one.
///
/// The margins are deliberately thin, which is only safe because
/// `nothing_on_the_face_is_clipped_by_the_early_bail` goes and measures how far
/// the outermost feature actually reaches at every mood.
const CHEEK_EDGE: f32 = 0.85;
const BROW_EDGE: f32 = 0.85;
const CHIN_EDGE: f32 = -0.7;

const EYE_X: f32 = 0.34;
const EYE_Y: f32 = 0.26;
const EYE_W: f32 = 0.15;
const EYE_H: f32 = 0.19;
const BROW_Y: f32 = 0.60;
const MOUTH_Y: f32 = -0.30;
/// How far the mouth bends at either extreme of mood. Well past where a
/// tasteful face would stop: these heads are 13 cm across and read from
/// half a street away, and at that distance tasteful is invisible.
const MOUTH_CURVE: f32 = 0.72;
/// Thickness of the mouth when it is shut, so that a neutral face has a line
/// rather than nothing.
const LIP_LINE: f32 = 0.055;

const FURIOUS: [f32; 3] = [0.90, 0.24, 0.14];
const EMOJI: [f32; 3] = [0.98, 0.76, 0.16];
const DELIGHTED: [f32; 3] = [1.00, 0.89, 0.28];
const EYE_INK: [f32; 3] = [0.10, 0.09, 0.11];
const BROW_INK: [f32; 3] = [0.26, 0.15, 0.07];
const MOUTH_DARK: [f32; 3] = [0.28, 0.10, 0.12];
const TOOTH: [f32; 3] = [0.97, 0.96, 0.93];
const BLUSH: [f32; 3] = [0.86, 0.15, 0.13];
const VEIN: [f32; 3] = [0.42, 0.02, 0.06];
const SWEAT: [f32; 3] = [0.60, 0.82, 0.96];
const TONGUE: [f32; 3] = [0.95, 0.44, 0.48];
const ROSY: [f32; 3] = [0.99, 0.55, 0.38];

// ------------------------------------------------------------- the paint ----

/// Coverage across a soft edge: 1 well inside, 0 well outside.
fn edge(inside: f32, soft: f32) -> f32 {
    smoothstep01(inside / soft + 0.5)
}

fn disc(p: Vec2, centre: Vec2, radius: f32, soft: f32) -> f32 {
    edge(radius - p.distance(centre), soft)
}

/// An axis-aligned ellipse. The distance is scaled back into face units after
/// the squash, so that `soft` means the same width of ramp whatever the aspect
/// — otherwise a narrowed eye would also blur.
fn ellipse(p: Vec2, centre: Vec2, half: Vec2, soft: f32) -> f32 {
    let half = half.max(Vec2::splat(1e-4));
    let offset = (p - centre) / half;
    edge((1.0 - offset.length()) * half.min_element(), soft)
}

fn capsule2d(p: Vec2, a: Vec2, b: Vec2, radius: f32, soft: f32) -> f32 {
    let along = b - a;
    let t = ((p - a).dot(along) / along.length_squared().max(1e-6)).clamp(0.0, 1.0);
    edge(radius - p.distance(a + along * t), soft)
}

/// A stroke following the graph of `f` from `x0` to `x1`.
///
/// Sampled as a short polyline rather than solved analytically: brows and
/// mouths are drawn from whatever curve reads best, and a closed-form distance
/// to an arbitrary one does not exist. Twelve segments is well past the point
/// the joints stop showing at this texture size.
fn stroke(p: Vec2, x0: f32, x1: f32, radius: f32, f: impl Fn(f32) -> f32) -> f32 {
    const STEPS: usize = 12;
    let mut cover: f32 = 0.0;
    let mut previous = Vec2::new(x0, f(x0));
    for step in 1..=STEPS {
        let x = x0 + (x1 - x0) * step as f32 / STEPS as f32;
        let next = Vec2::new(x, f(x));
        cover = cover.max(capsule2d(p, previous, next, radius, SOFT));
        previous = next;
    }
    cover
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn over(under: [f32; 3], colour: [f32; 3], alpha: f32) -> [f32; 3] {
    mix(under, colour, alpha)
}

/// The colour of the head itself: red-hot, emoji yellow, or brighter than that.
pub fn complexion_of(mood: f32) -> [f32; 3] {
    if mood < 0.0 {
        mix(EMOJI, FURIOUS, -mood)
    } else {
        mix(EMOJI, DELIGHTED, mood)
    }
}

/// Which way the mouth bends. Positive is a smile — corners above the middle —
/// and the sign changes at exactly neutral, which is what makes a flat line the
/// face of somebody with no opinion either way.
pub fn mouth_curvature(mood: f32) -> f32 {
    MOUTH_CURVE * mood.clamp(-1.0, 1.0)
}

/// The face, in face coordinates: the origin is between the eyes, +y is up, and
/// one unit is a quarter turn of the head.
pub fn shade(p: Vec2, mood: f32) -> [f32; 3] {
    let mood = mood.clamp(-1.0, 1.0);
    let mut colour = complexion_of(mood);
    if p.x.abs() > CHEEK_EDGE || p.y > BROW_EDGE || p.y < CHIN_EDGE {
        return colour;
    }
    let angry = (-mood).max(0.0);
    let happy = mood.max(0.0);
    let joy = smoothstep01((mood - 0.40) / 0.50);

    // Cheeks first, so everything else sits on top of the flush. The same
    // spots blush at both ends — hot with rage on the way down, rosy with
    // delight on the way up — because a face with nothing on its cheeks is a
    // face at rest, and neither extreme is anywhere near rest.
    for side in [-1.0f32, 1.0] {
        let reach = (1.0 - p.distance(Vec2::new(side * 0.52, -0.02)) / 0.26).clamp(0.0, 1.0);
        colour = over(colour, BLUSH, reach * reach * angry * 0.6);
        colour = over(colour, ROSY, reach * reach * joy * 0.45);
    }

    // The anger vein, and the sweat of somebody about to lose it entirely.
    //
    // Both sit well inside the face rather than out at the temples, and this is
    // the one place where being painted onto a sphere really bites: a feature
    // at 0.6 face units across and 0.7 up is 74° off the axis, which is the
    // limb of the head. It is there, it is simply edge-on and in shadow. The
    // vein goes between the brows, where an angry one belongs anyway, and it is
    // a much darker red than the furious complexion it has to show up against.
    let veined = smoothstep01((angry - 0.55) / 0.45);
    if veined > 0.0 {
        let centre = Vec2::new(0.0, 0.62);
        let mut mark: f32 = 0.0;
        for spoke in 0..3 {
            let angle = std::f32::consts::PI * spoke as f32 / 3.0;
            let arm = Vec2::new(angle.cos(), angle.sin()) * 0.12;
            mark = mark.max(capsule2d(p, centre - arm, centre + arm, 0.030, SOFT * 0.6));
        }
        colour = over(colour, VEIN, mark * veined);
    }
    let sweaty = smoothstep01((angry - 0.35) / 0.40);
    if sweaty > 0.0 {
        let bead = Vec2::new(0.60, 0.28);
        let drop =
            disc(p, bead, 0.09, SOFT).max(capsule2d(p, bead, bead + Vec2::Y * 0.15, 0.035, SOFT));
        colour = over(colour, SWEAT, drop * sweaty);
    }

    // The mouth: one lens-shaped opening whose curvature carries the mood and
    // whose height carries how strongly it is felt. A shut mouth is the same
    // shape with the opening closed down to the lip line, which is why a
    // neutral face gets a stroke rather than a gap in the geometry.
    let bend = mouth_curvature(mood);
    let width = 0.40 + 0.10 * happy - 0.02 * angry;
    let opening = 0.035 + 0.36 * mood.abs();
    // Mildly annoyed is its own expression: before the mouth commits to a
    // frown it goes wavy — the worried squiggle every cartoonist reaches for —
    // and the wave fades out again once real anger drags the corners down.
    let fret = smoothstep01(angry / 0.30) * (1.0 - smoothstep01((angry - 0.45) / 0.30));
    let squiggle = fret * 0.04 * (std::f32::consts::TAU * p.x / 0.22).sin();
    let lip = MOUTH_Y + bend * p.x * p.x + squiggle;
    let taper = (1.0 - (p.x / width).powi(2)).max(0.0).sqrt();
    let half = LIP_LINE * 0.5 + opening * 0.5 * taper;
    let mouth = edge(half - (p.y - lip).abs(), SOFT) * edge(width + LIP_LINE - p.x.abs(), SOFT);
    colour = over(colour, MOUTH_DARK, mouth);

    let bared = smoothstep01((angry - 0.30) / 0.50);
    if bared > 0.0 && mouth > 0.0 {
        // A row along the top of the opening, broken by gaps, which is as much
        // of a set of teeth as thirty texels can carry.
        let band = edge(p.y - (lip + half - opening * 0.5), SOFT * 0.6);
        let across = ((p.x + width) / (2.0 * width) * 7.0 + 100.0).fract();
        let gap = edge(0.40 - (across - 0.5).abs(), 0.08);
        colour = over(colour, TOOTH, mouth * band * gap * bared);
    }

    // A properly delighted mouth has a tongue lolling in the bottom of it.
    // Masked by the mouth's own coverage, so however the opening moves the
    // tongue can never escape the face.
    if joy > 0.0 && mouth > 0.0 {
        let loll = ellipse(
            p,
            Vec2::new(0.09, lip - half + opening * 0.18),
            Vec2::new(0.17, opening * 0.42),
            SOFT,
        );
        colour = over(colour, TONGUE, mouth * loll * joy);
    }

    // Eyes: a filled ellipse that narrows as the mood sours — to a furious
    // little bead, not just a slit, because rage concentrates — crossed over
    // to a pair of `^` arcs once it is properly delighted.
    for side in [-1.0f32, 1.0] {
        let local = p - Vec2::new(side * EYE_X, EYE_Y);
        let open = ellipse(
            local,
            Vec2::ZERO,
            Vec2::new(EYE_W * (1.0 - 0.45 * angry), EYE_H * (1.0 - 0.74 * angry)),
            SOFT,
        );
        let arc = stroke(local, -EYE_W, EYE_W, 0.055, |x| {
            EYE_H * 0.42 - (x.abs() / EYE_W) * EYE_H * 0.85
        });
        colour = over(colour, EYE_INK, open * (1.0 - joy) + arc * joy);
    }

    // Brows: the loudest feature on the face, and the one doing most of the
    // work. Anger drops the inner ends towards the nose; delight lifts the
    // whole thing and bows it. They thicken towards either extreme — a strong
    // feeling gets a heavier stroke, the way a cartoonist bears down on the
    // pen for the panel where somebody finally snaps.
    let heft = 0.055 + 0.025 * mood.abs();
    let tilt = angry * 0.22;
    // Kept modest on the happy side: the brow sits high on a sphere already,
    // and any more lift takes it over the crown, where it is foreshortened into
    // a smudge on top of the head rather than read as an expression.
    let lift = happy * 0.02 - angry * 0.05;
    let bow = happy * 0.04;
    for side in [-1.0f32, 1.0] {
        let inner = side * 0.15;
        let outer = side * 0.54;
        let at_nose = BROW_Y + lift - tilt;
        let at_temple = BROW_Y + lift + tilt * 0.25;
        let brow = stroke(p, inner, outer, heft, |x| {
            let along = ((x - inner) / (outer - inner)).clamp(0.0, 1.0);
            at_nose + (at_temple - at_nose) * along + bow * (along * std::f32::consts::PI).sin()
        });
        colour = over(colour, BROW_INK, brow);
    }

    colour
}

/// The mood at each baked level, evenly spaced across the whole range.
pub fn mood_at(level: usize) -> f32 {
    -1.0 + 2.0 * level.min(LEVELS - 1) as f32 / (LEVELS - 1) as f32
}

/// Which baked face a mood wears.
pub fn level_of(mood: f32) -> usize {
    let t = (mood.clamp(-1.0, 1.0) + 1.0) * 0.5;
    (t * (LEVELS - 1) as f32).round() as usize
}

// ------------------------------------------------------------- the assets ---

/// The head mesh: poles standing up on ±Y, face round at [`FACE_U`], divided
/// [`SECTORS`] by [`STACKS`].
///
/// Still a UV sphere and not an icosphere, whatever the triangle budget says:
/// the face is painted into these UVs, and an icosphere's seams run wherever
/// the subdivision put them — which would drag the eyes off the front of the
/// head and scatter the mouth across three charts.
pub fn head_mesh(radius: f32) -> Mesh {
    Sphere::new(radius)
        .mesh()
        .uv(SECTORS, STACKS)
        .rotated_by(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
}

/// The same face flat and cut out, for the HUD. Painted rather than cropped out
/// of the head texture, which at this mapping would be a 38-pixel window
/// blown up to twice its size.
fn portrait(mood: f32) -> Image {
    painted(PORTRAIT_SIZE, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let p = Vec2::new((u - 0.5) * 2.2, (0.5 - v) * 2.2);
        let colour = shade(p, mood);
        let head = disc(p, Vec2::ZERO, 1.0, 0.06);
        [
            byte(colour[0]),
            byte(colour[1]),
            byte(colour[2]),
            byte(head),
        ]
    })
}

/// The HUD's mood portrait, one image per level.
///
/// This held the emoji head and the matching hands as well — thirteen wrapped
/// 256² faces and thirteen flat complexions, the whole of the art direction
/// in which a flummi's mood *was* its colour. Nothing samples them any more:
/// every figure in the city is dressed through `figure::dress_person`, which
/// takes its head and its skin from `HumanFaces`, and the two vectors went on
/// being painted and uploaded at every startup for a year of nobody looking
/// at them. What is left is the one thing still on screen, which is the
/// symbol in the corner — and that one is *meant* to go yellow and glow red.
#[derive(Resource)]
pub struct FaceAssets {
    portraits: Vec<Handle<Image>>,
}

impl FaceAssets {
    pub fn portrait(&self, level: usize) -> Handle<Image> {
        self.portraits[level.min(LEVELS - 1)].clone()
    }
}

/// The baked face a figure is currently wearing, so that the swap only happens
/// when it actually changes rather than once a frame per citizen.
#[derive(Component, Debug)]
pub struct FaceLevel(pub usize);

/// Paints the HUD's mood portraits.
///
/// `materials` is still taken because the human faces are built alongside
/// these, from the same startup, and a caller that had to know which of the
/// two needed an `Assets<StandardMaterial>` would be a caller that knows more
/// about faces than it should.
pub fn build_assets(images: &mut Assets<Image>) -> FaceAssets {
    FaceAssets {
        portraits: (0..LEVELS)
            .map(|level| images.add(portrait(mood_at(level))))
            .collect(),
    }
}

/// Puts the right face on every head whose mood has moved a whole level.
pub fn wear_the_mood(
    human_assets: Res<crate::ai::figure::FigureAssets>,
    figures: Query<(
        &super::feeling::Mood,
        &mut FaceLevel,
        &Children,
        &crate::ai::appearance::Appearance,
    )>,
    mut parts: Query<(
        &mut MeshMaterial3d<StandardMaterial>,
        Option<&crate::ai::figure::Head>,
        Option<&crate::ai::figure::Bare>,
    )>,
    joints: Query<&Children>,
) {
    for (mood, mut level, children, appearance) in figures {
        let next = level_of(mood.value);
        if next == level.0 {
            continue;
        }
        level.0 = next;
        let face = human_assets
            .humans
            .face(appearance.skin, appearance.face, next);
        let bare = human_assets.humans.skin(appearance.skin);

        for &child in children {
            if let Ok((mut material, head, skin)) = parts.get_mut(child) {
                if head.is_some() {
                    material.0 = face.clone();
                } else if skin.is_some() {
                    material.0 = bare.clone();
                }
                continue;
            }
            // A hand hangs off the shoulder joint rather than off the body, so
            // that it swings with the arm — which puts it one level deeper than
            // everything else the mood has to reach. Only the parts marked
            // `Bare` are touched down here: the sleeve hanging off the same
            // joint is a coat and stays one.
            let Ok(limb) = joints.get(child) else {
                continue;
            };
            for &part in limb {
                if let Ok((mut material, _, skin)) = parts.get_mut(part)
                    && skin.is_some()
                {
                    material.0 = bare.clone();
                }
            }
        }
    }
}

// Human complexions stay put while expressions change. The symbolic HUD
// portrait keeps its exaggerated palette; the people in the street no longer
// turn yellow or glow red. This replaces the earlier all-emoji art direction.
const SKIN: [[f32; 3]; 6] = [
    [0.94, 0.77, 0.65],
    [0.82, 0.60, 0.44],
    [0.68, 0.45, 0.30],
    [0.52, 0.32, 0.21],
    [0.37, 0.22, 0.15],
    [0.24, 0.14, 0.10],
];

/// How wide a painted human face is.
///
/// The face is a quarter of the head's `u` and about half its `v`, so a 256²
/// sheet spends roughly 64 by 128 texels on the face itself and the rest on
/// plain complexion. That ratio is what sets the floor under how small a
/// feature can be and still be that feature: at the 128 this was painted at,
/// a pupil came out barely over one texel across and read as an aliased speck
/// in a white almond — a googly eye, not an eye.
const HUMAN_SIZE: u32 = 256;

/// Expressions painted per complexion and face, against `LEVELS` moods.
///
/// The thirteen-step ladder exists because an *emoji* changes colour
/// continuously and needs the steps or it bands. A human face keeps its
/// complexion and moves a brow, an eyelid and a mouth, and five steps of that
/// is finer than the eye separates at any distance a pedestrian is seen from.
/// Spending the difference on resolution instead is the trade: 6 x 3 x 5
/// sheets at 256² rather than 6 x 3 x 13 at 128² is fewer textures, a little
/// more memory, and four times the texels where they are actually looked at.
const HUMAN_STEPS: usize = 5;

/// The mood each painted step is drawn at: the middle of the band of `LEVELS`
/// it stands for, so neither end of the ladder is drawn at an extreme it only
/// half covers.
fn step_mood(step: usize) -> f32 {
    mood_at((step * 2 + 1) * LEVELS / (2 * HUMAN_STEPS))
}

/// And which step a mood level lands on.
fn step_of(level: usize) -> usize {
    level.min(LEVELS - 1) * HUMAN_STEPS / LEVELS
}

pub struct HumanFaces {
    faces: Vec<Handle<StandardMaterial>>,
    skin: Vec<Handle<StandardMaterial>>,
}

impl HumanFaces {
    pub fn face(&self, skin: usize, variant: usize, level: usize) -> Handle<StandardMaterial> {
        self.faces[((skin % SKIN.len()) * 3 + variant % 3) * HUMAN_STEPS + step_of(level)].clone()
    }

    pub fn skin(&self, skin: usize) -> Handle<StandardMaterial> {
        self.skin[skin % SKIN.len()].clone()
    }
}

fn human_shade(p: Vec2, mood: f32, tone: usize, variant: usize) -> [f32; 3] {
    let skin = SKIN[tone % SKIN.len()];
    let mut colour = skin;
    if p.x.abs() > 0.80 || p.y.abs() > 0.80 {
        return colour;
    }
    let mood = mood.clamp(-1.0, 1.0);
    // Sized for the distance a pedestrian is actually seen from, not for
    // anatomy. The face wraps a quarter of the head's `u`, so a citizen two
    // metres away is showing about sixty screen pixels of it — and features
    // at life proportions come out as a pair of dots and no mouth at all,
    // which is what the first pass shipped. These are pushed towards the
    // caricature the rest of the city is drawn at.
    let eye_space = 0.23 + variant as f32 * 0.025;
    let eye_height = 0.044 + 0.024 * (1.0 - mood.abs());
    let lip = mix(skin, [0.40, 0.13, 0.12], 0.74);
    for side in [-1.0, 1.0] {
        let eye = Vec2::new(side * eye_space, 0.16);
        let socket = ellipse(p, eye + Vec2::Y * 0.025, Vec2::new(0.14, 0.10), 0.035);
        colour = over(colour, mix(skin, [0.10, 0.06, 0.05], 0.35), socket * 0.32);
        colour = over(
            colour,
            [0.88, 0.86, 0.81],
            ellipse(p, eye, Vec2::new(0.115, eye_height), 0.012),
        );
        let iris = [[0.23, 0.14, 0.08], [0.22, 0.29, 0.24], [0.26, 0.32, 0.38]][variant % 3];
        let eye_mask = ellipse(p, eye, Vec2::new(0.111, eye_height), 0.012);
        // The iris is drawn *wider than the opening* and cropped back by it.
        // Drawn smaller it floats in a field of white with a gap above and
        // below, which is the one thing a human eye never does and a cartoon
        // one always does — the lids sit on the iris, top and bottom.
        colour = over(colour, iris, disc(p, eye, 0.062, 0.010) * eye_mask);
        colour = over(
            colour,
            [0.025, 0.02, 0.018],
            disc(p, eye, 0.028, 0.008) * eye_mask,
        );
        // Lash line. Without it the opening has no top and the eye reads as
        // painted on rather than set in.
        colour = over(
            colour,
            mix(skin, [0.08, 0.05, 0.05], 0.72),
            (ellipse(p, eye, Vec2::new(0.122, eye_height + 0.014), 0.008)
                - ellipse(p, eye, Vec2::new(0.115, eye_height), 0.010))
            .clamp(0.0, 1.0),
        );
        // Level at neutral, and the two halves of the range are not mirror
        // images of each other. A scowl drops the inner end towards the nose
        // and lifts the outer; pleasure raises the whole brow instead. Running
        // one signed term through both ends made a happy face inner-high,
        // which is not delight but pleading — and left the neutral face with
        // a permanent slight frown, because the two resting heights differed.
        let frown = (-mood).max(0.0);
        let lift = mood.max(0.0);
        let inner = Vec2::new(
            side * (eye_space - 0.110),
            0.288 - frown * 0.050 + lift * 0.022,
        );
        let outer = Vec2::new(
            side * (eye_space + 0.110),
            0.288 + frown * 0.026 + lift * 0.030,
        );
        colour = over(
            colour,
            BROW_INK,
            capsule2d(p, inner, outer, 0.026 + variant as f32 * 0.005, 0.012),
        );
        // A suggestion of cheek warmth, never a change of complexion.
        colour = over(
            colour,
            lip,
            ellipse(
                p,
                Vec2::new(side * 0.38, -0.10),
                Vec2::new(0.16, 0.12),
                0.05,
            ) * mood.abs()
                * 0.12,
        );
    }
    let mouth = stroke(p, -0.26, 0.26, 0.040, |x| -0.32 + mood * 1.4 * x * x);
    colour = over(colour, lip, mouth);
    // Nostrils and philtrum anchor the face under the modelled nose.
    for x in [-0.058, 0.058] {
        colour = over(
            colour,
            mix(skin, [0.1, 0.05, 0.04], 0.80),
            ellipse(p, Vec2::new(x, -0.115), Vec2::new(0.034, 0.020), 0.008),
        );
    }
    colour
}

pub fn build_humans(
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> HumanFaces {
    let mut result = HumanFaces {
        faces: Vec::new(),
        skin: Vec::new(),
    };
    // Shared atlas palette, bounded at startup: never allocate a new material
    // or image when a resident streams back in or their mood changes.
    for (tone, colour) in SKIN.iter().enumerate() {
        result.skin.push(materials.add(StandardMaterial {
            base_color: Color::srgb(colour[0], colour[1], colour[2]),
            perceptual_roughness: 0.62,
            reflectance: 0.35,
            ..default()
        }));
        for variant in 0..3 {
            for step in 0..HUMAN_STEPS {
                let texture = images.add(painted(
                    HUMAN_SIZE,
                    TextureFormat::Rgba8UnormSrgb,
                    |u, v| {
                        let around = (u - FACE_U + 1.5).fract() - 0.5;
                        let c = human_shade(
                            Vec2::new(around * 4.0, (0.5 - v) * 2.0),
                            step_mood(step),
                            tone,
                            variant,
                        );
                        [byte(c[0]), byte(c[1]), byte(c[2]), 255]
                    },
                ));
                result.faces.push(materials.add(StandardMaterial {
                    base_color_texture: Some(texture),
                    perceptual_roughness: 0.62,
                    reflectance: 0.35,
                    ..default()
                }));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mood_level_lands_on_a_painted_expression() {
        // The painted ladder is shorter than the mood ladder, so the mapping
        // between them is the one place an off-by-one indexes past the end of
        // the atlas — or, worse, quietly clamps the whole furious end onto one
        // step and leaves the city unable to scowl.
        let steps: Vec<_> = (0..LEVELS).map(step_of).collect();
        assert_eq!(*steps.first().unwrap(), 0);
        assert_eq!(*steps.last().unwrap(), HUMAN_STEPS - 1);
        assert!(steps.windows(2).all(|w| w[1] >= w[0]), "{steps:?}");
        assert!(
            (0..HUMAN_STEPS).all(|s| steps.contains(&s)),
            "a painted expression nothing wears: {steps:?}"
        );
        // And the mood each step is drawn at rises with it, from a scowl to
        // a grin, without either end falling off the range.
        let moods: Vec<_> = (0..HUMAN_STEPS).map(step_mood).collect();
        assert!(moods.windows(2).all(|w| w[1] > w[0]), "{moods:?}");
        assert!(moods[0] < -0.5 && *moods.last().unwrap() > 0.5, "{moods:?}");
    }

    #[test]
    fn human_complexions_do_not_change_with_mood() {
        for (skin, complexion) in SKIN.iter().enumerate() {
            for mood in [-1.0, 0.0, 1.0] {
                assert_eq!(human_shade(Vec2::new(1.2, 0.0), mood, skin, 0), *complexion);
            }
        }
    }

    /// Mean of `channel` minus the mean green, over the whole face. A crude
    /// reading, which is the point: it cannot be satisfied by one red pixel.
    fn redness(mood: f32) -> f32 {
        let mut sum = 0.0;
        let mut count = 0.0;
        for y in 0..48 {
            for x in 0..48 {
                let p = Vec2::new(x as f32 / 24.0 - 1.0, 1.0 - y as f32 / 24.0);
                let colour = shade(p, mood);
                sum += colour[0] - colour[1];
                count += 1.0;
            }
        }
        sum / count
    }

    #[test]
    fn an_angry_face_has_more_red_in_it_than_a_happy_one() {
        assert!(
            redness(-1.0) > redness(1.0) + 0.2,
            "furious read {:.3} against delighted {:.3}",
            redness(-1.0),
            redness(1.0)
        );
    }

    #[test]
    fn the_complexion_sours_all_the_way_down_rather_than_at_the_end() {
        // A face that only turns red in the last tenth is a face that spends
        // the whole game looking indifferent.
        let steps: Vec<f32> = (0..LEVELS).map(|l| redness(mood_at(l))).collect();
        for pair in steps.windows(2) {
            assert!(
                pair[0] > pair[1],
                "the ladder went the wrong way: {:?}",
                steps
            );
        }
    }

    #[test]
    fn a_furious_face_sweats_and_has_a_vein_showing() {
        // Both are drawn in a red that is close to the complexion they sit on,
        // so the check is that they change the texel at all — and that they
        // stay off a face that has nothing to be furious about.
        let vein = Vec2::new(0.0, 0.62);
        let bead = Vec2::new(0.60, 0.28);
        assert_ne!(
            shade(vein, -1.0),
            complexion_of(-1.0),
            "no vein at full fury"
        );
        assert_ne!(
            shade(bead, -1.0),
            complexion_of(-1.0),
            "no sweat at full fury"
        );
        assert_eq!(
            shade(vein, 0.5),
            complexion_of(0.5),
            "a cheerful flummi had a vein"
        );
        assert_eq!(
            shade(bead, 0.5),
            complexion_of(0.5),
            "and was sweating about it"
        );
    }

    #[test]
    fn the_mouth_curvature_changes_sign_at_neutral_mood() {
        assert!(mouth_curvature(-1.0) < 0.0, "a frown must open downwards");
        assert!(mouth_curvature(1.0) > 0.0, "and a grin upwards");
        assert_eq!(mouth_curvature(0.0), 0.0);
    }

    #[test]
    fn the_eyes_narrow_as_the_mood_sours() {
        // Sampled a little above the eye's centre, where a wide eye is dark and
        // a slit is not.
        let above_the_pupil = Vec2::new(EYE_X, EYE_Y + EYE_H * 0.6);
        let darkness = |mood| 1.0 - shade(above_the_pupil, mood)[0];
        assert!(
            darkness(0.0) > darkness(-1.0),
            "a furious flummi's eyes were as wide open as an indifferent one's"
        );
    }

    #[test]
    fn there_is_a_face_for_every_mood_and_a_mood_for_every_face() {
        for level in 0..LEVELS {
            assert_eq!(level_of(mood_at(level)), level);
        }
        assert_eq!(level_of(-1.0), 0);
        assert_eq!(level_of(1.0), LEVELS - 1);
        assert_eq!(level_of(0.0), LEVELS / 2);
        // Nothing outside the range can index past the end of the table.
        assert_eq!(level_of(-40.0), 0);
        assert_eq!(level_of(40.0), LEVELS - 1);
    }

    #[test]
    fn nothing_on_the_face_is_clipped_by_the_early_bail() {
        // A feature that reached the edge of the box would be cut off with a
        // hard straight line — the one failure mode of bailing early, and one
        // that no amount of squinting at a 13 cm head would catch. So the test
        // goes and finds the outermost texel that is not plain complexion.
        for level in 0..LEVELS {
            let mood = mood_at(level);
            let plain = complexion_of(mood);
            let (mut wide, mut high, mut low) = (0.0f32, -2.0f32, 2.0f32);
            for iy in -80..=80 {
                for ix in -80..=80 {
                    let p = Vec2::new(ix as f32 / 80.0, iy as f32 / 80.0);
                    if shade(p, mood) == plain {
                        continue;
                    }
                    wide = wide.max(p.x.abs());
                    high = high.max(p.y);
                    low = low.min(p.y);
                }
            }
            assert!(
                wide < CHEEK_EDGE - 0.02,
                "at mood {mood:.2} a feature reaches {wide:.2} across, against a cheek edge at {CHEEK_EDGE}"
            );
            assert!(
                high < BROW_EDGE - 0.02,
                "at mood {mood:.2} a feature reaches {high:.2} up, against a brow edge at {BROW_EDGE}"
            );
            assert!(
                low > CHIN_EDGE + 0.02,
                "at mood {mood:.2} a feature reaches {low:.2} down, against a chin edge at {CHIN_EDGE}"
            );
        }
    }

    #[test]
    fn the_back_of_the_head_is_plain() {
        // Half a turn from the face, every mood must be flat complexion — a
        // feature that wrapped round the back would appear as a second, upside
        // down face on the far side of the seam.
        for level in 0..LEVELS {
            let mood = mood_at(level);
            for y in -8..=8 {
                let p = Vec2::new(2.0, y as f32 / 8.0);
                assert_eq!(shade(p, mood), complexion_of(mood));
            }
        }
    }

    #[test]
    fn the_face_is_the_same_on_both_sides_of_the_seam() {
        // The head texture wraps, so u = 0 and u = 1 are the same column of
        // texels. If the face's own coordinates did not wrap with it there
        // would be a visible join down the back of every head.
        let around = |u: f32| (u - FACE_U + 1.5).fract() - 0.5;
        assert!((around(0.0) - around(1.0)).abs() < 1e-5);
    }
}
