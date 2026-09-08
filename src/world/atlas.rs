//! A real town's street plan, read off a baked OpenStreetMap extract.
//!
//! Everything else in `world` builds a city out of a seed. This builds one out
//! of a *file*, and it is the only thing in the game that does.
//!
//! ## What this reverses, and why
//!
//! `CityStyle` was written down as "a postcard, not a map", on the honest
//! grounds that a street plan matching the real Landshut needs curved blocks
//! the pipeline could not hold. Half of that is still true — the blocks are
//! still the thing that cannot be done — and the other half turned out to be
//! wrong: the *road graph* has been able to hold a real street plan all along.
//! It is a list of nodes with positions and edges with widths, its one
//! grid-shaped field is read by nothing outside the generator that fills it,
//! and the ground is already one asphalt plane with the streets carved out of
//! it rather than a mesh per road. So the graph took Landshut unchanged.
//!
//! What the blocks could not do, `frontage_lots` sidesteps rather than solves:
//! see there.
//!
//! ## Determinism
//!
//! The layout is now a pure function of `(seed, style, atlas)` rather than of
//! `(seed, style)`, and the atlas is a file that ships with the game. Chunks
//! still respawn identically, which is what the rule was protecting. What has
//! genuinely changed is that a city can now differ between *versions* — rerun
//! `tools/fetch-city.sh` a year from now and Landshut will have a new bypass —
//! which is why the baked file is committed rather than fetched at startup.
//!
//! ## Licence
//!
//! The data is © OpenStreetMap contributors, ODbL 1.0. It is the only asset in
//! this repository that is not CC0, and the attribution is in the baked file,
//! in `CREDITS.md`, and in the log line this module prints on load.

use bevy::math::Vec2;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use serde::Deserialize;

use super::citygen::CityLayout;
use super::roadgraph::RoadGraph;

/// One town, as baked by `tools/bake-city.py`.
#[derive(Debug, Clone, Deserialize)]
pub struct Atlas {
    pub name: String,
    /// The latitude and longitude the metres are measured from. Carried so a
    /// future feature — a compass, a real sun angle — has it, and so the file
    /// says where it is of.
    pub centre: (f64, f64),
    pub streets: Vec<Street>,
}

/// One way out of the extract: a polyline in metres, with a carriageway width.
#[derive(Debug, Clone, Deserialize)]
pub struct Street {
    pub name: String,
    pub width: f32,
    pub arterial: bool,
    pub points: Vec<(f32, f32)>,
}

/// How close two points have to be to be the same junction, in metres.
///
/// Small: the baker emits a decimetre of precision and a shared OSM node comes
/// out of the projection bit-for-bit identical in both ways that use it, so
/// this only has to survive the rounding. Anything larger starts welding
/// genuinely separate kerbs on a dual carriageway into one line.
const WELD: f32 = 0.35;

/// Shortest edge worth keeping, in metres.
///
/// Under this the two ends weld to the same node anyway, and what is left is a
/// self-loop that every downstream consumer — traffic, the minimap, the
/// furniture spawners — has to be careful about.
const SHORTEST: f32 = 1.2;

/// Reads a town off disk.
///
/// Returns `None` rather than panicking on anything: a missing or unreadable
/// atlas is a city that falls back to the generator with a warning, which is
/// the same discipline `audio::files` applies to a missing recording. A
/// half-built town is worse than a generated one.
pub fn load(name: &str) -> Option<Atlas> {
    let path = crate::core::assets::root()
        .join("cities")
        .join(format!("{name}.ron"));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            warn!("no atlas at {}: {error}", path.display());
            return None;
        }
    };
    match ron::from_str::<Atlas>(&text) {
        Ok(atlas) => Some(atlas),
        Err(error) => {
            warn!("{} is not an atlas: {error}", path.display());
            None
        }
    }
}

/// Builds a layout out of a town.
///
/// The blocks come out empty and that is not an oversight — see
/// `frontage_lots`, which is what fills a real city instead.
pub fn layout(atlas: &Atlas, seed: u64, half_extent: f32) -> CityLayout {
    let mut graph = RoadGraph::default();
    // Junction welding: two ways that share an OSM node project to the same
    // metre, so quantising to a decimetre and looking up is enough to turn five
    // hundred loose polylines into one connected network.
    let mut welded: HashMap<(i32, i32), super::roadgraph::NodeId> = HashMap::default();
    let key = |at: Vec2| ((at.x / WELD).round() as i32, (at.y / WELD).round() as i32);

    let mut clipped = 0usize;
    for street in &atlas.streets {
        let mut previous: Option<(Vec2, super::roadgraph::NodeId)> = None;
        for &(x, z) in &street.points {
            let at = Vec2::new(x, z);
            // Outside the square the game builds, the street simply stops. A
            // way that leaves and comes back — a ring road clipping a corner —
            // is left as two runs rather than joined across the outside, which
            // is what `previous = None` does here.
            if at.x.abs() > half_extent || at.y.abs() > half_extent {
                if previous.is_some() {
                    clipped += 1;
                }
                previous = None;
                continue;
            }

            let node = *welded.entry(key(at)).or_insert_with(|| {
                // The grid key is what the generator uses to find the junction
                // at a crossing of two of its own street lists. A real town has
                // no such lists, so every node here gets its own key and
                // `node_at_grid` simply never finds anything — which is fine,
                // because only the generator ever asks.
                let index = graph.node_count() as u32;
                graph.add_node(at, ((index >> 16) as u16, index as u16))
            });

            if let Some((before, from)) = previous
                && from != node
                && before.distance(at) >= SHORTEST
            {
                graph.connect(from, node, street.width, street.arterial);
            }
            previous = Some((at, node));
        }
    }

    info!(
        "{}: {} streets, {} junctions, {} roads ({clipped} runs clipped at the edge) \
         — map data (c) OpenStreetMap contributors, ODbL 1.0",
        atlas.name,
        atlas.streets.len(),
        graph.node_count(),
        graph.edge_count(),
    );

    CityLayout {
        seed,
        half_extent,
        // A real town has no street lists: those are the generator's two axes
        // of its own grid, and nothing outside it reads them.
        x_streets: Vec::new(),
        z_streets: Vec::new(),
        blocks: Vec::new(),
        graph,
        // The Isar is a river and this is a canal dug through a grid. Landshut
        // deserves better than the wrong water in the wrong place, so until
        // there is a real one there is none.
        canal: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn town(streets: Vec<Street>) -> Atlas {
        Atlas {
            name: "Test".into(),
            centre: (0.0, 0.0),
            streets,
        }
    }

    fn street(points: &[(f32, f32)]) -> Street {
        Street {
            name: "Teststraße".into(),
            width: 8.0,
            arterial: false,
            points: points.to_vec(),
        }
    }

    /// Two ways that meet share a junction rather than passing through one
    /// another.
    #[test]
    fn streets_that_meet_are_welded() {
        // A cross: one way north to south, one east to west, sharing the
        // middle point exactly the way two OSM ways share a node.
        let atlas = town(vec![
            street(&[(0.0, -50.0), (0.0, 0.0), (0.0, 50.0)]),
            street(&[(-50.0, 0.0), (0.0, 0.0), (50.0, 0.0)]),
        ]);
        let layout = layout(&atlas, 1, 1000.0);
        assert_eq!(layout.graph.node_count(), 5, "the middle was not shared");
        assert_eq!(layout.graph.edge_count(), 4);

        // And the junction really has four arms, which is the thing traffic
        // and the signals both read.
        let middle = layout
            .graph
            .nearest_node(Vec2::ZERO)
            .expect("a node at the crossing");
        assert_eq!(layout.graph.node(middle).edges.len(), 4);
    }

    /// A street that leaves the square is cut rather than stretched across it.
    #[test]
    fn a_street_stops_at_the_edge_of_the_world() {
        let atlas = town(vec![street(&[
            (-100.0, 0.0),
            (0.0, 0.0),
            (900.0, 0.0),
            (2_000.0, 0.0),
            (2_100.0, 0.0),
        ])]);
        let layout = layout(&atlas, 1, 1000.0);
        // The three points inside stay and are joined; the two outside are
        // dropped, and no edge reaches out to them.
        assert_eq!(layout.graph.node_count(), 3);
        assert_eq!(layout.graph.edge_count(), 2);
        for edge in layout.graph.edges() {
            for end in [edge.a, edge.b] {
                let at = layout.graph.node(end).pos;
                assert!(at.x.abs() <= 1000.0 && at.y.abs() <= 1000.0);
            }
        }
    }

    /// A way that leaves the square and comes back is two runs, not one.
    #[test]
    fn a_street_that_leaves_and_returns_is_not_bridged() {
        // The failure this stops is a ring road clipping a corner: joined
        // across the gap it lays a kilometre of asphalt straight through the
        // middle of the town.
        let atlas = town(vec![street(&[
            (-500.0, 0.0),
            (-400.0, 0.0),
            (-5_000.0, 0.0),
            (400.0, 0.0),
            (500.0, 0.0),
        ])]);
        let layout = layout(&atlas, 1, 1000.0);
        assert_eq!(layout.graph.edge_count(), 2, "the gap was bridged");
        for edge in layout.graph.edges() {
            assert!(edge.length < 200.0, "an edge crossed the whole town");
        }
    }

    /// Points too close together do not become an edge.
    #[test]
    fn a_kerb_wobble_is_not_a_street() {
        let atlas = town(vec![street(&[(0.0, 0.0), (0.05, 0.0), (60.0, 0.0)])]);
        let layout = layout(&atlas, 1, 1000.0);
        // The wobble welds into the first node, so there are two nodes and one
        // road rather than three and two.
        assert_eq!(layout.graph.node_count(), 2);
        assert_eq!(layout.graph.edge_count(), 1);
    }

    /// A real town has no grid, and asking for one finds nothing.
    #[test]
    fn an_atlas_city_has_no_street_lists() {
        let atlas = town(vec![street(&[(0.0, 0.0), (40.0, 0.0)])]);
        let layout = layout(&atlas, 7, 1000.0);
        assert!(layout.x_streets.is_empty() && layout.z_streets.is_empty());
        assert!(layout.canal.is_none());
        assert_eq!(layout.seed, 7);
    }
}
