//! Dogs and cats: the city's first quadrupeds.
//!
//! The figure machinery generalises further than it knew. A quadruped here is
//! four [`Limb`]-carrying leg joints under a body — and because the walk
//! cycle already swings `LeftLeg`/`RightArm` in phase and the other diagonal
//! opposite, hanging front-left on `LeftLeg` and back-right on `RightArm`
//! produces a trot without a line of new animation code.
//!
//! The dog is on a leash, and the leash is this codebase's first Avian
//! joint: a distance limit to its owner, an ordinary pedestrian who does not
//! know how lucky they are. The dog propels itself — the joint is not a tow
//! rope — but when a car launches the owner across the junction, the leash
//! is what takes the dog along, and that is the joint earning its keep. The
//! drawn leash is one straight rod restretched between hand and collar every
//! frame — no slack, no segments. A leash under comedy tension is always
//! taut anyway, and the eye forgives a straight line long before it forgives
//! a dog towed by nothing.
//!
//! The cat answers to nobody. It carries no `Mood` — not a suppressed mood,
//! *no* mood, so every contagion, provocation and grudge system skips it by
//! construction — and `NeverTumbles`, because a cat, whatever happens, lands
//! on its feet. Dogs, being dogs, carry a wide-open temperament and catch
//! the street's feelings instantly.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::figure::{Limb, Rest, WalkCycle};
use super::pedestrian::Pedestrian;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::mood::feeling::{Mood, Temperament};

/// How many of each are about.
const DOGS: usize = 3;
const CATS: usize = 3;
const DESPAWN: f32 = 190.0;
/// Cats appear in this ring around the player, like everybody else.
const CAT_SPAWN: (f32, f32) = (30.0, 100.0);

/// The leash's reach, in metres.
const LEASH: f32 = 2.4;
/// Past this the dog hurries back to heel.
const HEEL: f32 = 1.4;
const DOG_PACE: f32 = 3.2;
const CAT_PACE: f32 = 1.3;

// The dog must be able to stand at heel without the joint fighting it, and
// the joint must catch it before it forgets it has an owner. A compile-time
// fact, so it is asserted like the figure's proportions are.
const _: () = assert!(LEASH > HEEL + 0.5);

#[derive(Component)]
pub struct Dog {
    pub owner: Entity,
    /// This dog's voice. One recorded bark, many dogs — the pitch is the
    /// identity, the same trick the crowd's voiceboxes play.
    pub pitch: f32,
}

#[derive(Component)]
pub struct Cat {
    /// Where it is currently deigning to go.
    target: Vec2,
    /// Seconds until it changes its mind.
    whim: f32,
    /// The voice it almost never uses.
    pub pitch: f32,
}

/// The visible leash: a thin rod restretched between owner and dog every
/// frame. Purely cosmetic — the physics is the [`DistanceJoint`]'s job.
#[derive(Component)]
pub struct LeashRope {
    dog: Entity,
}

#[derive(Resource)]
struct AnimalRng(ChaCha8Rng);

#[derive(Resource)]
struct AnimalTimer(Timer);

impl Default for AnimalTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(1.1, TimerMode::Repeating))
    }
}

/// Shared meshes and coats.
#[derive(Resource)]
struct AnimalKit {
    body: Handle<Mesh>,
    head: Handle<Mesh>,
    leg: Handle<Mesh>,
    tail: Handle<Mesh>,
    cat_body: Handle<Mesh>,
    cat_head: Handle<Mesh>,
    cat_leg: Handle<Mesh>,
    /// A unit-height rod, scaled to whatever span the leash covers.
    rope: Handle<Mesh>,
    leather: Handle<StandardMaterial>,
    dog_coats: Vec<Handle<StandardMaterial>>,
    cat_coats: Vec<Handle<StandardMaterial>>,
}

/// Dog proportions, from the body's origin. No compile-time capsule asserts
/// here: the collider is a ball around the torso and the snout honestly pokes
/// out of it, which for something this low to the ground costs nothing.
const DOG_STAND: f32 = 0.26;
const CAT_STAND: f32 = 0.18;

pub struct AnimalPlugin;

impl Plugin for AnimalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnimalTimer>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (maintain_animals, heel_and_prowl, draw_leashes)
                    .chain()
                    .in_set(GameSet::Ai)
                    .after(super::pedestrian::Walking),
            );
    }
}

fn setup(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(AnimalRng(stream_for(config.world_seed, stream::ANIMALS)));
    let fur = |materials: &mut Assets<StandardMaterial>, color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.95,
            ..default()
        })
    };
    commands.insert_resource(AnimalKit {
        body: meshes.add(Cuboid::new(0.24, 0.2, 0.46)),
        head: meshes.add(Sphere::new(0.11)),
        leg: meshes.add(Capsule3d {
            radius: 0.035,
            half_length: 0.06,
        }),
        tail: meshes.add(Cuboid::new(0.05, 0.05, 0.2)),
        cat_body: meshes.add(Cuboid::new(0.16, 0.14, 0.34)),
        cat_head: meshes.add(Sphere::new(0.08)),
        cat_leg: meshes.add(Capsule3d {
            radius: 0.025,
            half_length: 0.045,
        }),
        rope: meshes.add(Cylinder::new(0.014, 1.0)),
        leather: materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.16, 0.12),
            perceptual_roughness: 0.8,
            ..default()
        }),
        dog_coats: [
            Color::srgb(0.45, 0.32, 0.18),
            Color::srgb(0.12, 0.11, 0.10),
            Color::srgb(0.78, 0.72, 0.62),
        ]
        .into_iter()
        .map(|color| fur(&mut materials, color))
        .collect(),
        cat_coats: [
            Color::srgb(0.10, 0.10, 0.11),
            Color::srgb(0.75, 0.45, 0.18),
            Color::srgb(0.52, 0.52, 0.55),
        ]
        .into_iter()
        .map(|color| fur(&mut materials, color))
        .collect(),
    });
}

/// Hangs a quadruped's parts off a spawned body.
fn build_quadruped(
    entity: &mut EntityCommands,
    body: Handle<Mesh>,
    head: Handle<Mesh>,
    leg: Handle<Mesh>,
    tail: Option<Handle<Mesh>>,
    coat: Handle<StandardMaterial>,
    scale: f32,
) {
    entity.insert(WalkCycle::default());
    entity.with_children(|parent| {
        let torso = Vec3::new(0.0, 0.0, 0.0);
        parent.spawn((
            Rest::at(torso),
            Mesh3d(body),
            MeshMaterial3d(coat.clone()),
            Transform::from_translation(torso),
        ));
        let muzzle = Vec3::new(0.0, 0.1 * scale, -0.3 * scale);
        parent.spawn((
            Rest::at(muzzle),
            Mesh3d(head),
            MeshMaterial3d(coat.clone()),
            Transform::from_translation(muzzle),
        ));
        if let Some(tail) = tail {
            let dock = Vec3::new(0.0, 0.12 * scale, 0.28 * scale);
            parent.spawn((
                Rest::at(dock),
                Mesh3d(tail),
                MeshMaterial3d(coat.clone()),
                // Cocked upward: the whole difference between a tail and a
                // second snout.
                Transform::from_translation(dock).with_rotation(Quat::from_rotation_x(-0.6)),
            ));
        }
        // Diagonal pairs share a phase, which is a trot. The limb enum was
        // built for a biped; the quadruped borrows it shamelessly.
        for (limb, x, z) in [
            (Limb::LeftLeg, -0.09, -0.14),
            (Limb::RightArm, 0.09, 0.14),
            (Limb::RightLeg, 0.09, -0.14),
            (Limb::LeftArm, -0.09, 0.14),
        ] {
            let hip = Vec3::new(
                x * scale / 0.26 * DOG_STAND,
                -0.08 * scale,
                z * scale / 0.26 * DOG_STAND,
            );
            parent
                .spawn((
                    limb,
                    Rest::at(hip),
                    Transform::from_translation(hip),
                    Visibility::default(),
                ))
                .with_children(|joint| {
                    joint.spawn((
                        Mesh3d(leg.clone()),
                        MeshMaterial3d(coat.clone()),
                        Transform::from_xyz(0.0, -0.09 * scale, 0.0),
                    ));
                });
        }
    });
}

fn maintain_animals(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: ResMut<AnimalTimer>,
    kit: Res<AnimalKit>,
    mut rng: ResMut<AnimalRng>,
    focus: Res<super::focus::SimFocus>,
    owners: Query<(Entity, &Transform), (With<Pedestrian>, Without<Dog>)>,
    dogs: Query<(Entity, &Transform, &Dog)>,
    cats: Query<(Entity, &Transform), With<Cat>>,
    alive: Query<(), With<Pedestrian>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let focus = focus.ground();

    // Dogs: despawn with distance or with a vanished owner — a leash to a
    // despawned pedestrian is a crash waiting on a solver tick.
    let mut dog_count = 0usize;
    for (entity, transform, dog) in &dogs {
        let gone = alive.get(dog.owner).is_err();
        if gone || transform.translation.xz().distance(focus) > DESPAWN {
            // Forgiving: an animal is leashed to a citizen, and a citizen the
            // crowd recycled took the leash and everything on it.
            commands.entity(entity).try_despawn();
        } else {
            dog_count += 1;
        }
    }
    let mut cat_count = 0usize;
    for (entity, transform) in &cats {
        if transform.translation.xz().distance(focus) > DESPAWN {
            // Forgiving: an animal is leashed to a citizen, and a citizen the
            // crowd recycled took the leash and everything on it.
            commands.entity(entity).try_despawn();
        } else {
            cat_count += 1;
        }
    }

    // New dogs latch onto owners who do not have one yet.
    if dog_count < DOGS {
        let leashed: Vec<Entity> = dogs.iter().map(|(_, _, dog)| dog.owner).collect();
        let candidates: Vec<(Entity, Vec2)> = owners
            .iter()
            .filter(|(entity, _)| !leashed.contains(entity))
            .map(|(entity, transform)| (entity, transform.translation.xz()))
            .filter(|(_, at)| at.distance(focus) < 120.0)
            .collect();
        if !candidates.is_empty() {
            let (owner, at) = candidates[rng.0.random_range(0..candidates.len())];
            let coat = kit.dog_coats[rng.0.random_range(0..kit.dog_coats.len())].clone();
            let side = Vec2::new(rng.0.random_range(-1.0..1.0), rng.0.random_range(-1.0..1.0))
                .normalize_or_zero()
                * 1.2;
            let mut dog = commands.spawn((
                Name::new("Dog"),
                Dog {
                    owner,
                    pitch: rng.0.random_range(0.75..1.35),
                },
                Transform::from_xyz(at.x + side.x, DOG_STAND + 0.3, at.y + side.y),
                RigidBody::Dynamic,
                Collider::sphere(DOG_STAND),
                LockedAxes::ROTATION_LOCKED,
                Bouncer::new(DOG_STAND),
                // Wide open: a dog is mostly contagion with legs. It catches
                // the street's mood faster than anybody on two feet.
                Temperament {
                    contagion: 1.4,
                    ..Temperament::easygoing()
                },
                Mood::new(0.4),
                Visibility::default(),
            ));
            build_quadruped(
                &mut dog,
                kit.body.clone(),
                kit.head.clone(),
                kit.leg.clone(),
                Some(kit.tail.clone()),
                coat,
                0.26,
            );
            let dog = dog.id();
            // The first joint in the codebase: a leash is a limit, not a rod.
            commands.spawn(
                DistanceJoint::new(owner, dog)
                    .with_limits(0.0, LEASH)
                    .with_compliance(0.001),
            );
            // And the rod the eye sees, kept honest by `draw_leashes`.
            commands.spawn((
                Name::new("Leash"),
                LeashRope { dog },
                Mesh3d(kit.rope.clone()),
                MeshMaterial3d(kit.leather.clone()),
                Transform::from_scale(Vec3::ZERO),
            ));
        }
    }

    // Cats appear wherever suits them, which is near the player, at a remove.
    if cat_count < CATS {
        let angle = rng.0.random_range(0.0..std::f32::consts::TAU);
        let radius = rng.0.random_range(CAT_SPAWN.0..CAT_SPAWN.1);
        let at = focus + Vec2::new(angle.cos(), angle.sin()) * radius;
        let coat = kit.cat_coats[rng.0.random_range(0..kit.cat_coats.len())].clone();
        let mut cat = commands.spawn((
            Name::new("Cat"),
            Cat {
                target: at,
                whim: 0.0,
                pitch: rng.0.random_range(0.9..1.25),
            },
            Transform::from_xyz(at.x, CAT_STAND + 0.3, at.y),
            RigidBody::Dynamic,
            Collider::sphere(CAT_STAND),
            LockedAxes::ROTATION_LOCKED,
            crate::bounce::launch::NeverTumbles,
            Bouncer::new(CAT_STAND),
            Visibility::default(),
        ));
        build_quadruped(
            &mut cat,
            kit.cat_body.clone(),
            kit.cat_head.clone(),
            kit.cat_leg.clone(),
            Some(kit.tail.clone()),
            coat,
            0.18,
        );
    }
}

/// Dogs heel, cats prowl.
fn heel_and_prowl(
    time: Res<Time>,
    mut rng: ResMut<AnimalRng>,
    owners: Query<&Transform, (With<Pedestrian>, Without<Dog>, Without<Cat>)>,
    mut dogs: Query<
        (&Dog, &Transform, &mut Bouncer, &mut WalkCycle, &Mood),
        (Without<Cat>, Without<Launched>),
    >,
    mut cats: Query<
        (&mut Cat, &Transform, &mut Bouncer, &mut WalkCycle),
        (Without<Dog>, Without<Launched>),
    >,
) {
    let dt = time.delta_secs();

    for (dog, transform, mut bouncer, mut cycle, mood) in &mut dogs {
        let Ok(owner) = owners.get(dog.owner) else {
            continue;
        };
        let here = transform.translation.xz();
        let to_owner = owner.translation.xz() - here;
        let desired = if to_owner.length() > HEEL {
            to_owner.normalize_or_zero() * DOG_PACE
        } else {
            Vec2::ZERO
        };
        bouncer.desired = desired;
        // A dog's whole feelings are in its hop.
        bouncer.hop_scale = (1.0 + 0.7 * mood.value).clamp(0.6, 1.8);
        cycle.speed = desired.length();
    }

    for (mut cat, transform, mut bouncer, mut cycle) in &mut cats {
        let here = transform.translation.xz();
        cat.whim -= dt;
        if cat.whim <= 0.0 || here.distance(cat.target) < 0.6 {
            cat.whim = rng.0.random_range(4.0..9.0);
            let angle = rng.0.random_range(0.0..std::f32::consts::TAU);
            let range = rng.0.random_range(0.0..9.0);
            cat.target = here + Vec2::new(angle.cos(), angle.sin()) * range;
        }
        let to_target = cat.target - here;
        let desired = if to_target.length() > 0.6 {
            to_target.normalize_or_zero() * CAT_PACE
        } else {
            // Sitting. A cat at rest is a decision, not an absence.
            Vec2::ZERO
        };
        bouncer.desired = desired;
        // Cats do not bounce. Cats have never bounced.
        bouncer.hop_scale = 0.3;
        cycle.speed = desired.length();
    }
}

/// Keeps every drawn leash spanning from its owner's hand to its dog's
/// collar: the rod is a unit cylinder, so midpoint, point the axis down the
/// span, scale Y to the length. Runs a frame behind the physics at worst,
/// which on a 2.4 m strap is invisible. If either end is gone the rope goes
/// too — `maintain_animals` buries the dog, this buries the leash.
fn draw_leashes(
    mut commands: Commands,
    dogs: Query<(&Dog, &Transform), Without<LeashRope>>,
    owners: Query<&Transform, (With<Pedestrian>, Without<LeashRope>, Without<Dog>)>,
    mut ropes: Query<(Entity, &LeashRope, &mut Transform), (Without<Dog>, Without<Pedestrian>)>,
) {
    for (entity, rope, mut transform) in &mut ropes {
        let ends = dogs.get(rope.dog).ok().and_then(|(dog, at)| {
            owners.get(dog.owner).ok().map(|owner| {
                (
                    // The hand rides below the capsule's centre; the collar
                    // sits just over the dog's back.
                    owner.translation + Vec3::new(0.0, -0.25, 0.0),
                    at.translation + Vec3::new(0.0, 0.06, 0.0),
                )
            })
        });
        let Some((hand, collar)) = ends else {
            // Forgiving: an animal is leashed to a citizen, and a citizen the
            // crowd recycled took the leash and everything on it.
            commands.entity(entity).try_despawn();
            continue;
        };
        let reach = collar - hand;
        let length = reach.length();
        if length < 0.05 {
            transform.scale = Vec3::ZERO;
            continue;
        }
        *transform = Transform {
            translation: hand.midpoint(collar),
            rotation: Quat::from_rotation_arc(Vec3::Y, reach / length),
            scale: Vec3::new(1.0, length, 1.0),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diagonal_pairs_trot() {
        // Front-left shares its phase with back-right, and the other diagonal
        // opposes — the borrowed biped enum has to keep making a trot.
        use crate::ai::figure::limb_angle;
        for step in 0..32 {
            let phase = std::f32::consts::TAU * step as f32 / 32.0;
            let fl = limb_angle(Limb::LeftLeg, phase);
            let br = limb_angle(Limb::RightArm, phase);
            assert!(
                (fl.signum() - br.signum()).abs() < f32::EPSILON || fl.abs() < 1e-3,
                "the trot broke at {phase:.2}: {fl:.2} vs {br:.2}"
            );
        }
    }
}
