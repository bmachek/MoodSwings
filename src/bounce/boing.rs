//! The sound a city made of rubber makes.
//!
//! An impact is read off a sudden change in velocity rather than off a
//! collision event, which is the same rule `vehicle::damage` uses and for the
//! same reason: it catches every way a body can be stopped hard — a wall,
//! another flummi, a bumper, landing badly off a roof — through one code path
//! instead of one rule per collision pair.
//!
//! The threshold matters more than usual here. Everything bounces constantly,
//! so a threshold set too low turns the street into a bag of springs; set too
//! high and being launched across a junction is silent. It sits just above the
//! velocity a body loses to its own hop.
//!
//! The hop itself is invisible here by construction, not by threshold: the
//! bounce controller *assigns* the rebound at the bottom of every arc, and it
//! books that assignment into [`PreviousVelocity`] as it does so
//! (`bounce::controller`). What this module sees is only what the world did to
//! a body — a bumper, a wall, being thrown — never what the body did to
//! itself, which is what keeps a jump from reading as an assault.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::controller::Bouncer;
use crate::audio::bank::SoundBank;
use crate::audio::{effect_gain, spatial_once};
use crate::core::config::GameConfig;
use crate::core::schedule::GameSet;

/// Velocity change in one tick, in m/s, below which a knock is not worth a
/// sound. Above the give-and-take of an ordinary hop, below a real collision.
const WALLOP_FLOOR: f32 = 3.2;
/// The change that counts as being hit as hard as anything ever is. Louder
/// impacts than this exist; they do not sound any louder.
const WALLOP_FULL: f32 = 18.0;
/// How far a boing carries.
const EARSHOT: f32 = 20.0;
const GAIN: f32 = 0.8;

/// Somebody got hit. Written here and read by anything that cares how the city
/// is feeling about it.
#[derive(Message, Debug, Clone, Copy)]
pub struct Wallop {
    pub entity: Entity,
    pub position: Vec3,
    /// Velocity lost in the impact, in m/s.
    pub severity: f32,
}

/// Somebody came down hard on their own account.
///
/// The other half of the contract above. Everything a body does to itself is
/// booked out of [`Wallop`] by the bounce controller, which is what keeps a
/// jump from reading as an assault — and left the hardest landing in the game
/// silent, with `Bouncer::landing_speed` documented as the thing the boing is
/// pitched off and read by nobody. So a landing has a message of its own. It
/// makes a sound and it changes nobody's mood, because falling over is not an
/// insult, however much it looks like one.
#[derive(Message, Debug, Clone, Copy)]
pub struct Landed {
    pub entity: Entity,
    pub position: Vec3,
    /// Downward speed at the moment of contact, in m/s.
    pub speed: f32,
}

/// Arrival speed, in m/s, at which a landing is worth hearing.
///
/// Above the rhythm of a bouncing body, which arrives at its own hop speed and
/// would otherwise tap on every step, and above a kerb (2.3). A jump lands at
/// 7.3 and a roof at whatever the roof is worth.
const LANDING_FLOOR: f32 = 4.2;
/// And the arrival that rings as loud as this sound ever does.
const LANDING_FULL: f32 = 14.0;

/// Last tick's velocity, so a change in it can be spotted.
#[derive(Component, Default)]
pub struct PreviousVelocity(pub Vec3);

pub struct BoingPlugin;

impl Plugin for BoingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Wallop>()
            .add_message::<Landed>()
            .add_systems(
                Update,
                (spot_wallops, play_boings, play_landings)
                    .chain()
                    .in_set(GameSet::Simulation),
            );
    }
}

/// How hard a knock reads, 0 to 1. Pure, so the mix can be argued about
/// without a physics world.
pub fn wallop_strength(delta: f32) -> f32 {
    if delta < WALLOP_FLOOR {
        return 0.0;
    }
    ((delta - WALLOP_FLOOR) / (WALLOP_FULL - WALLOP_FLOOR)).clamp(0.05, 1.0)
}

/// How hard a landing reads, 0 to 1. The same shape as [`wallop_strength`] on
/// its own floor, because it is the same sound with a different cause.
pub fn landing_strength(speed: f32) -> f32 {
    if speed < LANDING_FLOOR {
        return 0.0;
    }
    ((speed - LANDING_FLOOR) / (LANDING_FULL - LANDING_FLOOR)).clamp(0.05, 1.0)
}

fn spot_wallops(
    mut commands: Commands,
    mut wallops: MessageWriter<Wallop>,
    mut bodies: Query<
        (
            Entity,
            &Transform,
            &LinearVelocity,
            Option<&mut PreviousVelocity>,
        ),
        With<Bouncer>,
    >,
) {
    for (entity, transform, velocity, previous) in &mut bodies {
        let Some(mut previous) = previous else {
            // First sight of this body. Seeding from its current velocity
            // rather than from zero stops a flummi that spawned in mid-air
            // yelping on the frame it appears.
            commands.entity(entity).insert(PreviousVelocity(velocity.0));
            continue;
        };
        let delta = (velocity.0 - previous.0).length();
        previous.0 = velocity.0;

        let severity = wallop_strength(delta);
        if severity > 0.0 {
            wallops.write(Wallop {
                entity,
                position: transform.translation,
                severity: delta,
            });
        }
    }
}

fn play_boings(
    mut commands: Commands,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    mut wallops: MessageReader<Wallop>,
) {
    for wallop in wallops.read() {
        let force = wallop_strength(wallop.severity);
        commands.spawn((
            AudioPlayer(bank.boing.clone()),
            spatial_once(effect_gain(&config, GAIN * force), EARSHOT)
                // Harder knocks ring lower and longer, the way a bigger ball
                // does. The range is wide because this is the sound the whole
                // game is built out of and it must not become a single note.
                .with_speed(1.35 - force * 0.55),
            Transform::from_translation(wallop.position),
        ));
    }
}

fn play_landings(
    mut commands: Commands,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    mut landings: MessageReader<Landed>,
) {
    for landing in landings.read() {
        let force = landing_strength(landing.speed);
        if force <= 0.0 {
            continue;
        }
        commands.spawn((
            AudioPlayer(bank.boing.clone()),
            // Quieter than a knock of the same size and pitched a little
            // higher: the body hitting the pavement is one surface, not two
            // flummis meeting.
            spatial_once(effect_gain(&config, GAIN * 0.7 * force), EARSHOT)
                .with_speed(1.5 - force * 0.55),
            Transform::from_translation(landing.position),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bounce::controller::{Bouncer, JUMP_SCALE, bounce_bodies};

    #[test]
    fn an_ordinary_hop_makes_no_noise() {
        // A flummi gives back a little under 3 m/s at the bottom of every
        // bounce. If that registered, the street would be a bag of springs.
        assert_eq!(wallop_strength(2.9), 0.0);
    }

    /// Wallops seen since the last look.
    #[derive(Resource, Default)]
    struct Heard(usize);

    fn count_wallops(mut wallops: MessageReader<Wallop>, mut heard: ResMut<Heard>) {
        heard.0 += wallops.read().count();
    }

    /// Landings loud enough to be worth a sound, since the last look.
    #[derive(Resource, Default)]
    struct Landings(usize);

    fn count_landings(mut landings: MessageReader<Landed>, mut heard: ResMut<Landings>) {
        heard.0 += landings
            .read()
            .filter(|landing| landing_strength(landing.speed) > 0.0)
            .count();
    }

    /// Builds a body on flat ground with the wallop detector watching it.
    ///
    /// `hop` is written every tick the way the owner of a body does it, so a
    /// test can ask for the bouncing city (1.0), the walking one (0.0) or a
    /// jump without rebuilding anything.
    fn watched(frame: f64) -> (App, Entity) {
        let mut app = crate::bounce::testing::physics_app(frame);
        app.init_resource::<Heard>();
        app.add_systems(Update, (bounce_bodies, spot_wallops, count_wallops).chain());
        crate::bounce::testing::ground(&mut app);
        let body = app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 2.0, 0.0),
                RigidBody::Dynamic,
                Collider::capsule(0.32, 1.05),
                LockedAxes::ROTATION_LOCKED,
                Bouncer::new(1.05 * 0.5 + 0.32),
            ))
            .id();
        crate::bounce::testing::finish(&mut app);
        (app, body)
    }

    fn hop_for(app: &mut App, body: Entity, hop: f32, ticks: usize) {
        for _ in 0..ticks {
            app.world_mut().get_mut::<Bouncer>(body).unwrap().hop_scale = hop;
            app.update();
        }
    }

    #[test]
    fn a_body_bouncing_and_jumping_under_its_own_steam_never_wallops_itself() {
        // The rebound at the bottom of every hop is *assigned* by the bounce
        // controller, and at 2×hop_speed per landing it is well over the
        // wallop floor — a jump lands past the outrage limit. If the detector
        // reads those assignments, the street boings on every step and a
        // player makes themselves furious by jumping, which is the bug this
        // test pins down. Only what the world does to a body may register.
        //
        // Both gaits, because they land differently: the bouncing one leaves
        // with its own hop, the walking one gives back the solver's
        // restitution instead, and neither is anything anybody did to it.
        for hop in [1.0f32, 0.0] {
            let (mut app, body) = watched(crate::bounce::testing::TICK);
            hop_for(&mut app, body, hop, 240);
            app.world_mut().resource_mut::<Heard>().0 = 0;

            // Five seconds on the spot.
            hop_for(&mut app, body, hop, 320);
            let resting = app.world().resource::<Heard>().0;
            assert_eq!(
                resting, 0,
                "{resting} wallops from a body at hop {hop} doing nothing"
            );

            // And a deliberate jump, landing included.
            hop_for(&mut app, body, JUMP_SCALE, 1);
            hop_for(&mut app, body, hop, 180);
            let jumping = app.world().resource::<Heard>().0;
            assert_eq!(
                jumping, 0,
                "{jumping} wallops from a jump at hop {hop} nobody was hit by"
            );
        }
    }

    /// A stutter is not an assault.
    ///
    /// Only the vertical assignment was booked, so on a frame long enough for
    /// the steering to close the whole gap to `desired` in one step — a chunk
    /// streaming in, which this repository logs at 80ms and up — a body that
    /// wanted to sprint knocked itself over. Measured: none at 16 and 50ms,
    /// one at 80 and 120.
    #[test]
    fn a_long_frame_is_not_a_knock() {
        for frame in [0.080f64, 0.120, 0.250] {
            let (mut app, body) = watched(crate::bounce::testing::TICK);
            hop_for(&mut app, body, 0.0, 240);
            app.world_mut().resource_mut::<Heard>().0 = 0;

            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(frame),
            ));
            {
                let mut bouncer = app.world_mut().get_mut::<Bouncer>(body).unwrap();
                bouncer.hop_scale = 0.0;
                // A sprint asked for from a standstill: the widest gap the
                // steering is ever handed.
                bouncer.desired = Vec2::new(7.6, 0.0);
            }
            app.update();
            let heard = app.world().resource::<Heard>().0;
            assert_eq!(
                heard,
                0,
                "a {:.0}ms frame walloped a body that was only trying to run",
                frame * 1000.0
            );
        }
    }

    /// A landing has a sound of its own, and the resting rhythm does not.
    #[test]
    fn a_fall_lands_audibly_and_a_hop_does_not() {
        let (mut app, body) = watched(crate::bounce::testing::TICK);
        app.init_resource::<Landings>();
        app.add_systems(Update, count_landings);
        hop_for(&mut app, body, 1.0, 240);
        app.world_mut().resource_mut::<Landings>().0 = 0;

        // Five seconds of the travelling bounce: the rhythm of the whole city,
        // and it must not tap on every step.
        hop_for(&mut app, body, 1.0, 320);
        let resting = app.world().resource::<Landings>().0;
        assert_eq!(resting, 0, "{resting} landings heard from an ordinary hop");

        // A deliberate jump, asked for once and spent once.
        app.world_mut().get_mut::<Bouncer>(body).unwrap().pending = Some(JUMP_SCALE);
        hop_for(&mut app, body, 1.0, 200);
        let jumped = app.world().resource::<Landings>().0;
        assert!(jumped >= 1, "a jump from 2.7m came down in silence");
    }

    /// A one-shot hop survives the writers that rewrite the scale every frame.
    #[test]
    fn a_pending_hop_is_spent_on_the_next_landing_however_long_that_takes() {
        let (mut app, body) = watched(crate::bounce::testing::TICK);
        hop_for(&mut app, body, 1.0, 240);
        let resting = height_reached(&mut app, body, 1.0, 90);

        app.world_mut().get_mut::<Bouncer>(body).unwrap().pending = Some(JUMP_SCALE);
        // `hop_for` writes the ordinary scale every tick, the way the pavement
        // AI does. Before `pending` existed that overwrote the trick, and a
        // landing falls in one frame out of twenty: the skater's ollie fired
        // about that often. The window is a jump's whole arc, which is two and
        // a half times an ordinary one.
        let jumped = height_reached(&mut app, body, 1.0, 150);
        assert!(
            jumped > resting + 0.5,
            "the one-shot reached {jumped:.2}m against an ordinary {resting:.2}m: it was overwritten"
        );

        // And exactly once: two arcs later it is an ordinary hop again.
        hop_for(&mut app, body, 1.0, 90);
        let after = height_reached(&mut app, body, 1.0, 90);
        assert!(
            after < resting + 0.3,
            "the one-shot was still going at {after:.2}m, an ordinary hop being {resting:.2}m"
        );
    }

    /// Highest the body gets over a span of ticks, with the ordinary scale
    /// written every one of them.
    fn height_reached(app: &mut App, body: Entity, hop: f32, ticks: usize) -> f32 {
        let mut high = f32::MIN;
        for _ in 0..ticks {
            app.world_mut().get_mut::<Bouncer>(body).unwrap().hop_scale = hop;
            app.update();
            high = high.max(app.world().get::<Transform>(body).unwrap().translation.y);
        }
        high
    }

    #[test]
    fn being_run_over_registers_at_full_force() {
        assert_eq!(wallop_strength(40.0), 1.0);
    }

    #[test]
    fn a_knock_just_over_the_floor_is_audible_rather_than_silent() {
        // Clamped away from zero on purpose: the first thing over the line
        // should be a quiet boing, not a muted one.
        assert!(wallop_strength(WALLOP_FLOOR + 0.01) > 0.0);
    }

    #[test]
    fn harder_knocks_read_as_harder() {
        assert!(wallop_strength(6.0) < wallop_strength(12.0));
    }
}
