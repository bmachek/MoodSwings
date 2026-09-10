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
//! things were built. So the pitch gets the eaves detail it needs from the
//! flank and the screen gets the arithmetic.
//!
//! ## What a gabled building gives up
//!
//! Its rooftop clutter. Air handling, tanks and vent stacks sit on a flat deck
//! and a pitched roof does not have one, so a gabled building would have its
//! plant floating inside its own rafters. `buildings` skips the clutter for
//! them, which is also correct: these are houses, and a house's roof has a
//! chimney on it and nothing else.

use bevy::camera::visibility::VisibilityRange;
use bevy::math::Affine2;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::{ChaCha8Rng, rand_core::SeedableRng};

use super::buildings::ChunkOf;
use super::texture;

/// How far the screen itself is drawn. As far as the building it caps: a
/// roofline is a silhouette, and a silhouette is the last thing to stop
/// mattering.
pub const RANGE: f32 = 900.0;

/// And how far the *coping* is — the stone slab across the top of every step.
///
/// Half the meshes in a screen are coping, and coping is a hundred and twenty
/// millimetres thick. Past a couple of hundred metres it is under a pixel and
/// all it contributes is a lighter edge on a skyline that already has one, so
/// on a city where four buildings in five are gabled it is the obvious half to
/// drop first.
///
/// Two hundred and thirty was tuned against a shell that turned into a plain
/// box at two hundred and fifty, so the coping outlived the wall it sat on by
/// twenty metres and no further. The wall now keeps its courses to four
/// hundred and sixty, and a gabled skyline without its copings is a row of
/// plain triangles.
const COPING_RANGE: f32 = 380.0;

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
/// pitch.
const RISE: (f32, f32) = (0.55, 0.85);

/// How far the screen stands proud of the wall below it.
const PROUD: f32 = 0.10;
/// And how thick it is.
const THICK: f32 = 0.32;

/// How high the ridge stands above the eaves, as a fraction of the *frontage*.
///
/// The frontage and not the depth, and the difference is the whole shape of the
/// roof: on a `Giebelhaus` the ridge runs back from the street, so the roof
/// falls to the two *side* walls and the span it crosses is how wide the house
/// is. Measured off the depth instead — which is what this did first, and what
/// the test below caught — a narrow deep house grows a ridge two metres taller
/// than the screen that is supposed to be hiding it.
///
/// It also makes the pitch a constant: every roof in the row leans at
/// `atan(2 * PITCH)`, which is a shade over forty degrees and is a roof.
const PITCH: f32 = 0.42;

/// How far the tiles carry on past the wall they are resting on.
///
/// The single most load-bearing number in this module, and for a long time it
/// was zero. A roof whose covering stops dead on the plane of the wall is not
/// a roof — it is a lid — and a street of lids is a street of extruded boxes,
/// which is exactly what an aerial of this town looked like. What an eaves
/// overhang buys is a *shadow*: a dark line the width of a hand under the
/// tiles, running the whole length of every house, which is the thing that
/// separates the roof from the wall at any distance at all.
///
/// Four hundred millimetres at the eaves, which is a modest Bavarian one, and
/// three at the verge, where the gable screen is standing in front of it
/// anyway.
const EAVES_OVERHANG: f32 = 0.40;
const VERGE_OVERHANG: f32 = 0.30;

/// The board closing the ends of the rafters, and the gutter hung on it.
///
/// A fascia is what you actually see of an overhang from underneath: a pale
/// vertical strip under the tile edge. The gutter in front of it is the one
/// horizontal line on a house that is neither wall nor roof, and it catches the
/// sun along its whole length, which is why it reads from a street away.
const FASCIA_DROP: f32 = 0.20;
const FASCIA_THICK: f32 = 0.05;
const GUTTER_RADIUS: f32 = 0.065;

/// The capping along the ridge. A half-round in life, and a half-round here.
const RIDGE_RADIUS: f32 = 0.11;

/// A dormer's proportions, in metres: how far along the roof it stands, how
/// wide across the slope it is, and how far it rises out of the tiles.
///
/// It is the defining feature of a Landshut roofscape and there was not one in
/// the game. They sit on the *flanks*, which on a `Giebelhaus` is the side
/// facing the neighbour rather than the street — so they are worth almost
/// nothing from a pavement and a great deal from anywhere above one, which is
/// where a roofscape is looked at.
const DORMER: (f32, f32, f32) = (1.35, 1.05, 0.95);

/// How far up the slope from the eaves a dormer sits, as a fraction of the
/// horizontal run. Low: a dormer high on a slope is a skylight.
const DORMER_UP: f32 = 0.34;

/// Roofs shallower than this in their run carry no dormers — there is nothing
/// to put one in.
const DORMER_MIN_RUN: f32 = 4.2;

/// And how far they are drawn. Nearer than the roof itself: a dormer is a
/// metre across and four of them on a skyline are four pixels.
const DORMER_RANGE: f32 = 320.0;

#[derive(Resource)]
pub struct GableKit {
    cube: Handle<Mesh>,
    /// A unit cylinder lying on its own Y. Scaled into the gutter along the
    /// eaves and the capping along the ridge — both are half-rounds in life,
    /// and a box reads as neither.
    round: Handle<Mesh>,
    /// Zinc: the gutter, and the flashing round a dormer.
    metal: Handle<StandardMaterial>,
    /// What a dormer's cheeks are rendered in, and the dark of its glazing.
    dormer: Handle<StandardMaterial>,
    glass: Handle<StandardMaterial>,
    /// The screen wears the building's own wall material, which this module
    /// does not own — so what is kept here is only what a roof is made of.
    ///
    /// Three roofs rather than one, because a real old town has not been
    /// re-tiled all at once: a house done last summer sits between one that
    /// was done in the seventies and one nobody has touched since the moss
    /// took it. Picked from the building's own seed, so a roof keeps its age
    /// across a chunk respawn.
    tile: [Handle<StandardMaterial>; 3],
    cap: Handle<StandardMaterial>,
}

/// How many times the tile image repeats over one leaf of a roof: up the
/// slope first, then along the ridge.
///
/// Not the same both ways, because a leaf is not square. A Landshut house is
/// eight to fourteen metres wide and about as deep, so the fall is around four
/// metres and the ridge runs twice that — and a Biberschwanz is a hand's width
/// whichever way you measure it. These two numbers are what put roughly a
/// hand's width in both directions.
///
/// Fixed rather than per-house: every leaf shares one cube mesh, so a
/// per-building tiling would mean a material per building.
const LAP: Vec2 = Vec2::new(3.0, 5.5);

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> GableKit {
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
        tile: ages.map(|age| {
            materials.add(StandardMaterial {
                // Old clay, not the grey felt the flat roofs are covered in.
                base_color: age,
                base_color_texture: Some(clay.clone()),
                normal_map_texture: Some(relief.clone()),
                // The same image twice: occlusion takes its red channel and
                // the metallic/roughness slot takes its green and blue.
                occlusion_texture: Some(surface.clone()),
                metallic_roughness_texture: Some(surface.clone()),
                uv_transform: Affine2::from_scale(LAP),
                // One, because it multiplies the map. Anything else throws the
                // green channel away — see `material::ScannedSet::apply`.
                perceptual_roughness: 1.0,
                metallic: 1.0,
                ..default()
            })
        }),
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
/// Returns how far the ridge stands above the eaves, which is the one thing
/// about this roof that anybody outside the module needs and cannot work out:
/// the rise is drawn from the building's own seed here, so recomputing it
/// anywhere else would mean replaying the same draws in the same order for
/// ever. `world::plume` wants it to stand a chimney on the tiles rather than
/// inside them.
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
    // first.
    height: f32,
    eaves: f32,
    yaw: f32,
    chunk: IVec2,
    lod_scale: f32,
) -> f32 {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x6AB1_E501);
    let steps = rng.random_range(STEPS.0..=STEPS.1);
    let screen = draw_screen(&mut rng);
    // Off the frontage, and then held to the house. The width is what sets the
    // pitch and the pitch is what makes a row read as one street — but a
    // generator that hands out lots wider than its buildings are tall will
    // otherwise put a ten-metre screen on a five-metre house, which is not an
    // Altstadt, it is a row of billboards. `CityStyle::lot_scale` narrows the
    // plots so the clamp rarely bites; this is what happens when it does.
    let rise = (frontage * rng.random_range(RISE.0..RISE.1)).min(height * 0.62);
    let ridge = (frontage * PITCH).min(rise * 0.72);

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
    // When this roof was last done. From the building's own seed like
    // everything else about it.
    let age = rng.random_range(0..kit.tile.len());
    let slope = (ridge / (frontage * 0.5)).atan();
    // The tiles carry on past the wall. Everything below is measured off `run`
    // — the horizontal half-span from ridge to tile edge — rather than off the
    // frontage, so the overhang moves the eaves, the fascia, the gutter and
    // the dormers together and cannot be added to one of them and forgotten on
    // the next.
    let run = frontage * 0.5 + EAVES_OVERHANG;
    let leaf = run / slope.cos();
    let depth = throat + VERGE_OVERHANG * 2.0;
    // How far the tile edge hangs below the top of the wall.
    let drop = EAVES_OVERHANG * slope.tan();
    let along = Vec2::new(outward.y, -outward.x);

    for side in [-1.0f32, 1.0] {
        // Each leaf is centred half way along its own slope, which the
        // overhang has moved outward and down.
        let at = center + along * (side * run * 0.5);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.tile[age].clone()),
            // Negated, and this is the whole difference between a roof and a
            // gutter. `rotation_z` by a positive angle lifts the local +X end,
            // and the leaf on the +X side of the ridge has its *outer* edge
            // there — so `side * slope` raises both outer edges and drops the
            // middle, and every house in the town wears a trough. The eaves go
            // down and the ridge goes up.
            Transform::from_xyz(at.x, eaves + (ridge - drop) * 0.5, at.y)
                .with_rotation(turn * Quat::from_rotation_z(-side * slope))
                .with_scale(Vec3::new(leaf, 0.14, depth)),
            range.clone(),
        ));

        // The fascia closing the rafter ends, and the gutter hung on it. Both
        // sit at the tile edge — out past the wall by the overhang and down by
        // what the pitch does over that distance — and both run the full depth
        // of the roof, which is what makes them a *line* rather than a detail.
        let edge = center + along * (side * run);
        let lip = eaves - drop;
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.cap.clone()),
            Transform::from_xyz(edge.x, lip - FASCIA_DROP * 0.5, edge.y)
                .with_rotation(turn)
                .with_scale(Vec3::new(FASCIA_THICK, FASCIA_DROP, depth)),
            close.clone(),
        ));
        let hang = center + along * (side * (run + GUTTER_RADIUS * 0.6));
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.round.clone()),
            MeshMaterial3d(kit.metal.clone()),
            // A cylinder stands on its own Y; the gutter lies along the depth,
            // which is the roof's local Z.
            Transform::from_xyz(hang.x, lip - FASCIA_DROP * 0.55, hang.y)
                .with_rotation(turn * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::new(GUTTER_RADIUS, depth, GUTTER_RADIUS)),
            close.clone(),
        ));

        dormers(
            commands,
            kit,
            seed,
            center,
            along,
            outward,
            side,
            run,
            slope,
            eaves + ridge,
            turn,
            chunk,
            lod_scale,
        );
    }

    // And the capping along the ridge, which is the one line on a pitched roof
    // that is lit from both sides at once.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.round.clone()),
        MeshMaterial3d(kit.tile[age].clone()),
        Transform::from_xyz(center.x, eaves + ridge + RIDGE_RADIUS * 0.4, center.y)
            .with_rotation(turn * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
            .with_scale(Vec3::new(RIDGE_RADIUS, depth, RIDGE_RADIUS)),
        range.clone(),
    ));

    ridge
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

/// Puts one or two dormers on a roof leaf.
///
/// Three boxes each: the cheeks, the glazing set into the front of them, and a
/// small flat lid with a zinc edge. Not a pitched dormer roof, which would be
/// four more meshes for a shape that is one pixel tall from anywhere the
/// dormer is visible at all — what reads is the *interruption* in the slope
/// and the dark rectangle in it.
#[allow(clippy::too_many_arguments)]
fn dormers(
    commands: &mut Commands,
    kit: &GableKit,
    seed: u64,
    center: Vec2,
    // `along` is across the frontage, which is the way the roof falls;
    // `outward` is down the depth, which is the way the ridge runs.
    along: Vec2,
    outward: Vec2,
    side: f32,
    run: f32,
    slope: f32,
    ridge_top: f32,
    turn: Quat,
    chunk: IVec2,
    lod_scale: f32,
) {
    if run < DORMER_MIN_RUN {
        return;
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
        return;
    }

    let end = (DORMER_RANGE * lod_scale).min(700.0);
    let range = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: end..(end * 1.1),
        use_aabb: false,
    };
    // Where on the slope: `DORMER_UP` of the way from the tile edge to the
    // ridge, measured horizontally. The roof surface at that point is what the
    // dormer stands on.
    let across = run * (1.0 - DORMER_UP);
    let base = ridge_top - across * slope.tan();
    let at = center + along * (side * across);
    // Spaced down the depth of the roof, so two of them are a pair of dormers
    // rather than a pair of ears.
    let spacing = DORMER.0 * 2.4;

    for index in 0..count {
        let offset = (index as f32 - (count as f32 - 1.0) * 0.5) * spacing;
        let stand = at + outward * offset;

        // The cheeks. Set *into* the slope by a little, so the box does not
        // float on top of the tiles at the low end of it.
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.dormer.clone()),
            Transform::from_xyz(stand.x, base + DORMER.2 * 0.5 - 0.12, stand.y)
                .with_rotation(turn)
                .with_scale(Vec3::new(DORMER.1, DORMER.2, DORMER.0)),
            range.clone(),
        ));
        // The glazing, standing a few centimetres proud of the outward cheek.
        let face = stand + along * (side * (DORMER.1 * 0.5 + 0.02));
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.glass.clone()),
            Transform::from_xyz(face.x, base + DORMER.2 * 0.52 - 0.06, face.y)
                .with_rotation(turn)
                .with_scale(Vec3::new(0.05, DORMER.2 * 0.55, DORMER.0 * 0.72)),
            range.clone(),
        ));
        // And the lid, oversailing on all four sides with a zinc edge.
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(kit.metal.clone()),
            Transform::from_xyz(stand.x, base + DORMER.2 - 0.06, stand.y)
                .with_rotation(turn)
                .with_scale(Vec3::new(DORMER.1 + 0.22, 0.08, DORMER.0 + 0.22)),
            range.clone(),
        ));
    }
}

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
            // The roof leans at the same angle whatever the house is wide, for
            // the same reason and by the same arithmetic.
            let roof = (2.0f32 * PITCH).atan();
            assert!(
                (0.55..0.95).contains(&roof),
                "a {roof:.2} radian roof is a spire or a shed"
            );
        }
    }

    /// The eaves are below the ridge.
    #[test]
    fn a_roof_sheds_outwards_rather_than_inwards() {
        // The failure this pins is a roof rotated the right amount about the
        // right axis in the wrong direction: two leaves that meet in a valley
        // at the middle and rise to their outer edges, which is a gutter the
        // length of the house and reads, from the air, as a town of troughs.
        let slope = 0.7f32;
        let leaf = 6.0f32;
        for side in [-1.0f32, 1.0] {
            let turn = Quat::from_rotation_z(-side * slope);
            // The leaf runs along its own X; its outer end is the one on the
            // same side of the ridge as the leaf itself.
            let outer = turn * Vec3::new(side * leaf * 0.5, 0.0, 0.0);
            let inner = turn * Vec3::new(-side * leaf * 0.5, 0.0, 0.0);
            assert!(
                outer.y < inner.y,
                "the {side} leaf rises {:.2} from ridge to eaves",
                outer.y - inner.y
            );
        }
    }

    /// The screen stands taller than the roof it is hiding.
    #[test]
    fn the_screen_hides_its_own_roof() {
        // This is the only thing a gable screen is for. If the ridge pokes out
        // over the top of it the building has a triangle growing out of a
        // staircase, which is not a thing anybody has ever built.
        for frontage in [6.0f32, 12.0, 20.0] {
            for throat in [8.0f32, 14.0, 22.0] {
                let rise = frontage * RISE.0;
                let ridge = frontage * PITCH;
                assert!(
                    rise > ridge * 0.85,
                    "a {frontage}x{throat}m house hides {rise:.1}m of a {ridge:.1}m ridge"
                );
            }
        }
    }
}
