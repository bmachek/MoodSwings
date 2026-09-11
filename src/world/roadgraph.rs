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

/// Conservative conflict rule for two movements sharing a junction. It is
/// intentionally geometry-free until lane connectors exist: a straight or
/// turning movement reserves the crossing, while two same-direction straight
/// movements can share it. This gives signals and pedestrian gates one common
/// policy to build on.
pub fn movements_conflict(a: TurnKind, b: TurnKind) -> bool {
    !(a == TurnKind::Straight && b == TurnKind::Straight)
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

    #[test]
    fn conflict_rule_allows_only_parallel_straight_flow() {
        assert!(!movements_conflict(TurnKind::Straight, TurnKind::Straight));
        assert!(movements_conflict(TurnKind::Straight, TurnKind::Left));
        assert!(movements_conflict(TurnKind::Right, TurnKind::Right));
        assert!(movements_conflict(TurnKind::UTurn, TurnKind::Straight));
    }
}
