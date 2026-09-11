//! Agents that inhabit the city: traffic, cyclists, the crowd on the
//! pavement, and the animals among their feet.

pub mod animal;
pub mod appearance;
pub mod archetype;
pub mod brolly;
pub mod busker;
pub mod captain;
pub mod crossing;
pub mod cyclist;
pub mod errands;
pub mod figure;
pub mod focus;
pub mod giveway;
pub mod observe;
pub mod pedestrian;
pub mod pigeon;
pub mod queue;
pub mod resident;
pub mod social;
pub mod steering;
pub mod traffic;

use bevy::prelude::*;

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<traffic::Impatient>().add_plugins((
            focus::FocusPlugin,
            observe::AgentObserverPlugin,
            traffic::TrafficPlugin,
            giveway::GiveWayPlugin,
            pedestrian::PedestrianPlugin,
            social::SocialPlugin,
            errands::ErrandPlugin,
            queue::QueuePlugin,
            crossing::CrossingPlugin,
            captain::CaptainPlugin,
            cyclist::CyclistPlugin,
            animal::AnimalPlugin,
            pigeon::PigeonPlugin,
            brolly::BrollyPlugin,
            busker::BuskerPlugin,
        ));
    }
}
