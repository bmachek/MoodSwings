//! Cloud.
//!
//! Everything the weather did was already right except the thing you would
//! photograph it against. `weather` moves a cover value, and off it hang the
//! sun dimming, the shadows softening, the haze closing in and the windows
//! coming on for a dark afternoon — while the sky itself stayed a clean blue
//! gradient at every hour and in every downpour, because Bevy's atmosphere is a
//! scattering model and a scattering model has no clouds in it. The largest
//! surface in most frames was the one surface with nothing on it.
//!
//! So: a dome, drawn on the inside, moved onto the camera every frame so it is
//! effectively at infinity, carrying one horizontal deck of cloud that the view
//! ray is intersected against. The shape is `assets/shaders/sky.wgsl`'s
//! business; this module owns the two things a shader has no way to know.
//!
//! **What colour a cloud is.** It is not one colour and it is not a constant.
//! The lit side of a cumulus at noon is fifteen or twenty thousand candela per
//! square metre and its own shadow is a quarter of that; an hour before sunset
//! both are amber and half as bright; after dark the underside of an overcast
//! over a city is lit sodium-orange from below by the city itself, which is one
//! of the few genuinely beautiful things a night sky does and costs one `mix`
//! here. Those are radiances, in the same nits the rest of the world is lit in,
//! because the material is unlit and what it writes goes into an HDR buffer
//! that a real exposure divides — anything normalised to one comes out black.
//!
//! **Where the deck has got to.** Wind moves it, and the offset is accumulated
//! here rather than computed in the shader from a clock: a shader multiplying a
//! rising time by a wind speed runs out of mantissa within an hour of play and
//! the cloud starts to judder.
//!
//! The dome is five kilometres across, which is past the far plane the frustum
//! is culled against — hence [`NoFrustumCulling`]. It is not past the *clip*
//! plane, because Bevy's perspective projection is an infinite reverse-Z one and
//! has no far clip at all; `far` is a culling distance and nothing else. Getting
//! that the wrong way round costs an afternoon looking for a shader bug in a
//! dome that was never submitted.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::player::camera::CameraRig;

use super::timeofday::{TimeOfDay, daylight, sun_direction, sun_elevation};
use super::weather::Weather;

const SHADER: &str = "shaders/sky.wgsl";

/// Radius of the dome, in metres. Beyond anything the streamer will ever spawn,
/// so a tower never pokes through the cloud it is supposed to be under.
const DOME: f32 = 5_000.0;

/// Metres from the ground to the base of the deck.
///
/// Low, for cloud: a cumulus base sits nearer two thousand. Bringing it down
/// makes the deck cover more of the sky for the same field, and — because the
/// ray crosses it sooner — puts more of the field's own detail overhead where it
/// can be seen, rather than compressed into the horizon where it cannot.
const DECK: f32 = 1_150.0;

/// How fast the deck blows across the sky, in metres per second of wall clock.
///
/// Faster than any wind actually is at cloud height, and deliberately: the deck
/// is over a kilometre up, so a real fifteen metres a second subtends about a
/// degree a minute and reads as perfectly still. This is the one number here
/// that is a lie rather than a measurement.
const WIND: f32 = 34.0;

/// The lit side of cloud at the top of the day, in cd/m².
const SUNLIT_NITS: f32 = 19_000.0;
/// And the same cloud's own shadow. A quarter, roughly — cloud is a very
/// efficient scatterer, so its shaded side is much brighter relative to its lit
/// side than any solid object's is.
const SHADE_NITS: f32 = 5_200.0;
/// What an overcast holds over a city after dark, lit from below by the city.
///
/// Small in absolute terms and not small at all once the aperture has opened
/// five stops for the night: this is what makes a rainy night sky orange
/// instead of black, which is most of what a rainy night sky is.
const GLOW_NITS: f32 = 26.0;

#[derive(Clone, Copy, Debug, ShaderType, Reflect)]
pub struct SkySettings {
    pub sunlit: Vec4,
    pub shade: Vec4,
    /// Camera position in `xyz`; `w` is unused and exists because a `vec3` in a
    /// uniform is padded to four floats whether or not it is written as one.
    pub eye: Vec4,
    /// Direction to the sun in `xyz`, how far up the day is in `w`.
    pub sun: Vec4,
    pub drift: Vec2,
    pub coverage: f32,
    pub height: f32,
}

impl Default for SkySettings {
    fn default() -> Self {
        Self {
            sunlit: Vec4::splat(1.0),
            shade: Vec4::splat(0.5),
            eye: Vec4::ZERO,
            sun: Vec4::new(0.0, 1.0, 0.0, 1.0),
            drift: Vec2::ZERO,
            coverage: 0.0,
            height: DECK,
        }
    }
}

#[derive(Asset, AsBindGroup, Reflect, Clone, Default)]
pub struct SkyMaterial {
    #[uniform(0)]
    pub settings: SkySettings,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    /// Blended, so the deck composites over the atmosphere behind it and a thin
    /// cloud is thin rather than a grey patch. That also puts it in the
    /// transparent phase, which is where it belongs for a second reason: it is
    /// then depth-tested against the city without writing depth, so a building
    /// in front of it occludes it and nothing behind it is disturbed.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}

/// Marks the dome, so it can be found and moved.
#[derive(Component)]
struct SkyDome;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .add_systems(Startup, spawn_dome)
            .add_systems(Update, drive_the_sky);
    }
}

fn spawn_dome(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    commands.spawn((
        Name::new("Sky"),
        SkyDome,
        // Turned inside out, which is the whole of what makes it visible: the
        // camera stands at the centre, so every triangle it can see is a *back*
        // face of the sphere and back faces are culled. Reversing the winding
        // rather than asking the pipeline to cull the other way, because cull
        // mode on a plain `Material` means implementing `specialize` and that
        // signature moves between Bevy versions where a list of indices does
        // not.
        //
        // Three subdivisions is plenty. Nothing about the shading varies across
        // a triangle — every fragment recomputes its own ray — so the mesh is
        // only there to cover the screen.
        Mesh3d(
            meshes.add(inside_out(
                Sphere::new(DOME)
                    .mesh()
                    .ico(3)
                    .unwrap_or_else(|_| Sphere::new(DOME).mesh().uv(24, 16)),
            )),
        ),
        MeshMaterial3d(materials.add(SkyMaterial::default())),
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

/// Reverses a mesh's triangle winding, so it faces the space it encloses.
fn inside_out(mut mesh: Mesh) -> Mesh {
    match mesh.indices_mut() {
        Some(bevy::mesh::Indices::U16(indices)) => {
            for triangle in indices.as_chunks_mut::<3>().0 {
                triangle.swap(1, 2);
            }
        }
        Some(bevy::mesh::Indices::U32(indices)) => {
            for triangle in indices.as_chunks_mut::<3>().0 {
                triangle.swap(1, 2);
            }
        }
        None => warn!("the sky dome has no indices; it will not be drawn"),
    }
    mesh
}

/// Cloud colours for an hour and a sky.
///
/// Split out from the system so the interesting half is a pure function over
/// two numbers: the lit and shaded radiance of cloud, in nits. Returned rather
/// than written, so a test can ask what midnight looks like without a renderer.
fn tint(hours: f32, cover: f32) -> (Vec3, Vec3) {
    let day = daylight(hours);
    let cover = cover.clamp(0.0, 1.0);
    // Warm when the sun is low, and only while it is still up: past sunset the
    // light on a cloud is scattered sky rather than a beam, and it goes cold
    // long before it goes dark.
    let elevation = sun_elevation(hours);
    let low = (1.0 - (elevation / 0.30).abs().min(1.0)).max(0.0) * day;

    let white = Vec3::new(1.0, 0.985, 0.955);
    let amber = Vec3::new(1.0, 0.66, 0.40);
    let lit_hue = white.lerp(amber, low * 0.85);
    // The shaded side of a cloud is lit by the sky, so it is blue — and it is
    // blue in proportion to how little of the sun is reaching it, which is
    // why an overcast's underside is the greyest thing in the frame.
    let shade_hue = Vec3::new(0.60, 0.66, 0.80)
        .lerp(Vec3::new(0.72, 0.72, 0.74), cover)
        .lerp(amber, low * 0.35);

    // Sodium, from underneath, and only where there is enough cloud to catch
    // it. A clear night sky is not orange.
    let city = Vec3::new(1.0, 0.62, 0.30) * GLOW_NITS * (0.25 + 0.75 * cover) * (1.0 - day);

    let lit = lit_hue * SUNLIT_NITS * day + city;
    let shade = shade_hue * SHADE_NITS * day + city;
    (lit, shade)
}

fn drive_the_sky(
    time: Res<Time>,
    clock: Res<TimeOfDay>,
    weather: Res<Weather>,
    cameras: Query<&GlobalTransform, With<CameraRig>>,
    mut dome: Query<(&mut Transform, &MeshMaterial3d<SkyMaterial>), With<SkyDome>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut drift: Local<Vec2>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    let eye = camera.translation();

    // Wind direction is fixed rather than drawn from anywhere. `weather` has no
    // wind vector to ask — the trees take their sway from a speed alone — and
    // inventing one here would be a second source of truth about which way the
    // weather is going.
    *drift += Vec2::new(0.82, 0.57).normalize() * WIND * time.delta_secs();

    let (lit, shade) = tint(clock.hours, weather.cover);
    let sun = sun_direction(clock.hours);

    for (mut transform, handle) in &mut dome {
        transform.translation = eye;
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        material.settings = SkySettings {
            sunlit: lit.extend(1.0),
            shade: shade.extend(1.0),
            eye: eye.extend(1.0),
            sun: Vec3::from(sun).extend(daylight(clock.hours)),
            drift: *drift,
            coverage: weather.cover.clamp(0.0, 1.0),
            height: DECK,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lit side of a cloud is brighter than its own shadow at every hour
    /// there is any sun at all. If that ever inverts the deck reads as a
    /// photographic negative, and it is exactly the sort of thing a sign error
    /// in the hue mixing would do without changing anything else.
    #[test]
    fn cloud_is_always_brighter_where_the_sun_is_on_it() {
        for step in 0..48 {
            let hours = step as f32 * 0.5;
            if daylight(hours) < 0.05 {
                continue;
            }
            for cover in [0.0, 0.5, 1.0] {
                let (lit, shade) = tint(hours, cover);
                assert!(
                    lit.length() > shade.length(),
                    "hour {hours} cover {cover}: lit {lit:?} shade {shade:?}"
                );
            }
        }
    }

    /// Cloud is lit by the sun, so it goes out with the sun. What is left at
    /// midnight is the city's own glow on the underside of an overcast — small,
    /// but not nothing, and not present at all under a clear sky.
    #[test]
    fn the_night_keeps_only_what_the_city_throws_up_at_it() {
        let (clear, _) = tint(1.0, 0.0);
        let (overcast, _) = tint(1.0, 1.0);
        assert!(clear.length() < overcast.length());
        assert!(overcast.length() < GLOW_NITS * 2.0, "midnight is not day");
        assert!(
            clear.length() > 0.0,
            "even a clear night has some sky in it"
        );
    }

    /// Noon is the brightest cloud there is, and dusk is warmer than noon.
    #[test]
    fn a_low_sun_makes_amber_cloud_and_a_high_one_makes_white() {
        let (noon, _) = tint(12.0, 0.0);
        let (dusk, _) = tint(17.6, 0.0);
        assert!(
            noon.length() > dusk.length(),
            "dusk is not dimmer than noon"
        );
        assert!(
            dusk.x / dusk.z > noon.x / noon.z * 1.2,
            "dusk is not warmer than noon"
        );
    }
}
