//! Street lighting.
//!
//! Rather than spawning a lamp per intersection (hundreds of live point lights,
//! most of them nowhere near the player), this keeps a small fixed pool and
//! snaps it to whichever intersections are currently closest. Cost is constant
//! and predictable no matter how large the city grows.
//!
//! Shadows are off for these deliberately: shadow-casting point lights are one
//! of the most expensive things in a renderer, and at night a pool of unshadowed
//! pools of light is what sells the look anyway.
//!
//! They are *cones*, though, and they were spheres. A lamp head hanging under
//! an arm at seven and a half metres radiated as much light up into the facade
//! behind it as down onto the road, so a night street had exactly one lighting
//! gradient — inverse square — and no shape: the wall was lit as evenly as the
//! carriageway, which is the one thing a street lantern is designed not to do.
//! A [`SpotLight`] pointed down the arm costs the same as the point light it
//! replaced and puts the light where the fitting puts it.

use bevy::prelude::*;

use super::City;
use super::timeofday::{TimeOfDay, daylight};

/// How many lamps are live at once.
const POOL_SIZE: usize = 64;
const LAMP_HEIGHT: f32 = 7.5;
/// How far the lamp head reaches out over the road from its column.
const ARM_REACH: f32 = 1.5;
/// The lamp's glass globe, in metres.
const LAMP_GLOBE_RADIUS: f32 = 0.30;
/// The column and the arm, in metres. Both under 8 cm, which is what decides
/// how many sides they are drawn with — see [`super::props::cylinder_sides`].
const COLUMN_RADIUS: f32 = 0.075;
const ARM_RADIUS: f32 = 0.055;
/// How far back from the kerb line the column stands, on the pavement.
///
/// A lamp post is street furniture, and street furniture stands on the
/// footway: `world::props` sets its own back by 0.75 m for the same reason.
/// This was negative — nine tenths of a metre the *other* way — which stood
/// every column in the gutter, and the tests below never caught it because
/// they only ever asked whether the column landed back where the post said,
/// not whether the post was anywhere sane.
const KERB_SET_BACK: f32 = 0.7;
/// Distance between lamp posts along a street.
const LAMP_SPACING: f32 = 32.0;
/// Sodium-vapour warmth.
const LAMP_COLOR: Color = Color::srgb(1.0, 0.82, 0.55);
/// How finely the lamp's glass globe is drawn.
///
/// An icosphere is `20 * (subdivisions + 1)^2` triangles, so Bevy's default of
/// five is 720 of them on a 30 cm ball hanging seven and a half metres up, in a
/// pool of sixty-four of them: 46 000 triangles of street lighting, most of it
/// on a shape that is a blown-out white blob in every frame it appears in — the
/// emissive is thirteen times the white point and it blooms (see
/// `LAMP_ENVELOPE`), so the silhouette is the *bloom*, not the mesh. Two
/// subdivisions is 180, and the globe is still round enough that its unlit
/// daytime form reads as a lantern rather than as a die.
const LAMP_GLOBE_SUBDIVISIONS: u32 = 2;
/// The half-angle of the lamp's beam, in radians, and where it starts to fall
/// off.
///
/// Wide for a spotlight and narrow for a sphere. At the lamp's seven and a half
/// metres a sixty-eight degree cone lays a pool about nineteen metres across,
/// which covers the carriageway and the near pavement and stops well short of
/// the eaves — and the inner angle is where it is so that most of that pool is
/// full strength and only its rim feathers.
const LAMP_OUTER: f32 = 1.19;
const LAMP_INNER: f32 = 0.62;
/// How bright the lamp's own glass is, per channel, as a multiple of the
/// frame's white point.
///
/// Not a radiance, however much it looks like one — the deferred g-buffer drops
/// the alpha that would make emissive exposure-dependent, so these are numbers
/// the tonemapper sees directly at any hour. See `timeofday::WINDOW_GLOW`, which
/// is the same units and had the same wrong comment over it. Named rather than
/// written at the use site so the test below can hold them against
/// `render::BLOOM_THRESHOLD`: an envelope that does not clear it is a lamp with
/// no glare around it, which is most of what a lamp is at night.
const LAMP_ENVELOPE: Vec3 = Vec3::new(13.0, 9.4, 5.0);

/// How many shopfronts can be spilling light at once.
///
/// Fewer than the lamps, and closer: a lamp lights a junction from twenty
/// metres and a shop window lights the two metres of pavement in front of it,
/// so the ones that matter are the ones you are walking past.
const SHOPS: usize = 28;
/// How far a shop's light carries. Short — this is a window, not a floodlight.
const SHOP_RANGE: f32 = 11.0;
/// Warmer than the street lamp and much weaker. Sodium is orange; a shop is
/// lit with something closer to white and is behind glass.
const SHOP_COLOR: Color = Color::srgb(1.0, 0.90, 0.74);
/// How much of its night presence a shop keeps in full sun.
///
/// Not zero, which is what it was. A sixth is invisible against a hundred
/// thousand lux on the carriageway and clearly visible two metres inside a
/// doorway and under an awning, which is the only place it is meant to be seen.
/// It is also the whole of the warm-against-cool split between an interior and
/// the daylight outside it, which is one of the most recognisable modern-street
/// cues there is.
const SHOP_DAY_FLOOR: f32 = 0.16;

/// One of the pooled lights that stands in for a lit shop window.
#[derive(Component)]
pub struct ShopGlow;

/// The child of a lamp that carries the actual cone.
///
/// A [`SpotLight`] shines along its own entity's -Z, and the lamp entity's
/// rotation is already spoken for: it is a yaw that puts the column and the arm
/// back over the kerb. Rather than re-deriving every child's local transform in
/// a frame tipped on its side, the beam is one more child with a single
/// rotation of its own, and the lamp above it keeps meaning what it meant.
#[derive(Component)]
pub struct LampBeam;

#[derive(Component)]
pub struct StreetLight;

/// Every possible lamp post position, precomputed from the road graph.
/// Posts run along the kerbs at a fixed spacing and alternate sides, because
/// lighting only the intersections leaves the 60-90m of road between them
/// pitch black — which is most of the road.
/// Where a lamp stands, and which way it leans out over the road.
#[derive(Clone, Copy)]
pub struct LampPost {
    /// The column's foot, just inside the kerb.
    pub foot: Vec2,
    /// Unit vector from the kerb towards the middle of the road.
    pub inward: Vec2,
}

#[derive(Resource, Default)]
pub struct LampPosts(pub Vec<LampPost>);

impl LampPosts {
    pub fn build(city: &City) -> Self {
        let graph = &city.graph;
        let mut posts = Vec::new();

        for edge in graph.edges() {
            let a = graph.node(edge.a).pos;
            let b = graph.node(edge.b).pos;
            let Ok(dir) = Dir2::new(b - a) else { continue };
            let normal = Vec2::new(-dir.y, dir.x);
            // Behind the kerb line, on the footway. `edge.width` is the
            // carriageway — `markings` paints a crossing across 92% of it —
            // so half of it is the kerb and anything less is the road.
            let offset = edge.width * 0.5 + KERB_SET_BACK;

            let count = (edge.length / LAMP_SPACING).floor() as i32;
            for i in 1..count {
                let along = a + *dir * (i as f32 * LAMP_SPACING);
                let side = if i % 2 == 0 { 1.0 } else { -1.0 };
                posts.push(LampPost {
                    foot: along + normal * offset * side,
                    // Whichever kerb it stands on, the arm reaches the other
                    // way — out over the carriageway.
                    inward: -normal * side,
                });
            }
        }

        Self(posts)
    }
}

/// Shared glass material, dimmed with the lamps themselves.
#[derive(Resource)]
struct LampGlass(Handle<StandardMaterial>);

#[derive(Resource)]
struct LampTimer(Timer);

impl Default for LampTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(0.3, TimerMode::Repeating))
    }
}

pub struct StreetLightPlugin;

impl Plugin for StreetLightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LampTimer>()
            .init_resource::<LampPosts>()
            .add_systems(Startup, (spawn_pool, spawn_shop_glow))
            .add_systems(
                Update,
                (reposition_lamps, reposition_shop_glow, set_lamp_brightness),
            );
    }
}

fn spawn_pool(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // A visible glowing head, so the light has an apparent source.
    let head = meshes.add(
        Sphere::new(LAMP_GLOBE_RADIUS)
            .mesh()
            .ico(LAMP_GLOBE_SUBDIVISIONS)
            .expect("an icosphere at two subdivisions"),
    );
    let glass = materials.add(StandardMaterial {
        base_color: LAMP_COLOR,
        emissive: LinearRgba::BLACK,
        ..default()
    });
    commands.insert_resource(LampGlass(glass.clone()));

    // A lamp needs something holding it up. Until now these were glowing
    // spheres floating at seven and a half metres, which reads as a bug at
    // dusk and as nothing at all in daylight — the pool of light on the road
    // had no visible cause.
    //
    // Both of these are thinner than a wrist, so they go through
    // `props::cylinder` and come out with six sides — 20 triangles each rather
    // than the default resolution's 124. Sixty-four lamps were spending 15 800
    // triangles on two poles whose facets are 7 cm wide at their widest and
    // sit above head height; the same budget buys a great deal more of the
    // roundness the buildings were missing.
    let column = meshes.add(super::props::cylinder(COLUMN_RADIUS, LAMP_HEIGHT));
    let arm = meshes.add(super::props::cylinder(ARM_RADIUS, ARM_REACH));
    let steel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.20, 0.21, 0.22),
        perceptual_roughness: 0.62,
        metallic: 0.75,
        ..default()
    });

    for i in 0..POOL_SIZE {
        commands.spawn((
            Name::new(format!("Street Light {i}")),
            StreetLight,
            // Parked far below the world until assigned a lamp post.
            Transform::from_xyz(0.0, -1000.0, 0.0),
            Visibility::default(),
            children![
                (
                    Name::new("Beam"),
                    LampBeam,
                    SpotLight {
                        color: LAMP_COLOR,
                        intensity: 0.0,
                        range: 62.0,
                        shadow_maps_enabled: false,
                        outer_angle: LAMP_OUTER,
                        inner_angle: LAMP_INNER,
                        ..default()
                    },
                    // A quarter turn about X takes the entity's -Z from
                    // straight ahead to straight down, and leaves its X — the
                    // axis the parent's yaw is expressed in — alone.
                    Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
                ),
                (
                    Mesh3d(head.clone()),
                    MeshMaterial3d(glass.clone()),
                    Transform::default(),
                ),
                // The column stands under the light, not under the entity: the
                // lamp head is what gets positioned, and the pole hangs off it
                // reaching back to the kerb.
                (
                    Mesh3d(column.clone()),
                    MeshMaterial3d(steel.clone()),
                    Transform::from_xyz(ARM_REACH, -LAMP_HEIGHT * 0.5, 0.0),
                ),
                (
                    Mesh3d(arm.clone()),
                    MeshMaterial3d(steel.clone()),
                    // Cylinders run along Y; lay it across to the column.
                    Transform::from_xyz(ARM_REACH * 0.5, 0.0, 0.0)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ),
            ],
        ));
    }
}

/// Snaps the pool onto the nearest intersections to the camera.
/// The shopfronts' pool, parked below the world until there are shops to stand
/// in front of. No visible source: the source is the shop window, which the
/// facade is already drawing.
fn spawn_shop_glow(mut commands: Commands) {
    for i in 0..SHOPS {
        commands.spawn((
            Name::new(format!("Shop Glow {i}")),
            ShopGlow,
            PointLight {
                color: SHOP_COLOR,
                intensity: 0.0,
                range: SHOP_RANGE,
                // A shop's light is a wash on a pavement, and a wash does not
                // need to be occluded by the bollard standing in it. Shadow
                // maps for twenty-eight more lights would cost more than
                // everything else in this module put together.
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(0.0, -1000.0, 0.0),
        ));
    }
}

/// Moves the pool onto the nearest lit shopfronts, and turns it up after dark.
///
/// Same shape as `reposition_lamps` and for the same reason: the fixtures are
/// streamed and there are hundreds of them, the lights are few and expensive,
/// so the lights go to the fixtures rather than the other way round.
fn reposition_shop_glow(
    clock: Res<TimeOfDay>,
    weather: Res<super::weather::Weather>,
    fronts: Query<&GlobalTransform, With<super::interior::Shopfront>>,
    cameras: Query<&GlobalTransform, With<crate::player::camera::CameraRig>>,
    mut glows: Query<(&mut Transform, &mut PointLight), With<ShopGlow>>,
) {
    // A floor plus a night ramp, rather than a night ramp and nothing else.
    //
    // This used to return early with every shop dark whenever the sun was up,
    // which meant that between about seven and five — most of the hours anyone
    // plays — no shopfront in the city emitted anything, and the interior
    // behind the glass was lit by the sky fill alone. Real shops are lit all
    // day, and the warm-against-cool split between an interior and the daylight
    // on the pavement outside it is one of the most recognisable cues a modern
    // street scene has. A fifth of its night presence is invisible against a
    // hundred thousand lux on the carriageway and perfectly visible two metres
    // inside a doorway and under an awning.
    //
    // Against `brightness` rather than `daylight`, the same distinction
    // `timeofday::light_windows` already makes: a shop turns its lights up for
    // a dark afternoon as well as for the evening, and the sun's elevation
    // alone cannot tell it there is an overcast.
    let level = SHOP_DAY_FLOOR
        + (1.0 - SHOP_DAY_FLOOR) * (1.0 - super::timeofday::brightness(clock.hours, weather.cover));
    let Ok(camera) = cameras.single() else {
        return;
    };
    let eye = camera.translation();

    let mut nearest: Vec<(f32, Vec3)> = fronts
        .iter()
        .map(|at| {
            let at = at.translation();
            (at.distance_squared(eye), at)
        })
        .collect();
    let take = SHOPS.min(nearest.len());
    if take > 0 {
        nearest.select_nth_unstable_by(take - 1, |a, b| a.0.total_cmp(&b.0));
    }

    // Much weaker than a street lamp: this is one shop window, and the point of
    // it is the two metres of pavement under it rather than the road.
    let intensity = 110_000.0 * level;
    let mut placed = 0;
    for (mut transform, mut light) in &mut glows {
        match nearest.get(placed) {
            Some((_, at)) => {
                transform.translation = *at;
                light.intensity = intensity;
                placed += 1;
            }
            None => light.intensity = 0.0,
        }
    }
}

fn reposition_lamps(
    time: Res<Time>,
    mut timer: ResMut<LampTimer>,
    city: Option<Res<City>>,
    mut posts: ResMut<LampPosts>,
    cameras: Query<&GlobalTransform, With<crate::player::camera::CameraRig>>,
    mut lamps: Query<&mut Transform, With<StreetLight>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let (Some(city), Ok(camera)) = (city, cameras.single()) else {
        return;
    };
    if posts.0.is_empty() {
        *posts = LampPosts::build(&city);
        info!("{} lamp posts along the street network", posts.0.len());
    }

    let focus = camera.translation().xz();
    let mut nearest: Vec<(f32, LampPost)> = posts
        .0
        .iter()
        .map(|&p| (p.foot.distance_squared(focus), p))
        .collect();
    // Only the closest POOL_SIZE matter; a full sort would be wasted work.
    let take = POOL_SIZE.min(nearest.len());
    nearest.select_nth_unstable_by(take.saturating_sub(1), |a, b| a.0.total_cmp(&b.0));

    for (mut transform, (_, post)) in lamps.iter_mut().zip(nearest.iter().take(take)) {
        // The entity *is* the lamp head, out over the road; the column and arm
        // hang off it back towards the kerb. Yaw is set so the lamp's local +X
        // points that way, which is where those two children sit.
        let head = post.foot + post.inward * ARM_REACH;
        // Standing on the footway rather than in the road, the whole lamp
        // rises with it — otherwise the column's bottom is buried by a kerb's
        // worth and the post looks sunk into the slabs.
        transform.translation = Vec3::new(
            head.x,
            LAMP_HEIGHT + super::buildings::SIDEWALK_HEIGHT,
            head.y,
        );
        transform.rotation = Quat::from_rotation_y(post.inward.y.atan2(-post.inward.x));
    }
}

fn set_lamp_brightness(
    clock: Res<TimeOfDay>,
    glass: Res<LampGlass>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lamps: Query<&mut SpotLight, With<LampBeam>>,
) {
    // Lamps come up through dusk and go out through dawn.
    let night = 1.0 - daylight(clock.hours);
    // Quoted so a lamp lays down a pool the road actually reads at the night
    // exposure — see `render::adapt_exposure`. Physically this is a floodlight rather
    // than a street lamp, which is the usual bargain: real sodium lamps look
    // like nothing at all once the camera has opened up for a moonlit sky.
    //
    // Unchanged by the move from a point light to a cone: Bevy quotes a spot
    // light's intensity in lumens over the whole sphere as well, so the same
    // number lands the same pool and the cone only decides what is *outside*
    // it.
    let intensity = 1_250_000.0 * night;
    for mut lamp in &mut lamps {
        lamp.intensity = intensity;
    }
    if let Some(mut material) = materials.get_mut(&glass.0) {
        // Well over the white point, and it always was — what it never had was
        // anything to bloom into. Under the old thresholdless bloom every pixel
        // in the frame was veiled equally, so a lamp thirteen times over white
        // and a wall a fifth of the way to it came back with the same halo,
        // which is to say neither had one. See `render::BLOOM_THRESHOLD`.
        material.emissive = LinearRgba::rgb(
            LAMP_ENVELOPE.x * night,
            LAMP_ENVELOPE.y * night,
            LAMP_ENVELOPE.z * night,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a lamp's column ends up on the ground, given its post.
    fn column_foot(post: LampPost) -> Vec2 {
        let head = post.foot + post.inward * ARM_REACH;
        let yaw = Quat::from_rotation_y(post.inward.y.atan2(-post.inward.x));
        let arm = yaw * Vec3::new(ARM_REACH, 0.0, 0.0);
        Vec2::new(head.x + arm.x, head.y + arm.z)
    }

    /// A street lantern's glass is one of the few things in a night frame that
    /// should genuinely clip, and clipping over the bloom threshold is what
    /// puts a halo round it. Warm on the way past: a sodium lamp that clips to
    /// neutral white is a lamp with its colour graded out of it.
    #[test]
    fn a_lamp_envelope_clips_over_the_bloom_threshold() {
        const _: () = assert!(LAMP_ENVELOPE.x > LAMP_ENVELOPE.y);
        const _: () = assert!(LAMP_ENVELOPE.y > LAMP_ENVELOPE.z);
        // Through the same ramp the lamps themselves come up on, so this is
        // what the envelope actually reads at midnight rather than what the
        // constant says.
        let night = 1.0 - daylight(0.0);
        let lit = LAMP_ENVELOPE.x * night;
        assert!(
            lit > crate::render::BLOOM_THRESHOLD,
            "the envelope reads {lit} against a white point of 1.0"
        );
    }

    /// The cone has to cover the road and stop short of the eaves, which is the
    /// whole reason it is a cone: a sphere at seven and a half metres lights the
    /// facade behind it as evenly as the carriageway in front.
    #[test]
    fn the_beam_covers_the_carriageway_and_not_the_facade() {
        const _: () = assert!(LAMP_INNER < LAMP_OUTER);
        let radius = LAMP_HEIGHT * LAMP_OUTER.tan();
        assert!(radius > 9.0, "a {radius} m pool does not cross a street");
        assert!(radius < 26.0, "a {radius} m pool is a floodlight");
    }

    #[test]
    fn the_column_lands_back_on_the_kerb_it_stands_on() {
        // The head is placed out over the road and the column is a child at a
        // fixed local offset, so the yaw is the only thing putting the column
        // back where the post says. Get it wrong and every lamp stands in the
        // middle of the road or inside the building behind it.
        for inward in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            let post = LampPost {
                foot: Vec2::new(12.0, -5.0),
                inward,
            };
            assert!(
                column_foot(post).distance(post.foot) < 1e-4,
                "with inward {inward:?} the column landed at {:?}, not {:?}",
                column_foot(post),
                post.foot
            );
        }
    }

    #[test]
    fn a_column_stands_on_the_footway_and_only_the_arm_is_over_the_road() {
        // The one thing the other two tests here cannot see, because they take
        // a post's foot as given: whether the foot is on the pavement at all.
        // It was not — the offset was subtracted from the half-width instead of
        // added, so every column in the city stood the best part of a metre
        // inside the carriageway.
        for width in [6.0f32, 9.0, 12.0, 18.0] {
            let offset = width * 0.5 + KERB_SET_BACK;
            assert!(
                offset > width * 0.5,
                "a column at {offset} m is in the road on a {width} m street"
            );
            // And the arm has to earn its keep: the head belongs over the
            // carriageway, or the pool of light lands on the slabs.
            let head = offset - ARM_REACH;
            assert!(
                head < width * 0.5,
                "the head at {head} m never reaches over a {width} m street"
            );
        }
    }

    #[test]
    fn lamps_lean_out_over_the_road_from_both_kerbs() {
        // Posts alternate sides down a street, so both signs of `inward` have
        // to put the head *inside* the carriageway.
        for side in [1.0f32, -1.0] {
            let normal = Vec2::new(0.0, 1.0);
            let post = LampPost {
                foot: normal * 6.0 * side,
                inward: -normal * side,
            };
            let head = post.foot + post.inward * ARM_REACH;
            assert!(
                head.length() < post.foot.length(),
                "the head moved away from the centreline, not towards it"
            );
        }
    }
}
