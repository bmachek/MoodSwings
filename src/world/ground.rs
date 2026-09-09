//! The material the open ground is drawn with.
//!
//! A [`StandardMaterial`] extended with variation at a scale no tiled texture
//! has. See `assets/shaders/ground.wgsl` for the reasoning; briefly, a grass
//! scan tiled every three and a half metres is right at three and a half metres
//! and identical at three and a half kilometres, so the land around a town read
//! off a map came out as one flat green plain — which is the largest surface in
//! an aerial framing and was the one surface with nothing on it.
//!
//! Two constructors, and which one goes where matters:
//!
//! * [`GroundBreakup::plain`] is the open ground — the one forty-kilometre
//!   quad. It is the only one that reads the town mask, because it is the only
//!   surface that spans both a market square and a field two kilometres out.
//! * [`GroundBreakup::patch`] is a park lawn or a garden: tens of metres
//!   across, bounded by kerbs on all four sides, and mown. There is no room in
//!   it for a feature a hundred metres wide, and the first version of this
//!   module said so and then handed every lawn `default()` anyway — a
//!   sixty-two-metre lattice over a twenty-five-metre lawn is not variation,
//!   it is one smooth corner of the noise field, so every park came out a
//!   slightly different flat green from its neighbour. `patch` sizes the
//!   feature to the thing it is drawn on and turns the drying right down,
//!   because a lawn somebody mows does not go to straw.
//!
//! `Default` is `patch`, deliberately: a lawn is what most callers want, and
//! the one caller that wants the plain is the one that also has the mask to
//! hand and so cannot use a default anyway.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

const SHADER: &str = "shaders/ground.wgsl";

/// Metres across one repeat of the largest scale of variation.
///
/// The size of a field, near enough, and the first value was three times this
/// and invisible — because the gap between two streets in this town is about a
/// hundred and fifty metres, so a feature a hundred and sixty metres across is
/// one smooth lattice cell filling the whole of what you can see of the ground.
/// Variation you cannot see two of is not variation.
///
/// Below about forty it stops reading as terrain and starts reading as a stain
/// on the grass.
const FIELD: f32 = 62.0;

/// And across one repeat of a lawn's variation. A plot wide, not a field wide.
const PATCH: f32 = 9.0;

/// What a grass scan has to be multiplied by before it is turf.
///
/// The same argument as `buildings::ROOF_TINT`, and it was missed for the same
/// length of time: `ScannedSet::apply` leaves `base_color` at white, so a
/// material that does not tint after it is drawn at the photograph's own
/// albedo. `Grass005_2K-JPG_Color.jpg` has a mean linear albedo of
/// (0.132, 0.230, 0.025) — a daylit lawn, correctly exposed, and about two and
/// a half times as bright as real turf and five times as saturated. Every
/// other surface in the city was already tinted; grass was the one that was
/// not, and it is also the largest surface in the game, which is why the town
/// read as buildings standing in a field of poster paint.
///
/// The numbers are the ratio between what the scan measures and what turf
/// actually is, near enough (0.075, 0.094, 0.025) linear. Blue stays at one
/// because the scan has almost none and a multiplier cannot add any back —
/// that half of the job belongs to `GroundSettings::saturation`, in the
/// shader, and neither half works without the other.
pub const TURF_TINT: Color = Color::linear_rgb(0.57, 0.41, 1.0);

/// And what to paint it when no scan was downloaded.
///
/// Lower and greyer than the green it replaced: the painted fallback was
/// srgb(0.29, 0.43, 0.24), which is two parts green to one part red — the same
/// poster paint the scan was guilty of, arrived at independently.
pub const TURF_PAINT: Color = Color::srgb(0.30, 0.34, 0.24);

#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
pub struct GroundSettings {
    pub tile: f32,
    /// How far the variation may move the ground's value, either way.
    pub value: f32,
    /// How much of the high ground has dried out.
    pub dry: f32,
    /// How much of the surface's own hue survives the pull towards its
    /// luminance.
    ///
    /// The one axis a `base_color` tint cannot reach. The grass scan's mean
    /// linear albedo is (0.132, 0.230, 0.025) — nine parts green to one part
    /// blue, where real turf is nearer two — and a multiplier can only ever
    /// take channels away, so no tint on earth can put the blue back. Pulling
    /// towards luminance can, and it is the difference between poster paint
    /// and grass.
    pub saturation: f32,
    /// How much bare earth comes through where the cover is thin.
    pub dirt: f32,
    /// How loudly the town mask is allowed to speak, 0 to 1.
    ///
    /// Zero for anything that is not the world plain. A lawn inside a park has
    /// no business asking whether it is in a town — it *is* in a town, and it
    /// is a mown lawn regardless — and the binding falls back to a white
    /// texture when no mask is supplied, which without this would read as
    /// "maximally urban everywhere".
    pub urban: f32,
    /// Metres from the middle of the world to the edge of the town mask.
    pub half_extent: f32,
    pub pad: f32,
}

impl GroundSettings {
    /// The open ground: fields outside the town, backland inside it.
    fn plain() -> Self {
        Self {
            tile: FIELD,
            // A third either way, which on paper is a lot and on grass is
            // barely anything — see the note in the shader about where sRGB
            // spends its codes. The variation that carries this is `dry`, and
            // now also `dirt`.
            value: 0.34,
            // Two thirds rather than the eighty-five the multiply version ran
            // at: drying now moves towards a fixed straw instead of scaling
            // the grass, so the same number goes very much further.
            dry: 0.62,
            // Just over half. Below about 0.4 the plain goes olive-grey and
            // reads as ash rather than as grass; above 0.7 the scan's own
            // green comes back and with it the poster paint.
            saturation: 0.58,
            dirt: 0.18,
            urban: 1.0,
            half_extent: 1000.0,
            pad: 0.0,
        }
    }

    /// A lawn: a park, a garden, anything with a kerb round it.
    fn patch() -> Self {
        Self {
            tile: PATCH,
            value: 0.16,
            // A mown lawn does not go to straw. It goes patchy, which `dirt`
            // does and `dry` does not.
            dry: 0.12,
            saturation: 0.62,
            dirt: 0.16,
            urban: 0.0,
            half_extent: 1000.0,
            pad: 0.0,
        }
    }
}

impl Default for GroundSettings {
    fn default() -> Self {
        Self::patch()
    }
}

#[derive(Asset, AsBindGroup, Reflect, Clone, Default)]
pub struct GroundBreakup {
    #[uniform(100)]
    pub settings: GroundSettings,
    /// Where the town is, rasterised once at startup — see
    /// `world::urban_mask`. Red is the backland right behind a pavement, green
    /// is anywhere inside the built-up envelope.
    ///
    /// `Option`, and it has to be: the derive turns a bare `Handle` into a
    /// hard `RetryNextUpdate` when the handle names nothing, so a lawn with no
    /// mask would simply never draw. `None` binds the white fallback instead,
    /// which `settings.urban = 0` then ignores.
    #[texture(101)]
    #[sampler(102)]
    pub town: Option<Handle<Image>>,
    /// What the town's own floor is made of.
    ///
    /// The first pass at this varied the ground's *colour* by the mask and
    /// nothing else, and a colour is not a surface: the only texture bound was
    /// grass, so a courtyard forty metres across came out as a large smooth
    /// beige plane with a soft edge on it — which reads worse than the meadow
    /// it replaced, because meadow at least has a photograph under it. Grit is
    /// a photograph too, and mixing towards it rather than towards a constant
    /// is the difference between a yard and a stain.
    ///
    /// Sampled in the same UVs as the grass, so it tiles at the same few
    /// metres and needs no second set of coordinates.
    #[texture(103)]
    #[sampler(104)]
    pub yard: Option<Handle<Image>>,
}

impl GroundBreakup {
    /// The open ground, with the town rasterised into it and something for its
    /// floor to be made of.
    pub fn plain(town: Handle<Image>, yard: Handle<Image>, half_extent: f32) -> Self {
        Self {
            settings: GroundSettings {
                half_extent,
                ..GroundSettings::plain()
            },
            town: Some(town),
            yard: Some(yard),
        }
    }

    /// A lawn: something a plot wide with a kerb round it.
    pub fn patch() -> Self {
        Self {
            settings: GroundSettings::patch(),
            town: None,
            yard: None,
        }
    }
}

impl MaterialExtension for GroundBreakup {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    /// The same file, branching on `PREPASS_PIPELINE`. Not optional: the ground
    /// is opaque and therefore shades through the g-buffer, so a variation that
    /// never reached it would be a variation the lighting pass never saw.
    fn deferred_fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub type GroundMaterial = ExtendedMaterial<StandardMaterial, GroundBreakup>;

pub struct GroundPlugin;

impl Plugin for GroundPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<GroundMaterial>::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lawn_never_reads_the_town_mask() {
        // It has none to read, and the binding's fallback is a white texture:
        // a lawn that listened to it would come out as courtyard grit in the
        // middle of a park.
        let lawn = GroundBreakup::patch();
        assert!(lawn.town.is_none());
        assert_eq!(lawn.settings.urban, 0.0);
        assert_eq!(GroundBreakup::default().settings.urban, 0.0);
    }

    #[test]
    fn a_lawns_variation_is_a_plot_wide_and_the_plains_is_a_field_wide() {
        // The module docstring's whole argument. A lawn is tens of metres
        // across; a feature sixty metres wide laid over it is a flat colour
        // offset, which is what made every park a different green.
        assert!(GroundSettings::patch().tile < 20.0);
        assert!(GroundSettings::plain().tile > 40.0);
        // And a mown lawn does not go to straw.
        assert!(GroundSettings::patch().dry < GroundSettings::plain().dry * 0.25);
    }

    #[test]
    fn nothing_runs_at_the_scans_own_saturation() {
        // The grass scan is nine parts green to one part blue. Every ground
        // material has to pull some of that out, because a `base_color`
        // multiplier can only ever take colour away and the missing channel is
        // the one it would have to add.
        for settings in [GroundSettings::plain(), GroundSettings::patch()] {
            assert!(settings.saturation > 0.0 && settings.saturation < 0.75);
        }
    }
}
