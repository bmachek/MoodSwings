//! The game playing itself, and watching for what goes wrong while it does.
//!
//! ```sh
//! cargo run --release -- --patrol 120          # two minutes on foot and in a car
//! cargo run --release -- --patrol 60 --city minga
//! ```
//!
//! `core::capture` renders one frame of a scene that has been *posed*. Nothing
//! moves in it, nothing is provoked, and nothing is asked to survive a minute.
//! That leaves a whole class of defect with nowhere to be caught: a leak that
//! only shows after the twentieth chunk, a body that gets flung through the
//! world by one bad contact, a mood that walks out of its range, a resource
//! that grows for ever. None of those are things a screenshot can be wrong
//! about, and none of them are things a unit test sees either, because they
//! need the whole app running for longer than a test would.
//!
//! So this walks. It writes `ActionState<Action>` directly — gameplay has never
//! read a key, only the action, which is exactly what makes a synthetic player
//! possible — and steers towards a junction, then the next one, taunting and
//! whistling as it goes, taking a car when it passes one and getting out again
//! a while later.
//!
//! What it is really for is the [`Watch`]: every second it takes the city's
//! vital signs and complains about anything that has gone wrong. A patrol that
//! ends with nothing to report is the point; a patrol that ends with a list is
//! a morning's work.
//!
//! It is not a benchmark and not a screenshot. Frame times are reported because
//! a hitch is a defect, not because the number is meant to be quoted — see the
//! README on why a settled window is the only one worth comparing.

use bevy::prelude::*;
use leafwing_input_manager::prelude::ActionState;

use crate::mood::feeling::Mood;
use crate::player::input::Action;
use crate::player::interact::Driving;
use crate::player::on_foot::Player;

/// How long the patrol runs for, in seconds, and where it is up to.
#[derive(Resource)]
pub struct Patrol {
    pub seconds: f32,
    elapsed: f32,
    /// The route it is walking, nearest waypoint first, and how long it has
    /// been trying to reach the next one.
    ///
    /// A route rather than a point, and that is the difference between a
    /// patrol that walks the city and one that walks into a wall. It used to
    /// pick the nearest junction within a hundred and forty metres and hold
    /// forward at it — which works on an empty grid and does not work at all on
    /// a town read off a map, where the straight line between two junctions
    /// goes through four houses. A hundred seconds of it reached one junction.
    route: Vec<Vec2>,
    stuck_for: f32,
    /// Ticks since the last taunt and the last attempt at a car.
    since_shout: f32,
    since_car: f32,
    /// Junctions visited, so the report says how much of the city was seen.
    visited: usize,
}

/// How close counts as arrived, in metres.
const ARRIVED: f32 = 6.0;
/// How long to spend on one waypoint before giving up on it.
///
/// Under the twelve seconds the watch calls "the player has not moved", and
/// deliberately: a patrol that has not moved for twelve seconds should have
/// tried something else by then, and the complaint should mean *rerouting did
/// not help* rather than "there is a traffic car in the way". Above the watch's
/// threshold the two instruments disagree with each other — the patrol is still
/// patiently waiting and the watch is already calling it stuck — and the
/// complaint stops carrying information.
const PATIENCE: f32 = 22.0;
/// Seconds between provocations, and between attempts to find a car.
const SHOUT_EVERY: f32 = 3.0;
const CAR_EVERY: f32 = 25.0;
/// How far a junction may be and still be worth walking to.
const LEG: f32 = 140.0;

/// How far behind the kerb a waypoint is nudged, onto the pavement.
///
/// Measured from the kerb rather than from the centreline, which is the whole
/// of what was wrong with it. It used to be 6.5 m off the *middle* of the
/// street on the argument that this was "wider than the widest half-carriageway
/// plus a pavement" — and that was not true even then, because the widest
/// arterial here is 16.9 m and its kerb is at 8.45. Once the bake began folding
/// a street's parallel ways into one, the Altstadt became twenty-nine metres
/// across and a waypoint 6.5 m off its centreline landed in the middle of the
/// carriageway, among the parked cars. The patrol walked into one and stopped,
/// four times a minute, and reported that the player had not moved.
///
/// Overshooting puts the waypoint in a wall, which the arrival radius forgives;
/// undershooting puts it in the traffic, which nothing does.
const PAVEMENT_WALK: f32 = 1.7;

/// Above this, in metres, something that belongs on the road is not on it.
///
/// Higher than any roof a car can be knocked onto and lower than the towers,
/// because a car on a roof is a physics accident and reads as a joke, while a
/// car at a hundred metres is a spawner that put it there.
const STRAY_HEIGHT: f32 = 60.0;

/// Upward speed, in m/s, that no contact in a game about rubber should produce.
///
/// A flummi's own hop is under six and the hardest crash in `vehicle::impact`
/// is meant to toss a car rather than fire it. Twelve is comfortably above
/// everything intended and far below what an ejected overlap produces.
const LAUNCH_SPEED: f32 = 12.0;

pub fn is_patrol_mode() -> bool {
    seconds().is_some()
}

/// Seconds asked for on the command line, if any.
fn seconds() -> Option<f32> {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|a| a == "--patrol")?;
    Some(
        args.get(at + 1)
            .and_then(|value| value.parse().ok())
            .unwrap_or(60.0),
    )
}

pub struct PatrolPlugin;

impl Plugin for PatrolPlugin {
    fn build(&self, app: &mut App) {
        let Some(seconds) = seconds() else {
            return;
        };
        app.insert_resource(Patrol {
            seconds,
            elapsed: 0.0,
            route: Vec::new(),
            stuck_for: 0.0,
            since_shout: 0.0,
            since_car: 0.0,
            visited: 0,
        })
        .init_resource::<Watch>()
        // Before `Input`, so what it writes is what the gameplay sets read
        // this frame rather than next.
        .add_systems(
            Update,
            (walk_about, keep_watch)
                .chain()
                .in_set(crate::core::schedule::GameSet::Input),
        );
    }
}

/// Steers the synthetic player, and provokes the neighbours.
/// A route through the city, as junctions to walk to in order.
///
/// A*, over the same road graph the traffic and the crowd use. Straight-line
/// targets are what this replaced, and the reason is that a town read off a map
/// has buildings between its junctions: the patrol would pick the nearest node,
/// hold forward, walk into a terrace and stand there for its whole twenty-two
/// seconds of patience before picking another node behind the same terrace.
/// Over a hundred seconds it reached one junction, which is not a patrol of a
/// city, it is a very thorough test of one wall.
///
/// The destination is stepped through the node list by elapsed time rather than
/// drawn, so a patrol covers different ground the longer it runs without
/// touching a generation stream.
fn plan(city: &crate::world::City, here: Vec2, elapsed: f32) -> Vec<Vec2> {
    let graph = &city.graph;
    let Some(start) = graph.nearest_node(here) else {
        return Vec::new();
    };
    let nodes = graph.node_count().max(1);
    let step = (elapsed * 11.0) as usize;
    // Somewhere worth walking to: far enough to cross a few streets, near
    // enough that the route is not the whole town.
    let mut goal = None;
    for offset in 0..nodes.min(160) {
        let index = (step + offset * 37) % nodes;
        let id = crate::world::roadgraph::NodeId(index as u32);
        let away = graph.node(id).pos.distance(here);
        if (LEG * 0.4..LEG).contains(&away) {
            goal = Some(id);
            break;
        }
    }
    let Some(goal) = goal else {
        return Vec::new();
    };
    let Some(route) = graph.path(start, goal) else {
        return Vec::new();
    };
    // Onto the pavement.
    //
    // The junction nodes are on the *centreline*, and walking a city down the
    // middle of its roads is both not what a player does and a reliable way to
    // be stopped by a traffic car or a parked one — which is what the watch was
    // reporting as "the player has not moved for twelve seconds". Offset to the
    // right of each leg, which is the pavement on the side traffic drives, the
    // route walks the same city on the surface it was mitred for.
    let mut walked = Vec::with_capacity(route.len());
    let mut previous = here;
    let mut behind: Option<crate::world::roadgraph::NodeId> = None;
    for node in route {
        let at = graph.node(node).pos;
        if let Ok(direction) = Dir2::new(at - previous) {
            // This street's own kerb, not a number that hopes to clear every
            // street in the town. A city read off a map runs from a five-metre
            // lane to a twenty-nine-metre market square, and no single offset
            // is on the pavement of both.
            let half = behind
                .and_then(|from| graph.neighbors(from).find(|(to, _)| *to == node))
                .map_or(4.0, |(_, edge)| graph.edge(edge).width * 0.5);
            walked.push(at + crate::ai::steering::right_of(*direction) * (half + PAVEMENT_WALK));
        }
        previous = at;
        behind = Some(node);
    }
    // The waypoint the patrol is already standing on is not a waypoint; walking
    // to where you are is how a route ends before it starts.
    walked.retain(|pos| pos.distance(here) > ARRIVED);
    walked
}

fn walk_about(
    time: Res<Time>,
    city: Option<Res<crate::world::City>>,
    mut patrol: ResMut<Patrol>,
    mut players: Query<(&Transform, &mut ActionState<Action>, Option<&Driving>), With<Player>>,
    mut rigs: Query<&mut crate::player::camera::CameraRig>,
) {
    let Some(city) = city else { return };
    let Ok((at, mut actions, driving)) = players.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    patrol.elapsed += dt;
    patrol.since_shout += dt;
    patrol.since_car += dt;

    let here = Vec2::new(at.translation.x, at.translation.z);

    // Tick off the waypoint it is standing on, and give up on one it cannot
    // reach — a car parked across a pavement, a hedge, a flight of steps.
    if patrol
        .route
        .first()
        .is_some_and(|next| here.distance(*next) < ARRIVED)
    {
        patrol.route.remove(0);
        patrol.visited += 1;
        patrol.stuck_for = 0.0;
    } else if patrol.stuck_for > PATIENCE {
        patrol.stuck_for = 0.0;
        // One waypoint, not the whole route. Both were tried and measured over
        // two minutes of Landshut: dropping one visits twenty-three junctions,
        // replanning from the same blocked spot visits ten — because the new
        // goal is picked from where the patrol is standing and comes out
        // behind the same obstacle.
        if !patrol.route.is_empty() {
            patrol.route.remove(0);
        }
    } else {
        patrol.stuck_for += dt;
    }

    // Out of route: plan a new one, along the road graph rather than straight
    // through the buildings between here and there.
    if patrol.route.is_empty() {
        patrol.route = plan(&city, here, patrol.elapsed);
    }

    // Steer. `Move` is in the player's own frame — forward is -Z rotated by the
    // camera's yaw — but the on-foot controller resolves it against the camera,
    // so aiming the *camera* at the target and holding forward is both simpler
    // and closer to what a player does.
    let mut heading = Vec2::Y;
    if let Some(target) = patrol.route.first() {
        let to = *target - here;
        if to.length_squared() > 1e-4 {
            heading = to.normalize();
        }
    }
    actions.set_axis_pair(&Action::Look, Vec2::ZERO);
    actions.set_axis_pair(&Action::Move, Vec2::new(0.0, 1.0));
    // The camera is what forward *means* — the on-foot controller resolves
    // `Move` against it — so the patrol aims the camera and holds forward,
    // which is both simpler than steering in the player's frame and closer to
    // what a person does. Written straight rather than through `Look`, which is
    // a rate: a patrol that has to accelerate its own mouse spends the first
    // second of every leg spinning.
    //
    // A camera at yaw 0 looks down -Z, so facing `heading` is `atan2` of its
    // negation.
    for mut rig in &mut rigs {
        rig.yaw = (-heading.x).atan2(-heading.y);
    }

    // Sprint on the long legs, which is also the only thing that exercises the
    // bounce controller at speed.
    if patrol
        .route
        .first()
        .is_some_and(|next| here.distance(*next) > 25.0)
    {
        actions.press(&Action::Sprint);
    } else {
        actions.release(&Action::Sprint);
    }

    // Be rude, then be nice, on a cycle. Both directions matter: the grudge
    // path and the apology path are the two longest chains of consequence in
    // the game and neither has any other coverage that runs for a minute.
    for action in [Action::Taunt, Action::Cheer, Action::Jump, Action::Interact] {
        actions.release(&action);
    }
    if patrol.since_shout > SHOUT_EVERY {
        patrol.since_shout = 0.0;
        let turn = (patrol.elapsed / SHOUT_EVERY) as u32 % 3;
        actions.press(match turn {
            0 => &Action::Taunt,
            1 => &Action::Cheer,
            _ => &Action::Jump,
        });
    }

    // And take a car now and then, which is the one path that moves the player
    // faster than the streamer expects.
    if patrol.since_car > CAR_EVERY {
        patrol.since_car = 0.0;
        actions.press(&Action::Interact);
    }
    // Hold the throttle down when driving; the target steering above still
    // applies, because a car reads the same `Move`.
    if driving.is_some() {
        actions.set_axis_pair(&Action::Move, Vec2::new(0.0, 1.0));
    }
}

/// The city's vital signs, sampled once a second.
#[derive(Resource, Default)]
pub struct Watch {
    since: f32,
    ticks: u32,
    /// Entities at the first tick, and the most ever seen. A world that streams
    /// correctly comes back to about where it started; one that leaks does not.
    baseline: Option<usize>,
    peak: usize,
    /// Meshes, materials and images at the first tick and now. These are the
    /// documented hazard: a mesh built in the streaming path rather than at
    /// startup is added afresh every time a chunk comes back, for ever, and
    /// nothing else in the game would ever notice.
    assets: Option<[usize; 3]>,
    /// Where the player was a second ago, and how long it has been there.
    was_at: Option<Vec3>,
    still_for: u32,
    /// The worst frame seen, and how many were bad enough to be felt.
    worst_frame: f32,
    hitches: u32,
    complaints: Vec<String>,
}

fn keep_watch(
    time: Res<Time>,
    patrol: Res<Patrol>,
    mut watch: ResMut<Watch>,
    agents: Res<crate::ai::observe::AgentObservations>,
    giveway: Res<crate::ai::giveway::GiveWay>,
    queues: Res<crate::ai::queue::Queues>,
    entities: Query<()>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    sinks: Query<
        (),
        Or<(
            With<bevy::audio::AudioSink>,
            With<bevy::audio::SpatialAudioSink>,
        )>,
    >,
    players: Query<(&Transform, &avian3d::prelude::LinearVelocity), With<Player>>,
    vehicles: Query<
        (
            &Transform,
            &avian3d::prelude::LinearVelocity,
            Option<&crate::player::interact::DrivenBy>,
        ),
        With<crate::vehicle::spawn::Vehicle>,
    >,
    walkers: Query<&Transform, With<crate::ai::pedestrian::Pedestrian>>,
    moods: Query<&Mood>,
    mut exit: MessageWriter<AppExit>,
) {
    // Every frame, not every tick: a hitch is a single frame and averaging it
    // over a second is exactly how a hitch stops being visible in a report.
    let frame = time.delta_secs();
    watch.worst_frame = watch.worst_frame.max(frame);
    if frame > 0.1 {
        watch.hitches += 1;
        // Said out loud, with its size and the moment. A hitch is not only a
        // frame-rate complaint: physics catches up on the next step, and
        // anything resting a millimetre inside a static collider is ejected by
        // however much it has to make up. Whether the parked cars leaving the
        // ground are a hitch or a placement bug is a question about *when*, and
        // the report could not answer it.
        info!(
            "patrol {:.0}s: a {:.0}ms frame",
            patrol.elapsed,
            frame * 1000.0
        );
    }

    watch.since += frame;
    if watch.since < 1.0 {
        return;
    }
    watch.since = 0.0;
    watch.ticks += 1;
    if watch.ticks.is_multiple_of(10) {
        let blocked = agents
            .agents
            .values()
            .filter(|agent| agent.motion == crate::ai::observe::Motion::Blocked)
            .count();
        info!(
            "patrol agents: {} observed, {blocked} blocked, {} blocked episodes, \
             {} waiting at a mouth over {} runs, {} given way so far, \
             {} standing in {} queues ({:?})",
            agents.agents.len(),
            agents.blocked_episodes,
            giveway.waiting,
            giveway.occupied_runs(),
            giveway.stood_aside,
            queues.standing(),
            queues.lines(),
            queues.tally(),
        );
    }
    let at = patrol.elapsed;

    // Collected rather than pushed straight into the watch: a closure that
    // borrows the resource would hold that borrow across everything below it,
    // and everything below it also needs the resource.
    let mut complaints: Vec<String> = Vec::new();
    let mut complain = |what: String| complaints.push(what);

    // The player is the one body whose state is always interesting: it is the
    // only one a human would notice going wrong.
    if let Ok((transform, velocity)) = players.single() {
        let here = transform.translation;
        if !here.is_finite() {
            complain(format!("the player's position is {here:?}"));
        } else if here.y < -12.0 {
            complain(format!(
                "the player fell out of the world at y={:.1}",
                here.y
            ));
        } else if here.y > 400.0 {
            complain(format!("the player is {:.0}m up", here.y));
        }
        let speed = velocity.0.length();
        if !speed.is_finite() || speed > 220.0 {
            complain(format!("the player is doing {speed:.0}m/s"));
        }
    }

    // A mood outside its range means something is adding to it without
    // clamping, and it is the sort of thing that shows up three systems away
    // as a face stuck at its extreme.
    if let Some(worst) = moods
        .iter()
        .map(|mood| mood.value)
        .find(|value| !(-1.001..=1.001).contains(value) || !value.is_finite())
    {
        complain(format!("a citizen's mood is {worst}"));
    }

    // Anything of the city's own that has left the city. A car on a roof is a
    // physics accident and reads as one; a car at two hundred metres is a
    // spawner that put it there, and the difference is the height. Reported as
    // the worst one and a count, because when this goes wrong it goes wrong for
    // dozens at once and a line each is a wall of text.
    let strays =
        |name: &str, highest: Option<f32>, count: usize, complain: &mut dyn FnMut(String)| {
            if let Some(highest) = highest {
                complain(format!(
                    "{count} {name} above the rooftops, the highest at y={highest:.0}"
                ));
            }
        };
    let aloft = |transforms: &mut dyn Iterator<Item = f32>, ceiling: f32| {
        let mut count = 0;
        let mut highest: Option<f32> = None;
        for y in transforms {
            if !y.is_finite() || y > ceiling {
                count += 1;
                highest = Some(highest.map_or(y, |best: f32| best.max(y)));
            }
        }
        (highest, count)
    };
    let (car_high, cars) = aloft(
        &mut vehicles.iter().map(|(at, ..)| at.translation.y),
        STRAY_HEIGHT,
    );
    strays("cars", car_high, cars, &mut complain);

    // The apex is the symptom; the launch is the bug. A car that reaches
    // seventy metres left the ground at nearly forty a second, and *where* it
    // was standing when it did is the only thing that identifies what threw it.
    for (at, velocity, driven) in &vehicles {
        if velocity.0.y > LAUNCH_SPEED {
            complain(format!(
                "a {} car is going up at {:.0}m/s from ({:.0}, {:.1}, {:.0})",
                if driven.is_some() { "driven" } else { "parked" },
                velocity.0.y,
                at.translation.x,
                at.translation.y,
                at.translation.z,
            ));
        }
    }
    let (walker_high, people) = aloft(&mut walkers.iter().map(|at| at.translation.y), STRAY_HEIGHT);
    strays("citizens", walker_high, people, &mut complain);

    // Standing still for a long time is a bug even when nothing has crashed:
    // it means the patrol has walked into something it cannot walk out of, and
    // so could a player.
    if let Ok((transform, _)) = players.single() {
        let here = transform.translation;
        match watch.was_at {
            Some(before) if before.distance(here) < 0.6 => {
                watch.still_for += 1;
                if watch.still_for == 12 {
                    // Where, and not just that. A patrol that stops is a patrol
                    // that has walked into something, and the only useful
                    // question about it is what — which means the position, the
                    // way every other complaint here carries one.
                    complain(format!(
                        "the player has not moved for twelve seconds, at \
                         ({:.0}, {:.1}, {:.0})",
                        here.x, here.y, here.z
                    ));
                }
            }
            _ => watch.still_for = 0,
        }
        watch.was_at = Some(here);
    }

    // Assets. The one leak this codebase warns about by name: a mesh built in
    // the streaming path is added again every time a chunk comes back, and
    // since nothing ever removes it the only symptom is memory. Chunks churn
    // constantly while the patrol walks, so ten percent of growth over a
    // minute is not noise.
    let now = [meshes.len(), materials.len(), images.len()];
    let then = *watch.assets.get_or_insert(now);
    if watch.ticks > 15 {
        for (index, name) in ["meshes", "materials", "images"].iter().enumerate() {
            if now[index] > then[index] + then[index] / 10 + 32 {
                complain(format!(
                    "{name} have grown from {} to {}",
                    then[index], now[index]
                ));
            }
        }
    }

    // A car waiting its turn at the mouth of a single-file street is not
    // blocked and is deliberately exempt from the recovery timer, which means
    // a give-way rule that has gone wrong is the one failure with no symptom
    // at all: `ai::observe` reads a yielding car as *waiting* — it is not
    // asking to move — and nothing else would ever mention it. So the watch
    // asks directly. Well past the alternation a busy Gasse produces, and well
    // under the minute a five-hundred-metre run legitimately takes.
    if giveway.longest_wait > 40.0 {
        complain(format!(
            "a car has given way for {:.0}s, with {} waiting",
            giveway.longest_wait, giveway.waiting
        ));
    }

    // A queue is the one thing the crowd does that has no symptom anywhere
    // else: somebody standing in a line is not blocked, is not waiting on a
    // give-way, is not off its route and is not going anywhere. A line whose
    // head cannot reach the door never advances and never complains, and one
    // wedged queue outside one shop in a town of several hundred is exactly
    // the failure a human would walk past. Well past the longest honest
    // service — see `ai::queue::SERVICE` — and past the patience that empties
    // a line that is merely slow.
    if queues.longest_stall() > 75.0 {
        complain(format!(
            "a queue has not moved for {:.0}s, with {} standing in {} lines",
            queues.longest_stall(),
            queues.standing(),
            queues.lines()
        ));
    }

    // Rodio mixes every source it has been handed whether or not it is audible,
    // so this is a count of work rather than of sound. It was thirteen hundred
    // once; anything above a few dozen means a choir has stopped capping what
    // exists again.
    let sounding = sinks.iter().count();
    if sounding > 60 {
        complain(format!("{sounding} audio sources are being mixed"));
    }

    // Entities. Chunks stream in and out, so the count moves — what must not
    // happen is that it only ever goes up. `ChunkOf` is the whole contract
    // here: anything spawned by streaming and not tagged with it is never
    // despawned, and the count is the only place that shows.
    let count = entities.iter().count();
    watch.peak = watch.peak.max(count);
    let baseline = *watch.baseline.get_or_insert(count);
    if watch.ticks > 20 && count > baseline * 3 && count == watch.peak {
        complain(format!(
            "entities have grown from {baseline} to {count} and are still climbing"
        ));
    }

    for complaint in &complaints {
        warn!("patrol {at:.0}s: {complaint}");
    }
    watch.complaints.extend(
        complaints
            .into_iter()
            .map(|what| format!("{at:.0}s {what}")),
    );

    if patrol.elapsed >= patrol.seconds {
        info!(
            "patrol done: {:.0}s, {} junctions, entities {}..{} (peak {}), \
             assets {:?}..{:?}, worst frame {:.0}ms, {} hitches, {} complaints",
            patrol.elapsed,
            patrol.visited,
            baseline,
            count,
            watch.peak,
            then,
            now,
            watch.worst_frame * 1000.0,
            watch.hitches,
            watch.complaints.len(),
        );
        for complaint in &watch.complaints {
            warn!("  {complaint}");
        }
        exit.write(AppExit::Success);
    }
}
