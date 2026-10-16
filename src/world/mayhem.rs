//! Damage to the world — the only kind of damage this game has.
//!
//! Vehicles and people cannot be hurt; the *street* is fair game. Bolted-down
//! furniture that used to be an immovable post can be sheared off its footing
//! by a car arriving fast enough: the prop turns dynamic mid-frame and leaves
//! with a share of the car's speed, a hop and a tumble, so a parking meter
//! taken at 40 km/h cartwheels down the pavement instead of stopping the car
//! dead. A sheared hydrant additionally leaves a geyser behind — the oldest
//! joke in the open-world genre, and it is old because it works.
//!
//! None of it is permanent. A chunk that streams out takes its wreckage with
//! it and regenerates pristine, the same way the crowd's mood resets — the
//! city heals the moment you stop looking at it, which suits a game about
//! letting off steam better than a scarred save file would.
//!
//! The shear check is pure box arithmetic in the car's own frame, one frame
//! *proud* of the collider, for the same reason `bounce::launch::brushes` is:
//! the conversion has to land before the solver resolves a contact against
//! the static collider, or the car slams to a halt against a sign that was
//! about to give way.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::buildings::ChunkOf;
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::vehicle::spawn::Vehicle;
use crate::vehicle::spec::VehicleSpec;

/// How far outside a car's bodywork the shear check reaches, in metres.
/// Slightly more than a solver step at city speeds, so the conversion beats
/// the contact.
const SHEAR_REACH: f32 = 0.55;
/// Fraction of the car's speed a sheared prop leaves with.
const CARRY: f32 = 0.75;
/// Upward part of the send-off, in m/s. A sheared sign goes over the bonnet.
const POP_UP: f32 = 4.5;
/// Tumble handed to a sheared prop, in rad/s per m/s of car speed. High on
/// purpose: a slow graceful topple is a documentary, a cartwheel is a joke.
const TUMBLE: f32 = 0.55;

/// On a piece of street furniture that is bolted down but not forever.
///
/// `props` puts this on everything with a `Bolted` footing that a car should
/// be able to take off that footing — which is everything except the bollard,
/// whose whole job is stopping cars.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct Breakaway {
    /// Car speed, in m/s, below which the footing holds.
    pub at: f32,
    /// What the prop weighs once it is airborne, in kilograms.
    pub mass: f32,
    /// Whether shearing this leaves a geyser at the stump. True of exactly
    /// the hydrant.
    pub geyser: bool,
}

/// Something bolted just left its footing. The audio module plays the twang
/// off this rather than being called, so deleting the sound changes nothing,
/// and `mood::schadenfreude` reads the crowd's delight off it the same way.
#[derive(Message, Debug, Clone, Copy)]
pub struct PropSheared {
    pub position: Vec3,
    /// How fast whatever sheared it arrived, in m/s.
    pub speed: f32,
    /// What just went airborne, in kilograms. A parking meter at speed is
    /// funnier than a cone nudged over, and the listeners need the numbers to
    /// know the difference.
    pub mass: f32,
}

/// A sheared hydrant's stump, throwing water.
#[derive(Component)]
pub struct Geyser {
    pub life: Timer,
}

/// On a prop that [`shear`] sent flying. The pavement reads it as something
/// to run from while it is still moving fast enough to matter
/// (`CrowdConfig::flying_scare_speed`); the marker simply stays on afterwards,
/// because the speed filter on the reading side ages it out for free and a
/// system to take it off on settling would be a system for nothing.
#[derive(Component)]
pub struct SentFlying;

/// Below this, a geyser is too far gone to be worth running from or to throw
/// anybody: the main is giving out and the spray is a dribble.
pub const LIVE_PRESSURE: f32 = 0.2;

/// One thrown droplet. Animated by hand rather than given to the solver:
/// a geyser is a hundred of these a second and none of them needs to push
/// anything, only to fly up and come back down.
#[derive(Component)]
struct Droplet {
    velocity: Vec3,
}

/// The droplet mesh and water material, shared by every geyser, plus the spawn
/// metronome. Same pattern as the old smoke assets: one mesh, one material,
/// however many droplets are up.
#[derive(Resource)]
pub struct GeyserAssets {
    timer: Timer,
    droplet: Handle<Mesh>,
    water: Handle<StandardMaterial>,
}

/// Runtime chaos gets its own stream. Droplet jitter is not world generation —
/// it depends on what the player crashed into — but it must still not draw
/// from a generation stream, or knocking over a hydrant would reshuffle props.
#[derive(Resource)]
struct MayhemRng(ChaCha8Rng);

pub struct MayhemPlugin;

impl Plugin for MayhemPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PropSheared>()
            .add_systems(Startup, setup_assets)
            .add_systems(
                Update,
                (shear_props, erupt_geysers, toss_over_geysers, fly_droplets)
                    .chain()
                    .in_set(GameSet::Simulation),
            );
    }
}

fn setup_assets(
    mut commands: Commands,
    config: Res<crate::core::config::GameConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(GeyserAssets {
        timer: Timer::from_seconds(0.035, TimerMode::Repeating),
        droplet: meshes.add(Sphere::new(0.075)),
        // Unlit and translucent: at the speed a droplet crosses the screen it
        // is a streak of colour, and lighting a streak buys nothing.
        water: materials.add(StandardMaterial {
            base_color: Color::srgba(0.55, 0.75, 0.95, 0.65),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    });
    commands.insert_resource(MayhemRng(stream_for(config.world_seed, stream::MAYHEM)));
}

/// The send-off, in world space: what a prop sheared by a car travelling at
/// `velocity` leaves with, given where it stood relative to the car.
///
/// Pure, so the choreography is testable. Linear: most of the car's speed,
/// plus the hop. Angular: a tumble about the axis perpendicular to travel —
/// the axis a real object clipped at the ankle actually rotates about — with
/// the sign chosen by which side of the car it was on, so two meters taken
/// with both wings go spinning in opposite directions.
pub fn send_off(velocity: Vec3, offset: Vec3) -> (Vec3, Vec3) {
    let speed = velocity.length();
    let carried = velocity * CARRY + Vec3::Y * POP_UP;
    let along = velocity / speed.max(1e-4);
    // Perpendicular to travel, horizontal: the cartwheel axis.
    let axle = Vec3::Y.cross(along);
    let side = offset.dot(axle).signum();
    let tumble = axle * (speed * TUMBLE) + Vec3::Y * (side * speed * TUMBLE * 0.4);
    (carried, tumble)
}

fn shear_props(
    mut commands: Commands,
    mut sheared: MessageWriter<PropSheared>,
    vehicles: Query<(&Transform, &LinearVelocity, &VehicleSpec), With<Vehicle>>,
    props: Query<(Entity, &Transform, &Breakaway, Option<&ChunkOf>)>,
) {
    // The slowest shear threshold in use; below it no car can take anything
    // off its footing and the whole pass can be skipped.
    let moving: Vec<_> = vehicles
        .iter()
        .filter(|(_, velocity, _)| velocity.length_squared() > 3.0 * 3.0)
        .collect();
    if moving.is_empty() {
        return;
    }

    for (prop, transform, breakaway, chunk) in &props {
        for (car, velocity, spec) in &moving {
            let speed = velocity.length();
            if speed < breakaway.at {
                continue;
            }
            let offset = transform.translation - car.translation;
            let local = car.rotation.inverse() * offset;
            if local.x.abs() > spec.half_extents.x + SHEAR_REACH
                || local.z.abs() > spec.half_extents.z + SHEAR_REACH
                || local.y.abs() > spec.half_extents.y + 2.5
            {
                continue;
            }

            shear(
                &mut commands,
                &mut sheared,
                prop,
                transform,
                breakaway,
                chunk,
                velocity.0,
                offset,
            );
            break;
        }
    }
}

/// Takes one bolted prop off its footing: sends it flying, announces it, and
/// leaves the plumbing behind if there was any.
///
/// Public because the capture harness needs to break a hydrant on demand —
/// waiting for the traffic to find one is not a screenshot. `offset` is the
/// prop's position relative to whatever hit it; it decides which way the
/// tumble spins.
pub fn shear(
    commands: &mut Commands,
    sheared: &mut MessageWriter<PropSheared>,
    prop: Entity,
    transform: &Transform,
    breakaway: &Breakaway,
    chunk: Option<&ChunkOf>,
    velocity: Vec3,
    offset: Vec3,
) {
    let (carried, tumble) = send_off(velocity, offset);
    commands.entity(prop).remove::<Breakaway>().insert((
        RigidBody::Dynamic,
        Mass(breakaway.mass),
        LinearVelocity(carried),
        AngularVelocity(tumble),
        SentFlying,
    ));
    sheared.write(PropSheared {
        position: transform.translation,
        speed: velocity.length(),
        mass: breakaway.mass,
    });

    if breakaway.geyser {
        let mut fountain = commands.spawn((
            Name::new("Geyser"),
            Geyser {
                life: Timer::from_seconds(45.0, TimerMode::Once),
            },
            // Water comes out of the broken main, which is at ground
            // level whatever height the hydrant's centre stood at.
            Transform::from_translation(transform.translation.with_y(0.1)),
            Visibility::default(),
        ));
        // The hydrant's chunk, so the plumbing streams out with the
        // street it broke on rather than spraying an empty void.
        if let Some(ChunkOf(chunk)) = chunk {
            fountain.insert(ChunkOf(*chunk));
        }
    }
}

/// How hard a geyser is still throwing water, 0 to 1, over its life.
///
/// Full pressure for most of it, then the main gives out over the last
/// quarter — a fountain that faded linearly from the first second would spend
/// most of its life looking broken rather than glorious.
pub fn pressure(elapsed: f32) -> f32 {
    ((1.0 - elapsed) / 0.25).clamp(0.0, 1.0)
}

/// The water's half of a geyser throw: straight up, at a pressure-scaled
/// speed, plus a little shove off the stump so the arc lands beside it rather
/// than straight back on. Pure so the bop line can be argued in a test;
/// `offset` is the victim's position relative to the stump, flattened.
///
/// Never below 60% of full send while there is any pressure at all: a weak
/// little hop off a dying geyser reads as a stumble, not a ride.
pub fn toss(head: f32, speed: f32, offset: Vec3) -> Vec3 {
    if head < LIVE_PRESSURE {
        return Vec3::ZERO;
    }
    // Deterministic from the offset, so no stream is needed. Somebody dead
    // centre goes straight up.
    let aside = offset.with_y(0.0).normalize_or_zero() * TOSS_ASIDE;
    Vec3::Y * speed * (0.6 + 0.4 * head) + aside
}

/// Sideways part of a geyser throw, in m/s.
const TOSS_ASIDE: f32 = 1.2;

/// The geyser's own verb: a flummi who blunders onto a live stump goes up
/// with the water.
///
/// The throw keeps whatever the body was doing across the ground and replaces
/// what it was doing vertically. From a standing or walking body, then, the
/// velocity change the wallop detector sees is the water's own vector — held
/// under `MoodConfig::bop_limit` by a test, so the throw reads as a friendly
/// bop (a hopping body caught on its way down adds its fall, and can tip just
/// over). The landing does not read as a bop at all: a body coming back down
/// from nearly three metres hits the pavement at nine or ten metres a second
/// of lost velocity, and `feel_wallops` books that at full fuse. So the
/// ride's delight is handed over here, explicitly, as
/// `SchadenfreudeConfig::geyser_glee` — enough to leave most of the street
/// happier for the trip and still not enough to cheer a Wutbürger up, who is
/// insulted by the landing as he is by everything. A headless test rides the
/// whole thing, knock by knock, and holds both halves of that.
///
/// `launch` is the one way off anybody's feet, so `wheee` plays off the
/// `KnockedDown` it inserts, the steadfast stay upright, and the knockdown
/// window is also why this cannot machine-gun anybody: no second throw until
/// they are back on their feet — at which point landing back on the stump goes
/// up again, which is the joke.
fn toss_over_geysers(
    mut commands: Commands,
    config: Res<crate::core::config::GameConfig>,
    geysers: Query<(&Geyser, &Transform)>,
    mut victims: Query<
        (
            Entity,
            &Transform,
            &mut LinearVelocity,
            Option<&mut crate::mood::feeling::Mood>,
            Has<crate::bounce::launch::NeverTumbles>,
        ),
        (
            With<crate::bounce::controller::Bouncer>,
            Without<Vehicle>,
            Without<crate::player::interact::Driving>,
            Without<crate::bounce::launch::KnockedDown>,
        ),
    >,
) {
    let tune = &config.schadenfreude;
    let reach = tune.geyser_toss_radius * tune.geyser_toss_radius;
    for (geyser, stump) in &geysers {
        let head = pressure(geyser.life.fraction());
        if head < LIVE_PRESSURE {
            continue;
        }
        for (victim, transform, mut velocity, mood, steadfast) in &mut victims {
            let offset = (transform.translation - stump.translation).with_y(0.0);
            if offset.length_squared() > reach {
                continue;
            }
            let throw = velocity.0.with_y(0.0) + toss(head, tune.geyser_toss, offset);
            crate::bounce::launch::launch(&mut commands, victim, &mut velocity, throw, !steadfast);
            if let Some(mut mood) = mood {
                mood.value = (mood.value + tune.geyser_glee).clamp(-1.0, 1.0);
            }
        }
    }
}

fn erupt_geysers(
    mut commands: Commands,
    time: Res<Time>,
    mut assets: ResMut<GeyserAssets>,
    mut rng: ResMut<MayhemRng>,
    mut geysers: Query<(Entity, &mut Geyser, &Transform)>,
) {
    let spawn = assets.timer.tick(time.delta()).just_finished();

    for (entity, mut geyser, transform) in &mut geysers {
        geyser.life.tick(time.delta());
        if geyser.life.is_finished() {
            commands.entity(entity).despawn();
            continue;
        }
        if !spawn {
            continue;
        }

        let head = pressure(geyser.life.fraction());
        for _ in 0..3 {
            // A tight upward cone. The lateral jitter is what turns a line of
            // spheres into spray.
            let sway = Vec3::new(
                rng.0.random_range(-1.2..1.2),
                rng.0.random_range(11.0..15.0) * (0.4 + 0.6 * head),
                rng.0.random_range(-1.2..1.2),
            );
            commands.spawn((
                Droplet { velocity: sway },
                Mesh3d(assets.droplet.clone()),
                MeshMaterial3d(assets.water.clone()),
                Transform::from_translation(transform.translation)
                    // Stretched along the way it flies, so a sphere reads as
                    // a gout of water rather than a bead.
                    .with_scale(Vec3::new(1.0, 2.6, 1.0)),
            ));
        }
    }
}

fn fly_droplets(
    mut commands: Commands,
    time: Res<Time>,
    mut droplets: Query<(Entity, &mut Droplet, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut droplet, mut transform) in &mut droplets {
        droplet.velocity.y -= 9.81 * dt;
        transform.translation += droplet.velocity * dt;
        // Into the pavement, and gone. No splash: at this droplet rate the
        // next three are already where the splash would have been.
        if transform.translation.y < 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_query_of_a_system_here_fights_another() {
        use crate::bounce::testing::initialises;
        initialises(toss_over_geysers);
    }

    #[test]
    fn a_geyser_throw_lands_on_its_victim_as_a_bop_rather_than_an_insult() {
        // What the wallop detector sees is the change in velocity, and the
        // throw is added to whatever the body was doing — so the water's own
        // vector is the whole of it. Full pressure is the hardest there is,
        // and the shove off the stump counts too.
        let config = crate::core::config::GameConfig::default();
        for offset in [Vec3::ZERO, Vec3::X * 0.4, Vec3::new(-0.8, 0.3, 0.7)] {
            let hardest = toss(1.0, config.schadenfreude.geyser_toss, offset);
            assert!(
                hardest.length() < config.mood.bop_limit,
                "a {:.2} m/s throw against a bop limit of {}",
                hardest.length(),
                config.mood.bop_limit
            );
        }
    }

    /// Every knock a body takes from the moment a full-pressure geyser throws
    /// it until it has come to rest, as the wallop detector sees them.
    ///
    /// Through the real `toss_over_geysers`, not a hand-written launch: the
    /// detector reads velocity changes, and how the throw is composed with
    /// whatever the body was already doing is exactly the thing in question.
    fn knocks_from_a_ride(hop: f32) -> Vec<f32> {
        use crate::bounce::boing::{Wallop, spot_wallops};
        use crate::bounce::controller::{Bouncer, bounce_bodies};
        use crate::bounce::testing::{TICK, finish, ground, physics_app};

        #[derive(Resource, Default)]
        struct Knocks(Vec<f32>);
        fn listen(mut wallops: MessageReader<Wallop>, mut knocks: ResMut<Knocks>) {
            knocks
                .0
                .extend(wallops.read().map(|wallop| wallop.severity));
        }

        let mut app = physics_app(TICK);
        app.init_resource::<Knocks>();
        app.add_systems(
            Update,
            (bounce_bodies, toss_over_geysers, spot_wallops, listen).chain(),
        );
        ground(&mut app);
        let stand = crate::ai::pedestrian::STAND_HEIGHT;
        let body = app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, stand, 0.0),
                RigidBody::Dynamic,
                Collider::capsule(crate::ai::pedestrian::RADIUS, crate::ai::pedestrian::HEIGHT),
                LockedAxes::ROTATION_LOCKED,
                Bouncer::new(stand),
            ))
            .id();
        finish(&mut app);
        let walk = |app: &mut App, ticks: usize| {
            for _ in 0..ticks {
                app.world_mut().get_mut::<Bouncer>(body).unwrap().hop_scale = hop;
                app.update();
            }
        };
        walk(&mut app, 240);
        app.world_mut().resource_mut::<Knocks>().0.clear();

        // A fresh main, a step off-centre: the stump the body blundered onto.
        let at = app.world().get::<Transform>(body).unwrap().translation;
        app.world_mut().spawn((
            Geyser {
                life: Timer::from_seconds(45.0, TimerMode::Once),
            },
            Transform::from_translation(at.with_y(0.1) + Vec3::X * 0.3),
        ));
        walk(&mut app, 240);
        app.world().resource::<Knocks>().0.clone()
    }

    #[test]
    fn a_geyser_ride_cheers_up_most_of_the_street_and_not_the_wutburger() {
        // The throw is a bop and the landing is not: a body coming back down
        // from nearly three metres arrives harder than `bop_limit`, and
        // `feel_wallops` books that at full fuse. So the ride's delight is
        // handed over by hand — and this is the whole ride, every knock of it,
        // against what each kind of citizen makes of it. Both gaits, because
        // a hopping body can be thrown on its way down, which is a bigger
        // change in velocity than being thrown from a standstill.
        use crate::mood::feeling::{Temperament, jolt};
        let config = crate::core::config::GameConfig::default();
        for gait in crate::core::config::Gait::ALL {
            let knocks = knocks_from_a_ride(gait.hop());
            assert!(
                knocks.len() >= 2,
                "{gait:?}: a throw and a landing should both register, got {knocks:?}"
            );
            let ride = |temper: &Temperament| {
                config.schadenfreude.geyser_glee
                    + knocks
                        .iter()
                        .map(|severity| jolt(*severity, temper, &config.mood))
                        .sum::<f32>()
            };
            for temper in [
                Temperament::serene(),
                Temperament::easygoing(),
                Temperament::ordinary(),
            ] {
                assert!(
                    ride(&temper) > 0.0,
                    "{gait:?}: the {} came off the geyser {:+.2} for knocks {knocks:?}",
                    temper.name(),
                    ride(&temper)
                );
            }
            let rager = ride(&Temperament::ragemonger());
            assert!(
                rager < 0.0,
                "{gait:?}: a Wutbürger enjoyed the geyser ({rager:+.2}), knocks {knocks:?}"
            );
        }
    }

    #[test]
    fn a_dying_geyser_throws_nobody_and_a_live_one_gives_a_proper_ride() {
        let speed = 5.8;
        assert_eq!(toss(0.0, speed, Vec3::X), Vec3::ZERO);
        assert_eq!(toss(LIVE_PRESSURE * 0.9, speed, Vec3::X), Vec3::ZERO);
        // Any pressure worth the name still sends somebody up properly rather
        // than tipping them off the stump.
        assert!(toss(LIVE_PRESSURE, speed, Vec3::ZERO).y >= speed * 0.6);
        // Dead centre goes straight up; anywhere else lands beside the stump.
        assert_eq!(toss(1.0, speed, Vec3::ZERO).xz(), Vec2::ZERO);
        assert!(toss(1.0, speed, Vec3::X * 0.5).x > 0.0);
    }

    #[test]
    fn a_sheared_prop_leaves_with_the_car_rather_than_against_it() {
        let velocity = Vec3::new(0.0, 0.0, -14.0);
        let (carried, _) = send_off(velocity, Vec3::X);
        assert!(
            carried.z < -8.0,
            "the prop should carry most of the car's speed: {carried:?}"
        );
        assert!(carried.y > 3.0, "and go over the bonnet, not under it");
    }

    #[test]
    fn a_sheared_prop_cartwheels_rather_than_topples() {
        let velocity = Vec3::new(12.0, 0.0, 0.0);
        let (_, tumble) = send_off(velocity, Vec3::Z);
        // The cartwheel axis for travel along +X is ±Z.
        assert!(
            tumble.z.abs() > 2.0,
            "12 m/s should be a proper cartwheel, got {tumble:?}"
        );
    }

    #[test]
    fn props_taken_with_opposite_wings_spin_opposite_ways() {
        let velocity = Vec3::new(0.0, 0.0, -10.0);
        let (_, left) = send_off(velocity, Vec3::new(-1.0, 0.0, 0.0));
        let (_, right) = send_off(velocity, Vec3::new(1.0, 0.0, 0.0));
        assert!(
            left.y.signum() != right.y.signum(),
            "both wings spun the same way: {left:?} vs {right:?}"
        );
    }

    #[test]
    fn a_geyser_holds_its_pressure_and_then_dies() {
        assert_eq!(pressure(0.0), 1.0, "a fresh main is at full pressure");
        assert_eq!(pressure(0.5), 1.0, "and stays there for most of its life");
        assert!(
            pressure(0.9) < 0.5,
            "the last quarter is where it gives out"
        );
        assert_eq!(pressure(1.0), 0.0);
    }
}
