//! Crossing the road.
//!
//! The one behaviour that makes a citizen read as having a mind rather than a
//! route. Everything else the crowd does — walking, stopping for a word,
//! looking in a window, bolting from a car — happens on the pavement, where
//! nothing has to be judged. Stepping off a kerb does: you have to want to be
//! on the other side, you have to wait, you have to look, and then you have to
//! commit. A flummi that does those four things in that order is doing
//! something an observer can *read*, and reading it is what tells them the
//! city is inhabited rather than animated.
//!
//! It is also what the pavements were mitred for. Until the junctions turned
//! their corners properly there was nowhere to cross *to*: the pavement stopped
//! short of every crossing and the citizen's own walking line ran out into the
//! carriageway, so a crossing would have been a walk from one piece of road to
//! another.
//!
//! Deliberately not tied to the painted zebras. `world::markings` paints one
//! per junction approach on streets over twenty-six metres long, which is a
//! minority of Landshut, and a citizen who may only cross where there is paint
//! is a citizen who almost never crosses. What is modelled is the German
//! pedestrian rather than the crossing: they wait at the kerb, they look, and
//! then they go — at the corner if there is one and in the middle of the block
//! if there is not.
//!
//! Like `ai::social` and `ai::errands`, this runs after `Walking` and writes
//! over the walking intent, so a crossing that is cancelled leaves nothing to
//! undo.

use bevy::prelude::*;
use rand::RngExt;

use super::figure::Attention;
use super::pedestrian::{Pedestrian, Walking};
use crate::audio::AudioRng;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::schedule::GameSet;
use crate::mood::grudge::Grudge;
use crate::world::City;

/// Chance per second that somebody on a pavement decides the other side is
/// where they want to be.
///
/// Low. A crossing takes several seconds of standing about and then a walk
/// across a carriageway, so at anything much higher the street turns into a
/// stream of people ping-ponging across it rather than walking down it.
const WANDERLUST: f32 = 0.022;

/// How far from a junction a crossing may start.
///
/// Crossing *at* a junction means stepping into the middle of it, where two
/// carriageways overlap and the ribbons are laid over one another — there is
/// no far kerb to aim at. Held back by rather more than a pavement's width.
const CLEAR_OF_JUNCTION: f32 = 12.0;

/// Streets under this are stepped over rather than crossed, and are skipped:
/// the whole performance is three seconds of standing still to cover four
/// metres, which reads as somebody having second thoughts.
const WORTH_CROSSING: f32 = 6.0;

/// How far up the road a citizen looks, and how fast something has to be
/// moving to be worth waiting for.
const LOOK_UP_THE_ROAD: f32 = 22.0;
const WORTH_WAITING_FOR: f32 = 1.6;

/// How long a citizen takes to react once the road is clear.
///
/// Drawn per crossing, and it is the whole reason a group does not step off
/// the kerb in formation like a chorus line.
const REACTION: (f32, f32) = (0.35, 1.4);

/// And how long they will stand there before giving up and walking on.
const GIVE_UP: f32 = 26.0;

/// How much quicker than a stroll somebody crosses a road.
const HURRY: f32 = 1.35;

/// Crossing, or about to.
#[derive(Component)]
pub struct Crossing {
    /// The kerb being left and the pavement being aimed at.
    pub kerb: Vec2,
    pub far: Vec2,
    /// Counts down while waiting for a gap; the walk starts at zero.
    pub react: f32,
    /// Seconds spent on the near kerb, so a citizen on an impossible road
    /// eventually shrugs and carries on.
    pub waited: f32,
    /// True once they have committed and are on the carriageway.
    pub stepped_off: bool,
}

pub struct CrossingPlugin;

impl Plugin for CrossingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (decide_to_cross, wait_and_cross)
                .chain()
                .in_set(GameSet::Ai)
                // Last of the four sets that write `Bouncer::desired`, and it
                // has to be: somebody halfway across a carriageway is not
                // available for a chat or a shop window, and the only way to
                // say that once is to run after both.
                .after(Walking)
                .after(super::social::Socialising)
                .after(super::errands::Errands),
        );
    }
}

/// Somebody looks across the road and decides that is where they want to be.
fn decide_to_cross(
    mut commands: Commands,
    time: Res<Time>,
    city: Res<City>,
    mut rng: ResMut<AudioRng>,
    candidates: Query<
        (Entity, &Transform, &Pedestrian),
        (
            Without<Crossing>,
            Without<Grudge>,
            Without<Launched>,
            Without<super::errands::Errand>,
            Without<super::errands::Browsing>,
            Without<super::busker::Listening>,
        ),
    >,
) {
    let dt = time.delta_secs();
    for (entity, transform, pedestrian) in &candidates {
        if pedestrian.panic > 0.0 || rng.random::<f32>() > WANDERLUST * dt {
            continue;
        }
        let a = city.graph.node(pedestrian.from).pos;
        let b = city.graph.node(pedestrian.to).pos;
        let Some(width) = city
            .graph
            .neighbors(pedestrian.from)
            .find(|(node, _)| *node == pedestrian.to)
            .map(|(_, edge)| city.graph.edge(edge).width)
        else {
            continue;
        };
        if width < WORTH_CROSSING {
            continue;
        }

        let Ok(direction) = Dir2::new(b - a) else {
            continue;
        };
        let length = a.distance(b);
        let here = transform.translation.xz();
        let along = (here - a).dot(*direction).clamp(0.0, length);
        // Not in a junction: there is no far kerb in the middle of one.
        if along < CLEAR_OF_JUNCTION || along > length - CLEAR_OF_JUNCTION {
            continue;
        }

        let normal = super::steering::right_of(*direction);
        let centre = a + *direction * along;
        // The kerb on this side, and the pavement on the other. `side` is
        // measured against the same normal the walking line is, so crossing is
        // simply the same line with the sign flipped.
        let kerb = centre + normal * (pedestrian.side * (width * 0.5 - 0.3));
        let far = centre - normal * (pedestrian.side * (width * 0.5 + PAVEMENT));

        commands.entity(entity).insert(Crossing {
            kerb,
            far,
            react: rng.random_range(REACTION.0..REACTION.1),
            waited: 0.0,
            stepped_off: false,
        });
    }
}

/// How far past the kerb the far pavement's walking line sits.
///
/// The same number `ai::pedestrian` uses. Not imported from it: that one is
/// private, and a crossing wants to arrive on the walking line rather than on
/// the kerb edge, which is the same question asked from the other side.
const PAVEMENT: f32 = 1.9;

/// Standing at the kerb, looking, and then going.
#[allow(clippy::type_complexity)]
fn wait_and_cross(
    mut commands: Commands,
    time: Res<Time>,
    traffic: Query<
        (&Transform, &avian3d::prelude::LinearVelocity),
        With<crate::vehicle::spawn::Vehicle>,
    >,
    // `Without<Vehicle>` is not decoration. Both queries touch `Transform` and
    // one of them mutably, which Bevy rejects at the first run of the system
    // rather than at compile time — and a pedestrian is never a vehicle, so
    // the filter that makes them disjoint changes nothing about what is
    // matched. The capture harness caught this, which is what it is for.
    mut crossers: Query<
        (
            Entity,
            &mut Transform,
            &mut Bouncer,
            &mut Pedestrian,
            &mut Crossing,
        ),
        Without<crate::vehicle::spawn::Vehicle>,
    >,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    // Everything on the road that is actually going somewhere. A parked car is
    // not a reason to stand on a kerb.
    let moving: Vec<(Vec2, Vec2)> = traffic
        .iter()
        .filter(|(_, velocity)| velocity.0.xz().length() > WORTH_WAITING_FOR)
        .map(|(transform, velocity)| (transform.translation.xz(), velocity.0.xz()))
        .collect();

    for (entity, mut transform, mut bouncer, mut pedestrian, mut crossing) in &mut crossers {
        if pedestrian.panic > 0.0 {
            // A car has already made the decision. Get off the road.
            commands.entity(entity).remove::<Crossing>();
            continue;
        }
        let here = transform.translation.xz();

        if crossing.stepped_off {
            let to_far = crossing.far - here;
            if to_far.length() < 0.8 {
                // Arrived. The walking line is the other pavement now, which is
                // the whole point of the exercise.
                pedestrian.side = -pedestrian.side;
                commands.entity(entity).remove::<Crossing>();
                continue;
            }
            bouncer.desired = to_far.normalize_or_zero() * pedestrian.speed * HURRY;
            continue;
        }

        // Walk to the kerb first. Somebody who sets off across the road from
        // the middle of the pavement is crossing diagonally, which nobody does
        // and which puts them in the carriageway for half as long again.
        let to_kerb = crossing.kerb - here;
        if to_kerb.length() > 0.6 {
            bouncer.desired = to_kerb.normalize_or_zero() * pedestrian.speed * 0.9;
            continue;
        }

        // At the kerb. Stand, and look up the road the traffic is coming from.
        bouncer.desired = Vec2::ZERO;
        crossing.waited += dt;
        let looking = crossing.far - crossing.kerb;
        if let Ok(facing) = Dir2::new(looking) {
            transform.rotation =
                Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
        }

        // Anything bearing down on the crossing line, from either direction.
        // Not "a car nearby": a car parked at the kerb next to them and a car
        // going the other way down the far side are both not their problem, and
        // waiting for either is a citizen standing on a kerb for ever.
        let coming = moving.iter().any(|(at, velocity)| {
            let to_them = *at - crossing.kerb;
            let range = to_them.length();
            range < LOOK_UP_THE_ROAD && velocity.dot(-to_them) > 0.0 && {
                // Will it pass through where they are about to walk? Compare
                // the car's heading against the line to the far kerb: a car
                // travelling along the street crosses it, one turning away
                // does not.
                let heading = velocity.normalize_or_zero();
                heading.dot(looking.normalize_or_zero()).abs() < 0.7
            }
        });

        // Look at the nearest thing coming, which is what a glance up the road
        // *is* and is also the cue that they are about to step off.
        if let Some((at, _)) = moving
            .iter()
            .filter(|(at, _)| at.distance(crossing.kerb) < LOOK_UP_THE_ROAD)
            .min_by(|(a, _), (b, _)| {
                a.distance(crossing.kerb)
                    .total_cmp(&b.distance(crossing.kerb))
            })
        {
            commands
                .entity(entity)
                .insert(Attention::to(Vec3::new(at.x, 1.0, at.y), now, 0.4));
        }

        if coming {
            // Keep a little reaction time in hand: somebody who has just
            // watched a car go past does not step off the instant its bumper
            // clears them.
            crossing.react = crossing.react.max(0.25);
            if crossing.waited > GIVE_UP {
                commands.entity(entity).remove::<Crossing>();
            }
            continue;
        }
        // Clear. React, then commit.
        crossing.react -= dt;
        if crossing.react <= 0.0 {
            crossing.stepped_off = true;
        }
    }
}
