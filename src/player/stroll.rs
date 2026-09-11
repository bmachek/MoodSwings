//! Optional street moments. No timer to beat, currency, penalty or failure:
//! three invitations to discover the city's existing toys at one's own pace.
//! Progress belongs to this outing, not to the deterministic world or save.

use std::collections::HashSet;

use avian3d::prelude::LinearVelocity;
use bevy::prelude::*;
use leafwing_input_manager::prelude::ActionState;

use super::input::Action;
use super::interact::Driving;
use super::on_foot::Player;
use crate::ai::busker::Busker;
use crate::ai::resident::CitizenId;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::config::GameConfig;
use crate::core::schedule::GameSet;
use crate::mood::provoke::Cheered;

#[derive(Resource, Default)]
pub struct StreetMoments {
    pub walked: f32,
    pub listened: f32,
    pub cheered: HashSet<CitizenId>,
    pub completed: [bool; 3],
    pub notice: String,
    pub notice_left: f32,
    previous: Option<Vec3>,
}

pub struct StrollPlugin;

impl Plugin for StrollPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StreetMoments>()
            .add_systems(Update, notice_the_street.in_set(GameSet::Simulation));
    }
}

/// A quick load, debug teleport or car ride must not finish a walking goal.
/// The generous speed ceiling accommodates the rubber controller; actual
/// walking still requires ground contact and movement intent in the caller.
fn walking_step(before: Option<Vec3>, now: Vec3, dt: f32) -> f32 {
    let Some(before) = before else { return 0.0 };
    let distance = before.xz().distance(now.xz());
    if dt <= 0.0 || distance > 12.0 * dt + 0.05 {
        0.0
    } else {
        distance
    }
}

fn notice_the_street(
    time: Res<Time>,
    config: Res<GameConfig>,
    mut moments: ResMut<StreetMoments>,
    mut cheers: MessageReader<Cheered>,
    citizens: Query<&CitizenId>,
    buskers: Query<&Busker>,
    players: Query<
        (
            Entity,
            &Transform,
            &LinearVelocity,
            &Bouncer,
            &ActionState<Action>,
            Option<&Driving>,
            Has<Launched>,
        ),
        With<Player>,
    >,
) {
    let Ok((player, transform, velocity, bouncer, actions, driving, launched)) = players.single()
    else {
        cheers.clear();
        return;
    };
    let at = transform.translation;
    let dt = time.delta_secs();
    let tune = &config.stroll;
    moments.notice_left = (moments.notice_left - dt).max(0.0);
    if !tune.enabled {
        moments.previous = Some(at);
        cheers.clear();
        return;
    }
    // `bounce_bodies` skips a launched body, so `grounded` freezes at whatever
    // it last was — and for the player that is always `true`, the resting hop
    // never clearing the ground probe. Without this, being punted across a
    // junction by a car counts as a walk, which is the one thing the distance
    // filter below exists to prevent.
    let on_foot = driving.is_none() && !launched;
    if on_foot && bouncer.grounded && actions.axis_pair(&Action::Move).length_squared() > 0.01 {
        let step = walking_step(moments.previous, at, dt);
        moments.walked = (moments.walked + step).min(tune.walk_metres.max(1.0));
    }
    moments.previous = Some(at);
    if on_foot
        && velocity.0.length() < 2.0
        && buskers
            .iter()
            .any(|b| b.pitch.distance(at.xz()) < tune.listening_radius.max(0.0))
    {
        moments.listened = (moments.listened + dt).min(tune.listeners_seconds.max(1.0));
    }
    for cheer in cheers.read() {
        if cheer.by == player
            && moments.cheered.len() < tune.cheer_people.clamp(1, 100)
            && let Ok(citizen) = citizens.get(cheer.who)
        {
            // Stable identity, so the same neighbour respawning round the
            // next corner cannot be counted as somebody new.
            moments.cheered.insert(*citizen);
        }
    }
    let reached = [
        moments.walked >= tune.walk_metres.max(1.0),
        moments.cheered.len() >= tune.cheer_people.clamp(1, 100),
        moments.listened >= tune.listeners_seconds.max(1.0),
    ];
    let messages = [
        "Stadtmoment: Frische Luft! Die Stadt kennt jetzt deine Schuhe.",
        "Stadtmoment: Gute Laune ist ansteckend. Du warst der Anfang.",
        "Stadtmoment: Erste Reihe. Die Straße ist heute deine Bühne.",
    ];
    for (i, done) in reached.into_iter().enumerate() {
        if done && !moments.completed[i] {
            moments.completed[i] = true;
            moments.notice = messages[i].to_string();
            moments.notice_left = tune.notice_seconds.max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_load_or_teleport_is_not_a_walk() {
        assert_eq!(walking_step(None, Vec3::X, 0.016), 0.0);
        assert_eq!(walking_step(Some(Vec3::ZERO), Vec3::X * 250.0, 0.016), 0.0);
        assert_eq!(walking_step(Some(Vec3::ZERO), Vec3::X, 0.0), 0.0);
    }

    #[test]
    fn walking_measures_the_street_not_the_height_of_a_hop() {
        assert!(
            (walking_step(Some(Vec3::ZERO), Vec3::new(0.1, 2.0, 0.0), 0.016) - 0.1).abs() < 1e-5
        );
    }
}
