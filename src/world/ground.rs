//! The material the open ground is drawn with.
//!
//! A [`StandardMaterial`] extended with variation at a scale no tiled texture
//! has. See `assets/shaders/ground.wgsl` for the reasoning; briefly, a grass
//! scan tiled every three and a half metres is right at three and a half metres
//! and identical at three and a half kilometres, so the land around a town read
//! off a map came out as one flat green plain — which is the largest surface in
//! an aerial framing and was the one surface with nothing on it.
//!
//! Only the open ground, not a park lawn. A park is tens of metres across and
//! its grass is bounded by kerbs on all four sides; there is no room in it for a
//! feature a hundred metres wide, and adding one would only make the lawn look
//! blotchy. The plain is where the missing scale actually is.

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

#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
pub struct GroundSettings {
    pub tile: f32,
    /// How far the variation may move the ground's value, either way.
    pub value: f32,
    /// How much of the high ground has dried out.
    pub dry: f32,
    pub pad: f32,
}

impl Default for GroundSettings {
    fn default() -> Self {
        Self {
            tile: FIELD,
            // A third either way, which on paper is a lot and on grass is
            // barely anything — see the note in the shader about where sRGB
            // spends its codes. The variation that carries this is `dry`.
            value: 0.34,
            dry: 0.85,
            pad: 0.0,
        }
    }
}

#[derive(Asset, AsBindGroup, Reflect, Clone, Default)]
pub struct GroundBreakup {
    #[uniform(100)]
    pub settings: GroundSettings,
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
