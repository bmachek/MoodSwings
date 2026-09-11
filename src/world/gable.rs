//! The screen wall in front of a Landshut roof, and the roof behind it.
//!
//! Every building in this city is a box with a slab on top, and for a city of
//! nowhere in particular that is the honest answer. It is the wrong answer for
//! exactly one postcard: what anybody who has stood in the Landshut Altstadt
//! remembers is not a street plan and not a colour, it is *the roofline* — a
//! row of tall narrow houses whose front walls carry on upwards past the roof,
//! each one a different height, each one hiding whatever is behind it.
//!
//! ## What the thing actually is
//!
//! It is a `Vorschussmauer`, and Landshut's Altstadt is the northern end of
//! the *Inn-Salzach* building style it belongs to. The wall is pulled up past
//! the eaves as a fire screen — the region burnt down often enough that a
//! masonry wall between one roof and the next was worth building, and it also
//! gave the ladders something safe to stand against. Behind it sits a steep
//! `Grabendach` that falls *inwards* and drains through spouts in the screen,
//! so from the street there is no roof at all: the house reads as a flat,
//! nearly rectangular front, and a row of them reads as one canyon wall.
//!
//! The top of the screen comes in three forms and this module builds all
//! three — see [`Screen`]. Only one of them is the stepped gable everybody
//! thinks of first; the commonest is a plain horizontal top with a cornice
//! under it, and the one that makes a skyline is the curved `Schweifgiebel`.
//!
//! ## Why the screen and not the roof
//!
//! The roof behind it is a plain pitch, and it is there almost entirely for
//! the view from above. From a pavement you never see it: the whole point of a
//! screen is that it stands proud of the roof and hides it, which is why the
//! things were built. So the screen gets the arithmetic here, and the pitch
//! is `world::roof`'s — the same roof every unscreened house in the town
//! wears, stopped short of the front wall so nothing of it comes through the
//! screen.
//!
//! ## What a screen is not
//!
//! A board. The first pass built it as one — a plain slab of the wall colour
//! standing up to twice the height of the house, with no windows, and the
//! roof's verge poking through its face — and from the pavement a row of
//! them read as stage flats. A real screen is the *wall continued*: the same
//! render, an attic storey of small windows with pale surrounds, a cornice at
//! the eaves line, and it stands one storey to one and a half above the
//! eaves, never more. All of that is here now, and the height is held to the
//! storey rather than to the frontage.
//!
//! ## What a gabled building gives up
//!
//! Its rooftop clutter. Air handling, tanks and vent stacks sit on a flat deck
//! and a pitched roof does not have one, so a gabled building would have its
//! plant floating inside its own rafters. `buildings` skips the clutter for
//! them, which is also correct: these are houses, and a house's roof has a
//! chimney on it and nothing else.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::{ChaCha8Rng, rand_core::SeedableRng};

use super::buildings::ChunkOf;
use super::roof;

/// How far the screen itself is drawn. As far as the building it caps: a
/// roofline is a silhouette, and a silhouette is the last thing to stop
/// mattering.
pub const RANGE: f32 = roof::RANGE;

/// And how far the *coping* is — the stone slab across the top of every step.
///
/// Half the meshes in a screen are coping, and coping is a hundred and twenty
/// millimetres thick. Past a couple of hundred metres it is under a pixel and
/// all it contributes is a lighter edge on a skyline that already has one, so
/// on a city where four buildings in five are gabled it is the obvious half to
/// drop first. The same distance the roof's own trim is dropped at, for the
/// same reason.
const COPING_RANGE: f32 = roof::TRIM_RANGE;

/// Steps up one side of a screen. Four is a gable, seven is a wedding cake.
const STEPS: (u32, u32) = (3, 6);

/// The cornice at the eaves: how deep it is and how far it oversails.
///
/// Shallow, and it is the most valuable twelve centimetres on the building. A
/// screen wall with no cornice under it is simply a taller wall, and a row of
/// taller walls is exactly what "the buildings look like boxes" means.
const CORNICE: f32 = 0.26;
const CORNICE_OUT: f32 = 0.30;

/// How far the drainage spout stands out of the screen.
const SPOUT: f32 = 0.55;

/// The narrowest a screen's top band may be, against the frontage.
///
/// Not zero. A stepped gable's last step is the pier the flagpole goes on and
/// it has width; a band of nothing is a band that vanishes and leaves the
/// coping floating.
const MIN_PIER: f32 = 0.14;

/// How high the screen stands above the eaves, as a fraction of the frontage.
///
/// Measured off the *width* and not off the building's height, which is what
/// makes a row of them read as one street rather than as a stack of unrelated
/// triangles: a narrow house gets a steep gable and a wide one gets a shallow
/// gable, exactly as they do in life, because both were built to the same
/// pitch. Then held to [`SCREEN_STOREYS`], which is what stops a wide house
/// growing a screen taller than itself.
const RISE: (f32, f32) = (0.55, 0.85);

/// How far above the eaves a screen may stand, in storeys of the house under
/// it.
///
/// Between one and one and a half in life — the screen hides a roof, and the
/// roof is one attic storey tall — and the first pass let it run to the
/// frontage's share with only the height as a backstop, which on a wide
/// Lowrise plot was twice the height of the house. The user circled that:
/// from the pavement a screen that tall reads as a billboard standing on a
/// house, not as a house.
const SCREEN_STOREYS: (f32, f32) = (1.15, 1.6);

/// The attic windows in the screen: how wide and how tall each pane is, how
/// far up the screen the row sits, and the surround round each.
///
/// They are what make the screen read as a storey. A `Vorschussmauer` is the
/// wall carried on, and the wall has windows in it — small ones, because the
/// attic behind is low — with the same pale surround every other window on the
/// front has.
const PANE: (f32, f32) = (0.84, 1.02);
const PANE_UP: f32 = 0.33;
const SURROUND: f32 = 0.16;

/// How far the screen stands proud of the wall below it.
const PROUD: f32 = 0.10;
/// And how thick it is.
const THICK: f32 = 0.32;

/// The one kit every roof in the town draws from: the screen's own pieces
/// and, because the screen and the suburb wear the same tiles, everything
/// `world::roof` is made of too.
#[derive(Resource)]
pub struct GableKit {
    pub(super) cube: Handle<Mesh>,
    /// A unit cylinder lying on its own Y. Scaled into the gutter along the
    /// eaves and the capping along the ridge — both are half-rounds in life,
    /// and a box reads as neither.
    pub(super) round: Handle<Mesh>,
    /// The roof itself: two slopes, the wall triangles that close a gable,
    /// and the half-pyramid that closes a hip — see `roof::prism`.
    pub(super) prism: Handle<Mesh>,
    pub(super) ends: Handle<Mesh>,
    pub(super) hip: Handle<Mesh>,
    /// Zinc: the gutter, and the flashing round a dormer.
    pub(super) metal: Handle<StandardMaterial>,
    /// What a dormer's cheeks are rendered in, and the dark of its glazing.
    pub(super) dormer: Handle<StandardMaterial>,
    pub(super) glass: Handle<StandardMaterial>,
    /// The screen wears the building's own wall material, which this module
    /// does not own — so what is kept here is only what a roof is made of.
    ///
    /// Three ages of clay at every tiling `roof::bucket` can ask for, indexed
    /// `bucket * roof::AGES + age`. The age is picked from the building's own
    /// seed, so a roof keeps it across a chunk respawn; the tiling is picked
    /// from the roof's size, so a tile stays a hand across on a shed and on
    /// a block.
    pub(super) tile: Vec<Handle<StandardMaterial>>,
    pub(super) cap: Handle<StandardMaterial>,
}

impl GableKit {
    /// The clay for a leaf `up` metres up the slope and `along` metres along
    /// the ridge, at one of the three ages.
    pub fn tile(&self, age: usize, up: f32, along: f32) -> Handle<StandardMaterial> {
        let i = roof::bucket(up, along) * roof::AGES + age.min(roof::AGES - 1);
        self.tile[i.min(self.tile.len() - 1)].clone()
    }
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> GableKit {
    GableKit {
        // With tangents, and that is not decoration: Bevy applies a normal map
        // only where the mesh carries a tangent basis, so the roof relief that
        // has been painted and bound since this module was written was being
        // dropped on the floor. A bare `Cuboid` has none. The city's own
        // `with_tangents` runs mikktspace, which is the basis the shader
        // agrees with.
        cube: meshes.add(super::buildings::with_tangents(
            Cuboid::new(1.0, 1.0, 1.0).mesh().build(),
        )),
        // Eight sides rather than a smooth tube. A gutter is sixty-five
        // millimetres of radius seen from twenty metres, and every extra
        // segment is a segment on every eaves of every house in the town.
        round: meshes.add(super::buildings::with_tangents(
            Cylinder::new(1.0, 1.0).mesh().resolution(8).build(),
        )),
        prism: meshes.add(super::buildings::with_tangents(roof::prism())),
        ends: meshes.add(super::buildings::with_tangents(roof::gable_ends())),
        hip: meshes.add(super::buildings::with_tangents(roof::hip_end())),
        metal: materials.add(StandardMaterial {
            base_color: Color::srgb(0.52, 0.53, 0.55),
            perceptual_roughness: 0.42,
            metallic: 0.85,
            ..default()
        }),
        dormer: materials.add(StandardMaterial {
            // Rendered, and paler than any wall in the palette: a dormer cheek
            // catches the sky and is the brightest thing on a roof.
            base_color: Color::srgb(0.74, 0.72, 0.68),
            perceptual_roughness: 0.9,
            ..default()
        }),
        glass: materials.add(StandardMaterial {
            base_color: Color::srgb(0.06, 0.07, 0.09),
            perceptual_roughness: 0.12,
            metallic: 0.0,
            reflectance: 0.6,
            ..default()
        }),
        tile: roof::build_tiles(materials, images),
        cap: materials.add(StandardMaterial {
            // The stone coping along the top of every step, which is what
            // stops the steps reading as a staircase made of the wall.
            base_color: Color::srgb(0.72, 0.70, 0.66),
            perceptual_roughness: 0.88,
            ..default()
        }),
    }
}

/// What the top of a screen wall does.
///
/// Three forms, and the first pass at this module built only the second one —
/// which is the one everybody pictures and is not the one most of the street
/// is. A row where every house wears the same stepped gable reads as a stage
/// set; the variety is most of what makes a real skyline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    /// A flat top with a cornice under it. The plainest and the commonest: an
    /// Inn-Salzach front is described as a "nearly rectangular front surface",
    /// and this is that.
    Straight,
    /// `Treppengiebel`: steps up to a pier in the middle.
    Stepped,
    /// `Schweifgiebel`: an ogee, shouldered low and drawn to a point. The one
    /// that makes a skyline, and geometrically the same loop as the steps with
    /// a curve where the staircase was.
    Curved,
}

impl Screen {
    /// How many bands the screen is built out of.
    ///
    /// A stepped gable *is* its bands and wants few of them; a curve is a
    /// curve and wants enough that the eye stops counting. A straight top is
    /// one band and a coping.
    fn bands(self, steps: u32) -> u32 {
        match self {
            Screen::Straight => 1,
            Screen::Stepped => steps,
            Screen::Curved => CURVE_BANDS,
        }
    }

    /// Half-width of the screen at a height fraction, 0 at the eaves and 1 at
    /// the top, as a fraction of the frontage.
    ///
    /// The whole difference between the three forms is this function. Written
    /// as a profile rather than as three loops so the failure that made the
    /// first version look wrong rather than broken — a screen whose bands do
    /// not reach the middle leaves a flat top, and one that overshoots grows a
    /// notch at its own apex — is one thing to test rather than three.
    fn across(self, up: f32) -> f32 {
        match self {
            // Full width all the way, and then the coping caps it.
            Screen::Straight => 1.0,
            // A staircase: linear.
            Screen::Stepped => 1.0 - up,
            // An ogee. Full width at the shoulder, then falling away with a
            // slack S rather than a straight line — which is the whole of what
            // separates a scrolled gable from a triangle.
            Screen::Curved => {
                let t = (up - CURVE_SHOULDER).max(0.0) / (1.0 - CURVE_SHOULDER);
                1.0 - t * t * (3.0 - 2.0 * t)
            }
        }
    }
}

/// How many bands a curve is drawn with, and how far up its shoulder sits.
///
/// Fourteen is where the steps stop being countable at the distance a roofline
/// is looked at. The shoulder is what makes it an ogee: the wall carries on at
/// full width for the first third and only then starts to fall away.
const CURVE_BANDS: u32 = 14;
const CURVE_SHOULDER: f32 = 0.34;

/// How often each form is built.
///
/// Straight is the commonest, which is the thing the first pass had exactly
/// backwards. The curve is rare enough to be worth seeing.
fn draw_screen(rng: &mut ChaCha8Rng) -> Screen {
    match rng.random_range(0.0..1.0) {
        roll if roll < 0.52 => Screen::Straight,
        roll if roll < 0.83 => Screen::Stepped,
        _ => Screen::Curved,
    }
}

/// Raises a stepped gable on one building's front, and pitches its roof.
///
/// Returns the roof behind the screen, which is the one thing about this
/// building that anybody outside the module needs and cannot work out: the
/// screen's rise is drawn from the building's own seed here, and the roof is
/// held under it, so recomputing either anywhere else would mean replaying
/// the same draws in the same order for ever. `world::plume` wants it to
/// stand a chimney on the tiles rather than inside them.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    kit: &GableKit,
    wall: &Handle<StandardMaterial>,
    seed: u64,
    center: Vec2,
    frontage: f32,
    throat: f32,
    // `height` is how tall the wall is and `eaves` is how far above the ground
    // its top sits; the two differ by the kerb, and the clamp below wants the
    // first. `storey` is what one floor of the facade measures, which is what
    // the screen's height is held to.
    height: f32,
    storey: f32,
    eaves: f32,
    yaw: f32,
    chunk: IVec2,
    lod_scale: f32,
) -> roof::Pitch {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x6AB1_E501);
    let steps = rng.random_range(STEPS.0..=STEPS.1);
    let screen = draw_screen(&mut rng);
    // Off the frontage, and then held to the house — to its storeys first,
    // and to its height as a backstop. The width is what sets the pitch and
    // the pitch is what makes a row read as one street; but a generator that
    // hands out lots wider than its buildings are tall will otherwise put a
    // ten-metre screen on a five-metre house, which is not an Altstadt, it is
    // a row of billboards. `SCREEN_STOREYS` is the clamp that actually bites.
    let rise = (frontage * rng.random_range(RISE.0..RISE.1))
        .clamp(storey * SCREEN_STOREYS.0, storey * SCREEN_STOREYS.1)
        .min(height * 0.62);

    let end = (RANGE * lod_scale).min(2_000.0);
    let range = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: end..(end * 1.05),
        use_aabb: false,
    };
    let near = (COPING_RANGE * lod_scale).min(600.0);
    let close = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: near..(near * 1.1),
        use_aabb: false,
    };
    let turn = Quat::from_rotation_y(yaw);
    // The screen's own frame: `outward` is the way the front face looks, and
    // the steps march up the middle from both sides at once.
    let outward = Vec2::new(yaw.sin(), yaw.cos());
    let front = center + outward * (throat * 0.5 + PROUD - THICK * 0.5);

    // The cornice at the eaves, under the screen. Every one of the three forms
    // has one and it is the line that separates the wall from what is standing
    // on top of it — without it a straight screen is simply a taller wall, and
    // a taller wall is what "the buildings look like boxes" means.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.cube.clone()),
        MeshMaterial3d(kit.cap.clone()),
        Transform::from_xyz(front.x, eaves + CORNICE * 0.5, front.y)
            .with_rotation(turn)
            .with_scale(Vec3::new(
                frontage + CORNICE_OUT,
                CORNICE,
                THICK + CORNICE_OUT,
            )),
        close.clone(),
    ));

    let bands = screen.bands(steps);
    for index in 0..bands {
        let up = (index + 1) as f32 / bands as f32;
        let was = index as f32 / bands as f32;
        // The band is as wide as the *bottom* of its own slice, so a curve
        // comes out as a staircase whose treads follow it rather than as one
        // that cuts the corner off every turn.
        let width = frontage * screen.across(was).max(MIN_PIER);
        let top = eaves + rise * up;
        let bottom = eaves + rise * was;
        let band = top - bottom;

        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(wall.clone()),
            Transform::from_xyz(front.x, bottom + band * 0.5, front.y)
                .with_rotation(turn)
                .with_scale(Vec3::new(width, band, THICK)),
            range.clone(),
        ));
        // The coping: a slab of stone across the top of each band, oversailing
        // it a little. Without it a stepped screen is a wall with notches cut
        // in it, and with it it is a gable. On a curve every band would be a
        // ladder of ledges, so only the last one is capped — the rest of the
        // curve is the wall itself, which is what a rendered `Schweifgiebel`
        // is.
        let capped = match screen {
            Screen::Curved => index + 1 == bands,
            _ => true,
        };
        if capped {
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(kit.cap.clone()),
                Transform::from_xyz(front.x, top + 0.06, front.y)
                    .with_rotation(turn)
                    .with_scale(Vec3::new(width + 0.22, 0.12, THICK + 0.16)),
                close.clone(),
            ));
        }
    }

    // The attic windows: a row of two or three small panes a third of the way
    // up the screen, each in a pale surround, and each standing a little proud
    // of the face so it is a window in a wall rather than a decal on one.
    // This is what turns the screen from a board into a storey. Nothing is
    // drawn for them — their count and their spacing come off the frontage —
    // so the screen, the oriel and the roof keep the draws they had.
    {
        let panes = if frontage < 9.0 { 2 } else { 3 };
        let middle = eaves + rise * PANE_UP;
        // Inside the screen's own profile at the height of the row's top: a
        // stepped gable is already narrowing there, and a pane hanging out
        // past the step is a pane in mid-air.
        let top_up = ((rise * PANE_UP + PANE.1 * 0.5) / rise).min(1.0);
        let reach = (frontage * screen.across(top_up) * 0.5 - PANE.0 * 0.5 - SURROUND).max(0.0);
        let spacing = (frontage * 0.24).min(reach / ((panes - 1) as f32 * 0.5).max(1.0));
        for index in 0..panes {
            let along = Vec2::new(outward.y, -outward.x)
                * ((index as f32 - (panes - 1) as f32 * 0.5) * spacing);
            let surround = front + along + outward * (THICK * 0.5 + 0.015);
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(kit.cap.clone()),
                Transform::from_xyz(surround.x, middle, surround.y)
                    .with_rotation(turn)
                    .with_scale(Vec3::new(PANE.0 + SURROUND, PANE.1 + SURROUND, 0.03)),
                close.clone(),
            ));
            let glass = front + along + outward * (THICK * 0.5 + 0.045);
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(kit.glass.clone()),
                Transform::from_xyz(glass.x, middle, glass.y)
                    .with_rotation(turn)
                    .with_scale(Vec3::new(PANE.0, PANE.1, 0.03)),
                close.clone(),
            ));
        }
    }

    // The oriel, on the houses that have one.
    //
    // An `Erker` is the other half of what an Inn-Salzach front is: a flat,
    // nearly rectangular wall with one thing standing out of it. It is worth
    // more to the silhouette of a street than anything else on the facade,
    // because it is the only part of the building that is not in the plane of
    // the building — a row of houses with one is a row of houses, and a row
    // without is a row of walls.
    if rng.random_range(0.0..1.0) < ORIEL && frontage > ORIEL_MIN_FRONT {
        oriel(
            commands, kit, wall, &mut rng, front, outward, turn, frontage, eaves, height, chunk,
            &range, &close,
        );
    }

    // The drain. A `Grabendach` falls inwards and lets the water out through
    // the screen, and the spout is the one thing on an Inn-Salzach front that
    // says out loud there is a roof behind it.
    for side in [-1.0f32, 1.0] {
        let across = Vec2::new(outward.y, -outward.x) * (side * frontage * 0.36);
        let at = front + across;
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.round.clone()),
            MeshMaterial3d(kit.metal.clone()),
            Transform::from_xyz(at.x, eaves + rise * 0.22, at.y)
                .with_rotation(turn * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::new(0.075, SPOUT, 0.075)),
            close.clone(),
        ));
    }

    // And the roof behind it. The screen hides most of it from a pavement —
    // that is what a screen is for — but the flanks and the whole of every
    // aerial are this, and until it grew an overhang it was two lids.
    //
    // It is the same roof every unscreened house in the town wears, built by
    // `world::roof`, with two differences a screen forces on it: it stops
    // inside the screen's back face rather than oversailing the front wall
    // (the verge used to come through the screen and draw a triangle of tile
    // edge on its face — the user circled exactly that), and its ridge is held
    // under the screen's top. It is placed off the same centre and yaw the
    // screen is, so the two cannot drift apart on a corner house.
    //
    // When this roof was last done. From the building's own seed like
    // everything else about it, and drawn *after* the oriel so the draw order
    // is what it always was.
    let age = rng.random_range(0..roof::AGES);
    let pitch = roof::Pitch::screened(frontage, throat, height, rise, THICK - PROUD + 0.01);
    roof::spawn(
        commands, kit, wall, seed, age, center, yaw, eaves, &pitch, chunk, lod_scale,
    );
    pitch
}

/// How often a house carries an oriel, and the narrowest frontage that can.
///
/// Not every house: an `Erker` was a thing you paid for, and a street where
/// every front has one reads as a pattern rather than as a town. The width
/// floor is because a bay a third of a four-metre frontage is a bay a metre
/// across, which from a pavement is a pipe.
const ORIEL: f32 = 0.34;
const ORIEL_MIN_FRONT: f32 = 7.0;

/// Its proportions: how much of the frontage it takes, how far it stands out,
/// and which band of the wall it occupies.
const ORIEL_WIDE: (f32, f32) = (0.34, 0.52);
const ORIEL_OUT: (f32, f32) = (0.55, 0.95);
// Two storeys of a four-storey house. The first pass ran to 0.86 and the bay
// came out taller than it was wide by three to one, which is not an oriel, it
// is a lift shaft bolted to the front.
const ORIEL_BAND: (f32, f32) = (0.36, 0.76);

/// A bay window standing out of the front wall.
///
/// Five boxes: the corbel it sits on, the body, glazing on the front and both
/// returns, and the little roof over it. Glazed on three faces, because a bay
/// that is only glazed at the front is a cupboard — the whole point of the
/// thing is that you can see up and down the street from inside it.
#[allow(clippy::too_many_arguments)]
fn oriel(
    commands: &mut Commands,
    kit: &GableKit,
    wall: &Handle<StandardMaterial>,
    rng: &mut ChaCha8Rng,
    front: Vec2,
    outward: Vec2,
    turn: Quat,
    frontage: f32,
    eaves: f32,
    height: f32,
    chunk: IVec2,
    range: &VisibilityRange,
    close: &VisibilityRange,
) {
    let wide = frontage * rng.random_range(ORIEL_WIDE.0..ORIEL_WIDE.1);
    let out = rng.random_range(ORIEL_OUT.0..ORIEL_OUT.1);
    // Off centre as often as not: a bay is on the room it belongs to.
    let along =
        Vec2::new(outward.y, -outward.x) * (rng.random_range(-0.22..0.22) * (frontage - wide));
    let base = eaves - height * (1.0 - ORIEL_BAND.0);
    let top = eaves - height * (1.0 - ORIEL_BAND.1);
    let tall = top - base;
    if tall < 2.0 {
        return;
    }
    let middle = front + along + outward * (out * 0.5);

    // The corbel: a wedge under the bay, narrower than it and shallower, so
    // the bay reads as carried rather than as stuck on.
    let corbel = front + along + outward * (out * 0.32);
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.cube.clone()),
        MeshMaterial3d(kit.cap.clone()),
        Transform::from_xyz(corbel.x, base - CORBEL * 0.5, corbel.y)
            .with_rotation(turn)
            .with_scale(Vec3::new(wide * 0.86, CORBEL, out * 0.64)),
        close.clone(),
    ));

    // The body.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.cube.clone()),
        MeshMaterial3d(wall.clone()),
        Transform::from_xyz(middle.x, base + tall * 0.5, middle.y)
            .with_rotation(turn)
            .with_scale(Vec3::new(wide, tall, out)),
        range.clone(),
    ));

    // Glazing on the front and the two returns, standing a little proud so it
    // is a window in a wall rather than a decal on one.
    let glass = front + along + outward * (out + 0.02);
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.cube.clone()),
        MeshMaterial3d(kit.glass.clone()),
        Transform::from_xyz(glass.x, base + tall * 0.52, glass.y)
            .with_rotation(turn)
            .with_scale(Vec3::new(wide * 0.7, tall * 0.62, 0.05)),
        close.clone(),
    ));
    for side in [-1.0f32, 1.0] {
        let cheek = middle + Vec2::new(outward.y, -outward.x) * (side * (wide * 0.5 + 0.02));
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.glass.clone()),
            Transform::from_xyz(cheek.x, base + tall * 0.52, cheek.y)
                .with_rotation(turn)
                .with_scale(Vec3::new(0.05, tall * 0.62, out * 0.66)),
            close.clone(),
        ));
    }

    // And the little roof over it, oversailing on three sides.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.cube.clone()),
        MeshMaterial3d(kit.metal.clone()),
        Transform::from_xyz(middle.x, top + ORIEL_CAP * 0.5, middle.y)
            .with_rotation(turn)
            .with_scale(Vec3::new(wide + 0.26, ORIEL_CAP, out + 0.20)),
        close.clone(),
    ));
}

/// How deep the corbel under an oriel is, and the lid over it.
const CORBEL: f32 = 0.34;
const ORIEL_CAP: f32 = 0.16;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every screen starts at the width of the wall and never gets wider.
    ///
    /// The two failures this guards against are the ones that make a screen
    /// look wrong rather than broken: a profile that does not start at the
    /// wall leaves a ledge at the eaves, and one that widens anywhere grows a
    /// shoulder halfway up.
    #[test]
    fn a_screen_starts_at_the_wall_and_only_narrows() {
        for screen in [Screen::Straight, Screen::Stepped, Screen::Curved] {
            assert!(
                (screen.across(0.0) - 1.0).abs() < 1e-6,
                "{screen:?} does not start at the width of the wall"
            );
            let mut previous = 1.0;
            for step in 0..=40 {
                let up = step as f32 / 40.0;
                let across = screen.across(up);
                assert!(
                    across <= previous + 1e-6,
                    "{screen:?} widens at {up}: {across} against {previous}"
                );
                assert!((0.0..=1.0).contains(&across), "{screen:?} is {across} wide");
                previous = across;
            }
        }
    }

    /// A stepped gable comes to a pier and a curve comes to a point.
    #[test]
    fn each_screen_ends_the_way_its_name_says() {
        // The staircase reaches nothing at the very top, and the pier below it
        // is one step wide — that is what the last step of a Treppengiebel is.
        assert!(Screen::Stepped.across(1.0).abs() < 1e-6);
        // The curve holds full width through its shoulder and only then falls
        // away, which is the whole of what makes it an ogee rather than a
        // triangle.
        assert!((Screen::Curved.across(CURVE_SHOULDER * 0.5) - 1.0).abs() < 1e-6);
        assert!(Screen::Curved.across(1.0).abs() < 1e-6);
        // And a straight top is straight all the way to the coping.
        assert!((Screen::Straight.across(1.0) - 1.0).abs() < 1e-6);
        // A curve is drawn with enough bands that the steps are not countable.
        assert!(Screen::Curved.bands(4) > Screen::Stepped.bands(4) * 2);
    }

    /// A narrow house gets a steep gable and a wide one a shallow gable.
    #[test]
    fn the_pitch_is_the_same_whatever_the_house_is_wide() {
        // The rise is a fraction of the *frontage*, so the angle at the apex is
        // the same on every house in a row — which is what makes a parade of
        // them read as one street built at one time, rather than as a shelf of
        // unrelated triangles.
        let angle = |frontage: f32, share: f32| ((frontage * share) / (frontage * 0.5)).atan();
        for share in [RISE.0, RISE.1] {
            let narrow = angle(6.0, share);
            let wide = angle(18.0, share);
            assert!(
                (narrow - wide).abs() < 1e-5,
                "a 6m house pitches at {narrow:.3} and an 18m one at {wide:.3}"
            );
            // And the pitch is a roof pitch rather than a spire or a shed.
            assert!(
                (0.8..1.4).contains(&narrow),
                "a pitch of {narrow:.2} radians is not a house"
            );
        }
    }

    /// A screen stands one storey to one and a half above the eaves, whatever
    /// the frontage says.
    #[test]
    fn the_screen_is_an_attic_storey_and_not_a_billboard() {
        // The rise the frontage asks for, held to the storeys of the house.
        // The failure this pins is the one the user circled: a wide Lowrise
        // plot whose screen ran to twice the height of the house under it.
        let rise = |frontage: f32, share: f32, height: f32, storey: f32| {
            (frontage * share)
                .clamp(storey * SCREEN_STOREYS.0, storey * SCREEN_STOREYS.1)
                .min(height * 0.62)
        };
        for (frontage, height, storey) in [
            (6.0f32, 9.0f32, 4.5f32),
            (12.0, 16.0, 4.0),
            (20.0, 18.0, 4.5),
            (9.0, 22.0, 5.5),
        ] {
            for share in [RISE.0, RISE.1] {
                let got = rise(frontage, share, height, storey);
                assert!(
                    got <= storey * SCREEN_STOREYS.1 + 1e-4,
                    "a {frontage}m house grew a {got:.1}m screen on {storey:.1}m storeys"
                );
                assert!(
                    got >= (storey * SCREEN_STOREYS.0).min(height * 0.62) - 1e-4,
                    "a {frontage}m house's screen is {got:.1}m, under a storey"
                );
                assert!(got <= height * 0.62 + 1e-4);
            }
        }
    }

    /// The screen stands taller than the roof it is hiding, and the roof
    /// stops inside it.
    #[test]
    fn the_screen_hides_its_own_roof() {
        // This is the only thing a gable screen is for. If the ridge pokes out
        // over the top of it the building has a triangle growing out of a
        // staircase, which is not a thing anybody has ever built.
        for frontage in [6.0f32, 12.0, 20.0] {
            for throat in [8.0f32, 14.0, 22.0] {
                for rise in [3.7f32, 5.0, 7.0] {
                    let pitch =
                        roof::Pitch::screened(frontage, throat, 16.0, rise, THICK - PROUD + 0.01);
                    assert!(
                        rise > pitch.rise,
                        "a {frontage}x{throat}m house hides {rise:.1}m of a {:.1}m ridge",
                        pitch.rise
                    );
                    // And the roof ends behind the screen's back face rather
                    // than coming through it.
                    assert!(pitch.ends.1 < 0.0);
                    assert!(pitch.ends.1 >= -(THICK - PROUD) - 0.02);
                }
            }
        }
    }

    /// The attic windows sit inside every form of screen.
    #[test]
    fn the_windows_stay_inside_the_screen() {
        for screen in [Screen::Straight, Screen::Stepped, Screen::Curved] {
            for frontage in [5.0f32, 7.0, 9.0, 14.0] {
                let rise = 4.5f32;
                let panes = if frontage < 9.0 { 2 } else { 3 };
                let top_up = ((rise * PANE_UP + PANE.1 * 0.5) / rise).min(1.0);
                let reach =
                    (frontage * screen.across(top_up) * 0.5 - PANE.0 * 0.5 - SURROUND).max(0.0);
                let spacing = (frontage * 0.24).min(reach / ((panes - 1) as f32 * 0.5).max(1.0));
                let outer = (panes - 1) as f32 * 0.5 * spacing + PANE.0 * 0.5 + SURROUND;
                assert!(
                    outer <= frontage * screen.across(top_up) * 0.5 + 1e-4,
                    "{screen:?} on a {frontage}m house hangs a pane {outer:.2}m out"
                );
                // Above the cornice, and the whole row under the top.
                assert!(rise * PANE_UP - PANE.1 * 0.5 > CORNICE);
                assert!(rise * PANE_UP + PANE.1 * 0.5 < rise);
            }
        }
    }
}
