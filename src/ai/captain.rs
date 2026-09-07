//! Captain Erdnuss.
//!
//! One resident menace with a peanut for a head. He is deliberately in no
//! README and no roadmap — an inside joke ships in the code or not at all —
//! but everything about how he works is written down here, because a joke
//! that cannot be maintained stops being funny the first time it breaks.
//!
//! He is the city's endgegner, and the mechanics are entirely borrowed: he
//! picks whoever is nearest and stands *much* too close (the crowd keeps
//! `separation_radius` of personal space; he closes to arm's length and
//! stays), he greets every new acquaintance with the word ERDNUSS — painted
//! on a speech bubble, since the sound bank is wordless by rule and one
//! recorded exception was enough — and he keeps up a steady output of coughs,
//! burps, spit, retching and gagging, each of which goes out as an ordinary
//! [`Provocation`] taunt. The street sours, grudges form, pursuers arrive,
//! and he gets grabbed by the collar and shaken through exactly the systems
//! everybody else answers to. It never helps: his temperament has a fuse of
//! nearly nothing and a floor near delight, so he picks himself up, giggles,
//! and finds somebody new to stand next to. That is the boss fight — he
//! cannot be defeated, only launched, and the city hates him for free.
//!
//! Not a [`Pedestrian`]: he does not flee cars, keep to routes, stop for
//! chats or gawk at accidents, and giving him the component would enrol him
//! in all four. He walks his own line, which is other people's.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use rand::RngExt;

use super::figure::{Bare, FigureAssets, Head, Rest, WalkCycle};
use super::pedestrian::Pedestrian;
use super::steering::right_of;
use crate::audio::bank::SoundBank;
use crate::audio::{AudioRng, effect_gain, spatial_once};
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::mood::face::FaceAssets;
use crate::mood::feeling::{Mood, Temperament};
use crate::mood::provoke::{Provocation, Provoker, Rudeness};
use crate::mood::scuffle::Scuffle;
use crate::mood::voice::Voicebox;
use crate::player::on_foot::Player;
use crate::world::City;
use crate::world::buildings::SIDEWALK_HEIGHT;
use crate::world::texture::{glyph, painted_rect};

/// The same capsule the crowd wears; he has to fit through the same doors.
const RADIUS: f32 = 0.32;
const HEIGHT: f32 = 1.05;
const STAND: f32 = HEIGHT * 0.5 + RADIUS;

/// How often the city rolls for him when he is not resident, and the odds.
/// Together: he turns up every few minutes, which is exactly often enough.
const HAUNT_EVERY: f32 = 6.0;
const HAUNT_CHANCE: f32 = 0.22;

/// How far he can spot somebody worth standing next to.
const STALK_RANGE: f32 = 28.0;
/// How close he stands. Inside everybody's personal space, which is the
/// entire characterisation in one number.
const CREEP: f32 = 0.95;
const STALK_SPEED: f32 = 1.35;
const WANDER_SPEED: f32 = 1.1;

/// Seconds between eruptions, either side of the draw. The retch and gag
/// recordings run several seconds each, so the floor keeps one eruption
/// from starting inside the last — mostly.
const ERUPT: (f32, f32) = (3.2, 6.5);
const ERUPT_GAIN: f32 = 0.85;
const ERUPT_EARSHOT: f32 = 26.0;
/// How long the greeting bubble hangs over his head.
const GREETING_SECONDS: f32 = 1.8;

/// The man himself. At most one exists; `haunt` enforces it.
#[derive(Component, Default)]
pub struct CaptainErdnuss {
    /// The last soul he introduced himself to, so the greeting fires once
    /// per acquaintance rather than sixty times a second.
    greeted: Option<Entity>,
    /// Countdown to the next eruption.
    erupt: f32,
    /// Where he is ambling when nobody is worth standing next to.
    wander: Option<Vec2>,
    /// The speech bubble child, once `crown` has built it.
    bubble: Option<Entity>,
    /// Whether the peanut has been fitted yet. The figure is dressed by the
    /// same `figure::dress` as everybody (bald, so there is no cap to fight)
    /// and the head is swapped the frame after.
    crowned: bool,
}

/// The greeting, hanging over his head.
#[derive(Component)]
pub struct GreetingBubble {
    pub left: f32,
}

#[derive(Resource)]
struct CaptainAssets {
    lobe: Handle<Mesh>,
    eye: Handle<Mesh>,
    quad: Handle<Mesh>,
    shell: Handle<StandardMaterial>,
    ink: Handle<StandardMaterial>,
    bubble: Handle<StandardMaterial>,
}

pub struct CaptainPlugin;

impl Plugin for CaptainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(
            Update,
            (haunt, crown, stalk, fade_greetings)
                .chain()
                .in_set(GameSet::Ai)
                // He overrides nobody's intent but his own, but the scuffle
                // he so richly earns writes his `Bouncer` too, so he keeps
                // the same slot in the frame as the rest of the social layer.
                .after(super::pedestrian::Walking),
        );
    }
}

// ------------------------------------------------------------- the paint ----

/// Whether (u, v) in a text band lands on ink — the same 5×7 cells the
/// street signs use, which is why the greeting has no umlauts or
/// punctuation to offer. ERDNUSS needs neither.
fn on_ink(text: &[u8], u: f32, v: f32) -> bool {
    if !(0.0..1.0).contains(&v) {
        return false;
    }
    let cells = text.len() as f32;
    let column = u * cells;
    let index = column.floor();
    if index < 0.0 || index >= cells {
        return false;
    }
    let inside = (column - index - 0.14) / 0.72;
    if !(0.0..1.0).contains(&inside) {
        return false;
    }
    let rows = glyph(text[index as usize]);
    let bit = (inside * 5.0) as usize;
    let row = (v * 7.0) as usize;
    rows[row.min(6)] & (1 << (4 - bit.min(4))) != 0
}

/// The speech bubble: a rounded white field with the greeting on it.
fn bubble_image() -> Image {
    let text: &[u8] = b"ERDNUSS";
    painted_rect(256, 64, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        // Face coordinates with square units: the quad is 4:1.
        let p = Vec2::new((u - 0.5) * 4.0, v - 0.5);
        let half = Vec2::new(1.92, 0.44);
        let corner = 0.22;
        let q = (p.abs() - half + Vec2::splat(corner)).max(Vec2::ZERO);
        let d = q.length() - corner;
        if d > 0.0 {
            return [0, 0, 0, 0];
        }
        if d > -0.07 {
            return [32, 30, 34, 255];
        }
        let band_u = (u - 0.10) / 0.80;
        let band_v = (v - 0.20) / 0.60;
        if (0.0..1.0).contains(&band_u) && on_ink(text, band_u, band_v) {
            [28, 26, 30, 255]
        } else {
            [246, 243, 236, 255]
        }
    })
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let bubble = images.add(bubble_image());
    commands.insert_resource(CaptainAssets {
        lobe: meshes.add(Sphere::new(0.125)),
        eye: meshes.add(Sphere::new(0.02)),
        quad: meshes.add(Rectangle::new(1.0, 0.25)),
        // Peanut shell: matte, tan, unmistakable.
        shell: materials.add(StandardMaterial {
            base_color: Color::srgb(0.80, 0.62, 0.38),
            perceptual_roughness: 0.92,
            ..default()
        }),
        ink: materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.07, 0.07),
            perceptual_roughness: 0.5,
            ..default()
        }),
        bubble: materials.add(StandardMaterial {
            base_color_texture: Some(bubble),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            // Readable — well, visible — from behind too; a one-sided
            // greeting would blink out whenever he turned.
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
    });
}

// ----------------------------------------------------------- the systems ----

/// Keeps at most one of him in the world: despawned when the player leaves
/// him behind, rolled back in when the city has been safe for too long.
fn haunt(
    mut commands: Commands,
    time: Res<Time>,
    mut clock: Local<f32>,
    config: Res<GameConfig>,
    city: Res<City>,
    figures: Res<FigureAssets>,
    faces: Res<FaceAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: ResMut<AudioRng>,
    players: Query<&Transform, With<Player>>,
    captains: Query<(Entity, &Transform), With<CaptainErdnuss>>,
) {
    *clock += time.delta_secs();
    if *clock < HAUNT_EVERY {
        return;
    }
    *clock = 0.0;
    let Ok(player) = players.single() else { return };
    let focus = player.translation.xz();
    let crowd = &config.crowd;

    if let Ok((captain, at)) = captains.single() {
        if at.translation.xz().distance(focus) > crowd.despawn {
            commands.entity(captain).despawn();
        }
        return;
    }
    if rng.random::<f32>() > HAUNT_CHANCE {
        return;
    }

    // The same arrival ring the crowd uses: far enough not to pop in on
    // camera, near enough to be met within a stroll.
    let candidates: Vec<_> = city
        .graph
        .edges()
        .filter(|edge| {
            let midpoint = city
                .graph
                .node(edge.a)
                .pos
                .midpoint(city.graph.node(edge.b).pos);
            (crowd.spawn_min..crowd.spawn_max).contains(&midpoint.distance(focus))
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    let edge = candidates[rng.random_range(0..candidates.len())];
    let a = city.graph.node(edge.a).pos;
    let b = city.graph.node(edge.b).pos;
    let t: f32 = rng.random_range(0.2..0.8);
    let side = if rng.random::<f32>() < 0.5 { 1.0 } else { -1.0 };
    let position =
        a.lerp(b, t) + right_of((b - a).normalize_or_zero()) * side * (edge.width * 0.5 + 1.9);

    // Nothing moves him: the fuse is nearly nothing, the floor is near
    // delight, and he catches nobody's mood because he has never once
    // wondered what anybody else is feeling.
    let temper = Temperament {
        baseline: 0.6,
        fuse: 0.05,
        recovery: 0.9,
        contagion: 0.0,
        grudge: 0.0,
    };
    let worn = faces.wear(temper.baseline);
    // A long anonymous coat, the colour of something best walked past.
    let coat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.23, 0.24, 0.18),
        perceptual_roughness: 0.9,
        ..default()
    });

    let mut captain = commands.spawn((
        Name::new("Captain Erdnuss"),
        CaptainErdnuss::default(),
        Transform::from_xyz(position.x, SIDEWALK_HEIGHT + STAND + 0.1, position.y),
        RigidBody::Dynamic,
        Collider::capsule(RADIUS, HEIGHT),
        LockedAxes::ROTATION_LOCKED,
        Bouncer::new(STAND),
        temper,
        Mood::new(temper.baseline),
        // The lowest voice in the city. His giggle is the whole horror.
        Voicebox::new(0.72),
        Provoker::default(),
        Visibility::default(),
    ));
    // Dressed as a missionary purely for the bald head: there must be no
    // hair cap to fight the peanut. His own stream, so his trousers cannot
    // reshuffle anybody else's.
    let mut wardrobe = stream_for(config.world_seed, stream::CAPTAIN);
    super::figure::dress(
        &mut captain,
        &figures,
        coat,
        &worn,
        super::archetype::Archetype::Missionary,
        &mut wardrobe,
    );
}

/// Fits the peanut, the frame after `haunt` dressed him: the head child is
/// re-shelled into the lower lobe, the upper lobe and eyes are added, the
/// hands are gloved, and the greeting bubble is hung over the crown.
fn crown(
    mut commands: Commands,
    assets: Res<CaptainAssets>,
    mut captains: Query<(Entity, &mut CaptainErdnuss, &Children)>,
    mut heads: Query<
        (
            &mut Mesh3d,
            &mut MeshMaterial3d<StandardMaterial>,
            &mut Rest,
        ),
        With<Head>,
    >,
    joints: Query<&Children>,
    mut hands: Query<&mut MeshMaterial3d<StandardMaterial>, (With<Bare>, Without<Head>)>,
) {
    for (entity, mut captain, children) in &mut captains {
        if captain.crowned {
            continue;
        }
        captain.crowned = true;

        for &child in children {
            if let Ok((mut mesh, mut material, mut rest)) = heads.get_mut(child) {
                // The lower lobe takes the head's slot, so it squashes and
                // scales through the same `Rest` machinery as any head.
                mesh.0 = assets.lobe.clone();
                material.0 = assets.shell.clone();
                *rest = Rest::posed(Vec3::new(0.0, 0.58, 0.0), Vec3::new(1.0, 1.1, 0.95));
                continue;
            }
            // Gloves. Bare parts follow the mood's complexion on everybody
            // else, and emoji-yellow hands under a peanut read as a bug.
            let Ok(limb) = joints.get(child) else {
                continue;
            };
            for &part in limb {
                if let Ok(mut material) = hands.get_mut(part) {
                    material.0 = assets.ink.clone();
                }
            }
        }

        let mut bubble = None;
        commands.entity(entity).with_children(|parent| {
            // The upper lobe, waisted into the lower one.
            let upper = Vec3::new(0.0, 0.72, 0.0);
            parent.spawn((
                Rest::posed(upper, Vec3::new(0.75, 0.75, 0.72)),
                Mesh3d(assets.lobe.clone()),
                MeshMaterial3d(assets.shell.clone()),
                Transform::from_translation(upper),
            ));
            // Two small dark eyes on the upper lobe. No painted face: the
            // face system says how somebody feels, and he does not.
            for side in [-1.0f32, 1.0] {
                let at = Vec3::new(side * 0.038, 0.735, -0.082);
                parent.spawn((
                    Rest::at(at),
                    Mesh3d(assets.eye.clone()),
                    MeshMaterial3d(assets.ink.clone()),
                    Transform::from_translation(at),
                ));
            }
            // The greeting, hidden until he has somebody to greet. A quad
            // faces +Z and a figure faces −Z, hence the half turn.
            let at = Vec3::new(0.0, 1.04, 0.0);
            bubble = Some(
                parent
                    .spawn((
                        GreetingBubble { left: 0.0 },
                        Rest::at(at),
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(assets.bubble.clone()),
                        Transform::from_translation(at)
                            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                        Visibility::Hidden,
                    ))
                    .id(),
            );
        });
        captain.bubble = bubble;
    }
}

/// The whole of his social life: find the nearest person, stand far too
/// close, greet them once, and erupt at intervals.
fn stalk(
    time: Res<Time>,
    config: Res<GameConfig>,
    city: Res<City>,
    bank: Option<Res<SoundBank>>,
    mut commands: Commands,
    mut rng: ResMut<AudioRng>,
    mut provocations: MessageWriter<Provocation>,
    mut captains: Query<
        (
            Entity,
            &mut CaptainErdnuss,
            &mut Transform,
            &mut Bouncer,
            &mut WalkCycle,
            &LinearVelocity,
            &Voicebox,
        ),
        (
            Without<Launched>,
            Without<Scuffle>,
            Without<Pedestrian>,
            Without<Player>,
        ),
    >,
    citizens: Query<(Entity, &Transform), (With<Pedestrian>, Without<CaptainErdnuss>)>,
    players: Query<(Entity, &Transform), (With<Player>, Without<CaptainErdnuss>)>,
    mut bubbles: Query<(&mut GreetingBubble, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    for (entity, mut captain, mut transform, mut bouncer, mut cycle, velocity, voice) in
        &mut captains
    {
        let here = transform.translation;
        // The legs are paced off the body, exactly as the player's are:
        // nothing else paces a figure that is not a pedestrian.
        cycle.speed = velocity.0.xz().length();

        // Whoever is nearest, the player very much included.
        let victim = citizens
            .iter()
            .chain(players.iter())
            .map(|(who, at)| (at.translation.distance(here), who, at.translation))
            .filter(|(apart, ..)| *apart < STALK_RANGE)
            .min_by(|a, b| a.0.total_cmp(&b.0));

        match victim {
            Some((apart, who, there)) => {
                captain.wander = None;
                // A new acquaintance is greeted, once. ERDNUSS.
                if captain.greeted != Some(who) {
                    captain.greeted = Some(who);
                    if let Some(bubble) = captain.bubble
                        && let Ok((mut greeting, mut visibility)) = bubbles.get_mut(bubble)
                    {
                        greeting.left = GREETING_SECONDS;
                        *visibility = Visibility::Visible;
                    }
                    if let Some(bank) = bank.as_deref() {
                        commands.spawn((
                            AudioPlayer(bank.burp.clone()),
                            spatial_once(effect_gain(&config, ERUPT_GAIN), ERUPT_EARSHOT)
                                .with_speed(voice.pitch * 1.35),
                            Transform::from_translation(here + Vec3::Y * 0.6),
                        ));
                    }
                }

                let towards = (there - here).with_y(0.0);
                bouncer.desired = if apart > CREEP {
                    towards.normalize_or_zero().xz() * STALK_SPEED
                } else {
                    Vec2::ZERO
                };
                if let Ok(facing) = Dir2::new(towards.xz()) {
                    transform.rotation =
                        Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
                }
            }
            None => {
                // Nobody about: amble along the graph until there is.
                let goal = match captain.wander {
                    Some(goal) if goal.distance(here.xz()) > 3.0 => Some(goal),
                    _ => {
                        let next = city.graph.nearest_node(here.xz()).and_then(|node| {
                            let neighbours: Vec<_> =
                                city.graph.neighbors(node).map(|(id, _)| id).collect();
                            (!neighbours.is_empty()).then(|| {
                                let pick = neighbours[rng.random_range(0..neighbours.len())];
                                city.graph.node(pick).pos
                            })
                        });
                        captain.wander = next;
                        next
                    }
                };
                if let Some(goal) = goal {
                    let towards = goal - here.xz();
                    bouncer.desired = towards.normalize_or_zero() * WANDER_SPEED;
                    if let Ok(facing) = Dir2::new(towards) {
                        transform.rotation =
                            Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
                    }
                }
            }
        }

        // The eruption: one of the five, out loud, and out through the same
        // message every raspberry in the city uses — so the crowd sours,
        // grudges form, and the collar-grabbing takes care of itself.
        captain.erupt -= dt;
        if captain.erupt <= 0.0 {
            captain.erupt = rng.random_range(ERUPT.0..ERUPT.1);
            provocations.write(Provocation {
                by: entity,
                at: here,
                kind: Rudeness::Taunt,
            });
            if let Some(bank) = bank.as_deref() {
                let repertoire = [&bank.cough, &bank.burp, &bank.spit, &bank.retch, &bank.gag];
                let sound = repertoire[rng.random_range(0..repertoire.len())].clone();
                commands.spawn((
                    AudioPlayer(sound),
                    spatial_once(effect_gain(&config, ERUPT_GAIN), ERUPT_EARSHOT)
                        .with_speed(voice.pitch * rng.random_range(0.92..1.12)),
                    Transform::from_translation(here + Vec3::Y * 0.6),
                ));
            }
        }
    }
}

/// Counts the greeting down and hides the bubble when it has been said.
fn fade_greetings(time: Res<Time>, mut bubbles: Query<(&mut GreetingBubble, &mut Visibility)>) {
    for (mut greeting, mut visibility) in &mut bubbles {
        if greeting.left <= 0.0 {
            continue;
        }
        greeting.left -= time.delta_secs();
        if greeting.left <= 0.0 {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn he_stands_inside_everybodys_personal_space() {
        // The characterisation in one assert: he closes to within the
        // separation radius the whole crowd politely keeps.
        let crowd = GameConfig::default().crowd;
        assert!(CREEP < crowd.separation_radius + 0.1);
    }

    #[test]
    fn nothing_the_city_can_do_will_reach_him() {
        // A taunt lands scaled by the fuse and a cheer by the contagion;
        // both are dialled to nearly nothing, which is what makes him an
        // endgegner rather than a citizen: the mood game has no handle on
        // him. He can only be launched.
        let temper = Temperament {
            baseline: 0.6,
            fuse: 0.05,
            recovery: 0.9,
            contagion: 0.0,
            grudge: 0.0,
        };
        let tune = GameConfig::default().mood;
        let worst = crate::mood::provoke::sting(0.0, &temper, &tune).abs();
        assert!(worst < 0.03, "a taunt moved him by {worst}");
        let best = crate::mood::provoke::warmth(0.0, &temper, &tune);
        assert!(best < 0.15, "a cheer moved him by {best}");
    }

    #[test]
    fn the_greeting_is_painted_in_letters_the_font_can_actually_draw() {
        // The 5×7 font has digits and capitals only; a greeting with an
        // umlaut or an exclamation mark would paint as blank cells.
        for letter in b"ERDNUSS" {
            assert_ne!(glyph(*letter), [0u8; 7], "{} is blank", *letter as char);
        }
    }

    #[test]
    fn the_bubble_says_something_rather_than_nothing() {
        // Somewhere in the middle band of the bubble there must be ink, and
        // the corners must be transparent — the two failure modes of painted
        // texture code being a blank white card and an opaque billboard.
        let image = bubble_image();
        let data = image.data.as_ref().expect("painted images carry data");
        let texel = |x: u32, y: u32| {
            let at = ((y * 256 + x) * 4) as usize;
            [data[at], data[at + 1], data[at + 2], data[at + 3]]
        };
        assert_eq!(texel(2, 2)[3], 0, "the corner should be transparent");
        let mut inked = false;
        for x in 20..236 {
            let [r, _, _, a] = texel(x, 32);
            if a == 255 && r < 100 {
                inked = true;
                break;
            }
        }
        assert!(inked, "the greeting band is empty");
    }
}
