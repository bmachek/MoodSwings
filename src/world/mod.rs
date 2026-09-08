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
pub mod interior;
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
pub mod stadium;
pub mod statues;
pub mod streaming;
pub mod streetlights;
pub mod streetside;
pub mod texture;
pub mod timeofday;
pub mod vegetation;
pub mod weather;
pub mod worksite;

use avian3d::prelude::*;
use bevy::math::Affine2;
use bevy::prelude::*;

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
    mut wear: ResMut<Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    mut wet: ResMut<weather::WetSurfaces>,
) {
    let started = std::time::Instant::now();
    // A style that names a town builds that town; everything else is grown
    // from the seed. A named town that will not load falls back to the
    // generator with a warning rather than to an empty world — the same
    // discipline a missing sound gets.
    let mut layout = config
        .city
        .atlas()
        .and_then(atlas::load)
        .map(|town| atlas::layout(&town, config.world_seed, config.world.half_extent))
        .unwrap_or_else(|| {
            citygen::generate(config.world_seed, config.world.half_extent, config.city)
        });
    // A town read off a map arrives as a road network and nothing else. What
    // fills it is not blocks — a real block is not a rectangle — but frontages
    // marched down each side of each street; see `world::streetside`.
    if layout.blocks.is_empty() {
        layout.blocks = streetside::lots(&layout, config.world_seed, config.city);
    }

    info!(
        "city built in {:.1}ms: {} blocks, {} buildings, {} intersections, {} roads",
        started.elapsed().as_secs_f32() * 1000.0,
        layout.blocks.len(),
        layout.building_count(),
        layout.graph.node_count(),
        layout.graph.edge_count(),
    );

    river::spawn(&mut commands, &layout, &mut meshes, &mut materials);

    let city = City(layout);
    commands.insert_resource(streaming::ChunkIndex::build(&city));
    commands.insert_resource(city);
    commands.insert_resource(props::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(litter::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(bunting::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(plume::build_assets(&mut meshes, &mut materials));
    commands.insert_resource(gable::build_assets(&mut meshes, &mut materials));
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
    commands.insert_resource(vegetation::build_assets(&mut meshes, &mut materials));
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

/// The road surface. Streets are not meshed individually: the ground *is* the
/// asphalt, and the raised pavement slabs on each block carve the street grid
/// out of it as negative space. One quad instead of thousands of road polys.
///
/// That one quad is two kilometres across, so the asphalt tiles a few hundred
/// times over it. Everything that makes that survivable — a texture that wraps,
/// a mip chain, anisotropic filtering — lives in `texture`.
fn setup_ground(
    mut commands: Commands,
    config: Res<GameConfig>,
    library: Res<material::MaterialLibrary>,
    city: Res<City>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut roads: ResMut<Assets<road::RoadMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // A town read off a map runs the ground the other way round. The generator
    // covers the world in asphalt and lets its block slabs carve the streets
    // out of it as negative space — one quad for the whole city — and that only
    // works because its blocks tile the ground. A real town's do not, so the
    // world would be a tarmac plain with houses on it. Here the ground is
    // grass and the roads are laid on top of it, one ribbon per street.
    let streetside = city.blocks.first().is_some_and(|block| !block.paved);
    // The visible plane runs far past the city, so that from a rooftop the
    // world does not end in a rectangle hanging in mid-air; the atmosphere
    // hazes the surplus into the horizon within a couple of kilometres. It
    // costs one more quad. The collider only needs to cover the city.
    let played = config.world.half_extent * 2.0 + 200.0;
    let size = GROUND_VIEW_EXTENT;
    let sheet = meshes.add(buildings::with_tangents(
        Plane3d::default().mesh().size(size, size).build(),
    ));
    if streetside {
        // The asphalt a ribbon is made of: the same material the one big quad
        // would have used, at a tiling of one, because a ribbon carries its own
        // size in its UVs instead.
        let asphalt = roads.add(road::RoadMaterial {
            base: road_material(&library, images.as_mut(), ASPHALT_TILE),
            extension: road::RoadSheen::default(),
        });
        commands.insert_resource(streetside::build_ribbons(&city, meshes.as_mut(), asphalt));
        commands.spawn((
            Name::new("Ground"),
            Mesh3d(sheet),
            MeshMaterial3d(materials.add(landscape(&library, images.as_mut(), size))),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        commands.spawn((
            Name::new("Ground collider"),
            RigidBody::Static,
            Collider::cuboid(played, 2.0, played),
            Transform::from_xyz(0.0, -1.0, 0.0),
        ));
        return;
    }
    commands.spawn((
        Name::new("Road surface"),
        Mesh3d(sheet),
        // Not registered with `WetSurfaces` any more. The road's wetness is a
        // uniform its own shader reads, so it varies across the surface instead
        // of being one value recomputed onto the material — see `world::road`.
        MeshMaterial3d(roads.add(road::RoadMaterial {
            base: road_material(&library, images.as_mut(), size),
            extension: road::RoadSheen::default(),
        })),
    ));
    commands.spawn((
        Name::new("Ground collider"),
        RigidBody::Static,
        Collider::cuboid(played, 2.0, played),
        Transform::from_xyz(0.0, -1.0, 0.0),
    ));
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

/// What is between the streets when the streets are not carved out of asphalt.
///
/// Only a town read off a map needs this. The generator's ground is the road
/// surface itself and its blocks cover everything else, so it never has any
/// bare ground to show.
fn landscape(
    library: &material::MaterialLibrary,
    images: &mut Assets<Image>,
    size: f32,
) -> StandardMaterial {
    let mut lawn = StandardMaterial {
        uv_transform: Affine2::from_scale(Vec2::splat(size / GRASS_TILE)),
        // Grass is not wet-registered on purpose, the same as a park's: rain
        // darkens it and does not polish it, and the polish is the whole of
        // what `WetSurfaces` does.
        perceptual_roughness: 1.0,
        ..default()
    };
    match library.get(material::set::GRASS) {
        Some(scanned) => scanned.apply(&mut lawn),
        None => {
            lawn.base_color = Color::srgb(0.29, 0.43, 0.24);
            lawn.base_color_texture = Some(images.add(texture::grass()));
        }
    }
    lawn
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
