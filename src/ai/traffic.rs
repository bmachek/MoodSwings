//! Ambient traffic.
//!
//! Traffic cars are ordinary vehicles: same collider, same suspension, same
//! arcade tyre model, differing only in that a system writes their
//! `VehicleInput` instead of a player. That means ramming one behaves correctly
//! for free, and a police cruiser in M5 is the same code with a different goal.
//!
//! They are not persistent. A car the player has driven away from is despawned
//! and a new one faded in ahead, because simulating a whole city's worth of
//! traffic buys nothing the player can see.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::steering::{ground_axes, lane_point, steer_towards, throttle_for_speed};
use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::vehicle::controller::{VehicleInput, VehicleState};
use crate::vehicle::spawn::{AlwaysSimulated, VehicleAssets, resting_height, spawn_vehicle};
use crate::vehicle::spec::VehicleClass;
use crate::world::City;
use crate::world::roadgraph::{NodeId, TurnKind};

// How many cars, and how far out they come and go, is
// `GameConfig::traffic` now: "how alive is this city" is a thing the player
// turns up, and half of it used to be a `const` nothing could reach.
/// Distance to a junction at which the next road is chosen.
///
/// Capped against the segment being driven — see [`arrival_radius`]. A real
/// town's streets arrive as polylines and a third of Landshut's segments are
/// shorter than this, so a fixed radius handed the car straight past its own
/// bend and drove it across the pavement into the frontages.
const JUNCTION_RADIUS: f32 = 9.0;

/// How much room a new traffic car needs around it before it is spawned.
const CLEAR_SPAWN: f32 = 6.5;

/// How close to the far node this car has to be before it picks the next road.
///
/// Never more than a third of the segment, so a chain of five-metre Altstadt
/// segments is steered down rather than skipped.
fn arrival_radius(length: f32) -> f32 {
    JUNCTION_RADIUS.min(length * 0.34).max(2.0)
}

#[derive(Component)]
pub struct TrafficDriver {
    /// The segment currently being driven, as a pair of intersections.
    pub from: NodeId,
    pub to: NodeId,
    /// And the one after it.
    ///
    /// A driver has to know one junction ahead or it cannot aim past the one
    /// it is arriving at, and a car that aims *at* a junction node steers for
    /// the middle of the crossing and then snaps onto the next street. On a
    /// grid that reads as a slightly wide turn. On a town read off a map,
    /// where a curved street is a dozen segments of five metres, the aim point
    /// is at the end of the segment for the whole of it and the car chords
    /// across every bend — over the kerb and into the frontages.
    pub after: NodeId,
    pub turn: TurnKind,
    pub lane_width: f32,
    /// Target cruising speed in m/s.
    pub cruise_speed: f32,
    /// Seconds spent going nowhere, and whether the horn has gone yet.
    pub stuck: f32,
    pub honked: bool,
    /// A queue is a legitimate stop. Keep its duration separate from the
    /// recovery timer, or adding a traffic light would delete its whole queue.
    pub waiting: f32,
    pub observation: DriverObservation,
    pub desired_speed: f32,
}

/// What the driver actually used to choose its speed this frame. Exposed to
/// the inspector rather than guessed again from nearby scenery there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DriverObservation {
    #[default]
    Clear,
    Following(Entity),
    Obstacle(Entity),
    /// Standing at the mouth of a street, or at the line of a crossing, it
    /// has been told to wait for. Somebody else's right of way is a reason to
    /// be stopped, and a reason the recovery timer has no business acting on
    /// — see [`ai::giveway`] and [`ai::junction`].
    Yielding,
}

/// A driver who has been sitting still long enough to lean on the horn.
///
/// Its own message rather than a `VehicleImpact` with no impact in it: the
/// crash honk means "you hit me" and this one means "move", and the audio
/// side gets to answer them differently. Read by `audio::sfx`.
#[derive(Message, Debug, Clone, Copy)]
pub struct Impatient {
    pub at: Vec3,
}

#[derive(Resource)]
pub struct TrafficRng(pub ChaCha8Rng);

#[derive(Resource)]
struct TrafficTimer(Timer);

impl Default for TrafficTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(0.5, TimerMode::Repeating))
    }
}

/// The traffic systems, as a set, so anything that has to decide something
/// *before* a car is driven can say so. `ai::giveway` was the first of them
/// and `ai::junction` is the second: one owns the street a car is about to
/// enter, the other the crossing at the end of it.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Driving;

pub struct TrafficPlugin;

impl Plugin for TrafficPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrafficTimer>()
            .add_systems(Startup, seed_rng)
            .add_systems(
                Update,
                (maintain_population, drive_traffic)
                    .chain()
                    .in_set(Driving)
                    .in_set(crate::core::schedule::GameSet::Ai),
            );
    }
}

fn seed_rng(mut commands: Commands, config: Res<GameConfig>) {
    commands.insert_resource(TrafficRng(stream_for(
        config.world_seed,
        stream::VEHICLE_SPAWNS ^ 0x7AFF1C,
    )));
}

/// Picks the road to take at a junction, preferring to carry straight on.
fn choose_exit(city: &City, from: NodeId, at: NodeId, rng: &mut ChaCha8Rng) -> NodeId {
    let here = city.graph.node(at).pos;
    let incoming = (here - city.graph.node(from).pos).normalize_or_zero();

    let mut exits: Vec<(NodeId, f32)> = city
        .graph
        .neighbors(at)
        .filter(|(node, _)| *node != from)
        .map(|(node, edge)| {
            let direction = (city.graph.node(node).pos - here).normalize_or_zero();
            // Straight on is worth the most, and a street two cars cannot pass
            // on is worth a good deal less than any street they can. Not a ban:
            // an empty Gasse is as wrong as a jammed one, and half of
            // Landshut's road length is that narrow — refusing it outright
            // shatters the drivable network into eighty-eight pieces. This is
            // a preference, and the penalty is large enough that a narrow
            // street straight ahead loses to an ordinary one round a corner.
            let narrow = super::steering::single_file(city.graph.edge(edge).width);
            (
                node,
                incoming.dot(direction) - if narrow { NARROW_PENALTY } else { 0.0 },
            )
        })
        .collect();

    if exits.is_empty() {
        // Dead end: the only way out is back.
        return from;
    }

    // Mostly continue straight, so traffic reads as going somewhere rather than
    // wandering; the rest of the time, turn.
    exits.sort_by(|a, b| b.1.total_cmp(&a.1));
    if rng.random_range(0.0..1.0) < 0.65 || exits.len() == 1 {
        exits[0].0
    } else {
        exits[rng.random_range(1..exits.len())].0
    }
}

/// What a single-file street costs a driver choosing where to turn.
///
/// More than the whole range of the straight-on score, which runs -1 to 1, so
/// a narrow exit is only taken when every exit is narrow — or when the random
/// branch below picks one anyway, which is what keeps the alleys from emptying.
const NARROW_PENALTY: f32 = 2.5;

/// How wide the street between two junctions is, if they are joined at all.
fn width_between(city: &City, from: NodeId, to: NodeId) -> Option<f32> {
    city.graph
        .neighbors(from)
        .find(|(node, _)| *node == to)
        .map(|(_, edge)| city.graph.edge(edge).width)
}

fn maintain_population(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<GameConfig>,
    mut timer: ResMut<TrafficTimer>,
    city: Res<City>,
    assets: Res<VehicleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: ResMut<TrafficRng>,
    focus: Res<super::focus::SimFocus>,
    traffic: Query<(Entity, &Transform), With<TrafficDriver>>,
    // Every car in the city, parked or moving. Startup-resident, so this is
    // the complete list and a candidate can be rejected before it is spawned
    // rather than ejected afterwards — a dynamic body appearing inside a
    // static one is the launch the parked-car spawner already carries a
    // comment about, and traffic was reintroducing it from the other side.
    parked: Query<&Transform, With<crate::vehicle::spawn::Vehicle>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let focus = focus.ground();

    let mut alive = 0usize;
    for (entity, transform) in &traffic {
        if transform.translation.xz().distance(focus) > config.traffic.despawn {
            commands.entity(entity).despawn();
        } else {
            alive += 1;
        }
    }

    if alive >= config.traffic.population {
        return;
    }

    // Gather the eligible ring of road once, rather than sampling the whole
    // graph and rejecting: only a few percent of a 1400-edge network lies in
    // the spawn band, so rejection sampling misses far more often than it hits
    // and `Iterator::nth` makes each miss a linear scan.
    let candidates: Vec<_> = city
        .graph
        .edges()
        .filter(|edge| {
            let midpoint = city
                .graph
                .node(edge.a)
                .pos
                .midpoint(city.graph.node(edge.b).pos);
            // Never inside a Gasse. Two cars faded in at opposite ends of a
            // street they cannot pass on are a deadlock nobody asked for and
            // nobody granted: `ai::giveway` arbitrates who goes *in*, and has
            // nothing to say to two cars that were already there.
            !super::steering::single_file(edge.width)
                && (config.traffic.spawn_min..config.traffic.spawn_max)
                    .contains(&midpoint.distance(focus))
        })
        .collect();
    if candidates.is_empty() {
        return;
    }

    // Where this tick has already put a car. `Commands` are deferred, so a car
    // spawned two lines below is invisible to the `parked` query until the
    // next frame — and the very first tick of a session spawns the whole
    // population at once, fifty cars none of which can see each other. Two of
    // them landing in the same place is the launch the spawner's own comment
    // is about, arriving from the one direction it was not watching.
    let mut placed: Vec<Vec2> = Vec::new();
    let mut attempts = 0;
    while alive < config.traffic.population && attempts < 60 {
        attempts += 1;
        let edge = candidates[rng.0.random_range(0..candidates.len())];
        let a = city.graph.node(edge.a).pos;
        let b = city.graph.node(edge.b).pos;

        // Randomly pick a direction of travel along this segment.
        let (from, to, start, end) = if rng.0.random_range(0.0..1.0) < 0.5 {
            (edge.a, edge.b, a, b)
        } else {
            (edge.b, edge.a, b, a)
        };
        let Ok(direction) = Dir2::new(end - start) else {
            continue;
        };

        let t: f32 = rng.0.random_range(0.15..0.85);
        let position = lane_point(start, end, edge.width, t);
        // Is anything already standing here? Circles rather than boxes: the
        // point is to keep a car's length of daylight round a spawn, and a
        // near miss that costs one of sixty attempts is cheaper than the
        // solver's opinion about two overlapping cars.
        if parked
            .iter()
            .map(|other| other.translation.xz())
            .chain(placed.iter().copied())
            .any(|other| other.distance_squared(position) < CLEAR_SPAWN.powi(2))
        {
            continue;
        }
        placed.push(position);
        let class = VehicleClass::CIVILIAN[rng.0.random_range(0..VehicleClass::CIVILIAN.len())];
        let mut spec = class.spec();
        (spec.body_color, spec.body_metallic, spec.body_age) =
            crate::vehicle::paint::street_paint(&mut rng.0);

        let transform = Transform::from_xyz(position.x, resting_height(&spec), position.y)
            .with_rotation(Quat::from_rotation_y(
                crate::vehicle::spawn::heading_towards(*direction),
            ));

        let cruise = rng.0.random_range(8.0..15.0);
        let vehicle = spawn_vehicle(&mut commands, &assets, &mut materials, spec, transform);
        let after = choose_exit(&city, from, to, &mut rng.0);
        commands.entity(vehicle).insert((
            TrafficDriver {
                from,
                to,
                after,
                turn: city.graph.turn_kind(from, to, after),
                lane_width: edge.width,
                cruise_speed: cruise,
                stuck: 0.0,
                honked: false,
                waiting: 0.0,
                observation: DriverObservation::Clear,
                desired_speed: cruise,
            },
            AlwaysSimulated,
        ));

        alive += 1;
    }

    debug!(
        "traffic: {alive}/{} alive, {} candidate segments, {attempts} attempts",
        config.traffic.population,
        candidates.len()
    );
}

fn drive_traffic(
    mut commands: Commands,
    time: Res<Time>,
    city: Res<City>,
    spatial: SpatialQuery,
    giveway: Res<super::giveway::GiveWay>,
    junctions: Res<super::junction::Junctions>,
    mut rng: ResMut<TrafficRng>,
    mut horns: MessageWriter<Impatient>,
    mut cars: Query<(
        Entity,
        &mut TrafficDriver,
        &Transform,
        &VehicleState,
        &mut VehicleInput,
    )>,
) {
    let dt = time.delta_secs();
    // Read everyone before writing anyone. Besides avoiding conflicting Bevy
    // queries, this gives every driver the same view of this tick's queue.
    let leaders: std::collections::HashMap<_, _> = cars
        .iter()
        .map(|(entity, _, transform, _, _)| {
            (entity, (*transform.forward(), transform.up().dot(Vec3::Y)))
        })
        .collect();
    for (entity, mut driver, transform, state, mut input) in &mut cars {
        let position = transform.translation.xz();
        let start = city.graph.node(driver.from).pos;
        let end = city.graph.node(driver.to).pos;

        // Hand over to the next segment on arrival at the junction, and carry
        // straight on into the steering below rather than skipping a frame.
        // A third of Landshut's segments are shorter than the old fixed
        // radius, so the handover fired on the frame the car entered them and
        // the whole segment was driven on the last frame's steering.
        let mut start = start;
        let mut end = end;
        // Unless it is being held at this junction, in which case it has not
        // arrived at anything: the handover is what puts a car *on* the next
        // street, and the whole of the hold is that it may not go there yet.
        // The stop line is 3.6 m short of the node and the arrival radius
        // reaches nine, so without this the car books itself in from the
        // queue and the street it was waiting for is its own.
        // Two rules can hold one car: the Gasse it is about to enter belongs
        // to the other direction, or the crossing at the end of it belongs to
        // somebody crossing. Both answer in the same currency — the junction
        // to stop short of — so brake to whichever is nearer, which is the one
        // actually constraining this car.
        let held = [giveway.hold(entity), junctions.hold(entity)]
            .into_iter()
            .flatten()
            .min_by(|a, b| {
                position
                    .distance_squared(*a)
                    .total_cmp(&position.distance_squared(*b))
            });
        if held.is_none() && position.distance(end) < arrival_radius(start.distance(end)) {
            driver.from = driver.to;
            driver.to = driver.after;
            driver.after = choose_exit(&city, driver.from, driver.to, &mut rng.0);
            driver.turn = city.graph.turn_kind(driver.from, driver.to, driver.after);
            driver.lane_width =
                width_between(&city, driver.from, driver.to).unwrap_or(driver.lane_width);
            start = city.graph.node(driver.from).pos;
            end = city.graph.node(driver.to).pos;
        }

        // Pure pursuit: aim at a point further along the lane the faster we go,
        // which is what stops the car sawing at the wheel on a straight.
        let segment = end - start;
        let length = segment.length().max(1.0);
        let travelled = ((position - start).dot(segment) / (length * length)).clamp(0.0, 1.0);
        let lookahead = 7.0 + state.forward_speed.abs() * 0.85;
        let reach = travelled * length + lookahead;
        let target = if reach <= length {
            lane_point(start, end, driver.lane_width, reach / length)
        } else {
            // Past the end of this segment: the aim point walks onto the next
            // one. This is what turns a chain of short segments into a curve
            // the car follows instead of a sequence of nodes it lunges at.
            let over = reach - length;
            let ahead = city.graph.node(driver.after).pos;
            let next_length = end.distance(ahead).max(1.0);
            let next_width =
                width_between(&city, driver.to, driver.after).unwrap_or(driver.lane_width);
            lane_point(end, ahead, next_width, (over / next_length).min(1.0))
        };

        let (forward, right) = ground_axes(transform);
        input.steer = steer_towards(forward, right, target - position);

        // Ease off through corners.
        let cornering = 1.0 - input.steer.abs() * 0.55;
        let approach = turn_approach_factor(driver.turn, (end - position).length());
        let mut desired = driver.cruise_speed * cornering * approach;

        // Somebody else has the street this one is about to enter. Stop at the
        // mouth, on the same braking law the following distance uses, so it
        // arrives at the line rather than at the car already in the Gasse.
        if let Some(mouth) = held {
            desired = desired.min(stopping_speed(
                position.distance(mouth) - super::giveway::STOP_SHORT,
            ));
        }

        // And keep a gap to whatever is in front. The ray used to be a yes/no
        // question — anything within five metres plus a second and a bit of
        // travel and the answer was full reverse — which had two consequences
        // and both were visible from the pavement. Cars alternately charged
        // and slammed instead of forming a queue, because the throttle had
        // exactly two settings; and a *stopped* car kept asking for negative
        // throttle, which below walking pace is not braking, it is reverse
        // gear. What that produced is the permanent scrum the delivery module
        // already carries a header about.
        //
        // The ray now answers *how far*, and how far decides a speed: the
        // fastest this car could be going and still stop in the gap it has,
        // with a couple of metres left over. Which is what following distance
        // is, and it queues without anybody being told to queue.
        let look = STOPPING_ROOM + state.forward_speed.abs() * HEADWAY;
        let nose = transform.translation + *transform.forward() * NOSE + Vec3::Y * 0.2;
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);
        let ahead = Dir3::new(*transform.forward())
            .ok()
            .and_then(|d| spatial.cast_ray(nose, d, look, true, &filter));
        driver.observation = DriverObservation::Clear;
        if let Some(hit) = ahead {
            desired = desired.min(following_speed(hit.distance));
            // An overturned car or oncoming traffic is an obstruction, not
            // a queue we should patiently preserve for ever. The same is true
            // when this car itself has been knocked onto its side.
            let queued = leaders.get(&hit.entity).is_some_and(|(heading, upright)| {
                same_queue(
                    transform.forward().dot(*heading),
                    transform.up().dot(Vec3::Y),
                    *upright,
                )
            });
            driver.observation = if queued {
                DriverObservation::Following(hit.entity)
            } else {
                DriverObservation::Obstacle(hit.entity)
            };
        }

        // Being held is a *reason*, and it outranks whatever the ray happened
        // to find: a car waiting its turn at a mouth or a stop line has not
        // failed to get anywhere. It is also what keeps the recovery timer off
        // it, which is why both holds have a life of their own — see
        // `ai::giveway` and `ai::junction`.
        if held.is_some() {
            driver.observation = DriverObservation::Yielding;
        }

        // Never reverse into the street behind. `longitudinal_force` reads a
        // negative throttle as braking only while the car is actually moving
        // forwards; under half a metre a second it is the reverse gear, and a
        // car that has stopped behind an obstruction it cannot pass would back
        // out of the queue for ever.
        input.throttle = throttle_for_speed(state.forward_speed, desired);
        if desired < 0.2 && state.forward_speed < 0.5 {
            input.throttle = 0.0;
        }
        // And nothing else would hold it there. Below half a metre a second
        // the throttle cannot brake, `throttle_for_speed` has a deadband under
        // four tenths, and the only other longitudinal force on the car is
        // quadratic drag, which at walking pace is nothing at all. A car told
        // to stop at a line therefore coasts through it at 0.4 m/s — six
        // metres over a fifteen-second wait, which is the far side of the
        // junction. What holds a stopped car is the handbrake, which is what
        // `player::interact` already leaves on when a car is abandoned.
        input.handbrake = held.is_some() && state.forward_speed.abs() < 1.0;
        driver.desired_speed = desired;

        // Preserve a legitimate queue. Only an unexplained stop or a static
        // obstruction advances recovery; every recovery is reported so it
        // cannot disguise a navigation failure as healthy throughput.
        if driver.note_stop(state.forward_speed, dt) {
            horns.write(Impatient {
                at: transform.translation,
            });
        }
        if driver.stuck > GIVE_UP {
            warn!(
                "traffic recovery: {entity:?} at {position:?}, {:?}, blocked for {:.1}s",
                driver.observation, driver.stuck
            );
            commands.entity(entity).try_despawn();
        }
    }
}

/// How far in front of the body the obstacle ray starts, and how much room a
/// car wants at rest and per metre a second of speed.
const NOSE: f32 = 2.6;
const STOPPING_ROOM: f32 = 6.0;
const HEADWAY: f32 = 1.6;

/// How long a stopped car waits before it leans on the horn, and before it is
/// given up on entirely.
const HONK_AFTER: f32 = 5.0;
const GIVE_UP: f32 = 26.0;

/// And how long a car will wait its turn at a mouth before it stops counting
/// as waiting its turn. Past this it is stuck like anything else is stuck.
const YIELD_PATIENCE: f32 = 75.0;

/// Slow before the junction, where steering alone is too late to make a
/// narrow street turn believable. The factor reaches one outside the
/// approach window so an intended turn never permanently reduces cruising
/// speed on the preceding road.
fn turn_approach_factor(turn: TurnKind, distance: f32) -> f32 {
    let window = 16.0;
    if distance >= window {
        return 1.0;
    }
    let near = 1.0 - (distance / window).clamp(0.0, 1.0);
    let target = match turn {
        TurnKind::Straight => 1.0,
        TurnKind::Left => 0.84,
        TurnKind::Right => 0.76,
        TurnKind::UTurn => 0.55,
    };
    1.0 - near * (1.0 - target)
}

fn same_queue(alignment: f32, own_up: f32, leader_up: f32) -> bool {
    alignment > 0.5 && own_up > 0.5 && leader_up > 0.5
}

impl TrafficDriver {
    /// Returns true only on the first impatient honk of this stop.
    fn note_stop(&mut self, speed: f32, dt: f32) -> bool {
        if speed.abs() >= 0.3 {
            self.waiting = 0.0;
            self.stuck = 0.0;
            self.honked = false;
            return false;
        }
        self.waiting += dt;
        // A queue and a right of way are both legitimate stops: only an
        // unexplained one, or a static obstruction, advances the timer that
        // deletes the car. The right of way is bounded, though, and that
        // matters more than it looks — a yielding car is exempt from this
        // timer *and* reads to `ai::observe` as waiting rather than blocked,
        // because it is not asking to move, so a give-way gone wrong is the
        // one kind of stuck with no symptom anywhere. `giveway` has a timeout
        // on every way a reservation can outlive its cars; this is the belt
        // under those braces, set past any wait the alternation produces.
        let legitimate = match self.observation {
            DriverObservation::Following(_) => true,
            DriverObservation::Yielding => self.waiting < YIELD_PATIENCE,
            _ => false,
        };
        if legitimate {
            self.stuck = 0.0;
        } else {
            self.stuck += dt;
        }
        // A driver who knows why it is waiting is more patient about it. Not
        // silent — a horn at the mouth of a Gasse is the joke — but five
        // seconds of it from every car in a queue is a dozen spatial one-shots
        // a junction, which the patrol complains about by name.
        let patience = if matches!(self.observation, DriverObservation::Yielding) {
            HONK_AFTER * 2.6
        } else {
            HONK_AFTER
        };
        if self.waiting > patience && !self.honked {
            self.honked = true;
            return true;
        }
        false
    }
}

/// The fastest a car may be going and still stop in `room` metres.
///
/// A braking law rather than a rule of thumb: `v = sqrt(2·a·s)` is the speed
/// something can shed over a distance at a given deceleration. Below zero room
/// it asks for zero, and the throttle goes to a hold rather than to reverse.
fn stopping_speed(room: f32) -> f32 {
    const BRAKING: f32 = 3.4;
    (2.0 * BRAKING * room.max(0.0)).sqrt()
}

/// The fastest a car may be going with `gap` metres of clear road in front.
///
/// The same law, against the distance left once the car in front has been
/// given its bumper's worth of room. A stop line wants no such buffer — it is
/// a line, not a car — which is why the two are separate.
fn following_speed(gap: f32) -> f32 {
    const BUFFER: f32 = 3.2;
    stopping_speed(gap - BUFFER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driver() -> TrafficDriver {
        TrafficDriver {
            from: NodeId(0),
            to: NodeId(1),
            after: NodeId(2),
            turn: TurnKind::Straight,
            lane_width: 8.0,
            cruise_speed: 10.0,
            stuck: 0.0,
            honked: false,
            waiting: 0.0,
            observation: DriverObservation::Clear,
            desired_speed: 10.0,
        }
    }

    #[test]
    fn a_queue_can_wait_longer_than_the_recovery_timeout() {
        let mut driver = driver();
        driver.observation = DriverObservation::Following(Entity::PLACEHOLDER);
        let mut horns = 0;
        for _ in 0..120 {
            horns += usize::from(driver.note_stop(0.0, 1.0));
        }
        assert_eq!(driver.waiting, 120.0);
        assert_eq!(driver.stuck, 0.0);
        assert_eq!(horns, 1);
    }

    #[test]
    fn an_obstruction_still_reaches_recovery() {
        let mut driver = driver();
        driver.observation = DriverObservation::Obstacle(Entity::PLACEHOLDER);
        driver.note_stop(0.0, GIVE_UP + 1.0);
        assert!(driver.stuck > GIVE_UP);
    }

    #[test]
    fn joining_a_queue_clears_recovery_but_not_impatience() {
        let mut driver = driver();
        driver.note_stop(0.0, 12.0);
        driver.observation = DriverObservation::Following(Entity::PLACEHOLDER);
        assert!(!driver.note_stop(0.0, 1.0));
        assert_eq!(driver.stuck, 0.0);
        assert_eq!(driver.waiting, 13.0);
        driver.observation = DriverObservation::Clear;
        driver.note_stop(0.0, 1.0);
        assert_eq!(driver.stuck, 1.0);
    }

    #[test]
    fn moving_again_starts_a_new_stop_and_honk_window() {
        let mut driver = driver();
        assert!(driver.note_stop(0.0, 10.0));
        assert!(!driver.note_stop(2.0, 1.0));
        assert_eq!(driver.waiting, 0.0);
        assert_eq!(driver.stuck, 0.0);
        assert!(!driver.honked);
        assert!(driver.note_stop(0.0, 10.0));
    }

    #[test]
    fn oncoming_and_overturned_cars_do_not_masquerade_as_a_queue() {
        assert!(same_queue(1.0, 1.0, 1.0));
        assert!(!same_queue(-1.0, 1.0, 1.0));
        assert!(!same_queue(0.0, 1.0, 1.0));
        assert!(!same_queue(1.0, -1.0, 1.0));
        assert!(!same_queue(1.0, 1.0, -1.0));
    }

    #[test]
    fn giving_way_is_a_wait_until_it_has_gone_on_too_long() {
        // The exemption that keeps a give-way queue alive, and the bound that
        // stops it being a way for a car to be stuck for ever in silence.
        let mut driver = driver();
        driver.observation = DriverObservation::Yielding;
        driver.note_stop(0.0, GIVE_UP + 1.0);
        assert_eq!(driver.stuck, 0.0, "a car waiting its turn was given up on");
        driver.note_stop(0.0, YIELD_PATIENCE);
        assert!(
            driver.stuck > 0.0,
            "a car that has yielded for two minutes is not yielding, it is stuck"
        );
    }

    #[test]
    fn turns_brake_only_inside_the_approach_window() {
        assert_eq!(turn_approach_factor(TurnKind::Right, 20.0), 1.0);
        assert!(
            turn_approach_factor(TurnKind::Right, 2.0) < turn_approach_factor(TurnKind::Left, 2.0)
        );
        assert!(
            turn_approach_factor(TurnKind::UTurn, 2.0) < turn_approach_factor(TurnKind::Right, 2.0)
        );
    }
}
