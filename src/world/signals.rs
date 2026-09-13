//! What the lights say, and who they say it to.
//!
//! The masts have stood on every arterial crossing in the town since
//! `world::props` learned to build them, and every one of them has been dark.
//! The comment beside the three lens colours said why, and it was the right
//! answer at the time: "a signal showing green down every approach of a
//! crossroads at once would be a clearer lie than one showing nothing". A
//! signal can only show a phase if something is keeping one, and nothing was.
//!
//! [`ai::junction`](crate::ai::junction) is what changed. Once a crossing has
//! movements that conflict and a rule for who takes it, a phase is just that
//! rule written where the traffic — and the player — can read it, and the
//! lenses have something true to show.
//!
//! Two things about the shape of this are deliberate.
//!
//! **The phase is a pure function of the junction and the clock**, not a state
//! machine ticking on a component. Signals are spawned by chunk streaming, so
//! a crossing the player drives away from and comes back to is a *different*
//! set of entities; a machine on the entity would restart its cycle every time
//! the street came back, and the lights would change when you looked at them.
//! It also means `ai::junction` can ask about a crossing whose masts are not
//! spawned at all, which is most of the town.
//!
//! **A capture holds the clock still.** `core::capture` freezes time of day so
//! the warmup frames do not drift the sky, but frame times are not identical
//! between runs, so anything read off `elapsed_secs` would land on a different
//! phase in the before shot and the after one — and `tools/shoot.sh` exists to
//! compare those two. Under capture the phase comes from the junction alone:
//! stable across runs, and still different from one crossing to the next, so a
//! shot of a street shows lights rather than a row of identical reds.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::City;
use super::roadgraph::{NodeId, RoadGraph};
use crate::core::schedule::GameSet;

/// What one approach to a junction is being shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aspect {
    Red,
    /// Red and amber together — the German pause before green. Worth having
    /// for two seconds of every cycle because it is the half-second of warning
    /// that makes a queue set off as a queue rather than one car at a time.
    RedAmber,
    Green,
    Amber,
}

impl Aspect {
    /// Which lenses are lit, in the order `world::props` hangs them: red at
    /// the top, amber in the middle, green at the bottom.
    pub fn lit(self) -> [bool; 3] {
        match self {
            Aspect::Red => [true, false, false],
            Aspect::RedAmber => [true, true, false],
            Aspect::Green => [false, false, true],
            Aspect::Amber => [false, true, false],
        }
    }

    /// Whether a car that has not yet committed to the crossing may enter.
    ///
    /// Green only. Amber is the end of a phase and the cars still in the
    /// junction are clearing it — letting a new one in on amber is how a
    /// crossing ends up with somebody stranded in the middle of it when the
    /// other axis goes green. A car already inside does not ask: the asking
    /// window in `ai::junction` has a floor for exactly that.
    pub fn go(self) -> bool {
        matches!(self, Aspect::Green)
    }
}

/// How long each part of a phase lasts, in seconds.
///
/// One axis at a time, with an all-red gap between them so the junction
/// genuinely empties. The gap is what makes the rule safe rather than merely
/// fair — `ai::junction` still holds the conflict test underneath, so this is
/// belt and braces, and a crossing is the one place in this game where being
/// twice as careful costs two seconds and being half as careful costs a car.
const RED_AMBER: f32 = 2.0;
const GREEN: f32 = 14.0;
const AMBER: f32 = 3.0;
const CLEAR: f32 = 2.0;

/// One axis's turn, and then the other's.
const HALF: f32 = RED_AMBER + GREEN + AMBER + CLEAR;
const CYCLE: f32 = HALF * 2.0;

/// Which arms of one signalled junction go together.
#[derive(Debug, Clone)]
struct Signal {
    /// The arm's far node, and which half of the cycle it moves in.
    arms: HashMap<NodeId, u8>,
    /// Where in the cycle this junction starts, so the town does not blink in
    /// unison. Deterministic in the node id, like everything else the layout
    /// derives — see the determinism rule in CLAUDE.md.
    offset: f32,
}

/// Every junction in the town that has lights on it.
#[derive(Resource, Debug, Default)]
pub struct Signals {
    at: HashMap<NodeId, Signal>,
}

impl Signals {
    /// Works the signalled junctions out of the layout, once.
    ///
    /// The test for *which* junctions is the same one `props::spawn_junction`
    /// applies when it decides whether to build the masts — three arms or
    /// more, at least one of them a main road. The two have to agree or the
    /// town grows either a phase with no lamp or a lamp with no phase, and
    /// neither would fail loudly.
    pub fn of(graph: &RoadGraph) -> Self {
        let mut at = HashMap::default();
        for (id, node) in graph.nodes() {
            if node.edges.len() < 3 {
                continue;
            }
            if !node.edges.iter().any(|&e| graph.edge(e).arterial) {
                continue;
            }
            let here = node.pos;
            // The main axis is the widest arm's bearing. A signalled crossing
            // is signalled because a main road runs through it, and the main
            // road is the one the phase should be named after.
            let mut bearings: Vec<(NodeId, Vec2, f32)> = node
                .edges
                .iter()
                .filter_map(|&e| {
                    let edge = graph.edge(e);
                    let other = if edge.a == id { edge.b } else { edge.a };
                    let direction = (graph.node(other).pos - here).try_normalize()?;
                    Some((other, direction, edge.width))
                })
                .collect();
            if bearings.len() < 3 {
                continue;
            }
            bearings.sort_by(|a, b| b.2.total_cmp(&a.2));
            let axis = bearings[0].1;

            // An arm is on the main axis if it lies along it *either way* —
            // the two halves of one road through a crossing move together, or
            // the phase would stop a straight run at its own junction.
            let mut arms: HashMap<NodeId, u8> = HashMap::default();
            for (other, direction, _) in &bearings {
                let along = direction.dot(axis).abs();
                arms.insert(*other, u8::from(along < 0.5));
            }
            // A Y where every arm is within sixty degrees of the widest leaves
            // the second half of the cycle showing red to nobody. Whichever
            // arm is least like the main axis takes it.
            if arms.values().all(|group| *group == 0)
                && let Some((other, _, _)) = bearings
                    .iter()
                    .min_by(|a, b| a.1.dot(axis).abs().total_cmp(&b.1.dot(axis).abs()))
            {
                arms.insert(*other, 1);
            }

            at.insert(
                id,
                Signal {
                    arms,
                    // Any spread will do as long as it is the same every time
                    // the city is built. A prime-ish stride over the node id
                    // walks the whole cycle without landing two neighbours
                    // together as often as a smaller one would.
                    offset: (id.0 as f32 * 7.13) % CYCLE,
                },
            );
        }
        Self { at }
    }

    /// Whether this crossing has lights at all.
    pub fn signalled(&self, at: NodeId) -> bool {
        self.at.contains_key(&at)
    }

    pub fn count(&self) -> usize {
        self.at.len()
    }

    /// What the signal facing a driver arriving from `arm` is showing.
    ///
    /// `None` where there is no signal — which is most of the town, and is
    /// not the same answer as green: an unsignalled crossing is handed back
    /// to `ai::junction`'s priority rules rather than waved through.
    pub fn aspect(&self, at: NodeId, arm: NodeId, now: f32) -> Option<Aspect> {
        let signal = self.at.get(&at)?;
        let group = *signal.arms.get(&arm)?;
        let phase = (now + signal.offset).rem_euclid(CYCLE);
        let (turn, elapsed) = if phase < HALF {
            (0, phase)
        } else {
            (1, phase - HALF)
        };
        if turn != group {
            return Some(Aspect::Red);
        }
        Some(if elapsed < RED_AMBER {
            Aspect::RedAmber
        } else if elapsed < RED_AMBER + GREEN {
            Aspect::Green
        } else if elapsed < RED_AMBER + GREEN + AMBER {
            Aspect::Amber
        } else {
            Aspect::Red
        })
    }
}

/// The clock the phases run on, shared with `ai::junction`.
///
/// It has to be shared. If the traffic read the wall clock and the lamps read
/// a frozen one, a capture would show cars crossing on a red — which is the
/// one thing the comment this module replaced was written to avoid.
///
/// Real seconds, because a traffic light is the one thing in this city that
/// does not care what time of day it is — except under capture, where it has
/// to be the same on every run or two shots of one street are two different
/// pictures. There the junction's own offset is the whole clock: every
/// crossing sits at a fixed, different point in its cycle, which is a street
/// with lights on it rather than a street with one light on it.
pub fn phase_clock(time: &Time) -> f32 {
    if crate::core::capture::is_capture_mode() {
        0.0
    } else {
        time.elapsed_secs()
    }
}

/// One lamp on one signal head, and which approach it faces.
///
/// The arm rather than a direction, because that is what [`Signals`] is keyed
/// by and a mast streamed back in has to find its own phase again.
#[derive(Component, Debug, Clone, Copy)]
pub struct SignalLamp {
    pub at: NodeId,
    pub arm: NodeId,
    /// 0 red, 1 amber, 2 green — the order the head hangs them in.
    pub lamp: usize,
}

pub struct SignalPlugin;

impl Plugin for SignalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Signals>()
            // `PostStartup`, because `world::generate_city` inserts the layout
            // in `Startup` and this is a pure function of it. The same
            // silently load-bearing ordering `giveway::remember_runs` has.
            .add_systems(PostStartup, remember_signals)
            .add_systems(Update, show_aspect.in_set(GameSet::Ui));
    }
}

fn remember_signals(mut commands: Commands, city: Option<Res<City>>) {
    let Some(city) = city else {
        return;
    };
    let signals = Signals::of(&city.graph);
    debug!("signals: {} signalled junctions", signals.count());
    commands.insert_resource(signals);
}

/// Lights the lamp the phase says and darkens the other two.
///
/// A material swap rather than a material edit. Every lens of one colour in
/// the town shares one handle — lit and dark are two handles, not two hundred
/// — so the batching that draws several hundred signal heads in one call
/// survives, which a material per lamp would not. The same reasoning the
/// number plates are built on, and for once it is cheap: six materials for
/// the whole town.
fn show_aspect(
    time: Res<Time>,
    signals: Res<Signals>,
    assets: Option<Res<super::props::PropAssets>>,
    mut lamps: Query<(&SignalLamp, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(assets) = assets else {
        return;
    };
    let now = phase_clock(&time);
    for (lamp, mut material) in &mut lamps {
        let Some(aspect) = signals.aspect(lamp.at, lamp.arm, now) else {
            continue;
        };
        let wanted = if aspect.lit()[lamp.lamp] {
            assets.signal_lens_lit[lamp.lamp].clone()
        } else {
            assets.signal_lens[lamp.lamp].1.clone()
        };
        // Only when it actually changes. Writing through the `&mut` marks the
        // component changed whether or not the handle differs, and a lamp that
        // reports a change sixty times a second puts every signal head in the
        // town back through material extraction every frame.
        if material.0 != wanted {
            material.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::atlas::Surface;

    /// A crossroads with a main road east-west through it.
    fn signalled() -> RoadGraph {
        let mut graph = RoadGraph::default();
        let centre = graph.add_node(Vec2::ZERO, (1, 1));
        for (i, (pos, arterial)) in [
            (Vec2::new(-40.0, 0.0), true),
            (Vec2::new(0.0, 40.0), false),
            (Vec2::new(40.0, 0.0), true),
            (Vec2::new(0.0, -40.0), false),
        ]
        .into_iter()
        .enumerate()
        {
            let arm = graph.add_node(pos, (i as u16, 9));
            let width = if arterial { 12.0 } else { 7.5 };
            graph.connect(centre, arm, width, arterial, Surface::Asphalt);
        }
        graph
    }

    const CENTRE: NodeId = NodeId(0);
    const WEST: NodeId = NodeId(1);
    const SOUTH: NodeId = NodeId(2);
    const EAST: NodeId = NodeId(3);
    const NORTH: NodeId = NodeId(4);

    #[test]
    fn a_road_through_a_crossing_moves_as_one() {
        // Both halves of the main road share a phase. A rule that gave each
        // arm its own turn would stop a straight run at its own junction, and
        // it is the commonest thing a signalled crossing has to not do.
        let signals = Signals::of(&signalled());
        for now in [0.0, 5.0, 11.0, 19.0, 25.0, 33.0, 41.0] {
            assert_eq!(
                signals.aspect(CENTRE, WEST, now),
                signals.aspect(CENTRE, EAST, now),
                "the two ends of the main road disagree at {now}s"
            );
            assert_eq!(
                signals.aspect(CENTRE, NORTH, now),
                signals.aspect(CENTRE, SOUTH, now),
                "the two ends of the side road disagree at {now}s"
            );
        }
    }

    #[test]
    fn the_two_axes_are_never_green_together() {
        // The whole point. Sampled finely enough to catch an off-by-one at a
        // phase boundary, which is the only place this could go wrong.
        let signals = Signals::of(&signalled());
        let mut greens = 0;
        for step in 0..(CYCLE * 20.0) as u32 {
            let now = step as f32 * 0.05;
            let main = signals.aspect(CENTRE, WEST, now).unwrap();
            let side = signals.aspect(CENTRE, NORTH, now).unwrap();
            assert!(
                !(main.go() && side.go()),
                "both axes go at {now}s: {main:?} and {side:?}"
            );
            greens += u32::from(main.go());
        }
        assert!(greens > 0, "the main road is never green");
    }

    #[test]
    fn every_arm_gets_a_turn() {
        // A junction whose second group is empty shows red to nobody for half
        // of every cycle, and the arm that never goes green is a street the
        // traffic can never leave by.
        let signals = Signals::of(&signalled());
        for arm in [WEST, SOUTH, EAST, NORTH] {
            let go = (0..(CYCLE * 10.0) as u32).any(|step| {
                signals
                    .aspect(CENTRE, arm, step as f32 * 0.1)
                    .is_some_and(Aspect::go)
            });
            assert!(go, "the arm at {arm:?} is never given a green");
        }
    }

    #[test]
    fn a_back_street_crossing_has_no_lights() {
        // The masts are only built where a main road crosses, and this has to
        // give the same answer or the town grows a phase with no lamp.
        let mut graph = RoadGraph::default();
        let centre = graph.add_node(Vec2::ZERO, (1, 1));
        for (i, pos) in [
            Vec2::new(-40.0, 0.0),
            Vec2::new(0.0, 40.0),
            Vec2::new(40.0, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            let arm = graph.add_node(pos, (i as u16, 9));
            graph.connect(centre, arm, 7.5, false, Surface::Asphalt);
        }
        let signals = Signals::of(&graph);
        assert!(!signals.signalled(CENTRE));
        assert_eq!(signals.aspect(CENTRE, NodeId(1), 0.0), None);
    }

    #[test]
    fn neighbouring_junctions_do_not_blink_together() {
        // Not cosmetic. A town whose every light changes on the same tick
        // sends the whole of its traffic at the whole of its junctions at
        // once, and the queues that makes are an artefact of the offset
        // rather than of the traffic.
        let mut graph = signalled();
        // A second crossroads down the same main road, built from later node
        // ids — which is all the offset reads.
        let centre = graph.add_node(Vec2::new(200.0, 0.0), (5, 5));
        graph.connect(centre, EAST, 12.0, true, Surface::Asphalt);
        for (i, pos) in [
            Vec2::new(200.0, 40.0),
            Vec2::new(200.0, -40.0),
            Vec2::new(340.0, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            let arm = graph.add_node(pos, (i as u16 + 6, 5));
            let arterial = pos.y == 0.0;
            graph.connect(
                centre,
                arm,
                if arterial { 12.0 } else { 7.5 },
                arterial,
                Surface::Asphalt,
            );
        }

        let signals = Signals::of(&graph);
        assert_eq!(signals.count(), 2, "both crossings should be signalled");
        let differs = (0..(CYCLE * 10.0) as u32).any(|step| {
            let now = step as f32 * 0.1;
            signals.aspect(CENTRE, WEST, now) != signals.aspect(centre, EAST, now)
        });
        assert!(
            differs,
            "two crossings are showing the same phase all cycle"
        );
    }

    #[test]
    fn amber_does_not_admit_anybody() {
        // A car let in on amber is a car stranded in the middle when the other
        // axis goes green two seconds later.
        assert!(!Aspect::Amber.go());
        assert!(!Aspect::RedAmber.go());
        assert!(Aspect::Green.go());
        assert!(!Aspect::Red.go());
    }
}
