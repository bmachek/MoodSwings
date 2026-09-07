//! Turning city layout data into meshes, materials and colliders.
//!
//! Every building, kerb and roof shares ONE unit-cube mesh handle and is sized
//! by its transform; every block top shares one unit quad. Bevy batches
//! entities that share a mesh *and* material handle into a single draw call, so
//! the city costs roughly one draw call per material rather than one per
//! building.
//!
//! Texturing widens that material table, and it is worth being honest about the
//! trade. A facade needs its window grid to match the building's height, and
//! that is a property of the entity, not the material — so buildings are
//! bucketed into four height classes and the table becomes
//! `districts x palette x class`, about eighty materials instead of twenty.
//! Eighty draw calls for an entire city is still nothing; eighty *thousand*
//! would not be, which is why the bucket count stays small and fixed.
//!
//! The cube mesh is built here rather than taken from `Cuboid`, whose UVs are
//! rotated a quarter turn on the ±X faces and flipped on -Z. That is invisible
//! on noise and glaring on a window grid: two walls of every building would
//! have had their floors running vertically.
//!
//! The collider is a unit cube too: Avian scales colliders by the entity's
//! global transform, so the same scale drives visual and physical size and the
//! two can never drift apart.

use avian3d::prelude::*;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};

use super::citygen::{Block, Building, District, PALETTE_SIZE, Quarter};
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;

use super::rooftop::{self, RoofKit};
use super::shell::{self, ShellKit};
use super::texture::{self, FacadeClass};

/// Pavement height above the road surface.
pub const SIDEWALK_HEIGHT: f32 = 0.28;

/// Height of the plinth course at the foot of every building, in metres.
///
/// Buildings met the pavement at a bare edge, which is the join a real street
/// never has: there is always a plinth, a step, a stall riser or at minimum a
/// change of material, and its shadow line is what makes a wall look like it is
/// *standing on* the ground rather than pushed into it.
const PLINTH_HEIGHT: f32 = 0.62;
/// How far the plinth stands proud of the wall above it.
const PLINTH_PROUD: f32 = 0.11;
/// How far away the plinth stops being drawn, before `lod_scale`.
///
/// It is eleven centimetres deep. Past a couple of hundred metres that is well
/// under a pixel, and all it contributes is another edge for the anti-aliasing
/// to chew on.
const PLINTH_RANGE: f32 = 260.0;

/// Roughly how wide a paving slab or a patch of grass should be, in metres.
const GROUND_TILE: f32 = 2.6;
/// Tiling factors block tops are quantised to, so they can share materials.
const GROUND_BUCKETS: [f32; 4] = [8.0, 12.0, 17.0, 24.0];

const CLASS_COUNT: usize = FacadeClass::ALL.len();

#[derive(Resource)]
pub struct CityAssets {
    pub unit_cube: Handle<Mesh>,
    /// A 1x1 quad in the XZ plane, laid over each block as its walking surface.
    unit_quad: Handle<Mesh>,
    /// Indexed by `(district_index * PALETTE_SIZE + palette) * CLASS_COUNT + class`.
    building: Vec<Handle<super::facade::FacadeMaterial>>,
    roof: Handle<StandardMaterial>,
    kerb: Handle<StandardMaterial>,
    park_kerb: Handle<StandardMaterial>,
    /// One per entry in [`GROUND_BUCKETS`].
    paving: Vec<Handle<StandardMaterial>>,
    grass: Vec<Handle<StandardMaterial>>,
    /// A four-sided unit cone — the church spire's pyramid, built once here
    /// because chunks respawn and a mesh added per spawn would leak.
    spire: Handle<Mesh>,
}

/// Marks which chunk an entity belongs to, so streaming can despawn it.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct ChunkOf(pub IVec2);

fn district_index(district: District) -> usize {
    match district {
        District::Downtown => 0,
        District::Midtown => 1,
        District::Residential => 2,
        District::Industrial => 3,
        District::Park => 4,
    }
}

/// Four tones per district, tinting a shared greyscale facade. Blocks of
/// colour are still the art direction; the texture only says where the windows
/// are.
fn palette(district: District) -> [Color; PALETTE_SIZE as usize] {
    match district {
        District::Downtown => [
            Color::srgb(0.42, 0.48, 0.58),
            Color::srgb(0.33, 0.40, 0.51),
            Color::srgb(0.53, 0.58, 0.63),
            Color::srgb(0.28, 0.35, 0.45),
        ],
        District::Midtown => [
            Color::srgb(0.61, 0.58, 0.53),
            Color::srgb(0.50, 0.48, 0.46),
            Color::srgb(0.69, 0.65, 0.58),
            Color::srgb(0.44, 0.43, 0.43),
        ],
        District::Residential => [
            Color::srgb(0.71, 0.58, 0.48),
            Color::srgb(0.77, 0.69, 0.56),
            Color::srgb(0.60, 0.47, 0.39),
            Color::srgb(0.66, 0.61, 0.52),
        ],
        District::Industrial => [
            Color::srgb(0.48, 0.46, 0.42),
            Color::srgb(0.56, 0.45, 0.36),
            Color::srgb(0.39, 0.40, 0.41),
            Color::srgb(0.51, 0.49, 0.44),
        ],
        District::Park => [Color::srgb(0.30, 0.44, 0.26); PALETTE_SIZE as usize],
    }
}

/// The quarters' own palettes, overriding the district's where a block sits
/// in one. This is most of what makes a quarter legible from the street:
/// the same city, suddenly wearing somebody else's colours.
fn quarter_palette(quarter: Quarter) -> [Color; PALETTE_SIZE as usize] {
    match quarter {
        Quarter::Italia => [
            Color::srgb(0.72, 0.44, 0.30),
            Color::srgb(0.80, 0.62, 0.38),
            Color::srgb(0.84, 0.74, 0.58),
            Color::srgb(0.62, 0.33, 0.26),
        ],
        Quarter::Fernost => [
            Color::srgb(0.60, 0.20, 0.16),
            Color::srgb(0.78, 0.62, 0.28),
            Color::srgb(0.36, 0.52, 0.42),
            Color::srgb(0.48, 0.46, 0.44),
        ],
    }
}

/// Which district's wall grain a quarter borrows. There are six scanned wall
/// sets in the whole game; a quarter recolours, it does not re-photograph.
fn quarter_grain(quarter: Quarter) -> District {
    match quarter {
        Quarter::Italia => District::Residential,
        Quarter::Fernost => District::Midtown,
    }
}

/// Material-table group index: the five districts first, then the quarters.
fn quarter_index(quarter: Quarter) -> usize {
    5 + match quarter {
        Quarter::Italia => 0,
        Quarter::Fernost => 1,
    }
}

/// A unit cube whose six faces all agree about which way is up.
///
/// Side faces run U along their horizontal axis and V from the bottom edge to
/// the top; the top and bottom map U to X and V to Z. Without that, a facade
/// texture arrives sideways on two walls out of four.
fn unit_cube_mesh() -> Mesh {
    // (position, normal, uv), four vertices per face.
    let faces: [([f32; 3], [f32; 3], [f32; 2]); 24] = [
        // +Z
        ([-0.5, -0.5, 0.5], [0.0, 0.0, 1.0], [0.0, 0.0]),
        ([0.5, -0.5, 0.5], [0.0, 0.0, 1.0], [1.0, 0.0]),
        ([0.5, 0.5, 0.5], [0.0, 0.0, 1.0], [1.0, 1.0]),
        ([-0.5, 0.5, 0.5], [0.0, 0.0, 1.0], [0.0, 1.0]),
        // -Z
        ([0.5, -0.5, -0.5], [0.0, 0.0, -1.0], [0.0, 0.0]),
        ([-0.5, -0.5, -0.5], [0.0, 0.0, -1.0], [1.0, 0.0]),
        ([-0.5, 0.5, -0.5], [0.0, 0.0, -1.0], [1.0, 1.0]),
        ([0.5, 0.5, -0.5], [0.0, 0.0, -1.0], [0.0, 1.0]),
        // +X
        ([0.5, -0.5, 0.5], [1.0, 0.0, 0.0], [0.0, 0.0]),
        ([0.5, -0.5, -0.5], [1.0, 0.0, 0.0], [1.0, 0.0]),
        ([0.5, 0.5, -0.5], [1.0, 0.0, 0.0], [1.0, 1.0]),
        ([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], [0.0, 1.0]),
        // -X
        ([-0.5, -0.5, -0.5], [-1.0, 0.0, 0.0], [0.0, 0.0]),
        ([-0.5, -0.5, 0.5], [-1.0, 0.0, 0.0], [1.0, 0.0]),
        ([-0.5, 0.5, 0.5], [-1.0, 0.0, 0.0], [1.0, 1.0]),
        ([-0.5, 0.5, -0.5], [-1.0, 0.0, 0.0], [0.0, 1.0]),
        // +Y
        ([-0.5, 0.5, 0.5], [0.0, 1.0, 0.0], [0.0, 0.0]),
        ([0.5, 0.5, 0.5], [0.0, 1.0, 0.0], [1.0, 0.0]),
        ([0.5, 0.5, -0.5], [0.0, 1.0, 0.0], [1.0, 1.0]),
        ([-0.5, 0.5, -0.5], [0.0, 1.0, 0.0], [0.0, 1.0]),
        // -Y
        ([-0.5, -0.5, -0.5], [0.0, -1.0, 0.0], [0.0, 0.0]),
        ([0.5, -0.5, -0.5], [0.0, -1.0, 0.0], [1.0, 0.0]),
        ([0.5, -0.5, 0.5], [0.0, -1.0, 0.0], [1.0, 1.0]),
        ([-0.5, -0.5, 0.5], [0.0, -1.0, 0.0], [0.0, 1.0]),
    ];

    let indices: Vec<u32> = (0..6u32)
        .flat_map(|face| {
            let base = face * 4;
            [base, base + 1, base + 2, base + 2, base + 3, base]
        })
        .collect();

    Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        faces.iter().map(|(p, _, _)| *p).collect::<Vec<_>>(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        faces.iter().map(|(_, n, _)| *n).collect::<Vec<_>>(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        faces.iter().map(|(_, _, uv)| *uv).collect::<Vec<_>>(),
    )
    .with_inserted_indices(Indices::U32(indices))
}

/// Adds a mikktspace tangent basis, or leaves the mesh alone and says so.
///
/// A missing tangent attribute makes the normal-mapped pipeline fail to build
/// rather than fall back, so a loud warning beats a silently black city.
pub fn with_tangents(mut mesh: Mesh) -> Mesh {
    if let Err(error) = mesh.generate_tangents() {
        warn!("no tangents for a mesh, normal maps will be wrong: {error}");
    }
    mesh
}

/// Nearest tiling factor that puts ground tiles near [`GROUND_TILE`] across a
/// surface `extent` metres wide.
fn ground_bucket(extent: f32) -> usize {
    let wanted = (extent / GROUND_TILE).max(1.0);
    GROUND_BUCKETS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - wanted).abs().total_cmp(&(*b - wanted).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    library: &super::material::MaterialLibrary,
    facades_out: &mut Assets<super::facade::FacadeMaterial>,
    wet: &mut super::weather::WetSurfaces,
) -> CityAssets {
    let districts = [
        District::Downtown,
        District::Midtown,
        District::Residential,
        District::Industrial,
        District::Park,
    ];

    // One set of facade maps per height class, shared by every district that
    // has a building of that height.
    let facades: Vec<_> = FacadeClass::ALL
        .iter()
        .map(|&class| {
            let maps = texture::facade(class);
            (
                images.add(maps.base),
                images.add(maps.emissive),
                images.add(maps.surface),
                images.add(maps.normal),
            )
        })
        .collect();

    // The five districts' groups first, then one group per quarter — the
    // same `group * PALETTE_SIZE + palette` addressing throughout, which is
    // what `material_for` and `quarter_index` agree on.
    let groups: Vec<([Color; PALETTE_SIZE as usize], District)> = districts
        .iter()
        .map(|&district| (palette(district), district))
        .chain(
            [Quarter::Italia, Quarter::Fernost]
                .into_iter()
                .map(|quarter| (quarter_palette(quarter), quarter_grain(quarter))),
        )
        .collect();

    let mut building = Vec::with_capacity(groups.len() * PALETTE_SIZE as usize * CLASS_COUNT);
    for (colors, grain_district) in groups {
        for (slot, color) in colors.into_iter().enumerate() {
            // The grain is the district's, but how it is dressed — scale, and
            // whether it is turned — belongs to the palette slot, so a street
            // of one district is not a street of one photograph.
            let grain = super::facade::FacadeGrain::for_district(library, grain_district, slot);
            for (&class, (base, emissive, surface, normal)) in FacadeClass::ALL.iter().zip(&facades)
            {
                building.push(facades_out.add(super::facade::FacadeMaterial {
                    base: StandardMaterial {
                        base_color: color,
                        base_color_texture: Some(base.clone()),
                        emissive_texture: Some(emissive.clone()),
                        // Dark until dusk; `timeofday::light_windows` drives it.
                        emissive: LinearRgba::BLACK,
                        metallic_roughness_texture: Some(surface.clone()),
                        normal_map_texture: Some(normal.clone()),
                        perceptual_roughness: 1.0,
                        metallic: 1.0,
                        ..default()
                    },
                    extension: grain.clone().for_class(class),
                }));
            }
        }
    }

    // Painted stand-ins, made whether or not they end up used: the scanned
    // library decides per surface, and a set can be present for the pavement
    // and missing for the grass.
    let paving_texture = images.add(texture::paving());
    let paving_relief = images.add(texture::paving_normal());
    let grass_texture = images.add(texture::grass());

    let mut paving = Vec::with_capacity(GROUND_BUCKETS.len());
    let mut grass = Vec::with_capacity(GROUND_BUCKETS.len());
    for tiling in GROUND_BUCKETS {
        let uv_transform = Affine2::from_scale(Vec2::splat(tiling));

        let mut slabs = StandardMaterial {
            uv_transform,
            ..default()
        };
        match library.get(super::material::set::PAVEMENT) {
            Some(scanned) => scanned.apply(&mut slabs),
            None => {
                slabs.base_color = Color::srgb(0.50, 0.50, 0.52);
                slabs.base_color_texture = Some(paving_texture.clone());
                slabs.normal_map_texture = Some(paving_relief.clone());
                slabs.perceptual_roughness = 0.95;
            }
        }
        let (dry_color, dry_roughness) = (slabs.base_color, slabs.perceptual_roughness);
        let handle = materials.add(slabs);
        // Pavements soak like the road does. Grass does not — wet grass is
        // darker but no glossier, and the shine is the whole point here.
        wet.add(handle.clone(), dry_color, dry_roughness);
        paving.push(handle);

        let mut lawn = StandardMaterial {
            uv_transform,
            ..default()
        };
        match library.get(super::material::set::GRASS) {
            Some(scanned) => scanned.apply(&mut lawn),
            None => {
                lawn.base_color = Color::srgb(0.29, 0.43, 0.24);
                lawn.base_color_texture = Some(grass_texture.clone());
                lawn.perceptual_roughness = 1.0;
            }
        }
        grass.push(materials.add(lawn));
    }

    let mut tar = StandardMaterial {
        // Roofs are only ever seen from a distance, so one tiling suits all.
        uv_transform: Affine2::from_scale(Vec2::splat(6.0)),
        ..default()
    };
    match library.get(super::material::set::ROOF) {
        Some(scanned) => scanned.apply(&mut tar),
        None => {
            tar.base_color = Color::srgb(0.38, 0.38, 0.40);
            tar.base_color_texture = Some(images.add(texture::roof()));
            tar.normal_map_texture = Some(images.add(texture::roof_normal()));
            tar.perceptual_roughness = 0.96;
        }
    }

    CityAssets {
        // Normal mapping needs a tangent basis, and mikktspace is the one the
        // shader agrees with; hand-written tangents are how normal maps end up
        // lit from the wrong side on two faces out of six.
        unit_cube: meshes.add(with_tangents(unit_cube_mesh())),
        unit_quad: meshes.add(with_tangents(
            Plane3d::default().mesh().size(1.0, 1.0).build(),
        )),
        building,
        roof: materials.add(tar),
        kerb: materials.add(StandardMaterial {
            base_color: Color::srgb(0.50, 0.50, 0.51),
            perceptual_roughness: 0.95,
            ..default()
        }),
        park_kerb: materials.add(StandardMaterial {
            base_color: Color::srgb(0.33, 0.31, 0.26),
            perceptual_roughness: 1.0,
            ..default()
        }),
        paving,
        grass,
        spire: meshes.add(Cone::new(1.0, 1.0).mesh().resolution(4).build()),
    }
}

impl CityAssets {
    fn material_for(
        &self,
        district: District,
        quarter: Option<Quarter>,
        palette: u8,
        class: FacadeClass,
    ) -> Handle<super::facade::FacadeMaterial> {
        let group = match quarter {
            Some(quarter) => quarter_index(quarter),
            None => district_index(district),
        };
        let slot = group * PALETTE_SIZE as usize + palette as usize;
        let i = slot * CLASS_COUNT + class.index();
        self.building[i.min(self.building.len() - 1)].clone()
    }

    /// Every facade material, for the day/night cycle to light up.
    pub fn building_materials(&self) -> &[Handle<super::facade::FacadeMaterial>] {
        &self.building
    }

    /// The kerb concrete, for structures that are honestly made of it.
    pub fn concrete(&self) -> Handle<StandardMaterial> {
        self.kerb.clone()
    }

    /// The tarred-roof material, for anything that wants to read as roofing.
    pub fn roof_material(&self) -> Handle<StandardMaterial> {
        self.roof.clone()
    }

    /// The unit pyramid the church spires scale from.
    pub fn spire(&self) -> Handle<Mesh> {
        self.spire.clone()
    }

    /// The block paving tiled for a surface `extent` metres across — the
    /// garage decks wear the same slabs the block tops do.
    pub fn paving_for(&self, extent: f32) -> Handle<StandardMaterial> {
        self.paving[ground_bucket(extent)].clone()
    }
}

/// Spawns one block's pavement and buildings, tagged for streaming.
/// Everything spawning a block needs beyond the block itself.
///
/// A struct rather than five more positional arguments: the roofs need the
/// world seed to be reproducible and the level-of-detail scale to know how far
/// to draw, and threading those through as bare parameters was already the
/// point at which the call became unreadable.
pub struct BlockContext<'a> {
    pub assets: &'a CityAssets,
    pub roofs: &'a RoofKit,
    pub shells: &'a ShellKit,
    pub signs: &'a crate::world::signage::SignKit,
    pub lots: &'a crate::world::lots::LotKit,
    pub statues: &'a crate::world::statues::StatueKit,
    pub stadium: &'a crate::world::stadium::StadiumKit,
    pub interior: &'a crate::world::interior::InteriorKit,
    /// `None` only before the bank has landed — streaming simply spawns that
    /// chunk's emitters never, which resolves itself on the next re-entry.
    pub bank: Option<&'a crate::audio::bank::SoundBank>,
    /// `None` only before the figure and face kits have landed — an interior
    /// spawned that early simply opens without its staff.
    pub cast: Option<crate::world::interior::CastContext<'a>>,
    pub seed: u64,
    pub lod_scale: f32,
}

pub fn spawn_block(commands: &mut Commands, ctx: &BlockContext, block: &Block, chunk: IVec2) {
    let assets = ctx.assets;
    let area = block.area;
    let size = area.size();
    let center = area.center();
    let park = block.district == District::Park;

    // The kerb slab gets the collider: at 28cm it is a step the player walks up
    // onto, and without one they would stand sunk into it. One static box per
    // block is cheap.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(assets.unit_cube.clone()),
        MeshMaterial3d(if park {
            assets.park_kerb.clone()
        } else {
            assets.kerb.clone()
        }),
        Transform::from_xyz(center.x, SIDEWALK_HEIGHT * 0.5, center.y).with_scale(Vec3::new(
            size.x,
            SIDEWALK_HEIGHT,
            size.y,
        )),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
    ));

    // The walking surface is a separate quad rather than the top of the slab,
    // so paving can tile at a metre or two while the kerb face beside it stays
    // plain concrete instead of a stack of squashed slabs.
    let bucket = ground_bucket((size.x + size.y) * 0.5);
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(assets.unit_quad.clone()),
        MeshMaterial3d(if park {
            assets.grass[bucket].clone()
        } else {
            assets.paving[bucket].clone()
        }),
        // A few millimetres proud of the slab, which is enough to settle the
        // depth test without being visible from standing height.
        Transform::from_xyz(center.x, SIDEWALK_HEIGHT + 0.004, center.y)
            .with_scale(Vec3::new(size.x, 1.0, size.y)),
    ));

    for building in &block.buildings {
        spawn_building(commands, ctx, block, building, chunk);
    }
    for vacant in &block.vacants {
        crate::world::lots::spawn_lot(commands, ctx.lots, ctx.signs, block, vacant, chunk);
    }
    if park {
        crate::world::statues::spawn(commands, ctx.statues, ctx.seed, block, chunk);
    }

    // The places that make a sound of their own. Emitters stream with the
    // block like everything else here; `audio::sfx::tend_emitters` decides
    // which few of them are actually audible.
    if let Some(bank) = ctx.bank {
        use crate::audio::sfx::{AmbienceEmitter, gain};
        let emitter = |commands: &mut Commands,
                       at: Vec2,
                       height: f32,
                       sound: &Handle<crate::audio::synth::SynthSound>,
                       loudness: f32| {
            commands.spawn((
                ChunkOf(chunk),
                Transform::from_xyz(at.x, height, at.y),
                bevy::audio::AudioPlayer(sound.clone()),
                // Muted until `tend_emitters` ranks it, so the first frame
                // cannot blare — the vehicle voices' trick.
                bevy::audio::PlaybackSettings::LOOP
                    .with_spatial(true)
                    .muted(),
                AmbienceEmitter { gain: loudness },
            ));
        };

        if park {
            emitter(commands, center, 4.0, &bank.birdsong, gain::PARK_BIRDS);
        }
        // An industrial block hums as a block: the drone belongs to the
        // district, not to any one shed, so one emitter at the centre reads
        // as "this whole street works for a living".
        if block.district == District::Industrial {
            emitter(commands, center, 3.0, &bank.industry, gain::INDUSTRY);
        }
        // A quarter's tune hangs over its blocks the same way — this is the
        // zone-ambience variation RNG key 16 was reserved for, though it
        // turned out to need a wedge of geometry rather than a die roll.
        if let Some(quarter) = block.quarter {
            let air = match quarter {
                Quarter::Italia => &bank.mandolin,
                Quarter::Fernost => &bank.guzheng,
            };
            emitter(commands, center, 3.0, air, gain::QUARTER);
        }
        for building in &block.buildings {
            if building.kind == super::citygen::BuildingKind::Restaurant {
                emitter(
                    commands,
                    building.footprint.center(),
                    2.2,
                    &bank.chatter,
                    gain::CHATTER,
                );
            }
            // The bowl roars. The same recording the demos march under —
            // a crowd is a crowd; only the occasion differs.
            if building.kind == super::citygen::BuildingKind::Stadium {
                emitter(
                    commands,
                    building.footprint.center(),
                    6.0,
                    &bank.uproar,
                    gain::STADIUM,
                );
            }
        }
        for vacant in &block.vacants {
            match vacant.purpose {
                super::citygen::VacantUse::GasStation => emitter(
                    commands,
                    vacant.rect.center(),
                    2.5,
                    &bank.forecourt,
                    gain::FORECOURT,
                ),
                // Ball height, roughly: the dribble should come off the
                // tarmac, not hover over the fence.
                super::citygen::VacantUse::Court => emitter(
                    commands,
                    vacant.rect.center(),
                    1.2,
                    &bank.court,
                    gain::COURT,
                ),
                // A market sounds like a restaurant with the walls removed,
                // which is what it is.
                super::citygen::VacantUse::Market => emitter(
                    commands,
                    vacant.rect.center(),
                    2.0,
                    &bank.chatter,
                    gain::CHATTER,
                ),
                _ => {}
            }
        }
    }
}

fn spawn_building(
    commands: &mut Commands,
    ctx: &BlockContext,
    block: &Block,
    building: &Building,
    chunk: IVec2,
) {
    let district = block.district;
    let assets = ctx.assets;
    let size = building.footprint.size();
    let center = building.footprint.center();
    let height = building.height;
    let class = FacadeClass::for_height(height);

    // One seed for everything about this building's roof, derived from where it
    // stands. Chunks regenerate on re-entry, so anything keyed on spawn order
    // would give the same building a different roof each time.
    let seed = rooftop::seed_for(ctx.seed, building.footprint);
    let parapet = rooftop::parapet(seed, class);

    // Which face fronts the street: the one nearest the block perimeter,
    // which is the side with a pavement under it. Decided once, up here,
    // because three things hang off it — the sign, the doorway, and the room
    // behind the doorway — and they must all agree which way is out.
    use std::f32::consts::{FRAC_PI_2, PI};
    let footprint = building.footprint;
    let gaps = [
        footprint.min.x - block.area.min.x,
        block.area.max.x - footprint.max.x,
        footprint.min.y - block.area.min.y,
        block.area.max.y - footprint.max.y,
    ];
    let front = gaps
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map(|(side, _)| side)
        .unwrap_or(3);
    let yaw = match front {
        0 => -FRAC_PI_2,
        1 => FRAC_PI_2,
        2 => PI,
        _ => 0.0,
    };

    // The wall, at three levels of detail. All three carry the same transform
    // and the same material, and `use_aabb: false` measures from the entity's
    // origin, so all three measure the same distance and hand over to one
    // another on precisely the same metre — which is what Bevy needs before it
    // will dither one into the next instead of blinking between them.
    let material = assets.material_for(district, block.quarter, building.palette, class);
    let (near, far) = shell::ranges(ctx.lod_scale);
    // Which balconies and which awnings, from the building's own seed rather
    // than from a counter, for the same reason its roof is.
    let variant = (seed >> 19) as u32;
    // A quarter picks its restaurants' chain for them: every dining room in
    // Klein-Neapel is the Pizzeria, every one in the Fernost-Viertel is the
    // Wok — which is how real quarters advertise themselves, one cuisine
    // repeated until it is a neighbourhood. Only the sign is forced; the
    // shell variant stays the building's own, so the street still varies.
    let sign_variant = match (block.quarter, building.kind) {
        (Some(Quarter::Italia), super::citygen::BuildingKind::Restaurant) => 1,
        (Some(Quarter::Fernost), super::citygen::BuildingKind::Restaurant) => 3,
        _ => variant,
    };

    // An enterable kind gets the shell with the doorway carved into its +Z
    // face — so the whole wall stack is turned to put +Z on the front, with
    // the scale axes swapped to match. A plain building keeps the unrotated
    // transform it always had; rotating it too would be free, but a diff
    // that moves every wall in the city to open a few doors is not.
    let door_shell = if building.kind.enterable() {
        ctx.shells.door(class, variant)
    } else {
        None
    };
    let (frontage, throat) = if front < 2 {
        (size.y, size.x)
    } else {
        (size.x, size.y)
    };
    // The parking garage is not a facade with an inside implied — it has no
    // facade at all. Its whole structure comes from `world::garage`, plus
    // the sign over its mouth, and nothing else of a building's anatomy
    // applies: no shells, no roof slab, no plinth, no rooftop clutter.
    // The stadium owns its whole structure: bowl, stands, crowd, wave.
    if building.kind == super::citygen::BuildingKind::Stadium {
        super::stadium::spawn(
            commands,
            assets,
            ctx.stadium,
            seed,
            center,
            frontage,
            throat,
            yaw,
            chunk,
        );
        hang_sign(
            commands,
            ctx,
            building,
            sign_variant,
            front,
            yaw,
            frontage,
            SIDEWALK_HEIGHT + 4.6,
            chunk,
        );
        return;
    }
    // The church replaces its box the same way the garage does: the whole
    // structure comes from `world::church`, plus the sign on the nave.
    if building.kind == super::citygen::BuildingKind::Church {
        super::church::spawn(
            commands, assets, center, frontage, throat, height, yaw, chunk,
        );
        hang_sign(
            commands,
            ctx,
            building,
            sign_variant,
            front,
            yaw,
            // The board hangs on the tower, which is much narrower than the
            // footprint — the same clamp the tower's own side length uses.
            (frontage * 0.32).min(5.5),
            SIDEWALK_HEIGHT + 3.9,
            chunk,
        );
        return;
    }
    if building.kind == super::citygen::BuildingKind::ParkingGarage {
        super::garage::spawn(
            commands, assets, center, frontage, throat, height, yaw, chunk,
        );
        hang_sign(
            commands,
            ctx,
            building,
            sign_variant,
            front,
            yaw,
            frontage,
            SIDEWALK_HEIGHT + 3.6,
            chunk,
        );
        return;
    }

    let wall = if door_shell.is_some() {
        Transform::from_xyz(center.x, height * 0.5 + SIDEWALK_HEIGHT, center.y)
            .with_rotation(Quat::from_rotation_y(yaw))
            .with_scale(Vec3::new(frontage, height, throat))
    } else {
        Transform::from_xyz(center.x, height * 0.5 + SIDEWALK_HEIGHT, center.y)
            .with_scale(Vec3::new(size.x, height, size.y))
    };

    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(
            door_shell
                .clone()
                .unwrap_or_else(|| ctx.shells.get(class, shell::Detail::Full, variant)),
        ),
        MeshMaterial3d(material.clone()),
        wall,
        VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: shell::handover(near),
            use_aabb: false,
        },
    ));
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(ctx.shells.get(class, shell::Detail::Coarse, variant)),
        MeshMaterial3d(material.clone()),
        wall,
        VisibilityRange {
            start_margin: shell::handover(near),
            end_margin: shell::handover(far),
            use_aabb: false,
        },
    ));
    // The plain box — and, for a sealed building, the collider with it,
    // deliberately on the level of detail that is never culled by *distance*,
    // only by being close. A visibility range hides a mesh and does not touch
    // its collider, so the building stays solid at every distance; putting it
    // anywhere else would work today and break the first time these ranges
    // are reordered.
    let mut far_box = commands.spawn((
        ChunkOf(chunk),
        Mesh3d(assets.unit_cube.clone()),
        MeshMaterial3d(material),
        wall,
        VisibilityRange {
            start_margin: shell::handover(far),
            end_margin: f32::INFINITY..f32::INFINITY,
            use_aabb: false,
        },
    ));
    if door_shell.is_none() {
        // Unit cube: Avian scales it by the transform above.
        far_box.insert((RigidBody::Static, Collider::cuboid(1.0, 1.0, 1.0)));
    } else {
        // An enterable building cannot be a scaled cube: its collider needs a
        // doorway. The compound stands on its own unscaled entity — its
        // plates are metric, and scaling them by the transform would turn
        // the door gap into a fraction of whatever the building measures.
        commands.spawn((
            ChunkOf(chunk),
            RigidBody::Static,
            enterable_collider(class, frontage, throat, height),
            Transform::from_xyz(center.x, SIDEWALK_HEIGHT, center.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
        ));
        crate::world::interior::spawn(
            commands,
            ctx.interior,
            ctx.cast.as_ref(),
            building.kind,
            &crate::world::interior::Doorframe {
                center,
                yaw,
                width: frontage,
                depth: throat,
                height,
                class,
            },
            seed,
            chunk,
            ctx.lod_scale,
        );
    }

    // A capping slab, slightly oversailing the walls. It hides the windowed top
    // face of the cube, and the overhang reads as a parapet from street level —
    // which is most of what stops a box looking like a box. Visual only: the
    // wall collider already reaches this high.
    //
    // Its proportions come from the building's own seed rather than from a
    // constant. That costs nothing — the slab was already an entity with its
    // own transform — and it is the only variation in the roofline that still
    // reads from a kilometre up, where the clutter below is sub-pixel.
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(assets.unit_cube.clone()),
        MeshMaterial3d(assets.roof.clone()),
        Transform::from_xyz(
            center.x,
            height + SIDEWALK_HEIGHT + parapet.thickness * 0.5,
            center.y,
        )
        .with_scale(Vec3::new(
            size.x + parapet.overhang * 2.0,
            parapet.thickness,
            size.y + parapet.overhang * 2.0,
        )),
    ));

    // The plinth course. Shares the kerb material on purpose — the base of a
    // building and the kerb in front of it are the two things at street level
    // that take the most abuse, and in most cities they are the same stone.
    // Not on an enterable building: a course wrapping all four walls would
    // bar the doorway with sixty centimetres of stone, and a shopfront meets
    // the pavement glass-to-ground anyway.
    let plinth_draw = (PLINTH_RANGE * ctx.lod_scale).max(1.0);
    if door_shell.is_none() {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(assets.unit_cube.clone()),
            MeshMaterial3d(assets.kerb.clone()),
            Transform::from_xyz(center.x, SIDEWALK_HEIGHT + PLINTH_HEIGHT * 0.5, center.y)
                .with_scale(Vec3::new(
                    size.x + PLINTH_PROUD * 2.0,
                    PLINTH_HEIGHT,
                    size.y + PLINTH_PROUD * 2.0,
                )),
            VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin: (plinth_draw * 0.9)..plinth_draw,
                use_aabb: false,
            },
            // The wall behind it casts the same shadow from the same place. A
            // second caster eleven centimetres in front buys nothing and costs
            // a pass over every building in the city.
            NotShadowCaster,
        ));
    }

    // The sign, for any kind that hangs one, centred on the fascia band the
    // facade painter reserves over the ground storey.
    let storey = height / class.grid().1;
    hang_sign(
        commands,
        ctx,
        building,
        sign_variant,
        front,
        yaw,
        frontage,
        SIDEWALK_HEIGHT + storey * 0.875,
        chunk,
    );

    // An advertising poster on a blind side wall, for the anonymous kinds
    // only. A supermarket advertising over its own sign is clutter; a block
    // of flats renting its gable out is a business model. Placement comes
    // off the building's own seed like everything else about it, from bits
    // the roof and the sign variant are not already using.
    use super::citygen::BuildingKind;
    if matches!(
        building.kind,
        BuildingKind::Apartments | BuildingKind::Offices
    ) && height >= 12.0
        && (seed >> 27) & 0b111 < 3
    {
        let (mesh, material, poster) = ctx.signs.advert((seed >> 33) as u32);
        // The two faces perpendicular to the front are the blind ones; one
        // seed bit picks which. The poster must fit the wall it is pasted
        // to with paper to spare, or it wraps the corner.
        let side = if front < 2 {
            2 + ((seed >> 41) & 1) as usize
        } else {
            ((seed >> 41) & 1) as usize
        };
        let side_width = if side < 2 { size.y } else { size.x };
        let fit = (side_width * 0.55 / poster.x)
            .min(height * 0.38 / poster.y)
            .min(1.0);
        if fit > 0.45 {
            let proud = 0.14;
            let at = match side {
                0 => Vec2::new(footprint.min.x - proud, center.y),
                1 => Vec2::new(footprint.max.x + proud, center.y),
                2 => Vec2::new(center.x, footprint.min.y - proud),
                _ => Vec2::new(center.x, footprint.max.y + proud),
            };
            let side_yaw = match side {
                0 => -FRAC_PI_2,
                1 => FRAC_PI_2,
                2 => PI,
                _ => 0.0,
            };
            let poster_draw = (crate::world::signage::RANGE * ctx.lod_scale).max(1.0);
            commands.spawn((
                ChunkOf(chunk),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(at.x, SIDEWALK_HEIGHT + height * 0.6, at.y)
                    .with_rotation(Quat::from_rotation_y(side_yaw))
                    .with_scale(Vec3::splat(fit)),
                VisibilityRange {
                    start_margin: 0.0..0.0,
                    end_margin: (poster_draw * 0.9)..poster_draw,
                    use_aabb: false,
                },
                NotShadowCaster,
            ));
        }
    }

    // The painted ground storey, for the sealed civic kinds: fire-station
    // roller doors, the town hall's pilasters, the taped-shut police door,
    // the barracks gate. A quad stretched across the front face, standing
    // just proud of the wall — under the sign, over the shop glass the
    // class would otherwise paint there. It clears the plinth's band by
    // starting above it, and stops under the fascia the sign hangs on.
    if let Some((mesh, material)) = ctx.signs.frontage(building.kind) {
        let proud = 0.14;
        let at = match front {
            0 => Vec2::new(footprint.min.x - proud, center.y),
            1 => Vec2::new(footprint.max.x + proud, center.y),
            2 => Vec2::new(center.x, footprint.min.y - proud),
            _ => Vec2::new(center.x, footprint.max.y + proud),
        };
        let foot = SIDEWALK_HEIGHT + PLINTH_HEIGHT + 0.02;
        let top = SIDEWALK_HEIGHT + storey * texture::FASCIA.0 - 0.05;
        let strip = (top - foot).max(1.2);
        let sign_draw = (crate::world::signage::RANGE * ctx.lod_scale).max(1.0);
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(at.x, foot + strip * 0.5, at.y)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::new(frontage * 0.96, strip, 1.0)),
            VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin: (sign_draw * 0.9)..sign_draw,
                use_aabb: false,
            },
            NotShadowCaster,
        ));
    }

    // And what accumulated on the deck. Sits on top of the slab, so nothing is
    // buried in it and nothing floats over it.
    rooftop::spawn(
        commands,
        ctx.roofs,
        ChunkOf(chunk),
        center,
        height + SIDEWALK_HEIGHT + parapet.thickness,
        &rooftop::plan(seed, building.footprint, class),
        ctx.lod_scale,
    );
}

/// Hangs a building's sign on its front face, `fascia` metres up, scaled
/// down if the board would outgrow the wall it is bolted to. It goes on the
/// face nearest the block perimeter — the side the lot fronts, which is the
/// side with a pavement under it — the same front the doorway and the room
/// behind it chose.
#[allow(clippy::too_many_arguments)]
fn hang_sign(
    commands: &mut Commands,
    ctx: &BlockContext,
    building: &Building,
    variant: u32,
    front: usize,
    yaw: f32,
    frontage: f32,
    fascia: f32,
    chunk: IVec2,
) {
    let Some((mesh, material, board)) = ctx.signs.get(building.kind, variant) else {
        return;
    };
    let footprint = building.footprint;
    let center = footprint.center();
    let proud = crate::world::signage::PROUD;
    let at = match front {
        0 => Vec2::new(footprint.min.x - proud, center.y),
        1 => Vec2::new(footprint.max.x + proud, center.y),
        2 => Vec2::new(center.x, footprint.min.y - proud),
        _ => Vec2::new(center.x, footprint.max.y + proud),
    };
    let fit = (frontage * 0.8 / board.x).min(1.0);
    let sign_draw = (crate::world::signage::RANGE * ctx.lod_scale).max(1.0);
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_xyz(at.x, fascia, at.y)
            .with_rotation(Quat::from_rotation_y(yaw))
            .with_scale(Vec3::splat(fit)),
        VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: (sign_draw * 0.9)..sign_draw,
            use_aabb: false,
        },
        // The wall behind it casts the same shadow from the same place.
        NotShadowCaster,
    ));
}

/// The collider for a building the player can walk into, in the door frame:
/// `x` across the front, `+z` towards the street, `y` up from the pavement.
///
/// Six plates instead of one scaled cube: the back and side walls, the two
/// front segments either side of the doorway, and one slab covering
/// everything above the door head — which seals the room's ceiling and keeps
/// the upper storeys solid against airborne traffic in a single shape. The
/// gap between the front segments is `shell::door_width`, the same number the
/// doored shell carved out — that agreement is the whole contract.
fn enterable_collider(class: FacadeClass, width: f32, depth: f32, height: f32) -> Collider {
    use crate::world::interior::WALL;
    let head = shell::door_head(class, height);
    let door = shell::door_width(class, width);
    let flank = (width - door) * 0.5;

    let mut plates: Vec<(Vec3, Quat, Collider)> = vec![
        (
            Vec3::new(0.0, (head + height) * 0.5, 0.0),
            Quat::IDENTITY,
            Collider::cuboid(width, height - head, depth),
        ),
        (
            Vec3::new(0.0, head * 0.5, -(depth - WALL) * 0.5),
            Quat::IDENTITY,
            Collider::cuboid(width, head, WALL),
        ),
    ];
    for side in [-1.0f32, 1.0] {
        plates.push((
            Vec3::new(side * (width - WALL) * 0.5, head * 0.5, 0.0),
            Quat::IDENTITY,
            Collider::cuboid(WALL, head, depth),
        ));
        plates.push((
            Vec3::new(
                side * (door + flank) * 0.5,
                head * 0.5,
                (depth - WALL) * 0.5,
            ),
            Quat::IDENTITY,
            Collider::cuboid(flank, head, WALL),
        ));
    }
    Collider::compound(plates)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ray through the middle of the doorway passes; the same ray a door's
    /// width to the side does not. Asked of the collider itself, because the
    /// shell's own test only vouches for the mesh.
    #[test]
    fn the_compound_collider_keeps_the_doorway_open() {
        let (width, depth, height) = (18.0, 16.0, 20.0);
        let class = FacadeClass::for_height(height);
        let collider = enterable_collider(class, width, depth, height);

        // Each ray starts a building-depth out front and stops at the middle
        // of the room — long enough to pierce the front wall, short enough
        // not to report the back wall as a bricked-up door.
        let waist = shell::door_head(class, height) * 0.5;
        let through = collider.intersects_ray(
            Vec3::ZERO,
            Quat::IDENTITY,
            Vec3::new(0.0, waist, depth),
            Vec3::NEG_Z,
            depth,
        );
        assert!(!through, "the doorway is bricked up");

        let beside = collider.intersects_ray(
            Vec3::ZERO,
            Quat::IDENTITY,
            Vec3::new(shell::door_width(class, width) * 1.5, waist, depth),
            Vec3::NEG_Z,
            depth,
        );
        assert!(beside, "the front wall beside the door is missing");

        // And over the head the building is sealed again.
        let above = collider.intersects_ray(
            Vec3::ZERO,
            Quat::IDENTITY,
            Vec3::new(0.0, shell::door_head(class, height) + 1.0, depth),
            Vec3::NEG_Z,
            depth,
        );
        assert!(above, "there is a hole over the door head");
    }

    #[test]
    fn the_cube_faces_all_agree_about_up() {
        let mesh = unit_cube_mesh();
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap();
        let uvs = mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap();
        let (positions, uvs) = match (positions, uvs) {
            (
                bevy::render::mesh::VertexAttributeValues::Float32x3(p),
                bevy::render::mesh::VertexAttributeValues::Float32x2(u),
            ) => (p, u),
            _ => panic!("unexpected attribute formats"),
        };

        // The four side faces come first. On every one of them, V must climb
        // with Y, or facades land sideways or upside down.
        for vertex in 0..16 {
            let y = positions[vertex][1];
            let v = uvs[vertex][1];
            assert_eq!(
                y > 0.0,
                v > 0.5,
                "side vertex {vertex} has V running against Y"
            );
        }
    }

    #[test]
    fn ground_buckets_pick_the_nearest_tiling() {
        // A 20m block wants about 8 tiles across; a 60m block wants 23.
        assert_eq!(ground_bucket(20.0), 0);
        assert_eq!(ground_bucket(62.0), GROUND_BUCKETS.len() - 1);
        // Anything degenerate still has to land in range.
        for extent in [0.0, 0.5, 5.0, 400.0] {
            assert!(ground_bucket(extent) < GROUND_BUCKETS.len());
        }
    }

    #[test]
    fn every_district_palette_and_class_addresses_a_distinct_material() {
        let districts = [
            District::Downtown,
            District::Midtown,
            District::Residential,
            District::Industrial,
            District::Park,
        ];
        let mut seen = std::collections::HashSet::new();
        for district in districts {
            for palette in 0..PALETTE_SIZE {
                for class in FacadeClass::ALL {
                    let slot = district_index(district) * PALETTE_SIZE as usize + palette as usize;
                    assert!(
                        seen.insert(slot * CLASS_COUNT + class.index()),
                        "material index collision at {district:?}/{palette}/{class:?}"
                    );
                }
            }
        }
        // The quarters address their own groups past the districts' five,
        // colliding with nothing.
        for quarter in [Quarter::Italia, Quarter::Fernost] {
            for palette in 0..PALETTE_SIZE {
                for class in FacadeClass::ALL {
                    let slot = quarter_index(quarter) * PALETTE_SIZE as usize + palette as usize;
                    assert!(
                        seen.insert(slot * CLASS_COUNT + class.index()),
                        "material index collision at {quarter:?}/{palette}/{class:?}"
                    );
                }
            }
        }
        assert_eq!(
            seen.len(),
            (districts.len() + 2) * PALETTE_SIZE as usize * CLASS_COUNT
        );
    }

    #[test]
    fn the_quarters_are_wedges_somewhere_in_the_middle_ring() {
        use super::super::citygen::{Quarter, quarter_for};
        // Sweep the ring on two seeds: both quarters must exist, must not
        // overlap, and must leave most of the city unthemed.
        for seed in [0xA17E_5EED_u64, 2709413613] {
            let mut italia = 0;
            let mut fernost = 0;
            let mut plain = 0;
            for i in 0..720 {
                let angle = i as f32 / 720.0 * std::f32::consts::TAU;
                let at = Vec2::new(angle.cos(), angle.sin()) * 470.0;
                match quarter_for(seed, at) {
                    Some(Quarter::Italia) => italia += 1,
                    Some(Quarter::Fernost) => fernost += 1,
                    None => plain += 1,
                }
            }
            assert!(italia > 0, "seed {seed:#x} has no Klein-Neapel");
            assert!(fernost > 0, "seed {seed:#x} has no Fernost-Viertel");
            assert!(
                plain > (italia + fernost) * 2,
                "seed {seed:#x} is more theme park than city"
            );
            // And the wedges stop at the rings: downtown and the far belt
            // stay themselves.
            assert!(quarter_for(seed, Vec2::new(50.0, 0.0)).is_none());
            assert!(quarter_for(seed, Vec2::new(900.0, 0.0)).is_none());
        }
    }
}
