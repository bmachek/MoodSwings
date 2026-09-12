//! Who gets the crossing.
//!
//! [`giveway`](super::giveway) settled the Gasse: a street too narrow for two
//! cars is entered by one direction at a time. What it deliberately left for
//! later is the other half of the same question, and the bigger one — the
//! *junction*, where two cars on streets wide enough for anybody still cannot
//! both have the middle. Its own header names this as next, and points at
//! [`movements_conflict`] as "the policy waiting for a caller". This is the
//! caller.
//!
//! Until now nothing in the game had right of way at a crossing. Traffic
//! braked for obstacles and for nothing else, so two cars arriving at a
//! crossroads at once did not negotiate: they drove at each other until the
//! obstacle ray saw metal, stopped nose to nose in the middle, and sat there
//! until `traffic::GIVE_UP` deleted one of them twenty-six seconds later. The
//! signals standing on every approach have been unlit since they were built,
//! with a comment in `world::props` saying so, because there was no rule for
//! them to show.
//!
//! What this owns:
//!
//! * **Claims.** Who has a junction, as a movement across it rather than as a
//!   car in it — several cars can hold one crossing at once as long as their
//!   paths do not cross, which is what makes a crossroads a crossroads rather
//!   than a turnstile. Opposing straights go together; a left turn waits for
//!   the oncoming one.
//! * **Priority.** Who takes it when the claims do conflict: the main road
//!   first, then whoever has the other on their right, then whoever has waited
//!   longest. That middle rule is *rechts vor links*, and it is the one a
//!   player watching from the pavement can actually read.
//! * **Holds.** Where a car that has not got it must stop, published exactly
//!   the way `giveway` publishes its own, so `traffic::drive_traffic` reads
//!   one kind of answer from two sources and brakes to the nearer.
//!
//! What it is **not**, in the same sense `giveway` is not: a way of deleting
//! cars. A car held at a stop line is `Yielding`, which the recovery timer
//! leaves alone — so every way a claim can outlive the car that made it has to
//! be a way it also expires, or one wreck in one crossing closes it for the
//! session. There are three, and each has its own answer below.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use super::steering::right_of;
use super::traffic::{Driving, TrafficDriver};
use crate::core::schedule::GameSet;
use crate::vehicle::controller::VehicleState;
use crate::vehicle::spawn::Vehicle;
use crate::world::City;
use crate::world::roadgraph::{Movement, NodeId, RoadGraph, movements_conflict};
use crate::world::signals::{Aspect, Signals, phase_clock};

/// One movement that has been granted a junction.
#[derive(Debug, Clone, Copy)]
struct Claim {
    who: Entity,
    movement: Movement,
    /// When it was granted. What decides taking it back from a car that is
    /// never going to clear it — the wreck case.
    since: f32,
}

/// Who is waiting for which crossing, and where they have to stop.
#[derive(Resource, Default)]
pub struct Junctions {
    claims: HashMap<NodeId, Vec<Claim>>,
    /// Where each held driver must stop short of, this frame.
    holds: HashMap<Entity, Vec2>,
    /// And who was held last frame, so a stop is counted once rather than
    /// sixty times a second.
    waited_last_frame: HashSet<Entity>,
    /// For the patrol and the dev panel. A car waiting its turn at a crossing
    /// is not blocked, is not in a queue and is not on a give-way run, so
    /// without these nothing anywhere else in the game can see it at all.
    pub waiting: usize,
    pub longest_wait: f32,
    /// How many times somebody has been made to wait at a crossing since the
    /// session started. Counted once per stop rather than per frame, so the
    /// difference between "the rule never fires" and "it fires and nobody has
    /// to wait" stays visible — which is the whole question of whether it
    /// earns its keep.
    pub yielded: u64,
}

impl Junctions {
    /// The junction this driver has to stop before, if it is being held.
    pub fn hold(&self, driver: Entity) -> Option<Vec2> {
        self.holds.get(&driver).copied()
    }

    /// How many crossings are spoken for right now.
    pub fn claimed(&self) -> usize {
        self.claims.len()
    }
}

/// How long anybody may hold a junction before it is taken back.
///
/// A claim is refreshed every frame its owner is still approaching or still
/// in the crossing, and dropped the frame it is not — a car deleted, launched
/// onto a roof or driven off by the player hands the junction straight back,
/// and there is no grace period to hold the crossing shut behind one that has
/// simply gone through. What that leaves is the car that still exists, still
/// has the claim, and is not going anywhere — on its roof in the middle of the crossing, or wedged
/// against a bollard. Everybody waiting for it is *waiting*, which is neither
/// blocked nor recovered nor counted, so nothing else in the game would ever
/// mention it. Generous next to the two or three seconds crossing a junction
/// actually takes, because a legitimate crawl behind a queue that is itself
/// clearing is still progress.
const STUCK: f32 = 22.0;

/// How much room a car wants between itself and the crossing it waits at.
///
/// The same line `giveway` stops its cars at, and deliberately the same
/// constant: a car should not stop in two different places depending on which
/// of the two rules is holding it.
pub use super::giveway::STOP_SHORT;

/// Inside this, the car is in the junction and going through it whatever the
/// answer is. Well under [`STOP_SHORT`], so a car obeying a hold parks on the
/// line; this is only reached by one whose claim was taken back after it had
/// already set off, and the answer for that car is to clear the crossing, not
/// to stop across it.
const COMMITTED: f32 = STOP_SHORT * 0.6;

// Two facts about the three constants above, held by the compiler rather than
// by a test, because that is what they are. `drive_traffic` brakes the body
// origin to `STOP_SHORT` short of the node and the bonnet is `traffic::NOSE` —
// 2.6 m — in front of it, so a shorter stop line parks the nose in the
// crossing; and a car obeying a hold sits exactly on that line, so if the
// committed threshold reached it the game would read every waiting car as
// having already gone.
const _: () = assert!(STOP_SHORT > 2.6);
const _: () = assert!(COMMITTED < STOP_SHORT);

/// How far out a car starts asking for the junction it is about to enter.
///
/// Its own braking distance and then some. The floor matters as much as the
/// ceiling: a car that has already stopped at the line has to keep asking, or
/// the hold it is obeying vanishes and it lurches into the crossing.
fn asking_distance(speed: f32) -> f32 {
    STOP_SHORT + 8.0 + speed * speed / (2.0 * 3.4)
}

/// How long a car may sit still short of a crossing it has been granted
/// before it loses it.
///
/// Long enough that a car easing to a halt at the line and setting off again
/// keeps its turn — anything shorter and a junction hands itself back and
/// forth without letting anybody through. Short next to [`STUCK`], because
/// this is the ordinary case and that is the wreck.
const HESITATE: f32 = 3.0;

/// How close a car has to be to a junction to still count as being in it.
///
/// The crossing's own radius and then a car's length again. The floor is not
/// comfort: `traffic::drive_traffic` hands a car over onto its exit street at
/// `arrival_radius`, which reaches nine metres, and from that moment the only
/// thing naming the junction it is standing in is this distance. Anything
/// shorter drops the claim out from under a car that has not crossed yet, and
/// lets the conflicting movement in on top of it.
fn inside_radius(graph: &RoadGraph, at: NodeId) -> f32 {
    graph.junction_reach(at) + 9.5
}

pub struct JunctionPlugin;

impl Plugin for JunctionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Junctions>().add_systems(
            Update,
            give_way_at_junctions
                .in_set(GameSet::Ai)
                // Before the cars are driven, for the reason `giveway` gives:
                // what this decides is read while they are driven, and the
                // routes are stepped on inside `Driving`.
                .before(Driving),
        );
    }
}

/// A car that is using a junction right now: which, how far off, and how long
/// it has been standing still.
#[derive(Debug, Clone, Copy)]
struct Using {
    at: NodeId,
    away: f32,
    stopped: f32,
}

/// A car that would like to cross.
#[derive(Debug, Clone, Copy)]
struct Asking {
    driver: Entity,
    movement: Movement,
    /// What the signal facing this car says, where the crossing has one.
    aspect: Option<Aspect>,
    /// Whether it is coming up a main road. The graph has carried this since
    /// the generator was written and nothing has ever asked it a question
    /// about priority before.
    arterial: bool,
    /// How long it has been standing still already, so nobody starves.
    waited: f32,
}

fn give_way_at_junctions(
    time: Res<Time>,
    city: Res<City>,
    signals: Res<Signals>,
    mut junctions: ResMut<Junctions>,
    drivers: Query<(Entity, &TrafficDriver, &Transform, &VehicleState)>,
    // Read-only like the query above and disjoint from it by filter, so the
    // two may share `Transform` — see the trap in CLAUDE.md about two queries
    // in one system, which only bites when either is mutable.
    strangers: Query<(&Transform, &VehicleState), (With<Vehicle>, Without<TrafficDriver>)>,
) {
    let now = time.elapsed_secs();
    // The lamps' clock, not this one: under capture they are two different
    // numbers on purpose, and a car may not cross on a light nobody is showing.
    let showing = phase_clock(&time);
    let graph = &city.graph;
    let junctions = junctions.as_mut();
    junctions.holds.clear();

    // Who is still using what. Rebuilt from the ECS every frame rather than
    // signed out on the way past, for the reason `giveway` gives: a car can
    // leave a junction by being deleted, launched over a roof or driven off by
    // the player, and not one of those is a place to remember to release a
    // claim.
    let mut live: HashMap<Entity, Using> = HashMap::default();
    for (entity, driver, transform, _) in &drivers {
        let here = transform.translation.xz();
        // Approaching it, or on the way out of it and not yet clear.
        if graph.is_junction(driver.to) {
            live.insert(
                entity,
                Using {
                    at: driver.to,
                    away: here.distance(graph.node(driver.to).pos),
                    stopped: driver.waiting,
                },
            );
        }
        if graph.is_junction(driver.from) {
            let away = here.distance(graph.node(driver.from).pos);
            if away < inside_radius(graph, driver.from) {
                live.insert(
                    entity,
                    Using {
                        at: driver.from,
                        away,
                        stopped: driver.waiting,
                    },
                );
            }
        }
    }

    junctions.claims.retain(|at, claims| {
        claims.retain(|claim| {
            // Still approaching it, or still in it. Anything else — through
            // and away, deleted, or on its roof in a front garden — is not
            // using this crossing and must not be holding it.
            let Some(using) = live.get(&claim.who) else {
                return false;
            };
            if using.at != *at {
                return false;
            }
            // Asked for it, was let in, and has not come. A car granted a
            // crossing and then stopped short of it by the queue in front is
            // holding a junction it is not in and cannot enter — the
            // downstream-space problem, and the one way a claim can be both
            // perfectly live and pure loss. Everybody crossing waits behind a
            // car that is waiting behind a car. Give it back; it will ask
            // again when the queue moves, and asking again is cheap.
            if using.stopped > HESITATE && using.away > inside_radius(graph, *at) {
                return false;
            }
            if now - claim.since > STUCK {
                // Said out loud, because it has no other symptom. Everybody
                // held behind this is yielding, and yielding is a reason to be
                // stopped that the recovery timer is built to respect.
                warn!(
                    "junction: {:?} has held the crossing at {:?} for {:.0}s and it is being \
                     taken back",
                    claim.who,
                    graph.node(*at).pos,
                    now - claim.since
                );
                return false;
            }
            true
        });
        !claims.is_empty()
    });

    // Who wants in. A driver asks for the junction it is *about* to enter, and
    // only once it is close enough that the answer still leaves it room to
    // stop.
    let mut asking: HashMap<NodeId, Vec<Asking>> = HashMap::default();
    for (entity, driver, transform, state) in &drivers {
        let at = driver.to;
        if !graph.is_junction(at) {
            continue;
        }
        // Already holding this one: a car in the crossing does not ask again,
        // and asking would deadlock it against its own claim.
        if junctions
            .claims
            .get(&at)
            .is_some_and(|claims| claims.iter().any(|claim| claim.who == entity))
        {
            continue;
        }
        let away = transform.translation.xz().distance(graph.node(at).pos);
        // Too far out to be asking yet — or so close that stopping would leave
        // the car across the line it was told to wait at.
        if !(COMMITTED..=asking_distance(state.forward_speed.abs())).contains(&away) {
            continue;
        }
        let movement = Movement {
            from: driver.from,
            at,
            to: driver.after,
        };
        asking.entry(at).or_default().push(Asking {
            driver: entity,
            movement,
            // What the light on this approach is showing, if there is one.
            // `None` is not green: it is a crossing with no lights, which is
            // handed to the priority rules below instead.
            aspect: signals.aspect(at, driver.from, showing),
            arterial: graph
                .edge_between(driver.from, at)
                .is_some_and(|edge| graph.edge(edge).arterial),
            waited: driver.waiting,
        });
    }

    let mut longest = 0.0f32;
    for (at, mut queue) in asking {
        let node = graph.node(at).pos;
        // The third way a junction can be spoken for by something that will
        // never release it: somebody who is not traffic is driving through it.
        // Only a *moving* one counts. Landshut has 857 parked cars and a good
        // many of them stand within a crossing's own radius of its node, so a
        // standing car would shut junctions all over the town — and a car that
        // has genuinely stopped in one is already something the obstacle ray
        // in `drive_traffic` brakes for.
        let stranger = strangers.iter().any(|(transform, state)| {
            state.forward_speed.abs() > 1.0
                && transform.translation.xz().distance(node) < inside_radius(graph, at)
        });

        // Main road first, then whoever has nobody on their right, then the
        // longest wait, then the entity — so the answer never depends on the
        // order the query happened to hand the cars over.
        //
        // *Rechts vor links* is a rule about a pair and not an order over a
        // crowd: at a four-way where everybody has somebody on their right it
        // says nothing at all, which is exactly the deadlock real drivers
        // break by eye contact. Counting the cars on my right that I actually
        // conflict with makes it a key rather than a comparison, and the
        // deadlock resolves itself — four cars all scoring one fall through to
        // the wait and the index, and somebody goes.
        let mut yields_to: HashMap<Entity, usize> = HashMap::default();
        for ask in &queue {
            let count = queue
                .iter()
                .filter(|other| {
                    other.driver != ask.driver
                        && movements_conflict(graph, ask.movement, other.movement)
                        && on_my_right(graph, ask.movement, other.movement)
                })
                .count();
            yields_to.insert(ask.driver, count);
        }
        let rank = |ask: &Asking| yields_to.get(&ask.driver).copied().unwrap_or(0);
        queue.sort_by(|a, b| {
            b.arterial
                .cmp(&a.arterial)
                .then(rank(a).cmp(&rank(b)))
                .then(b.waited.total_cmp(&a.waited))
                .then(a.driver.index().cmp(&b.driver.index()))
        });

        let mut worst = 0.0f32;
        for ask in &queue {
            let granted = junctions.claims.entry(at).or_default();
            // A light that is not green is the end of the question: no
            // priority rule outranks a red, and a car let in on amber is one
            // stranded in the crossing when the other axis goes. Where there
            // is no light there is no `aspect`, and the rules below decide.
            //
            // The conflict test still applies *underneath* a green, and has
            // to: a two-phase signal gives one road green in both directions
            // at once, so the car turning across the oncoming lane has a green
            // and still has to wait for it. That is the same rule as at an
            // unsignalled crossing and it falls out of the same geometry.
            let stop = ask.aspect.is_some_and(|aspect| !aspect.go());
            let blocked = stranger
                || stop
                || granted
                    .iter()
                    .any(|claim| movements_conflict(graph, ask.movement, claim.movement));
            if !blocked {
                granted.push(Claim {
                    who: ask.driver,
                    movement: ask.movement,
                    since: now,
                });
                continue;
            }
            junctions.holds.insert(ask.driver, node);
            worst = worst.max(ask.waited);
        }
        longest = longest.max(worst);
    }
    // An empty vector left behind by `or_default` would keep its node in the
    // map for good and make `claimed()` a count of junctions ever used.
    junctions.claims.retain(|_, claims| !claims.is_empty());

    junctions.yielded += junctions
        .holds
        .keys()
        .filter(|car| !junctions.waited_last_frame.contains(*car))
        .count() as u64;
    junctions.waited_last_frame.clear();
    junctions
        .waited_last_frame
        .extend(junctions.holds.keys().copied());
    junctions.waiting = junctions.holds.len();
    junctions.longest_wait = longest;
}

/// Is the car making `theirs` sitting on the arm to my right?
///
/// The arm, not the car: what *rechts vor links* asks about is which road
/// somebody is coming up, and a car fifteen metres back down that road is
/// still coming up it.
fn on_my_right(graph: &RoadGraph, mine: Movement, theirs: Movement) -> bool {
    let at = graph.node(mine.at).pos;
    let approach = (at - graph.node(mine.from).pos).normalize_or_zero();
    let arm = (graph.node(theirs.from).pos - at).normalize_or_zero();
    right_of(approach).dot(arm) > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::atlas::Surface;

    /// The same four-armed crossing `roadgraph`'s tests use: west, south,
    /// east and north around a centre at the origin.
    fn crossroads() -> RoadGraph {
        let mut graph = RoadGraph::default();
        let centre = graph.add_node(Vec2::ZERO, (1, 1));
        for (i, pos) in [
            Vec2::new(-40.0, 0.0),
            Vec2::new(0.0, 40.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(0.0, -40.0),
        ]
        .into_iter()
        .enumerate()
        {
            let arm = graph.add_node(pos, (i as u16, 9));
            graph.connect(centre, arm, 7.5, false, Surface::Asphalt);
        }
        graph
    }

    const CENTRE: NodeId = NodeId(0);
    const WEST: NodeId = NodeId(1);
    const SOUTH: NodeId = NodeId(2);
    const EAST: NodeId = NodeId(3);
    const NORTH: NodeId = NodeId(4);

    fn movement(from: NodeId, to: NodeId) -> Movement {
        Movement {
            from,
            at: CENTRE,
            to,
        }
    }

    #[test]
    fn the_car_on_the_right_is_the_one_on_the_right() {
        // Heading east into the crossing, the southern arm is the one on my
        // right — `+y` here is `+z` in the world, which is the side
        // `right_of` points to for a car heading east. Getting this mirrored
        // would invert every unmarked junction in the town and look, from the
        // pavement, exactly like traffic that ignores the rule.
        let graph = crossroads();
        assert!(on_my_right(
            &graph,
            movement(WEST, EAST),
            movement(SOUTH, NORTH)
        ));
        assert!(!on_my_right(
            &graph,
            movement(WEST, EAST),
            movement(NORTH, SOUTH)
        ));
    }

    #[test]
    fn a_car_coming_the_other_way_is_on_neither_side() {
        // Directly opposite is not to the right, and must not be: two
        // opposing straights do not conflict at all, and a rule that made one
        // of them wait for the other would stop a road that has right of way
        // over itself.
        let graph = crossroads();
        assert!(!on_my_right(
            &graph,
            movement(WEST, EAST),
            movement(EAST, WEST)
        ));
    }

    #[test]
    fn a_car_keeps_asking_after_it_has_stopped() {
        // The floor on the asking window is not decoration. A car parked on
        // the line is doing nought, so its asking distance is the shortest it
        // will ever be; if that fell short of where it is standing, the hold
        // would disappear from under it and it would lurch into the crossing.
        assert!(
            asking_distance(0.0) > STOP_SHORT,
            "a stopped car at the line has stopped asking for the junction"
        );
    }
}
