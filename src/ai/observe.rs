//! Evidence about the existing agents, before replacing their decision logic.
//!
//! Several behaviours can coexist and write movement in the same frame. The
//! inspector deliberately lists their markers rather than claiming to know
//! which writer won. Movement is measured from the final intent after game
//! logic; the next decision layer can replace markers with explicit reasons.
//!
//! These are live entity observations, not persistent citizen identities.
//! Entries disappear with their bodies and history is bounded. A future
//! resident registry must own identity across streaming and saving separately.

use std::collections::{HashMap, VecDeque};

use avian3d::prelude::LinearVelocity;
use bevy::{ecs::query::QueryData, prelude::*};

use super::{archetype::Archetype, pedestrian::Pedestrian, traffic::TrafficDriver};
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::states::{AppState, InGameState};
use crate::core::{config::AgentWatchConfig, config::GameConfig, schedule::GameSet};
use crate::mood::feeling::Mood;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Moving,
    Waiting,
    Blocked,
    Airborne,
}

/// Ground displacement over a window, not velocity on one frame: footsteps
/// and rubber bounces have legitimate instants at zero speed. Vertical motion
/// must not let somebody bouncing against a wall pass as making progress.
#[derive(Debug)]
struct Progress {
    anchor: Vec2,
    seconds: f32,
}

impl Progress {
    fn new(at: Vec2) -> Self {
        Self {
            anchor: at,
            seconds: 0.0,
        }
    }

    fn update(
        &mut self,
        at: Vec2,
        intends_motion: bool,
        airborne: bool,
        dt: f32,
        config: &AgentWatchConfig,
    ) -> Motion {
        if airborne || !intends_motion {
            self.anchor = at;
            self.seconds = 0.0;
            return if airborne {
                Motion::Airborne
            } else {
                Motion::Waiting
            };
        }
        if at.distance(self.anchor) >= config.progress_metres.max(0.01) {
            self.anchor = at;
            self.seconds = 0.0;
        } else {
            self.seconds += dt;
        }
        if self.seconds >= config.blocked_seconds.max(0.1) {
            Motion::Blocked
        } else {
            Motion::Moving
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Activity(u16);

impl Activity {
    pub fn labels(self) -> impl Iterator<Item = &'static str> {
        const LABELS: [&str; 12] = [
            "panic",
            "grudge",
            "celebrating",
            "scuffle",
            "chatting",
            "loitering",
            "watching incident",
            "shop errand",
            "browsing",
            "crossing",
            "playing music",
            "listening",
        ];
        LABELS
            .into_iter()
            .enumerate()
            .filter(move |(bit, _)| self.0 & (1 << bit) != 0)
            .map(|(_, label)| label)
    }
}

#[derive(Debug)]
pub struct Transition {
    pub at: f32,
    pub motion: Motion,
    pub activity: Activity,
    pub traffic: Option<super::traffic::DriverObservation>,
}

#[derive(Debug)]
pub struct AgentSample {
    pub citizen: Option<super::resident::CitizenId>,
    pub position: Vec3,
    pub kind: String,
    pub mood: Option<f32>,
    pub speed: f32,
    pub desired_speed: f32,
    pub motion: Motion,
    pub activity: Activity,
    pub route: Option<(
        crate::world::roadgraph::NodeId,
        crate::world::roadgraph::NodeId,
    )>,
    pub traffic: Option<super::traffic::DriverObservation>,
    pub waiting: f32,
    pub recovery: f32,
    pub history: VecDeque<Transition>,
    progress: Progress,
}

#[derive(Resource, Default)]
pub struct AgentObservations {
    pub agents: HashMap<Entity, AgentSample>,
    /// Counts transitions into blocked movement, including bodies since
    /// despawned. Unlike the live count this cannot hide a recycled problem.
    pub blocked_episodes: u64,
}

#[derive(QueryData)]
struct AgentQuery {
    entity: Entity,
    transform: &'static Transform,
    velocity: &'static LinearVelocity,
    pedestrian: Option<&'static Pedestrian>,
    citizen: Option<&'static super::resident::CitizenId>,
    driver: Option<&'static TrafficDriver>,
    bouncer: Option<&'static Bouncer>,
    archetype: Option<&'static Archetype>,
    mood: Option<&'static Mood>,
    launched: Has<Launched>,
    knocked_down: Has<crate::bounce::launch::KnockedDown>,
    grudge: Has<crate::mood::grudge::Grudge>,
    celebrating: Has<crate::mood::grudge::Pirouette>,
    scuffle: Has<crate::mood::scuffle::Scuffle>,
    chatting: Has<super::social::Chatting>,
    loitering: Has<super::social::Loitering>,
    watching: Has<super::social::Rubbernecking>,
    errand: Has<super::errands::Errand>,
    browsing: Has<super::errands::Browsing>,
    crossing: Has<super::crossing::Crossing>,
    busker: Has<super::busker::Busker>,
    busking: Has<super::social::Busking>,
    listening: Has<super::busker::Listening>,
}

pub struct AgentObserverPlugin;

impl Plugin for AgentObserverPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AgentObservations>().add_systems(
            Update,
            observe
                .in_set(GameSet::Ui)
                .run_if(in_state(AppState::InGame))
                .run_if(in_state(InGameState::Playing)),
        );
    }
}

fn observe(
    time: Res<Time>,
    config: Res<GameConfig>,
    mut observations: ResMut<AgentObservations>,
    agents: Query<
        AgentQuery,
        Or<(
            With<Pedestrian>,
            With<TrafficDriver>,
            With<super::cyclist::Cyclist>,
        )>,
    >,
) {
    observations
        .agents
        .retain(|entity, _| agents.get(*entity).is_ok());
    for agent in &agents {
        let at = agent.transform.translation;
        let flags = [
            agent.pedestrian.is_some_and(|p| p.panic > 0.0),
            agent.grudge,
            agent.celebrating,
            agent.scuffle,
            agent.chatting,
            agent.loitering,
            agent.watching,
            agent.errand,
            agent.browsing,
            agent.crossing,
            agent.busker || agent.busking,
            agent.listening,
        ];
        let activity = Activity(
            flags
                .iter()
                .enumerate()
                .fold(0, |bits, (i, on)| bits | (u16::from(*on) << i)),
        );
        let desired_speed = agent.driver.map_or_else(
            || agent.bouncer.map_or(0.0, |b| b.desired.length()),
            |d| d.desired_speed,
        );
        let sample = observations
            .agents
            .entry(agent.entity)
            .or_insert_with(|| AgentSample {
                citizen: None,
                position: at,
                kind: if agent.driver.is_some() {
                    "Driver".into()
                } else if agent.pedestrian.is_some() {
                    agent
                        .archetype
                        .map_or_else(|| "Pedestrian".into(), |a| format!("{a:?}"))
                } else {
                    "Cyclist".into()
                },
                mood: None,
                speed: 0.0,
                desired_speed: 0.0,
                motion: Motion::Waiting,
                activity: Activity(0),
                route: None,
                traffic: None,
                waiting: 0.0,
                recovery: 0.0,
                history: VecDeque::with_capacity(6),
                progress: Progress::new(at.xz()),
            });
        let motion = sample.progress.update(
            at.xz(),
            desired_speed > config.agent_watch.intent_speed,
            agent.launched || agent.knocked_down,
            time.delta_secs(),
            &config.agent_watch,
        );
        let newly_blocked = motion == Motion::Blocked && sample.motion != Motion::Blocked;
        let traffic = agent.driver.map(|d| d.observation);
        if sample.history.is_empty()
            || motion != sample.motion
            || activity != sample.activity
            || traffic != sample.traffic
        {
            if sample.history.len() == 6 {
                sample.history.pop_front();
            }
            sample.history.push_back(Transition {
                at: time.elapsed_secs(),
                motion,
                activity,
                traffic,
            });
        }
        sample.motion = motion;
        sample.activity = activity;
        sample.position = at;
        sample.citizen = agent.citizen.copied();
        sample.speed = agent.velocity.0.xz().length();
        sample.desired_speed = desired_speed;
        sample.mood = agent.mood.map(|m| m.value);
        sample.route = agent
            .driver
            .map(|d| (d.from, d.to))
            .or_else(|| agent.pedestrian.map(|p| (p.from, p.to)));
        sample.traffic = traffic;
        sample.waiting = agent.driver.map_or(0.0, |d| d.waiting);
        sample.recovery = agent.driver.map_or(0.0, |d| d.stuck);
        if newly_blocked {
            observations.blocked_episodes += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_does_not_become_a_navigation_failure() {
        let mut progress = Progress::new(Vec2::ZERO);
        let config = AgentWatchConfig::default();
        assert_eq!(
            progress.update(Vec2::ZERO, false, false, 60.0, &config),
            Motion::Waiting
        );
        assert_eq!(progress.seconds, 0.0);
    }

    #[test]
    fn intent_without_ground_progress_becomes_blocked() {
        let mut progress = Progress::new(Vec2::ZERO);
        let config = AgentWatchConfig::default();
        assert_eq!(
            progress.update(Vec2::ZERO, true, false, 9.0, &config),
            Motion::Blocked
        );
        assert_eq!(
            progress.update(Vec2::X, true, false, 0.1, &config),
            Motion::Moving
        );
        assert_eq!(progress.seconds, 0.0);
    }

    #[test]
    fn slow_progress_accumulates_across_frames() {
        let mut progress = Progress::new(Vec2::ZERO);
        let config = AgentWatchConfig::default();
        for i in 1..50 {
            assert_eq!(
                progress.update(Vec2::X * i as f32 * 0.1, true, false, 0.5, &config),
                Motion::Moving
            );
        }
    }

    #[test]
    fn a_launch_resets_the_blocked_window() {
        let mut progress = Progress::new(Vec2::ZERO);
        let config = AgentWatchConfig::default();
        progress.update(Vec2::ZERO, true, false, 9.0, &config);
        assert_eq!(
            progress.update(Vec2::ZERO, true, true, 10.0, &config),
            Motion::Airborne
        );
        assert_eq!(
            progress.update(Vec2::ZERO, true, false, 0.1, &config),
            Motion::Moving
        );
    }

    #[test]
    fn observations_follow_components_and_release_despawned_bodies() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<GameConfig>()
            .init_resource::<AgentObservations>()
            .add_systems(Update, observe);
        let person = app
            .world_mut()
            .spawn((
                Pedestrian {
                    from: crate::world::roadgraph::NodeId(0),
                    to: crate::world::roadgraph::NodeId(1),
                    side: 1.0,
                    speed: 1.5,
                    panic: 0.0,
                    current_speed: 0.0,
                },
                Transform::default(),
                LinearVelocity::ZERO,
                Bouncer::new(1.0),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().resource::<AgentObservations>().agents[&person].motion,
            Motion::Waiting
        );
        app.world_mut()
            .entity_mut(person)
            .insert(super::super::social::Chatting {
                with: Entity::PLACEHOLDER,
                left: 5.0,
            });
        app.update();
        let records = app.world().resource::<AgentObservations>();
        assert_eq!(
            records.agents[&person]
                .activity
                .labels()
                .collect::<Vec<_>>(),
            vec!["chatting"]
        );
        assert_eq!(records.agents[&person].history.len(), 2);
        app.world_mut().despawn(person);
        app.update();
        assert!(
            app.world()
                .resource::<AgentObservations>()
                .agents
                .is_empty()
        );
    }
}
