//! The signs that say who may use a street.
//!
//! `world::streetname` puts the name on the corner; this puts the law under
//! it. Two facts the map has always carried and the game never asked for:
//! nine per cent of Landshut's road length is one-way, and nearly five
//! kilometres of it is a Fussgaengerzone — including the Altstadt, the market
//! street the whole postcard is of, down which this game had been driving
//! traffic and parking forty cars.
//!
//! The behaviour is in `roadgraph::Rules` and the two places that read it
//! (`ai::traffic` and `vehicle::spawn`). This is the half a player can see.
//! It has to be both halves or neither: a street that is empty of cars for no
//! visible reason reads as a bug in the traffic, and a sign on a street the
//! traffic ignores reads as a bug in the sign.
//!
//! ## Where a sign goes
//!
//! At a junction, and only at one. An OSM street arrives as a polyline of a
//! dozen segments, so a sign per edge would be a sign per bend — the Neustadt
//! would carry eleven Einbahnstraße plates down one side. A real sign stands
//! where the rule *starts*, which is at the mouth: see [`at_mouth`].
//!
//! ## Which sign
//!
//! The four the StVO uses for exactly this, and they are four rather than two
//! because a rule has two ends and they do not look alike:
//!
//! * **Zeichen 220** — Einbahnstraße, the blue plate with the white arrow, at
//!   the end you may drive in.
//! * **Zeichen 267** — Verbot der Einfahrt, the red disc with the white bar,
//!   at the end you may not. This is the one that does the work: it is what
//!   you see when you look up a one-way street the wrong way, and it is the
//!   only sign here a player will ever be stopped by.
//! * **Zeichen 242.1 / 242.2** — Fussgaengerzone and its end, the blue square
//!   with the two figures, one each way at the boundary. Both, because you
//!   walk out of a pedestrian zone as often as you walk into one.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::citygen::SIDEWALK_WIDTH;
use super::roadgraph::{EdgeId, NodeId, Oneway, RoadGraph};
use super::texture::painted_rect;

/// How far a sign is drawn.
///
/// Further than a name plate, which is a hand's width of lettering nobody can
/// read at a distance. This is a shape and a colour: a red disc at a hundred
/// metres is still a red disc, and the whole use of it is being seen from up
/// the street rather than at the corner.
pub const RANGE: f32 = 150.0;

/// The blue plate, in metres. A German Zeichen 220 in a town.
const PLATE: Vec2 = Vec2::new(0.90, 0.40);
/// And the round and square ones, which share a size.
const DISC: f32 = 0.60;
/// How high the middle of a sign rides above the pavement. Lower than the
/// two-and-a-half a Bundesstraße uses, because these are read from a car in a
/// narrow street and from the pavement beside it.
const HEIGHT: f32 = 2.20;

/// Texels across a sign face.
const TEXELS: u32 = 256;

// A sign hangs above head height and under a first-floor window, like the
// name plate it stands below.
const _: () = assert!(HEIGHT > 1.9 && HEIGHT < 2.8);

/// One of the four faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Zeichen 220. Carries which way the arrow points: a sign is read by
    /// somebody standing across the street from it, so the arrow has to point
    /// the way the traffic actually goes and not merely to the right.
    Einbahn { rightwards: bool },
    /// Zeichen 267.
    Einfahrtverbot,
    /// Zeichen 242.1.
    Fussgaengerzone,
    /// Zeichen 242.2.
    ZoneEnde,
}

impl Face {
    fn index(self) -> usize {
        match self {
            Face::Einbahn { rightwards: false } => 0,
            Face::Einbahn { rightwards: true } => 1,
            Face::Einfahrtverbot => 2,
            Face::Fussgaengerzone => 3,
            Face::ZoneEnde => 4,
        }
    }

    /// Whether this face is painted on the long blue plate rather than on the
    /// square one.
    fn oblong(self) -> bool {
        matches!(self, Face::Einbahn { .. })
    }
}

/// What sign stands at the mouth of `edge` where it meets `at`, if any.
///
/// The rule the whole module turns on, and it is a rule about *boundaries*.
/// A sign is put up where what is allowed changes: at a junction, because
/// somebody arriving there has to be told, and at a plain bend only where the
/// street on the far side of it is governed differently. In the middle of a
/// one-way street nothing changes and nothing is signed, which is what keeps
/// a curved Gasse from wearing the same plate at every kink in it.
///
/// Pure, and separately tested: it is the half of this module that can be
/// wrong in a way a screenshot of an empty street will not show.
pub fn at_mouth(graph: &RoadGraph, at: NodeId, edge: EdgeId) -> Option<Face> {
    let road = graph.edge(edge);
    let arms = graph.node(at).edges.len();
    // A boundary is a junction, or a bend where the rule on the other side is
    // a different rule. Anything else is the middle of a street.
    let boundary = arms >= 3
        || graph
            .node(at)
            .edges
            .iter()
            .any(|other| *other != edge && graph.edge(*other).rules != road.rules);
    if !boundary {
        return None;
    }
    if road.rules.pedestrian {
        // Into the zone or out of it. A node with nothing but pedestrian
        // streets on it is inside the zone and is not a boundary at all —
        // which the `rules` comparison above has already refused, unless it
        // is a junction. So ask the question the sign asks: is there any way
        // off this node that a car may use?
        let driveable_here = graph
            .node(at)
            .edges
            .iter()
            .any(|other| !graph.edge(*other).rules.pedestrian);
        return driveable_here.then_some(Face::Fussgaengerzone);
    }
    // Leaving one. The sign belongs on the street you leave along, facing the
    // way you are going, which is this edge seen from a node the zone touches.
    if road.rules.oneway == Oneway::Both {
        let leaving_zone = graph
            .node(at)
            .edges
            .iter()
            .any(|other| graph.edge(*other).rules.pedestrian);
        return leaving_zone.then_some(Face::ZoneEnde);
    }
    // One-way: which of its two ends this is.
    Some(if road.drivable_from(at) {
        Face::Einbahn {
            // Worked out by the caller, which is the only place that knows
            // which way the sign will be turned.
            rightwards: false,
        }
    } else {
        Face::Einfahrtverbot
    })
}

#[derive(Resource)]
pub struct RoadSignKit {
    plate: Handle<Mesh>,
    square: Handle<Mesh>,
    post: Handle<Mesh>,
    steel: Handle<StandardMaterial>,
    faces: Vec<Handle<StandardMaterial>>,
}

impl RoadSignKit {
    fn face(&self, face: Face) -> Option<&Handle<StandardMaterial>> {
        self.faces.get(face.index())
    }
}

/// German traffic blue and traffic red, and the white that goes on them.
const BLUE: [f32; 3] = [0.043, 0.208, 0.451];
const RED: [f32; 3] = [0.663, 0.075, 0.106];
const WHITE: [f32; 3] = [0.94, 0.94, 0.93];
/// The grey back of a sign, and its rim.
const BACK: [f32; 3] = [0.55, 0.56, 0.57];

fn ink(colour: [f32; 3]) -> [u8; 4] {
    [
        super::texture::byte(colour[0]),
        super::texture::byte(colour[1]),
        super::texture::byte(colour[2]),
        255,
    ]
}

/// Zeichen 220: a white arrow on blue, pointing along the street.
fn einbahn(rightwards: bool) -> Image {
    let (width, height) = (TEXELS, TEXELS * 4 / 9);
    painted_rect(width, height, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let u = if rightwards { u } else { 1.0 - u };
        // A white border inside a blue plate, the way the plate is made.
        let edge = 0.035;
        if u < edge || u > 1.0 - edge || v < edge * 2.25 || v > 1.0 - edge * 2.25 {
            return ink(WHITE);
        }
        // The arrow: a shaft down the middle and a head at the pointed end.
        let shaft = (v - 0.5).abs() < 0.10 && (0.16..0.70).contains(&u);
        // The head is a triangle whose width closes as it runs out to the tip.
        let head = (0.62..0.86).contains(&u) && (v - 0.5).abs() < (0.86 - u) * 1.30;
        if shaft || head { ink(WHITE) } else { ink(BLUE) }
    })
}

/// Zeichen 267: a white bar across a red disc.
fn einfahrtverbot() -> Image {
    painted_rect(TEXELS, TEXELS, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let radius = Vec2::new(u - 0.5, v - 0.5).length();
        if radius > 0.49 {
            // Outside the disc. The face is a quad and the sign is round, so
            // the corners have to go — and they go by being the grey of the
            // sign's own back rather than by an alpha channel, which would
            // buy a transparent pass over every sign in the town to hide four
            // triangles' worth of corner.
            return ink(BACK);
        }
        if radius > 0.455 {
            return ink([BACK[0] * 0.8, BACK[1] * 0.8, BACK[2] * 0.8]);
        }
        if (v - 0.5).abs() < 0.115 && radius < 0.44 {
            ink(WHITE)
        } else {
            ink(RED)
        }
    })
}

/// Zeichen 242: two figures on blue, an adult and a child, and for the end of
/// the zone the red band across them.
fn fussgaengerzone(ending: bool) -> Image {
    painted_rect(TEXELS, TEXELS, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let edge = 0.045;
        if u < edge || u > 1.0 - edge || v < edge || v > 1.0 - edge {
            return ink(WHITE);
        }
        // A walking figure: a head, a body that tapers, and two legs apart.
        // Drawn rather than written, because a pedestrian zone is the one
        // sign in this town that says what it means in pictures.
        let figure = |centre: f32, scale: f32| {
            let x = (u - centre) / scale;
            let y = (v - 0.52) / scale;
            let head = Vec2::new(x, y + 0.30).length() < 0.085;
            let body = (-0.20..0.10).contains(&y) && x.abs() < 0.075 - y * 0.10;
            // Two legs, leaning apart below the body.
            let legs = (0.10..0.34).contains(&y) && (x - (y - 0.10) * 0.55).abs() < 0.050;
            let back = (0.10..0.30).contains(&y) && (x + (y - 0.10) * 0.30).abs() < 0.050;
            head || body || legs || back
        };
        let white = figure(0.42, 1.0) || figure(0.62, 0.62);
        if ending {
            // The red band, corner to corner, over everything.
            let band = ((u - v) * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.045;
            if band {
                return ink(RED);
            }
        }
        if white { ink(WHITE) } else { ink(BLUE) }
    })
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> RoadSignKit {
    let faces = [
        einbahn(false),
        einbahn(true),
        einfahrtverbot(),
        fussgaengerzone(false),
        fussgaengerzone(true),
    ]
    .into_iter()
    .map(|image| {
        materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            // Retroreflective sheeting is not a mirror and is not a chalk
            // board. Flat enough that the face reads as one colour from up
            // the street, which is the whole job.
            perceptual_roughness: 0.55,
            metallic: 0.0,
            ..default()
        })
    })
    .collect();

    RoadSignKit {
        plate: meshes.add(Rectangle::new(PLATE.x, PLATE.y)),
        square: meshes.add(Rectangle::new(DISC, DISC)),
        post: meshes.add(Cylinder::new(0.5, 1.0)),
        steel: materials.add(StandardMaterial {
            base_color: Color::srgb(BACK[0], BACK[1], BACK[2]),
            perceptual_roughness: 0.62,
            metallic: 0.35,
            ..default()
        }),
        faces,
    }
}

/// Stands one sign at the mouth of a street.
///
/// `at` is the junction, `towards` the far end of the street being signed.
/// The face looks back at whoever is arriving, which is the opposite of what
/// a name plate does — a plate lies flat along its street and is read walking
/// past; a sign is aimed at a driver and has one correct angle.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    kit: &RoadSignKit,
    corridors: &super::streetside::Corridors,
    face: Face,
    at: Vec2,
    towards: Vec2,
    width: f32,
    widest: f32,
    chunk: IVec2,
    range: f32,
) {
    let Ok(direction) = Dir2::new(towards - at) else {
        return;
    };
    let normal = Vec2::new(-direction.y, direction.x);
    // On the right-hand pavement of the street it governs, a little way in
    // from the junction — the same walk out along the arm the name plates
    // take, and for the same reason: the arms of a real junction leave at
    // every angle, so the only honest way to keep a post off the tarmac is to
    // ask whether it is on it.
    let sideways = -normal * (width * 0.5 + SIDEWALK_WIDTH * 0.4);
    let Some(foot) = (0..4)
        .map(|step| at + *direction * (widest * 0.5 + 1.6 + step as f32 * 2.2) + sideways)
        .find(|foot| !corridors.in_the_road(*foot, 0.4))
    else {
        return;
    };

    let visibility = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: range..(range * 1.1),
        use_aabb: false,
    };
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.post.clone()),
        MeshMaterial3d(kit.steel.clone()),
        Transform::from_xyz(foot.x, SIDEWALK_HEIGHT + HEIGHT * 0.5, foot.y)
            .with_scale(Vec3::new(0.055, HEIGHT, 0.055)),
        visibility.clone(),
    ));

    // Which way the sign looks is not one answer, because these signs are not
    // all doing the same job.
    //
    // A prohibition is aimed at one driver: 267 and 242 stand across the
    // mouth of the street they govern and look back at whoever is about to
    // enter it. That is a face and a grey back.
    //
    // Zeichen 220 cannot be mounted that way and mean anything. Its whole
    // content is a *direction*, and a street seen head-on runs away from the
    // viewer — a left-or-right arrow has nothing to say about it. So it goes
    // where a real one goes: flat along its own street, like the name plate
    // above it, read by the driver on the crossing street who needs to know
    // which way this one runs. Both its faces are painted, and they are
    // painted with *opposite* arrows, because they are the same arrow seen
    // from the two sides — which is also why the back of a quad showing its
    // texture mirrored is a help here and a bug everywhere else.
    let plate = |commands: &mut Commands, face: Face, look: Vec2| {
        let Some(paint) = kit.face(face) else {
            return;
        };
        let mesh = if face.oblong() {
            &kit.plate
        } else {
            &kit.square
        };
        let at = foot + look * 0.035;
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(paint.clone()),
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT + HEIGHT, at.y)
                .with_rotation(Quat::from_rotation_y(look.x.atan2(look.y))),
            visibility.clone(),
            bevy::light::NotShadowCaster,
        ));
    };

    if let Face::Einbahn { .. } = face {
        // The traffic runs from the junction into the street, because this
        // sign only ever stands at the end a car may enter. A viewer on the
        // `normal` side of the plate is looking back along `-normal`, and for
        // them the street runs to the right; the other side sees it run left.
        plate(commands, Face::Einbahn { rightwards: true }, normal);
        plate(commands, Face::Einbahn { rightwards: false }, -normal);
        return;
    }

    let facing = -*direction;
    plate(commands, face, facing);
    // A plain grey back, so a sign read from behind is the back of a sign.
    let back = foot - facing * 0.035;
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.square.clone()),
        MeshMaterial3d(kit.steel.clone()),
        Transform::from_xyz(back.x, SIDEWALK_HEIGHT + HEIGHT, back.y)
            .with_rotation(Quat::from_rotation_y((-facing).x.atan2((-facing).y))),
        visibility,
        bevy::light::NotShadowCaster,
    ));
}

#[cfg(test)]
mod tests {
    use super::super::atlas::Surface;
    use super::super::roadgraph::Rules;
    use super::*;

    /// A straight street of `rules`, node by node, so a mouth can be asked
    /// about.
    fn run(rules: &[Rules]) -> (RoadGraph, Vec<NodeId>, Vec<EdgeId>) {
        let mut graph = RoadGraph::default();
        let nodes: Vec<_> = (0..=rules.len())
            .map(|i| graph.add_node(Vec2::new(i as f32 * 40.0, 0.0), (i as u16, 0)))
            .collect();
        let edges = rules
            .iter()
            .enumerate()
            .map(|(i, rules)| {
                graph.connect_under(nodes[i], nodes[i + 1], 8.0, false, Surface::Asphalt, *rules)
            })
            .collect();
        (graph, nodes, edges)
    }

    const OPEN: Rules = Rules {
        oneway: Oneway::Both,
        pedestrian: false,
    };
    const ALONG: Rules = Rules {
        oneway: Oneway::Along,
        pedestrian: false,
    };
    const QUIET: Rules = Rules {
        oneway: Oneway::Both,
        pedestrian: true,
    };

    /// The middle of a street is not a boundary, whatever the street is.
    ///
    /// The failure this is here for is a sign per *edge*: a real street
    /// arrives as a polyline of a dozen segments, and the Neustadt would wear
    /// eleven Einbahnstraße plates down one side of it.
    #[test]
    fn nothing_is_signed_in_the_middle_of_a_street() {
        let (graph, nodes, edges) = run(&[ALONG, ALONG, ALONG]);
        assert_eq!(at_mouth(&graph, nodes[1], edges[1]), None);
        assert_eq!(at_mouth(&graph, nodes[2], edges[1]), None);
    }

    /// A one-way street is signed at both ends, and not with the same sign.
    #[test]
    fn a_one_way_street_is_signed_at_both_ends() {
        let (graph, nodes, edges) = run(&[OPEN, ALONG, OPEN]);
        assert_eq!(
            at_mouth(&graph, nodes[1], edges[1]),
            Some(Face::Einbahn { rightwards: false }),
            "the end a car may drive in at"
        );
        assert_eq!(
            at_mouth(&graph, nodes[2], edges[1]),
            Some(Face::Einfahrtverbot),
            "the end it may not"
        );
    }

    /// And a pedestrian zone is signed on the way in and on the way out.
    #[test]
    fn a_pedestrian_zone_is_signed_at_its_boundary() {
        let (graph, nodes, edges) = run(&[OPEN, QUIET, OPEN]);
        assert_eq!(
            at_mouth(&graph, nodes[1], edges[1]),
            Some(Face::Fussgaengerzone),
            "walking in"
        );
        // The way out is signed on the street you leave along, not on the one
        // you leave.
        assert_eq!(
            at_mouth(&graph, nodes[2], edges[2]),
            Some(Face::ZoneEnde),
            "walking out"
        );
        // Deep inside the zone there is no boundary and no sign.
        let (graph, nodes, edges) = run(&[QUIET, QUIET, QUIET]);
        assert_eq!(at_mouth(&graph, nodes[1], edges[1]), None);
    }

    /// Every face is painted, and none of them comes out one flat colour.
    ///
    /// The same slip the plaques carry a test for: a sign that paints all
    /// field or all ink still builds and still stands, it just stops saying
    /// anything.
    #[test]
    fn every_face_has_a_picture_on_it() {
        for (label, image) in [
            ("220 left", einbahn(false)),
            ("220 right", einbahn(true)),
            ("267", einfahrtverbot()),
            ("242.1", fussgaengerzone(false)),
            ("242.2", fussgaengerzone(true)),
        ] {
            let data = image.data.as_ref().expect("the sign was not painted");
            let white = data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 200)
                .count();
            let all = data.as_chunks::<4>().0.len();
            assert!(
                white * 20 > all && white * 2 < all,
                "{label}: {:.0}% of the face is white",
                white as f32 / all as f32 * 100.0
            );
        }
    }

    /// The two Einbahn faces are mirror images and not the same picture.
    #[test]
    fn the_arrow_points_both_ways() {
        let (left, right) = (einbahn(false), einbahn(true));
        let (a, b) = (
            left.data.as_ref().expect("unpainted"),
            right.data.as_ref().expect("unpainted"),
        );
        assert_ne!(a, b, "both arrows point the same way");
    }
}
