//! Vehicles: arcade physics, spawning, and the cars themselves.

pub mod body;
pub mod controller;
pub mod delivery;
pub mod impact;
pub mod lights;
pub mod paint;
pub mod plate;
pub mod spawn;
pub mod spec;
pub mod trim;

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::core::schedule::GameSet;

pub struct VehiclePlugin;

impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((lights::VehicleLightsPlugin, delivery::DeliveryPlugin))
            .add_message::<impact::VehicleImpact>()
            .add_systems(Startup, setup_assets)
            .add_systems(PostStartup, spawn::spawn_parked_vehicles)
            // Forces must be applied before Avian steps in `FixedPostUpdate`,
            // and re-applied every tick because Avian clears them after.
            //
            // "Because Avian clears them after" is also why this needs a run
            // condition and not just a `GameSet` gate. `FixedUpdate` keeps
            // running while the pause menu is open; the physics schedule does
            // not, because `ui::menu::pause_physics` pauses `Time<Physics>` —
            // so the clearing stops and the applying does not, and a whole
            // menu's worth of suspension force lands on the car in the tick
            // after `Escape`. Measured in `the_pause_menu_does_not_charge_the_
            // suspension`: a second of menu threw a settled sedan five metres
            // up, five seconds threw it seventy. Nobody had ever seen it
            // because the menu itself was being drawn into the minimap and no
            // one could pause at all — see `player::camera::spawn_camera`.
            .add_systems(
                FixedUpdate,
                controller::drive_vehicles.run_if(|time: Res<Time<Physics>>| !time.is_paused()),
            )
            .add_systems(
                Update,
                // Chained so a fling never reads a stale impact: the same
                // frame that spots a crash reacts to it.
                (
                    impact::spot_impacts,
                    impact::fling_apart,
                    impact::recover_driven_vehicle,
                )
                    .chain()
                    .in_set(GameSet::Simulation),
            )
            .add_systems(
                Update,
                (spawn::activate_nearby_vehicles, spawn::update_wheel_visuals)
                    .in_set(GameSet::Simulation),
            );
    }
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(spawn::build_assets(
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(lights::build_assets(&mut meshes, &mut materials));
}
