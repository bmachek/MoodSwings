//! The monuments a real town actually has, where it actually has them.
//!
//! `world::statues` keeps the city's invented ones: five jokes on rotation in
//! the parks of a generated town, placed from the seed. This keeps the other
//! kind — the fountain a square is photographed *for*. A Bavarian Altstadt is
//! not a lawn with a plinth on it. It is a long paved street with something
//! standing in the middle of it, and in Landshut two of those are the reason
//! anybody points a camera at the square they stand on.
//!
//! ## Why these are not baked into the atlas
//!
//! Because the bake cannot see them. `tools/fetch-city.sh` asks Overpass for
//! `way["building"]` and nothing else, and a fountain is a *node* — it has no
//! ring, so `polygons_of` would throw it away even if it were fetched. Overture
//! carries buildings and not street furniture at all. So the one thing the
//! pipeline can never hand us is exactly the thing that stands in the middle of
//! the square, and a register here is the honest way to have it: a short list
//! of what the town is known to keep, anchored to a street the atlas *does*
//! know the shape of.
//!
//! ## What is claimed, and what is not
//!
//! A register of real places invites invention, so the rule is narrow: an entry
//! names a monument the town is known for and the square it stands on, and
//! nothing more. It does **not** claim a surveyed position — the atlas has no
//! coordinate for it, and pretending otherwise would be a measurement nobody
//! took. What is placed is "on this square, clear of the carriageway", which is
//! true of the real one and is all the game needs to be.
//!
//! The third entry is a different kind of claim and says so: every market town
//! in this game keeps a fountain on its market street, because every market
//! town does. That one is a *rule*, not a record, and it is why this module is
//! worth its length for the towns that are not Landshut.
//!
//! ## Placement
//!
//! Pure, and computed once at world build rather than per chunk: the named
//! street is looked up in [`Signposts`], a point is taken along it, and the
//! monument is stepped sideways off the centreline until it is clear of every
//! carriageway (`streetside::Corridors`) and out of every building. A monument
//! that cannot find room is left out with a warning rather than stood in the
//! road — see [`place`]. The resolved list is a resource, so the chunk that
//! respawns it forever gets the same answer forever.

use avian3d::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::atlas::Signposts;
use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::citygen::CityLayout;
use super::roadgraph::RoadGraph;
use super::streetside::Corridors;
use super::texture::{encode, painted_rect, text_band};

/// How far a monument draws. The same reach `world::statues` gives its park
/// pieces: a landmark reads further than street furniture and need not survive
/// the horizon.
const RANGE: f32 = 420.0;

/// What shape a monument is.
///
/// Two, because the town has two: a basin with water in it, and a column with a
/// figure on top. Everything else a square keeps — a bench, a tree, a bollard —
/// already has a module.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Piece {
    /// A stone basin with water in it, a shaft up the middle and a bronze
    /// figure over that.
    Fountain,
    /// A stone column on a stepped base, with a gilt figure on top.
    Column,
}

impl Piece {
    /// How much room the piece needs, as a radius in metres. Used both to keep
    /// it off the carriageway and to keep it out of a wall.
    fn reach(self) -> f32 {
        match self {
            Piece::Fountain => BASIN_RADIUS,
            Piece::Column => COLUMN_BASE * 0.5,
        }
    }
}

/// One entry of the register: what stands where.
struct Listed {
    /// The atlas this is a fact about, by the name the baked file carries.
    /// A register entry is about *a town*, not about a style.
    town: &'static str,
    /// What the plaque says. The monument's own name, in the town's spelling.
    label: &'static str,
    /// The street it stands on, spelled as OpenStreetMap spells it — which is
    /// what [`Signposts`] holds and therefore what can be looked up.
    on: &'static str,
    piece: Piece,
}

/// The register.
///
/// Two named monuments and one rule. See the module doc for what each kind of
/// entry claims.
const LISTED: &[Listed] = &[
    // Landshut's best-known fountain, on the square inside the Ländtor at the
    // Isar end of the Altstadt. The square is short and the gate stands in the
    // middle of it, so the fountain is taken from the western arm rather than
    // from the middle, where the gate is.
    Listed {
        town: "Landshut",
        label: "NARRENBRUNNEN",
        on: "Ländtorplatz",
        piece: Piece::Fountain,
    },
    // The column the square is named after. This is the one entry whose
    // *existence* the atlas itself attests: a Dreifaltigkeitsplatz is a square
    // with a Dreifaltigkeitssäule on it, and the name is in the baked file.
    Listed {
        town: "Landshut",
        label: "DREIFALTIGKEITSSÄULE",
        on: "Dreifaltigkeitsplatz",
        piece: Piece::Column,
    },
    // And the rule. The Altstadt is Landshut's market street — the widest
    // sett-paved band in the town, which is what a market street is — and a
    // market street keeps a fountain. Not a record of a particular one: a
    // statement that a market town has one, which is as true of the invented
    // towns as of this one.
    Listed {
        town: "Landshut",
        label: "MARKTBRUNNEN",
        on: "Altstadt",
        piece: Piece::Fountain,
    },
];

/// The basin: how far across, how high its wall stands, and how thick the rim
/// reads from outside.
///
/// A metre and three quarters of radius: three and a half metres across, which
/// is a town's fountain — about the length of a small car, the only scale
/// reference a street ever offers. Two and a half was tried first and is a
/// city's; at five metres across it had to be stood so far back from the
/// carriageway that there was nowhere on any of Landshut's squares to put it,
/// which is how this number was arrived at rather than chosen.
const BASIN_RADIUS: f32 = 1.75;
const BASIN_HEIGHT: f32 = 0.62;
const BASIN_RIM: f32 = 0.22;
/// How far under the rim the water lies. A basin filled to its own rim is a
/// disc of water with no basin round it.
const WATER_BELOW: f32 = 0.14;

/// The shaft up the middle of the basin, and the bowl it carries.
const SHAFT_RADIUS: f32 = 0.3;
const SHAFT_HEIGHT: f32 = 1.75;
const BOWL_RADIUS: f32 = 0.78;
const BOWL_HEIGHT: f32 = 0.2;
/// The bronze figure over the bowl: a head-sized ball, at head height above it.
const FIGURE_RADIUS: f32 = 0.34;

/// The column: a stepped base, a shaft, a capital and a gilt figure.
///
/// Seven metres to the capital. A Baroque Säule on a small square runs eight to
/// twelve to the top of its figure, and the base is what makes it read as one —
/// a bare pole of the same height is a lamp post.
const COLUMN_BASE: f32 = 2.6;
const COLUMN_STEPS: usize = 3;
const COLUMN_STEP_HEIGHT: f32 = 0.24;
const COLUMN_PLINTH: Vec3 = Vec3::new(1.3, 1.5, 1.3);
const COLUMN_SHAFT_RADIUS: f32 = 0.34;
const COLUMN_SHAFT_HEIGHT: f32 = 5.2;
const COLUMN_CAPITAL: Vec3 = Vec3::new(0.86, 0.3, 0.86);
const COLUMN_FIGURE_RADIUS: f32 = 0.42;

/// The bronze plate on the front of the thing, in metres. The same plate the
/// park monuments wear — a city buys its plaques by the sheet.
const PLAQUE: Vec2 = Vec2::new(0.9, 0.3);

// The shaft has to stand *in* the basin and the figure has to stand on the
// bowl, or what is drawn is a fountain with a pillar beside it.
const _: () = assert!(SHAFT_RADIUS + 0.2 < BASIN_RADIUS - BASIN_RIM);
const _: () = assert!(BOWL_RADIUS < BASIN_RADIUS - BASIN_RIM);
const _: () = assert!(WATER_BELOW < BASIN_HEIGHT);
// And a column's shaft has to stand on its plinth.
const _: () = assert!(COLUMN_SHAFT_RADIUS * 2.0 < COLUMN_PLINTH.x);
const _: () = assert!(COLUMN_PLINTH.x < COLUMN_BASE);

#[derive(Resource)]
pub struct MonumentKit {
    step: Handle<Mesh>,
    plinth: Handle<Mesh>,
    basin: Handle<Mesh>,
    water: (Handle<Mesh>, Handle<StandardMaterial>),
    shaft: Handle<Mesh>,
    bowl: Handle<Mesh>,
    column: Handle<Mesh>,
    capital: Handle<Mesh>,
    figure: Handle<Mesh>,
    gilt_figure: Handle<Mesh>,
    stone: Handle<StandardMaterial>,
    weathered: Handle<StandardMaterial>,
    bronze: Handle<StandardMaterial>,
    gilt: Handle<StandardMaterial>,
    plaque: Handle<Mesh>,
    /// One painted plate per register entry, in the register's own order, so a
    /// monument finds its own by index and nothing has to hash a string at
    /// spawn time.
    plates: Vec<Handle<StandardMaterial>>,
}

/// Paints one plate: bronze field, raised rim, the name in a lighter alloy.
/// The park monuments' plaque at a single line's proportions — a fountain is
/// called one thing and does not need a subtitle.
fn plate_texture(label: &str) -> Image {
    let label = encode(label);
    painted_rect(384, 128, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        if !(0.03..=0.97).contains(&u) || !(0.08..=0.92).contains(&v) {
            return [64, 46, 28, 255];
        }
        if text_band(&label, u, (v - 0.28) / 0.44) {
            [226, 202, 152, 255]
        } else {
            [110, 82, 48, 255]
        }
    })
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> MonumentKit {
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.58, 0.57, 0.54),
        perceptual_roughness: 0.9,
        ..default()
    });
    let weathered = materials.add(StandardMaterial {
        base_color: Color::srgb(0.46, 0.47, 0.45),
        perceptual_roughness: 0.96,
        ..default()
    });
    // Bronze that has been outside. A fountain figure is green, not brown:
    // the brown is what it looked like the year it was cast.
    let bronze = materials.add(StandardMaterial {
        base_color: Color::srgb(0.31, 0.42, 0.36),
        perceptual_roughness: 0.55,
        metallic: 0.55,
        ..default()
    });
    let gilt = materials.add(StandardMaterial {
        base_color: Color::srgb(0.80, 0.66, 0.28),
        perceptual_roughness: 0.28,
        metallic: 0.9,
        ..default()
    });
    // The same blue-green the canal is drawn in, so the town's water is one
    // colour. Held in a basin rather than laid on the ground, so this one owes
    // `world::layer` nothing: its surface is 480 mm up, and the whole ground
    // stack is under forty.
    let water = materials.add(StandardMaterial {
        base_color: Color::srgba(0.16, 0.34, 0.42, 0.92),
        perceptual_roughness: 0.1,
        metallic: 0.35,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    MonumentKit {
        step: meshes.add(Cuboid::new(1.0, COLUMN_STEP_HEIGHT, 1.0)),
        plinth: meshes.add(Cuboid::new(
            COLUMN_PLINTH.x,
            COLUMN_PLINTH.y,
            COLUMN_PLINTH.z,
        )),
        basin: meshes.add(super::props::cylinder(BASIN_RADIUS, BASIN_HEIGHT)),
        water: (
            meshes.add(super::props::cylinder(
                BASIN_RADIUS - BASIN_RIM,
                // Thin rather than flat: a disc drawn at one height is a
                // surface that can tie with the basin's own top face at a
                // grazing angle. Ten centimetres of water has two faces and
                // neither of them is anybody else's.
                0.1,
            )),
            water,
        ),
        shaft: meshes.add(super::props::cylinder(SHAFT_RADIUS, SHAFT_HEIGHT)),
        bowl: meshes.add(super::props::cylinder(BOWL_RADIUS, BOWL_HEIGHT)),
        column: meshes.add(super::props::cylinder(
            COLUMN_SHAFT_RADIUS,
            COLUMN_SHAFT_HEIGHT,
        )),
        capital: meshes.add(Cuboid::new(
            COLUMN_CAPITAL.x,
            COLUMN_CAPITAL.y,
            COLUMN_CAPITAL.z,
        )),
        // Three subdivisions for the same reason `world::statues` gives its
        // granite sphere three: at two the facets catch the sun one at a time
        // and a figure reads as a dice.
        figure: meshes.add(
            Sphere::new(FIGURE_RADIUS)
                .mesh()
                .ico(3)
                .expect("an icosphere at three subdivisions"),
        ),
        gilt_figure: meshes.add(
            Sphere::new(COLUMN_FIGURE_RADIUS)
                .mesh()
                .ico(3)
                .expect("an icosphere at three subdivisions"),
        ),
        stone,
        weathered,
        bronze,
        gilt,
        plaque: meshes.add(Rectangle::new(PLAQUE.x, PLAQUE.y)),
        plates: LISTED
            .iter()
            .map(|listed| {
                materials.add(StandardMaterial {
                    base_color_texture: Some(images.add(plate_texture(listed.label))),
                    perceptual_roughness: 0.55,
                    metallic: 0.35,
                    ..default()
                })
            })
            .collect(),
    }
}

/// One monument, resolved to a place on the ground.
#[derive(Clone, Copy, Debug)]
pub struct Standing {
    pub at: Vec2,
    /// Which way the plaque faces: back towards the street it was set off.
    pub yaw: f32,
    pub piece: Piece,
    /// Which register entry this is, and so which painted plate it wears.
    pub listing: usize,
}

/// Every monument this town keeps, in the order the register lists them.
///
/// Empty for a town with no entries, which is every generated city — the
/// register is about places that exist.
#[derive(Resource, Default)]
pub struct TownMonuments(pub Vec<Standing>);

/// How far off the kerb line a monument is first tried, how far it may walk
/// looking for room, and in what steps.
///
/// It starts *outside* its own reach plus the street's half width, because the
/// question `Corridors::in_the_road` answers is about the carriageway and the
/// pavement is exactly where the fountain goes. `WALK` is then how far it may
/// search both across the street and along it: a square's room is rarely
/// opposite its own middle, and a gap between two terraces twelve metres up the
/// road is a place a fountain goes.
const CLEAR: f32 = 0.6;
const WALK: f32 = 16.0;
const WALK_STEP: f32 = 0.5;

/// The broadest part of the street that wears this name.
///
/// A monument stands where the square is widest, which is both what a square
/// *is* — the place the street opens out — and the only part of it with room
/// for one. So this does not walk the name's edges into a path and take a
/// fraction along it, which was the first attempt and put the market fountain
/// on a four-metre side arm of the Altstadt sixty metres from the market: it
/// takes the widest edge outright, breaking a tie on length and then on
/// `EdgeId`, so the answer is one edge and the same edge every time.
///
/// Returns where its middle is, which way it runs, and how wide it is.
fn widest_run(graph: &RoadGraph, signs: &Signposts, name: &str) -> Option<(Vec2, Vec2, f32)> {
    let index = signs.names.iter().position(|known| known == name)?;
    let mut best: Option<(f32, f32, u32)> = None;
    let mut found = None;
    for (id, wearing) in signs.per_edge.iter().enumerate() {
        if *wearing != Some(index) || id >= graph.edge_count() {
            continue;
        }
        let edge = graph.edge(super::roadgraph::EdgeId(id as u32));
        let (a, b) = (graph.node(edge.a).pos, graph.node(edge.b).pos);
        let length = a.distance(b);
        if length < 1.0 {
            continue;
        }
        // Widest, then longest, then lowest id: three keys, because a market
        // band is forty segments of exactly one width and a tie broken by
        // whichever came first in a hash order is a monument that moves when
        // the graph is rebuilt.
        let key = (edge.width, length, u32::MAX - id as u32);
        if best.is_none_or(|had| key > had) {
            best = Some(key);
            found = Some((a.midpoint(b), (b - a).normalize_or_zero(), edge.width));
        }
    }
    found
}

/// Is there room for something `reach` across standing here?
///
/// Two questions, both already answered for the whole town by somebody else:
/// is it on a carriageway (`Corridors`), and is it inside a building
/// (`streetside::room_for`). The second has to be the building's *rotated* box
/// rather than the block's — a block's `area` is the circumscribed square round
/// a building standing at an angle, which for a twenty-metre terrace is eight
/// metres of pavement the block does not actually occupy, and testing against
/// it rejected every square in Landshut.
fn has_room(layout: &CityLayout, corridors: &Corridors, at: Vec2, reach: f32) -> bool {
    !corridors.in_the_road(at, reach) && super::streetside::room_for(layout, at, reach)
}

/// Resolves the register against a town.
///
/// Pure: the same atlas, graph and layout give the same list forever, which is
/// what the chunk that respawns these needs. A monument with no room is left
/// out and said so — the one thing it must not do is stand in the road, which
/// is what every other spawner beside a street is careful about too.
pub fn place(
    town: &str,
    layout: &CityLayout,
    signs: &Signposts,
    corridors: &Corridors,
) -> TownMonuments {
    let mut out = Vec::new();
    for (listing, listed) in LISTED.iter().enumerate() {
        if listed.town != town {
            continue;
        }
        let Some((centre, direction, width)) = widest_run(&layout.graph, signs, listed.on) else {
            warn!(
                "{town}: no street called {:?} for the {}",
                listed.on, listed.label
            );
            continue;
        };
        let across = Vec2::new(-direction.y, direction.x);
        let reach = listed.piece.reach();
        let start = width * 0.5 + reach + CLEAR;
        // Every place worth trying, nearest first. Across the street and along
        // it, because a square's room is rarely opposite its own middle: the
        // Narrenbrunnen's own square has terraces hard against both kerbs at
        // the point the widest arm is measured from, and the gap it fits in is
        // eight metres up the road.
        //
        // Sorted by how far the monument would be moved, with the sideways
        // step counted at its face value and the shift along the street at
        // half — a fountain a little further up the same square is barely
        // moved; one on the far pavement is somewhere else. Ties are broken by
        // the order the candidates were generated in, which is fixed, so the
        // answer is the same every run.
        let mut tries: Vec<(f32, Vec2, f32)> = Vec::new();
        let steps = (WALK / WALK_STEP) as i32;
        for out in 0..=steps {
            for shift in -steps..=steps {
                for side in [1.0f32, -1.0] {
                    let offset = start + out as f32 * WALK_STEP;
                    let along = shift as f32 * WALK_STEP;
                    let cost = offset - start + along.abs() * 0.5;
                    let at = centre + across * (side * offset) + direction * along;
                    // Facing back at the street it was set off: the plaque is
                    // meant to be read by somebody walking past it.
                    let facing = -across * side;
                    tries.push((cost, at, facing.y.atan2(facing.x)));
                }
            }
        }
        tries.sort_by(|a, b| a.0.total_cmp(&b.0));
        let placed = tries
            .into_iter()
            .find(|(_, at, _)| has_room(layout, corridors, *at, reach))
            .map(|(_, at, yaw)| (at, yaw));
        let Some((at, yaw)) = placed else {
            warn!(
                "{town}: no room on {} for the {}; left out",
                listed.on, listed.label
            );
            continue;
        };
        out.push(Standing {
            at,
            // `atan2(x, z)` is the game's yaw; the maths above produced an
            // angle measured off +X, which is a quarter turn from it.
            yaw: std::f32::consts::FRAC_PI_2 - yaw,
            piece: listed.piece,
            listing,
        });
    }
    TownMonuments(out)
}

/// Raises every monument that falls inside this chunk.
///
/// Iterated rather than indexed: a town keeps a handful of these, and a
/// hash-map from chunk to monument would be more plumbing than the whole
/// register is.
pub fn spawn(commands: &mut Commands, kit: &MonumentKit, monuments: &TownMonuments, chunk: IVec2) {
    for standing in &monuments.0 {
        if super::streaming::chunk_of(standing.at) != chunk {
            continue;
        }
        raise(commands, kit, standing, chunk);
    }
}

fn raise(commands: &mut Commands, kit: &MonumentKit, standing: &Standing, chunk: IVec2) {
    let facing = Quat::from_rotation_y(standing.yaw);
    let range = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: (RANGE * 0.9)..RANGE,
        use_aabb: false,
    };
    let at = standing.at;
    // The squares these stand on are inside a street corridor, where
    // `Terrain::height` is held at exactly zero — the same thing every other
    // spawner beside a street relies on. See the trap in CLAUDE.md.
    let ground = SIDEWALK_HEIGHT;
    let plate = kit.plates.get(standing.listing).cloned();

    match standing.piece {
        Piece::Fountain => {
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.basin.clone()),
                MeshMaterial3d(kit.weathered.clone()),
                Transform::from_xyz(at.x, ground + BASIN_HEIGHT * 0.5, at.y),
                RigidBody::Static,
                Collider::cylinder(BASIN_RADIUS, BASIN_HEIGHT),
                range.clone(),
            ));
            let (water_mesh, water_material) = &kit.water;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(water_mesh.clone()),
                MeshMaterial3d(water_material.clone()),
                Transform::from_xyz(at.x, ground + BASIN_HEIGHT - WATER_BELOW, at.y),
                range.clone(),
                // No collider: the water is scenery, and a flummi that lands
                // in a fountain should land in the basin it can see the bottom
                // of. And no shadow — a disc of water casting one on the stone
                // two hand-widths under it is a black ring.
                NotShadowCaster,
            ));
            let shaft_at = ground + BASIN_HEIGHT + SHAFT_HEIGHT * 0.5 - WATER_BELOW;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.shaft.clone()),
                MeshMaterial3d(kit.stone.clone()),
                Transform::from_xyz(at.x, shaft_at, at.y),
                range.clone(),
            ));
            let bowl_at = shaft_at + SHAFT_HEIGHT * 0.5 + BOWL_HEIGHT * 0.5;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.bowl.clone()),
                MeshMaterial3d(kit.stone.clone()),
                Transform::from_xyz(at.x, bowl_at, at.y),
                range.clone(),
            ));
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.figure.clone()),
                MeshMaterial3d(kit.bronze.clone()),
                Transform::from_xyz(at.x, bowl_at + BOWL_HEIGHT * 0.5 + FIGURE_RADIUS, at.y),
                range.clone(),
            ));
            if let Some(plate) = plate {
                let forward = facing * Vec3::Z;
                commands.spawn((
                    ChunkOf(chunk),
                    Mesh3d(kit.plaque.clone()),
                    MeshMaterial3d(plate),
                    Transform::from_translation(
                        Vec3::new(at.x, ground + BASIN_HEIGHT * 0.6, at.y)
                            + forward * (BASIN_RADIUS + 0.02),
                    )
                    .with_rotation(facing),
                    range,
                    NotShadowCaster,
                ));
            }
        }
        Piece::Column => {
            // The steps, widest at the bottom. Three of them: two reads as a
            // kerb round a pole and four is a monument to a bigger town.
            let mut top = ground;
            for step in 0..COLUMN_STEPS {
                let side = COLUMN_BASE - step as f32 * (COLUMN_BASE - COLUMN_PLINTH.x) * 0.42;
                commands.spawn((
                    ChunkOf(chunk),
                    Mesh3d(kit.step.clone()),
                    MeshMaterial3d(kit.weathered.clone()),
                    Transform::from_xyz(at.x, top + COLUMN_STEP_HEIGHT * 0.5, at.y)
                        .with_rotation(facing)
                        .with_scale(Vec3::new(side, 1.0, side)),
                    RigidBody::Static,
                    Collider::cuboid(side, COLUMN_STEP_HEIGHT, side),
                    range.clone(),
                ));
                top += COLUMN_STEP_HEIGHT;
            }
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.plinth.clone()),
                MeshMaterial3d(kit.stone.clone()),
                Transform::from_xyz(at.x, top + COLUMN_PLINTH.y * 0.5, at.y).with_rotation(facing),
                RigidBody::Static,
                Collider::cuboid(COLUMN_PLINTH.x, COLUMN_PLINTH.y, COLUMN_PLINTH.z),
                range.clone(),
            ));
            if let Some(plate) = plate {
                let forward = facing * Vec3::Z;
                commands.spawn((
                    ChunkOf(chunk),
                    Mesh3d(kit.plaque.clone()),
                    MeshMaterial3d(plate),
                    Transform::from_translation(
                        Vec3::new(at.x, top + COLUMN_PLINTH.y * 0.55, at.y)
                            + forward * (COLUMN_PLINTH.z * 0.5 + 0.02),
                    )
                    .with_rotation(facing),
                    range.clone(),
                    NotShadowCaster,
                ));
            }
            top += COLUMN_PLINTH.y;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.column.clone()),
                MeshMaterial3d(kit.stone.clone()),
                Transform::from_xyz(at.x, top + COLUMN_SHAFT_HEIGHT * 0.5, at.y),
                range.clone(),
            ));
            top += COLUMN_SHAFT_HEIGHT;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.capital.clone()),
                MeshMaterial3d(kit.stone.clone()),
                Transform::from_xyz(at.x, top + COLUMN_CAPITAL.y * 0.5, at.y).with_rotation(facing),
                range.clone(),
            ));
            top += COLUMN_CAPITAL.y;
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(kit.gilt_figure.clone()),
                MeshMaterial3d(kit.gilt.clone()),
                Transform::from_xyz(at.x, top + COLUMN_FIGURE_RADIUS, at.y),
                range,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entry names a town, a monument and a street. A blank here is a
    /// monument that silently never appears.
    #[test]
    fn every_listing_is_well_formed() {
        for listed in LISTED {
            assert!(!listed.town.is_empty(), "an entry with no town");
            assert!(!listed.label.is_empty(), "an entry with no name");
            assert!(!listed.on.is_empty(), "{} stands on nothing", listed.label);
        }
    }

    /// Every Landshut entry finds its square, finds room on it, and is not in
    /// the road.
    ///
    /// The whole register is a set of strings matched against a baked file, so
    /// the failure mode it has is the silent one: a spelling the atlas does not
    /// use, or a square with houses right up to both kerbs, and the monument
    /// simply never appears. This is the test that notices — and, like the
    /// atlas's own, it fails rather than returns when the file is there and
    /// will not load.
    #[test]
    fn landshut_keeps_every_monument_it_is_listed_for() {
        use crate::core::config::CityStyle;
        let path = crate::core::assets::root().join("cities/landshut.ron");
        let town = super::super::atlas::load("landshut");
        assert!(
            town.is_some() || !path.exists(),
            "{} is on disk and does not load as an atlas",
            path.display()
        );
        let Some(town) = town else { return };

        let (mut layout, signs) = super::super::atlas::layout(&town, 1, 1_000.0);
        let real = super::super::atlas::footprints(
            &town,
            &layout.graph,
            1,
            1_000.0,
            CityStyle::Landshuepf,
        );
        let (blocks, _) = super::super::streetside::lots(&layout, 1, CityStyle::Landshuepf, real);
        layout.blocks = blocks;
        let corridors = Corridors::build(&layout);
        let placed = place(&town.name, &layout, &signs, &corridors);

        let wanted = LISTED
            .iter()
            .filter(|listed| listed.town == town.name)
            .count();
        assert_eq!(
            placed.0.len(),
            wanted,
            "{} of {wanted} of Landshut's monuments found a place",
            placed.0.len()
        );
        for standing in &placed.0 {
            let listed = &LISTED[standing.listing];
            assert!(
                !corridors.in_the_road(standing.at, standing.piece.reach()),
                "the {} stands on the carriageway of {}",
                listed.label,
                listed.on
            );
            assert!(
                standing.at.is_finite(),
                "the {} stands at {:?}",
                listed.label,
                standing.at
            );
        }
        // And no two of them in the same place, which is what a register with
        // the same street twice in it would quietly produce.
        for (a, one) in placed.0.iter().enumerate() {
            for other in &placed.0[a + 1..] {
                assert!(
                    one.at.distance(other.at) > BASIN_RADIUS * 2.0,
                    "{} and {} stand on top of each other at {:?}",
                    LISTED[one.listing].label,
                    LISTED[other.listing].label,
                    one.at
                );
            }
        }
    }

    /// The plate painter is handed every label, so every label has to survive
    /// `encode` — which is where an umlaut either works or turns into a space.
    #[test]
    fn every_label_paints() {
        for listed in LISTED {
            let painted = encode(listed.label);
            assert_eq!(
                painted.len(),
                listed.label.chars().count(),
                "{} does not encode one byte per character",
                listed.label
            );
            assert!(
                !painted.iter().all(|byte| *byte == b' '),
                "{} paints as a blank plate",
                listed.label
            );
        }
    }
}
