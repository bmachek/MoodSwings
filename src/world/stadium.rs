//! The stadium, and the crowd that lives in it.
//!
//! Every city on the roadmap's postcard list has one, and a stadium is two
//! things this codebase already knows how to build: a garage (tiered
//! concrete on columns, inward-facing) and a parade (a crowd that exists to
//! be seen). The zoning pass stamps one `BuildingKind::Stadium` on the
//! city's edge and this module raises the bowl: a painted pitch, four
//! tiered stands, floodlights on the corners — and the spectators, who are
//! the whole point.
//!
//! The spectators are not pedestrians. They have no AI, no collider, no
//! mood and no route; they are seats with faces, two entities each, and
//! they do the one thing a stadium crowd does that no street crowd can:
//! **die Welle**. The la-ola is a pulse of standing-up that circles the
//! bowl forever, driven by elapsed time and each seat's angle — no state,
//! no coordination, no message passing, just `sin(t - angle)` clamped to
//! its crest. In a city of rubber people the wave is practically a hop,
//! which is why it reads as native here.
//!
//! Home end and away end: the long sides wear FC WUMMS blue, the short
//! ends SV BOING red. Nobody has ever scored; the scoreboard says so.

use avian3d::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::buildings::{ChunkOf, CityAssets, SIDEWALK_HEIGHT};
use super::texture::{encode, painted_rect, text_band};
use crate::core::schedule::GameSet;

/// How far the bowl draws. It replaces a whole building's silhouette on the
/// skyline, so it reads far — but the crowd inside only ever matters from
/// nearby, and [`SEAT_RANGE`] cuts the seats long before the concrete goes.
const RANGE: f32 = 900.0;
const SEAT_RANGE: f32 = 260.0;

/// Stand geometry: three tiers of concrete, each this deep and this high.
const TIER_DEPTH: f32 = 2.6;
const TIER_RISE: f32 = 1.15;
/// Metres between spectators along a tier.
const SEAT_SPACING: f32 = 2.3;
/// The la-ola: how fast the crest circles the bowl (radians of bowl angle
/// per second) and how high a spectator rises on it.
const WAVE_SPEED: f32 = 1.1;
const WAVE_LIFT: f32 = 0.6;

/// One occupied seat: where it rests and where it sits around the bowl.
#[derive(Component)]
pub struct Seat {
    rest_y: f32,
    /// Angle around the bowl's centre, in radians — the wave's clock face.
    angle: f32,
}

#[derive(Resource)]
pub struct StadiumKit {
    body: Handle<Mesh>,
    head: Handle<Mesh>,
    home: Handle<StandardMaterial>,
    away: Handle<StandardMaterial>,
    face: Handle<StandardMaterial>,
    pitch: Handle<Mesh>,
    turf: Handle<StandardMaterial>,
    board: Handle<Mesh>,
    score: Handle<StandardMaterial>,
}

/// The pitch: turf, halfway line, centre circle, two goal boxes. Painted
/// once; the quad is scaled to the pitch it lands on.
fn pitch_texture() -> Image {
    painted_rect(512, 384, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let line = |hit: bool| {
            if hit {
                Some([234, 240, 232, 255])
            } else {
                None
            }
        };
        let paint = line((u - 0.5).abs() < 0.004)
            .or(line({
                let d = Vec2::new((u - 0.5) * 512.0, (v - 0.5) * 384.0).length();
                (d - 52.0).abs() < 2.0
            }))
            .or(line(
                !(0.13..=0.87).contains(&u) && (0.28..0.72).contains(&v) && {
                    let edge_u = if u < 0.5 {
                        (u - 0.13).abs()
                    } else {
                        (u - 0.87).abs()
                    };
                    edge_u < 0.004 || (v - 0.28).abs() < 0.006 || (v - 0.72).abs() < 0.006
                },
            ));
        if let Some(white) = paint {
            return white;
        }
        // Mown stripes, because a pitch without them is a lawn.
        let stripe = ((u * 10.0) as u32).is_multiple_of(2);
        if stripe {
            [58, 122, 62, 255]
        } else {
            [50, 110, 54, 255]
        }
    })
}

/// The fixtures this league has, and how each of them is going.
///
/// Every one of them is unfinished, and that is the whole joke: nothing in
/// this city can be decided, because nothing in it can be stopped. The five
/// clubs are named after what happens to a rubber ball — Wumms, Boing, Dotz,
/// Abprall, Delle — which is also how half the real ones got their names,
/// one abstraction further back.
///
/// One derby per city rather than one per league: the board is built once at
/// startup from the world seed, so a given town has a fixture the way it has
/// a street plan, and coming back to it finds the same match still level.
const FIXTURES: [(&str, &str); 6] = [
    ("FC WUMMS - SV BOING", "0 : 0 - VERLÄNGERUNG: EWIG"),
    ("SV DOTZ - FC ABPRALL", "2 : 2 - SEIT DIENSTAG"),
    ("TSV GUMMI 04 - SC DELLE", "1 : 1 - TORLINIE STRITTIG"),
    ("FC WUMMS - TSV GUMMI 04", "3 : 3 - NIEMAND ZÄHLT MEHR"),
    ("SV BOING - SC DELLE", "0 : 0 - BALL IST WEG"),
    ("FC ABPRALL - SV DOTZ", "5 : 5 - WIR SPIELEN WEITER"),
];

/// Which fixture this city's stadium is showing.
fn fixture_for(seed: u64) -> &'static (&'static str, &'static str) {
    // The seed's own bits, mixed once so that two neighbouring seeds do not
    // get neighbouring fixtures — the same avalanche the calendar needed.
    let mut h = seed ^ 0x9E37_79B9_7F4A_7C15;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    &FIXTURES[(h % FIXTURES.len() as u64) as usize]
}

/// The scoreboard: the fixture, and the eternal result.
fn score_texture(seed: u64) -> Image {
    let (fixture, result) = fixture_for(seed);
    let title = encode(fixture);
    let score = encode(result);
    painted_rect(512, 128, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        if !(0.015..=0.985).contains(&u) || !(0.06..=0.94).contains(&v) {
            return [18, 20, 24, 255];
        }
        let lit =
            text_band(&title, u, (v - 0.12) / 0.38) || text_band(&score, u, (v - 0.58) / 0.30);
        if lit {
            [244, 214, 74, 255]
        } else {
            [30, 34, 40, 255]
        }
    })
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    seed: u64,
) -> StadiumKit {
    StadiumKit {
        body: meshes.add(Cuboid::new(0.42, 0.62, 0.34)),
        head: meshes.add(Sphere::new(0.16)),
        home: materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.32, 0.62),
            perceptual_roughness: 0.9,
            ..default()
        }),
        away: materials.add(StandardMaterial {
            base_color: Color::srgb(0.66, 0.16, 0.14),
            perceptual_roughness: 0.9,
            ..default()
        }),
        // One face for the whole crowd: delighted. Nobody at a game where
        // nobody has ever scored has anywhere better to be.
        face: materials.add(StandardMaterial {
            base_color: Color::srgb(0.94, 0.78, 0.18),
            perceptual_roughness: 0.75,
            ..default()
        }),
        pitch: meshes.add(Plane3d::default().mesh().size(1.0, 1.0)),
        turf: materials.add(StandardMaterial {
            base_color_texture: Some(images.add(pitch_texture())),
            perceptual_roughness: 0.95,
            ..default()
        }),
        board: meshes.add(Rectangle::new(7.0, 1.75)),
        score: materials.add(StandardMaterial {
            base_color_texture: Some(images.add(score_texture(seed))),
            perceptual_roughness: 0.7,
            ..default()
        }),
    }
}

/// Raises the bowl on a stamped footprint.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    assets: &CityAssets,
    kit: &StadiumKit,
    seed: u64,
    center: Vec2,
    width: f32,
    depth: f32,
    yaw: f32,
    chunk: IVec2,
) {
    let spin = Quat::from_rotation_y(yaw);
    let place = |at: Vec3| spin * at + Vec3::new(center.x, SIDEWALK_HEIGHT, center.y);
    let far = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: (RANGE * 0.9)..RANGE,
        use_aabb: false,
    };
    let near = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: (SEAT_RANGE * 0.9)..SEAT_RANGE,
        use_aabb: false,
    };

    // The pitch, inset behind the stands.
    let stands = TIER_DEPTH * 3.0 + 0.8;
    let pitch = Vec2::new(width - stands * 2.0, depth - stands * 2.0);
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.pitch.clone()),
        MeshMaterial3d(kit.turf.clone()),
        Transform::from_translation(place(Vec3::new(0.0, 0.01, 0.0)))
            .with_rotation(spin)
            .with_scale(Vec3::new(pitch.x, 1.0, pitch.y)),
        NotShadowCaster,
        far.clone(),
    ));

    // The stands: three tiers per side, rectangular rings of concrete. Each
    // tier is four boxes; each box is a step the crowd sits on and a wall
    // the ball bounces off, so every one carries a collider.
    let mut ticket = seed;
    let mut roll = move || {
        // splitmix64: the seats need no stream, only reproducibility.
        ticket = ticket.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = ticket;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as f64 / u64::MAX as f64
    };

    for tier in 0..3usize {
        let inner_x = pitch.x * 0.5 + tier as f32 * TIER_DEPTH;
        let inner_z = pitch.y * 0.5 + tier as f32 * TIER_DEPTH;
        let top = (tier as f32 + 1.0) * TIER_RISE;
        // Four sides: ±x (the long ends), ±z (home and away). Each step box
        // runs the full side minus the corners, which stay open — a bowl
        // with sealed corners is a bathtub.
        for (side, along_z) in [(1.0f32, true), (-1.0, true), (1.0, false), (-1.0, false)] {
            let (at, size) = if along_z {
                (
                    Vec3::new(side * (inner_x + TIER_DEPTH * 0.5), top * 0.5, 0.0),
                    Vec3::new(TIER_DEPTH, top, inner_z * 2.0 - 1.0),
                )
            } else {
                (
                    Vec3::new(0.0, top * 0.5, side * (inner_z + TIER_DEPTH * 0.5)),
                    Vec3::new(inner_x * 2.0 - 1.0, top, TIER_DEPTH),
                )
            };
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(assets.unit_cube.clone()),
                MeshMaterial3d(assets.concrete()),
                Transform::from_translation(place(at))
                    .with_rotation(spin)
                    .with_scale(size),
                RigidBody::Static,
                Collider::cuboid(1.0, 1.0, 1.0),
                far.clone(),
            ));

            // The crowd on this tier's top step.
            let run = (if along_z { size.z } else { size.x }) - 1.0;
            let seats = (run / SEAT_SPACING) as i32;
            for i in 0..seats {
                if roll() > 0.72 {
                    continue;
                }
                let offset = (i as f32 + 0.5) * SEAT_SPACING - run * 0.5;
                let local = if along_z {
                    Vec3::new(at.x, top + 0.31, offset)
                } else {
                    Vec3::new(offset, top + 0.31, at.z)
                };
                let world = place(local);
                // The wave clock reads the seat's angle around the bowl.
                let angle = local.z.atan2(local.x);
                let coat = if along_z { &kit.home } else { &kit.away };
                commands
                    .spawn((
                        ChunkOf(chunk),
                        Mesh3d(kit.body.clone()),
                        MeshMaterial3d(coat.clone()),
                        Transform::from_translation(world),
                        Seat {
                            rest_y: world.y,
                            angle,
                        },
                        near.clone(),
                        NotShadowCaster,
                    ))
                    .with_child((
                        Mesh3d(kit.head.clone()),
                        MeshMaterial3d(kit.face.clone()),
                        Transform::from_xyz(0.0, 0.47, 0.0),
                        near.clone(),
                        NotShadowCaster,
                    ));
            }
        }
    }

    // Floodlights: four corner masts, each a pole and a head of glare.
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            let foot = Vec3::new(
                sx * (pitch.x * 0.5 + stands - 1.2),
                0.0,
                sz * (pitch.y * 0.5 + stands - 1.2),
            );
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(assets.unit_cube.clone()),
                MeshMaterial3d(assets.concrete()),
                Transform::from_translation(place(foot + Vec3::Y * 7.0))
                    .with_rotation(spin)
                    .with_scale(Vec3::new(0.4, 14.0, 0.4)),
                RigidBody::Static,
                Collider::cuboid(1.0, 1.0, 1.0),
                far.clone(),
            ));
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(assets.unit_cube.clone()),
                MeshMaterial3d(kit.face.clone()),
                Transform::from_translation(place(foot + Vec3::Y * 14.4))
                    .with_rotation(spin * Quat::from_rotation_x(0.5))
                    .with_scale(Vec3::new(2.2, 1.4, 0.3)),
                NotShadowCaster,
                far.clone(),
            ));
        }
    }

    // The scoreboard, over the -z end, facing the pitch.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.board.clone()),
        MeshMaterial3d(kit.score.clone()),
        Transform::from_translation(place(Vec3::new(
            0.0,
            TIER_RISE * 3.0 + 2.6,
            -(pitch.y * 0.5 + stands - TIER_DEPTH),
        )))
        .with_rotation(spin),
        NotShadowCaster,
        far,
    ));
}

/// Die Welle. A crest of standing-up circles the bowl at [`WAVE_SPEED`];
/// each seat rises as the crest passes its angle and settles behind it.
fn laola(time: Res<Time>, mut seats: Query<(&Seat, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (seat, mut transform) in &mut seats {
        // A narrow pulse rather than a sine: most of the bowl sits, one
        // sector stands, which is what a wave looks like.
        let crest = ((t * WAVE_SPEED - seat.angle).sin() - 0.88).max(0.0) / 0.12;
        transform.translation.y = seat.rest_y + crest * crest * WAVE_LIFT;
    }
}

pub struct StadiumPlugin;

impl Plugin for StadiumPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, laola.in_set(GameSet::Simulation));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scoreboard_fits_the_font() {
        use super::super::texture::glyph;
        for (fixture, result) in FIXTURES {
            for text in [fixture, result] {
                for code in encode(text) {
                    assert!(
                        code == b' ' || glyph(code) != [0; 7],
                        "the scoreboard needs {:?} and the font has none",
                        code as char
                    );
                }
                // The board is 512 texels across and a name written wider
                // than about thirty cells stops resolving into letters.
                assert!(
                    text.chars().count() <= 28,
                    "{text:?} is too long to read from the stands"
                );
            }
        }
    }

    #[test]
    fn every_town_has_a_derby_and_not_always_the_same_one() {
        let mut seen = std::collections::HashSet::new();
        for seed in 0..400u64 {
            seen.insert(fixture_for(seed).0);
        }
        assert_eq!(seen.len(), FIXTURES.len(), "only {seen:?} are ever played");
        // And a town's fixture is a fact about the town: the stadium is
        // respawned with its chunk and must not re-draw the match.
        assert_eq!(fixture_for(0xA17E_5EED), fixture_for(0xA17E_5EED));
    }

    #[test]
    fn nobody_in_this_league_has_ever_won_anything() {
        // The league table is the joke and the joke is that it is empty.
        // A fixture that had been decided would be the one thing in this
        // city that finished.
        for (fixture, result) in FIXTURES {
            let goals: Vec<&str> = result
                .split(" - ")
                .next()
                .expect("a result starts with a score")
                .split(" : ")
                .collect();
            assert_eq!(goals.len(), 2, "{fixture}: {result:?} is not a score");
            assert_eq!(goals[0], goals[1], "{fixture} has been decided");
        }
    }

    #[test]
    fn the_wave_is_a_crest_and_not_a_tide() {
        // At any instant, only a narrow sector of the bowl should be off
        // its seat — a wave that lifts half the stadium is a mexican
        // stand-up, not a mexican wave.
        let t = 3.7f32;
        let mut standing = 0;
        const SEATS: usize = 360;
        for i in 0..SEATS {
            let angle = i as f32 / SEATS as f32 * std::f32::consts::TAU;
            let crest = ((t * WAVE_SPEED - angle).sin() - 0.88).max(0.0) / 0.12;
            if crest > 0.05 {
                standing += 1;
            }
        }
        assert!(standing > 0, "the wave never passes anybody");
        assert!(
            standing < SEATS / 5,
            "{standing} of {SEATS} seats are up at once"
        );
    }

    #[test]
    fn the_board_is_lit_letters_on_a_dark_panel() {
        // Every fixture actually gets painted, and gets painted as a
        // scoreboard: a band slip that lights the whole panel or none of it
        // builds, hangs over the stand, and says nothing from the terraces.
        for seed in 0..FIXTURES.len() as u64 * 7 {
            let image = score_texture(seed);
            let data = image.data.as_ref().expect("the scoreboard was not painted");
            let lit = data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[..3] == [244, 214, 74])
                .count() as f32
                / (data.len() / 4) as f32;
            assert!(
                (0.02..0.40).contains(&lit),
                "seed {seed}: {lit:.3} of the board is lit"
            );
        }
    }
}
