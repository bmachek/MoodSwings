//! Who gets the piece of road that two cars cannot both have.
//!
//! A town read off a map is not a grid, and the part of it the generator never
//! had to think about is the Gasse. Landshut's bake floors a street at four
//! metres and 45% of its forty-five kilometres of road is under four and a
//! half, which on [`steering::single_file`](super::steering::single_file)'s
//! arithmetic is a carriageway two cars physically cannot pass on. The traffic
//! did not know that. It sent cars down them in both directions, the cars met,
//! touched, and stopped — and because a single forward ray at 1.9 m of lateral
//! separation cannot see the car it is pressed against, neither driver could
//! even say what had stopped it. Twenty-six seconds later `traffic::GIVE_UP`
//! deleted them both and called it a recovery. Two ninety-second patrols
//! recorded 42 and 49 of those, and 63% were on a four-metre street.
//!
//! The fix is not to keep the traffic out of the old town. Banning the narrow
//! streets shatters the drivable network into eighty-eight pieces, the largest
//! a tenth of the whole, because a main street pinched between two houses for
//! thirty metres cuts off everything beyond it. The fix is the one the real
//! Altstadt uses: a Gasse is single file, and you wait at the mouth for the
//! one coming the other way.
//!
//! So this owns two things and will own a third:
//!
//! * **Runs.** The single-file stretches, worked out once from the layout. A
//!   run reaches from one passing place to the next, and a passing place is a
//!   junction — three streets meeting leave somewhere to pull aside, a bend
//!   does not. Landshut has 184 of them among its 1565 streets: median 84 m,
//!   longest 481 m, and 102 with only one way in.
//! * **Right of way over a run.** Whoever is in it has it, and cars behind
//!   them going the same way may follow. The moment anybody is standing at the
//!   far mouth the near mouth stops admitting, the run drains, and the far end
//!   takes it. That alternation is the whole starvation rule.
//! * Junctions, next: the movements that conflict at a crossing and who yields
//!   to whom. [`crate::world::roadgraph::movements_conflict`] is the policy
//!   waiting for a caller.
//!
//! What this is *not* is a way of deleting cars. A car held at a mouth is
//! waiting, not blocked — `traffic::DriverObservation::Yielding` says so and
//! the recovery timer leaves it alone — which is exactly why the hold has to
//! be sound rather than merely conservative. Every way a record can outlive
//! the cars it was about has its own timeout below.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::steering::single_file;
use super::traffic::{Driving, TrafficDriver};
use crate::core::schedule::GameSet;
use crate::vehicle::controller::VehicleState;
use crate::world::City;
use crate::world::roadgraph::{EdgeId, NodeId, RoadGraph};

/// One single-file stretch, from one passing place to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RunId(pub u32);

/// The single-file stretches of the town.
///
/// Derived from the layout, which is resident and does not change while the
/// game runs, so this is worked out once rather than asked per frame. Plain
/// data over the graph, so the derivation is testable on a hand-built network.
#[derive(Resource, Debug, Default)]
pub struct Runs {
    /// Which run each edge belongs to, indexed by `EdgeId`.
    of_edge: Vec<Option<RunId>>,
    /// How long each run is, end to end, in metres.
    metres: Vec<f32>,
    /// How many junctions it can be entered by. Two for an ordinary Gasse
    /// between two streets; one for a cul-de-sac; none for a loop of alleys
    /// nothing else touches, which no car can reach in the first place.
    mouths: Vec<u8>,
}

impl Runs {
    /// Flood-fills the single-file edges into runs.
    ///
    /// Two narrow edges are the same run when they meet at a *bend* — a node
    /// with exactly two streets at it. At anything else the run ends: a
    /// junction is where a car can pull aside, and a dead end is where it
    /// turns round. Without that rule the whole Altstadt is one run and one
    /// car at a time is allowed into the old town.
    pub fn of(graph: &RoadGraph) -> Self {
        let mut of_edge = vec![None; graph.edge_count()];
        let mut metres = Vec::new();
        let mut mouth_count: Vec<u8> = Vec::new();
        for index in 0..graph.edge_count() {
            let first = EdgeId(index as u32);
            if of_edge[index].is_some() || !single_file(graph.edge(first).width) {
                continue;
            }
            let run = RunId(metres.len() as u32);
            of_edge[index] = Some(run);
            let mut length = 0.0;
            let mut mouths: Vec<NodeId> = Vec::new();
            let mut pending = vec![first];
            while let Some(edge) = pending.pop() {
                length += graph.edge(edge).length;
                for end in [graph.edge(edge).a, graph.edge(edge).b] {
                    if graph.node(end).edges.len() != 2 {
                        // Where the run meets the rest of the town. A dead end
                        // is not one of these: a car can turn round there, but
                        // it cannot arrive there.
                        if graph.node(end).edges.len() >= 3 && !mouths.contains(&end) {
                            mouths.push(end);
                        }
                        continue;
                    }
                    for (_, next) in graph.neighbors(end) {
                        let slot = next.0 as usize;
                        if of_edge[slot].is_none() && single_file(graph.edge(next).width) {
                            of_edge[slot] = Some(run);
                            pending.push(next);
                        }
                    }
                }
            }
            metres.push(length);
            mouth_count.push(mouths.len().min(u8::MAX as usize) as u8);
        }
        Self {
            of_edge,
            metres,
            mouths: mouth_count,
        }
    }

    pub fn of_edge(&self, edge: EdgeId) -> Option<RunId> {
        self.of_edge.get(edge.0 as usize).copied().flatten()
    }

    /// The run of the street between two junctions, if it is single file.
    pub fn between(&self, graph: &RoadGraph, from: NodeId, to: NodeId) -> Option<RunId> {
        edge_between(graph, from, to).and_then(|edge| self.of_edge(edge))
    }

    pub fn count(&self) -> usize {
        self.metres.len()
    }

    pub fn metres(&self, run: RunId) -> f32 {
        self.metres.get(run.0 as usize).copied().unwrap_or_default()
    }

    /// A run with one way in and out holds one car, not a convoy.
    ///
    /// A hundred and two of Landshut's hundred and eighty-four runs are that
    /// shape — more than half, which was the surprise — and the car at the
    /// bottom of one is coming back. Direction cannot be named by the mouth a
    /// car went in by when there is only one mouth, so the rule that lets a
    /// queue follow its leader in has to be turned off: the second car would
    /// be let in behind the first and meet it coming out.
    fn one_at_a_time(&self, run: RunId) -> bool {
        self.mouths.get(run.0 as usize).copied().unwrap_or(0) < 2
    }

    /// How long one direction may hold a run before the record is assumed to
    /// be about cars that no longer exist. A van knocked onto its roof in a
    /// Gasse must not close it for the rest of the session, and a five hundred
    /// metre run legitimately takes a minute to walk a convoy through.
    fn patience(&self, run: RunId) -> f32 {
        self.metres(run) / CRAWL + GRACE
    }
}

/// The edge joining two junctions, if they are joined at all.
fn edge_between(graph: &RoadGraph, from: NodeId, to: NodeId) -> Option<EdgeId> {
    graph
        .neighbors(from)
        .find(|(node, _)| *node == to)
        .map(|(_, edge)| edge)
}

/// Which end of a run the cars in it went in by, and when.
///
/// Three timestamps because a record can outlive its cars in three different
/// ways, and each wants a different answer.
#[derive(Debug, Clone, Copy)]
struct Held {
    /// The junction they entered by. A run is a chain, so naming the end is
    /// enough to name the direction — except on a run with one mouth, where
    /// it is not, which is what `holder` is for.
    entered: NodeId,
    /// The car most recently let in. Only load-bearing for a cul-de-sac,
    /// where one car at a time means *that* car and not merely that mouth.
    holder: Option<Entity>,
    /// Last moment a car was inside it *or* was let in. A grant has to survive
    /// the second or two before the car actually arrives, or two cars at
    /// opposite mouths are both let in.
    seen: f32,
    /// Last moment a car was actually inside. What decides the handover: once
    /// the run has genuinely emptied and somebody is waiting at the far end,
    /// the record goes and they take it.
    used: f32,
    /// When this direction took the run over. What decides giving up on it.
    since: f32,
}

/// Who is waiting for what, and where they have to stop.
#[derive(Resource, Default)]
pub struct GiveWay {
    runs: HashMap<RunId, Held>,
    /// Where each held driver must stop short of, this frame.
    holds: HashMap<Entity, Vec2>,
    /// And who was held last frame, so a stop is counted once rather than
    /// sixty times a second.
    waited_last_frame: bevy::platform::collections::HashSet<Entity>,
    /// For the patrol and the dev panel, which would otherwise have to infer
    /// all of this from cars standing still.
    pub waiting: usize,
    pub longest_wait: f32,
    /// How many times a car has been sent to the back of somebody else's
    /// street since the session started. Cumulative and counted once per
    /// stop, so a run with nothing in it reads as zero rather than as a rule
    /// that is working — the difference between "it never fires" and "it
    /// fires and nobody waits" is the whole question of whether this earns
    /// its keep.
    pub stood_aside: u64,
}

impl GiveWay {
    /// The junction this driver has to stop before, if it is being held.
    pub fn hold(&self, driver: Entity) -> Option<Vec2> {
        self.holds.get(&driver).copied()
    }

    pub fn occupied_runs(&self) -> usize {
        self.runs.len()
    }
}

/// How long a record outlives all activity — nobody inside, nobody asking.
const FORGET: f32 = 3.0;

/// How long a run stays reserved for its direction after the last car has
/// left it, while somebody waits at the other end. Short, because this is the
/// pause between one direction and the next; longer than a frame, because a
/// convoy has gaps in it and a car granted entry needs time to arrive.
const HANDOVER: f32 = 1.5;

/// The slowest a car could be crawling down a run and still be getting
/// somewhere, in m/s, and the grace on top of a whole run at that pace.
const CRAWL: f32 = 2.5;
const GRACE: f32 = 25.0;

/// How much room a car wants between itself and the mouth it waits at.
///
/// `traffic::NOSE` is 2.6 m ahead of the body origin, so this leaves the
/// bonnet about a metre short of the junction rather than in the middle of it.
pub const STOP_SHORT: f32 = 3.6;

/// Inside this, a car is in the mouth and has committed to the street.
///
/// Well under [`STOP_SHORT`], so a car obeying a hold parks on the line and
/// stays there; this is only reached by one whose grant was taken back after
/// it had already set off, and the answer for that car is to go, not to stop
/// across the junction.
const COMMITTED: f32 = STOP_SHORT * 0.6;

/// How far out a car starts asking for the run it is about to enter.
///
/// Its own braking distance and then some: asking later than it can stop is
/// asking after the answer has stopped mattering, and the floor matters just
/// as much — a car that has *already* stopped short of the mouth has to keep
/// asking, or the hold it is obeying disappears and it lurches forward.
fn asking_distance(speed: f32) -> f32 {
    STOP_SHORT + 8.0 + speed * speed / (2.0 * 3.4)
}

pub struct GiveWayPlugin;

impl Plugin for GiveWayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Runs>()
            .init_resource::<GiveWay>()
            .add_systems(PostStartup, remember_runs)
            .add_systems(
                Update,
                give_way
                    .in_set(GameSet::Ai)
                    // Before the cars are driven, because what it decides is
                    // read while they are. The routes are stepped on inside
                    // `Driving` too, so this sees the street each car was on
                    // last frame — which is the street it is still on, since
                    // the handover happens within a couple of metres of the
                    // junction and the ask reaches ten.
                    .before(Driving),
            );
    }
}

/// Works the single-file runs out of the layout, once.
///
/// `PostStartup`, because `world::generate_city` inserts the layout in
/// `Startup` and this is a pure function of it. The same silently load-bearing
/// ordering `figure::FigureAssets` has.
fn remember_runs(mut commands: Commands, city: Option<Res<City>>) {
    let Some(city) = city else {
        return;
    };
    let runs = Runs::of(&city.graph);
    let mut lengths: Vec<f32> = (0..runs.count())
        .map(|i| runs.metres(RunId(i as u32)))
        .collect();
    lengths.sort_by(f32::total_cmp);
    info!(
        "give way: {} single-file runs among {} streets, {} of them with one \
         way in, median {:.0}m, longest {:.0}m",
        runs.count(),
        city.graph.edge_count(),
        (0..runs.count())
            .filter(|i| runs.one_at_a_time(RunId(*i as u32)))
            .count(),
        lengths.get(lengths.len() / 2).copied().unwrap_or_default(),
        lengths.last().copied().unwrap_or_default(),
    );
    commands.insert_resource(runs);
}

/// One car's request to be let into a run.
#[derive(Debug, Clone, Copy)]
struct Asking {
    driver: Entity,
    /// The junction it would go in by.
    mouth: NodeId,
    /// How long it has been standing still already, so the longest wait wins.
    waited: f32,
}

fn give_way(
    time: Res<Time>,
    city: Res<City>,
    runs: Res<Runs>,
    mut giveway: ResMut<GiveWay>,
    drivers: Query<(Entity, &TrafficDriver, &Transform, &VehicleState)>,
) {
    if runs.count() == 0 {
        return;
    }
    let now = time.elapsed_secs();
    let graph = &city.graph;
    let giveway = giveway.as_mut();
    giveway.holds.clear();

    // Who is in a run already. Rebuilt from the ECS every frame rather than
    // patched on entry and exit, for the same reason `resident::reconcile` is:
    // a car can leave a run by being deleted, launched onto a roof or driven
    // off by the player, and not one of those is a place to remember to sign
    // out. Which car is credited with an unrecorded run does depend on query
    // order — it only happens to a car that started inside one, and
    // `traffic::maintain_population` does not put them there.
    let mut inside: HashMap<RunId, NodeId> = HashMap::default();
    for (_, driver, _, _) in &drivers {
        if let Some(run) = runs.between(graph, driver.from, driver.to) {
            inside.entry(run).or_insert(driver.from);
        }
    }

    // Who wants in. A driver asks for the street it is *about* to enter, and
    // only once it is close enough that the answer still leaves it room to
    // stop.
    let mut asking: HashMap<RunId, Vec<Asking>> = HashMap::default();
    for (entity, driver, transform, state) in &drivers {
        let Some(run) = runs.between(graph, driver.to, driver.after) else {
            continue;
        };
        // Already in it: carrying on down the same run needs nobody's
        // permission, and asking for it would deadlock it against itself.
        if runs.between(graph, driver.from, driver.to) == Some(run) {
            continue;
        }
        let mouth = graph.node(driver.to).pos;
        let away = transform.translation.xz().distance(mouth);
        // Too far out to be asking yet — or so close that stopping would
        // leave the car across the mouth it was told to wait at. A stop line
        // behind the nose is what `arrival_radius` exists to avoid on the
        // steering side: `stopping_speed` of a gap already passed is zero, and
        // a car braking to zero inside the junction never leaves it.
        if !(COMMITTED..=asking_distance(state.forward_speed.abs())).contains(&away) {
            continue;
        }
        asking.entry(run).or_default().push(Asking {
            driver: entity,
            mouth: driver.to,
            waited: driver.waiting,
        });
    }

    for (run, held) in giveway.runs.iter_mut() {
        if inside.contains_key(run) {
            held.seen = now;
            held.used = now;
        }
    }
    for (&run, &from) in &inside {
        giveway.runs.entry(run).or_insert(Held {
            entered: from,
            holder: None,
            seen: now,
            used: now,
            since: now,
        });
    }
    giveway.runs.retain(|_, held| now - held.seen < FORGET);
    // The handover, and the only two ways a direction loses a run it has.
    //
    // Both need somebody actually waiting the other way: an uncontested run
    // belongs to whoever is driving down it for as long as they like, and a
    // record nobody is arguing with harms nobody. Given a contest, either the
    // run has emptied — the ordinary case, and the pause is just long enough
    // that a convoy's gaps do not count as empty — or it has not emptied in
    // the time a whole run at a crawl would take, in which case what is in
    // there is not driving anywhere. That second one is a van on its roof in
    // a Gasse, and it must not close the street for the rest of the session.
    for (run, queue) in &asking {
        let Some(held) = giveway.runs.get(run) else {
            continue;
        };
        if !queue.iter().any(|ask| ask.mouth != held.entered) {
            continue;
        }
        let drained = now - held.used > HANDOVER;
        let abandoned = now - held.since > runs.patience(*run);
        if abandoned {
            // Said out loud. A wreck in a Gasse is the one failure this whole
            // module has no other symptom for: everybody waiting for it is
            // *waiting*, which is neither blocked nor recovered nor counted.
            warn!(
                "give way: a {:.0}m run has been held for {:.0}s and is being taken back",
                runs.metres(*run),
                now - held.since
            );
        }
        if drained || abandoned {
            giveway.runs.remove(run);
        }
    }

    let mut longest = 0.0f32;
    for (run, mut queue) in asking {
        // Longest wait first, then by entity, so the answer does not depend on
        // the order the query happened to hand the cars over.
        queue.sort_by(|a, b| {
            b.waited
                .total_cmp(&a.waited)
                .then(a.driver.index().cmp(&b.driver.index()))
        });
        let single = runs.one_at_a_time(run);
        let record = giveway.runs.get(&run).copied();
        let open = match record {
            // Being driven, and somebody is waiting the other way: nobody new
            // goes in at either end and the run drains. Reserved but empty —
            // a car on its way to the mouth — keeps its direction, or the
            // grant would be taken back before it could be used, and the two
            // ends would hand it back and forth for ever without either
            // actually going anywhere.
            Some(held) => {
                let contested = single || queue.iter().any(|ask| ask.mouth != held.entered);
                (!contested || !inside.contains_key(&run)).then_some(held.entered)
            }
            None => Some(queue[0].mouth),
        };
        let mut holder = record.and_then(|held| held.holder);
        let mut worst = 0.0f32;
        for ask in &queue {
            // On a cul-de-sac the mouth does not name a direction, so the
            // grant is to a car rather than to an end of the street.
            let theirs = !single || holder.is_none_or(|car| car == ask.driver);
            if open == Some(ask.mouth) && theirs {
                holder = Some(ask.driver);
                let record = giveway.runs.entry(run).or_insert(Held {
                    entered: ask.mouth,
                    holder,
                    seen: now,
                    used: now,
                    since: now,
                });
                record.holder = holder;
                record.seen = now;
                // A car that has been let in and is on its way counts as
                // using the street. Otherwise the handover pause expires
                // under it while it is still driving to the mouth.
                record.used = now;
                continue;
            }
            giveway.holds.insert(ask.driver, graph.node(ask.mouth).pos);
            worst = worst.max(ask.waited);
        }
        if worst > runs.patience(run) {
            warn!(
                "give way: somebody has waited {worst:.0}s at a {:.0}m run",
                runs.metres(run)
            );
        }
        longest = longest.max(worst);
    }

    giveway.stood_aside += giveway
        .holds
        .keys()
        .filter(|car| !giveway.waited_last_frame.contains(*car))
        .count() as u64;
    giveway.waited_last_frame.clear();
    giveway
        .waited_last_frame
        .extend(giveway.holds.keys().copied());
    giveway.waiting = giveway.holds.len();
    giveway.longest_wait = longest;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::atlas::Surface;

    /// A wide street, a narrow chain of three off it, and a wide one at the
    /// far end: one run, with a passing place at each mouth.
    fn alley() -> RoadGraph {
        gasse(4.0)
    }

    /// The same three streets at whatever width the caller wants them.
    fn gasse(middle: f32) -> RoadGraph {
        let mut graph = RoadGraph::default();
        for (i, pos) in [
            Vec2::new(-40.0, 0.0),
            Vec2::ZERO,
            Vec2::new(20.0, 0.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(60.0, 0.0),
            Vec2::new(100.0, 0.0),
            // A side street at each mouth, so both are junctions.
            Vec2::new(0.0, -30.0),
            Vec2::new(60.0, -30.0),
        ]
        .into_iter()
        .enumerate()
        {
            graph.add_node(pos, (i as u16, 0));
        }
        let wide = 12.0;
        graph.connect(NodeId(0), NodeId(1), wide, true, Surface::Asphalt);
        graph.connect(NodeId(1), NodeId(2), middle, false, Surface::Sett);
        graph.connect(NodeId(2), NodeId(3), middle, false, Surface::Sett);
        graph.connect(NodeId(3), NodeId(4), middle, false, Surface::Sett);
        graph.connect(NodeId(4), NodeId(5), wide, true, Surface::Asphalt);
        graph.connect(NodeId(1), NodeId(6), wide, false, Surface::Asphalt);
        graph.connect(NodeId(4), NodeId(7), wide, false, Surface::Asphalt);
        graph
    }

    #[test]
    fn a_chain_of_narrow_streets_is_one_run() {
        let graph = alley();
        let runs = Runs::of(&graph);
        assert_eq!(runs.count(), 1);
        let run = runs.between(&graph, NodeId(1), NodeId(2)).unwrap();
        assert_eq!(runs.between(&graph, NodeId(2), NodeId(3)), Some(run));
        assert_eq!(runs.between(&graph, NodeId(3), NodeId(4)), Some(run));
        // Driving it the other way is the same piece of road.
        assert_eq!(runs.between(&graph, NodeId(4), NodeId(3)), Some(run));
        // The wide streets at either end are nobody's run.
        assert_eq!(runs.between(&graph, NodeId(0), NodeId(1)), None);
        assert_eq!(runs.between(&graph, NodeId(4), NodeId(5)), None);
        assert!((runs.metres(run) - 60.0).abs() < 1e-3);
    }

    #[test]
    fn a_junction_ends_a_run() {
        // The same alley with a third street at its middle node. A car can
        // pull aside there, so what was one run is two — and one car at a
        // time in the whole old town is exactly what that avoids.
        let mut graph = alley();
        let side = graph.add_node(Vec2::new(20.0, -30.0), (20, 0));
        graph.connect(NodeId(2), side, 12.0, false, Surface::Asphalt);
        let runs = Runs::of(&graph);
        assert_eq!(runs.count(), 2);
        assert_ne!(
            runs.between(&graph, NodeId(1), NodeId(2)),
            runs.between(&graph, NodeId(2), NodeId(3))
        );
    }

    #[test]
    fn a_street_two_cars_fit_on_is_not_a_run_at_all() {
        let mut graph = RoadGraph::default();
        graph.add_node(Vec2::ZERO, (0, 0));
        graph.add_node(Vec2::new(50.0, 0.0), (1, 0));
        graph.connect(NodeId(0), NodeId(1), 7.5, false, Surface::Asphalt);
        assert_eq!(Runs::of(&graph).count(), 0);
        // And a street nobody can pass on is, whatever else is true of it.
        let mut graph = graph;
        graph.add_node(Vec2::new(50.0, 50.0), (2, 0));
        graph.connect(NodeId(1), NodeId(2), 4.0, true, Surface::Asphalt);
        assert_eq!(Runs::of(&graph).count(), 1);
    }

    fn town(graph: RoadGraph) -> App {
        let mut app = App::new();
        let runs = Runs::of(&graph);
        app.init_resource::<Time>()
            .init_resource::<GiveWay>()
            .insert_resource(City(crate::world::citygen::CityLayout {
                seed: 1,
                half_extent: 500.0,
                x_streets: Vec::new(),
                z_streets: Vec::new(),
                blocks: Vec::new(),
                graph,
                canal: None,
                grounds: Vec::new(),
                waters: Vec::new(),
                relief: None,
            }))
            .insert_resource(runs)
            .add_systems(Update, give_way);
        app
    }

    /// A car on the street `from -> to`, standing `back` metres short of `to`.
    fn car(app: &mut App, from: NodeId, to: NodeId, after: NodeId, back: f32) -> Entity {
        let graph = &app.world().resource::<City>().graph;
        let end = graph.node(to).pos;
        let at = end + (graph.node(from).pos - end).normalize_or_zero() * back;
        app.world_mut()
            .spawn((
                TrafficDriver {
                    from,
                    to,
                    after,
                    turn: crate::world::roadgraph::TurnKind::Straight,
                    lane_width: 4.0,
                    cruise_speed: 8.0,
                    stuck: 0.0,
                    honked: false,
                    waiting: 0.0,
                    observation: super::super::traffic::DriverObservation::Clear,
                    desired_speed: 0.0,
                },
                Transform::from_xyz(at.x, 0.0, at.y),
                VehicleState::default(),
            ))
            .id()
    }

    fn wait(app: &mut App, seconds: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(seconds));
    }

    fn held(app: &App, car: Entity) -> bool {
        app.world().resource::<GiveWay>().hold(car).is_some()
    }

    #[test]
    fn one_end_of_a_gasse_gets_it_and_the_other_waits() {
        let mut app = town(alley());
        let west = car(&mut app, NodeId(0), NodeId(1), NodeId(2), 8.0);
        let east = car(&mut app, NodeId(5), NodeId(4), NodeId(3), 8.0);
        app.update();
        assert_ne!(
            held(&app, west),
            held(&app, east),
            "either both cars were let into a street they cannot pass on, or neither was"
        );
        // And the answer does not wander from frame to frame.
        let waiting = held(&app, west);
        for _ in 0..5 {
            wait(&mut app, 0.1);
            app.update();
            assert_eq!(held(&app, west), waiting);
        }
    }

    #[test]
    fn a_car_already_in_it_keeps_it_and_a_convoy_follows() {
        let mut app = town(alley());
        // One inside, one behind it at the same mouth, one at the far end.
        car(&mut app, NodeId(1), NodeId(2), NodeId(3), 5.0);
        let behind = car(&mut app, NodeId(0), NodeId(1), NodeId(2), 8.0);
        let against = car(&mut app, NodeId(5), NodeId(4), NodeId(3), 8.0);
        app.update();
        assert!(
            held(&app, against),
            "a car was sent into a Gasse against the one already in it"
        );
        // The convoy is held too, but only because somebody is waiting the
        // other way: that is the whole starvation rule. With nobody waiting it
        // follows its leader in.
        assert!(held(&app, behind));
        app.world_mut().despawn(against);
        wait(&mut app, 0.1);
        app.update();
        assert!(
            !held(&app, behind),
            "a car was kept out of a street its own leader is driving down"
        );
    }

    #[test]
    fn an_emptied_run_changes_hands() {
        let mut app = town(alley());
        let inside = car(&mut app, NodeId(1), NodeId(2), NodeId(3), 5.0);
        let against = car(&mut app, NodeId(5), NodeId(4), NodeId(3), 8.0);
        app.update();
        assert!(held(&app, against));
        // It drives out of the far end; the waiting car may not go in until
        // the handover pause has passed, and then must.
        app.world_mut().despawn(inside);
        wait(&mut app, 0.2);
        app.update();
        assert!(
            held(&app, against),
            "the far end was let in while the run was still warm"
        );
        wait(&mut app, HANDOVER + 0.2);
        app.update();
        assert!(
            !held(&app, against),
            "the run never changed hands and the far end waits for ever"
        );
    }

    #[test]
    fn a_grant_is_not_taken_back_before_it_can_be_used() {
        // The livelock this nearly shipped with. A car let into a Gasse has to
        // drive to the mouth first, and all the while the car at the far end
        // is standing still and its wait is growing — so it becomes the
        // longest waiter, takes the run off a car that has not moved yet, and
        // then the same thing happens to it. Both ends hand the street back
        // and forth and neither ever enters.
        let mut app = town(alley());
        let west = car(&mut app, NodeId(0), NodeId(1), NodeId(2), 8.0);
        let east = car(&mut app, NodeId(5), NodeId(4), NodeId(3), 8.0);
        app.world_mut()
            .get_mut::<TrafficDriver>(east)
            .unwrap()
            .waiting = 20.0;
        app.update();
        assert!(
            held(&app, west),
            "the longest waiter did not get the street"
        );
        assert!(!held(&app, east));
        // Now the other one has been standing there twice as long, and the
        // car that was let in still has not reached the mouth.
        app.world_mut()
            .get_mut::<TrafficDriver>(west)
            .unwrap()
            .waiting = 40.0;
        for _ in 0..6 {
            wait(&mut app, HANDOVER * 0.5);
            app.update();
            assert!(
                !held(&app, east),
                "the grant was taken back from a car that was still on its way to the mouth"
            );
        }
    }

    #[test]
    fn a_cul_de_sac_takes_one_car_at_a_time() {
        // One way in is one way out, so "the same mouth" no longer means "the
        // same direction": the car at the bottom has turned round and is
        // coming back up the street the next one would be let into. Forty-five
        // of Landshut's two hundred runs are this shape.
        let mut graph = RoadGraph::default();
        for (i, pos) in [
            Vec2::new(-40.0, 0.0),
            Vec2::ZERO,
            Vec2::new(20.0, 0.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(0.0, -30.0),
        ]
        .into_iter()
        .enumerate()
        {
            graph.add_node(pos, (i as u16, 0));
        }
        graph.connect(NodeId(0), NodeId(1), 12.0, true, Surface::Asphalt);
        graph.connect(NodeId(1), NodeId(4), 12.0, false, Surface::Asphalt);
        graph.connect(NodeId(1), NodeId(2), 4.0, false, Surface::Sett);
        graph.connect(NodeId(2), NodeId(3), 4.0, false, Surface::Sett);
        let runs = Runs::of(&graph);
        assert_eq!(runs.count(), 1);
        assert!(runs.one_at_a_time(RunId(0)));

        let mut app = town(graph);
        car(&mut app, NodeId(1), NodeId(2), NodeId(3), 5.0);
        let behind = car(&mut app, NodeId(0), NodeId(1), NodeId(2), 8.0);
        app.update();
        assert!(
            held(&app, behind),
            "a second car was sent up a cul-de-sac behind the one turning round in it"
        );
    }

    #[test]
    fn a_wreck_in_a_gasse_gives_the_street_back() {
        // Nothing else in the game would notice. Everybody queued behind a
        // car that cannot move is *waiting*, which is neither blocked nor
        // recovered, so the street would simply be shut for the session.
        let mut app = town(alley());
        let wreck = car(&mut app, NodeId(1), NodeId(2), NodeId(3), 5.0);
        let against = car(&mut app, NodeId(5), NodeId(4), NodeId(3), 8.0);
        app.update();
        assert!(held(&app, against));
        // It never moves and never despawns.
        for _ in 0..8 {
            wait(&mut app, 8.0);
            app.update();
        }
        assert!(
            app.world().get::<TrafficDriver>(wreck).is_some(),
            "the wreck was supposed to still be there"
        );
        assert!(
            !held(&app, against),
            "a car that cannot move kept a street shut for ever"
        );
    }

    #[test]
    fn an_uncontested_run_is_nobodys_business_however_long_it_takes() {
        // The other half of the same rule: patience is only ever spent on an
        // argument. A lorry pottering down a long Gasse with nobody waiting
        // must not have its own street taken off it.
        let mut app = town(alley());
        car(&mut app, NodeId(1), NodeId(2), NodeId(3), 5.0);
        let behind = car(&mut app, NodeId(0), NodeId(1), NodeId(2), 8.0);
        for _ in 0..8 {
            wait(&mut app, 8.0);
            app.update();
        }
        assert!(!held(&app, behind));
    }

    #[test]
    fn nobody_is_held_for_a_street_two_cars_fit_on() {
        // The same town with the Gasse widened, and nothing else changed.
        let mut app = town(gasse(9.0));
        let west = car(&mut app, NodeId(0), NodeId(1), NodeId(2), 8.0);
        let east = car(&mut app, NodeId(5), NodeId(4), NodeId(3), 8.0);
        app.update();
        assert!(!held(&app, west) && !held(&app, east));
    }

    #[test]
    fn the_real_town_is_full_of_them_but_not_made_of_one() {
        // The whole point of ending a run at a junction, on the town this was
        // written for. One run would mean one car at a time in the Altstadt.
        let Some(atlas) = crate::world::atlas::load("landshut") else {
            panic!("the committed Landshut atlas should load");
        };
        let (layout, _) = crate::world::atlas::layout(&atlas, 1, 1000.0);
        let runs = Runs::of(&layout.graph);
        assert!(
            runs.count() > 100,
            "{} runs is too few for a town this narrow",
            runs.count()
        );
        let longest = (0..runs.count())
            .map(|i| runs.metres(RunId(i as u32)))
            .fold(0.0f32, f32::max);
        assert!(
            longest < 600.0,
            "a {longest:.0}m single-file run is most of the old town at once"
        );
    }
}
