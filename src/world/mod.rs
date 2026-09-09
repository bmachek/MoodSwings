//! The world: procedural city, physics, streaming, and the day/night cycle.

pub mod atlas;
pub mod buildings;
pub mod bunting;
pub mod church;
pub mod citygen;
pub mod decals;
pub mod facade;
pub mod frontage;
pub mod gable;
pub mod garage;
pub mod ground;
pub mod interior;
pub mod layer;
pub mod litter;
pub mod lots;
pub mod markings;
pub mod material;
pub mod mayhem;
pub mod plume;
pub mod props;
pub mod river;
pub mod road;
pub mod roadgraph;
pub mod rooftop;
pub mod shell;
pub mod signage;
pub mod sky;
pub mod stadium;
pub mod statues;
pub mod streaming;
pub mod streetlights;
pub mod streetname;
pub mod streetside;
pub mod terrain;
pub mod texture;
pub mod timeofday;
pub mod vegetation;
pub mod weather;
pub mod worksite;

use avian3d::prelude::*;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};

use crate::core::config::GameConfig;

/// The generated city. Held whole rather than streamed — see `streaming`.
#[derive(Resource, Deref)]
pub struct City(pub citygen::CityLayout);

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        let bounce = app.world().resource::<GameConfig>().bounce.clone();
        app.add_plugins((
            // `interpolate_all`: physics ticks at 64Hz while frames come at
            // whatever vsync grants, and without easing every body's rendered
            // pose is a stair-step held for 0, 1 or 2 ticks per frame — which
            // reads as lag, most of all on the car the camera is bolted to.
            // Interpolation costs one tick (~16ms) of visual latency, which
            // nothing in a game without aiming will ever notice.
            PhysicsPlugins::default().set(PhysicsInterpolationPlugin::interpolate_all()),
            material::MaterialLibraryPlugin,
            facade::FacadePlugin,
            road::RoadPlugin,
            weather::WeatherPlugin,
            timeofday::TimeOfDayPlugin,
            streetlights::StreetLightPlugin,
            vegetation::VegetationPlugin,
            mayhem::MayhemPlugin,
            stadium::StadiumPlugin,
            litter::LitterPlugin,
            worksite::WorksitePlugin,
            bunting::BuntingPlugin,
            plume::PlumePlugin,
            frontage::FrontagePlugin,
        ))
        // A second call rather than a longer tuple: `Plugins` is implemented up
        // to a fixed arity and the tuple above is at it.
        .add_plugins((sky::SkyPlugin, ground::GroundPlugin))
        // Everything in this city is made of rubber, and the solver is where
        // that is decided. `Max` rather than the default average: a rubber ball
        // bounces off concrete because *it* is elastic, and asking concrete to
        // agree would flatten every bounce against the world by half.
        .insert_resource(DefaultRestitution(
            Restitution::new(bounce.restitution).with_combine_rule(CoefficientCombine::Max),
        ))
        .insert_resource(avian3d::dynamics::solver::SolverConfig {
            restitution_threshold: bounce.threshold,
            // One pass leaves a body resting on a floor with several contact
            // points bouncing unevenly, which reads as a wobble rather than as
            // rubber.
            restitution_iterations: 4,
            ..default()
        })
        .init_resource::<streaming::ActiveChunks>()
        .init_resource::<streaming::StreamTimer>()
        .add_systems(PreStartup, facade::load_shader)
        .add_systems(Startup, (generate_city, setup_ground).chain())
        .add_systems(Update, streaming::update_streaming);
    }
}

fn generate_city(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    library: Res<material::MaterialLibrary>,
    mut facades: ResMut<Assets<facade::FacadeMaterial>>,
    mut grounds: ResMut<Assets<ground::GroundMaterial>>,
    mut wear: ResMut<Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    mut wet: ResMut<weather::WetSurfaces>,
) {
    let started = std::time::Instant::now();
    // A style that names a town builds that town; everything else is grown
    // from the seed. A named town that will not load falls back to the
    // generator with a warning rather than to an empty world — the same
    // discipline a missing sound gets.
    let layout = config
        .city
        .atlas()
        .and_then(atlas::load)
        .map(|town| atlas::layout(&town, config.world_seed, config.world.half_extent))
        .unwrap_or_else(|| {
            (
                citygen::generate(config.world_seed, config.world.half_extent, config.city),
                atlas::Signposts::default(),
            )
        });
    let (mut layout, signs) = layout;
    // A town read off a map arrives as a road network and nothing else. What
    // fills it is not blocks — a real block is not a rectangle — but frontages
    // marched down each side of each street; see `world::streetside`.
    let mut frontage = streetside::Frontage::default();
    if layout.blocks.is_empty() {
        let (blocks, holes) = streetside::lots(&layout, config.world_seed, config.city);
        layout.blocks = blocks;
        frontage = holes;
    }
    // Spent by `setup_ground`, which is the next system in the chain and
    // therefore sees the insert. The gaps are decided here because they fall
    // out of the marcher's own walk; the meshes that fill them are built there,
    // because that is where `Ribbons` is.
    commands.insert_resource(frontage);

    info!(
        "city built in {:.1}ms: {} blocks, {} buildings, {} intersections, {} roads",
        started.elapsed().as_secs_f32() * 1000.0,
        layout.blocks.len(),
        layout.building_count(),
        layout.graph.node_count(),
        layout.graph.edge_count(),
    );

    commands.insert_resource(streetname::build_assets(
        &signs.names,
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(signs);

    river::spawn(&mut commands, &layout, &mut meshes, &mut materials);

    let city = City(layout);
    commands.insert_resource(streaming::ChunkIndex::build(&city));
    commands.insert_resource(city);
    commands.insert_resource(props::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(litter::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(bunting::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(plume::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(gable::build_assets(
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(streetside::build_assets(&mut meshes));
    commands.insert_resource(worksite::build_assets(
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(rooftop::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(shell::build_assets(&mut meshes));
    commands.insert_resource(decals::build_assets(&mut images, &mut wear));
    commands.insert_resource(signage::build_assets(
        &mut images,
        &mut materials,
        &mut meshes,
    ));
    commands.insert_resource(lots::build_assets(&mut meshes, &mut materials, &mut images));
    commands.insert_resource(statues::build_assets(
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(stadium::build_assets(
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(interior::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(frontage::build_assets(
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(vegetation::build_assets(
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(markings::build_assets(
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(buildings::build_assets(
        config.city,
        &mut meshes,
        &mut materials,
        &mut images,
        &library,
        &mut facades,
        &mut grounds,
        &mut wet,
    ));
}

/// Metres of road covered by one repeat of the asphalt texture.
///
/// The scanned set is photographed at roughly two metres across. Tiling it at
/// its true size makes the repeat obvious on a long straight, so it is stretched
/// somewhat — the trade is between visible repetition and visible blur, and at
/// the angle a road is actually seen from, blur loses.
pub(crate) const ASPHALT_TILE: f32 = 6.0;

/// How wide the road surface is drawn, in metres. Far beyond the streamed city
/// on purpose — see `setup_ground`.
const GROUND_VIEW_EXTENT: f32 = 40_000.0;

/// The most texture repeats one cell of a ground plane is allowed to carry.
///
/// This is a floating-point budget rather than a look. A texture coordinate is
/// an `f32`, and an `f32` near U has a spacing of about `U × 2⁻²³` between the
/// numbers it can represent; a 2K texture's texel is `1/2048`. The two meet at
/// U ≈ 4096, and *at* that value a UV cannot address one texel from the next.
///
/// The ground used to be a single forty-kilometre quad with its UVs scaled by
/// `extent / tile` — six and a half thousand. One texel of precision, exactly.
/// It did not show as blur or as a seam, which is why it survived so long: what
/// the hardware does with a UV that cannot resolve a texel is compute garbage
/// screen-space derivatives from it, and derivatives are what choose the mip
/// level. Neighbouring pixels landed on different levels at random, so the road
/// three metres in front of the camera was a per-pixel lottery between the top
/// of the asphalt's mip chain and the middle of it — dense black speckle over
/// grey, worst close up, clean in the distance where the correct level is high
/// anyway. It was hidden for as long as a flat five hundred lux of ambient was
/// washing the road out; taking that away is what put it on screen.
///
/// So the plane is cut into cells and every cell starts its UVs at zero. At 128
/// repeats a cell there are five bits of headroom under a texel, and the largest
/// ground in the game is a hundred cells on a side.
const CELL_REPEATS: f32 = 128.0;

/// How finely a ground cell is subdivided, by how far out it is.
///
/// `(square distance from the middle of the world, quads across one cell)`.
/// Read in order; the first entry a cell's centre is inside wins.
///
/// The whole ground used to be one quad per cell — one vertex every four
/// hundred and forty-eight metres, four and a half of them across the entire
/// town. That is not a coarse surface, it is a *plane*, and no amount of work
/// in `ground.wgsl` puts a hollow or a horizon into a plane. It also meant the
/// ground could not be displaced at all: there was nowhere to put the shape.
///
/// The near ring is fourteen-metre quads, which samples the gentle field in
/// `world::terrain` five times per wavelength and is finer than anything the
/// player can walk to. Past the town the steps quadruple twice, because what
/// is out there is a nine-hundred-metre landscape and a fifty-six-metre quad
/// still carries sixteen samples across one hill.
const GROUND_STEPS: [(f32, u32); 4] = [
    (1_600.0, 32),
    (4_500.0, 8),
    (13_500.0, 2),
    (f32::INFINITY, 1),
];

/// How far a cell's skirt hangs below its own edge, as a share of one quad.
///
/// Two cells of different fineness do not share vertices along the edge
/// between them, so the finer one's displaced midpoints leave a crack against
/// the coarser one's straight edge. A skirt is the standard answer and it is
/// the right one here: a vertical curtain hanging *down* from each cell's
/// border is behind the ground from every viewpoint above the ground, and the
/// player cannot get under it.
const GROUND_SKIRT: f32 = 1.4;

/// A ground surface whose texture coordinates restart every cell.
///
/// Covers at least `extent` metres square, centred on the origin, facing up,
/// and shaped by `terrain` — which returns exactly zero anywhere anything is
/// built, so this is still a dead-flat plane wherever the town stands on it.
///
/// The cells are whole numbers of texture repeats, so the seam between two of
/// them falls exactly where the texture wraps anyway and cannot be seen; a
/// cell's own vertices are shared, so the finest ring costs one vertex per
/// quad rather than four.
///
/// Tangents are written rather than generated. For a heightfield whose UVs run
/// along world X and Z the tangent basis is arithmetic — `mikktspace` on two
/// hundred thousand vertices is a startup hitch bought for an answer already
/// known — and `the_written_tangents_are_the_ones_mikktspace_would_have_found`
/// holds the two to each other.
fn tiled_ground(extent: f32, tile: f32, terrain: &terrain::Terrain) -> Mesh {
    let cell = tile * CELL_REPEATS;
    let cells = (extent / cell).ceil().max(1.0) as u32;
    let half = cells as f32 * cell * 0.5;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut tangents: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for row in 0..cells {
        for column in 0..cells {
            let x0 = -half + column as f32 * cell;
            let z0 = -half + row as f32 * cell;
            let out = (x0 + cell * 0.5).abs().max((z0 + cell * 0.5).abs());
            let steps = GROUND_STEPS
                .iter()
                .find(|(reach, _)| out < *reach)
                .map(|(_, steps)| *steps)
                .unwrap_or(1);
            let step = cell / steps as f32;
            let base = positions.len() as u32;

            for iz in 0..=steps {
                for ix in 0..=steps {
                    let at = Vec2::new(x0 + ix as f32 * step, z0 + iz as f32 * step);
                    let normal = terrain.normal(at, step * 0.5);
                    positions.push([at.x, terrain.height(at), at.y]);
                    normals.push(normal.to_array());
                    uvs.push([
                        ix as f32 / steps as f32 * CELL_REPEATS,
                        iz as f32 / steps as f32 * CELL_REPEATS,
                    ]);
                    // U runs along +X, so the tangent is the surface direction
                    // that has no Z in it, and the handedness is negative
                    // because V runs along +Z while `cross(normal, tangent)`
                    // on an upward face points at −Z.
                    let tangent = Vec3::new(1.0, -normal.x / normal.y.max(1e-4), 0.0).normalize();
                    tangents.push([tangent.x, tangent.y, tangent.z, -1.0]);
                }
            }

            let vertex = |ix: u32, iz: u32| base + iz * (steps + 1) + ix;
            for iz in 0..steps {
                for ix in 0..steps {
                    let (a, b) = (vertex(ix, iz), vertex(ix + 1, iz));
                    let (c, d) = (vertex(ix + 1, iz + 1), vertex(ix, iz + 1));
                    indices.extend([a, c, b, a, d, c]);
                }
            }

            // The skirt. One quad per border edge, hanging straight down.
            let drop = step * GROUND_SKIRT;
            let mut hem = |a: u32, b: u32| {
                let start = positions.len() as u32;
                for corner in [a, b] {
                    let top = positions[corner as usize];
                    positions.push(top);
                    positions.push([top[0], top[1] - drop, top[2]]);
                    let uv = uvs[corner as usize];
                    uvs.push(uv);
                    uvs.push(uv);
                    for _ in 0..2 {
                        normals.push(normals[corner as usize]);
                        tangents.push(tangents[corner as usize]);
                    }
                }
                // Both windings: a skirt is never meant to be seen at all, and
                // which side of it faces out depends on which border it is on.
                indices.extend([start, start + 1, start + 3, start, start + 3, start + 2]);
                indices.extend([start, start + 3, start + 1, start, start + 2, start + 3]);
            };
            for i in 0..steps {
                hem(vertex(i, 0), vertex(i + 1, 0));
                hem(vertex(i, steps), vertex(i + 1, steps));
                hem(vertex(0, i), vertex(0, i + 1));
                hem(vertex(steps, i), vertex(steps, i + 1));
            }
        }
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
    .with_inserted_indices(Indices::U32(indices))
}

/// Points a side of the ground's collider.
///
/// Four metres between samples over the played extent, which is finer than
/// anything the gentle field does — it moves by forty centimetres over seventy
/// metres — and cheap: parry builds the shape implicitly from the lattice
/// rather than from triangles, so this is five hundred and fifty rows of
/// floats and no BVH.
const GROUND_LATTICE: usize = 550;

/// The ground, as something to stand on.
///
/// A heightfield rather than the flat box this used to be. It has to agree
/// with [`tiled_ground`] to the millimetre or the player walks on air, and it
/// does for the only reason that is maintainable: both of them ask
/// `world::terrain` and neither of them has an opinion of its own.
///
/// Parry indexes its lattice `[z][x]` — rows are Z — which Avian's own doc
/// comment has the other way round. It matters here because the ground is not
/// symmetrical.
fn ground_collider(terrain: &terrain::Terrain, played: f32) -> Collider {
    let last = GROUND_LATTICE - 1;
    let heights = (0..GROUND_LATTICE)
        .map(|iz| {
            let z = (iz as f32 / last as f32 - 0.5) * played;
            (0..GROUND_LATTICE)
                .map(|ix| {
                    let x = (ix as f32 / last as f32 - 0.5) * played;
                    terrain.height(Vec2::new(x, z))
                })
                .collect()
        })
        .collect();
    Collider::heightfield(heights, Vec3::new(played, 1.0, played))
}

/// The road surface. Streets are not meshed individually: the ground *is* the
/// asphalt, and the raised pavement slabs on each block carve the street grid
/// out of it as negative space. One quad instead of thousands of road polys.
///
/// That one quad is two kilometres across, so the asphalt tiles a few hundred
/// times over it. Everything that makes that survivable — a texture that wraps,
/// a mip chain, anisotropic filtering, and a UV that stays inside what an `f32`
/// can say — lives in `texture` and in [`tiled_ground`].
#[allow(clippy::too_many_arguments)]
fn setup_ground(
    mut commands: Commands,
    config: Res<GameConfig>,
    library: Res<material::MaterialLibrary>,
    city: Res<City>,
    frontage: Res<streetside::Frontage>,
    foliage: Res<vegetation::FoliageKit>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut roads: ResMut<Assets<road::RoadMaterial>>,
    mut grounds: ResMut<Assets<ground::GroundMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // A town read off a map runs the ground the other way round. The generator
    // covers the world in asphalt and lets its block slabs carve the streets
    // out of it as negative space — one quad for the whole city — and that only
    // works because its blocks tile the ground. A real town's do not, so the
    // world would be a tarmac plain with houses on it. Here the ground is
    // grass and the roads are laid on top of it, one ribbon per street.
    let streetside = city.blocks.first().is_some_and(|block| !block.paved);
    // The visible ground runs far past the city, so that from a rooftop the
    // world does not end in a rectangle hanging in mid-air; the atmosphere
    // hazes the surplus into the horizon within a couple of kilometres. The
    // collider only needs to cover the city.
    let played = config.world.half_extent * 2.0 + 200.0;
    // And the shape of it. A generated grid city paves its own whole footprint
    // with block slabs laid at a fixed height, so it takes the landscape past
    // the town and a level floor under it; a real town has open back land in
    // it and gets both — see `world::terrain`, which is where the rule that
    // makes this safe at all is written down.
    let reach = config.world.half_extent + ENVELOPE + BACKLAND + 60.0;
    let terrain = terrain::Terrain::new(&city, reach, streetside, config.world_seed);
    if streetside {
        // The asphalt a ribbon is made of: the same material the one big quad
        // would have used, at a tiling of one, because a ribbon carries its own
        // size in its UVs instead.
        // One material per surface the extract knows about, built up front:
        // a ribbon picks by its edge's surface, so a cobbled Altstadt costs
        // four materials for the whole town rather than one per street.
        let paving = atlas::Surface::ALL
            .map(|surface| roads.add(carriageway(surface, &library, images.as_mut())));
        // The pavement's own slabs, at a tiling of one: the footway meshes
        // carry their true size in their UVs, so the material must not scale
        // them a second time. Not wet-registered — the streetside pavements are
        // spawned per chunk from a shared handle, and `WetSurfaces` wants a
        // material it can recolour, which this shares with nothing else that
        // would want it left alone.
        let mut slabs = StandardMaterial {
            perceptual_roughness: 0.95,
            ..default()
        };
        match library.get(material::set::PAVEMENT) {
            Some(scanned) => {
                scanned.apply(&mut slabs);
                slabs.base_color = Color::srgb(0.62, 0.61, 0.60);
            }
            None => {
                slabs.base_color = Color::srgb(0.52, 0.52, 0.53);
                slabs.base_color_texture = Some(images.add(texture::paving()));
                slabs.normal_map_texture = Some(images.add(texture::paving_normal()));
            }
        }
        let slabs = materials.add(slabs);
        info!("{} gaps in the town's frontage to fill", frontage.len());
        commands.insert_resource(streetside::build_ribbons(
            &city,
            &frontage,
            meshes.as_mut(),
            paving,
            slabs,
            materials.add(forecourt(&library, images.as_mut())),
            materials.add(boundary_wall(&library, images.as_mut())),
            // At a repeat of one, because a boundary's mesh carries its own
            // size in its UVs — see `FoliageKit::hedge_leaves`.
            foliage.hedge_leaves(1.0, materials.as_mut()),
        ));
        // The mask covers the town and a margin round it, and the margin is
        // what makes the clamped edge safe: past it there are no streets, so
        // the border reads as open country and the sampler is free to extend
        // that to the horizon. The same reach the terrain's level field uses,
        // and for the same reason.
        let started = std::time::Instant::now();
        let mask = images.add(urban_mask(&city, reach));
        // And what the town's floor is made of. The scanned grit if it was
        // downloaded and the painted roof grain if it was not — both are the
        // same thing at the scale a yard is seen from, which is a surface of
        // small stones. Sampled in the grass's own UVs, so it needs no second
        // tiling and no second set of coordinates.
        let yard = library
            .get(material::set::ROOF)
            .map(|scanned| scanned.color.clone())
            .unwrap_or_else(|| images.add(texture::roof()));
        info!(
            "town mask rasterised in {:.1}ms at {}²",
            started.elapsed().as_secs_f32() * 1000.0,
            MASK_SIZE
        );
        commands.spawn((
            Name::new("Ground"),
            Mesh3d(meshes.add(tiled_ground(GROUND_VIEW_EXTENT, GRASS_TILE, &terrain))),
            // Not a plain material. A grass scan is right at the size it was
            // photographed and identical at every size above that, and this one
            // quad is kilometres across — see `world::ground`.
            MeshMaterial3d(grounds.add(ground::GroundMaterial {
                base: landscape(&library, images.as_mut()),
                extension: ground::GroundBreakup::plain(mask, yard, reach),
            })),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        commands.spawn((
            Name::new("Ground collider"),
            RigidBody::Static,
            ground_collider(&terrain, played),
            Transform::IDENTITY,
        ));
        commands.insert_resource(terrain);
        return;
    }
    commands.spawn((
        Name::new("Road surface"),
        Mesh3d(meshes.add(tiled_ground(GROUND_VIEW_EXTENT, ASPHALT_TILE, &terrain))),
        // Not registered with `WetSurfaces` any more. The road's wetness is a
        // uniform its own shader reads, so it varies across the surface instead
        // of being one value recomputed onto the material — see `world::road`.
        MeshMaterial3d(roads.add(road::RoadMaterial {
            base: road_material(&library, images.as_mut(), ASPHALT_TILE),
            extension: road::RoadSheen::default(),
        })),
    ));
    // Still a box here, and it can be: a grid city's terrain is level
    // everywhere inside the played extent by construction — the landscape
    // starts past the collider — so a heightfield would be five hundred and
    // fifty rows of zeroes.
    commands.spawn((
        Name::new("Ground collider"),
        RigidBody::Static,
        Collider::cuboid(played, 2.0, played),
        Transform::from_xyz(0.0, -1.0, 0.0),
    ));
    commands.insert_resource(terrain);
}

/// Metres of ground one repeat of the grass covers.
///
/// Bigger than the material a park lawn is painted with, and it has to be: this
/// one quad is forty kilometres across. The park's tilings are picked to put a
/// slab at about a metre and a half over a patch a few tens of metres wide, and
/// the largest of them stretched over the whole world comes out at a repeat
/// more than a kilometre long — which is not grass, it is a green smear with
/// streaks in it. That is what the first pass at this looked like.
const GRASS_TILE: f32 = 3.5;

/// Texels a side of the town mask — see [`urban_mask`].
///
/// A thousand and twenty-four over a town two and a half kilometres across is
/// a texel every two and a bit metres, which is finer than the softest edge in
/// the mask by a factor of fifteen. It is a map of where the town is, not a
/// texture of the ground: nothing in it needs to resolve a kerb.
const MASK_SIZE: u32 = 1024;

/// How far behind a pavement the ground still belongs to the street.
///
/// A yard, a forecourt, the strip of grit somebody parks a van on. Thirty-odd
/// metres is about as deep as the back land of a European town goes before it
/// turns into whatever the middle of the block is; it is also, not by accident,
/// a little more than the deepest plot `streetside` hands out.
const BACKLAND: f32 = 34.0;

/// And how far out is still town at all.
///
/// The gap between two streets in Landshut is about a hundred and fifty metres,
/// so a hundred and forty reaches the middle of a block from both sides and
/// runs out well before the next town over. This is the term that stops the
/// inside of a block from being a meadow.
const ENVELOPE: f32 = 140.0;

/// Where the town is, rasterised once at startup.
///
/// Red is the back land: the ground immediately behind a pavement, including
/// the wedge at an oblique corner that no pavement covers. Green is the
/// built-up envelope — anywhere a block interior could be. `ground.wgsl` reads
/// both and shades the plain from them, which is the only way one material can
/// draw a market square and a field two kilometres out and mean something
/// different by each.
///
/// A pure function of the road graph, so a pure function of `(seed, style,
/// atlas)`: the mask is part of the layout, not part of the streaming, and it
/// is built once for a world that regenerates its chunks from the same three
/// inputs. Nothing here draws from an RNG.
///
/// Returned with a mip chain, because the plain is seen at every angle from
/// standing height to a rooftop and a mask without one shimmers exactly where
/// the ground goes flat to the eye.
fn urban_mask(city: &citygen::CityLayout, reach: f32) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = MASK_SIZE as usize;
    let per_texel = reach * 2.0 / MASK_SIZE as f32;
    let at = |index: usize| -reach + (index as f32 + 0.5) * per_texel;

    // Stamped, not sampled: walking every texel and asking it for its distance
    // to every street is a thousand-squared times two thousand, which is two
    // thousand million. Each street instead visits only the texels its own
    // corridor plus the back land can reach, which for a twenty-metre segment
    // is about a thousand of them.
    let mut near = vec![0.0f32; size * size];
    for edge in city.graph.edges() {
        let a = city.graph.node(edge.a).pos;
        let b = city.graph.node(edge.b).pos;
        // The corridor is the carriageway and its pavements — the ground that
        // is already paved. Everything measured here is measured from the far
        // side of that.
        let corridor = edge.width * 0.5 + citygen::SIDEWALK_WIDTH;
        let span = corridor + BACKLAND;
        let low = (a.min(b) - Vec2::splat(span) + Vec2::splat(reach)) / per_texel;
        let high = (a.max(b) + Vec2::splat(span) + Vec2::splat(reach)) / per_texel;
        let x0 = (low.x.floor().max(0.0) as usize).min(size - 1);
        let x1 = (high.x.ceil().max(0.0) as usize).min(size - 1);
        let z0 = (low.y.floor().max(0.0) as usize).min(size - 1);
        let z1 = (high.y.ceil().max(0.0) as usize).min(size - 1);
        let run = b - a;
        let length_squared = run.length_squared().max(1e-6);
        for z in z0..=z1 {
            let world_z = at(z);
            for x in x0..=x1 {
                let point = Vec2::new(at(x), world_z);
                let t = ((point - a).dot(run) / length_squared).clamp(0.0, 1.0);
                let beyond = (point.distance(a + run * t) - corridor).max(0.0);
                // Linear rather than smooth: this field is blurred twice below
                // to make the envelope, and a smoothstep here would only be
                // smoothed again. The shader puts the curve back on.
                let value = 1.0 - (beyond / BACKLAND).min(1.0);
                let cell = &mut near[z * size + x];
                *cell = cell.max(value);
            }
        }
    }

    // The envelope is the back land seen from far enough away that individual
    // streets stop mattering. Two box blurs rather than one: a single box
    // leaves square corners on the town, which show up on the ground as
    // straight edges nothing in the world explains.
    let radius = ((ENVELOPE * 0.5) / per_texel).round().max(1.0) as usize;
    let mut envelope = near.clone();
    for _ in 0..2 {
        blur(&mut envelope, size, radius);
    }

    let mut data = vec![0u8; size * size * 4];
    for index in 0..size * size {
        let x = index % size;
        let z = index / size;
        // The outermost ring is forced empty. The sampler clamps at the edge,
        // so whatever the border says is what the ground says from there to
        // the horizon — and a street that happens to end on the border would
        // otherwise paint a yard forty kilometres long.
        let border = x == 0 || z == 0 || x == size - 1 || z == size - 1;
        let (a, b) = if border {
            (0.0, 0.0)
        } else {
            // The envelope is an average, so its range depends on how dense
            // the streets are rather than on anything absolute; the two ends
            // here are read off Landshut — a block interior lands near a tenth
            // and open country lands at nothing.
            (
                near[index],
                ((envelope[index] - 0.015) / 0.13).clamp(0.0, 1.0),
            )
        };
        data[index * 4] = texture::byte(a);
        data[index * 4 + 1] = texture::byte(b);
        data[index * 4 + 3] = 255;
    }

    // The mip chain, the same way `texture::painted_rect` builds one, but in
    // linear: this is a mask and averaging it in sRGB would bend it.
    let mut level = data.clone();
    let (mut width, mut height) = (MASK_SIZE, MASK_SIZE);
    let mut levels = 1;
    while width > 1 || height > 1 {
        let (next, nw, nh) = texture::downsample(&level, width, height, false);
        data.extend_from_slice(&next);
        level = next;
        width = nw;
        height = nh;
        levels += 1;
    }

    let mut image = Image::new_uninit(
        Extent3d {
            width: MASK_SIZE,
            height: MASK_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        // Clamped, not repeated. Every other texture in this project tiles;
        // this one is a map, and a map that wrapped would put a second town
        // beyond the edge of the first.
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 4,
        ..default()
    });
    image
}

/// One separable box blur, in place, with running sums.
///
/// Running sums because the radius is fifty-odd texels and the naive form is
/// that many taps per texel per axis; this is two adds and a subtract however
/// wide the kernel is.
fn blur(field: &mut [f32], size: usize, radius: usize) {
    let window = (radius * 2 + 1) as f32;
    let mut scratch = vec![0.0f32; field.len()];
    // Horizontal.
    for row in 0..size {
        let base = row * size;
        let mut sum: f32 = 0.0;
        for x in 0..=radius.min(size - 1) {
            sum += field[base + x];
        }
        for x in 0..size {
            scratch[base + x] = sum / window;
            let leaving = x as isize - radius as isize;
            if leaving >= 0 {
                sum -= field[base + leaving as usize];
            }
            let arriving = x + radius + 1;
            if arriving < size {
                sum += field[base + arriving];
            }
        }
    }
    // And vertical.
    for column in 0..size {
        let mut sum: f32 = 0.0;
        for z in 0..=radius.min(size - 1) {
            sum += scratch[z * size + column];
        }
        for z in 0..size {
            field[z * size + column] = sum / window;
            let leaving = z as isize - radius as isize;
            if leaving >= 0 {
                sum -= scratch[leaving as usize * size + column];
            }
            let arriving = z + radius + 1;
            if arriving < size {
                sum += scratch[arriving * size + column];
            }
        }
    }
}

/// What is between the streets when the streets are not carved out of asphalt.
///
/// Only a town read off a map needs this. The generator's ground is the road
/// surface itself and its blocks cover everything else, so it never has any
/// bare ground to show.
fn landscape(library: &material::MaterialLibrary, images: &mut Assets<Image>) -> StandardMaterial {
    let mut lawn = StandardMaterial {
        // No `uv_transform`: the tiling is in the mesh, where the numbers stay
        // small enough for an `f32` to tell one texel from the next. See
        // [`CELL_REPEATS`].
        // Grass is not wet-registered on purpose, the same as a park's: rain
        // darkens it and does not polish it, and the polish is the whole of
        // what `WetSurfaces` does.
        perceptual_roughness: 1.0,
        ..default()
    };
    match library.get(material::set::GRASS) {
        Some(scanned) => {
            scanned.apply(&mut lawn);
            // The tint is not optional, and this is the surface that proved
            // it. `apply` leaves `base_color` white, so for as long as this
            // line was missing the largest surface in the game was drawn at a
            // daylit photograph's own albedo — two and a half times too bright
            // and five times too saturated. The town read as buildings
            // standing in a field of poster paint, which is exactly the
            // complaint the flat roofs raised once already; see
            // `ground::TURF_TINT`.
            lawn.base_color = ground::TURF_TINT;
        }
        None => {
            lawn.base_color = ground::TURF_PAINT;
            lawn.base_color_texture = Some(images.add(texture::grass()));
        }
    }
    lawn
}

/// The gravel a yard behind a gap in the frontage is laid with.
///
/// The roof set doing duty as grit, the same way the gravelled carriageways
/// use it — a chipping is a chipping, and the alternative was a twelfth
/// download for a surface that is only ever seen at a glancing angle. Darker
/// than the road version, because a yard nobody sweeps is darker than a lane
/// somebody drives on.
fn forecourt(library: &material::MaterialLibrary, images: &mut Assets<Image>) -> StandardMaterial {
    let mut grit = StandardMaterial {
        perceptual_roughness: 0.96,
        ..default()
    };
    match library.get(material::set::ROOF) {
        Some(scanned) => {
            scanned.apply(&mut grit);
            // Darker than the gravelled *carriageway* built from the same
            // set. A lane somebody drives on is swept by its own traffic; a
            // yard behind a gap in a terrace is not, and the first pass at
            // this read as a pale concrete slab dropped on the ground rather
            // than as part of it.
            // `Gravel023` measures a mean linear albedo of 0.70 — it is
            // white chippings photographed in the sun, the brightest scan in
            // the library by a factor of three — so this has to come down a
            // long way before a yard stops reading as a drift of snow —
            // measured off the render, one tint down from here still came back
            // at one and six tenths of the carriageway beside it. This lands
            // at about one and a fifth, which is where a gravel yard belongs:
            // paler than the tarmac, darker than the pavement slabs.
            grit.base_color = Color::srgb(0.295, 0.272, 0.232);
        }
        None => {
            grit.base_color = Color::srgb(0.29, 0.275, 0.24);
            grit.base_color_texture = Some(images.add(texture::roof()));
            grit.normal_map_texture = Some(images.add(texture::roof_normal()));
        }
    }
    grit
}

/// And what a boundary wall across that gap is built of.
///
/// Rough concrete rather than the pavement's slabs: a garden wall and a
/// footway made of one material is the tell that neither was chosen, and the
/// rough set is the only one in the library that reads as blockwork somebody
/// rendered themselves.
fn boundary_wall(
    library: &material::MaterialLibrary,
    images: &mut Assets<Image>,
) -> StandardMaterial {
    let mut wall = StandardMaterial {
        perceptual_roughness: 0.94,
        ..default()
    };
    match library.get(material::set::CONCRETE_ROUGH) {
        Some(scanned) => {
            scanned.apply(&mut wall);
            wall.base_color = Color::srgb(0.55, 0.53, 0.50);
        }
        None => {
            wall.base_color = Color::srgb(0.46, 0.45, 0.42);
            wall.base_color_texture = Some(images.add(texture::paving()));
            wall.normal_map_texture = Some(images.add(texture::paving_normal()));
        }
    }
    wall
}

/// Metres of street one repeat of each paving covers.
///
/// Derived from the scan rather than guessed, because guessing is what put the
/// Altstadt in 3 cm setts. Count the stones across one repeat, multiply by what
/// that stone is in life, and that is the number:
///
/// * `SETT_TILE` — `PavingStones138` runs about 7.5 stones across and 14.8 down
///   a 1024x2048 scan. At 16 cm Grosspflaster that is 1.20 m by 2.37 m. It is
///   the one portrait scan in the library, so it is also the one tile that
///   cannot be square: scaled isotropically its stones come out twice as long
///   as they are wide.
/// * `SLAB_TILE` — `PavingStones151` runs about 42 stones and 2.5 fan arcs
///   across a square repeat. At 3.4 m a stone is 8 cm and a Segmentbogen chord
///   is 1.36 m, which is a real laying pattern; at the old 1.6 m it was 3.8 cm
///   and half an arc, which is not.
///
/// Grit is looser: what it has to avoid is reading as a pattern, and the repeat
/// is what does that.
const SETT_TILE: Vec2 = Vec2::new(1.20, 2.37);
const SLAB_TILE: Vec2 = Vec2::splat(3.4);
pub(crate) const GRIT_TILE: f32 = 2.1;

/// The carriageway material for one kind of surface, and how much asphalt
/// ageing it takes.
///
/// A ribbon's UVs already tile at [`ASPHALT_TILE`], because that is what the
/// mesh was built for and one mesh serves whatever is laid on it. So a paving
/// with a different true size arrives as a `uv_transform` on top — which is
/// also the only way it could arrive, since the same ribbon may be resurfaced
/// by nothing more than a change to a tag in the extract.
fn carriageway(
    surface: atlas::Surface,
    library: &material::MaterialLibrary,
    images: &mut Assets<Image>,
) -> road::RoadMaterial {
    use atlas::Surface;

    // Set, tile, tint, how much asphalt ageing it takes, how coarse its relief
    // is, and how deep its deepest joint really is in metres — see
    // `ScannedSet::deepen`. Zero there means no parallax at all, which is not
    // an omission: it is a fetch budget spent where there is something to find.
    let (set, tile, tint, wear, relief, joint) = match surface {
        // Tarmac is the one surface the ageing in `road.wgsl` describes: it is
        // poured, so it is patched and it cracks. It is also the finest, which
        // is why its relief is the one that has to lie down at a grazing angle
        // — and why it is the one surface here with no parallax. Its relief is
        // a few millimetres of chipping, which is under a pixel at any range
        // you can see the road from, and it covers more of the screen than
        // everything else in this match put together.
        Surface::Asphalt => (
            material::set::ROAD,
            Vec2::splat(ASPHALT_TILE),
            Color::srgb(0.50, 0.50, 0.52),
            1.0,
            0.0,
            0.0,
        ),
        // A sett is a rounded granite block with a sand joint around it, and
        // it is the surface a player stands closest to in the whole Altstadt.
        // This is the case parallax was worth loading the height maps for, and
        // at 16 cm the joint is deep enough to be worth marching a ray into.
        Surface::Sett => (
            material::set::SETT,
            SETT_TILE,
            Color::srgb(0.62, 0.61, 0.60),
            0.0,
            1.0,
            0.035,
        ),
        Surface::Slabs => (
            material::set::PAVEMENT,
            SLAB_TILE,
            Color::srgb(0.66, 0.65, 0.63),
            0.0,
            0.65,
            0.016,
        ),
        Surface::Gravel => (
            material::set::ROOF,
            Vec2::splat(GRIT_TILE),
            Color::srgb(0.55, 0.51, 0.45),
            0.0,
            0.45,
            0.020,
        ),
    };

    let mut base = StandardMaterial {
        uv_transform: Affine2::from_scale(Vec2::new(ASPHALT_TILE / tile.x, ASPHALT_TILE / tile.y)),
        ..default()
    };
    match library.get(set) {
        Some(scanned) => {
            scanned.apply(&mut base);
            if joint > 0.0 {
                // The narrow axis: parallax depth is a fraction of a repeat and
                // an oblong tile has two of them, so take the one that makes
                // the ray shorter rather than the one that flatters it.
                scanned.deepen(&mut base, joint, tile.x.min(tile.y));
            }
            base.base_color = tint;
        }
        None => {
            // Every painted fallback is square, so it takes a square tile —
            // the oblong one belongs to the scan it was measured off. The
            // narrow axis, because that is the one the stone size came from:
            // `texture::SETTS` cobbles across 1.20 m is 17 cm, which is the
            // same Grosspflaster the scanned path now lays.
            base.uv_transform = Affine2::from_scale(Vec2::splat(ASPHALT_TILE / tile.x.min(tile.y)));
            let (color, relief) = match surface {
                Surface::Sett => (texture::cobbles(), texture::cobbles_normal()),
                Surface::Gravel => (texture::roof(), texture::roof_normal()),
                // Asphalt's own painted fallback is the one `road_material`
                // has always had; slabs borrow the pavement's.
                Surface::Asphalt => (texture::asphalt(), texture::asphalt_normal()),
                Surface::Slabs => (texture::paving(), texture::paving_normal()),
            };
            base.base_color = tint;
            base.base_color_texture = Some(images.add(color));
            base.normal_map_texture = Some(images.add(relief));
            base.perceptual_roughness = 0.94;
        }
    }

    road::RoadMaterial {
        base,
        extension: road::RoadSheen {
            settings: road::RoadSettings {
                wear,
                relief,
                ..default()
            },
        },
    }
}

/// The asphalt, scanned if it was downloaded and painted if it was not.
fn road_material(
    library: &material::MaterialLibrary,
    images: &mut Assets<Image>,
    size: f32,
) -> StandardMaterial {
    let mut asphalt = StandardMaterial {
        uv_transform: Affine2::from_scale(Vec2::splat(size / ASPHALT_TILE)),
        ..default()
    };

    match library.get(material::set::ROAD) {
        Some(scanned) => {
            scanned.apply(&mut asphalt);
            // The scan is a bright, freshly-laid surface photographed in
            // daylight; dropped into a street canyon it reads as concrete. The
            // tint is the one liberty taken with it, and only in value.
            asphalt.base_color = Color::srgb(0.50, 0.50, 0.52);
        }
        None => {
            // Real asphalt sits near 0.08 linear. Much under that looks like
            // tarmac at midday and like a hole in the world under a street
            // lamp: too little comes back to show a pool at all.
            asphalt.base_color = Color::srgb(0.31, 0.31, 0.325);
            asphalt.base_color_texture = Some(images.add(texture::asphalt()));
            asphalt.normal_map_texture = Some(images.add(texture::asphalt_normal()));
            asphalt.perceptual_roughness = 0.96;
        }
    }
    asphalt
}
