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
///
/// Nineteen thousand was the low end of the plausible range and it left the
/// brightest surface in a cloudy frame at 0.48 of the camera's white point —
/// so the one thing in the whole picture that a photographer would expect to be
/// clipping came out as light grey, and the frame had no white in it anywhere.
/// A sunlit cumulus top really is thirty to fifty thousand candela per square
/// metre; at forty-two thousand it lands just over the white point at the noon
/// aperture, which is a cloud that blows out where the sun is straight on it and
/// holds detail everywhere else — and, being over `render::BLOOM_THRESHOLD`, is
/// finally something for the bloom pass to find in daylight.
const SUNLIT_NITS: f32 = 42_000.0;
/// And the same cloud's own shadow. A quarter, roughly — cloud is a very
/// efficient scatterer, so its shaded side is much brighter relative to its lit
/// side than any solid object's is. Raised with the lit side and by less than
/// it, so the deck gains contrast rather than merely gaining brightness: the
/// ratio was 3.65 and is now 4.7.
const SHADE_NITS: f32 = 9_000.0;
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

// ------------------------------------------------------- the same field ----

/// Metres across one repeat of the cloud field. Must match `SCALE` in
/// `sky.wgsl`; see [`shade`] for why the two are the same field rather than
/// two fields that look alike.
const SCALE: f32 = 2600.0;

/// How much of the direct beam a solid cloud takes.
///
/// Nearly all of it. What is left under a cumulus at noon is skylight, and the
/// skylight is the environment map's job — it is not dimmed here, which is why
/// the shadow of a cloud is blue rather than black.
const CLOUD_SHADE: f32 = 0.86;

/// `world::texture::hash`, and `hash2` in `sky.wgsl`. Three copies of eight
/// lines, and they have to be *bit-identical*: see the note in the shader.
fn hash2(x: i32, y: i32) -> f32 {
    let mut h =
        (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ 0xC2B2_AE3D;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_F491);
    h ^= h >> 13;
    h as f32 / u32::MAX as f32
}

fn value_noise(p: Vec2) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let (x, y) = (i.x as i32, i.y as i32);
    let a = hash2(x, y);
    let b = hash2(x + 1, y);
    let c = hash2(x, y + 1);
    let d = hash2(x + 1, y + 1);
    a.lerp(b, u.x).lerp(c.lerp(d, u.x), u.y)
}

fn fbm(p: Vec2) -> f32 {
    let mut sum = 0.0;
    let mut amplitude = 0.5;
    let mut total = 0.0;
    let mut at = p;
    for _ in 0..5 {
        sum += value_noise(at) * amplitude;
        total += amplitude;
        at = at * 2.17 + Vec2::new(19.3, -7.1);
        amplitude *= 0.52;
    }
    sum / total
}

/// How much of the sun reaches a point on the ground, 0 to 1.
///
/// The one thing the sky knew and the ground did not. A deck of cloud that
/// throws no shadow is a painting on a dome, and the giveaway is not the sky —
/// it is that a street stays evenly lit while a cumulus visibly crosses the sun.
///
/// This is the *same field* the shader draws, evaluated where the sun's ray
/// leaves the deck above `at`, which is why the hash had to become integer
/// arithmetic: two fields that merely looked alike would put the shadow
/// somewhere other than under the cloud, and that is worse than no shadow.
///
/// One sample, not a shadow map. The whole visible city is a kilometre across
/// and a cloud is two, so within a framing the shading is very nearly uniform;
/// what this buys is not a moving edge on the ground but the thing that
/// actually reads — the light coming and going as the sky moves over.
pub fn shade(at: Vec2, sun: Vec3, drift: Vec2, coverage: f32) -> f32 {
    // A sun on the horizon casts its shadow from a cloud a very long way away,
    // and past the point where this deck is a plausible model of the sky. It is
    // also the hour at which nothing is lit by the beam anyway.
    if sun.y < 0.12 {
        return 1.0;
    }
    let along = DECK / sun.y;
    let here = (at + Vec2::new(sun.x, sun.z) * along + drift) / SCALE;

    // The same threshold the shader uses to decide where cloud is.
    let line = 0.615f32.lerp(0.185, coverage.clamp(0.0, 1.0));
    let mass = ((fbm(here) - line) / 0.155).clamp(0.0, 1.0);
    let mass = mass * mass * (3.0 - 2.0 * mass);
    1.0 - mass * CLOUD_SHADE
}

/// How much of the sun is getting through, where the player is standing.
///
/// A resource rather than a direct write to the light, because the sun belongs
/// to `world::timeofday` — it is the module that knows what an hour means — and
/// two systems writing one `DirectionalLight` is how a day/night cycle starts
/// disagreeing with itself.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CloudShade(pub f32);

impl Default for CloudShade {
    fn default() -> Self {
        Self(1.0)
    }
}

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .init_resource::<CloudShade>()
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
    mut shading: ResMut<CloudShade>,
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
    shading.0 = self::shade(
        Vec2::new(eye.x, eye.z),
        Vec3::from(sun),
        *drift,
        weather.cover,
    );

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

    /// A cloud has to shade the ground *under itself*, and the only way to be
    /// sure of that is for the two evaluations of the field to be the same
    /// arithmetic. This pins the half that can be tested without a GPU: the
    /// field is deterministic, bounded, and continuous — a hash that had gone
    /// back to `sin` would still pass the first two and fail the third, because
    /// a last-bit difference in the input would land in a different cell.
    #[test]
    fn the_cloud_field_is_the_same_field_twice_and_is_smooth_between_samples() {
        let sun = Vec3::new(0.3, 0.8, 0.2).normalize();
        let at = |x: f32| shade(Vec2::new(x, 40.0), sun, Vec2::new(12.0, -3.0), 0.6);

        for x in [-4000.0f32, -12.5, 0.0, 337.0, 9000.0] {
            let once = at(x);
            assert_eq!(once, at(x), "the field is not deterministic at {x}");
            assert!((0.0..=1.0).contains(&once), "{once} at {x}");
        }

        // A metre apart is a four-thousandth of a cloud, so two samples that
        // close must agree to well within the shading they can produce.
        let mut worst = 0.0f32;
        for step in 0..400 {
            let x = step as f32 * 7.0;
            worst = worst.max((at(x) - at(x + 1.0)).abs());
        }
        assert!(worst < 0.02, "the field jumps by {worst} over a metre");
    }

    /// The sun is not dimmed by cloud that is not there, and is by cloud that
    /// is. Both directions matter: a shadow under a clear sky is the exact
    /// failure a mismatched field would produce.
    #[test]
    fn cover_is_what_decides_whether_the_sun_is_behind_something() {
        let sun = Vec3::new(0.25, 0.85, 0.46).normalize();
        let sample = |cover: f32| {
            let mut dimmest = 1.0f32;
            let mut lit = 0;
            for step in 0..300 {
                let at = Vec2::new(step as f32 * 130.0, step as f32 * -70.0);
                let shade = shade(at, sun, Vec2::ZERO, cover);
                dimmest = dimmest.min(shade);
                if shade > 0.999 {
                    lit += 1;
                }
            }
            (dimmest, lit)
        };

        let (_, clear_lit) = sample(0.0);
        let (heavy_dimmest, heavy_lit) = sample(1.0);
        // What separates the two skies is *how much* of the ground is in full
        // sun, not how dark the darkest patch gets: a clear sky in this field
        // still has the odd thick cloud in it, and under one of those the beam
        // is as gone as it is under an overcast.
        assert!(
            clear_lit > 150,
            "a clear sky shadowed {}/300",
            300 - clear_lit
        );
        assert!(
            heavy_lit < 30,
            "an overcast left {heavy_lit}/300 in full sun"
        );
        assert!(heavy_dimmest > 0.05, "an overcast put the sun out entirely");
    }

    /// A sun on the horizon would take its shadow from cloud kilometres away,
    /// where a flat deck stops being a plausible sky at all — and it is the
    /// hour at which there is no beam left to dim.
    #[test]
    fn a_low_sun_is_left_alone() {
        let low = Vec3::new(0.99, 0.05, 0.0).normalize();
        assert_eq!(shade(Vec2::new(100.0, 20.0), low, Vec2::ZERO, 1.0), 1.0);
    }

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

    /// The one thing a sunlit frame in this game never had: something over the
    /// white point. Cloud is the brightest surface in any frame that has some,
    /// and it is one of the few things here quoted in honest radiance — the sky
    /// shader multiplies by `view.exposure` by hand — so the check is arithmetic
    /// rather than a screenshot. Under the old 19,000 the brightest thing in a
    /// cloudy noon landed at 0.48 and there was no white anywhere in the image.
    #[test]
    fn a_sunlit_cloud_clips_at_the_noon_aperture() {
        use crate::render::{BLOOM_THRESHOLD, DAY_EV100, exposure};

        let (lit, shade) = tint(12.0, 0.4);
        let brightest = lit.x * exposure(DAY_EV100);
        assert!(
            brightest > BLOOM_THRESHOLD,
            "the brightest cloud in the sky reads {brightest} against a white point of 1.0"
        );
        // And its own shadow does not, or the deck is one flat white sheet.
        assert!(shade.x * exposure(DAY_EV100) < 0.5);
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
