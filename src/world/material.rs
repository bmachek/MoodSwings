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
    /// The fine mosaic a German pavement is laid in — Mosaikpflaster, thumb-
    /// sized stones in fanned courses. Also what a square or a shopping street
    /// is surfaced with, which is why `Surface::Slabs` takes it too.
    pub const PAVEMENT: &str = "PavingStones151";
    pub const ROOF: &str = "Gravel023";
    pub const GRASS: &str = "Grass005";
    /// Kopfsteinpflaster: hand-sized granite setts with moss in the joints,
    /// which is what a fifth of Landshut's carriageways are tagged as and what
    /// the Altstadt is made of.
    ///
    /// These two used to be the other way round, and it is worth saying why it
    /// was wrong rather than just which is which now. 151 is a *mosaic* — some
    /// forty stones across one repeat — and it was carrying the Altstadt
    /// carriageway at a 1.35 m tile, which put a sett at 3.2 cm. Real
    /// Grosspflaster is 15 to 18. The Altstadt read as snakeskin, and the
    /// chunky scan that would have read as Kopfsteinpflaster was meanwhile on
    /// the pavement, stretched better than two to one because it is the one
    /// portrait scan in the library and every caller scaled it with a
    /// `Vec2::splat`.
    pub const SETT: &str = "PavingStones138";

    // Walls. Six of them, because one set dressed four ways still leaves every
    // brick building in the city cut from the same photograph — and a facade
    // is the surface the player spends the most time looking at.
    pub const CONCRETE: &str = "Concrete034";
    pub const CONCRETE_ROUGH: &str = "Concrete046";
    pub const BRICK: &str = "Bricks097";
    pub const BRICK_PALE: &str = "Bricks104";
    pub const BRICK_OLD: &str = "Bricks075A";
    pub const PLASTER: &str = "PaintedPlaster006";

    pub const ALL: [&str; 11] = [
        ROAD,
        PAVEMENT,
        ROOF,
        GRASS,
        SETT,
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
    /// The height field the scan was measured off, inverted for Bevy. See
    /// [`ScannedSet::deepen`], which is the only thing that uses it, and
    /// [`invert`] for why it cannot be used as it arrives.
    pub depth: Handle<Image>,
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
    ///
    /// # It does not set `base_color`, and the caller must
    ///
    /// `StandardMaterial::default().base_color` is white, so a material that
    /// takes a set and stops here is drawn at the scan's own albedo — a
    /// correctly exposed daylight photograph of a surface, which is a
    /// systematically brighter and more saturated thing than the surface. The
    /// flat roofs found this out first ("a city of snow", see
    /// `buildings::ROOF_TINT`) and the grass found it out again three
    /// milestones later, on the largest surface in the game.
    ///
    /// It is deliberately still the caller's job rather than an argument here.
    /// Two of the eleven sets are used for something other than what they were
    /// photographed as — the roof grain doubles as gravel, the pavement as
    /// carriageway slabs — and the tint is the only thing that makes that
    /// work, so it belongs where the decision is, not where the maps are. What
    /// this doc block is for is that the omission is silent: nothing warns, and
    /// the surface merely comes out looking like a rendering from 2010.
    pub fn apply(&self, material: &mut StandardMaterial) {
        material.base_color_texture = Some(self.color.clone());
        material.normal_map_texture = Some(self.normal.clone());
        material.metallic_roughness_texture = Some(self.roughness.clone());
        material.occlusion_texture = self.occlusion.clone();
        material.perceptual_roughness = 1.0;
        material.metallic = 0.0;
    }

    /// Gives a surface its real depth, by parallax over the set's height map.
    ///
    /// Every one of these sets ships a `Displacement.jpg`, the fetch script has
    /// always downloaded it, and until now nothing opened it. What that cost is
    /// exactly the thing a normal map cannot buy: at a grazing angle a normal
    /// map's relief flattens into the plane it is painted on, so a cobbled
    /// square seen from standing height was a photograph of cobbles lying on a
    /// sheet of glass. Parallax moves the texture instead of merely relighting
    /// it, and the joint between two setts then goes *behind* the stone in
    /// front of it, which is the whole cue.
    ///
    /// # The two numbers
    ///
    /// `metres` is how deep this material's deepest joint really is — three
    /// centimetres between setts, less between slabs. `tile` is how many metres
    /// of surface one repeat of the maps covers. Both are needed because Bevy's
    /// `parallax_depth_scale` is in units of *transformed* UV, which is to say
    /// one repeat of the texture, so a scale that is right for a pavement tiled
    /// every 2.6 m is eight times wrong for the same set tiled every 21.
    ///
    /// # Why this is opt-in rather than part of [`apply`](Self::apply)
    ///
    /// Because it is the one thing in this module that is not free, and the one
    /// thing that can look worse. Bevy's parallax loop spends up to
    /// `max_parallax_layer_count` texture fetches per fragment and spends the
    /// most of them at exactly the grazing angles a ground plane is mostly seen
    /// at, so a 40 km world plane would pay for it over most of the screen and
    /// have nothing to show: the plain is grass, and grass has no joints for a
    /// ray to fall into. It belongs on the surfaces close to the player with
    /// real relief in them, and the caller is the only one that knows which
    /// those are.
    pub fn deepen(&self, material: &mut StandardMaterial, metres: f32, tile: f32) {
        material.depth_map = Some(self.depth.clone());
        // Clamped because Bevy's own documentation warns that anything past 0.1
        // distorts, and a mistyped tile size is otherwise a surface that
        // swims.
        material.parallax_depth_scale = (metres / tile.max(0.05)).clamp(0.0, 0.08);
        // Eight rather than the default sixteen. The refinement step below the
        // march is what removes the stairsteps, and doubling the layers on a
        // surface whose relief is three centimetres buys a difference nobody
        // has been able to see in a screenshot.
        material.max_parallax_layer_count = 8.0;
        material.parallax_mapping_method = bevy::pbr::ParallaxMappingMethod::Occlusion;
    }
}

#[derive(Resource, Default)]
pub struct MaterialLibrary {
    sets: HashMap<&'static str, ScannedSet>,
    /// Maps still waiting for their mip chain. Emptied as they arrive.
    pending: HashSet<AssetId<Image>>,
    /// Which of those are height maps, and therefore have to be turned upside
    /// down on arrival. See [`invert`].
    heights: HashSet<AssetId<Image>>,
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
            depth: linear(map_path(name, "Displacement")),
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
        library.pending.insert(scanned.depth.id());
        library.heights.insert(scanned.depth.id());
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
        if library.heights.remove(id)
            && let Err(reason) = invert(&mut image)
        {
            warn!("a height map was not inverted ({reason}); its relief will be inside out");
        }
        if let Err(reason) = delight(&mut image) {
            warn!("a scanned colour map was not de-lit ({reason}); it will crush");
        }
        if let Err(reason) = add_mip_chain(&mut image) {
            warn!("no mip chain for a scanned map ({reason}); it will alias");
        }
        image.sampler = ImageSampler::Descriptor(tiling_sampler());
    }
}

/// Turns a height map into a depth map, in place.
///
/// ambientCG ships displacement the way every displacement map is authored:
/// white is the top of the relief. Bevy's parallax loop reads the same channel
/// the other way up — it walks *into* the surface from zero and stops when the
/// map's value is no longer greater than how far it has walked, so nought is
/// the surface and one is the bottom of the deepest joint. Handing it the map
/// as it arrives does not fail, it inverts the relief: the mortar stands proud
/// and every sett is a hole, which is a thing the eye notices immediately and
/// cannot name.
///
/// Done to the bytes rather than in linear light on purpose. This is not a
/// colour, it is a parameterisation of a distance, and one minus it is the same
/// parameterisation measured from the other end.
fn invert(image: &mut Image) -> Result<(), &'static str> {
    if image.texture_descriptor.mip_level_count > 1 {
        return Err("already mipped");
    }
    let Some(data) = image.data.as_mut() else {
        return Err("pixels were dropped before the render world");
    };
    for texel in data.as_chunks_mut::<4>().0 {
        // Colour channels only: the alpha of a height map is not a height.
        for channel in texel.iter_mut().take(3) {
            *channel = 255 - *channel;
        }
    }
    Ok(())
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

    /// The height maps arrive the way displacement is authored everywhere —
    /// white is the top — and Bevy's parallax walks the other way. Getting
    /// this backwards does not fail, it turns every joint into a ridge.
    #[test]
    fn a_height_map_comes_out_measured_from_the_other_end() {
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
        // The top of a sett, and the sand joint beside it.
        image.data = Some(vec![240, 240, 240, 255, 12, 12, 12, 255]);

        invert(&mut image).unwrap();
        let data = image.data.as_ref().unwrap();
        assert_eq!(data[0], 15, "the stone should now read as the surface");
        assert_eq!(data[4], 243, "the joint should now read as the deepest");
        assert_eq!(data[3], 255, "alpha is not a height");
    }

    /// The parallax scale is in units of one repeat of the texture, not in
    /// metres, which is the whole reason `deepen` takes both numbers. A three
    /// centimetre sett joint on setts that repeat every 1.35 m is a very
    /// different number from the same joint on a surface tiled every ten.
    #[test]
    fn the_parallax_depth_is_measured_in_repeats_rather_than_in_metres() {
        let set = ScannedSet {
            color: Handle::default(),
            normal: Handle::default(),
            roughness: Handle::default(),
            occlusion: None,
            depth: Handle::default(),
        };

        let mut close = StandardMaterial::default();
        set.deepen(&mut close, 0.030, 1.35);
        assert!((close.parallax_depth_scale - 0.030 / 1.35).abs() < 1e-6);
        assert!(close.depth_map.is_some());

        let mut spread = StandardMaterial::default();
        set.deepen(&mut spread, 0.030, 10.0);
        assert!(
            spread.parallax_depth_scale < close.parallax_depth_scale,
            "the same joint stretched over more surface has to parallax less"
        );
    }

    /// Bevy's own documentation says anything past 0.1 distorts. A tile size
    /// mistyped as centimetres would otherwise produce a surface that swims.
    #[test]
    fn no_tile_size_can_ask_for_a_parallax_that_distorts() {
        let set = ScannedSet {
            color: Handle::default(),
            normal: Handle::default(),
            roughness: Handle::default(),
            occlusion: None,
            depth: Handle::default(),
        };
        for tile in [0.0, 0.001, 0.05, 1.0] {
            let mut material = StandardMaterial::default();
            set.deepen(&mut material, 0.5, tile);
            assert!(
                material.parallax_depth_scale <= 0.08,
                "a tile of {tile} m asked for {}",
                material.parallax_depth_scale
            );
        }
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
