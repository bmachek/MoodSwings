//! What stands on the roofs.
//!
//! Every building in this city was two boxes: a wall and a capping slab. From
//! the pavement that is nearly enough, because you cannot see a roof from the
//! pavement. From anywhere with height it was the single most damning view in
//! the game — four thousand identical white rectangles, a circuit board rather
//! than a skyline.
//!
//! Real roofs are the untidiest surface a city has. Nobody designs them and
//! nobody looks at them, so they accumulate: plant, extract fans, water tanks,
//! the stair head that had to come up somewhere, an aerial nobody has taken
//! down. That untidiness is most of what makes a skyline read as built.
//!
//! Two separate levers, and they are worth keeping separate because they cost
//! completely different things:
//!
//! * **The parapet varies per building.** Height, overhang and inset are drawn
//!   from the building's own seed. This is *free* — the slab was already its
//!   own entity with its own transform — and it is the half that reads from a
//!   kilometre up, where an air-conditioning unit is a fraction of a pixel.
//! * **Clutter is placed on the roof deck.** This costs an entity and a draw
//!   call each, so it carries a [`VisibilityRange`] and stops being drawn well
//!   before it stops being resolvable. It is the half that reads from a window,
//!   a rooftop, or a helicopter.
//!
//! Placement is on a coarse grid with jitter inside each cell rather than by
//! rejection sampling. Two reasons: a grid cannot produce two pieces occupying
//! the same volume no matter what the RNG does, and it terminates in a fixed
//! number of steps rather than in however many tries overlap-testing needs.
//!
//! The seed comes from the footprint, not from a draw-order counter. Chunks are
//! regenerated whenever the player walks back into them, and a counter would
//! re-roll a different roof each time.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::citygen::Rect;
use super::texture::FacadeClass;

/// How far inside the parapet clutter is allowed to stand, in metres.
///
/// Nothing should poke out over the edge: seen from the street, a box
/// overhanging a parapet is unmistakably wrong in a way that a box sitting
/// safely behind one never is.
const MARGIN: f32 = 1.4;

/// Coarse grid cell, in metres. Also the largest piece that can be placed,
/// because a piece is jittered within one cell and must not leave it.
///
/// Sized from the plant rather than the other way round. The first pass used
/// four and a half metres and the pieces that fitted in it came out too small
/// to read from a neighbouring rooftop — a roofscape of gravel rather than of
/// machinery. Real air handling is two to four metres across.
const CELL: f32 = 7.0;

/// Distance at which clutter stops being drawn, before `lod_scale`.
///
/// Chosen from the size of the pieces rather than from a frame budget: below
/// about a metre on screen these read as noise on the roofline, and the
/// parapet variation carries the silhouette from there out.
pub const CLUTTER_RANGE: f32 = 420.0;

/// The radius each round piece is *typically* drawn at, in metres.
///
/// These decide nothing but how many sides the piece's shared unit cylinder
/// gets — the mesh is unit and [`Placement::size`] scales it — so they are the
/// midpoints of the ranges [`Piece::size`] draws from rather than a size
/// anything actually is. Keep them in step with those ranges: a tank that
/// quietly grew to three metres would still be drawn with a tank's sixteen
/// sides, which is this pass's mistake made again in the other direction.
const VENT_RADIUS: f32 = 0.35;
const TANK_RADIUS: f32 = 1.2;
const AERIAL_RADIUS: f32 = 0.06;

/// A unit cylinder — radius 1, height 2 — at the resolution `radius` earns.
///
/// The radius is passed separately because the mesh cannot know it: every
/// round piece on every roof shares one of these and is scaled to size by its
/// own transform.
fn unit_cylinder(radius: f32) -> Mesh {
    Cylinder::new(1.0, 2.0)
        .mesh()
        .resolution(super::props::cylinder_sides(radius))
        .build()
}

/// One thing that ended up on a roof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    /// Air handling. Boxy, and the most common thing up there by a distance.
    Plant,
    /// An extract stack.
    Vent,
    /// A water tank on short legs.
    Tank,
    /// The stair core, which has to surface somewhere.
    StairHead,
    /// A mast. Tall, thin, and the only thing here that breaks the roofline
    /// against the sky.
    Aerial,
}

impl Piece {
    /// Whether this piece is worth a place in the shadow map.
    ///
    /// A vent stack is a quarter of a metre across and a mast is five
    /// centimetres. Both are finer than a shadow-map texel at any distance the
    /// roof is visible from, so what they contribute is aliasing along the edge
    /// of somebody else's shadow rather than a shadow of their own. The boxy
    /// pieces stay: a stair head at low sun lays a shadow right across the deck,
    /// and that is most of what makes a roof read as a surface.
    fn casts_shadow(self) -> bool {
        matches!(self, Self::Plant | Self::Tank | Self::StairHead)
    }
}

impl Piece {
    /// Half-extents, drawn per placement so no two are quite the same size.
    fn size(self, rng: &mut ChaCha8Rng) -> Vec3 {
        match self {
            Self::Plant => Vec3::new(
                rng.random_range(1.1..2.2),
                rng.random_range(0.7..1.3),
                rng.random_range(0.8..1.6),
            ),
            Self::Vent => {
                let radius = rng.random_range(0.22..0.48);
                Vec3::new(radius, rng.random_range(0.5..1.1), radius)
            }
            Self::Tank => {
                let radius = rng.random_range(0.9..1.5);
                Vec3::new(radius, rng.random_range(1.4..2.4), radius)
            }
            Self::StairHead => Vec3::new(
                rng.random_range(2.0..3.2),
                rng.random_range(1.6..2.4),
                rng.random_range(1.8..2.8),
            ),
            Self::Aerial => Vec3::new(0.06, rng.random_range(2.0..4.2), 0.06),
        }
    }

    /// What tends to be on which kind of building, as weights.
    ///
    /// A house has a vent and an aerial and nothing else, because a house does
    /// not have plant. A tower has all of it, because a tower has to put its
    /// lifts, its tanks and its air handling somewhere and the roof is where
    /// they go.
    fn table(class: FacadeClass) -> &'static [(Piece, u32)] {
        match class {
            FacadeClass::House => &[(Piece::Vent, 6), (Piece::Aerial, 4)],
            FacadeClass::Lowrise => &[(Piece::Vent, 5), (Piece::Plant, 4), (Piece::Aerial, 2)],
            FacadeClass::Midrise => &[
                (Piece::Plant, 6),
                (Piece::Vent, 4),
                (Piece::StairHead, 3),
                (Piece::Tank, 2),
                (Piece::Aerial, 1),
            ],
            FacadeClass::Tower => &[
                (Piece::Plant, 7),
                (Piece::StairHead, 4),
                (Piece::Tank, 4),
                (Piece::Vent, 3),
                (Piece::Aerial, 2),
            ],
        }
    }

    fn pick(class: FacadeClass, rng: &mut ChaCha8Rng) -> Piece {
        let table = Self::table(class);
        let total: u32 = table.iter().map(|(_, weight)| weight).sum();
        let mut roll = rng.random_range(0..total);
        for (piece, weight) in table {
            if roll < *weight {
                return *piece;
            }
            roll -= weight;
        }
        table[0].0
    }
}

/// Which of the kit's finishes a piece wears.
///
/// Three, and no more, because each one is a material and a material is a
/// batch. Three is enough to stop a roof reading as one moulding — which is
/// what the first pass looked like, every box the same pale grey, closer to
/// gravel than to machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Galvanised sheet: light, slightly metallic. New-ish plant.
    Steel,
    /// Painted render. Stair heads and lift overruns are built, not delivered.
    Housing,
    /// Weathered. Everything up here has been rained on for twenty years.
    Weathered,
}

/// One piece, positioned relative to the centre of the roof deck.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub piece: Piece,
    /// Offset in the footprint plane, metres from the roof centre.
    pub offset: Vec2,
    /// Half-extents. Y is half the height, so the piece sits at `offset` with
    /// its base on the deck when raised by `size.y`.
    pub size: Vec3,
    pub yaw: f32,
    pub tone: Tone,
}

/// How the capping slab is shaped for one building.
///
/// Free variation: the slab is already an entity with its own transform, so
/// making these three numbers depend on the building costs nothing at all and
/// is what stops a skyline reading as one extrusion.
#[derive(Debug, Clone, Copy)]
pub struct Parapet {
    pub thickness: f32,
    pub overhang: f32,
}

/// A stable key for one building.
///
/// Derived from where it stands rather than from when it was spawned, because
/// a chunk is regenerated every time it is streamed back in and anything
/// order-dependent would re-roll a different roof each time.
pub fn seed_for(world_seed: u64, footprint: Rect) -> u64 {
    let center = footprint.center();
    // Quantised to a centimetre before hashing, so a float that comes back from
    // generation one ulp different does not produce a different roof.
    let x = (center.x * 100.0).round() as i64 as u64;
    let z = (center.y * 100.0).round() as i64 as u64;
    crate::core::rng::stream_for_chunk(
        world_seed,
        crate::core::rng::stream::BUILDINGS,
        (x as i32, z as i32),
    )
    .random::<u64>()
        ^ (x << 20)
        ^ z
}

/// Chooses the capping slab's proportions.
pub fn parapet(seed: u64, class: FacadeClass) -> Parapet {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x9017);

    // Taller buildings get deeper parapets, because they have to: the higher
    // the roof the more of it is plant, and the more there is to hide.
    let (low, high) = match class {
        FacadeClass::House => (0.30, 0.55),
        FacadeClass::Lowrise => (0.40, 0.80),
        FacadeClass::Midrise => (0.55, 1.15),
        FacadeClass::Tower => (0.70, 1.60),
    };

    Parapet {
        thickness: rng.random_range(low..high),
        overhang: rng.random_range(0.10..0.34),
    }
}

/// Lays out the clutter on one roof.
///
/// Returns an empty plan rather than refusing when the roof is too small to
/// hold anything — a two-metre outbuilding having a bare roof is correct, not
/// an error.
pub fn plan(seed: u64, footprint: Rect, class: FacadeClass) -> Vec<Placement> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let deck = footprint.size() - Vec2::splat(MARGIN * 2.0);
    if deck.x < CELL || deck.y < CELL {
        return Vec::new();
    }

    let cells = UVec2::new((deck.x / CELL) as u32, (deck.y / CELL) as u32);
    let slots = cells.x * cells.y;

    // Occupancy, not a count: a big roof carries more than a small one because
    // it has more roof, and a house's roof stays nearly bare however large it
    // is. Capped so a tower block does not turn into a warehouse of boxes.
    let occupancy = match class {
        FacadeClass::House => 0.10,
        FacadeClass::Lowrise => 0.18,
        FacadeClass::Midrise => 0.26,
        FacadeClass::Tower => 0.34,
    };
    let wanted = ((slots as f32 * occupancy).round() as u32).min(14);
    if wanted == 0 {
        return Vec::new();
    }

    // Walk every cell once and take each with probability `wanted / slots`.
    // Sampling without replacement, so a cell can never be chosen twice and no
    // two pieces can share a volume — which is the whole reason for the grid.
    let mut placements = Vec::with_capacity(wanted as usize);
    let mut remaining = wanted;
    let mut left = slots;

    for index in 0..slots {
        if remaining == 0 {
            break;
        }
        if rng.random_range(0..left) >= remaining {
            left -= 1;
            continue;
        }
        remaining -= 1;
        left -= 1;

        let cell = UVec2::new(index % cells.x, index / cells.x);
        let piece = Piece::pick(class, &mut rng);
        let size = piece.size(&mut rng);

        // Centre of this cell, relative to the middle of the deck.
        let origin = Vec2::new(
            (cell.x as f32 + 0.5 - cells.x as f32 * 0.5) * CELL,
            (cell.y as f32 + 0.5 - cells.y as f32 * 0.5) * CELL,
        );
        // Jitter, bounded so the piece stays inside its own cell.
        let slack = (Vec2::splat(CELL * 0.5) - size.xz()).max(Vec2::ZERO);
        let offset = origin
            + Vec2::new(
                rng.random_range(-1.0..1.0) * slack.x,
                rng.random_range(-1.0..1.0) * slack.y,
            );

        // A stair head is built and rendered; everything else is delivered on a
        // lorry and then left outside. Weathering is the common case rather
        // than the exception, which is what stops the roofscape reading as new.
        let tone = match piece {
            Piece::StairHead => Tone::Housing,
            _ if rng.random_range(0.0..1.0) < 0.45 => Tone::Weathered,
            _ => Tone::Steel,
        };

        placements.push(Placement {
            piece,
            offset,
            size,
            tone,
            // Plant sits square to the building it serves; masts have no facing
            // worth speaking of, so they get a free angle.
            yaw: match piece {
                Piece::Aerial => rng.random_range(0.0..std::f32::consts::TAU),
                _ => rng.random_range(-0.12..0.12),
            },
        });
    }

    placements
}

/// How much narrower a plant unit is at the lid than at the deck.
///
/// A plain [`Cuboid`] made every air-handling unit in the city a perfect
/// extrusion, and a roofscape of perfect extrusions is most of what reads as
/// "generated" from a window three streets away: real plant is a sheet-metal
/// case whose panels are folded in at the top so rain runs off the lid rather
/// than down the seams. A seventh is enough that the near vertical edge and
/// the far one diverge on screen, which is the whole cue — and it is free,
/// because a frustum is still six quads and twelve triangles, exactly what the
/// cuboid cost.
///
/// A tenth was tried first and does not read at all past about twenty metres;
/// a quarter reads as a skip rather than as machinery.
const PLANT_TAPER: f32 = 0.14;

/// A unit box with its lid drawn in by `taper` on all four sides.
///
/// Built at half-extents of one in every axis, exactly like the [`Cuboid`] it
/// replaces, so a [`Placement`]'s half-extents still scale it directly and the
/// whole city's plant is still one mesh.
///
/// Flat-shaded with four vertices per face rather than eight shared corners:
/// the fold where a panel meets the lid is a hard edge on a real steel case,
/// and averaged corner normals would round it into something inflatable. That
/// costs 24 vertices where a smooth frustum needs 8, and no triangles at all.
fn tapered_box(taper: f32) -> Mesh {
    let lid_scale = 1.0 - taper;
    // The deck ring, and the lid ring directly above it. Both run the same way
    // round, so the sides can be walked as consecutive pairs.
    let deck = [
        Vec3::new(-1.0, -1.0, -1.0),
        Vec3::new(-1.0, -1.0, 1.0),
        Vec3::new(1.0, -1.0, 1.0),
        Vec3::new(1.0, -1.0, -1.0),
    ];
    let lid = deck.map(|c| Vec3::new(c.x * lid_scale, 1.0, c.z * lid_scale));

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(24);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(24);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(24);
    let mut indices: Vec<u32> = Vec::with_capacity(36);
    {
        // One planar quad, wound anticlockwise seen from outside. A tapered
        // side is still planar — its two horizontal edges are parallel, just
        // different lengths — so one normal per face is exact rather than an
        // approximation.
        let mut quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| {
            let normal = (b - a).cross(d - a).normalize();
            let first = positions.len() as u32;
            for (corner, uv) in [
                (a, [0.0, 1.0]),
                (b, [1.0, 1.0]),
                (c, [1.0, 0.0]),
                (d, [0.0, 0.0]),
            ] {
                positions.push(corner.to_array());
                normals.push(normal.to_array());
                uvs.push(uv);
            }
            indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
        };

        for i in 0..4 {
            let next = (i + 1) % 4;
            quad(deck[i], deck[next], lid[next], lid[i]);
        }
        quad(lid[0], lid[1], lid[2], lid[3]);
        // The underside is never seen — it is flat on the deck — but a mesh
        // with a hole in it shadows like a mesh with a hole in it.
        quad(deck[3], deck[2], deck[1], deck[0]);
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// The shared meshes and materials every roof draws from.
///
/// Five meshes and three materials for the whole city's roofscape, so however
/// much of it is resident it stays a handful of batches — the same bargain the
/// street furniture makes.
///
/// The round pieces were one shared unit cylinder at Bevy's default resolution
/// of 32, which is 124 triangles, and that one mesh had to serve a 1.2 m water
/// tank *and* a 6 cm aerial mast. Sized for the tank it was ruinous on the
/// mast; sized for the mast it would have facetted the tank. Splitting it into
/// three costs two more mesh handles — nothing, they are shared by the whole
/// city — and lets each take the resolution its own radius can show, via
/// [`super::props::cylinder_sides`].
#[derive(Resource)]
pub struct RoofKit {
    /// Air handling: a box with its lid drawn in, see [`tapered_box`].
    plant: Handle<Mesh>,
    /// The stair head, which stays a true box: it is a built structure with
    /// rendered walls, and a tapered stair core would read as a bunker.
    stair_head: Handle<Mesh>,
    vent: Handle<Mesh>,
    tank: Handle<Mesh>,
    aerial: Handle<Mesh>,
    /// Galvanised: light, rough, slightly metallic.
    steel: Handle<StandardMaterial>,
    /// Painted render, for stair heads and lift overruns.
    housing: Handle<StandardMaterial>,
    /// Twenty years of rain on painted steel.
    weathered: Handle<StandardMaterial>,
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> RoofKit {
    // Unit extents in every axis, so a placement's half-extents scale it
    // directly and one mesh serves every piece of its kind on every roof. The
    // three cylinders differ only in resolution: they are asked for at the
    // *typical* radius their `Piece::size` draws, because the mesh is unit and
    // the scale is what decides how big any one of them ends up.
    RoofKit {
        plant: meshes.add(tapered_box(PLANT_TAPER)),
        stair_head: meshes.add(Cuboid::new(2.0, 2.0, 2.0)),
        vent: meshes.add(unit_cylinder(VENT_RADIUS)),
        tank: meshes.add(unit_cylinder(TANK_RADIUS)),
        aerial: meshes.add(unit_cylinder(AERIAL_RADIUS)),
        steel: materials.add(StandardMaterial {
            base_color: Color::srgb(0.56, 0.57, 0.58),
            perceptual_roughness: 0.62,
            metallic: 0.45,
            ..default()
        }),
        housing: materials.add(StandardMaterial {
            base_color: Color::srgb(0.44, 0.43, 0.41),
            perceptual_roughness: 0.88,
            ..default()
        }),
        weathered: materials.add(StandardMaterial {
            base_color: Color::srgb(0.31, 0.27, 0.24),
            perceptual_roughness: 0.94,
            metallic: 0.20,
            ..default()
        }),
    }
}

impl RoofKit {
    fn mesh(&self, piece: Piece) -> &Handle<Mesh> {
        match piece {
            Piece::Plant => &self.plant,
            Piece::StairHead => &self.stair_head,
            Piece::Vent => &self.vent,
            Piece::Tank => &self.tank,
            Piece::Aerial => &self.aerial,
        }
    }

    fn material(&self, tone: Tone) -> &Handle<StandardMaterial> {
        match tone {
            Tone::Steel => &self.steel,
            Tone::Housing => &self.housing,
            Tone::Weathered => &self.weathered,
        }
    }
}

/// Puts one building's clutter on its roof deck.
///
/// `deck` is the world-space height the pieces stand on — the top of the
/// capping slab, so nothing is buried in it and nothing floats above it.
///
/// `yaw` turns the whole plan onto the building's own frame. A plan is laid out
/// in the footprint's axes, and for a building placed along a real street the
/// footprint *is* the site frame — so a deck left unturned had its tanks and
/// vent stacks marching along the map's north while the roof they stand on
/// runs along its street, which puts half of them over the edge. Zero for a
/// generated block, whose footprint is a rectangle on the map already.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    kit: &RoofKit,
    chunk: super::buildings::ChunkOf,
    center: Vec2,
    deck: f32,
    yaw: f32,
    plan: &[Placement],
    lod_scale: f32,
) {
    let draw = (CLUTTER_RANGE * lod_scale).max(1.0);
    // Fades over the last eighth rather than blinking out. The dither band is
    // what stops a roofline visibly shedding its furniture as the camera pulls
    // back, which is more noticeable than the furniture itself.
    let fade = draw * 0.88;

    let turn = Quat::from_rotation_y(yaw);
    for placement in plan {
        let offset = turn * Vec3::new(placement.offset.x, 0.0, placement.offset.y);
        let position = Vec3::new(
            center.x + offset.x,
            deck + placement.size.y,
            center.y + offset.z,
        );

        let mut piece = commands.spawn((
            chunk,
            Mesh3d(kit.mesh(placement.piece).clone()),
            MeshMaterial3d(kit.material(placement.tone).clone()),
            Transform::from_translation(position)
                .with_rotation(turn * Quat::from_rotation_y(placement.yaw))
                .with_scale(placement.size),
            VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin: fade..draw,
                use_aabb: false,
            },
        ));

        if !placement.piece.casts_shadow() {
            piece.insert(NotShadowCaster);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roof(width: f32, depth: f32) -> Rect {
        Rect {
            min: Vec2::new(-width * 0.5, -depth * 0.5),
            max: Vec2::new(width * 0.5, depth * 0.5),
        }
    }

    /// Chunks are regenerated every time the player walks back in. If this
    /// fails, roofs reshuffle as you walk down a street and back up it.
    #[test]
    fn the_same_building_gets_the_same_roof_every_time() {
        let footprint = roof(24.0, 18.0);
        let seed = seed_for(0xA17E_5EED, footprint);
        assert_eq!(seed, seed_for(0xA17E_5EED, footprint));

        let first = plan(seed, footprint, FacadeClass::Midrise);
        let second = plan(seed, footprint, FacadeClass::Midrise);
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            assert_eq!(a.piece, b.piece);
            assert_eq!(a.offset, b.offset);
            assert_eq!(a.size, b.size);
        }
    }

    #[test]
    fn two_buildings_in_different_places_get_different_roofs() {
        let a = seed_for(1, roof(30.0, 30.0));
        let b = seed_for(
            1,
            Rect {
                min: Vec2::new(40.0, 40.0),
                max: Vec2::new(70.0, 70.0),
            },
        );
        assert_ne!(a, b);
    }

    /// A box overhanging a parapet is the one placement error that is obvious
    /// from the street, so it gets its own test.
    #[test]
    fn nothing_stands_over_the_parapet() {
        for class in FacadeClass::ALL {
            for (w, d) in [(12.0, 12.0), (24.0, 18.0), (60.0, 45.0), (18.0, 90.0)] {
                let footprint = roof(w, d);
                let half = footprint.size() * 0.5;
                for placement in plan(seed_for(9, footprint), footprint, class) {
                    let reach = placement.offset.abs() + placement.size.xz();
                    assert!(
                        reach.x <= half.x && reach.y <= half.y,
                        "{class:?} on {w}x{d}: {placement:?} reaches {reach:?} of {half:?}"
                    );
                }
            }
        }
    }

    /// The grid exists so this cannot happen. If the jitter bound is ever
    /// loosened, this is what catches it.
    #[test]
    fn no_two_pieces_occupy_the_same_space() {
        for class in FacadeClass::ALL {
            for seed in 0..40u64 {
                let footprint = roof(40.0, 32.0);
                let placements = plan(seed, footprint, class);
                for (i, a) in placements.iter().enumerate() {
                    for b in &placements[i + 1..] {
                        let gap = (a.offset - b.offset).abs();
                        let touching = a.size.xz() + b.size.xz();
                        assert!(
                            gap.x >= touching.x || gap.y >= touching.y,
                            "{class:?} seed {seed}: {a:?} overlaps {b:?}"
                        );
                    }
                }
            }
        }
    }

    /// A shed does not have plant on it, and trying to put a stair head on a
    /// three-metre roof is how pieces end up overhanging.
    #[test]
    fn a_roof_too_small_to_hold_anything_holds_nothing() {
        for class in FacadeClass::ALL {
            assert!(plan(seed_for(3, roof(4.0, 4.0)), roof(4.0, 4.0), class).is_empty());
            assert!(plan(seed_for(3, roof(7.0, 3.0)), roof(7.0, 3.0), class).is_empty());
        }
    }

    /// Three finishes exist so a roof does not read as one moulding. If a whole
    /// roofscape comes back in one tone, the material variety has been lost —
    /// which is exactly what the first pass looked like and is invisible in any
    /// test that only ever checks one piece.
    #[test]
    fn a_roofscape_is_not_all_one_finish() {
        let tones: Vec<Tone> = along_a_street(20)
            .into_iter()
            .flat_map(|f| plan(seed_for(6, f), f, FacadeClass::Midrise))
            .map(|placement| placement.tone)
            .collect();

        assert!(
            tones.len() > 10,
            "not enough pieces to judge: {}",
            tones.len()
        );
        assert!(tones.contains(&Tone::Steel), "nothing came out galvanised");
        assert!(
            tones.contains(&Tone::Weathered),
            "nothing came out weathered"
        );
    }

    /// Plant that reads as gravel from the next rooftop is plant nobody sees.
    /// These are the sizes real machinery comes in, and the first pass was
    /// under half of them.
    #[test]
    fn the_machinery_is_the_size_machinery_actually_is() {
        let footprint = roof(60.0, 48.0);
        for class in [FacadeClass::Midrise, FacadeClass::Tower] {
            for placement in plan(seed_for(8, footprint), footprint, class) {
                let full = placement.size * 2.0;
                match placement.piece {
                    Piece::Plant => {
                        assert!(full.x >= 2.0 && full.y >= 1.4, "plant came out {full:?}")
                    }
                    Piece::Tank => assert!(full.y >= 2.8, "tank came out {full:?}"),
                    Piece::StairHead => {
                        assert!(
                            full.x >= 4.0 && full.y >= 3.2,
                            "stair head came out {full:?}"
                        )
                    }
                    // Vents and masts are meant to be slight.
                    Piece::Vent | Piece::Aerial => {}
                }
            }
        }
    }

    #[test]
    fn a_tower_roof_is_busier_than_a_house_roof() {
        let footprint = roof(48.0, 40.0);
        let seed = seed_for(11, footprint);
        let house = plan(seed, footprint, FacadeClass::House).len();
        let tower = plan(seed, footprint, FacadeClass::Tower).len();
        assert!(tower > house, "tower {tower} vs house {house}");
    }

    /// Occupancy is a fraction of the roof, and a fraction with no cap turns a
    /// city block's worth of roof into a warehouse of boxes.
    #[test]
    fn even_an_enormous_roof_stays_within_its_budget() {
        let footprint = roof(400.0, 400.0);
        for class in FacadeClass::ALL {
            let count = plan(seed_for(5, footprint), footprint, class).len();
            assert!(count <= 14, "{class:?} placed {count}");
        }
    }

    /// Buildings down a street, as they actually stand: same footprint, one
    /// after another. Varying the *size* instead would have been the easy test
    /// to write and a useless one, because these roofs are seeded from where a
    /// building stands and a row of differently-sized boxes all centred on the
    /// origin shares one seed.
    fn along_a_street(count: usize) -> Vec<Rect> {
        (0..count)
            .map(|i| {
                let x = i as f32 * 22.0;
                Rect::new(Vec2::new(x, 0.0), Vec2::new(x + 20.0, 20.0))
            })
            .collect()
    }

    /// A skyline of one extrusion is what this whole module exists to break.
    #[test]
    fn parapets_differ_between_buildings() {
        let thicknesses: Vec<f32> = along_a_street(24)
            .into_iter()
            .map(|footprint| parapet(seed_for(2, footprint), FacadeClass::Midrise).thickness)
            .collect();

        let first = thicknesses[0];
        assert!(
            thicknesses.iter().any(|t| (t - first).abs() > 0.05),
            "every parapet came out the same: {thicknesses:?}"
        );
    }

    /// The other half of the roofline. Two neighbours with identical footprints
    /// must still get different clutter, or a terrace reads as one stamped
    /// repeat — which is the failure this module exists to prevent and is
    /// invisible to a test that only ever looks at one building.
    #[test]
    fn neighbouring_roofs_are_not_stamped_from_the_same_plan() {
        let street = along_a_street(12);
        let plans: Vec<Vec<Placement>> = street
            .iter()
            .map(|f| plan(seed_for(2, *f), *f, FacadeClass::Midrise))
            .collect();

        let differs = plans.windows(2).any(|pair| {
            pair[0].len() != pair[1].len()
                || pair[0]
                    .iter()
                    .zip(&pair[1])
                    .any(|(a, b)| a.piece != b.piece || a.offset != b.offset)
        });
        assert!(differs, "every roof on the street came out identical");
    }

    #[test]
    fn a_taller_building_carries_a_deeper_parapet() {
        let footprint = roof(30.0, 30.0);
        let seed = seed_for(4, footprint);
        assert!(
            parapet(seed, FacadeClass::Tower).thickness
                > parapet(seed, FacadeClass::House).thickness
        );
    }

    /// The plant unit is the one piece here that is hand-built rather than a
    /// primitive, so nothing else would notice it going wrong: a flipped
    /// winding is an invisible box, a missing face is a hole in a shadow, and
    /// a taper of zero is silently the extrusion it replaced.
    #[test]
    fn a_plant_unit_is_a_closed_frustum_at_a_cuboids_price() {
        let mesh = tapered_box(PLANT_TAPER);
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("the plant unit lost its indices");
        };
        assert_eq!(
            indices.len() / 3,
            12,
            "the taper is only free while it stays six quads"
        );

        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the plant unit lost its positions");
        };
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("the plant unit lost its normals");
        };
        assert_eq!(positions.len(), 24, "the folds stopped being hard edges");

        // Unit half-extents at the deck, so a `Placement`'s size still scales
        // it exactly the way it scaled the cuboid, and narrower at the lid, so
        // it is not an extrusion.
        let deck = positions
            .iter()
            .filter(|p| p[1] < 0.0)
            .fold(0.0f32, |w, p| w.max(p[0].abs()));
        let lid = positions
            .iter()
            .filter(|p| p[1] > 0.0)
            .fold(0.0f32, |w, p| w.max(p[0].abs()));
        assert!((deck - 1.0).abs() < 1e-6, "the deck is {deck} across");
        assert!(lid < deck, "the lid is {lid} against a deck of {deck}");
        assert!(lid > 0.5, "a lid at {lid} is a pyramid, not a plant unit");

        // Every face looks out. Wound the other way round the whole unit is
        // culled and a roof grows an invisible box.
        for (position, normal) in positions.iter().zip(normals) {
            let out = Vec3::from_array(*position);
            let normal = Vec3::from_array(*normal);
            assert!((normal.length() - 1.0).abs() < 1e-5);
            assert!(
                normal.dot(out) > 0.0,
                "a face at {out:?} points {normal:?}, back into the box"
            );
        }
    }

    /// A tank is nine times the radius of a vent stack and twenty times the
    /// radius of a mast, and all three used to share one 124-triangle cylinder
    /// sized for none of them.
    #[test]
    fn each_round_piece_is_drawn_at_its_own_radius() {
        use crate::world::props::cylinder_sides;
        assert!(cylinder_sides(AERIAL_RADIUS) < cylinder_sides(VENT_RADIUS));
        assert!(cylinder_sides(VENT_RADIUS) < cylinder_sides(TANK_RADIUS));
        // Sanity against the ranges these stand in for: a midpoint that has
        // drifted outside its own range means `Piece::size` moved and this
        // did not.
        assert!((0.22..0.48).contains(&VENT_RADIUS));
        assert!((0.9..1.5).contains(&TANK_RADIUS));
        assert_eq!(AERIAL_RADIUS, 0.06);
    }
}
