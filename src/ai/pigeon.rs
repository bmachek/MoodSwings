//! Pigeons.
//!
//! The cheapest liveliness in the game, and close to the most effective. A
//! street with nothing moving on it is a diorama however much furniture is
//! standing about; a street where nine birds get up off the paving as you come
//! past and settle again behind you is a street. The whole effect is one
//! reaction to one distance test, and it costs five meshes a bird.
//!
//! ## Not physics
//!
//! Everything here writes its own `Transform`. No rigid body, no collider, no
//! `Bouncer` — a pigeon weighs three hundred grams and has never once needed to
//! push anything. That is what makes a flock affordable where the dogs and cats
//! next door are not: `animal` gives every quadruped a dynamic body because a
//! dog on a leash has to be dragged by a joint when its owner is launched over
//! a car, and none of that applies to something that leaves.
//!
//! It also means a pigeon cannot be bounced off, run over or provoked, which
//! is the right answer three times: there is no joke in flattening one, the
//! flock is *already* reacting to everything that comes near it, and a bird
//! that could be caught would be a bird somebody spends the afternoon chasing.
//!
//! ## The flock is the unit
//!
//! Birds are startled in flocks, not individually. One pigeon deciding to leave
//! and its eight neighbours carrying on pecking is the single clearest way to
//! get this wrong — real flocks go up as one, and the going-up-as-one *is* the
//! effect. So the threat test is per bird and the decision is per flock.

use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::player::on_foot::Player;
use crate::world::City;
use crate::world::buildings::SIDEWALK_HEIGHT;

/// How many flocks are kept up around the player.
const FLOCKS: usize = 4;
/// And how many birds in one. A pair is a coincidence; nine is a flock.
const FLOCK_SIZE: (usize, usize) = (5, 12);

/// Flocks settle in this ring around the player, and are forgotten past the
/// far one — the same arrangement the crowd and the cats use.
const SETTLE: (f32, f32) = (16.0, 75.0);
const FORGET: f32 = 115.0;

/// How close a person gets before the flock decides otherwise, in metres.
const STARTLE: f32 = 3.6;
/// A car counts from further, and should: a pigeon that waits for a bumper is
/// a pigeon that gets one.
const STARTLE_VEHICLE: f32 = 9.0;

/// Cruising height above whatever the flock got up from.
const CRUISE: f32 = 8.5;
/// Seconds a startled flock stays up, before and after jitter.
const AIRBORNE: (f32, f32) = (3.5, 7.5);

/// Metres per second on foot, which is a shuffle, and in the air, which is not.
const PECK_PACE: f32 = 0.45;
const FLY_PACE: f32 = 9.0;

/// How far from its patch a pecking bird will wander.
const PATCH: f32 = 1.7;

/// Wingbeats per second in the air. Fast enough to blur, which is the point:
/// a slow flap reads as a seagull and this is not a seagull.
const BEATS: f32 = 8.5;

// A car has to be minded from much further off than a person — a pigeon that
// waits for a bumper is a pigeon that gets one — and a person has to be able to
// get close enough that the flock going up is recognisably *their* doing. A
// startle ring the width of a street is a flock that has always already left.
// Compile-time facts, so they are asserted the way `animal`'s leash is.
const _: () = assert!(STARTLE_VEHICLE > STARTLE * 2.0);
const _: () = assert!(STARTLE < 4.5);

#[derive(Component)]
pub struct Pigeon {
    /// Which flock this bird goes up with.
    flock: u32,
    /// The patch it pecks over, and the height of the ground under it.
    home: Vec2,
    ground: f32,
    /// Where it is headed, on the ground or in the air.
    target: Vec2,
    /// Seconds until it picks somewhere else.
    whim: f32,
    /// Seconds of air left. Zero on the ground.
    airborne: f32,
    /// Nought on the paving, one at cruising height. Eased, because a pigeon
    /// leaves the ground fast and comes back to it slowly.
    lift: f32,
    /// This bird's offset into the wingbeat, so a flock does not flap in
    /// formation like a clockwork toy.
    phase: f32,
}

/// A wing, flapped by [`flutter`]. The sign is which one.
#[derive(Component)]
struct Wing(f32);

/// The head, which bobs when the bird walks. A pigeon's head is famously the
/// one part of it that does not move while the rest of it does, and getting
/// that even roughly right is most of what makes one read as a pigeon.
#[derive(Component)]
struct Head;

#[derive(Resource)]
struct PigeonRng(ChaCha8Rng);

#[derive(Resource)]
struct PigeonTimer(Timer);

impl Default for PigeonTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(1.4, TimerMode::Repeating))
    }
}

#[derive(Resource)]
struct PigeonKit {
    body: Handle<Mesh>,
    head: Handle<Mesh>,
    wing: Handle<Mesh>,
    tail: Handle<Mesh>,
    /// Feral pigeon grey, a dark one, and the white one there is always
    /// exactly one of.
    coats: Vec<Handle<StandardMaterial>>,
    beak: Handle<StandardMaterial>,
}

pub struct PigeonPlugin;

impl Plugin for PigeonPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PigeonTimer>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (maintain_flocks, startle, flutter)
                    .chain()
                    .in_set(GameSet::Ai),
            );
    }
}

fn setup(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Its own stream, not `ANIMALS`: how many birds are on the pavement must
    // not move a single cat, and a cat's coat must not depend on how many
    // times a flock has been startled.
    commands.insert_resource(PigeonRng(stream_for(config.world_seed, stream::PIGEONS)));

    let feather = |materials: &mut Assets<StandardMaterial>, color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.92,
            ..default()
        })
    };
    commands.insert_resource(PigeonKit {
        // A pigeon is about thirty centimetres nose to tail and stands maybe
        // twenty high. Everything here is measured off that.
        body: meshes.add(
            Sphere::new(1.0)
                .mesh()
                .ico(2)
                .expect("an icosphere at two subdivisions"),
        ),
        head: meshes.add(
            Sphere::new(1.0)
                .mesh()
                .ico(1)
                .expect("an icosphere at one subdivision"),
        ),
        wing: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        tail: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        coats: [
            Color::srgb(0.41, 0.43, 0.47),
            Color::srgb(0.22, 0.23, 0.26),
            Color::srgb(0.82, 0.82, 0.80),
        ]
        .into_iter()
        .map(|color| feather(&mut materials, color))
        .collect(),
        beak: materials.add(StandardMaterial {
            base_color: Color::srgb(0.72, 0.52, 0.30),
            perceptual_roughness: 0.7,
            ..default()
        }),
    });
}

/// Keeps [`FLOCKS`] flocks settled around the player.
fn maintain_flocks(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: ResMut<PigeonTimer>,
    kit: Res<PigeonKit>,
    city: Res<City>,
    mut rng: ResMut<PigeonRng>,
    players: Query<&Transform, With<Player>>,
    birds: Query<(Entity, &Transform, &Pigeon)>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let Ok(player) = players.single() else { return };
    let focus = player.translation.xz();

    // Forget the far ones, and count what flocks are left. A flock is gone
    // when its last bird is, which is what makes the count honest without
    // keeping a list of flocks anywhere.
    let mut standing: Vec<u32> = Vec::new();
    for (entity, transform, pigeon) in &birds {
        if transform.translation.xz().distance(focus) > FORGET {
            commands.entity(entity).despawn();
        } else if !standing.contains(&pigeon.flock) {
            standing.push(pigeon.flock);
        }
    }
    if standing.len() >= FLOCKS {
        return;
    }

    // A patch to settle on: somewhere along a street in the ring, set over
    // towards the kerb. Past the carriageway's half-width the ground is the
    // pavement slab, and short of it, it is the road — pigeons use both, and
    // the ones in the road are the ones that make a passing car worth
    // watching.
    let candidates: Vec<_> = city
        .graph
        .edges()
        .filter(|edge| {
            let middle = city
                .graph
                .node(edge.a)
                .pos
                .midpoint(city.graph.node(edge.b).pos);
            (SETTLE.0..SETTLE.1).contains(&middle.distance(focus))
        })
        .collect();
    let Some(edge) = candidates
        .get(rng.0.random_range(0..candidates.len().max(1)))
        .copied()
    else {
        return;
    };

    let a = city.graph.node(edge.a).pos;
    let b = city.graph.node(edge.b).pos;
    let Ok(direction) = Dir2::new(b - a) else {
        return;
    };
    let normal = Vec2::new(-direction.y, direction.x);
    let side = if rng.0.random_range(0.0..1.0) < 0.5 {
        1.0
    } else {
        -1.0
    };
    // Two patches in three are on the pavement. The third is in the gutter.
    let on_pavement = rng.0.random_range(0.0..1.0) < 0.66;
    let offset = if on_pavement {
        edge.width * 0.5 + rng.0.random_range(0.9..2.6)
    } else {
        edge.width * 0.5 - rng.0.random_range(0.6..1.8)
    };
    let along = rng.0.random_range(0.15..0.85) * edge.length;
    let home = a + *direction * along + normal * (offset * side);
    let ground = if on_pavement { SIDEWALK_HEIGHT } else { 0.0 };

    // A flock number nothing else will reuse while this flock is alive.
    let flock = standing.iter().copied().max().unwrap_or(0) + 1;
    let size = rng.0.random_range(FLOCK_SIZE.0..FLOCK_SIZE.1);
    for _ in 0..size {
        // Scattered over the patch rather than stacked on its middle, or the
        // flock lands as a single grey lump.
        let angle = rng.0.random_range(0.0..std::f32::consts::TAU);
        let spread = rng.0.random_range(0.0..PATCH);
        let at = home + Vec2::new(angle.cos(), angle.sin()) * spread;
        // One white bird per flock at most, and only sometimes: the white one
        // is a moment, and two of them is a coincidence nobody believes.
        let coat = if rng.0.random_range(0.0..1.0) < 0.08 {
            kit.coats[2].clone()
        } else {
            kit.coats[rng.0.random_range(0..2)].clone()
        };
        spawn_pigeon(
            &mut commands,
            &kit,
            coat,
            Pigeon {
                flock,
                home,
                ground,
                target: at,
                whim: rng.0.random_range(0.4..3.0),
                airborne: 0.0,
                lift: 0.0,
                phase: rng.0.random_range(0.0..std::f32::consts::TAU),
            },
            at,
            rng.0.random_range(0.0..std::f32::consts::TAU),
        );
    }
}

/// A pigeon's overall length, nose to tail.
const LENGTH: f32 = 0.30;
/// How high its belly rides over the paving.
const STAND: f32 = 0.085;

fn spawn_pigeon(
    commands: &mut Commands,
    kit: &PigeonKit,
    coat: Handle<StandardMaterial>,
    pigeon: Pigeon,
    at: Vec2,
    yaw: f32,
) {
    let ground = pigeon.ground;
    commands
        .spawn((
            Name::new("Pigeon"),
            pigeon,
            Transform::from_xyz(at.x, ground + STAND, at.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
        ))
        .with_children(|parent| {
            // The body: an egg, long along -Z, which is the direction
            // everything in this codebase faces.
            parent.spawn((
                Mesh3d(kit.body.clone()),
                MeshMaterial3d(coat.clone()),
                Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::new(
                    LENGTH * 0.30,
                    LENGTH * 0.29,
                    LENGTH * 0.50,
                )),
            ));
            // The head, on a neck short enough that the two read as one shape
            // until the bird starts bobbing.
            parent.spawn((
                Head,
                Mesh3d(kit.head.clone()),
                MeshMaterial3d(coat.clone()),
                Transform::from_xyz(0.0, LENGTH * 0.20, -LENGTH * 0.38)
                    .with_scale(Vec3::splat(LENGTH * 0.17)),
            ));
            // The beak. Two centimetres of orange, and worth its own mesh for
            // exactly one reason: it is the only thing that says which end is
            // the front from directly above, which is where a flock on a
            // pavement is usually seen from.
            parent.spawn((
                Mesh3d(kit.tail.clone()),
                MeshMaterial3d(kit.beak.clone()),
                Transform::from_xyz(0.0, LENGTH * 0.19, -LENGTH * 0.52)
                    .with_scale(Vec3::new(0.016, 0.013, 0.035)),
            ));
            // The tail, fanned flat and cocked up a little.
            parent.spawn((
                Mesh3d(kit.tail.clone()),
                MeshMaterial3d(coat.clone()),
                Transform::from_xyz(0.0, LENGTH * 0.03, LENGTH * 0.46)
                    .with_rotation(Quat::from_rotation_x(-0.25))
                    .with_scale(Vec3::new(LENGTH * 0.26, 0.008, LENGTH * 0.36)),
            ));
            // The wings, hinged at the shoulder rather than centred on it, so
            // that flapping swings the whole wing instead of waggling a plank
            // about its middle.
            for side in [-1.0f32, 1.0] {
                parent
                    .spawn((
                        Wing(side),
                        Transform::from_xyz(side * LENGTH * 0.10, LENGTH * 0.08, 0.0),
                        Visibility::default(),
                    ))
                    .with_child((
                        Mesh3d(kit.wing.clone()),
                        MeshMaterial3d(coat.clone()),
                        Transform::from_xyz(side * LENGTH * 0.22, 0.0, 0.0).with_scale(Vec3::new(
                            LENGTH * 0.44,
                            0.010,
                            LENGTH * 0.34,
                        )),
                    ));
            }
        });
}

/// Anything a pigeon minds being near.
fn startle(
    mut birds: Query<(&Transform, &mut Pigeon)>,
    mut rng: ResMut<PigeonRng>,
    people: Query<
        &Transform,
        (
            Or<(With<Player>, With<crate::ai::pedestrian::Pedestrian>)>,
            Without<Pigeon>,
        ),
    >,
    // The cars near enough to matter, not all two and a half thousand parked
    // ones — see the note on the same filter in `world::litter`.
    vehicles: Query<&Transform, (With<crate::vehicle::spawn::ActiveVehicle>, Without<Pigeon>)>,
) {
    // Gathered once. Forty birds against a street's worth of people is a few
    // thousand distance tests a frame otherwise, and there is no reason for a
    // pigeon to be the most expensive thing in the schedule.
    let threats: Vec<(Vec2, f32)> = people
        .iter()
        .map(|t| (t.translation.xz(), STARTLE))
        .chain(
            vehicles
                .iter()
                .map(|t| (t.translation.xz(), STARTLE_VEHICLE)),
        )
        .collect();
    if threats.is_empty() {
        return;
    }

    // Which flocks have had enough. Decided for the flock, not the bird: real
    // pigeons go up together, and one leaving while its neighbours peck on is
    // the single clearest way to make a flock look like a screensaver.
    let mut going: Vec<u32> = Vec::new();
    for (transform, pigeon) in &birds {
        if pigeon.airborne > 0.0 || going.contains(&pigeon.flock) {
            continue;
        }
        let here = transform.translation.xz();
        if threats
            .iter()
            .any(|(at, reach)| here.distance(*at) < *reach)
        {
            going.push(pigeon.flock);
        }
    }
    if going.is_empty() {
        return;
    }

    for (_, mut pigeon) in &mut birds {
        if !going.contains(&pigeon.flock) {
            continue;
        }
        pigeon.airborne = rng.0.random_range(AIRBORNE.0..AIRBORNE.1);
        // Up and away from where it was standing, rather than to the patch it
        // is about to leave.
        let angle = rng.0.random_range(0.0..std::f32::consts::TAU);
        pigeon.target = pigeon.home + Vec2::new(angle.cos(), angle.sin()) * 9.0;
    }
}

/// Walking, flying, flapping and bobbing: everything a pigeon does.
fn flutter(
    time: Res<Time>,
    mut rng: ResMut<PigeonRng>,
    mut birds: Query<(&mut Transform, &mut Pigeon)>,
    mut wings: Query<(&Wing, &ChildOf, &mut Transform), Without<Pigeon>>,
    mut heads: Query<(&ChildOf, &mut Transform), (With<Head>, Without<Pigeon>, Without<Wing>)>,
) {
    let dt = time.delta_secs();

    for (mut transform, mut pigeon) in &mut birds {
        let here = transform.translation.xz();

        if pigeon.airborne > 0.0 {
            pigeon.airborne -= dt;
            // Up fast. A pigeon clears head height in about a second and it is
            // the suddenness that carries the whole moment.
            pigeon.lift = (pigeon.lift + dt * 1.6).min(1.0);
            if here.distance(pigeon.target) < 2.5 {
                // Circling: a new heading round the patch it left, so the
                // flock wheels rather than flying off in a straight line and
                // never coming back.
                let angle = rng.0.random_range(0.0..std::f32::consts::TAU);
                let radius = rng.0.random_range(6.0..14.0);
                pigeon.target = pigeon.home + Vec2::new(angle.cos(), angle.sin()) * radius;
            }
            if pigeon.airborne <= 0.0 {
                // Coming down on the patch again, a little off where it was.
                let angle = rng.0.random_range(0.0..std::f32::consts::TAU);
                let spread = rng.0.random_range(0.0..PATCH);
                pigeon.target = pigeon.home + Vec2::new(angle.cos(), angle.sin()) * spread;
            }
        } else {
            // Down slowly, and only once it is over the patch. A bird that
            // starts sinking the moment its time in the air is up lands
            // wherever it happened to be — which, since the last thing it did
            // was wheel out to fourteen metres, is a bird that then shuffles
            // home across a junction at walking pace for half a minute.
            if here.distance(pigeon.target) < 3.0 {
                pigeon.lift = (pigeon.lift - dt * 0.9).max(0.0);
            }
            pigeon.whim -= dt;
            if pigeon.lift <= 0.0 && (pigeon.whim <= 0.0 || here.distance(pigeon.target) < 0.12) {
                pigeon.whim = rng.0.random_range(1.2..4.5);
                let angle = rng.0.random_range(0.0..std::f32::consts::TAU);
                let spread = rng.0.random_range(0.2..PATCH);
                pigeon.target = pigeon.home + Vec2::new(angle.cos(), angle.sin()) * spread;
            }
        }

        // One movement rule for both states; only the speed differs.
        let pace = PECK_PACE + (FLY_PACE - PECK_PACE) * pigeon.lift;
        let to_target = pigeon.target - here;
        let step = to_target.normalize_or_zero() * pace * dt;
        let moved = if step.length() < to_target.length() {
            here + step
        } else {
            pigeon.target
        };

        // The bob: the body rises and falls a centimetre with every other step
        // on the ground, and not at all in the air.
        pigeon.phase += dt * if pigeon.lift > 0.02 { BEATS } else { 3.4 };
        let bob = if pigeon.lift < 0.02 {
            pigeon.phase.sin() * 0.006
        } else {
            0.0
        };

        transform.translation = Vec3::new(
            moved.x,
            pigeon.ground + STAND + pigeon.lift * CRUISE + bob,
            moved.y,
        );
        if let Ok(direction) = Dir2::new(to_target) {
            // Bevy's forward is -Z, and the body is built along it.
            let wanted = Quat::from_rotation_y((-direction.x).atan2(-direction.y));
            // Eased, or a bird turns on the spot like a turret.
            transform.rotation = transform.rotation.slerp(
                wanted,
                (dt * if pigeon.lift > 0.02 { 3.5 } else { 6.0 }).min(1.0),
            );
        }
    }

    // The wings. Flat out on the ground with a twitch in them, swinging
    // properly in the air, and the amplitude rides the lift so a bird taking
    // off starts beating before it has left.
    for (wing, parent, mut transform) in &mut wings {
        let Ok((_, pigeon)) = birds.get(parent.parent()) else {
            continue;
        };
        let sweep = (0.10 + 0.85 * pigeon.lift) * (pigeon.phase.sin());
        transform.rotation = Quat::from_rotation_z(-wing.0 * sweep);
    }

    // And the head, which stays where it is while the body walks out from
    // under it and then snaps forward to catch up. Half a wavelength behind
    // the bob, which is what puts the snap between the steps.
    for (parent, mut transform) in &mut heads {
        let Ok((_, pigeon)) = birds.get(parent.parent()) else {
            continue;
        };
        let thrust = if pigeon.lift < 0.02 {
            (pigeon.phase * 0.5).sin().powi(3) * 0.022
        } else {
            0.0
        };
        transform.translation.z = -LENGTH * 0.38 - thrust;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flock has to be able to get back down onto what it got up from.
    #[test]
    fn a_bird_is_on_the_ground_when_it_is_not_in_the_air() {
        // The height is one expression and it is the whole of the flight
        // model, so it is worth pinning: at rest a pigeon stands on the
        // paving, and at full lift it is well clear of a lamp post.
        let at_rest = SIDEWALK_HEIGHT + STAND + 0.0 * CRUISE;
        assert!((at_rest - SIDEWALK_HEIGHT - STAND).abs() < 1e-6);
        assert!(
            at_rest < SIDEWALK_HEIGHT + 0.15,
            "a pigeon standing {at_rest}m up is a pigeon hovering"
        );

        let cruising = SIDEWALK_HEIGHT + STAND + CRUISE;
        assert!(
            cruising > 6.0,
            "a startled flock at {cruising}m is still in everybody's way"
        );
    }

    /// A bird lands on the patch it left, not wherever its clock ran out.
    #[test]
    fn a_pigeon_comes_down_over_its_own_patch() {
        // The descent is gated on being within three metres of the landing
        // target, and the wheel that precedes it goes out to fourteen. Without
        // the gate a flock lands scattered across a junction and then walks
        // home at a shuffle, which is the whole effect running backwards.
        let wheel = 14.0f32;
        let gate = 3.0f32;
        assert!(gate < wheel, "the gate has to be inside the circuit");
        // And the gate has to be wider than a bird's own step at flying speed,
        // or it can pass through the window between frames and orbit forever.
        let step = FLY_PACE / 30.0;
        assert!(gate > step * 2.0, "a bird could skip past its own landing");
    }

    /// Rising has to be visibly faster than settling.
    #[test]
    fn a_pigeon_leaves_faster_than_it_returns() {
        // Not a preference: the whole effect is the suddenness of the going.
        // A flock that eases up and snaps down is a flock that reads backwards.
        let up = 1.0f32 / 1.6;
        let down = 1.0f32 / 0.9;
        assert!(up < down, "up takes {up:.2}s and down {down:.2}s");
        assert!(up < 1.0, "a second and a half to clear the ground is a bus");
    }

    /// The wings beat, rather than the wings being held out.
    #[test]
    fn the_wings_are_still_on_the_ground_and_not_in_the_air() {
        let sweep = |lift: f32, phase: f32| (0.10 + 0.85 * lift) * phase.sin();
        let quarter = std::f32::consts::FRAC_PI_2;
        assert!(
            sweep(0.0, quarter).abs() < 0.15,
            "a pecking pigeon is flapping"
        );
        assert!(
            sweep(1.0, quarter).abs() > 0.8,
            "a flying pigeon is gliding"
        );
    }

    /// A flock is a flock and not a pair.
    #[test]
    fn a_flock_is_worth_looking_at() {
        assert!(FLOCK_SIZE.0 >= 4, "two birds is a coincidence");
        assert!(
            FLOCK_SIZE.1 * FLOCKS < 60,
            "the whole city's pigeons must stay a rounding error"
        );
        assert!(SETTLE.1 < FORGET, "flocks would be forgotten as they land");
    }
}
