//! Procedural surface textures.
//!
//! No image files ship with this game, so every texture is painted into an
//! `Image` at startup by evaluating a function per pixel. The noise underneath
//! it is a hash of the lattice coordinate rather than a permutation table, and
//! the lattice wraps at a fixed period — which is what lets one 512-pixel
//! square tile across two kilometres of road without a seam.
//!
//! Two details cost more thought than they look:
//!
//! The noise, painting and mip helpers at the top are the shared kit: anything
//! in the project that needs to paint a texture uses them rather than growing
//! its own. The generators below them are the city's own surfaces.
//!
//! * **Mip chains are built here.** Bevy has no runtime mip generator, and a
//!   texture tiled a few hundred times across the ground plane without mips
//!   aliases into a shimmering mess the moment the camera moves. Levels are
//!   averaged in linear space for sRGB images, because averaging sRGB bytes
//!   directly darkens every level.
//! * **Facades carry three maps.** Base colour, an emissive mask saying which
//!   windows are lit, and a packed roughness/metallic map so glass catches the
//!   sun and the wall beside it does not. The emissive *strength* is not baked
//!   in: the day/night cycle drives it, so the city lights up at dusk. See
//!   `timeofday::light_windows`.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Facades are the only textures read close up and at a glancing angle.
const FACADE_SIZE: u32 = 512;
/// Emissive and roughness maps only ever modulate the base, so half is plenty.
const MASK_SIZE: u32 = 256;
const GROUND_SIZE: u32 = 256;

// ----------------------------------------------------------------- font ----

/// Glyphs, five wide and seven tall, most significant bit leftmost.
///
/// Indexed by [`glyph`]. Digits first so that `'0'..='9'` maps straight onto
/// the front of the table. This began life as the number-plate font and moved
/// into the shared kit when the buildings wanted signs: a 5×7 cell is the
/// smallest that still reads as letters instead of noise, and one hand-drawn
/// alphabet is exactly enough of them.
#[rustfmt::skip]
pub const FONT: [[u8; 7]; 36] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110], // 0
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110], // 1
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111], // 2
    [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110], // 3
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010], // 4
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110], // 5
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110], // 6
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000], // 7
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110], // 8
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100], // 9
    [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001], // A
    [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110], // B
    [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110], // C
    [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100], // D
    [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111], // E
    [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000], // F
    [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111], // G
    [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001], // H
    [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110], // I
    [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100], // J
    [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001], // K
    [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111], // L
    [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001], // M
    [0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001], // N
    [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110], // O
    [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000], // P
    [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101], // Q
    [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001], // R
    [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110], // S
    [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100], // T
    [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110], // U
    [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100], // V
    [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001], // W
    [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001], // X
    [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100], // Y
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111], // Z
];

/// The glyphs German gave the alphabet after the plates were done: umlauts,
/// the Eszett, and the punctuation a joke cannot land without. Sparse rather
/// than a second dense table, because the code points are nowhere near
/// contiguous — the umlauts sit at their Latin-1 positions, which [`encode`]
/// maps the real UTF-8 characters onto.
///
/// The umlauts spend their top row on the dots and compress the letter into
/// the remaining six — the same trade every 5×7 terminal font makes.
#[rustfmt::skip]
pub const EXTRA: [(u8, [u8; 7]); 12] = [
    (0xC4, [0b01010, 0b00000, 0b01110, 0b10001, 0b11111, 0b10001, 0b10001]), // Ä
    (0xD6, [0b01010, 0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110]), // Ö
    (0xDC, [0b01010, 0b00000, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]), // Ü
    // The eszett's top-left corner is open and neither bowl closes onto the
    // stem. Both bowls closed — which is what this was — is a B, and every
    // second sign in the town read STRABE.
    (0xDF, [0b01100, 0b10010, 0b10010, 0b10100, 0b10010, 0b10010, 0b10100]), // ß
    (b'.', [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100]),
    (b',', [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b11000]),
    (b'!', [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100]),
    (b'?', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100]),
    (b'-', [0b00000, 0b00000, 0b00000, 0b01110, 0b00000, 0b00000, 0b00000]),
    (b'\'', [0b01100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000]),
    (b'&', [0b01000, 0b10100, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101]),
    (b':', [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000]),
];

/// The rows of one character, or a blank cell for anything not in the font —
/// which is what makes a space a space.
pub fn glyph(character: u8) -> [u8; 7] {
    match character {
        b'0'..=b'9' => FONT[(character - b'0') as usize],
        b'A'..=b'Z' => FONT[(character - b'A') as usize + 10],
        _ => EXTRA
            .iter()
            .find(|(code, _)| *code == character)
            .map(|(_, rows)| *rows)
            .unwrap_or([0; 7]),
    }
}

/// A string as glyph codes: one byte per painted cell.
///
/// The paint loops index text byte-by-byte, which was fine while every sign
/// was ASCII — an umlaut is two UTF-8 bytes and would paint as two blank
/// cells. This is the one place that knows the difference: real characters
/// in, one glyph code per cell out. Lowercase folds to the capitals on the
/// way through, so a plaque can be written like language instead of like a
/// register entry. Anything the font cannot draw becomes a space, and the
/// tests on every sign catch the ones that matter.
pub fn encode(text: &str) -> Vec<u8> {
    text.chars()
        .map(|character| match character {
            'Ä' | 'ä' => 0xC4,
            'Ö' | 'ö' => 0xD6,
            'Ü' | 'ü' => 0xDC,
            'ß' => 0xDF,
            c if c.is_ascii() => c.to_ascii_uppercase() as u8,
            _ => b' ',
        })
        .collect()
}

/// Whether (u, v) inside a text band lands on ink. `v` runs 0..1 over the
/// glyph height; `u` runs 0..1 over the whole band, with an empty cell of
/// breathing room at either end. `text` is glyph codes from [`encode`].
///
/// This began life in `signage` and moved into the shared kit when the
/// statues wanted inscriptions: one band geometry, so every painted line of
/// text in the city sits in its cell the same way.
pub fn text_band(text: &[u8], u: f32, v: f32) -> bool {
    let cells = (text.len() + 2) as f32;
    let column = u * cells - 1.0;
    let index = column.floor();
    if index < 0.0 || index >= text.len() as f32 {
        return false;
    }
    let inside_x = (column - index - 0.14) / 0.72;
    if !(0.0..1.0).contains(&inside_x) || !(0.0..1.0).contains(&v) {
        return false;
    }
    let rows = glyph(text[index as usize]);
    let bit = (inside_x * 5.0) as usize;
    let row = (v * 7.0) as usize;
    rows[row.min(6)] & (1 << (4 - bit.min(4))) != 0
}

// ---------------------------------------------------------------- noise ----

fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h =
        x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_F491);
    h ^= h >> 13;
    h
}

/// A stable pseudo-random number in 0..1 for a lattice cell.
pub fn hash01(x: u32, y: u32, seed: u32) -> f32 {
    hash(x, y, seed) as f32 / u32::MAX as f32
}

pub fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise on a lattice that wraps every `period` cells.
///
/// The wrap is the whole point: it makes the texture tileable, so a road can be
/// one quad with the sampler repeating rather than thousands of unique polys.
fn value_noise(u: f32, v: f32, period: u32, seed: u32) -> f32 {
    let period = period.max(1);
    let x = u * period as f32;
    let y = v * period as f32;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (smoothstep(x - x0), smoothstep(y - y0));

    let xi = (x0 as i64).rem_euclid(period as i64) as u32;
    let yi = (y0 as i64).rem_euclid(period as i64) as u32;
    let xj = (xi + 1) % period;
    let yj = (yi + 1) % period;

    let bottom = hash01(xi, yi, seed).lerp(hash01(xj, yi, seed), fx);
    let top = hash01(xi, yj, seed).lerp(hash01(xj, yj, seed), fx);
    bottom.lerp(top, fy)
}

/// Summed octaves of [`value_noise`], each one wrapping at twice the rate of
/// the last so the whole stack still tiles.
pub fn fbm(u: f32, v: f32, period: u32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amplitude = 1.0;
    let mut total = 0.0;
    let mut p = period;
    for octave in 0..octaves {
        sum += value_noise(u, v, p, seed.wrapping_add(octave * 9781)) * amplitude;
        total += amplitude;
        amplitude *= 0.5;
        p = p.saturating_mul(2);
    }
    sum / total
}

/// Ridged noise: peaks where the underlying field crosses its midpoint, which
/// draws thin wandering lines rather than blobs. Used for cracks.
pub fn ridge(u: f32, v: f32, period: u32, octaves: u32, seed: u32) -> f32 {
    1.0 - (fbm(u, v, period, octaves, seed) * 2.0 - 1.0).abs()
}

// -------------------------------------------------------------- painting ----

pub fn srgb_to_linear(byte: u8) -> f32 {
    let c = byte as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

pub fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// One box-filter step of a mip chain.
pub fn downsample(src: &[u8], width: u32, height: u32, srgb: bool) -> (Vec<u8>, u32, u32) {
    let (dw, dh) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0u8; (dw * dh * 4) as usize];

    for y in 0..dh {
        for x in 0..dw {
            for channel in 0..4u32 {
                // Alpha is already linear; colour channels are only linear in
                // a linear format.
                let encoded = srgb && channel < 3;
                let mut sum = 0.0;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(width - 1);
                    let sy = (y * 2 + dy).min(height - 1);
                    let raw = src[((sy * width + sx) * 4 + channel) as usize];
                    sum += if encoded {
                        srgb_to_linear(raw)
                    } else {
                        raw as f32 / 255.0
                    };
                }
                let average = sum / 4.0;
                out[((y * dw + x) * 4 + channel) as usize] = byte(if encoded {
                    linear_to_srgb(average)
                } else {
                    average
                });
            }
        }
    }
    (out, dw, dh)
}

/// Paints an image by evaluating `paint` at the centre of every texel, then
/// builds its mip chain and a repeating, anisotropic sampler.
///
/// Built with `new_uninit` rather than `Image::new` because the latter asserts
/// that the data is exactly one mip level.
pub fn painted(size: u32, format: TextureFormat, paint: impl Fn(f32, f32) -> [u8; 4]) -> Image {
    painted_rect(size, size, format, paint)
}

/// [`painted`], for something that is not square.
///
/// Everything tiled over a surface here is square, because that is what makes a
/// tile; the exceptions are the things that are one object each and have their
/// own proportions, like a number plate.
pub fn painted_rect(
    width: u32,
    height: u32,
    format: TextureFormat,
    paint: impl Fn(f32, f32) -> [u8; 4],
) -> Image {
    let (full_width, full_height) = (width, height);
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let u = (x as f32 + 0.5) / width as f32;
            let v = (y as f32 + 0.5) / height as f32;
            data.extend_from_slice(&paint(u, v));
        }
    }

    let srgb = format == TextureFormat::Rgba8UnormSrgb;
    let mut level = data.clone();
    let (mut width, mut height) = (width, height);
    let mut levels = 1;
    while width > 1 || height > 1 {
        let (next, nw, nh) = downsample(&level, width, height, srgb);
        data.extend_from_slice(&next);
        level = next;
        width = nw;
        height = nh;
        levels += 1;
    }

    let mut image = Image::new_uninit(
        Extent3d {
            width: full_width,
            height: full_height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        // Nothing ever reads these back on the CPU.
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        // The road is seen almost edge-on almost all the time, which is exactly
        // the case trilinear filtering blurs into mud.
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

/// A tangent-space normal map sampled from a wrapping height field.
///
/// Central differences rather than a Sobel kernel: these height fields are
/// interpolated noise and already smooth, so the extra taps buy blur and
/// nothing else. `relief` is how tall the bumps are as a fraction of the tile —
/// small numbers only. Push it past a few percent and a flat surface starts to
/// look like it was moulded out of putty.
pub fn normal_map(size: u32, relief: f32, height: impl Fn(f32, f32) -> f32) -> Image {
    let step = 1.0 / size as f32;
    // Slope per texel, converted to slope per unit of UV.
    let scale = relief * size as f32 * 0.5;
    painted(size, TextureFormat::Rgba8Unorm, |u, v| {
        let dx = height(u + step, v) - height(u - step, v);
        let dy = height(u, v + step) - height(u, v - step);
        let normal = Vec3::new(-dx * scale, -dy * scale, 1.0).normalize();
        [
            byte(normal.x * 0.5 + 0.5),
            byte(normal.y * 0.5 + 0.5),
            byte(normal.z * 0.5 + 0.5),
            255,
        ]
    })
}

// ---------------------------------------------------------------- ground ----

/// Height field the asphalt's colour and its normal map are both built from,
/// so a crack that reads dark also reads deep.
fn asphalt_height(u: f32, v: f32) -> f32 {
    let grain = fbm(u, v, 48, 4, 11) - 0.5;
    let patches = fbm(u, v, 6, 3, 23) - 0.5;
    let mut height = 0.5 + grain * 0.5 + patches * 0.2;

    // Cracks are drawn from a high-frequency ridge and cut off hard. A gentler
    // threshold reads as tarmac rivers rather than as a crack.
    let crack = ridge(u, v, 14, 3, 67);
    if crack > 0.986 {
        height -= ((crack - 0.986) * 40.0).min(0.55);
    }
    height.clamp(0.0, 1.0)
}

/// Road surface: aggregate grain, patched repairs, and a few cracks.
pub fn asphalt() -> Image {
    painted(GROUND_SIZE, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let mut value = 0.90 + (asphalt_height(u, v) - 0.5) * 0.32;

        // Aggregate: individual chips of stone catching the light.
        let speck = hash01(
            (u * GROUND_SIZE as f32) as u32,
            (v * GROUND_SIZE as f32) as u32,
            41,
        );
        if speck > 0.988 {
            value += 0.22;
        }

        let c = byte(value);
        [c, c, byte(value * 0.995), 255]
    })
}

pub fn asphalt_normal() -> Image {
    normal_map(GROUND_SIZE, 0.007, asphalt_height)
}

/// Slabs per tile, on each axis.
const SLABS: f32 = 4.0;

/// Height field for the pavement: flat slabs, recessed joints.
fn paving_height(u: f32, v: f32) -> f32 {
    let (su, sv) = (u * SLABS, v * SLABS);
    let (fu, fv) = (su.fract(), sv.fract());
    let joint = fu.min(1.0 - fu).min(fv).min(1.0 - fv);
    let bevel = smoothstep01(joint / 0.035);
    0.25 + bevel * 0.7 + (fbm(u, v, 64, 3, 13) - 0.5) * 0.10
}

/// Pavement slabs, jointed on a grid.
pub fn paving() -> Image {
    painted(GROUND_SIZE, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let (cu, cv) = ((u * SLABS).floor(), (v * SLABS).floor());
        // Per-slab tone, so the pavement does not read as one flat sheet.
        let tone = hash01(cu as u32, cv as u32, 7) * 0.08 - 0.04;
        let value = 0.80 + tone + paving_height(u, v) * 0.17;

        let c = byte(value);
        [c, c, byte(value * 0.98), 255]
    })
}

pub fn paving_normal() -> Image {
    normal_map(GROUND_SIZE, 0.020, paving_height)
}

/// Park grass: two scales of mottling plus a fine speckle.
pub fn grass() -> Image {
    painted(GROUND_SIZE, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let broad = fbm(u, v, 4, 3, 31) - 0.5;
        let fine = fbm(u, v, 40, 4, 53) - 0.5;
        let value = 0.94 + broad * 0.22 + fine * 0.20;
        // Greener where it is darker: shaded grass reads more saturated.
        [
            byte(value * 0.88),
            byte(value * 1.04),
            byte(value * 0.78),
            255,
        ]
    })
}

// --------------------------------------------------------------- foliage ----

/// How much of a canopy is leaf and how much is gap.
///
/// Over about a half and a tree reads as a wire mesh; under four tenths and
/// the silhouette closes back up into the ball the geometry actually is. The
/// number is a threshold on a field that averages a half, so it runs backwards:
/// higher cuts away more.
const CANOPY_COVER: f32 = 0.47;

/// Leaf mass, as a field: high in the middle of a clump, low in the gaps.
fn canopy_height(u: f32, v: f32) -> f32 {
    // Two scales, because a crown has both. The broad one is the clump — the
    // handful of branches that carry a bough's worth of leaves — and the fine
    // one is the leaves themselves.
    let clump = fbm(u, v, 5, 3, 137);
    let leaves = fbm(u, v, 26, 3, 149);
    (clump * 0.62 + leaves * 0.38).clamp(0.0, 1.0)
}

/// The leaf mass on a crown, and the holes between it.
///
/// A tree is not a ball, and a canopy modelled as one is the single loudest
/// thing in a street that says a computer drew it: the geometry underneath here
/// really is four spheres merged, and no amount of shading fixes an outline that
/// smooth. What fixes it is throwing away part of the surface. The alpha channel
/// is a hard cut through the leaf-mass field, so the sphere's edge comes apart
/// into clumps and the sky shows through the gaps — the silhouette stops being a
/// circle without a single extra triangle.
///
/// The colour is kept close to white on purpose. It multiplies the species tint,
/// which is where a lime is meant to differ from a plane, and a texture that
/// carried its own green would flatten the four of them into one.
pub fn foliage() -> Image {
    painted(GROUND_SIZE, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let mass = canopy_height(u, v);
        // Leaves out at the edge of a clump are the ones in the light, and they
        // are also the young ones: brighter, and yellower.
        let edge = 1.0 - (mass - CANOPY_COVER).max(0.0) * 1.6;
        let value = 0.72 + edge * 0.42;
        [
            byte(value * 0.96),
            byte(value * 1.02),
            byte(value * 0.80),
            // The cut. Softened by a texel or two of the fine field so a mip
            // level down the chain still has an edge to average rather than a
            // stack of hard-clipped ones.
            byte(((mass - CANOPY_COVER) * 14.0).clamp(0.0, 1.0)),
        ]
    })
}

/// The same field as relief, so a clump that reads solid also reads round.
pub fn foliage_normal() -> Image {
    normal_map(GROUND_SIZE, 0.055, canopy_height)
}

fn roof_height(u: f32, v: f32) -> f32 {
    (fbm(u, v, 56, 4, 71) * 0.8 + fbm(u, v, 4, 3, 83) * 0.2).clamp(0.0, 1.0)
}

/// Flat roof: tar and gravel, with damp patches.
pub fn roof() -> Image {
    painted(GROUND_SIZE, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let value = 0.72 + (roof_height(u, v) - 0.5) * 0.55;
        [byte(value), byte(value * 0.99), byte(value * 1.02), 255]
    })
}

pub fn roof_normal() -> Image {
    normal_map(GROUND_SIZE, 0.018, roof_height)
}

/// Biberschwanz: the plain clay tile a Bavarian old town is roofed with.
///
/// The name is the shape — a beaver's tail, a rectangle with a rounded end —
/// and the shape is the whole of why a Landshut roof reads as one from across
/// the river. It is laid in double lap, every course offset half a tile from
/// the one below, so what the eye sees is rows of scallops rather than a grid.
///
/// ## Which way up
///
/// `u` runs *up the slope*, so the courses stack along it, and `v` runs along
/// the ridge. That is the opposite of how one would write it on paper, and it
/// is deliberate: a roof leaf is a scaled cube, and a cube's top face maps `u`
/// to its local X, which for a leaf is the direction of the fall. Written the
/// natural way round, the courses lapped sideways and the roof read as
/// clapboard — visible in `shots/m18-tiles.png` before this was fixed.
///
/// Height, not colour, is what carries it: the tint lives on the material, so
/// a new roof and a mossy one share this one image.
/// Where a point on a roof falls: which tile, and where inside it.
///
/// Split out because the albedo and the relief both need it and neither can be
/// derived from the other — a tile that is a shade darker than its neighbour is
/// not a tile that sits lower.
struct Tile {
    /// Which tile of the pattern, for hashing something per-tile.
    index: f32,
    course: f32,
    /// Across the tile and up the course, both 0..1.
    across: f32,
    up: f32,
}

/// Courses up the slope and tiles along the ridge, per repeat of the image.
/// The material tiles it further; this is only the pattern.
const COURSES: f32 = 9.0;
const TILES: f32 = 6.0;
/// How much of a course's height the rounded tail takes up.
const TAIL: f32 = 0.36;

fn tile_at(u: f32, v: f32) -> Tile {
    let course = (u * COURSES).floor();
    // Every other course is set half a tile over — the bond that stops the
    // joints lining up into gutters running down the roof.
    let stagger = if (course as i32).rem_euclid(2) == 0 {
        0.0
    } else {
        0.5
    };
    Tile {
        index: (v * TILES + stagger).floor(),
        course,
        across: (v * TILES + stagger).fract(),
        // Up the course: 0 at the tail, 1 where it goes under the course above.
        up: (u * COURSES).fract(),
    }
}

fn tile_height(u: f32, v: f32) -> f32 {
    let tile = tile_at(u, v);
    // A groove down each joint, and the lap line where this tile goes under
    // the next course.
    let joint = 1.0 - (((tile.across - 0.5) * 2.0).abs()).powi(8);
    let lap = ((1.0 - tile.up) / 0.14).clamp(0.0, 1.0);
    let face = (0.60 + joint * 0.20 + lap * 0.20).clamp(0.0, 1.0);

    // The rounded end. Measured from the middle of the tile, which is where it
    // hangs lowest; the corners of the tail sit a third of a course higher.
    let off = ((tile.across - 0.5) * 2.0).abs().min(1.0);
    let edge = TAIL * (1.0 - (1.0 - off * off).max(0.0).sqrt());
    if tile.up < edge {
        // Below the tail. This is *not* a hole — it is the tile of the course
        // below, seen through the gap between two round ends, sitting in their
        // shadow. Filling it with darkness put a black arrowhead between every
        // pair of tiles, which is the one thing a tiled roof does not have.
        let under = ((edge - tile.up) / TAIL).clamp(0.0, 1.0);
        return (face - 0.34 * (1.0 - under).powi(2) - 0.08).clamp(0.0, 1.0);
    }
    face
}

/// Clay tiles, for the pitched roofs.
pub fn tiles() -> Image {
    painted(GROUND_SIZE, TextureFormat::Rgba8UnormSrgb, |u, v| {
        let tile = tile_at(u, v);
        // One value per *tile*, not per texel. This is what a clay roof
        // actually looks like from the far pavement: the courses are a
        // texture, but what the eye reads is that every tile came out of the
        // kiln a slightly different colour. Shading alone gave a flat sheet as
        // soon as the courses mipped away.
        let fired = fbm(
            (tile.index + 0.5) / TILES,
            (tile.course + 0.5) / COURSES,
            7,
            2,
            0x7B1E,
        ) - 0.5;

        // Held under one, because a value that clips is a value with no
        // pattern left in it — the first pass at this ran to 1.24 and the top
        // quarter of every roof came out flat white.
        let value = (0.44 + tile_height(u, v) * 0.54 + fired * 0.26).clamp(0.06, 1.0);
        // Warmer where the tile is proud and cooler in the shadow of the lap,
        // which is what fired clay does and what keeps a tinted grey from
        // reading as plastic. The cooler tiles are also the paler ones.
        let warm = 1.0 + fired * 0.10;
        [
            byte(value * 1.05 * warm),
            byte(value * 0.96),
            byte(value * 0.90 / warm),
            255,
        ]
    })
}

pub fn tiles_normal() -> Image {
    normal_map(GROUND_SIZE, 0.038, tile_height)
}

// --------------------------------------------------------------- facades ----

/// How a building is glazed. Picked from height, because that is what actually
/// separates a house from a tower: floor spacing barely changes, so a taller
/// building simply has more, smaller-looking windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacadeClass {
    House,
    Lowrise,
    Midrise,
    Tower,
}

impl FacadeClass {
    pub const ALL: [FacadeClass; 4] = [
        FacadeClass::House,
        FacadeClass::Lowrise,
        FacadeClass::Midrise,
        FacadeClass::Tower,
    ];

    pub fn for_height(height: f32) -> Self {
        match height {
            h if h < 11.0 => FacadeClass::House,
            h if h < 26.0 => FacadeClass::Lowrise,
            h if h < 55.0 => FacadeClass::Midrise,
            _ => FacadeClass::Tower,
        }
    }

    pub fn index(self) -> usize {
        match self {
            FacadeClass::House => 0,
            FacadeClass::Lowrise => 1,
            FacadeClass::Midrise => 2,
            FacadeClass::Tower => 3,
        }
    }

    /// Windows across, floors up.
    ///
    /// Public because the shell geometry in `world::shell` is built on exactly
    /// this grid. A reveal cut anywhere else lands beside its own window.
    pub fn grid(self) -> (f32, f32) {
        match self {
            FacadeClass::House => (3.0, 2.0),
            FacadeClass::Lowrise => (5.0, 4.0),
            FacadeClass::Midrise => (7.0, 9.0),
            FacadeClass::Tower => (9.0, 17.0),
        }
    }

    /// Whether this kind of building has shops at street level.
    ///
    /// A house does not. Its ground floor is a front door and the same windows
    /// as upstairs, and giving it a shop window turns a residential street into
    /// a high street.
    pub fn has_shopfronts(self) -> bool {
        !matches!(self, FacadeClass::House)
    }

    /// Fraction of each cell the glass fills, horizontally and vertically.
    pub fn glazing(self) -> (f32, f32) {
        match self {
            FacadeClass::House => (0.46, 0.50),
            FacadeClass::Lowrise => (0.56, 0.52),
            FacadeClass::Midrise => (0.70, 0.56),
            // Curtain wall: glass edge to edge, hairline mullions.
            FacadeClass::Tower => (0.88, 0.72),
        }
    }

    /// Where the glass sits inside one cell, as fractions of that cell.
    ///
    /// The single description of the window grid. It is read twice — once here
    /// to paint the facade, and once by `world::shell` to cut the reveal out of
    /// the geometry — and the two have to agree to the last decimal or every
    /// window in the city has its frame a hand's width to one side.
    pub fn pane(self, row: u32) -> Pane {
        let (glass_w, glass_h) = self.glazing();

        // The ground storey is not a storey of rooms, and drawing it as one is
        // the single clearest sign that a facade was generated: real streets
        // are shops and lobbies at eye level with flats above, and eye level is
        // the only part a pedestrian ever looks at closely.
        if row == 0 && self.has_shopfronts() {
            // A shopfront runs nearly the full bay, from a low stallriser up to
            // the sign board.
            return Pane {
                u0: 0.08,
                u1: 0.92,
                v0: 0.16,
                v1: 0.74,
                ground: true,
            };
        }

        // Panes sit slightly above centre in their cell, leaving a spandrel
        // below.
        Pane {
            u0: 0.5 - glass_w * 0.5,
            u1: 0.5 + glass_w * 0.5,
            v0: 0.62 - glass_h * 0.5,
            v1: 0.62 + glass_h * 0.5,
            ground: false,
        }
    }

    /// How likely any one window is lit after dark.
    fn occupancy(self) -> f32 {
        match self {
            FacadeClass::House => 0.55,
            FacadeClass::Lowrise => 0.45,
            FacadeClass::Midrise => 0.38,
            FacadeClass::Tower => 0.30,
        }
    }
}

/// Where the sign board over a shopfront sits, as fractions of its cell.
///
/// Shared with `world::shell`, which stands the board proud of the wall.
pub const FASCIA: (f32, f32) = (0.78, 0.97);

/// Where the glass sits inside one cell of the window grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pane {
    pub u0: f32,
    pub u1: f32,
    pub v0: f32,
    pub v1: f32,
    /// True on the ground storey, which is a shopfront rather than a room.
    pub ground: bool,
}

impl Pane {
    /// Wall left over between the glass and the edge of its cell: left, right,
    /// below, above.
    ///
    /// Every one of them has to be greater than zero, or a reveal has no wall
    /// to sit in and the geometry degenerates — see the tests.
    pub fn margins(&self) -> [f32; 4] {
        [self.u0, 1.0 - self.u1, self.v0, 1.0 - self.v1]
    }
}

/// The four maps that make up one facade.
pub struct FacadeMaps {
    pub base: Image,
    /// White where a window is lit; the material's `emissive` scales it.
    pub emissive: Image,
    /// Packed the way glTF packs it: green is roughness, blue is metallic.
    pub surface: Image,
    /// Tangent-space relief: recessed panes, grooved floor lines.
    pub normal: Image,
}

/// How wide a stroke of paint is, as a fraction of the tag's own image.
const STROKE: f32 = 0.052;
/// And the black outline round it, which is what makes a tag read as a tag
/// rather than as a coloured squiggle.
const OUTLINE: f32 = 0.030;

/// Distance from a point to a line segment, in image space.
fn to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let run = b - a;
    let along = (p - a).dot(run) / run.length_squared().max(1e-6);
    a.lerp(b, along.clamp(0.0, 1.0)).distance(p)
}

/// A tag: one continuous scrawl, outlined, with the rest transparent.
///
/// Not letters. Every attempt to spell something at this texel count comes out
/// as a smudge that reads as a mistake rather than as writing, and a tag that
/// says a real word says it on every wall in the city. What a tag is *shaped*
/// like — one unbroken run of a fat marker, doubling back on itself, outlined
/// in black — is unmistakable at ten metres and takes six points and a distance
/// function.
///
/// `variant` picks a different scrawl and a different colour, so a street does
/// not carry the same signature twice.
pub fn graffiti(variant: u32) -> Image {
    const SIZE: u32 = 256;
    painted(SIZE, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        graffiti_at(u, v, variant)
    })
}

/// One texel of a tag, so a surface that is already painting itself — the site
/// hoarding, which has stripes under its graffiti — can lay one on without a
/// second quad to parent, scale and knock over alongside the first.
pub fn graffiti_at(u: f32, v: f32, variant: u32) -> [u8; 4] {
    // The chain, hashed from the variant and kept off the border so the stroke
    // and its outline both fit inside the image.
    let points: Vec<Vec2> = (0..7)
        .map(|i| {
            // Marching left to right *on average*, with enough slack in each
            // step to double back on the last one. That doubling-back is the
            // whole difference between a tag and a worm: a chain that only ever
            // advances comes out as one fat horizontal stroke, which is exactly
            // what the first version drew across every shutter in the city.
            let along = i as f32 / 6.0;
            Vec2::new(
                0.12 + along * 0.76 + (hash01(i, variant, 0x51a9) - 0.5) * 0.40,
                0.16 + hash01(i, variant, 0x7d13) * 0.68,
            )
            .clamp(Vec2::splat(0.11), Vec2::splat(0.89))
        })
        .collect();

    // Fill and outline. The fills are the colours somebody actually buys.
    let fill = match variant % 3 {
        0 => [0.92f32, 0.28, 0.14],
        1 => [0.20, 0.62, 0.88],
        _ => [0.94, 0.86, 0.16],
    };

    let p = Vec2::new(u, v);
    let mut near = f32::MAX;
    for pair in points.windows(2) {
        near = near.min(to_segment(p, pair[0], pair[1]));
    }
    {
        // A spray line is not a clean edge: the width wanders and the paint
        // fades out where the can was moving.
        let ragged = (fbm(u, v, 9, 3, 0x2be1 ^ variant) - 0.5) * 0.018;
        let ink = smoothstep01((STROKE + ragged - near) / 0.007);
        let edge = smoothstep01((STROKE + OUTLINE + ragged - near) / 0.007);

        // Black under the colour, so the outline shows wherever the fill does
        // not reach.
        let color = [
            fill[0] * ink + 0.04 * (1.0 - ink),
            fill[1] * ink + 0.04 * (1.0 - ink),
            fill[2] * ink + 0.045 * (1.0 - ink),
        ];
        [
            byte(color[0]),
            byte(color[1]),
            byte(color[2]),
            byte(edge * 0.92),
        ]
    }
}

pub fn smoothstep01(t: f32) -> f32 {
    smoothstep(t.clamp(0.0, 1.0))
}

/// Where a texel falls within its window cell.
struct Cell {
    column: u32,
    row: u32,
    /// 1 well inside the glass, 0 well outside, ramped across the reveal.
    ///
    /// Soft rather than boolean because the same value drives the height field
    /// the normal map is built from, and a one-texel cliff there produces a
    /// bevel with a staircase in it.
    pane: f32,
    /// True inside the glass.
    glass: bool,
    /// True on the ground storey, which is shops rather than rooms.
    ground: bool,
    /// 1 inside the sign board over a shopfront, 0 elsewhere.
    fascia: f32,
    /// 0 at the bottom of the pane, 1 at the top. Meaningless off the glass.
    up_pane: f32,
    /// Distance below the pane above, in cell heights; `None` above it.
    below_pane: Option<f32>,
}

/// Resolves a UV into the window grid. `v` runs up the building.
fn cell_at(class: FacadeClass, u: f32, v: f32) -> Cell {
    let (columns, rows) = class.grid();

    let su = u * columns;
    let sv = v * rows;
    let (column, row) = (su.floor(), sv.floor());
    let (fu, fv) = (su - column, sv - row);

    let pane_rect = class.pane(row.max(0.0) as u32);
    let Pane {
        u0,
        u1,
        v0,
        v1,
        ground,
    } = pane_rect;

    // The sign board sits between the top of the glazing and the floor line.
    let fascia = if ground {
        smoothstep01((fv - FASCIA.0) / 0.03) * smoothstep01((FASCIA.1 - fv) / 0.02)
    } else {
        0.0
    };

    // Softness of the reveal, in cell widths.
    const REVEAL: f32 = 0.02;
    let pane = smoothstep01((fu - u0) / REVEAL)
        * smoothstep01((u1 - fu) / REVEAL)
        * smoothstep01((fv - v0) / REVEAL)
        * smoothstep01((v1 - fv) / REVEAL);

    Cell {
        column: column as u32,
        row: row as u32,
        pane,
        glass: pane > 0.5,
        ground,
        fascia,
        up_pane: ((fv - v0) / (v1 - v0)).clamp(0.0, 1.0),
        // Grime runs down from the sill, so only the strip under a pane cares.
        below_pane: (fv < v0 && fu > u0 && fu < u1).then(|| (v0 - fv) / v0.max(1e-3)),
    }
}

pub fn facade(class: FacadeClass) -> FacadeMaps {
    let seed = 500 + class.index() as u32 * 131;

    let base = painted(FACADE_SIZE, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        let cell = cell_at(class, u, v);

        if cell.glass {
            // Glass is dark, and lighter towards the top of the pane where it
            // is reflecting sky rather than the street opposite.
            let tint = hash01(cell.column, cell.row, seed) * 0.10;
            let sky = cell.up_pane * 0.16;
            // A shop window is lit from inside during the day too, and has
            // something in it; it never goes as dark as an office pane.
            let shop = if cell.ground { 0.20 } else { 0.0 };
            let blind = if !cell.ground && hash01(cell.column, cell.row, seed + 3) > 0.82 {
                // Some panes have a blind pulled down.
                0.22
            } else {
                0.0
            };
            let (sky, blind) = (sky + shop, blind);
            return [
                byte(0.17 + sky * 0.8 + tint + blind),
                byte(0.20 + sky * 0.95 + tint + blind),
                byte(0.25 + sky + tint + blind * 0.9),
                255,
            ];
        }

        // The sign board over a shop: a painted panel, darker than the wall and
        // its own colour per bay, which is what makes a parade of shops read as
        // separate businesses rather than one long building.
        if cell.fascia > 0.5 {
            let shade = 0.20 + hash01(cell.column, 7, seed + 41) * 0.35;
            let warm = hash01(cell.column, 9, seed + 43);
            return [
                byte(shade * (0.7 + warm * 0.5)),
                byte(shade * (0.7 + (1.0 - warm) * 0.4)),
                byte(shade * 0.9),
                255,
            ];
        }

        // Wall. Kept close to white so the district palette on the material,
        // not the texture, decides what colour the building is.
        let mut value = 0.94 + (fbm(u, v, 40, 4, seed + 11) - 0.5) * 0.09;
        // Floor lines: a shadow where each storey meets the next.
        let (_, rows) = class.grid();
        let storey = (v * rows).fract();
        if storey < 0.04 {
            value -= 0.10 * (1.0 - storey / 0.04);
        }
        // Rain shadow under every sill, fading out as it runs down the wall.
        if let Some(depth) = cell.below_pane {
            let streak = fbm(u * 6.0, v, 24, 3, seed + 29);
            value -= (1.0 - depth).max(0.0) * 0.09 * (0.4 + streak * 0.9);
        }
        // Weathering: the base of a building is always dirtier than its top.
        value -= (1.0 - v).powi(4) * 0.06;

        let c = byte(value);
        [c, c, byte(value * 0.995), 255]
    });

    let emissive = painted(MASK_SIZE, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        let cell = cell_at(class, u, v);
        if !cell.glass {
            return [0, 0, 0, 255];
        }
        // The ground floor is shops and lobbies: nearly always lit.
        let occupancy = if cell.row == 0 {
            0.85
        } else {
            class.occupancy()
        };
        if hash01(cell.column, cell.row, seed + 7) > occupancy {
            return [0, 0, 0, 255];
        }

        let brightness = 0.55 + hash01(cell.column, cell.row, seed + 13) * 0.45;
        // A minority of interiors are fluorescent rather than tungsten, which
        // is what stops a night skyline reading as a single orange wash.
        let cool = hash01(cell.column, cell.row, seed + 17) > 0.72;
        let (r, g, b) = if cool {
            (0.80, 0.92, 1.00)
        } else {
            (1.00, 0.80, 0.52)
        };
        [
            byte(r * brightness),
            byte(g * brightness),
            byte(b * brightness),
            255,
        ]
    });

    // Panes sit back behind the wall, and each storey meets the next in a
    // shadow line. Both are what stop a facade reading as a decal on a box.
    let height = move |u: f32, v: f32| {
        let cell = cell_at(class, u, v);
        let (_, rows) = class.grid();
        let storey = (v * rows).fract();
        // Only the two features that are genuinely three-dimensional. Adding
        // the wall's colour noise here as well made a flat facade look like
        // poured concrete that had gone off badly.
        let reveal = 0.75 - cell.pane * 0.55 + cell.fascia * 0.22;
        reveal - smoothstep01(1.0 - storey / 0.04) * 0.22
    };
    let normal = normal_map(MASK_SIZE, 0.018, height);

    let surface = painted(MASK_SIZE, TextureFormat::Rgba8Unorm, move |u, v| {
        let cell = cell_at(class, u, v);
        let (roughness, metallic) = if cell.glass {
            (0.10, 0.55)
        } else {
            (0.88 + (fbm(u, v, 32, 2, seed + 23) - 0.5) * 0.12, 0.0)
        };
        [255, byte(roughness), byte(metallic), 255]
    });

    FacadeMaps {
        base,
        emissive,
        surface,
        normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_wraps_at_the_period() {
        // The left and right edges of a tile have to agree, or every tiled
        // surface in the game grows a grid of seams.
        for i in 0..64 {
            let v = i as f32 / 64.0;
            let left = fbm(0.0, v, 8, 4, 5);
            let right = fbm(1.0, v, 8, 4, 5);
            assert!((left - right).abs() < 1e-4, "seam at v={v}: {left} {right}");
            let bottom = fbm(v, 0.0, 8, 4, 5);
            let top = fbm(v, 1.0, 8, 4, 5);
            assert!((bottom - top).abs() < 1e-4, "seam at u={v}");
        }
    }

    #[test]
    fn noise_stays_in_range() {
        for i in 0..200 {
            let u = i as f32 / 200.0;
            let value = fbm(u, u * 0.37, 6, 5, 99);
            assert!((0.0..=1.0).contains(&value), "fbm out of range: {value}");
        }
    }

    #[test]
    fn a_painted_image_carries_a_full_mip_chain() {
        let image = paving();
        let levels = image.texture_descriptor.mip_level_count;
        assert_eq!(levels, GROUND_SIZE.ilog2() + 1, "chain must reach 1x1");

        // Every level must be present in the buffer, or wgpu rejects the upload.
        let mut expected = 0usize;
        let mut size = GROUND_SIZE;
        for _ in 0..levels {
            expected += (size * size * 4) as usize;
            size = (size / 2).max(1);
        }
        assert_eq!(image.data.as_ref().map(Vec::len), Some(expected));
    }

    #[test]
    fn facade_classes_follow_height() {
        assert_eq!(FacadeClass::for_height(7.0), FacadeClass::House);
        assert_eq!(FacadeClass::for_height(40.0), FacadeClass::Midrise);
        assert_eq!(FacadeClass::for_height(130.0), FacadeClass::Tower);
        // Indices address the material table, so they must be dense and unique.
        for (i, class) in FacadeClass::ALL.iter().enumerate() {
            assert_eq!(class.index(), i);
        }
    }

    #[test]
    fn the_ground_storey_is_shops_and_the_ones_above_are_not() {
        // Sampled up the middle of the first bay: the shopfront has to be
        // taller than the pane above it, or it is just another window and the
        // street has no eye level.
        let class = FacadeClass::Midrise;
        let (_, rows) = class.grid();
        let glass_run = |storey: f32| {
            (0..400)
                .filter(|i| {
                    let v = (storey + *i as f32 / 400.0) / rows;
                    cell_at(class, 0.5 / class.grid().0, v).glass
                })
                .count()
        };
        assert!(
            glass_run(0.0) > glass_run(1.0),
            "the shopfront is no taller than the flat above it"
        );
    }

    #[test]
    fn houses_have_no_shop_windows() {
        // A front door and the same windows as upstairs. Giving a terrace a
        // shopfront turns a residential street into a high street.
        assert!(!FacadeClass::House.has_shopfronts());
        for class in [
            FacadeClass::Lowrise,
            FacadeClass::Midrise,
            FacadeClass::Tower,
        ] {
            assert!(class.has_shopfronts(), "{class:?} should have shops");
        }
    }

    #[test]
    fn sign_boards_only_hang_over_shops() {
        let class = FacadeClass::Midrise;
        let (columns, rows) = class.grid();
        let u = 0.5 / columns;
        // Somewhere in the sign band of the ground storey.
        assert!(cell_at(class, u, 0.88 / rows).fascia > 0.5);
        // The same height in every storey above it is plain wall.
        for storey in 1..rows as u32 {
            let v = (storey as f32 + 0.88) / rows;
            assert_eq!(
                cell_at(class, u, v).fascia,
                0.0,
                "a sign board turned up on storey {storey}"
            );
        }
    }

    #[test]
    fn windows_are_lit_only_where_there_is_glass() {
        let maps = facade(FacadeClass::Tower);
        let data = maps.emissive.data.as_ref().unwrap();
        let lit = data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 8 || p[2] > 8)
            .count();
        let total = (MASK_SIZE * MASK_SIZE) as usize;
        assert!(lit > 0, "a tower at night should have lit windows");
        assert!(
            lit < total / 2,
            "more than half the facade glowing is not a building, it is a lamp"
        );
    }

    #[test]
    fn every_glyph_is_drawn_and_none_of_them_overflow_the_cell() {
        // Five bits wide, so bit 5 and up must be clear — a stray bit there
        // does not fail visibly, it just bleeds a pixel into the next letter.
        for (index, rows) in FONT.iter().enumerate() {
            assert!(rows.iter().any(|&row| row != 0), "glyph {index} is blank");
            for (line, &row) in rows.iter().enumerate() {
                assert!(
                    row < 0b100000,
                    "glyph {index} row {line} is wider than five cells"
                );
            }
        }
        for (code, rows) in EXTRA {
            assert!(
                rows.iter().any(|&row| row != 0),
                "extra glyph {code:#04x} is blank"
            );
            for (line, &row) in rows.iter().enumerate() {
                assert!(
                    row < 0b100000,
                    "extra glyph {code:#04x} row {line} is wider than five cells"
                );
            }
            // And each one is actually reachable through the lookup — an
            // entry whose code collides with the plate alphabet would be
            // shadowed and never paint.
            assert_eq!(glyph(code), rows, "glyph {code:#04x} is unreachable");
        }
    }

    #[test]
    fn the_umlauts_survive_the_trip_through_encode() {
        // The failure this catches is painting umlauts byte-by-byte: 'Ä' is
        // two UTF-8 bytes, so a sign painted from `str::as_bytes` shows two
        // blank cells where the letter should be.
        let coded = encode("Käßspatzen ÖD & GRAU?!");
        assert_eq!(coded.len(), "Käßspatzen ÖD & GRAU?!".chars().count());
        for &code in &coded {
            if code != b' ' {
                assert_ne!(glyph(code), [0; 7], "{code:#04x} came out blank");
            }
        }
        // Lowercase folds to the capitals the font actually has.
        assert_eq!(encode("boing"), encode("BOING"));
    }

    /// A tiled roof is rows of scallops, not a flat sheet with holes in it.
    #[test]
    fn clay_tiles_are_laid_in_courses() {
        let mut darkest = 1.0f32;
        let mut lightest = 0.0f32;
        for y in 0..128 {
            for x in 0..128 {
                let h = tile_height(x as f32 / 128.0, y as f32 / 128.0);
                assert!((0.0..=1.0).contains(&h));
                darkest = darkest.min(h);
                lightest = lightest.max(h);
            }
        }
        // There is relief, and none of it is a hole. The failure this pins is
        // the first version, whose shadow under a tail was a flat 0.18 wedge:
        // that reads as a black arrowhead between every pair of tiles.
        assert!(lightest - darkest > 0.25, "the roof is flat");
        assert!(darkest > 0.25, "there are holes in the roof: {darkest:.3}");

        // Nothing clips. A texel pinned at white or black is a texel with
        // no pattern left in it, and a roof of them is a flat sheet however
        // good the height field underneath is.
        for y in 0..64 {
            for x in 0..64 {
                let value = 0.44 + tile_height(x as f32 / 64.0, y as f32 / 64.0) * 0.54;
                assert!(value < 1.0, "the roof burns out: {value:.3}");
            }
        }

        // The courses stack up the slope, which is `u` — not across it. Two
        // points a course apart in `u` at the same `v` are at different points
        // of the pattern, because the bond offsets every other course.
        let one = tile_height(0.5 / 9.0, 0.5 / 6.0);
        let two = tile_height(1.5 / 9.0, 0.5 / 6.0);
        assert!(
            (one - two).abs() > 0.05,
            "the courses do not run up the slope: {one:.3} vs {two:.3}"
        );
    }
}
