//! The road network as a graph.
//!
//! This is the single structure that traffic AI, police pursuit, roadblock
//! placement and the minimap all read from. It is deliberately plain data with
//! no ECS involvement so it can be unit-tested without spinning up an App.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use bevy::math::Vec2;
use bevy::platform::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeId(pub u32);

#[derive(Debug, Clone)]
pub struct RoadNode {
    pub pos: Vec2,
    /// Index into the street lists that produced this intersection.
    pub grid: (u16, u16),
    pub edges: Vec<EdgeId>,
}

#[derive(Debug, Clone)]
pub struct RoadEdge {
    pub a: NodeId,
    pub b: NodeId,
    pub width: f32,
    pub arterial: bool,
    /// What it is paved with. Only a town read off a map has anything but
    /// asphalt here; the generator lays tarmac everywhere by construction.
    pub surface: super::atlas::Surface,
    pub length: f32,
}

/// The movement a vehicle makes at a junction. Kept in the road module so
/// traffic, crossings and future signals agree on the same geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    Straight,
    Left,
    Right,
    UTurn,
}

/// One vehicle's way across a junction: in along `from -> at`, out along
/// `at -> to`.
///
/// Three nodes rather than two edges, because a U-turn uses one edge twice
/// and a pair of edges cannot say which way round it was driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Movement {
    pub from: NodeId,
    pub at: NodeId,
    pub to: NodeId,
}

/// Whether two movements through the same junction want the same tarmac.
///
/// This was a table over [`TurnKind`] alone, and it could not be right. A turn
/// kind is named relative to the car making it, so two cars arriving at a
/// crossroads from *perpendicular* arms and carrying straight on are both
/// `Straight` — and they meet in the middle. The old rule said they could
/// share it, which is the single commonest conflict at any crossing in the
/// town. Its own comment knew the shape of the hole — "two *same-direction*
/// straight movements can share it" — and had no way to say so, because
/// nothing in a pair of turn kinds names an arm.
///
/// So the rule now reads the geometry it was always waiting for. Each
/// movement is the straight line from where it enters the junction, in its
/// own travel lane, to where it leaves, in its own; two of them conflict when
/// those lines cross. That is not an approximation of the priority rules, it
/// *is* them — an oncoming straight crosses a left turn and not a right one,
/// which is why left turns wait and right ones do not — and it falls out of
/// the lane offsets rather than being written down and kept in step by hand.
///
/// Two cases the crossing test cannot see on its own:
///
/// * **Same approach arm.** Two cars in one queue share an entry point and so
///   read as crossing. They are following each other, the obstacle ray in
///   `traffic::drive_traffic` owns that, and reserving it here would stop
///   every queue at every junction for good.
/// * **Same exit arm.** Their lines *meet* at the far end rather than crossing
///   anywhere, and two coincident endpoints are not something an `f32` can be
///   relied on to report as an intersection. A merge is a conflict.
pub fn movements_conflict(graph: &RoadGraph, a: Movement, b: Movement) -> bool {
    debug_assert_eq!(a.at, b.at, "two movements at different junctions");
    if a.from == b.from {
        return false;
    }
    if a.to == b.to {
        return true;
    }
    let (a0, a1) = graph.movement_chord(a);
    let (b0, b1) = graph.movement_chord(b);
    segments_cross(a0, a1, b0, b1)
}

/// Do two closed segments share a point?
///
/// The orientation test, with every degenerate case resolved towards *yes*.
/// A junction that is occasionally too cautious costs somebody a second of
/// their afternoon; one that is occasionally not is two cars in the middle of
/// a crossroads, which is the failure this whole module exists to stop.
fn segments_cross(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> bool {
    // In metres squared of cross product. The endpoints are lane offsets on a
    // town whose coordinates run to 1700 m, where the spacing between
    // representable floats is 200 um — see the layer table in CLAUDE.md for
    // the other place that number decides something — so exact zero is not an
    // answer this arithmetic ever gives.
    const FLAT: f32 = 1e-4;
    let side = |p: Vec2, q: Vec2, r: Vec2| {
        let d = (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
        if d > FLAT {
            1
        } else if d < -FLAT {
            -1
        } else {
            0
        }
    };
    let (d1, d2) = (side(a0, a1, b0), side(a0, a1, b1));
    let (d3, d4) = (side(b0, b1, a0), side(b0, b1, a1));
    if d1 == 0 || d2 == 0 || d3 == 0 || d4 == 0 {
        // Collinear, or one endpoint lying on the other line. Touching only
        // if the two actually reach each other: two movements down the same
        // infinite line but a hundred metres apart share nothing.
        return a0.min(a1).cmple(b0.max(b1)).all() && b0.min(b1).cmple(a0.max(a1)).all();
    }
    d1 != d2 && d3 != d4
}

#[derive(Debug, Clone, Default)]
pub struct RoadGraph {
    nodes: Vec<RoadNode>,
    edges: Vec<RoadEdge>,
    by_grid: HashMap<(u16, u16), NodeId>,
}

impl RoadGraph {
    pub fn add_node(&mut self, pos: Vec2, grid: (u16, u16)) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(RoadNode {
            pos,
            grid,
            edges: Vec::new(),
        });
        self.by_grid.insert(grid, id);
        id
    }

    pub fn connect(
        &mut self,
        a: NodeId,
        b: NodeId,
        width: f32,
        arterial: bool,
        surface: super::atlas::Surface,
    ) -> EdgeId {
        let length = self.node(a).pos.distance(self.node(b).pos);
        let id = EdgeId(self.edges.len() as u32);
        self.edges.push(RoadEdge {
            a,
            b,
            width,
            arterial,
            surface,
            length,
        });
        self.nodes[a.0 as usize].edges.push(id);
        self.nodes[b.0 as usize].edges.push(id);
        id
    }

    pub fn node(&self, id: NodeId) -> &RoadNode {
        &self.nodes[id.0 as usize]
    }

    pub fn edge(&self, id: EdgeId) -> &RoadEdge {
        &self.edges[id.0 as usize]
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &RoadNode)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (NodeId(i as u32), n))
    }

    pub fn edges(&self) -> impl Iterator<Item = &RoadEdge> {
        self.edges.iter()
    }

    pub fn node_at_grid(&self, grid: (u16, u16)) -> Option<NodeId> {
        self.by_grid.get(&grid).copied()
    }

    /// The other end of `edge` when arriving from `from`.
    pub fn other_end(&self, edge: EdgeId, from: NodeId) -> NodeId {
        let e = self.edge(edge);
        if e.a == from { e.b } else { e.a }
    }

    pub fn neighbors(&self, id: NodeId) -> impl Iterator<Item = (NodeId, EdgeId)> + '_ {
        self.node(id)
            .edges
            .iter()
            .map(move |&e| (self.other_end(e, id), e))
    }

    pub fn nearest_node(&self, pos: Vec2) -> Option<NodeId> {
        self.nodes
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.pos
                    .distance_squared(pos)
                    .total_cmp(&b.pos.distance_squared(pos))
            })
            .map(|(i, _)| NodeId(i as u32))
    }

    /// A* over edge length. Arterials are discounted so routes prefer main
    /// roads, which is both what real drivers do and what makes police
    /// pursuit read as purposeful rather than drunken.
    pub fn path(&self, start: NodeId, goal: NodeId) -> Option<Vec<NodeId>> {
        if start == goal {
            return Some(vec![start]);
        }

        let goal_pos = self.node(goal).pos;
        let mut open = BinaryHeap::new();
        let mut came_from: HashMap<NodeId, NodeId> = HashMap::default();
        let mut best: HashMap<NodeId, f32> = HashMap::default();
        let mut closed: HashSet<NodeId> = HashSet::default();

        best.insert(start, 0.0);
        open.push(Candidate {
            estimate: self.node(start).pos.distance(goal_pos),
            node: start,
        });

        while let Some(Candidate { node, .. }) = open.pop() {
            if node == goal {
                return Some(reconstruct(&came_from, goal));
            }
            if !closed.insert(node) {
                continue;
            }

            let cost_here = best.get(&node).copied().unwrap_or(f32::INFINITY);
            for (next, edge_id) in self.neighbors(node) {
                let edge = self.edge(edge_id);
                let step = edge.length * if edge.arterial { 0.8 } else { 1.0 };
                let tentative = cost_here + step;
                if tentative < best.get(&next).copied().unwrap_or(f32::INFINITY) {
                    best.insert(next, tentative);
                    came_from.insert(next, node);
                    open.push(Candidate {
                        // Heuristic uses the discounted rate so it stays
                        // admissible and A* keeps returning optimal paths.
                        estimate: tentative + self.node(next).pos.distance(goal_pos) * 0.8,
                        node: next,
                    });
                }
            }
        }

        None
    }

    /// Which way a vehicle arriving from `from` turns at `at` to leave by `to`.
    ///
    /// A positive 2D cross product is a turn to the driver's **right**, not
    /// their left. These `Vec2`s are `(x, z)` in the world — every caller gets
    /// them from `Transform::translation.xz()` — and the world's right-hand
    /// side is `steering::right_of(d) = (-d.z, d.x)`, which is the side
    /// `RIGHT_HAND_TRAFFIC` keeps its lane on and the side the patrol walks
    /// its pavement on. Heading east along `(1, 0)` and leaving along `(0, 1)`
    /// is `right_of((1, 0))` exactly, and it is a right turn.
    pub fn turn_kind(&self, from: NodeId, at: NodeId, to: NodeId) -> TurnKind {
        let incoming = (self.node(at).pos - self.node(from).pos).normalize_or_zero();
        let outgoing = (self.node(to).pos - self.node(at).pos).normalize_or_zero();
        let dot = incoming.dot(outgoing);
        if dot < -0.65 {
            return TurnKind::UTurn;
        }
        let cross = incoming.x * outgoing.y - incoming.y * outgoing.x;
        if cross.abs() < 0.22 {
            TurnKind::Straight
        } else if cross > 0.0 {
            TurnKind::Right
        } else {
            TurnKind::Left
        }
    }

    /// The edge joining two nodes, if they are joined at all.
    pub fn edge_between(&self, from: NodeId, to: NodeId) -> Option<EdgeId> {
        self.neighbors(from)
            .find(|(node, _)| *node == to)
            .map(|(_, edge)| edge)
    }

    /// How wide the road between two nodes is.
    pub fn width_between(&self, from: NodeId, to: NodeId) -> Option<f32> {
        self.edge_between(from, to)
            .map(|edge| self.edge(edge).width)
    }

    /// Whether anything actually crosses at this node.
    ///
    /// A town read off a map is mostly *bends*. The bake keeps a node at every
    /// vertex of every polyline, so a curved street carries one every few
    /// metres, and two edges meeting at one is a corner rather than a
    /// crossing: 1191 of Landshut's 1565 nodes are that. Holding cars at them
    /// would stop the traffic at a thousand places where there is nothing to
    /// give way to, and it is the distinction `LIVING_CITY.md` asks for under
    /// "polyline bend nodes".
    pub fn is_junction(&self, at: NodeId) -> bool {
        self.node(at).edges.len() >= 3
    }

    /// How far out from a node the junction reaches.
    ///
    /// Half the widest arm is the crossing's own radius — where the kerb line
    /// of the side street meets the through one. Floored, because a junction
    /// of three Gassen is still several metres across and a chord shorter than
    /// a car says nothing; capped, because the market band runs to 22 m and a
    /// chord that long would have movements conflicting a lane's width outside
    /// the junction they are in.
    pub fn junction_reach(&self, at: NodeId) -> f32 {
        let widest = self
            .node(at)
            .edges
            .iter()
            .map(|&e| self.edge(e).width)
            .fold(0.0, f32::max);
        (widest * 0.5).clamp(3.0, 11.0)
    }

    /// The straight line a vehicle draws across a junction making `movement`,
    /// from its own travel lane on the way in to its own on the way out.
    ///
    /// A lane connector without the arc. The arc is what the car actually
    /// drives and it is the wrong thing to test against: two arcs that miss
    /// each other by a metre still have to be driven one at a time, because
    /// the arcs are lines and the cars on them are two metres wide. The chord
    /// is the honest summary of "this movement has the middle of the junction".
    ///
    /// Both ends come from [`steering::lane_point`](crate::ai::steering::lane_point),
    /// which is what `traffic::drive_traffic` steers down, so the chord starts
    /// where the car is rather than in the middle of the road — and the side
    /// it picks cannot drift out of step with the side the cars use, which is
    /// the one error here that a screenshot would never show.
    pub fn movement_chord(&self, movement: Movement) -> (Vec2, Vec2) {
        use crate::ai::steering::lane_point;

        let at = self.node(movement.at).pos;
        let from = self.node(movement.from).pos;
        let to = self.node(movement.to).pos;
        let reach = self.junction_reach(movement.at);
        // A leg shorter than the junction is its own radius clamps to its far
        // end, which is the right answer: the chord then spans the whole of a
        // stub too short to hold a stop line anyway.
        let (into, out_of) = (from.distance(at).max(0.01), at.distance(to).max(0.01));
        let in_width = self
            .width_between(movement.from, movement.at)
            .unwrap_or(4.0);
        let out_width = self.width_between(movement.at, movement.to).unwrap_or(4.0);
        (
            lane_point(from, at, in_width, ((into - reach) / into).clamp(0.0, 1.0)),
            lane_point(at, to, out_width, (reach / out_of).clamp(0.0, 1.0)),
        )
    }
}

fn reconstruct(came_from: &HashMap<NodeId, NodeId>, goal: NodeId) -> Vec<NodeId> {
    let mut path = vec![goal];
    let mut current = goal;
    while let Some(&prev) = came_from.get(&current) {
        path.push(prev);
        current = prev;
    }
    path.reverse();
    path
}

/// Min-heap entry: `BinaryHeap` is a max-heap, so ordering is reversed.
struct Candidate {
    estimate: f32,
    node: NodeId,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.estimate == other.estimate && self.node == other.node
    }
}
impl Eq for Candidate {}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimate
            .total_cmp(&self.estimate)
            .then_with(|| other.node.cmp(&self.node))
    }
}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::atlas::Surface;

    fn cross() -> RoadGraph {
        let mut graph = RoadGraph::default();
        graph.add_node(Vec2::new(-10.0, 0.0), (0, 0));
        graph.add_node(Vec2::ZERO, (1, 0));
        graph.add_node(Vec2::new(0.0, 10.0), (1, 1));
        graph.add_node(Vec2::new(10.0, 0.0), (2, 0));
        graph
    }

    #[test]
    fn classifies_left_right_and_straight_movements() {
        let graph = cross();
        // Arriving from the west and leaving south. `(0, 10)` is +z, which is
        // `right_of` due east, so this is a right turn — the same answer the
        // lane offset and the pavement walk give for that side.
        assert_eq!(
            graph.turn_kind(NodeId(0), NodeId(1), NodeId(2)),
            TurnKind::Right
        );
        assert_eq!(
            graph.turn_kind(NodeId(0), NodeId(1), NodeId(3)),
            TurnKind::Straight
        );
        assert_eq!(
            graph.turn_kind(NodeId(3), NodeId(1), NodeId(2)),
            TurnKind::Left
        );
    }

    #[test]
    fn a_turn_is_named_for_the_side_the_lane_offset_uses() {
        // The one check that ties the classification to the rest of the game.
        // Whichever way `steering::right_of` points is what `Right` has to
        // mean here, or every give-way rule built on it reads the junction
        // mirrored — and the two live in different modules, so nothing else
        // would ever notice them disagreeing.
        let graph = cross();
        let at = graph.node(NodeId(1)).pos;
        let incoming = (at - graph.node(NodeId(0)).pos).normalize();
        let exit = (graph.node(NodeId(2)).pos - at).normalize();
        assert!(crate::ai::steering::right_of(incoming).dot(exit) > 0.9);
        assert_eq!(
            graph.turn_kind(NodeId(0), NodeId(1), NodeId(2)),
            TurnKind::Right
        );
    }

    #[test]
    fn classifies_a_dead_end_turnaround() {
        let graph = cross();
        assert_eq!(
            graph.turn_kind(NodeId(0), NodeId(1), NodeId(0)),
            TurnKind::UTurn
        );
    }

    /// A four-armed crossing of median Landshut streets, with the arms named
    /// for where they lie: west, south, east, north around a centre at the
    /// origin. `+y` here is `+z` in the world, which is the side `right_of`
    /// points to for a car heading east — see `turn_kind`.
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
    fn two_straights_on_crossing_arms_conflict() {
        // The case the old turn-kind table got wrong, and the commonest
        // conflict at any crossing in the town: both cars are `Straight` and
        // they are ninety degrees apart.
        let graph = crossroads();
        assert_eq!(
            graph.turn_kind(WEST, CENTRE, EAST),
            TurnKind::Straight,
            "the east-west movement is straight on"
        );
        assert_eq!(
            graph.turn_kind(NORTH, CENTRE, SOUTH),
            TurnKind::Straight,
            "and so is the north-south one"
        );
        assert!(movements_conflict(
            &graph,
            movement(WEST, EAST),
            movement(NORTH, SOUTH)
        ));
    }

    #[test]
    fn two_straights_facing_each_other_pass() {
        // Each keeps to its own side, so the chords are parallel and a metre
        // and a half apart. This is the case the old rule was written for and
        // the only one it got right.
        let graph = crossroads();
        assert!(!movements_conflict(
            &graph,
            movement(WEST, EAST),
            movement(EAST, WEST)
        ));
    }

    #[test]
    fn a_left_turn_waits_for_the_oncoming_straight() {
        // Nobody wrote this rule down. It is the lane offsets: turning across
        // the oncoming lane means crossing the line the oncoming car is on.
        let graph = crossroads();
        assert_eq!(graph.turn_kind(WEST, CENTRE, NORTH), TurnKind::Left);
        assert!(movements_conflict(
            &graph,
            movement(WEST, NORTH),
            movement(EAST, WEST)
        ));
    }

    #[test]
    fn a_right_turn_does_not() {
        // The other half of the same arithmetic, and the half that makes the
        // rule worth having: a right turn hugs the nearside kerb and never
        // reaches the oncoming lane, so it should not be made to wait.
        let graph = crossroads();
        assert_eq!(graph.turn_kind(WEST, CENTRE, SOUTH), TurnKind::Right);
        assert!(!movements_conflict(
            &graph,
            movement(WEST, SOUTH),
            movement(EAST, WEST)
        ));
    }

    #[test]
    fn one_queue_on_one_arm_is_not_a_junction_conflict() {
        // Two cars nose to tail share an entry point, which reads as a
        // crossing to any geometric test. They are following each other.
        let graph = crossroads();
        assert!(!movements_conflict(
            &graph,
            movement(WEST, EAST),
            movement(WEST, SOUTH)
        ));
    }

    #[test]
    fn two_movements_merging_into_one_arm_conflict() {
        // Their chords meet at the exit rather than crossing before it, which
        // is not something coincident f32 endpoints can be relied on to say.
        let graph = crossroads();
        assert!(movements_conflict(
            &graph,
            movement(WEST, SOUTH),
            movement(EAST, SOUTH)
        ));
    }

    #[test]
    fn a_bend_in_a_street_is_not_a_junction() {
        // 1191 of Landshut's 1565 nodes are this: a vertex in a polyline, not
        // a crossing. Holding cars at them would stop the town dead.
        let graph = crossroads();
        assert!(graph.is_junction(CENTRE), "four arms is a junction");
        assert!(!graph.is_junction(WEST), "one arm is the end of a street");

        let mut bend = RoadGraph::default();
        let corner = bend.add_node(Vec2::ZERO, (0, 0));
        let a = bend.add_node(Vec2::new(-20.0, 0.0), (1, 0));
        let b = bend.add_node(Vec2::new(0.0, 20.0), (2, 0));
        bend.connect(corner, a, 6.0, false, Surface::Asphalt);
        bend.connect(corner, b, 6.0, false, Surface::Asphalt);
        assert!(!bend.is_junction(corner), "two arms is a corner");
    }

    #[test]
    fn a_movement_starts_and_ends_in_its_own_lane() {
        // The one error here a screenshot would never show. If the chord were
        // built on the wrong side of the centreline every conflict above would
        // still pass — mirrored — so this ties it to the side the cars use.
        let graph = crossroads();
        let (entry, exit) = graph.movement_chord(movement(WEST, EAST));
        // Heading east, the travel lane is at +y.
        assert!(entry.y > 0.0, "entry {entry:?} is in the oncoming lane");
        assert!(exit.y > 0.0, "exit {exit:?} is in the oncoming lane");
        assert!(entry.x < 0.0 && exit.x > 0.0, "the chord crosses the node");
        let lane = crate::ai::steering::lane_point(
            Vec2::new(-40.0, 0.0),
            Vec2::ZERO,
            7.5,
            (40.0 - 3.75) / 40.0,
        );
        assert!(
            entry.distance(lane) < 1e-3,
            "entry {entry:?} is not where the car steers, {lane:?}"
        );
    }
}
