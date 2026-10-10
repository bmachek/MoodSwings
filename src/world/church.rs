//! The church, as the one building that is not a box.
//!
//! Every skyline this game wants to parody has its spires, and a spire is
//! the single strongest silhouette a city block can buy: a tower reading
//! over the rooflines says "somewhere" where a grid of slabs says
//! "anywhere". So the zoning pass stamps three `BuildingKind::Church`es and
//! this module replaces each stamped box with a nave, a front tower, a
//! four-sided spire and a cross — the same arrangement as `world::garage`,
//! which proved that a civic kind can own its whole structure.
//!
//! Same door-frame convention as the garage and the interiors: `x` across
//! the front, `+z` towards the street, origin at the footprint's centre on
//! the pavement; `world::buildings` picks the front and hands in the yaw.
//!
//! The zoned height is treated as *presence* rather than as a wall height:
//! the nave stays low and the tower takes the rest, because a church that
//! is all nave is a warehouse and one that is all tower is a chimney.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::buildings::{ChunkOf, CityAssets, SIDEWALK_HEIGHT};

/// The tower's footprint side, clamped so a small lot still carries one.
///
/// Clamped against the tower's *height* as well as the lot's width, because a
/// real town read off a map has a church the generator never had to imagine:
/// St. Martin's tower is a hundred and thirty metres, and five and a half
/// metres square under that is not a tower, it is a wire. A masonry tower runs
/// about a twelfth of its height across the base — St. Martin's is eleven
/// metres — so that is the other bound.
const TOWER_SIDE_MAX: f32 = 5.5;
/// How wide a tower is across the base as a share of how tall it stands.
const TOWER_SLENDERNESS: f32 = 1.0 / 12.0;
/// The spire's height as a share of the tower's.
const SPIRE_SHARE: f32 = 0.55;

/// How many stages a corner buttress steps in over.
const BUTTRESS_STEPS: u32 = 3;

/// Clear height of the way in: the tower's ground storey, and the arch out of
/// it into the nave.
///
/// A church door is tall and the arch behind it taller, but the *hole in the
/// collider* has to stay a hole a person walks through rather than a slot the
/// tower's whole weight of masonry is missing from. Four and a half metres is
/// a west door on a parish church and it is what the painted doorway on the
/// tower's foot has always suggested.
const PORCH: f32 = 4.6;

/// How far the pitched roof's cube stands proud of the nave's wall head, so
/// the eaves meet the wall instead of floating over it.
const ROOF_LIFT: f32 = 0.2;

/// Daylight between the vault and the roof's lower vertex.
///
/// Not a taste decision: laid at exactly the same height the two surfaces are
/// co-planar over the whole nave, and `world::layer`'s rule applies indoors
/// too — at this town's scale an `f32` cannot separate them, so the ceiling
/// and the underside of the roof flickered against each other in stripes the
/// width of the church. Half a metre is far more than the 200 µm the spacing
/// actually needs and it costs a church nothing.
const VAULT_CLEARANCE: f32 = 0.6;

/// And between the stone vault and the plaster `world::interior` lines it
/// with, for the same reason.
const VAULT_LINING: f32 = 0.15;

/// Structural thickness of the walls the nave is hollowed out of. The same
/// figure the shops use, and it has to be: `world::interior` lines the room
/// off it.
const WALL: f32 = super::interior::WALL;

/// What a walk-in church hands back, so the caller can furnish the nave.
///
/// The church builds its own structure — it is one of the kinds that
/// `BuildingKind::owns_its_structure` covers — so nothing outside here knows
/// how big the nave came out or where the tower left the door.
pub struct Nave {
    /// World position of the nave's middle, on the ground plane.
    pub center: Vec2,
    pub width: f32,
    pub depth: f32,
    /// Floor to ceiling, which for a nave is most of the building.
    pub clear: f32,
    /// The arch out of the porch.
    pub head: f32,
    pub opening: f32,
}

/// The smallest nave worth letting anybody into.
///
/// A stamped chapel on a small lot comes out four metres across, and a room
/// four metres across with a two-metre door in one end is a cupboard with
/// pews. Below this the church stays the solid block it always was, which is
/// also what every church in a generated city is: they are scenery.
const WALK_IN: (f32, f32) = (9.0, 8.0);

/// Spawns one church: nave, pitched roof, tower, spire, cross.
/// Every dimension a church is, worked out from what it was handed.
///
/// A plain function of six numbers, apart from the rest of the module, because
/// everything that can go wrong with a church's proportions goes wrong here
/// and none of it needs a GPU to catch. The walk-in nave in particular is a
/// stack of three constraints that have to hold together — the roof hangs
/// into the room, the vault goes under the roof, and the room has to be
/// taller than its own door afterwards — and each of those was a bug before
/// it was a rule.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub tower_h: f32,
    pub nave_h: f32,
    /// The width the church is *built* at, which is not always the width of
    /// the lot it was claimed on.
    pub width: f32,
    pub tower_side: f32,
    pub nave_depth: f32,
    /// Where the nave and the tower sit along the church's own depth.
    pub nave_z: f32,
    pub tower_z: f32,
    /// Half-size of the cube the pitched roof is made of.
    pub roof_half: f32,
    /// Height of the vault over the nave floor.
    pub vault: f32,
    /// Width of the way in through the tower.
    pub opening: f32,
    /// Whether this one is built as a room rather than as a block.
    pub walk_in: bool,
}

impl Shape {
    pub fn of(width: f32, depth: f32, height: f32, measured: Option<f32>, cathedral: bool) -> Self {
        // A parish tower is one and a half claims tall. The cathedral's is not
        // a multiple anybody signed off on — it is the tallest brick tower in
        // the world, the plaque says so, and the skyline has to back it up. But
        // a measured tower is neither: it is what somebody went up with a tape.
        let tower_h = measured.unwrap_or(height * if cathedral { 5.0 } else { 1.5 });
        let nave_h = match measured {
            // The nave of a real hall church is most of the height of its
            // aisles — St. Martin's interior is 28.8 m clear — and nothing
            // like a multiple of its tower.
            Some(_) => (tower_h * 0.22).clamp(9.0, 30.0),
            None => (height * 0.55).clamp(6.0, 12.0),
        };
        // The zoning pass claims *large* buildings, and a church the full width
        // of a department-store lot grows a roof that swallows its own tower —
        // the pitched roof's height is tied to the nave's width by the 45°
        // geometry. So the nave keeps church proportions and cedes the rest of
        // the lot to its own forecourt.
        //
        // A measured church keeps its own width up to a real basilica's: St.
        // Martin is 28 m across its aisles, and clamping that to eighteen turns
        // the building the town is named for into a chapel.
        let width = width.min(if measured.is_some() { 30.0 } else { 18.0 });
        let tower_side = (width * 0.32)
            .min(TOWER_SIDE_MAX.max(height * TOWER_SLENDERNESS))
            .min(width * 0.45);
        // The nave sits behind the tower; the tower stands on the street front.
        let nave_depth = depth - tower_side;

        // Where the ceiling has to go, which is not where the walls stop.
        //
        // The pitched roof is a cube turned 45° about the ridge, and its
        // *lower* vertex hangs as far below the eaves as its upper one stands
        // above them — a good fifteen metres on a building this wide. That was
        // free while the nave was solid, because the whole of it was buried in
        // masonry. Hollow the nave out and the same vertex is a stone wedge
        // hanging down the middle of the church, which is what the first take
        // of this drew. So the vault goes under it.
        let roof_half = width * 0.72;
        let vault =
            (nave_h + ROOF_LIFT - roof_half * std::f32::consts::FRAC_1_SQRT_2 - VAULT_CLEARANCE)
                .min(nave_h - WALL);

        Self {
            tower_h,
            nave_h,
            width,
            tower_side,
            nave_depth,
            nave_z: -tower_side * 0.5,
            tower_z: (depth - tower_side) * 0.5,
            roof_half,
            vault,
            // How wide the way through the tower is. Held well inside the
            // tower's own side so the masonry either side of the door is
            // masonry rather than a render of it.
            opening: (tower_side * 0.5).clamp(1.8, 3.2),
            // Big enough to stand in? Below that a church is scenery, as every
            // church in a generated city is — see [`WALK_IN`]. A generated one
            // never passes: its nave is twelve metres at the most and its roof
            // takes nine of them.
            walk_in: width >= WALK_IN.0 && nave_depth >= WALK_IN.1 && vault > PORCH + 2.0,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    assets: &CityAssets,
    center: Vec2,
    ground: f32,
    width: f32,
    depth: f32,
    height: f32,
    // The tower's real height in metres, where the map measured it.
    //
    // `height` means two different things depending on who is calling, and this
    // is how they are told apart. To the zoning pass it is a *claim* — the size
    // of the lot the church was stamped on — and the tower comes out a multiple
    // of it. To a town read off a map it is a measurement, and St. Martin's
    // tower is a hundred and thirty metres because that is what it is, not
    // because a lot was big.
    measured: Option<f32>,
    yaw: f32,
    cathedral: bool,
    chunk: IVec2,
) -> Option<Nave> {
    let spin = Quat::from_rotation_y(yaw);
    // On the pavement, or on the plateau the terrain holds under a chapel
    // kept up on the hill — see `Building::ground`.
    let place = |at: Vec3| spin * at + Vec3::new(center.x, ground + SIDEWALK_HEIGHT, center.y);
    // A measured church is a real one, and the real ones here are brick. The
    // generator's own stamped churches stay rendered, because a city grown from
    // a seed has no reason to be Bavarian.
    let walls = if measured.is_some() {
        assets.brick()
    } else {
        assets.concrete()
    };

    let shape = Shape::of(width, depth, height, measured, cathedral);
    let Shape {
        tower_h,
        nave_h,
        width,
        tower_side,
        nave_depth,
        nave_z,
        tower_z,
        roof_half,
        vault,
        opening,
        walk_in,
    } = shape;

    let solid = |commands: &mut Commands,
                 material: Handle<StandardMaterial>,
                 at: Vec3,
                 size: Vec3,
                 spun: Quat| {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(assets.stone_cube.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(place(at))
                .with_rotation(spin * spun)
                .with_scale(size),
            RigidBody::Static,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));
    };

    // A box from outside, a room from inside. Drawn as the shell it is —
    // four walls and a vault — rather than as one scaled cube, because a cube
    // cannot have a hole in it and a church that cannot be seen into is a
    // church nobody walks into. The outer faces sit exactly where the cube's
    // did, so the silhouette from the square is the one it always had.
    //
    // The collider is the same list of plates on its own unscaled entity: a
    // compound scaled by its transform would turn a three-metre arch into a
    // fraction of whatever the building measures.
    let hollow = |commands: &mut Commands,
                  material: Handle<StandardMaterial>,
                  at: Vec3,
                  size: Vec3,
                  gaps: [bool; 2],
                  gap_h: f32,
                  ceiling: f32| {
        let (w, h, d) = (size.x, size.y, size.z);
        let flank = (w - opening) * 0.5;
        // The vault, at the height asked for rather than under the wall head:
        // see `vault`. No floor — the ground is already under it, and a slab
        // here would bury `world::interior`'s own.
        let mut pieces: Vec<(Vec3, Vec3)> = vec![(
            Vec3::new(0.0, -h * 0.5 + ceiling + WALL * 0.5, 0.0),
            Vec3::new(w, WALL, d),
        )];
        for side in [-1.0f32, 1.0] {
            pieces.push((
                Vec3::new(side * (w - WALL) * 0.5, 0.0, 0.0),
                Vec3::new(WALL, h, d),
            ));
        }
        // `gaps[0]` is the `+z` face — the one the tower stands against —
        // and `gaps[1]` the `-z` one.
        for (i, side) in [1.0f32, -1.0].into_iter().enumerate() {
            let z = side * (d - WALL) * 0.5;
            if !gaps[i] {
                pieces.push((Vec3::new(0.0, 0.0, z), Vec3::new(w, h, WALL)));
                continue;
            }
            for jamb in [-1.0f32, 1.0] {
                pieces.push((
                    Vec3::new(jamb * (opening + flank) * 0.5, 0.0, z),
                    Vec3::new(flank, h, WALL),
                ));
            }
            if gap_h < h - 1.0e-3 {
                pieces.push((
                    Vec3::new(0.0, -h * 0.5 + gap_h + (h - gap_h) * 0.5, z),
                    Vec3::new(opening, h - gap_h, WALL),
                ));
            }
        }
        let mut plates: Vec<(Vec3, Quat, Collider)> = Vec::with_capacity(pieces.len());
        for (local, piece) in pieces {
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(assets.stone_cube.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(place(at + local))
                    .with_rotation(spin)
                    .with_scale(piece),
            ));
            plates.push((
                local,
                Quat::IDENTITY,
                Collider::cuboid(piece.x, piece.y, piece.z),
            ));
        }
        commands.spawn((
            ChunkOf(chunk),
            RigidBody::Static,
            Collider::compound(plates),
            Transform::from_translation(place(at)).with_rotation(spin),
        ));
    };

    // The nave, in the kerb's stone — a church is masonry or it is a hall.
    if walk_in {
        hollow(
            commands,
            walls.clone(),
            Vec3::new(0.0, nave_h * 0.5, nave_z),
            Vec3::new(width, nave_h, nave_depth),
            // Pierced towards the tower only, and the tower covers the hole.
            [true, false],
            PORCH,
            vault,
        );
    } else {
        solid(
            commands,
            walls.clone(),
            Vec3::new(0.0, nave_h * 0.5, nave_z),
            Vec3::new(width, nave_h, nave_depth),
            Quat::IDENTITY,
        );
    }
    // The pitched roof: a cube turned 45° about the ridge line reads as a
    // gable from every angle that matters. Its lower half is buried in the
    // nave, which is what keeps the eaves from floating.
    let half = roof_half;
    solid(
        commands,
        assets.roof_material(half.max(nave_depth)),
        Vec3::new(0.0, nave_h + ROOF_LIFT, nave_z),
        Vec3::new(half, half, nave_depth * 0.98),
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
    );

    // The tower, centred on the front face.
    if walk_in {
        // Its ground storey is the way in: a west door on the street and an
        // arch out of it into the nave, with the rest of the tower standing
        // on the masonry either side. The storey above is a plain block — a
        // spiral stair is a fine thing to model on the day anybody is allowed
        // up it. The street door stops short of the vault so there is a
        // lintel over it; the arch into the nave does not, because that is an
        // arch rather than a door.
        hollow(
            commands,
            walls.clone(),
            Vec3::new(0.0, PORCH * 0.5, tower_z),
            Vec3::new(tower_side, PORCH, tower_side),
            [true, true],
            PORCH * 0.78,
            PORCH - WALL,
        );
        solid(
            commands,
            walls.clone(),
            Vec3::new(0.0, PORCH + (tower_h - PORCH) * 0.5, tower_z),
            Vec3::new(tower_side, tower_h - PORCH, tower_side),
            Quat::IDENTITY,
        );
    } else {
        solid(
            commands,
            walls.clone(),
            Vec3::new(0.0, tower_h * 0.5, tower_z),
            Vec3::new(tower_side, tower_h, tower_side),
            Quat::IDENTITY,
        );
        // The dark doorway in the tower's foot: two jambs and a lintel of
        // shadow, painted in geometry because the tower wears no facade. A
        // walk-in church has a real hole instead, and painting shadow over it
        // would be a door with a door in it.
        solid(
            commands,
            // The doorway is a slab of shadow rather than roofing; the tiling
            // only has to be small enough not to smear.
            assets.roof_material(2.0),
            Vec3::new(0.0, 1.6, tower_z + tower_side * 0.5 + 0.03),
            Vec3::new(1.9, 3.2, 0.08),
            Quat::IDENTITY,
        );
    }

    // The spire: an octagonal helm. Visual only — nothing lands on a spire on
    // purpose, and anything arriving by accident can meet the tower's box
    // below it.
    let spire_h = tower_h * SPIRE_SHARE;
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(assets.spire()),
        MeshMaterial3d(assets.roof_material(tower_side.max(spire_h))),
        Transform::from_translation(place(Vec3::new(0.0, tower_h + spire_h * 0.5, tower_z)))
            // A cone stands with a vertex forward; half a face turns it so a
            // *face* looks down the street, which is how a helm sits on a
            // square tower.
            .with_rotation(
                spin * Quat::from_rotation_y(
                    std::f32::consts::PI / super::buildings::SPIRE_SIDES as f32,
                ),
            )
            .with_scale(Vec3::new(tower_side * 0.72, spire_h, tower_side * 0.72)),
    ));

    // Buttresses: four stepped piers on the tower's corners, each stepping in
    // twice on its way up. A tower this tall with nothing on its corners is a
    // chimney, and it is the one building in the town whose corners are looked
    // at from four hundred metres — the whole reason `CityStyle` names a real
    // town is the church at the end of its Altstadt.
    //
    // Stepped rather than tapered because a buttress is *courses* of stone,
    // and three scaled boxes say that in twelve triangles apiece where a taper
    // would need a mesh of its own.
    let pier = tower_side * 0.22;
    for step in 0..BUTTRESS_STEPS {
        let share = 1.0 - step as f32 / BUTTRESS_STEPS as f32;
        let stage = tower_h * share * 0.86;
        let out = pier * (1.0 - step as f32 * 0.24);
        for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            // The two on the nave side stand *inside* it: the tower's back
            // corners are already past the nave's front wall. That cost
            // nothing while the nave was solid stone, and the moment it was
            // hollowed out it was two four-metre piers of brick planted among
            // the pews, rising the full height of the tower. A west tower's
            // rear buttresses are absorbed into the nave in any case.
            if walk_in && sz < 0.0 {
                continue;
            }
            solid(
                commands,
                walls.clone(),
                Vec3::new(
                    sx * (tower_side * 0.5 + out * 0.5 - 0.08),
                    stage * 0.5,
                    tower_z + sz * (tower_side * 0.5 + out * 0.5 - 0.08),
                ),
                Vec3::new(out * 2.0, stage, out * 2.0),
                Quat::IDENTITY,
            );
        }
    }
    // And the string course the helm sits on, which is what stops the spire
    // reading as a party hat balanced on a post.
    solid(
        commands,
        walls.clone(),
        Vec3::new(0.0, tower_h - 0.18, tower_z),
        Vec3::new(tower_side * 1.16, 0.36, tower_side * 1.16),
        Quat::IDENTITY,
    );
    // Two tall louvre openings per face of the belfry, in the same shadow the
    // doorway is painted with.
    let louvre = tower_h * 0.07;
    for (dx, dz, turn) in [
        (0.0f32, 1.0f32, 0.0f32),
        (0.0, -1.0, 0.0),
        (1.0, 0.0, std::f32::consts::FRAC_PI_2),
        (-1.0, 0.0, std::f32::consts::FRAC_PI_2),
    ] {
        for side in [-1.0f32, 1.0] {
            let along = Vec3::new(-dz, 0.0, dx) * (tower_side * 0.20 * side);
            solid(
                commands,
                assets.roof_material(2.0),
                Vec3::new(
                    dx * (tower_side * 0.5 + 0.03),
                    tower_h - 0.5 - louvre,
                    tower_z + dz * (tower_side * 0.5 + 0.03),
                ) + along,
                Vec3::new(tower_side * 0.24, louvre * 2.0, 0.08),
                Quat::from_rotation_y(turn),
            );
        }
    }

    // The cross: two slim bars, proud of the spire's tip — scaled up on
    // the cathedral, where a parish cross would vanish at that altitude.
    let cross_base = tower_h + spire_h;
    let c = if cathedral { 2.2 } else { 1.0 };
    for (at, size) in [
        (
            Vec3::new(0.0, cross_base + 0.9 * c, tower_z),
            Vec3::new(0.12 * c, 1.8 * c, 0.12 * c),
        ),
        (
            Vec3::new(0.0, cross_base + 1.15 * c, tower_z),
            Vec3::new(0.85 * c, 0.12 * c, 0.12 * c),
        ),
    ] {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(assets.stone_cube.clone()),
            MeshMaterial3d(assets.concrete()),
            Transform::from_translation(place(at))
                .with_rotation(spin)
                .with_scale(size),
        ));
    }

    if !walk_in {
        return None;
    }

    let middle = spin * Vec3::new(0.0, 0.0, nave_z);
    Some(Nave {
        center: Vec2::new(center.x + middle.x, center.y + middle.z),
        width,
        depth: nave_depth,
        // Under the vault, not level with it. `world::interior` hangs its
        // ceiling lining with the underside at the clear height it is given,
        // and the stone vault's underside is at `vault` — two down-facing
        // faces in one plane, which is the layer rule again and drew the
        // church's whole ceiling in flickering stripes.
        clear: vault - VAULT_LINING,
        head: PORCH - WALL,
        opening,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// St. Martin as the committed atlas measures it: 130.6 m of tower on a
    /// footprint 91.5 by 27.8. The one church in this town anybody is going
    /// to try the door of.
    fn sankt_martin() -> Shape {
        Shape::of(91.5, 27.8, 130.6, Some(130.6), false)
    }

    #[test]
    fn the_basilica_can_be_walked_into_and_a_stamped_chapel_cannot() {
        assert!(sankt_martin().walk_in, "St. Martin is a block again");
        // What the zoning pass stamps: a claim between ten and fourteen
        // metres, no measurement, and a roof that takes most of the nave.
        for height in [10.0f32, 12.0, 14.0] {
            let chapel = Shape::of(16.0, 14.0, height, None, false);
            assert!(
                !chapel.walk_in,
                "a {height} m stamped chapel offered a nave {:.1} m clear",
                chapel.vault
            );
        }
    }

    /// The vault goes *under* the roof's lower vertex.
    ///
    /// The pitched roof is a cube turned 45°, so it hangs as far below the
    /// eaves as it stands above them — fifteen metres on St. Martin. While
    /// the nave was solid that was buried in masonry; hollowed out it is a
    /// stone wedge down the middle of the church, and laid level with the
    /// ceiling instead it is the whole nave flickering, which is
    /// `world::layer`'s rule indoors.
    #[test]
    fn the_vault_clears_the_roof_it_hangs_under() {
        let shape = sankt_martin();
        let vertex = shape.nave_h + ROOF_LIFT - shape.roof_half * std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            vertex - shape.vault >= VAULT_CLEARANCE - 1.0e-4,
            "the vault is {:.2} m under a roof that reaches {:.2} m",
            shape.vault,
            vertex
        );
        assert!(
            shape.vault > PORCH + 2.0,
            "a nave {:.1} m clear with a {PORCH} m door in it",
            shape.vault
        );
    }

    /// And the way in is a hole the tower covers.
    ///
    /// The nave is pierced towards the tower and drawn as a shell, so the
    /// hole is only invisible from the square while it stays inside the
    /// tower's own footprint. Widen the opening past the tower and the
    /// church has a slot through its west front.
    #[test]
    fn the_way_in_stays_behind_the_tower() {
        for shape in [
            sankt_martin(),
            Shape::of(40.0, 30.0, 60.0, Some(60.0), false),
            Shape::of(30.0, 40.0, 95.0, Some(95.0), true),
        ] {
            assert!(
                shape.opening + 2.0 * WALL <= shape.tower_side,
                "a {:.1} m opening in a {:.1} m tower",
                shape.opening,
                shape.tower_side
            );
            // And the two meet: the nave's front face is the tower's back one.
            let nave_front = shape.nave_z + shape.nave_depth * 0.5;
            let tower_back = shape.tower_z - shape.tower_side * 0.5;
            assert!(
                (nave_front - tower_back).abs() < 1.0e-3,
                "the nave ends at {nave_front} and the tower starts at {tower_back}"
            );
        }
    }

    #[test]
    fn the_tower_out_ranks_the_nave() {
        // The proportions are the whole silhouette: for any zoned height the
        // claim hands over (10..14), the nave stays squat and the tower with
        // its spire clearly overtops it — otherwise the skyline reads
        // "warehouse with a chimney" instead of "church".
        for height in [10.0f32, 12.0, 14.0] {
            let nave = (height * 0.55f32).clamp(6.0, 10.0);
            let tower = height * 1.5;
            assert!(nave < tower * 0.6, "at {height}m the nave swallows it");
            assert!(
                tower * (1.0 + SPIRE_SHARE) > height * 1.8,
                "at {height}m the spire fails to read over the block"
            );
        }
    }
}
