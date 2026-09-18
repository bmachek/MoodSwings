//! One physics world for every headless test that needs one.
//!
//! There were four of these, one per module, and they had drifted: each built
//! its own `App`, its own flat ground and its own idea of which plugins the
//! game installs. None of them installed the one that matters most, and the
//! gap was not cosmetic — `world::mod` runs Avian with
//! `PhysicsInterpolationPlugin::interpolate_all()`, which eases every rigid
//! body's rendered pose between ticks and treats an `Update`-time write to
//! `Transform` as a teleport. A dozen systems make exactly that write. With a
//! bare `PhysicsPlugins::default()` none of it shows, so the capture harness —
//! which needs a GPU — was the only instrument that could see it at all. Here
//! it is a `cargo test`.
//!
//! Only compiled for tests. It is scaffolding, not a feature.

use avian3d::prelude::*;
use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

/// The physics tick the game runs at, and what every test here steps in.
pub const TICK: f64 = 1.0 / 64.0;

/// An app with the game's physics in it and nothing else.
///
/// `frame` is how long each `app.update()` claims to have taken. Passing
/// [`TICK`] gives one physics tick per frame, which is what most of these
/// tests want; passing something shorter is how a test asks what happens at a
/// frame rate above the tick rate, where a frame may carry no tick at all and
/// interpolation is doing the most work.
pub fn physics_app(frame: f64) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        TransformPlugin,
        // Exactly as `world::mod` installs it. The easing is the point.
        PhysicsPlugins::default().set(PhysicsInterpolationPlugin::interpolate_all()),
    ));
    // Avian's collider cache reads `AssetEvent<Mesh>`; `AssetPlugin` alone does
    // not register the Mesh asset type outside a render app.
    app.init_asset::<Mesh>();
    // Real elapsed time in a test is effectively zero, so drive the clock.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        frame,
    )));
    app.init_resource::<crate::core::config::GameConfig>();
    // And the solver settings, which `world::mod` installs from the same
    // config block. A harness without them is a city that is not made of
    // rubber: every landing is absorbed, so "the body came down hard" reads as
    // "the body is standing still", and a test cannot tell a roof drop from a
    // kerb. The `Max` combine rule is the load-bearing half — a flummi bounces
    // off concrete because *it* is elastic.
    let bounce = app
        .world()
        .resource::<crate::core::config::GameConfig>()
        .bounce
        .clone();
    app.insert_resource(DefaultRestitution(
        Restitution::new(bounce.restitution).with_combine_rule(CoefficientCombine::Max),
    ));
    app.insert_resource(avian3d::dynamics::solver::SolverConfig {
        restitution_threshold: bounce.threshold,
        restitution_iterations: 4,
        ..default()
    });
    app.add_message::<super::boing::Wallop>();
    app.add_message::<super::boing::Landed>();
    app
}

/// Flat ground with its top face at y = 0.
pub fn ground(app: &mut App) {
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(400.0, 2.0, 400.0),
        Transform::from_xyz(0.0, -1.0, 0.0),
    ));
}

/// A kerb: a slab whose top face is `height` above the ground, its face at
/// `at` on the x axis and its high side running off to +x.
///
/// The city's own kerb is 0.28 m ([`crate::world::buildings::SIDEWALK_HEIGHT`])
/// and the thing every walking body in the game has to get up and down twenty
/// times a street.
pub fn kerb(app: &mut App, at: f32, height: f32) {
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(120.0, height * 2.0, 400.0),
        Transform::from_xyz(at + 60.0, 0.0, 0.0),
    ));
}

/// Finishes the app the way `run()` would.
///
/// Avian registers its diagnostics resources in `Plugin::finish`, and its
/// systems hard-require them; a bare `update()` loop never triggers it.
pub fn finish(app: &mut App) {
    app.finish();
    app.cleanup();
}
