//! Agents that inhabit the city: traffic, cyclists, the crowd on the
//! pavement, and the animals among their feet.

pub mod animal;
pub mod archetype;
pub mod brolly;
pub mod busker;
pub mod captain;
pub mod cyclist;
pub mod errands;
pub mod figure;
pub mod focus;
pub mod pedestrian;
pub mod pigeon;
pub mod social;
pub mod steering;
pub mod traffic;

use bevy::prelude::*;

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            focus::FocusPlugin,
            traffic::TrafficPlugin,
            pedestrian::PedestrianPlugin,
            social::SocialPlugin,
            errands::ErrandPlugin,
            captain::CaptainPlugin,
            cyclist::CyclistPlugin,
            animal::AnimalPlugin,
            pigeon::PigeonPlugin,
            brolly::BrollyPlugin,
            busker::BuskerPlugin,
        ));
    }
}
