//! Scanned PBR materials, and what to do when they are not there.
//!
//! The city's surfaces come from photogrammetry sets under `assets/materials/`,
//! fetched by `tools/fetch-materials.sh`. They are CC0, so nothing is owed to
//! anybody for shipping them — that licence is why these particular sets were
//! chosen over better-looking ones.
//!
//! Three things this module exists to get right:
//!
//! * **Missing is not broken.** The download is 200 MB and gitignored, so a
//!   fresh clone has none of it. Every lookup returns an `Option`, and the
//!   caller falls back to the procedural version in [`super::texture`]. The
//!   game looks worse and runs fine.
//! * **Colour space per map.** Only the colour map is sRGB. Loading roughness
//!   or a normal map through the sRGB curve is the classic way to get walls
//!   that are subtly, inexplicably wrong, so those are loaded linear.
//! * **Mip chains.** Bevy has no runtime mip generator and the loaders produce
//!   a single level. A 2K texture tiled a few hundred times across the ground
//!   without mips does not shimmer, it boils. So every loaded map gets a chain
//!   built on the CPU the frame it arrives.
//! * **De-lighting.** A colour map off a photograph is not an albedo map: it is
//!   an albedo map with the day the photograph was taken multiplied into it. See
//!   [`DELIGHT`].

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

/// Where the fetch script puts things, relative to `assets/`.
const ROOT: &str = "materials";
/// Resolution and encoding the fetch script asks for; it is part of the
/// filename, so it has to agree with the script.
const VARIANT: &str = "2K-JPG";

/// The scanned sets the city knows how to use.
///
/// Adding one here and to `tools/fetch-materials.sh` is the whole job; the
/// surface that wants it then asks the library by name.
pub mod set {
    pub const ROAD: &str = "Asphalt031";
    pub const PAVEMENT: &str = "PavingStones138";
    pub const ROOF: &str = "Gravel023";
    pub const GRASS: &str = "Grass005";

    // Walls. Six of them, because one set dressed four ways still leaves every
    // brick building in the city cut from the same photograph — and a facade
    // is the surface the player spends the most time looking at.
    pub const CONCRETE: &str = "Concrete034";
    pub const CONCRETE_ROUGH: &str = "Concrete046";
    pub const BRICK: &str = "Bricks097";
    pub const BRICK_PALE: &str = "Bricks104";
    pub const BRICK_OLD: &str = "Bricks075A";
    pub const PLASTER: &str = "PaintedPlaster006";

    pub const ALL: [&str; 10] = [
        ROAD,
        PAVEMENT,
        ROOF,
        GRASS,
        CONCRETE,
        CONCRETE_ROUGH,
        BRICK,
        BRICK_PALE,
        BRICK_OLD,
        PLASTER,
    ];
}

/// One scanned material's maps.
///
/// Occlusion is separate rather than packed because the sets ship it that way
/// and Bevy's `StandardMaterial` takes it in its own slot.
#[derive(Clone)]
pub struct ScannedSet {
    pub color: Handle<Image>,
    pub normal: Handle<Image>,
    pub roughness: Handle<Image>,
    pub occlusion: Option<Handle<Image>>,
}

impl ScannedSet {
    /// Points a material's texture slots at this set.
    ///
    /// The two multipliers matter. `perceptual_roughness` scales whatever the
    /// roughness map says, so anything but 1 quietly throws the scan away; and
    /// these sets ship roughness as its own greyscale map rather than packed
    /// glTF-style, which means its blue channel — where `StandardMaterial`
    /// looks for metalness — is a copy of the roughness. Zeroing the metallic
    /// multiplier is what stops wet-looking asphalt from reading as chrome.
    pub fn apply(&self, material: &mut StandardMaterial) {
        material.base_color_texture = Some(self.color.clone());
        material.normal_map_texture = Some(self.normal.clone());
        material.metallic_roughness_texture = Some(self.roughness.clone());
        material.occlusion_texture = self.occlusion.clone();
        material.perceptual_roughness = 1.0;
        material.metallic = 0.0;
    }
}

#[derive(Resource, Default)]
pub struct MaterialLibrary {
    sets: HashMap<&'static str, ScannedSet>,
    /// Maps still waiting for their mip chain. Emptied as they arrive.
    pending: HashSet<AssetId<Image>>,
}

impl MaterialLibrary {
    /// The set under `name`, or `None` if it was never downloaded.
    pub fn get(&self, name: &str) -> Option<&ScannedSet> {
        self.sets.get(name)
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }
}

pub struct MaterialLibraryPlugin;

impl Plugin for MaterialLibraryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialLibrary>()
            // Before `WorldPlugin`'s startup systems, which read the library to
            // decide what each surface is made of.
            .add_systems(PreStartup, discover)
            .add_systems(Update, finish_loaded_maps);
    }
}

/// Path of one map within a set, as the fetch script lays it out.
fn map_path(name: &str, map: &str) -> String {
    format!("{ROOT}/{name}/{name}_{VARIANT}_{map}.jpg")
}

/// Whether a set is actually on disk.
///
/// Checked against the filesystem rather than by trying to load and handling
/// the failure: `AssetServer` reports a missing file asynchronously, long after
/// the materials that need it have already been built.
fn present(name: &str, map: &str) -> bool {
    crate::core::assets::has(&map_path(name, map))
}

fn discover(asset_server: Res<AssetServer>, mut library: ResMut<MaterialLibrary>) {
    // Colour is the one map that is genuinely sRGB-encoded.
    let srgb = |path: String| asset_server.load(path);
    let linear = |path: String| {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.is_srgb = false;
                // The mip pass reads the pixels back, so they have to survive
                // the trip into the main world.
                settings.asset_usage = RenderAssetUsages::default();
            })
            .load(path)
    };

    for name in set::ALL {
        if !present(name, "Color") {
            continue;
        }

        let scanned = ScannedSet {
            color: srgb(map_path(name, "Color")),
            // GL, not DX: Bevy follows glTF, whose normal maps have +Y up.
            // Loading the DX variant flips every bump into a dent.
            normal: linear(map_path(name, "NormalGL")),
            roughness: linear(map_path(name, "Roughness")),
            occlusion: present(name, "AmbientOcclusion")
                .then(|| linear(map_path(name, "AmbientOcclusion"))),
        };

        library.pending.extend(
            [
                scanned.color.id(),
                scanned.normal.id(),
                scanned.roughness.id(),
            ]
            .into_iter()
            .chain(scanned.occlusion.as_ref().map(|h| h.id())),
        );
        library.sets.insert(name, scanned);
    }

    if library.is_empty() {
        info!("no scanned materials found; using procedural textures throughout");
    } else {
        info!(
            "{}/{} scanned material sets found in assets/{ROOT}",
            library.len(),
            set::ALL.len()
        );
    }
}

/// De-lights, mips and applies a tiling sampler as each map finishes loading.
///
/// The order is load-bearing: the mip chain has to be built from the corrected
/// texels, or every level below the top is a chain of averages of the wrong
/// image and a wall changes albedo as you walk away from it.
fn finish_loaded_maps(
    mut events: MessageReader<AssetEvent<Image>>,
    mut images: ResMut<Assets<Image>>,
    mut library: ResMut<MaterialLibrary>,
) {
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };
        if !library.pending.remove(id) {
            continue;
        }
        let Some(mut image) = images.get_mut(*id) else {
            continue;
        };
        if let Err(reason) = delight(&mut image) {
            warn!("a scanned colour map was not de-lit ({reason}); it will crush");
        }
        if let Err(reason) = add_mip_chain(&mut image) {
            warn!("no mip chain for a scanned map ({reason}); it will alias");
        }
        image.sampler = ImageSampler::Descriptor(tiling_sampler());
    }
}

/// How much of a scanned colour map's own contrast survives de-lighting.
///
/// A photogrammetry colour map is a photograph, and a photograph of a rough
/// surface has that surface's own shadows in it: the dark side of every chipping
/// in an asphalt scan, the shaded half of every stone, the soot in a mortar
/// joint. Used as an albedo those shadows get *multiplied by the renderer's own
/// lighting*, which shades the same relief a second time from a different sun.
///
/// The numbers say how far off it is. This asphalt scan's albedo runs from 0.006
/// to 0.9 in linear light — a hundred and fifty to one, across one material.
/// Real asphalt is closer to three to one. The rest is the photographer's day.
///
/// It never showed while a flat five hundred lux of ambient was propping the
/// shadows up. With that gone, the dark end of the scan crushed: a road three
/// metres in front of the camera came out as black salt-and-pepper over grey,
/// which reads as broken rendering rather than as tarmac.
///
/// So the colour is pulled back towards its own average. Not all the way — the
/// aggregate really is lighter than the bitumen and the mortar really is paler
/// than the brick — but far enough that what is left is albedo and what was
/// removed is weather. The relief it used to stand for is still there: it is in
/// the normal map and the occlusion map, where the renderer can light it.
const DELIGHT: f32 = 0.55;

/// Pulls a scanned colour map's contrast in towards its own mean, in place.
///
/// Only touches sRGB images, and that is not a heuristic: this module loads the
/// colour map through the sRGB curve and every other map linear, so the format
/// *is* the answer to "is this an albedo".
///
/// The compression happens in linear light rather than on the stored bytes,
/// because the thing being undone — a multiplication by the light — is a linear
/// operation, and halving it in gamma space would lighten the whole image as a
/// side effect.
fn delight(image: &mut Image) -> Result<(), &'static str> {
    if image.texture_descriptor.format != TextureFormat::Rgba8UnormSrgb {
        return Ok(());
    }
    if image.texture_descriptor.mip_level_count > 1 {
        return Err("already mipped");
    }
    let Some(data) = image.data.as_mut() else {
        return Err("pixels were dropped before the render world");
    };

    // A per-channel mean, so the correction is neutral: a single grey mean would
    // drag a warm brick towards its own luminance and cool it.
    let mut sum = [0.0f64; 3];
    for texel in data.as_chunks::<4>().0 {
        for channel in 0..3 {
            sum[channel] += super::texture::srgb_to_linear(texel[channel]) as f64;
        }
    }
    let texels = (data.len() / 4).max(1) as f64;
    let mean = sum.map(|s| (s / texels) as f32);

    for texel in data.as_chunks_mut::<4>().0 {
        for channel in 0..3 {
            let linear = super::texture::srgb_to_linear(texel[channel]);
            let pulled = mean[channel] + (linear - mean[channel]) * DELIGHT;
            texel[channel] = super::texture::byte(super::texture::linear_to_srgb(pulled));
        }
    }
    Ok(())
}

fn tiling_sampler() -> bevy::image::ImageSamplerDescriptor {
    bevy::image::ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        // The road and the pavement are seen almost edge-on almost always,
        // which is precisely where trilinear filtering turns to mud.
        anisotropy_clamp: 16,
        ..default()
    }
}

/// Appends every mip level below the loaded one, in place.
fn add_mip_chain(image: &mut Image) -> Result<(), &'static str> {
    if image.texture_descriptor.mip_level_count > 1 {
        return Ok(());
    }
    let format = image.texture_descriptor.format;
    let srgb = format == TextureFormat::Rgba8UnormSrgb;
    if !matches!(
        format,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
    ) {
        return Err("unexpected texture format");
    }

    let size = image.texture_descriptor.size;
    if size.depth_or_array_layers != 1 {
        return Err("not a plain 2D image");
    }
    let Some(data) = image.data.as_mut() else {
        return Err("pixels were dropped before the render world");
    };

    let (mut width, mut height) = (size.width, size.height);
    let mut level = data.clone();
    let mut levels = 1;
    while width > 1 || height > 1 {
        let (next, w, h) = super::texture::downsample(&level, width, height, srgb);
        data.extend_from_slice(&next);
        level = next;
        width = w;
        height = h;
        levels += 1;
    }
    image.texture_descriptor.mip_level_count = levels;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    #[test]
    fn map_paths_match_what_the_fetch_script_writes() {
        assert_eq!(
            map_path(set::ROAD, "Color"),
            "materials/Asphalt031/Asphalt031_2K-JPG_Color.jpg"
        );
    }

    #[test]
    fn a_mip_chain_is_appended_in_place() {
        let mut image = Image::new_uninit(
            Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.data = Some(vec![128; 4 * 4 * 4]);

        add_mip_chain(&mut image).expect("4x4 should mip");
        assert_eq!(image.texture_descriptor.mip_level_count, 3, "4, 2, then 1");
        // 16 + 4 + 1 texels, four bytes each.
        assert_eq!(image.data.as_ref().unwrap().len(), (16 + 4 + 1) * 4);
    }

    #[test]
    fn mipping_twice_is_a_no_op() {
        let mut image = Image::new_uninit(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        );
        image.data = Some(vec![200; 2 * 2 * 4]);

        add_mip_chain(&mut image).unwrap();
        let once = image.data.clone();
        add_mip_chain(&mut image).unwrap();
        assert_eq!(image.data, once, "a second pass must not stack more levels");
    }

    /// A colour map with a photographed hundred-to-one range in it comes back
    /// with a plausible one, without moving the average — the correction has to
    /// be about contrast alone or every wall in the city changes colour.
    #[test]
    fn de_lighting_narrows_a_scans_range_and_leaves_its_mean_where_it_was() {
        let mut image = Image::new_uninit(
            Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        // Two texels a long way apart: near black, and near white.
        image.data = Some(vec![20, 20, 20, 255, 230, 230, 230, 255]);

        let before = |image: &Image| {
            let data = image.data.as_ref().unwrap();
            (
                super::super::texture::srgb_to_linear(data[0]),
                super::super::texture::srgb_to_linear(data[4]),
            )
        };
        let (dark_was, light_was) = before(&image);
        delight(&mut image).unwrap();
        let (dark, light) = before(&image);

        assert!(light - dark < (light_was - dark_was) * 0.6, "not narrowed");
        assert!(dark > dark_was && light < light_was, "not narrowed inwards");
        // Within a byte's worth of the original average.
        let moved = ((dark + light) - (dark_was + light_was)).abs() / 2.0;
        assert!(moved < 0.01, "the average moved by {moved}");
    }

    /// Every other map in a set is a measurement rather than a photograph, and
    /// compressing a normal or a roughness map towards its mean would be
    /// vandalism. The format is what tells them apart.
    #[test]
    fn only_the_colour_map_is_de_lit() {
        let mut image = Image::new_uninit(
            Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        );
        image.data = Some(vec![20, 20, 20, 255, 230, 230, 230, 255]);
        let untouched = image.data.clone();

        delight(&mut image).unwrap();
        assert_eq!(image.data, untouched);
    }

    #[test]
    fn every_named_set_is_in_the_all_list() {
        for name in [
            set::ROAD,
            set::PAVEMENT,
            set::CONCRETE,
            set::BRICK,
            set::ROOF,
            set::GRASS,
        ] {
            assert!(set::ALL.contains(&name), "{name} missing from ALL");
        }
    }
}
