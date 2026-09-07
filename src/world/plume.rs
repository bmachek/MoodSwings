//! Smoke off a chimney, steam out of a gully.
//!
//! The one thing this city has never had is something rising. Everything else
//! moves along the ground — traffic, the crowd, the pigeons, a crisp packet —
//! and a skyline with nothing going up from it reads as a still life however
//! much is happening under it. Two plumes fix that from opposite ends: a thread
//! of smoke off a roof, which is the only thing in the game above the parapets
//! that moves at all, and steam curling out of a gully, which is the only thing
//! at ankle height that does.
//!
//! ## A pool, not a fountain
//!
//! The obvious build is a particle emitter: spawn a puff, let it rise, despawn
//! it. At a puff a second across seventy plumes that is seventy spawns and
//! seventy despawns a second, for ever, and the archetype churn costs more than
//! the picture.
//!
//! So each plume owns a fixed ring of puffs as children and *cycles* them. A
//! puff is never created or destroyed after the chunk spawns; it walks its
//! phase from nought to one — rising, spreading, thinning — and wraps back to
//! the bottom. That is the same trick `streetlights` plays with its lamp posts,
//! and it makes a plume cost exactly its puffs and nothing per second.
//!
//! ## Fading in five steps
//!
//! A puff thins as it climbs, which wants a per-puff alpha, which wants a
//! per-puff material — and a material per puff is the mistake the parked cars
//! were making, at a smaller scale but with the same arithmetic. Instead there
//! is a *ladder*: five shared materials at five alphas, and a puff swaps rungs
//! as it rises. Five steps over two seconds is a change every four hundred
//! milliseconds on something with no edges, and the swap only happens on the
//! frame the rung changes rather than every frame.

use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::buildings::ChunkOf;
use crate::core::schedule::GameSet;

/// How far a plume is drawn. Long for the smoke, because a thread over a
/// roofline is a silhouette against the sky and silhouettes survive distance
/// better than anything else in the world.
pub const RANGE: f32 = 200.0;

/// Puffs in one plume.
///
/// This is the number that decides whether a plume is a column or a string of
/// beads, and it is set against the *rise*: at seven the spacing near the mouth
/// is most of a metre and each puff starts a quarter of that across, so the
/// first three read as three separate balls climbing in single file. Eleven
/// closes the gap to half a metre, which the spread has caught up with by the
/// second one.
///
/// Paid for by lighting fewer fires rather than by spending more — see
/// [`CHIMNEYS`]. The whole city's plumes come to about the same number of puffs
/// either way, and a few good columns beat twice as many bad ones.
const PUFFS: usize = 11;

/// Rungs on the alpha ladder.
const RUNGS: usize = 5;

/// What is coming out.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plume {
    /// Off a chimney: dark, slow, and it hangs about.
    Smoke,
    /// Out of a gully: pale, quick, and gone in a couple of metres.
    Steam,
}

impl Plume {
    /// Metres a puff climbs over its life, and how many seconds that is.
    fn rise(self) -> (f32, f32) {
        match self {
            // Smoke leaves a flue fast and then loses interest.
            Plume::Smoke => (5.5, 3.4),
            // Steam off a warm sewer barely gets to head height.
            Plume::Steam => (2.1, 2.0),
        }
    }

    /// What one puff measures when it appears, and when it is spent.
    fn spread(self) -> (f32, f32) {
        match self {
            Plume::Smoke => (0.22, 1.35),
            Plume::Steam => (0.26, 1.05),
        }
    }

    /// How hard the wind pushes it, as a fraction of the wind's own speed.
    ///
    /// Not one. A puff is not a sail — it is already moving with the air it is
    /// in — and a plume that tracks the wind exactly leans over like a windsock
    /// and stops reading as something rising.
    fn windage(self) -> f32 {
        match self {
            Plume::Smoke => 0.34,
            Plume::Steam => 0.20,
        }
    }
}

/// One puff, and where it is in its own life.
#[derive(Component)]
pub struct Puff {
    /// Nought at the mouth, one when it is spent.
    phase: f32,
    /// Which way this one wanders as it goes up, so a column is a column and
    /// not a stack of identical balls.
    wander: Vec2,
    /// The rung it is currently on, so the material is only swapped on the
    /// frame it actually changes.
    rung: usize,
}

#[derive(Resource)]
pub struct PlumeKit {
    puff: Handle<Mesh>,
    stack: Handle<Mesh>,
    brick: Handle<StandardMaterial>,
    /// Five alphas of each, thickest first.
    smoke: Vec<Handle<StandardMaterial>>,
    steam: Vec<Handle<StandardMaterial>>,
}

impl PlumeKit {
    fn ladder(&self, plume: Plume) -> &[Handle<StandardMaterial>] {
        match plume {
            Plume::Smoke => &self.smoke,
            Plume::Steam => &self.steam,
        }
    }
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> PlumeKit {
    let vapour = |materials: &mut Assets<StandardMaterial>, color: Color, alpha: f32| {
        materials.add(StandardMaterial {
            base_color: color.with_alpha(alpha),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            // A puff is lit but it is not a surface: nothing should reflect off
            // it and nothing should read a normal from it.
            metallic: 0.0,
            reflectance: 0.0,
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    };
    let ladder = |materials: &mut Assets<StandardMaterial>, color: Color, top: f32| {
        (0..RUNGS)
            .map(|rung| {
                // Thickest at the mouth and gone at the top, on a curve rather
                // than a line: smoke loses most of its body in the first metre
                // and then drifts about being faint for a while.
                let t = rung as f32 / (RUNGS - 1) as f32;
                vapour(materials, color, top * (1.0 - t).powf(1.6))
            })
            .collect::<Vec<_>>()
    };

    PlumeKit {
        // Low subdivision on purpose: a puff is a blur and a smooth ball is a
        // balloon.
        puff: meshes.add(
            Sphere::new(1.0)
                .mesh()
                .ico(1)
                .expect("an icosphere at one subdivision"),
        ),
        stack: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        brick: materials.add(StandardMaterial {
            base_color: Color::srgb(0.36, 0.27, 0.23),
            perceptual_roughness: 0.96,
            ..default()
        }),
        smoke: ladder(materials, Color::srgb(0.34, 0.33, 0.32), 0.55),
        steam: ladder(materials, Color::srgb(0.88, 0.89, 0.90), 0.42),
    }
}

/// The range every plume is drawn at, with the preset's scale applied and a
/// ceiling on it. Shared, so the chimneys and the gullies cannot drift apart.
pub fn draw_range(lod_scale: f32) -> VisibilityRange {
    let end = (RANGE * lod_scale).min(900.0);
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: end..(end * 1.1),
        use_aabb: false,
    }
}

/// Chance any one building has a fire lit.
///
/// One in seventeen, which over four thousand buildings is a couple of hundred
/// threads of smoke across a two-kilometre city — a skyline with something
/// going on rather than a skyline on fire.
const CHIMNEYS: f32 = 0.06;

/// This building's own plume stream, keyed on its footprint the way its roof
/// and its frontage are — so a chunk walked back into lights the same fires.
pub fn stream_for_building(world_seed: u64, footprint: super::citygen::Rect) -> ChaCha8Rng {
    let centre = footprint.center();
    let x = (centre.x * 100.0).round() as i64 as i32;
    let z = (centre.y * 100.0).round() as i64 as i32;
    crate::core::rng::stream_for_chunk(world_seed, crate::core::rng::stream::PLUMES, (x, z))
}

/// Lights a fire on this building, if this building has one lit.
///
/// Not on a tower: a curtain wall has no flue, and a thread of woodsmoke off
/// the fortieth floor is the single most obviously wrong thing this module
/// could produce.
#[allow(clippy::too_many_arguments)]
pub fn maybe_chimney(
    commands: &mut Commands,
    kit: &PlumeKit,
    world_seed: u64,
    building: &super::citygen::Building,
    class: super::texture::FacadeClass,
    footprint_centre: Vec2,
    size: Vec2,
    yaw: f32,
    chunk: IVec2,
    range: &VisibilityRange,
) {
    if matches!(class, super::texture::FacadeClass::Tower) {
        return;
    }
    let mut rng = stream_for_building(world_seed, building.footprint);
    if rng.random_range(0.0..1.0) > CHIMNEYS {
        return;
    }
    // Back from the front edge and over to one side, which is where a stack
    // comes up: never in the middle of a roof and never on the parapet.
    let across = rng.random_range(-0.34..0.34) * size.x;
    let back = rng.random_range(0.08..0.34) * size.y;
    let at = footprint_centre + Vec2::new(across, back);
    chimney(
        commands,
        kit,
        at,
        super::buildings::SIDEWALK_HEIGHT + building.height,
        yaw,
        chunk,
        range,
        &mut rng,
    );
}

/// Puts a plume at a point, with its ring of puffs already in it.
fn raise(
    commands: &mut Commands,
    kit: &PlumeKit,
    plume: Plume,
    at: Vec3,
    chunk: IVec2,
    range: &VisibilityRange,
    rng: &mut ChaCha8Rng,
) {
    let ladder = kit.ladder(plume);
    commands
        .spawn((
            ChunkOf(chunk),
            plume,
            Transform::from_translation(at),
            Visibility::default(),
        ))
        .with_children(|parent| {
            for i in 0..PUFFS {
                // Spread evenly round the cycle, so the column is continuous
                // from the first frame rather than arriving as one clump that
                // has to space itself out over a life.
                let phase = i as f32 / PUFFS as f32;
                parent.spawn((
                    Puff {
                        phase,
                        wander: Vec2::new(rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0)),
                        rung: usize::MAX,
                    },
                    Mesh3d(kit.puff.clone()),
                    MeshMaterial3d(ladder[0].clone()),
                    Transform::from_scale(Vec3::splat(plume.spread().0)),
                    // Smoke that casts a shadow is a rock.
                    NotShadowCaster,
                    range.clone(),
                ));
            }
        });
}

/// A chimney on a roof, and what comes out of it.
#[allow(clippy::too_many_arguments)]
pub fn chimney(
    commands: &mut Commands,
    kit: &PlumeKit,
    at: Vec2,
    roof: f32,
    yaw: f32,
    chunk: IVec2,
    range: &VisibilityRange,
    rng: &mut ChaCha8Rng,
) {
    const HEIGHT: f32 = 1.35;
    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.stack.clone()),
        MeshMaterial3d(kit.brick.clone()),
        Transform::from_xyz(at.x, roof + HEIGHT * 0.5, at.y)
            .with_rotation(Quat::from_rotation_y(yaw))
            .with_scale(Vec3::new(0.72, HEIGHT, 0.55)),
        range.clone(),
    ));
    raise(
        commands,
        kit,
        Plume::Smoke,
        Vec3::new(at.x, roof + HEIGHT, at.y),
        chunk,
        range,
        rng,
    );
}

/// Steam out of a gully, with nothing to see it come from.
pub fn gully(
    commands: &mut Commands,
    kit: &PlumeKit,
    at: Vec2,
    chunk: IVec2,
    range: &VisibilityRange,
    rng: &mut ChaCha8Rng,
) {
    // Just above the road, so the first puff is already clear of it: a puff
    // half-buried in the asphalt reads as a hole rather than as a vent.
    raise(
        commands,
        kit,
        Plume::Steam,
        Vec3::new(at.x, 0.22, at.y),
        chunk,
        range,
        rng,
    );
}

pub struct PlumePlugin;

impl Plugin for PlumePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, drift.in_set(GameSet::Simulation));
    }
}

/// Everything currently going up.
fn drift(
    time: Res<Time>,
    weather: Res<super::weather::Weather>,
    kit: Option<Res<PlumeKit>>,
    plumes: Query<&Plume>,
    mut puffs: Query<(
        &mut Puff,
        &ChildOf,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Some(kit) = kit else { return };
    let dt = time.delta_secs();
    let wind = weather.wind;

    for (mut puff, parent, mut transform, mut material) in &mut puffs {
        let Ok(&plume) = plumes.get(parent.parent()) else {
            continue;
        };
        let (height, life) = plume.rise();
        let (small, large) = plume.spread();

        puff.phase = (puff.phase + dt / life).fract();
        let t = puff.phase;

        // Up, out into the wind, and wandering its own way as it goes. The
        // wander grows with the climb rather than being applied flat, so the
        // column leaves the mouth tight and opens out — which is what a plume
        // does and what a straight line of balls does not.
        let carried = wind * plume.windage() * t * life;
        let stray = puff.wander * t * t * 0.55;
        transform.translation = Vec3::new(
            carried.x + stray.x,
            height * t.powf(0.78),
            carried.y + stray.y,
        );
        transform.scale = Vec3::splat(small.lerp(large, t));

        // The rung, swapped only on the frame it changes.
        let ladder = kit.ladder(plume);
        let rung = ((t * RUNGS as f32) as usize).min(RUNGS - 1);
        if rung != puff.rung {
            puff.rung = rung;
            material.0 = ladder[rung].clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ladder starts solid and ends invisible.
    #[test]
    fn a_puff_thins_all_the_way_out() {
        let alpha = |rung: usize, top: f32| {
            let t = rung as f32 / (RUNGS - 1) as f32;
            top * (1.0 - t).powf(1.6)
        };
        for top in [0.55f32, 0.42] {
            assert!((alpha(0, top) - top).abs() < 1e-6, "the mouth is thin");
            assert!(alpha(RUNGS - 1, top) < 1e-6, "the top never disappears");
            // And it is monotone, or a plume pulses.
            for rung in 1..RUNGS {
                assert!(alpha(rung, top) < alpha(rung - 1, top));
            }
        }
        // Most of the body is lost early: at the halfway rung a puff should
        // already be well under half as thick as it started.
        assert!(alpha(2, 1.0) < 0.35);
    }

    /// The column rises fast and then loosens.
    #[test]
    fn a_plume_slows_as_it_climbs() {
        // The climb is `t^0.78`, so the first half of a puff's life covers more
        // than half its height — which is what makes the top of a plume the
        // part that spreads.
        let climb = |t: f32| t.powf(0.78);
        assert!(climb(0.5) > 0.5);
        assert!(climb(1.0) - climb(0.9) < climb(0.1) - climb(0.0));
    }

    /// Smoke goes higher and lasts longer than steam.
    #[test]
    fn smoke_outlives_steam() {
        let (smoke_up, smoke_life) = Plume::Smoke.rise();
        let (steam_up, steam_life) = Plume::Steam.rise();
        assert!(smoke_up > steam_up * 2.0);
        assert!(smoke_life > steam_life);
        // Steam from a gully must not reach a first-floor window; the joke is
        // that it curls round your ankles.
        assert!(steam_up < 3.0);
        // And both spread as they go rather than staying the same size.
        for plume in [Plume::Smoke, Plume::Steam] {
            let (small, large) = plume.spread();
            assert!(large > small * 2.0, "{plume:?} never opens out");
        }
    }

    /// A plume leans in the wind without lying down in it.
    #[test]
    fn the_wind_bends_a_plume_rather_than_flattening_it() {
        for plume in [Plume::Smoke, Plume::Steam] {
            let windage = plume.windage();
            assert!(windage > 0.1, "{plume:?} ignores the weather");
            assert!(windage < 0.5, "{plume:?} is a windsock");
            // At a stiff breeze the top of a plume must still be higher than it
            // is downwind, or the thing reads as a leak rather than a rise.
            let (height, life) = plume.rise();
            let downwind = 9.0 * windage * life;
            assert!(
                height > downwind * 0.5,
                "{plume:?} blows {downwind:.1}m sideways for {height:.1}m up"
            );
        }
    }
}
