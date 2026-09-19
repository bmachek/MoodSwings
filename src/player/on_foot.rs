//! The player on foot — or rather, the player in the air, most of the time.
//!
//! Movement went through a floating character controller, which solves stairs,
//! kerbs and slopes by hovering the body a fixed distance above whatever is
//! beneath it. That is exactly the right answer for a city made of kerbs and
//! corners, and exactly the wrong one for a city made of rubber: a body held
//! off the ground by a spring never forms a contact, and restitution is a
//! property of a contact. The player could be declared as elastic as you like
//! and would still land like a sack.
//!
//! So the float is gone and [`crate::bounce::controller`] has the job instead.
//! It costs the free kerb handling — a hop clears a kerb rather than stepping
//! over one — which turns out to be the better trade, because clearing a kerb
//! by bouncing over it is the game.

use avian3d::prelude::*;
use bevy::prelude::*;
use leafwing_input_manager::prelude::ActionState;

use crate::bounce::controller::{Bouncer, JUMP_SCALE};
use crate::core::config::GameConfig;
use crate::core::schedule::GameSet;
use crate::core::settings::KeyBindings;
use crate::mood::face::FaceLevel;
use crate::mood::feeling::{Mood, Temperament};
use crate::mood::provoke::Provoker;
use crate::mood::voice::Voicebox;
use crate::player::camera::CameraRig;
use crate::player::input::Action;
use crate::world::City;
use crate::world::buildings::SIDEWALK_HEIGHT;

#[derive(Component)]
pub struct Player;

pub const CAPSULE_RADIUS: f32 = 0.38;
/// Length of the cylindrical section; total height is this plus two radii.
pub const CAPSULE_LENGTH: f32 = 1.05;
/// Distance from the capsule's centre to its lowest point.
pub const STAND_HEIGHT: f32 = CAPSULE_LENGTH / 2.0 + CAPSULE_RADIUS;

const SPRINT_SPEED: f32 = 7.6;
/// Fraction of top speed used when not sprinting.
const JOG_PACE: f32 = 0.62;

/// One coat per archetype, plus the default red, built once.
///
/// Not a micro-optimisation. `redress_player` runs on every click of the
/// Charakter screen and `spawn_player` on every start, and both used to call
/// `materials.add` — a fresh `StandardMaterial` each time, which nothing ever
/// frees. Sixteen archetypes and a default is a closed set, so the honest
/// shape is the one `ai::pedestrian::PedestrianAssets` already uses for the
/// crowd: build the wardrobe at startup and hand out handles.
#[derive(Resource)]
pub struct PlayerCoats {
    /// Indexed by the archetype's position in [`Archetype::ALL`].
    by_archetype: Vec<Handle<StandardMaterial>>,
    /// For an archetype with no coat of its own.
    default: Handle<StandardMaterial>,
}

impl PlayerCoats {
    fn for_character(
        &self,
        character: crate::ai::archetype::Archetype,
    ) -> Handle<StandardMaterial> {
        crate::ai::archetype::Archetype::ALL
            .iter()
            .position(|&a| a == character)
            .and_then(|i| self.by_archetype.get(i))
            .cloned()
            .unwrap_or_else(|| self.default.clone())
    }
}

fn build_player_coats(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(PlayerCoats {
        by_archetype: crate::ai::archetype::Archetype::ALL
            .iter()
            .map(|&a| materials.add(player_coat(a)))
            .collect(),
        default: materials.add(player_coat(crate::ai::archetype::Archetype::default())),
    });
}

pub struct OnFootPlugin;

impl Plugin for OnFootPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CharacterChosen>()
            // Startup, because `spawn_player` is PostStartup and reads it —
            // the same silently load-bearing ordering `FigureAssets` has.
            .add_systems(Startup, build_player_coats)
            .add_systems(PostStartup, spawn_player)
            .add_systems(
                Update,
                drive_player
                    .in_set(GameSet::Simulation)
                    // Ahead of the bouncer, which reads what this writes.
                    .before(crate::bounce::controller::bounce_bodies),
            )
            // Plain Update, outside the gated sets: the character screen is
            // part of the pause menu, and a re-dress that waited for
            // `InGameState::Playing` would fire only after the menu closed —
            // which happens to work, but showing the new costume the moment
            // it is clicked is what makes the screen feel like a mirror.
            .add_systems(Update, redress_player);
    }
}

/// The pause menu chose a new figure; the body needs new clothes.
#[derive(bevy::ecs::message::Message)]
pub struct CharacterChosen;

fn spawn_player(
    mut commands: Commands,
    config: Res<GameConfig>,
    city: Res<City>,
    coats: Res<PlayerCoats>,
    figures: Res<crate::ai::figure::FigureAssets>,
    keybindings: Res<KeyBindings>,
) {
    // Start on an actual street rather than at the origin, which is usually
    // inside a downtown block.
    let start = city
        .graph
        .nearest_node(Vec2::ZERO)
        .map(|id| city.graph.node(id).pos)
        .unwrap_or(Vec2::ZERO);

    let temper = Temperament::ordinary();
    let level = crate::mood::face::level_of(temper.baseline);

    let mut player = commands.spawn((
        Name::new("Player"),
        Player,
        Transform::from_xyz(start.x, SIDEWALK_HEIGHT + STAND_HEIGHT + 0.2, start.y),
        Visibility::default(),
        RigidBody::Dynamic,
        Collider::capsule(CAPSULE_RADIUS, CAPSULE_LENGTH),
        // Upright, and — unlike the crowd — upright even mid-launch: the car
        // still throws the player across the junction, but the camera is
        // bolted to this body and a view that cartwheels with it is motion
        // sickness rather than comedy. The crowd does the tumbling.
        LockedAxes::ROTATION_LOCKED,
        crate::bounce::launch::NeverTumbles,
        Bouncer::new(STAND_HEIGHT),
        // An ordinary citizen rather than a special case, so that the player's
        // own face sours in a bad-tempered crowd and cheers up in a good one.
        // Being subject to the mood is what makes it a toy rather than a gauge.
        temper,
        Mood::new(temper.baseline),
        FaceLevel(level),
        // Dead centre of the crowd's range: the player's voice is the one the
        // others are heard against.
        Voicebox::new(1.0),
        Provoker::default(),
        // The player carries the input map; everything else reads ActionState.
        Action::input_map(&keybindings),
    ));

    // The same figure the crowd wears, dressed as whoever the player chose to
    // be — in a third-person game the player is on screen more than anything
    // else, so the archetype's costume matters most on this body of all.
    let coat = coats.for_character(config.character);
    let mut rng = player_wardrobe_rng(&config);
    crate::ai::figure::dress(
        &mut player,
        &figures,
        coat,
        level,
        config.character,
        &mut rng,
    );
}

/// The chosen archetype's coat, or the default red jacket that reads at a
/// distance.
fn player_coat(character: crate::ai::archetype::Archetype) -> StandardMaterial {
    StandardMaterial {
        base_color: character.coat().unwrap_or(Color::srgb(0.62, 0.20, 0.17)),
        perceptual_roughness: 0.82,
        ..default()
    }
}

/// The player's own wardrobe stream: from the crowd's key, salted by the
/// chosen character, so trousers and hair are stable for a choice rather
/// than reshuffling on every re-dress.
fn player_wardrobe_rng(config: &GameConfig) -> rand_chacha::ChaCha8Rng {
    crate::core::rng::stream_for(
        config.world_seed ^ (config.character as u64) << 32,
        crate::core::rng::stream::CROWD,
    )
}

/// Re-dresses the player when the character screen picks somebody new: the
/// figure's children go, the same body stays — collider, mood, input and all.
fn redress_player(
    mut chosen: MessageReader<CharacterChosen>,
    mut commands: Commands,
    config: Res<GameConfig>,
    figures: Res<crate::ai::figure::FigureAssets>,
    coats: Res<PlayerCoats>,
    players: Query<(Entity, &Mood), With<Player>>,
) {
    if chosen.is_empty() {
        return;
    }
    chosen.clear();
    let Ok((player, mood)) = players.single() else {
        return;
    };
    let level = crate::mood::face::level_of(mood.value);
    commands.entity(player).despawn_related::<Children>();
    let coat = coats.for_character(config.character);
    let mut rng = player_wardrobe_rng(&config);
    let mut player = commands.entity(player);
    crate::ai::figure::dress(
        &mut player,
        &figures,
        coat,
        level,
        config.character,
        &mut rng,
    );
}

fn drive_player(
    time: Res<Time>,
    config: Res<GameConfig>,
    rigs: Query<&CameraRig>,
    mut players: Query<
        (&ActionState<Action>, &mut Bouncer, &mut Rotation),
        (With<Player>, Without<crate::player::interact::Driving>),
    >,
) {
    let Ok((action_state, mut bouncer, mut rotation)) = players.single_mut() else {
        return;
    };

    // Movement is camera-relative: pushing forward means "away from the
    // camera", which is what every third-person game trains players to expect.
    let yaw = rigs.single().map(|rig| rig.yaw).unwrap_or(0.0);
    let frame = Quat::from_rotation_y(yaw);
    let input = action_state.clamped_axis_pair(&Action::Move);
    // Preserve stick magnitude — a gentle tilt should be a stroll, not a jog —
    // but cap the length rather than normalising it. `VirtualDPad` has no
    // circle bound and `clamped_axis_pair` clamps each axis on its own, so
    // W+D hands this a vector of length √2 and the keyboard walks diagonally
    // forty per cent faster than it walks forwards. Normalising killed the
    // stick ramp; clamping keeps it and fixes the keyboard with it.
    let direction =
        (frame * Vec3::NEG_Z * input.y + frame * Vec3::X * input.x).clamp_length_max(1.0);

    let pace = if action_state.pressed(&Action::Sprint) {
        SPRINT_SPEED
    } else {
        SPRINT_SPEED * JOG_PACE
    };
    bouncer.desired = direction.xz() * pace;

    // Ease into travel, and hold the last heading when idle. Written here
    // rather than left to the solver because rotation is locked: nothing else
    // is going to turn the body, and a figure that walks sideways looks like a
    // bug rather than like a joke. Onto Avian's `Rotation` rather than the
    // `Transform`, which on an interpolated body is a teleport and costs the
    // walk itself — see `ai::steering::face_eased`.
    crate::ai::steering::face_eased(
        &mut rotation,
        direction.xz(),
        config.stroll.turn_ease,
        time.delta_secs(),
    );

    // The resting hop is set every frame — the controller spends the scale on
    // each landing, the same contract `ai::pedestrian` uses for the crowd. See
    // `BounceConfig::player_hop_scale` for why the player of all people
    // bounces least.
    // The chosen archetype scales it again — a player in the wheelchair
    // glides exactly as the crowd's wheelchair users do — and the gait
    // setting scales everybody: a walking city walks its player too.
    bouncer.hop_scale = config.bounce.player_hop_scale * config.character.hop() * config.gait.hop();

    // Only off the ground. Held down, this would otherwise be a pogo stick with
    // no ceiling: every landing would take the bigger hop, and each one lands
    // faster than the last. Absolute rather than scaled by the resting hop, so
    // dialling the walk-bounce down does not also cost jump height.
    if action_state.pressed(&Action::Jump) && bouncer.grounded {
        bouncer.hop_scale = JUMP_SCALE;
    }
}

#[cfg(test)]
mod tests {

    /// Two queries in one system may not both touch a component if either is
    /// mutable, and Bevy says so by panicking at first run. Several of these
    /// queries changed from `&mut Transform` to `&Transform` + `&mut Rotation`
    /// when the facing moved off the transform, which is exactly the edit that
    /// creates the overlap by accident.
    #[test]
    fn no_query_of_a_system_here_fights_another() {
        use crate::bounce::testing::initialises;
        initialises(drive_player);
    }
    use super::*;
    use crate::bounce::testing::{TICK, finish, ground, kerb, physics_app};
    use crate::core::config::Gait;
    use crate::world::buildings::SIDEWALK_HEIGHT;

    /// Steps physics without a window, so "does the character bounce on the
    /// ground or sink through it" is a test rather than something we squint at
    /// in a screenshot.
    ///
    /// Parametrised by [`Gait`], which it did not used to be, and the omission
    /// was the whole reason a hover survived a year of green tests: the harness
    /// left `Bouncer::hop_scale` at its constructed 1.0 and so measured the
    /// bouncing city, while `Gait::Walking` — the default since the gait became
    /// a setting — has been what the player actually sees. Everything below
    /// drives `hop_scale` the way `drive_player` does, once per tick.
    fn harness(gait: Gait, spawn_height: f32) -> (App, Entity) {
        let mut app = physics_app(TICK);
        app.add_systems(Update, crate::bounce::controller::bounce_bodies);
        ground(&mut app);

        let player = app
            .world_mut()
            .spawn((
                Player,
                Transform::from_xyz(0.0, spawn_height, 0.0),
                RigidBody::Dynamic,
                Collider::capsule(CAPSULE_RADIUS, CAPSULE_LENGTH),
                LockedAxes::ROTATION_LOCKED,
                Bouncer::new(STAND_HEIGHT),
            ))
            .id();
        finish(&mut app);
        app.world_mut().insert_resource(Chosen(gait));
        (app, player)
    }

    /// The gait this run is driving, so `tick` does not have to be told twice.
    #[derive(Resource, Clone, Copy)]
    struct Chosen(Gait);

    /// One frame, with the resting hop written the way `drive_player` writes
    /// it: every frame, because the controller spends the scale at every
    /// landing.
    fn tick(app: &mut App, player: Entity) {
        let gait = app.world().resource::<Chosen>().0;
        let scale = app.world().resource::<GameConfig>().bounce.player_hop_scale * gait.hop();
        app.world_mut()
            .get_mut::<Bouncer>(player)
            .unwrap()
            .hop_scale = scale;
        app.update();
    }

    fn settle(app: &mut App, player: Entity, ticks: usize) {
        for _ in 0..ticks {
            tick(app, player);
        }
    }

    fn height_of(app: &App, player: Entity) -> f32 {
        app.world().get::<Transform>(player).unwrap().translation.y
    }

    /// Highest and lowest the body gets over a span of ticks.
    fn envelope(app: &mut App, player: Entity, ticks: usize) -> (f32, f32) {
        let mut low = f32::MAX;
        let mut high = f32::MIN;
        for _ in 0..ticks {
            tick(app, player);
            let y = height_of(app, player);
            low = low.min(y);
            high = high.max(y);
        }
        (low, high)
    }

    #[test]
    fn a_dropped_player_lands_on_the_ground_rather_than_through_it() {
        for gait in Gait::ALL {
            let (mut app, player) = harness(gait, 6.0);
            settle(&mut app, player, 240);
            let (low, _) = envelope(&mut app, player, 120);
            assert!(
                low > STAND_HEIGHT - 0.2,
                "{gait:?} sank to {low}, below the soles at {STAND_HEIGHT}"
            );
        }
    }

    /// The other half of that, and the half nobody was asking for.
    ///
    /// "Not through the floor" was the only bound on the low point, so a body
    /// that never came within half a metre of the floor passed it. The rebound
    /// is *assigned*, so whatever slack the landing gate allows is a height a
    /// body hops off without touching anything — free, for ever. Measured at
    /// 41 cm before the landing moved to the contact.
    #[test]
    fn the_low_point_of_every_hop_is_the_pavement() {
        for gait in Gait::ALL {
            let (mut app, player) = harness(gait, 2.0);
            settle(&mut app, player, 240);
            let (low, _) = envelope(&mut app, player, 200);
            assert!(
                low < STAND_HEIGHT + 0.06,
                "{gait:?} bottoms out at {:.3}m above its own soles: it is bouncing off thin air",
                low - STAND_HEIGHT
            );
        }
    }

    #[test]
    fn a_player_standing_still_keeps_bouncing() {
        // The whole conceit of the game — in the gait that has it. Walking was
        // always meant to keep its feet down; see `Gait`.
        let (mut app, player) = harness(Gait::Bouncing, 2.0);
        settle(&mut app, player, 240);
        let (low, high) = envelope(&mut app, player, 120);
        assert!(
            high - low > 0.1,
            "only moved {:.3}m over two seconds; that is standing, not bouncing",
            high - low
        );
    }

    #[test]
    fn a_walking_player_settles_instead_of_bouncing() {
        let (mut app, player) = harness(Gait::Walking, 2.0);
        settle(&mut app, player, 300);
        let (low, high) = envelope(&mut app, player, 120);
        assert!(
            high - low < 0.03,
            "walking bobbed {:.3}m with nothing driving it",
            high - low
        );
        assert!(
            (low - STAND_HEIGHT).abs() < 0.05,
            "walking came to rest {:.3}m off the ground",
            low - STAND_HEIGHT
        );
    }

    /// A step off a kerb is a step, not a parachute descent.
    ///
    /// With the landing firing anywhere inside the probe's reach and the
    /// walking hop at zero, the fall was reassigned to nothing every frame and
    /// the body sank the last 28 cm at about 15 cm/s. The harness measured 2.8
    /// seconds; a free fall is a quarter of one.
    #[test]
    fn stepping_off_a_kerb_takes_about_as_long_as_falling_off_one() {
        for gait in Gait::ALL {
            let (mut app, player) = harness(gait, STAND_HEIGHT + SIDEWALK_HEIGHT);
            let mut arrived = None;
            for t in 0..192 {
                tick(&mut app, player);
                if arrived.is_none() && height_of(&app, player) <= STAND_HEIGHT + 0.05 {
                    arrived = Some(t);
                }
            }
            let ticks = arrived.unwrap_or(usize::MAX);
            assert!(
                ticks < 32,
                "{gait:?} took {ticks} ticks to fall {SIDEWALK_HEIGHT}m; gravity does it in 15"
            );
        }
    }

    /// And getting back up one.
    ///
    /// The hop used to do this for nothing, which is why the floating
    /// controller was given up in the first place. Walking has no hop, so the
    /// step has to be found and paid for, or a citizen who crosses a road can
    /// never get back onto the pavement — and a child, whose capsule meets the
    /// stone above its own hemisphere, cannot ride over it either.
    #[test]
    fn a_walking_body_picks_its_feet_up_for_a_kerb() {
        for stand in [STAND_HEIGHT, STAND_HEIGHT * 0.62] {
            let mut app = physics_app(TICK);
            app.add_systems(Update, crate::bounce::controller::bounce_bodies);
            ground(&mut app);
            kerb(&mut app, 2.0, SIDEWALK_HEIGHT);
            let radius = CAPSULE_RADIUS * (stand / STAND_HEIGHT);
            let length = CAPSULE_LENGTH * (stand / STAND_HEIGHT);
            let body = app
                .world_mut()
                .spawn((
                    Transform::from_xyz(0.0, stand + 0.05, 0.0),
                    RigidBody::Dynamic,
                    Collider::capsule(radius, length),
                    LockedAxes::ROTATION_LOCKED,
                    Bouncer::new(stand),
                ))
                .id();
            finish(&mut app);

            for _ in 0..256 {
                let mut bouncer = app.world_mut().get_mut::<Bouncer>(body).unwrap();
                // Walking pace, straight at the kerb, hop switched off.
                bouncer.hop_scale = 0.0;
                bouncer.desired = Vec2::new(1.5, 0.0);
                app.update();
            }
            let at = app.world().get::<Transform>(body).unwrap().translation;
            assert!(
                at.x > 2.4,
                "a body standing {stand:.2}m stalled at x={:.2}, short of the kerb at 2.0",
                at.x
            );
            assert!(
                at.y > stand + SIDEWALK_HEIGHT - 0.08,
                "a body standing {stand:.2}m is at y={:.2}, not up on the {SIDEWALK_HEIGHT}m kerb",
                at.y
            );
        }
    }

    /// The jump is the one thing on this body a player presses on purpose.
    ///
    /// It is spent at a landing, and a walking body never lands: it rests, and
    /// resting is not arriving. The bouncing city hid that because a bouncing
    /// body is never at rest, which is why every test here used to pass while
    /// the gait the player actually plays could not leave the ground.
    #[test]
    fn a_jump_leaves_the_ground_in_either_gait() {
        for gait in Gait::ALL {
            let (mut app, player) = harness(gait, 2.0);
            settle(&mut app, player, 300);
            // What this body gets to without being asked: standing, in the
            // walking gait; the top of an ordinary arc in the bouncing one.
            let (_, ordinary) = envelope(&mut app, player, 90);

            // Held, the way `drive_player` writes it while the key is down —
            // long enough to reach the ground, because a bouncing body spends
            // most of its time off it.
            let mut high = f32::MIN;
            for _ in 0..45 {
                app.world_mut()
                    .get_mut::<Bouncer>(player)
                    .unwrap()
                    .hop_scale = JUMP_SCALE;
                app.update();
                high = high.max(height_of(&app, player));
            }
            for _ in 0..150 {
                tick(&mut app, player);
                high = high.max(height_of(&app, player));
            }
            assert!(
                high > ordinary + 1.0,
                "{gait:?} jumped to {high:.2}m, and gets to {ordinary:.2}m without trying"
            );
        }
    }

    #[test]
    fn the_bounce_holds_its_height_instead_of_dying_away() {
        // Restitution alone would damp out within a second or two. The hop is
        // assigned rather than added precisely so that it does not.
        let (mut app, player) = harness(Gait::Bouncing, 2.0);
        settle(&mut app, player, 240);
        let (_, early) = envelope(&mut app, player, 90);
        settle(&mut app, player, 300);
        let (_, late) = envelope(&mut app, player, 90);
        assert!(
            (late - early).abs() < 0.15,
            "bounce apex drifted from {early:.2} to {late:.2}"
        );
    }

    /// Feeds the bouncer directly, the way `drive_player` does.
    fn walk(app: &mut App, player: Entity, direction: Vec2, ticks: usize) {
        for _ in 0..ticks {
            app.world_mut().get_mut::<Bouncer>(player).unwrap().desired = direction * SPRINT_SPEED;
            tick(app, player);
        }
    }

    #[test]
    fn travelling_moves_the_character_at_roughly_the_asked_for_speed() {
        for gait in Gait::ALL {
            let (mut app, player) = harness(gait, 2.0);
            settle(&mut app, player, 180);

            let start = app.world().get::<Transform>(player).unwrap().translation;
            let ticks = 128;
            walk(&mut app, player, Vec2::NEG_Y, ticks);
            let end = app.world().get::<Transform>(player).unwrap().translation;

            let travelled = (end - start).with_y(0.0).length();
            let seconds = ticks as f32 / 64.0;
            // Allow for the acceleration ramp at the start, and for the reduced
            // authority a bouncing body has while it is off the ground.
            let expected = SPRINT_SPEED * seconds;
            assert!(
                travelled > expected * 0.7,
                "{gait:?} covered only {travelled:.2}m in {seconds:.2}s, expected near {expected:.2}m"
            );
            assert!(
                (end.z - start.z) < -1.0,
                "{gait:?} moved the wrong way along Z: {start:?} -> {end:?}"
            );
        }
    }

    #[test]
    fn a_player_left_alone_stays_where_they_are() {
        // Bouncing on the spot must not wander. A body that drifts while nobody
        // is touching it walks itself into the traffic over a minute.
        for gait in Gait::ALL {
            let (mut app, player) = harness(gait, 2.0);
            settle(&mut app, player, 240);
            let before = app.world().get::<Transform>(player).unwrap().translation;
            settle(&mut app, player, 240);
            let after = app.world().get::<Transform>(player).unwrap().translation;
            assert!(
                (after.xz() - before.xz()).length() < 0.3,
                "{gait:?} drifted from {before:?} to {after:?} while standing still"
            );
        }
    }
}
