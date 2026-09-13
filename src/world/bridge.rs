//! Where a street crosses a river, and what stands there.
//!
//! `world::river` says it plainly: *a bridge is a road that kept its asphalt*.
//! The water is laid at [`super::layer::WATER`], one millimetre off the grass
//! and thirteen under the lowest carriageway, so every street that crosses it
//! is already drawn over it and nothing has to find the crossing, cut the water
//! or ramp anything up. That is a very good trick and it is why this town has
//! rivers at all.
//!
//! What it is not is a bridge. Six streets cross the Isar and its two arms
//! inside the square this game builds, and every one of them was tarmac laid
//! flat on the water: no parapet, no edge, nothing standing in the river, and
//! from the bank no way to tell a bridge from a ford.
//!
//! ## Why nothing here hangs below the deck
//!
//! Because the deck cannot be raised. `Terrain::height` returns exactly zero
//! wherever anything is built — see `world::terrain` for why thirty spawners
//! depend on that — so a bridge here is flush with the road either side of it,
//! and there is no soffit to show and nowhere to put an arch. Anything hung
//! under the deck would be hung *in* the water, thirteen millimetres down.
//!
//! So this builds upward and into the river, which is what you can see from a
//! bank anyway:
//!
//! * **The parapet.** The one thing that says bridge at any distance. Stone,
//!   waist high, along the outer edge of each pavement, running a few metres
//!   onto each bank so it meets the river's own wall rather than stopping in
//!   mid-air. Solid, because bouncing off a bridge railing instead of going in
//!   is the better half of the joke — `world::river` settled that for the canal
//!   and this follows it.
//! * **The lamps.** A row of standards along the parapet. In silhouette, at the
//!   distance a bridge is usually seen from, the lamps are half of what reads.
//! * **The pier heads.** Stone noses breaking the surface either side of the
//!   deck, spaced along the span. The piers themselves are under a deck that is
//!   level with the water, so what is drawn is the part of them that would show
//!   — and a low bridge whose arches are at the water line is a real kind of
//!   bridge, which is the honest way to draw one that cannot have a soffit.
//!
//! ## And two things this had to fix to stand up at all
//!
//! Both in `world::river`, both invisible until something was built here.
//!
//! The **bank wall ran straight across every deck**: `spawn_waters` walls both
//! sides of every river segment and never asked whether a street was on top of
//! it, so each of the six bridges had two nine-hundred-millimetre stone walls
//! lying across its carriageway. They carry no collider, so the traffic drove
//! through them, which is why nothing ever complained.
//!
//! The **river's trampoline stood above the road**. `BOUNCE_LEVEL` was written
//! for the canal, whose water is at sixty millimetres; the atlas rivers lay
//! theirs at one. The same constant put the collider's top at twenty
//! millimetres — above the ground collider, above the drawn carriageway, and
//! carrying restitution 0.95 with a Max combine rule. Every crossing of the
//! Isar was a launch ramp.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::citygen::{CityLayout, SIDEWALK_WIDTH};

/// The narrowest water that gets a bridge rather than a culvert.
///
/// The same eight metres `world::river` uses to decide which watercourses have
/// banks rather than sides. A mill race under a street is in a pipe and the
/// street over it is a street; the Isar is not.
const NAVIGABLE: f32 = 8.0;

/// How far the parapet runs onto each bank past the water's own edge.
///
/// Three metres. A parapet that stops exactly at the water line is a wall
/// floating over a river; this one lands, and meets the bank wall the river
/// already stands along.
const ABUTMENT: f32 = 3.0;

/// The parapet: how high it stands over the deck, and how thick it reads.
const PARAPET_HEIGHT: f32 = 1.05;
const PARAPET_THICK: f32 = 0.32;

/// The lamp standards: how tall over the parapet, how thin, and how far apart
/// along it. Twelve metres is close enough that a bridge carries three or four
/// and far enough that they do not read as a fence.
const LAMP_HEIGHT: f32 = 2.9;
const LAMP_THICK: f32 = 0.13;
const LAMP_SPACING: f32 = 12.0;
/// The lantern on top: a small box, in the one warm material here.
const LANTERN: Vec3 = Vec3::new(0.3, 0.36, 0.3);

/// The pier heads: how far apart along the span, how far they stand proud of
/// the water, and how big the visible nose is.
///
/// Twenty-two metres is a masonry span. The Isar is fifty-two across, so it
/// gets two piers and the forty-two metre arms get one, which is what the
/// photographs show.
const PIER_SPACING: f32 = 22.0;
const PIER_RISE: f32 = 0.55;
const PIER_ALONG: f32 = 2.4;
const PIER_PROUD: f32 = 1.7;

// A parapet has to stand on the pavement rather than in the carriageway, and a
// lamp has to stand on the parapet rather than beside it.
const _: () = assert!(PARAPET_THICK < SIDEWALK_WIDTH);
const _: () = assert!(LAMP_THICK < PARAPET_THICK);

/// One street crossing one river.
#[derive(Clone, Copy, Debug)]
pub struct Crossing {
    /// Where the street's centreline meets the water's.
    pub at: Vec2,
    /// Which way the street runs there, as a unit vector.
    pub along: Vec2,
    /// The street's full width, kerb to kerb.
    pub deck: f32,
    /// How far the water reaches, measured *along the street* — so a river
    /// taken at forty-five degrees is half again as long to cross as it is
    /// wide, which is what the deck has to span.
    pub span: f32,
}

impl Crossing {
    /// How far the parapet stands from the centreline: the far side of the
    /// pavement.
    fn kerb(&self) -> f32 {
        self.deck * 0.5 + SIDEWALK_WIDTH - PARAPET_THICK * 0.5
    }

    /// Is this point on the deck — between the parapets and between the
    /// abutments?
    ///
    /// The street's own frame rather than a radius, because a bridge is long
    /// and narrow and a circle round its middle either misses its ends or
    /// swallows the bank.
    pub fn carries(&self, point: Vec2) -> bool {
        let across = Vec2::new(-self.along.y, self.along.x);
        let offset = point - self.at;
        offset.dot(self.along).abs() <= self.span * 0.5 + ABUTMENT
            && offset.dot(across).abs() <= self.kerb() + PARAPET_THICK
    }
}

/// Is anything at this point standing on a bridge deck?
///
/// Asked by `world::river`, which must not wall a carriageway or float a
/// trampoline over one.
pub fn on_a_deck(crossings: &[Crossing], point: Vec2) -> bool {
    crossings.iter().any(|bridge| bridge.carries(point))
}

/// Where two segments cross, if they do.
fn meeting(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<Vec2> {
    let (r, s) = (b - a, d - c);
    let denominator = r.perp_dot(s);
    if denominator.abs() < 1e-6 {
        return None;
    }
    let t = (c - a).perp_dot(s) / denominator;
    let u = (c - a).perp_dot(r) / denominator;
    if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
        return None;
    }
    Some(a + r * t)
}

/// Every place a street crosses a river, worked out once from the layout.
///
/// Pure, and quadratic only in principle: a bounding-box reject on each pair
/// throws away all but a handful of the town's edge-against-segment tests
/// before any arithmetic happens, and the answer for Landshut is six.
pub fn crossings(layout: &CityLayout) -> Vec<Crossing> {
    let mut found: Vec<Crossing> = Vec::new();
    for arm in &layout.waters {
        if arm.width < NAVIGABLE {
            continue;
        }
        for pair in arm.points.windows(2) {
            let (wet_a, wet_b) = (pair[0], pair[1]);
            let (low, high) = (wet_a.min(wet_b), wet_a.max(wet_b));
            for edge in layout.graph.edges() {
                let (road_a, road_b) =
                    (layout.graph.node(edge.a).pos, layout.graph.node(edge.b).pos);
                // The cheap reject: two segments whose boxes miss cannot meet,
                // and almost every pair in a town of sixteen hundred roads and
                // six thousand river points is such a pair.
                if road_a.min(road_b).cmpgt(high).any() || road_a.max(road_b).cmplt(low).any() {
                    continue;
                }
                let Some(point) = meeting(wet_a, wet_b, road_a, road_b) else {
                    continue;
                };
                let Some(along) = (road_b - road_a).try_normalize() else {
                    continue;
                };
                // How much river there is to cross, along the road. A river met
                // square on is its own width; one met obliquely is wider, and
                // the deck has to reach the whole way.
                let wet = (wet_b - wet_a).normalize_or_zero();
                let sine = wet.perp_dot(along).abs().max(0.25);
                let span = arm.width / sine;
                // One crossing per place, not one per segment of a polyline
                // that wanders across the same carriageway twice.
                if let Some(already) = found
                    .iter_mut()
                    .find(|had| had.at.distance(point) < arm.width + edge.width)
                {
                    already.span = already.span.max(span);
                    continue;
                }
                found.push(Crossing {
                    at: point,
                    along,
                    deck: edge.width,
                    span,
                });
            }
        }
    }
    // A stable order, so the same town builds the same bridges in the same
    // order however the graph was walked.
    found.sort_by(|a, b| a.at.x.total_cmp(&b.at.x).then(a.at.y.total_cmp(&b.at.y)));
    found
}

/// Builds every bridge in the town.
///
/// Spawned once at world build rather than streamed, for the reason
/// `world::river` gives for the water itself: a river that blinks out at the
/// stream radius is a hole in the most legible thing in the town, and a bridge
/// that blinks out with it is worse. Six bridges are about sixty entities.
pub fn spawn(
    commands: &mut Commands,
    crossings: &[Crossing],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    if crossings.is_empty() {
        return;
    }
    // Ashlar: the stone a river wall and a bridge parapet are the same stone,
    // a shade lighter than the bank so the two read as built at different
    // times, which in every one of these towns they were.
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.50, 0.49, 0.46),
        perceptual_roughness: 0.88,
        ..default()
    });
    let iron = materials.add(StandardMaterial {
        base_color: Color::srgb(0.17, 0.18, 0.19),
        perceptual_roughness: 0.55,
        metallic: 0.5,
        ..default()
    });
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.86, 0.66),
        // Warm and unlit: the lamp pool `world::streetlights` keeps is small
        // and spoken for, and a lantern that only *looks* lit is what reads
        // from the far bank either way.
        emissive: LinearRgba::rgb(1.4, 1.1, 0.5),
        perceptual_roughness: 0.35,
        ..default()
    });
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let lantern = meshes.add(Cuboid::new(LANTERN.x, LANTERN.y, LANTERN.z));

    let mut parapets = 0usize;
    let mut lamps = 0usize;
    let mut piers = 0usize;
    for bridge in crossings {
        let across = Vec2::new(-bridge.along.y, bridge.along.x);
        let yaw = bridge.along.x.atan2(bridge.along.y);
        let turn = Quat::from_rotation_y(yaw);
        let length = bridge.span + ABUTMENT * 2.0;

        for side in [-1.0f32, 1.0] {
            let line = bridge.at + across * (side * bridge.kerb());
            commands.spawn((
                Name::new("Bridge parapet"),
                Mesh3d(cube.clone()),
                MeshMaterial3d(stone.clone()),
                Transform::from_xyz(
                    line.x,
                    super::buildings::SIDEWALK_HEIGHT + PARAPET_HEIGHT * 0.5,
                    line.y,
                )
                .with_rotation(turn)
                // A unit cube scaled by its transform, and the collider with
                // it: Avian scales a collider by the transform, so the shape
                // below is one metre cubed and this is what makes it a wall.
                .with_scale(Vec3::new(PARAPET_THICK, PARAPET_HEIGHT, length)),
                RigidBody::Static,
                Collider::cuboid(1.0, 1.0, 1.0),
            ));
            parapets += 1;

            // The standards, counted out from the middle so a bridge is
            // symmetrical about its own centre rather than about one abutment.
            let each_way = (length * 0.5 / LAMP_SPACING).floor() as i32;
            for step in -each_way..=each_way {
                let foot = line + bridge.along * (step as f32 * LAMP_SPACING);
                let base = super::buildings::SIDEWALK_HEIGHT + PARAPET_HEIGHT;
                commands.spawn((
                    Name::new("Bridge lamp"),
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(iron.clone()),
                    Transform::from_xyz(foot.x, base + LAMP_HEIGHT * 0.5, foot.y)
                        .with_rotation(turn)
                        .with_scale(Vec3::new(LAMP_THICK, LAMP_HEIGHT, LAMP_THICK)),
                ));
                commands.spawn((
                    Name::new("Bridge lantern"),
                    Mesh3d(lantern.clone()),
                    MeshMaterial3d(glass.clone()),
                    Transform::from_xyz(foot.x, base + LAMP_HEIGHT + LANTERN.y * 0.5, foot.y)
                        .with_rotation(turn),
                ));
                lamps += 1;
            }
        }

        // The pier heads. Counted out from the middle like the lamps, and only
        // where there is a span worth carrying: a ten-metre crossing is a
        // culvert with a parapet on it.
        if bridge.span < PIER_SPACING {
            continue;
        }
        let bays = (bridge.span / PIER_SPACING).round().max(1.0);
        let gap = bridge.span / bays;
        let inner = (bays as i32 - 1).max(0);
        for step in 0..inner {
            let offset = -bridge.span * 0.5 + gap * (step + 1) as f32;
            for side in [-1.0f32, 1.0] {
                let nose = bridge.at
                    + bridge.along * offset
                    + across * (side * (bridge.kerb() + PIER_PROUD * 0.5));
                commands.spawn((
                    Name::new("Bridge pier"),
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(stone.clone()),
                    // Standing from the river bed — such as it is; see the
                    // module doc — up to just over the surface.
                    Transform::from_xyz(nose.x, PIER_RISE * 0.5 - 0.1, nose.y)
                        .with_rotation(turn)
                        .with_scale(Vec3::new(PIER_PROUD, PIER_RISE + 0.2, PIER_ALONG)),
                    RigidBody::Static,
                    Collider::cuboid(1.0, 1.0, 1.0),
                ));
                piers += 1;
            }
        }
    }
    info!(
        "{} bridges: {parapets} parapets, {lamps} lamps, {piers} pier heads",
        crossings.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest layout this module reads: a graph and some water.
    fn bare() -> CityLayout {
        CityLayout {
            seed: 1,
            half_extent: 1_000.0,
            x_streets: Vec::new(),
            z_streets: Vec::new(),
            blocks: Vec::new(),
            graph: super::super::roadgraph::RoadGraph::default(),
            canal: None,
            grounds: Vec::new(),
            waters: Vec::new(),
            relief: None,
        }
    }

    /// Two segments that cross, and two that do not.
    #[test]
    fn a_meeting_is_where_two_segments_actually_meet() {
        let hit = meeting(
            Vec2::new(-1.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, -1.0),
            Vec2::new(0.0, 1.0),
        );
        assert_eq!(hit, Some(Vec2::ZERO));
        // Parallel.
        assert_eq!(
            meeting(
                Vec2::ZERO,
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0)
            ),
            None
        );
        // Crossing lines, but past the end of one of the segments.
        assert_eq!(
            meeting(
                Vec2::new(-1.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 5.0),
                Vec2::new(0.0, 9.0)
            ),
            None
        );
    }

    /// A deck is long and narrow, and knows it.
    #[test]
    fn a_deck_carries_its_own_length_and_no_more() {
        let bridge = Crossing {
            at: Vec2::ZERO,
            along: Vec2::Y,
            deck: 12.0,
            span: 40.0,
        };
        assert!(bridge.carries(Vec2::ZERO));
        // Along it, out to the abutment and no further.
        assert!(bridge.carries(Vec2::new(0.0, 20.0)));
        assert!(!bridge.carries(Vec2::new(0.0, 26.0)));
        // Across it, out to the parapet and no further.
        assert!(bridge.carries(Vec2::new(bridge.kerb(), 0.0)));
        assert!(!bridge.carries(Vec2::new(bridge.kerb() + PARAPET_THICK * 2.0, 0.0)));
        assert!(on_a_deck(&[bridge], Vec2::new(2.0, 10.0)));
        assert!(!on_a_deck(&[bridge], Vec2::new(60.0, 10.0)));
    }

    /// An oblique crossing is longer than the river is wide.
    #[test]
    fn a_river_met_at_an_angle_takes_a_longer_deck() {
        let mut layout = bare();
        let a = layout.graph.add_node(Vec2::new(-60.0, 0.0), (0, 0));
        let b = layout.graph.add_node(Vec2::new(60.0, 0.0), (0, 1));
        layout
            .graph
            .connect(a, b, 10.0, true, super::super::atlas::Surface::Asphalt);
        // A river at forty-five degrees, twenty metres wide.
        layout.waters.push(super::super::citygen::Waterway {
            name: String::new(),
            width: 20.0,
            points: vec![Vec2::new(-40.0, -40.0), Vec2::new(40.0, 40.0)],
        });
        let found = crossings(&layout);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].at.distance(Vec2::ZERO) < 0.01, "{:?}", found[0].at);
        // sin 45 is about 0.707, so the span is the width over that.
        assert!(
            (found[0].span - 20.0 / std::f32::consts::FRAC_1_SQRT_2.recip()).abs() < 0.5
                || (found[0].span - 28.28).abs() < 0.5,
            "a 20 m river at 45 degrees spans {} m",
            found[0].span
        );
    }

    /// Landshut's six bridges are found, and each spans its own river.
    ///
    /// The Zweibrückenstraße crosses both arms of the Isar, which is what its
    /// name says it does, so a register of crossings that misses one of them
    /// is wrong in a way the map itself will tell you about.
    #[test]
    fn the_committed_landshut_finds_its_bridges() {
        use crate::core::config::CityStyle;
        let path = crate::core::assets::root().join("cities/landshut.ron");
        let town = super::super::atlas::load("landshut");
        assert!(
            town.is_some() || !path.exists(),
            "{} is on disk and does not load as an atlas",
            path.display()
        );
        let Some(town) = town else { return };
        let _ = CityStyle::Landshuepf;
        let (layout, _) = super::super::atlas::layout(&town, 1, 1_000.0);
        let found = crossings(&layout);

        assert_eq!(
            found.len(),
            6,
            "{} street-over-river crossings, not six: {:?}",
            found.len(),
            found.iter().map(|c| c.at).collect::<Vec<_>>()
        );
        for bridge in &found {
            // Every one of these is over the Isar or one of its arms, and the
            // narrowest of those is forty-two metres.
            assert!(
                bridge.span >= 42.0 && bridge.span < 200.0,
                "a {:.0} m span at {:?}",
                bridge.span,
                bridge.at
            );
            assert!(bridge.deck > 4.0, "a {:.1} m deck", bridge.deck);
            assert!(bridge.along.is_normalized(), "{:?}", bridge.along);
            // And its own middle is on its own deck, which is the property
            // `world::river` leans on to keep a wall off a carriageway.
            assert!(bridge.carries(bridge.at));
        }
        // No two of them in the same place: one crossing per street per river,
        // not one per segment of a polyline that wanders.
        for (index, one) in found.iter().enumerate() {
            for other in &found[index + 1..] {
                assert!(
                    one.at.distance(other.at) > 40.0,
                    "two bridges at {:?} and {:?}",
                    one.at,
                    other.at
                );
            }
        }
    }

    /// A mill race is not a river and gets no bridge.
    #[test]
    fn a_culverted_stream_is_not_bridged() {
        let mut layout = bare();
        let a = layout.graph.add_node(Vec2::new(-60.0, 0.0), (0, 0));
        let b = layout.graph.add_node(Vec2::new(60.0, 0.0), (0, 1));
        layout
            .graph
            .connect(a, b, 10.0, true, super::super::atlas::Surface::Asphalt);
        layout.waters.push(super::super::citygen::Waterway {
            name: String::new(),
            width: 5.0,
            points: vec![Vec2::new(0.0, -40.0), Vec2::new(0.0, 40.0)],
        });
        assert!(crossings(&layout).is_empty());
    }
}
