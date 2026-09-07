//! Agents that inhabit the city: traffic, cyclists, the crowd on the
//! pavement, and the animals among their feet.

pub mod animal;
pub mod archetype;
pub mod cyclist;
pub mod figure;
pub mod pedestrian;
pub mod steering;
pub mod traffic;

use bevy::prelude::*;

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            traffic::TrafficPlugin,
            pedestrian::PedestrianPlugin,
            cyclist::CyclistPlugin,
            animal::AnimalPlugin,
        ));
    }
}
