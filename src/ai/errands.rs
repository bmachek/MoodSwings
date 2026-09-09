//! Citizens with somewhere to be.
//!
//! The city already builds shopfronts — every building above house height puts
//! one out, marked with a [`Shopfront`] at the point on the pavement where its
//! light falls — and until now not one of them was a destination. People walked
//! past every window in the town for ever, and about one building in twenty
//! opens onto a room with a clerk standing in it that nobody has ever visited.
//! A city where nobody goes anywhere reads as a film set, however many people
//! are walking down the street.
//!
//! Three things happen here, and they are deliberately the cheap versions.
//!
//! * **Somebody stops at a window.** They walk over, stand, look at it, and go
//!   on. This is what `Archetype::loiter` was already doing blind — a stop with
//!   nothing to stop *at* — given a place to happen.
//! * **Somebody goes in.** They walk to the door and are gone. Not parked
//!   inside a room: rooms are streamed, and a body left standing in one that
//!   despawns is a body standing in a field. Going in is a despawn, and it
//!   costs the population budget exactly what walking off the despawn ring
//!   costs it.
//! * **Somebody comes out.** A share of every new arrival is emitted from a
//!   doorway instead of appearing halfway down a pavement, which is the other
//!   half of the same illusion and fixes the classic tell of a fake city.
//!
//! Everything overrides the walking intent the same way `ai::social` does: the
//! systems run after `Walking` and write `Bouncer::desired` on top of whatever
//! the route said, so a citizen whose errand is cancelled simply carries on
//! down the pavement from the next frame with nothing to undo.

use bevy::prelude::*;
use rand::RngExt;

use super::figure::Attention;
use super::pedestrian::{Pedestrian, Walking};
use crate::audio::AudioRng;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::schedule::GameSet;
use crate::mood::grudge::Grudge;
use crate::world::interior::Shopfront;

/// How far a citizen will go out of their way for a window, in metres.
///
/// A pavement's width plus a shop or two along. Further than this and the
/// detour reads as somebody being summoned rather than as somebody noticing
/// what they were walking past anyway.
const ERRAND_RANGE: f32 = 13.0;

/// Chance per second that an unoccupied citizen takes an interest in one.
const ERRAND_CHANCE: f32 = 0.055;

/// And the chance that the interest is the door rather than the window.
const GOES_IN: f32 = 0.42;

/// How long somebody stands at a window.
const BROWSE: (f32, f32) = (5.0, 16.0);

/// How near the front the errand counts as arrived, and how long it may take.
const ARRIVED: f32 = 1.4;
const PATIENCE: f32 = 22.0;

/// Where a citizen is going, and what they mean to do there.
#[derive(Component)]
pub struct Errand {
    /// The shopfront. Held as a position rather than as an entity because the
    /// front is streamed with its chunk: an errand outlives the thing it is
    /// aimed at, and a position keeps working while a handle would dangle.
    pub at: Vec3,
    /// True for the door, false for the window.
    pub inside: bool,
    /// Given up on after this. A front on the far side of a building the
    /// citizen cannot walk round is otherwise an errand for ever.
    pub until: f32,
}

/// Standing at a window, looking in.
#[derive(Component)]
pub struct Browsing {
    pub at: Vec3,
    pub until: f32,
}

/// The errand overrides, as a set, so the crossing can order itself after them.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Errands;

pub struct ErrandPlugin;

impl Plugin for ErrandPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (set_out, run_errands, hold_browse)
                .chain()
                .in_set(Errands)
                .in_set(GameSet::Ai)
                // Three sets now write `Bouncer::desired` over the walking
                // intent, and "whatever runs later wins" is only a rule if the
                // order is stated: Walking, then Socialising, then this.
                // Unordered, a citizen who was both chatting and shopping got
                // whichever answer the scheduler happened to run second, which
                // is a different answer on a different machine.
                .after(Walking)
                .after(super::social::Socialising),
        );
    }
}

/// Somebody notices a window.
fn set_out(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<AudioRng>,
    fronts: Query<&Transform, With<Shopfront>>,
    candidates: Query<
        (Entity, &Transform, &Pedestrian),
        (
            Without<Errand>,
            Without<Browsing>,
            Without<Grudge>,
            Without<Launched>,
            Without<super::busker::Listening>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    if fronts.is_empty() {
        return;
    }
    for (entity, transform, pedestrian) in &candidates {
        if pedestrian.panic > 0.0 || rng.random::<f32>() > ERRAND_CHANCE * dt {
            continue;
        }
        let here = transform.translation;
        // The nearest front worth crossing a pavement for. A linear scan, and
        // it can be: the roll above has already thrown away all but a handful
        // of citizens this frame.
        let Some(front) = fronts
            .iter()
            .map(|front| front.translation)
            .filter(|at| at.distance(here) < ERRAND_RANGE)
            .min_by(|a, b| a.distance(here).total_cmp(&b.distance(here)))
        else {
            continue;
        };
        commands.entity(entity).insert(Errand {
            at: front,
            inside: rng.random::<f32>() < GOES_IN,
            until: now + PATIENCE,
        });
    }
}

/// Walking to it, and arriving.
fn run_errands(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<AudioRng>,
    mut walkers: Query<(Entity, &Transform, &mut Bouncer, &Pedestrian, &Errand)>,
) {
    let now = time.elapsed_secs();
    for (entity, transform, mut bouncer, pedestrian, errand) in &mut walkers {
        // A panic, a grudge or a launch ends the errand — sociability is the
        // first thing anybody drops, and so is shopping.
        if pedestrian.panic > 0.0 || now > errand.until {
            commands.entity(entity).remove::<Errand>();
            continue;
        }
        let here = transform.translation;
        // Aimed at the pavement in front of the window rather than at the
        // window itself: the front marker stands at head height against the
        // glass, and walking *into* it is walking into a wall.
        let to = (errand.at - here).with_y(0.0);
        if to.length() > ARRIVED {
            bouncer.desired = to.normalize_or_zero().xz() * WALK_OVER;
            commands
                .entity(entity)
                .insert(Attention::to(errand.at, now, 0.5));
            continue;
        }

        commands.entity(entity).remove::<Errand>();
        if errand.inside {
            // In. Not parked inside the room: the rooms are streamed, and a
            // citizen left standing in one that despawns is a citizen standing
            // in a field. The population budget refills the street, and a
            // share of what it refills comes back out of a door.
            //
            // `try_despawn`, because `pedestrian::maintain_population` also
            // despawns citizens — for walking off the despawn ring — and the
            // two run in the same frame. A citizen who reaches a doorway on
            // the same tick they leave the ring is despawned twice, and a
            // second despawn of a live entity id is an error the moment
            // something else has reused it.
            commands.entity(entity).try_despawn();
        } else {
            commands.entity(entity).insert(Browsing {
                at: errand.at,
                until: now + rng.random_range(BROWSE.0..BROWSE.1),
            });
        }
    }
}

/// The pace of a detour: a shade under a stroll, because it is a few steps.
const WALK_OVER: f32 = 1.15;

/// Standing at the glass.
fn hold_browse(
    mut commands: Commands,
    time: Res<Time>,
    mut browsers: Query<(Entity, &mut Transform, &mut Bouncer, &Pedestrian, &Browsing)>,
) {
    let now = time.elapsed_secs();
    for (entity, mut transform, mut bouncer, pedestrian, browsing) in &mut browsers {
        if now > browsing.until || pedestrian.panic > 0.0 {
            commands.entity(entity).remove::<Browsing>();
            continue;
        }
        bouncer.desired = Vec2::ZERO;
        if let Ok(facing) = Dir2::new((browsing.at - transform.translation).xz()) {
            transform.rotation =
                Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
        }
        commands
            .entity(entity)
            .insert(Attention::to(browsing.at, now, 0.6));
    }
}
