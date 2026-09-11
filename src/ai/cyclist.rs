//! Cyclists: citizens on the carriageway.
//!
//! Structurally they sit between the crowd and the traffic. The body is a
//! pedestrian's — a dynamic capsule with a `Bouncer`, a mood, a voice and a
//! full set of feelings, so a car that clips one launches them exactly the
//! way it launches anybody, and a cyclist in a foul mood grumbles at the
//! junction like everybody else. But they *ride* like traffic: down the
//! kerbside of the lane on the road graph, at a pace no pedestrian keeps,
//! with the hop dialled to zero — a glide, with legs pumping over the frame
//! because the walk cycle does not know it has become a drivetrain, and that
//! is the joke.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::archetype::Archetype;
use super::figure::Rest;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::mood::face::{FaceAssets, FaceLevel};
use crate::mood::feeling::{Mood, MoodRng, Tempers};
use crate::mood::provoke::Provoker;
use crate::mood::voice::Voicebox;
use crate::world::City;
use crate::world::roadgraph::NodeId;

// How many are riding, and how far out, is `GameConfig::traffic` now — see
// there. A bike is traffic as far as "how busy is this city" is concerned, and
// the two numbers were being turned up in different places.

/// Cruising speed, in m/s. Under the traffic, over any walk.
const PACE: f32 = 5.2;
// How far inside the kerb line a bike rides is `steering::cycle_offset` now.
// A fixed 1.2 m put every cyclist in Landshut's narrow streets straight
// through the parked cars: on a five-metre lane the parked row is centred at
// 1.16 m and the bike rode at 1.30.
/// Close enough to a junction to pick the next street.
const ARRIVED: f32 = 6.0;

/// The same capsule the crowd wears.
const RADIUS: f32 = 0.32;
const HEIGHT: f32 = 1.05;
const STAND_HEIGHT: f32 = HEIGHT * 0.5 + RADIUS;

#[derive(Component)]
pub struct Cyclist {
    from: NodeId,
    to: NodeId,
    /// Which way the bike was pointing last tick, so a turn can be leaned into.
    heading: f32,
}

/// A wheel, so it can be turned. Deliberately not a [`super::figure::Limb`]:
/// `figure::animate` writes a rotation for every limb it finds and would take
/// the wheel over.
#[derive(Component)]
struct Wheel;

/// How far a bike goes for one turn of the pedals, in metres.
///
/// A town bike in a middle gear: 42 by 18 on 700c is about 4.9 m. Anything
/// much shorter and the rider looks like they are running away from something.
const METRES_PER_CRANK: f32 = 4.6;

/// The rolling radius of the wheel the bike is drawn with.
const WHEEL_RADIUS: f32 = 0.29;

/// How far a rider may lean into a turn, in radians.
///
/// Small, and hard-clamped. The target point is recomputed off the road graph
/// every tick and the raw derivative of the heading is noisy; a rider who
/// flicks fifteen degrees for one frame reads as a glitch rather than as a
/// corner. The collider is `LockedAxes::ROTATION_LOCKED`, so a lean written
/// straight into the transform sticks, but it does turn the capsule — at more
/// than this a leaning rider clips the parked cars they are riding inside of.
const LEAN_LIMIT: f32 = 0.26;
/// How fast the lean follows the turn. Low-passed for the same reason.
const LEAN_SLEW: f32 = 5.0;

#[derive(Resource)]
struct CyclistRng(ChaCha8Rng);

#[derive(Resource)]
struct CyclistTimer(Timer);

impl Default for CyclistTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(0.9, TimerMode::Repeating))
    }
}

/// The bike itself, as parts. One kit, shared.
#[derive(Resource)]
struct BikeKit {
    wheel: Handle<Mesh>,
    frame: Handle<Mesh>,
    bars: Handle<Mesh>,
    steel: Handle<StandardMaterial>,
}

pub struct CyclistPlugin;

impl Plugin for CyclistPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CyclistTimer>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (maintain_cyclists, ride)
                    .chain()
                    .in_set(GameSet::Ai)
                    // After the crowd's set: a bike is steered here and only
                    // here, but the ordering keeps the frame coherent.
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
    commands.insert_resource(CyclistRng(stream_for(config.world_seed, stream::CYCLISTS)));
    commands.insert_resource(BikeKit {
        wheel: meshes.add(Torus::new(0.24, 0.29)),
        frame: meshes.add(Cuboid::new(0.07, 0.08, 1.1)),
        bars: meshes.add(Cuboid::new(0.42, 0.05, 0.05)),
        steel: materials.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.30, 0.24),
            perceptual_roughness: 0.5,
            metallic: 0.55,
            ..default()
        }),
    });
}

fn maintain_cyclists(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<GameConfig>,
    mut timer: ResMut<CyclistTimer>,
    city: Res<City>,
    kit: Res<BikeKit>,
    figures: Res<super::figure::FigureAssets>,
    faces: Res<FaceAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: ResMut<CyclistRng>,
    mut tempers: ResMut<MoodRng>,
    mix: Res<Tempers>,
    focus: Res<super::focus::SimFocus>,
    cyclists: Query<(Entity, &Transform), With<Cyclist>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let focus = focus.ground();

    let mut riding = 0usize;
    for (entity, transform) in &cyclists {
        if transform.translation.xz().distance(focus) > config.traffic.despawn {
            commands.entity(entity).despawn();
        } else {
            riding += 1;
        }
    }
    if riding >= config.traffic.cyclists {
        return;
    }

    let candidates: Vec<_> = city
        .graph
        .edges()
        .filter(|edge| {
            let midpoint = city
                .graph
                .node(edge.a)
                .pos
                .midpoint(city.graph.node(edge.b).pos);
            (config.traffic.spawn_min..config.traffic.spawn_max).contains(&midpoint.distance(focus))
        })
        .collect();
    if candidates.is_empty() {
        return;
    }

    while riding < config.traffic.cyclists {
        let edge = candidates[rng.0.random_range(0..candidates.len())];
        let (from, to) = if rng.0.random_range(0.0..1.0) < 0.5 {
            (edge.a, edge.b)
        } else {
            (edge.b, edge.a)
        };
        let a = city.graph.node(from).pos;
        let b = city.graph.node(to).pos;
        let t: f32 = rng.0.random_range(0.15..0.85);
        let position = kerbside_point(a, b, edge.width, t);

        // The same stream discipline as the crowd: dispositions off MOOD,
        // wardrobe off this module's own stream.
        let temper = mix.draw(&mut tempers.0);
        let mood = temper.baseline;
        let worn = faces.wear(mood);
        let pitch = tempers.0.random_range(0.82..1.28);
        let coat = materials.add(StandardMaterial {
            base_color: Color::srgb(
                rng.0.random_range(0.2..0.8),
                rng.0.random_range(0.2..0.8),
                rng.0.random_range(0.2..0.8),
            ),
            perceptual_roughness: 0.85,
            ..default()
        });

        let mut rider = commands.spawn((
            Name::new("Cyclist"),
            Cyclist {
                from,
                to,
                heading: 0.0,
            },
            Transform::from_xyz(position.x, STAND_HEIGHT, position.y),
            RigidBody::Dynamic,
            Collider::capsule(RADIUS, HEIGHT),
            LockedAxes::ROTATION_LOCKED,
            Bouncer::new(STAND_HEIGHT),
            temper,
            Mood::new(mood),
            FaceLevel(worn.level),
            Voicebox::new(pitch),
            Provoker::default(),
            Archetype::Everyday,
            super::figure::Riding { crank: 0.0 },
            Visibility::default(),
        ));
        super::figure::dress(
            &mut rider,
            &figures,
            coat,
            &worn,
            Archetype::Everyday,
            &mut rng.0,
        );
        // The bike, hung as parts with a Rest each so the squash keeps them
        // attached. Forward is -Z, the way the shoes point.
        rider.with_children(|bike| {
            for z in [-0.62f32, 0.62] {
                let hub = Vec3::new(0.0, -STAND_HEIGHT + 0.29, z);
                bike.spawn((
                    Rest::at(hub),
                    Wheel,
                    Mesh3d(kit.wheel.clone()),
                    MeshMaterial3d(kit.steel.clone()),
                    Transform::from_translation(hub).with_rotation(wheel_upright()),
                ));
            }
            let spine = Vec3::new(0.0, -STAND_HEIGHT + 0.52, 0.0);
            bike.spawn((
                Rest::at(spine),
                Mesh3d(kit.frame.clone()),
                MeshMaterial3d(kit.steel.clone()),
                Transform::from_translation(spine),
            ));
            let bars = Vec3::new(0.0, -STAND_HEIGHT + 0.72, -0.48);
            bike.spawn((
                Rest::at(bars),
                Mesh3d(kit.bars.clone()),
                MeshMaterial3d(kit.steel.clone()),
                Transform::from_translation(bars),
            ));
        });
        riding += 1;
    }
}

/// Stands a flat-lying torus up as a wheel that rolls the way the bike goes.
///
/// A torus is built lying in the XZ plane, so its axis is Y and it has to be
/// turned onto the axle. The axle of a wheel that rolls along Z is X — which
/// is a rotation about *Z*, not about X. About X the axle ends up pointing
/// along the direction of travel instead, and the bike becomes a pair of
/// discs held out sideways: a sledge, or at second glance a very wide
/// wheelchair, which is what this looked like on the street for a while.
fn wheel_upright() -> Quat {
    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
}

/// A point riding the kerb side of the correct lane.
///
/// Which is not the kerb: on a street with parking there is a row of cars
/// between the two, and a bike goes inside them.
fn kerbside_point(a: Vec2, b: Vec2, width: f32, t: f32) -> Vec2 {
    let parked = super::steering::parked_on_the_right(a, b, width);
    super::steering::offset_point(a, b, super::steering::cycle_offset(width, parked), t)
}

fn ride(
    time: Res<Time>,
    city: Res<City>,
    mut rng: ResMut<CyclistRng>,
    mut cyclists: Query<
        (
            &mut Cyclist,
            &mut Bouncer,
            &mut Transform,
            &mut super::figure::WalkCycle,
            &mut super::figure::Riding,
            &Children,
        ),
        Without<Launched>,
    >,
    mut wheels: Query<&mut Transform, (With<Wheel>, Without<Cyclist>)>,
) {
    let dt = time.delta_secs();
    for (mut cyclist, mut bouncer, mut transform, mut cycle, mut riding, children) in &mut cyclists
    {
        let position = transform.translation.xz();
        let b = city.graph.node(cyclist.to).pos;

        if position.distance(b) < ARRIVED {
            let next = city
                .graph
                .neighbors(cyclist.to)
                .map(|(node, _)| node)
                .filter(|node| *node != cyclist.from)
                .collect::<Vec<_>>();
            if let Some(&next) = next.get(rng.0.random_range(0..next.len().max(1))) {
                cyclist.from = cyclist.to;
                cyclist.to = next;
            }
            continue;
        }

        let a = city.graph.node(cyclist.from).pos;
        let width = city
            .graph
            .neighbors(cyclist.from)
            .find(|(node, _)| *node == cyclist.to)
            .map(|(_, edge)| city.graph.edge(edge).width)
            .unwrap_or(9.0);
        let segment = b - a;
        let length = segment.length().max(1.0);
        let travelled = ((position - a).dot(segment) / (length * length)).clamp(0.0, 1.0);
        let target = kerbside_point(a, b, width, (travelled + 8.0 / length).min(1.0));

        let heading = (target - position).normalize_or_zero();
        bouncer.desired = heading * PACE;
        // Zero hop: a glide.
        bouncer.hop_scale = 0.0;
        // And no stride at all. This used to be `PACE * 0.45`, with a comment
        // admitting the joke — "the legs still pump, because the walk cycle
        // does not know it has become a drivetrain" — and what it drew was a
        // citizen walking briskly on top of a stationary bicycle. The legs are
        // on the pedals now (`figure::Riding`), so the walk cycle has nothing
        // left to say; zeroed rather than left running, because anything that
        // later pins a footstep or a gait sound to it must not fire on a rider.
        cycle.speed = 0.0;

        // The crank turns with the ground covered, not with the clock, so a
        // rider slowing for a junction slows their pedalling with it and the
        // feet can never drift out of step with the wheels.
        let turned = PACE * dt / METRES_PER_CRANK * std::f32::consts::TAU;
        riding.crank = (riding.crank + turned).rem_euclid(std::f32::consts::TAU);

        // The wheels off the same distance, for the same reason.
        let spin = PACE * dt / WHEEL_RADIUS;
        for &child in children {
            if let Ok(mut wheel) = wheels.get_mut(child) {
                // Post-multiplied, so the turn is about the torus's own hole
                // axis — which after `wheel_upright` is the axle. The sign is
                // pinned by a test: the first one drawn here rolled the bike
                // backwards, which is exactly the class of thing that does not
                // show in a still.
                wheel.rotation *= Quat::from_rotation_y(spin);
            }
        }

        if heading != Vec2::ZERO {
            let yaw = crate::vehicle::spawn::heading_towards(heading);
            // How hard this is turning, low-passed. A bike leans into a corner
            // by about atan(v * omega / g); at 5.2 m/s a brisk turn is a few
            // degrees, and the clamp is what stops the noise in the target
            // point becoming a twitch.
            let turn = super::figure::shortest_turn(cyclist.heading, yaw) / dt.max(1e-3);
            let wanted = (-turn * PACE / 9.81).atan().clamp(-LEAN_LIMIT, LEAN_LIMIT);
            let lean = transform.rotation.to_euler(EulerRot::YXZ).2;
            let eased = lean + (wanted - lean) * (LEAN_SLEW * dt).min(1.0);
            cyclist.heading = yaw;
            transform.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(eased);
        }
    }
}

#[cfg(test)]
mod tests {
    /// A cyclist who is pedalling is not a cyclist who is walking.
    #[test]
    fn a_rider_puts_their_feet_on_the_pedals_and_not_on_the_road() {
        use crate::ai::figure::{Limb, riding_angle, riding_stretch};
        use std::f32::consts::TAU;

        // Over a whole turn of the cranks, both feet stay on a circle about
        // the bottom bracket and the two are always half a turn apart.
        let mut lowest = f32::MAX;
        let mut highest = f32::MIN;
        for step in 0..48 {
            let crank = step as f32 / 48.0 * TAU;
            for leg in [Limb::LeftLeg, Limb::RightLeg] {
                let angle = riding_angle(leg, crank);
                let stretch = riding_stretch(leg, crank);
                assert!(
                    (0.35..=1.0).contains(&stretch),
                    "a leg at {stretch} of its length"
                );
                // The foot, back in the body's frame: down the leg, foreshortened.
                let reach = crate::ai::figure::body::LEG_LENGTH * stretch;
                let y = crate::ai::figure::body::HIP - reach * angle.cos();
                lowest = lowest.min(y);
                highest = highest.max(y);
            }
        }
        // The pedals go round: the foot rises and falls by about two crank
        // arms over a turn, and never drops through the bottom of the capsule.
        let swing = highest - lowest;
        assert!(
            swing > 0.2,
            "the feet only move {swing} m over a whole turn of the cranks"
        );
        assert!(
            lowest > crate::ai::figure::body::FEET - 0.02,
            "a foot reached {lowest}, below the bottom of the collider"
        );

        // And the two feet are never in the same place, which is what a walk
        // cycle on a bike looked like.
        for step in 0..24 {
            let crank = step as f32 / 24.0 * TAU;
            let left = riding_angle(Limb::LeftLeg, crank);
            let right = riding_angle(Limb::RightLeg, crank);
            assert!(
                (left - right).abs() > 0.05,
                "both feet at {left} with the cranks at {crank}"
            );
        }

        // The arms reach forward for the bars and stay there.
        for step in 0..8 {
            let crank = step as f32 / 8.0 * TAU;
            assert_eq!(
                riding_angle(Limb::LeftArm, crank),
                crate::ai::figure::body::BARS_REACH
            );
            assert_eq!(riding_stretch(Limb::RightArm, crank), 1.0);
        }
    }

    /// The top of a rolling wheel goes the way the bike is going.
    #[test]
    fn the_wheels_turn_the_way_the_bike_is_travelling() {
        // `wheel_upright` is a quarter turn about Z, which sends the torus's
        // local +X to +Y — so the top of the wheel is local +X, and its hole
        // axis (local +Y) has become the axle across the bike.
        // Forward is -Z. The top of a rolling wheel must go that way.
        let rim = Vec3::new(WHEEL_RADIUS, 0.0, 0.0);
        let before = wheel_upright() * Quat::from_rotation_y(0.0) * rim;
        let after = wheel_upright() * Quat::from_rotation_y(0.05) * rim;
        assert!(
            before.y > 0.0 && after.y > 0.0,
            "the sample point is not at the top of the wheel"
        );
        assert!(
            after.z < before.z,
            "the top of the wheel went from {} to {}, which is backwards",
            before.z,
            after.z
        );
    }

    use super::*;

    #[test]
    fn the_wheels_turn_on_an_axle_across_the_bike() {
        // The failure this catches is not subtle and is completely invisible
        // to every other check: the bike still rides its lane, still carries
        // its rider, and looks like a sledge.
        let axle = wheel_upright() * Vec3::Y;
        assert!(
            axle.x.abs() > 0.99,
            "the axle should lie across the bike, and points {axle:?}"
        );
        // And the wheel's plane therefore contains the direction of travel.
        assert!(
            axle.z.abs() < 0.01,
            "the axle points along the road: {axle:?}"
        );
    }

    #[test]
    fn a_bike_rides_inside_its_own_lane() {
        let a = Vec2::ZERO;
        let b = Vec2::new(0.0, 100.0);
        let width = 10.0;
        let at = kerbside_point(a, b, width, 0.5);
        // Travelling +Z, right is -X: the kerb side of the correct lane.
        assert!(at.x < 0.0, "wrong side of the road: {at:?}");
        assert!(
            at.x.abs() < width * 0.5,
            "riding the pavement is the crowd's job: {at:?}"
        );
        assert!(
            at.x.abs() > crate::ai::steering::lane_offset(width, true),
            "the middle of the lane belongs to the cars: {at:?}"
        );
        // And inside whatever is parked at that kerb, which on a ten-metre
        // street is a row of cars two and a half metres deep. Riding at a flat
        // 1.2 m off the kerb line, which is what this used to do, put every
        // cyclist in the town through the parked cars.
        let parked = width * 0.5 - crate::ai::steering::parked_depth(width);
        assert!(
            at.x.abs() < parked,
            "the bike is riding through the parked cars: {at:?}"
        );
    }
}
