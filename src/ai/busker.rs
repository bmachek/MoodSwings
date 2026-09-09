//! Somebody playing on the pavement, and the ring of people who stopped.
//!
//! The `busking` recording has been sitting in the sound bank since the sound
//! bank existed with nothing in the world to come out of, and the crowd has
//! been walking past everything it has ever been shown. This is one citizen who
//! stopped walking, and a handful more who stopped because of them.
//!
//! ## A busker is not a new kind of person
//!
//! The tempting build is a `Busker` archetype spawned like a pedestrian is —
//! and a pedestrian is forty lines of wardrobe, temperament, voice, collider,
//! stature and figure assembly, none of which a busker needs differently. So
//! there is no new kind of person here at all: a busker is an *ordinary
//! resident who has stopped*, marked with a component, handed a guitar, and
//! left to be dressed, moody and knock-downable exactly like everybody else.
//! Bounce one over a car and the pitch is vacant until somebody else takes it.
//!
//! Listeners are the same trick and it matters more there, because the point of
//! a crowd round a busker is that it is made of *the* crowd: the same citizens
//! who were walking past a moment ago, who will walk on again, and who each
//! carry the mood they arrived with.
//!
//! ## The ring answers to the city
//!
//! How many people stop is read off [`CityMood::average`]. That is the one
//! decision in this module that is really about the game rather than about
//! street furniture: a cheerful city gathers round somebody playing, and a city
//! that has just been provoked into a rage-wave walks past. It gives the mood
//! system something visible to do that is not a face — you can see the city's
//! temper from a hundred metres by how big the circle is.

use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::figure::{Rest, WalkCycle, body};
use super::pedestrian::Pedestrian;
use crate::bounce::controller::Bouncer;
use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::mood::feeling::CityMood;

/// How many pitches are working at once near the player.
///
/// Three rather than two, and the ring is pulled in to fifty-five metres with
/// it. At two, over a crowd of twenty-odd spread across a hundred and twenty
/// metres of streets, a busker is a thing that is definitely happening
/// somewhere and that you almost never walk into — which is the same as not
/// having built it.
const PITCHES: usize = 3;

/// A busker is picked from residents this far from the player: near enough to
/// walk into, far enough that one does not appear at your elbow — and further
/// out than [`EARSHOT`], so nobody ever takes a pitch standing inside somebody
/// else's audience.
const PICK: (f32, f32) = (26.0, 55.0);
/// And gives up the pitch past this, which is where the crowd is recycled
/// anyway.
const FORGET: f32 = 150.0;

/// How far the music carries.
///
/// This is not how close somebody has to be standing, it is how far away they
/// can be and still decide to come over — and the difference is the whole
/// effect. At a pavement's width the ring only ever collects whoever happened
/// to be walking through it, which at this crowd's density is nobody: a busker
/// played to an empty circle with two people wandering past behind. At twenty
/// metres people *arrive*, on foot, from up the street, which is what a crowd
/// gathering actually looks like.
const EARSHOT: f32 = 22.0;
/// And where the ring stands: near enough to be an audience, far enough that
/// nobody is standing on the hat.
const RING: (f32, f32) = (1.6, 3.4);

/// The size of the ring at the two ends of the city's mood.
///
/// Never nought. There is always one person who has stopped, even in a foul
/// city, and a busker playing to nobody at all reads as a bug rather than as a
/// bad afternoon.
const AUDIENCE: (usize, usize) = (1, 7);

/// Seconds between reconsidering who is listening. Slow on purpose: a ring that
/// re-forms every frame is a ring that jitters, and nobody decides to stop and
/// listen eleven times a second.
const RETHINK: f32 = 1.3;

/// On the citizen who has stopped to play.
#[derive(Component)]
pub struct Busker {
    /// Where the pitch is. Kept so the ring can be measured from the spot
    /// rather than from a body that may have been bounced off it.
    pub pitch: Vec2,
}

/// On the guitar's two parts.
///
/// A marker rather than despawning the busker's children, and this one would
/// have been expensive to find in a screenshot: a citizen's arms, legs, head
/// and face are *all* children of the same entity, so clearing the children to
/// take the guitar away takes the person with it and leaves a face floating
/// over a pavement with nothing under it.
#[derive(Component)]
struct Guitar;

/// On a citizen who has stopped to listen, and where they stopped.
#[derive(Component)]
pub struct Listening {
    to: Entity,
    stand: Vec2,
}

#[derive(Resource)]
struct BuskerRng(ChaCha8Rng);

#[derive(Resource)]
struct BuskerTimer(Timer);

impl Default for BuskerTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(RETHINK, TimerMode::Repeating))
    }
}

#[derive(Resource)]
struct BuskerKit {
    body: Handle<Mesh>,
    neck: Handle<Mesh>,
    /// Spruce and rosewood, near enough.
    timber: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
}

pub struct BuskerPlugin;

impl Plugin for BuskerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuskerTimer>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (take_a_pitch, gather, hold_still)
                    .chain()
                    .in_set(GameSet::Ai)
                    .after(super::pedestrian::Walking),
            );
    }
}

fn setup(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(BuskerRng(stream_for(config.world_seed, stream::BUSKERS)));
    commands.insert_resource(BuskerKit {
        // A guitar is two shapes: a flattened body and a stick. At the size a
        // pedestrian is ever seen, the waist is a rumour.
        body: meshes.add(
            Sphere::new(1.0)
                .mesh()
                .ico(2)
                .expect("an icosphere at two subdivisions"),
        ),
        neck: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        timber: materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.42, 0.22),
            perceptual_roughness: 0.55,
            ..default()
        }),
        dark: materials.add(StandardMaterial {
            base_color: Color::srgb(0.22, 0.14, 0.09),
            perceptual_roughness: 0.60,
            ..default()
        }),
    });
}

/// Keeps [`PITCHES`] citizens standing and playing.
fn take_a_pitch(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: ResMut<BuskerTimer>,
    kit: Res<BuskerKit>,
    mut rng: ResMut<BuskerRng>,
    focus: Res<super::focus::SimFocus>,
    playing: Query<(Entity, &Transform), With<Busker>>,
    idle: Query<(Entity, &Transform), (With<Pedestrian>, Without<Busker>, Without<Listening>)>,
    guitars: Query<(Entity, &ChildOf), With<Guitar>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let focus = focus.ground();

    let mut working = 0usize;
    for (entity, transform) in &playing {
        if transform.translation.xz().distance(focus) > FORGET {
            // Off the pitch. The guitar goes and the citizen is an ordinary
            // pedestrian again from the next frame — which, being past the
            // despawn ring, is a citizen about to be recycled anyway.
            commands.entity(entity).remove::<Busker>();
            for (part, owner) in &guitars {
                if owner.parent() == entity {
                    // Forgiving: the guitar is a child of the busker, and a
                    // busker who walked off the despawn ring took it with them.
                    commands.entity(part).try_despawn();
                }
            }
        } else {
            working += 1;
        }
    }
    if working >= PITCHES {
        return;
    }

    let candidates: Vec<(Entity, Vec2)> = idle
        .iter()
        .map(|(entity, transform)| (entity, transform.translation.xz()))
        .filter(|(_, at)| (PICK.0..PICK.1).contains(&at.distance(focus)))
        .collect();
    if candidates.is_empty() {
        return;
    }
    let (entity, at) = candidates[rng.0.random_range(0..candidates.len())];

    // The guitar, held across the front the way somebody standing up plays
    // one. A `Rest` so `figure::animate` places it: it then squashes with its
    // owner, which for a guitar is not accurate and is very funny.
    let (across, neck) = guitar();
    commands.entity(entity).insert(Busker { pitch: at });
    commands.entity(entity).with_children(|parent| {
        parent.spawn((
            Guitar,
            Rest::posed(across, Vec3::new(0.19, 0.23, 0.075)),
            Mesh3d(kit.body.clone()),
            MeshMaterial3d(kit.timber.clone()),
            Transform::from_translation(across),
        ));
        parent.spawn((
            Guitar,
            Rest::posed(neck, Vec3::new(0.44, 0.045, 0.035)),
            Mesh3d(kit.neck.clone()),
            MeshMaterial3d(kit.dark.clone()),
            Transform::from_translation(neck).with_rotation(Quat::from_rotation_z(0.55)),
        ));
    });
}

/// Where the guitar's body and neck hang, in body-local metres.
///
/// Its own function so the arithmetic can be tested. There is no reliable way
/// to photograph this: a busker is one of three citizens picked out of a crowd
/// that has wandered somewhere else by the time a second run reaches the same
/// frame, and six attempts at framing one produced six empty pavements. What
/// *can* be pinned is that the thing is in front of the chest rather than
/// inside it, under the chin rather than through it, and above the hip.
fn guitar() -> (Vec3, Vec3) {
    (
        Vec3::new(0.06, body::SHOULDER - 0.30, -0.20),
        Vec3::new(-0.30, body::SHOULDER - 0.12, -0.20),
    )
}

/// How many people the city's temper will stop for.
///
/// Pulled out so the one line that ties this module to the mood system can be
/// read, and tested, on its own.
fn audience_for(mood: f32) -> usize {
    // The mood runs -1 to 1; the ring runs from the one person who always
    // stops to a proper crowd.
    let warmth = (mood * 0.5 + 0.5).clamp(0.0, 1.0);
    let span = (AUDIENCE.1 - AUDIENCE.0) as f32;
    AUDIENCE.0 + (warmth * span).round() as usize
}

/// Who stops, and who moves on.
fn gather(
    mut commands: Commands,
    timer: Res<BuskerTimer>,
    city: Res<CityMood>,
    mut rng: ResMut<BuskerRng>,
    buskers: Query<(Entity, &Busker)>,
    passing: Query<(Entity, &Transform), (With<Pedestrian>, Without<Busker>, Without<Listening>)>,
    listeners: Query<(Entity, &Listening)>,
) {
    // Piggybacks on the pitch timer rather than keeping a second one: both
    // want the same slow cadence and running them on different clocks means a
    // ring that fills a beat after the busker appears and empties a beat after
    // they leave.
    if !timer.0.just_finished() {
        return;
    }

    let wanted = audience_for(city.average);
    for (busker, pitch) in &buskers {
        let mut standing: Vec<Entity> = listeners
            .iter()
            .filter(|(_, listening)| listening.to == busker)
            .map(|(entity, _)| entity)
            .collect();

        // Too many, or the city has soured: the ones at the back drift off.
        while standing.len() > wanted {
            if let Some(leaving) = standing.pop() {
                commands.entity(leaving).remove::<Listening>();
            }
        }

        // And whoever has wandered into earshot fills the rest of the ring.
        for (entity, transform) in &passing {
            if standing.len() >= wanted {
                break;
            }
            let here = transform.translation.xz();
            if here.distance(pitch.pitch) > EARSHOT {
                continue;
            }
            // A place on the circle, roughly where they came from, so nobody
            // walks through the busker to reach the far side.
            let bearing = (here - pitch.pitch).to_angle() + rng.0.random_range(-0.5..0.5);
            let radius = rng.0.random_range(RING.0..RING.1);
            let stand = pitch.pitch + Vec2::from_angle(bearing) * radius;
            commands
                .entity(entity)
                .insert(Listening { to: busker, stand });
            standing.push(entity);
        }
    }

    // A listener whose busker has gone back to walking is just a person
    // standing in the street, which is the one way this can go wrong quietly.
    for (entity, listening) in &listeners {
        if buskers.get(listening.to).is_err() {
            commands.entity(entity).remove::<Listening>();
        }
    }
}

/// Buskers stand still and listeners walk to their spot and then stand still.
///
/// Runs after `pedestrian::Walking`, which is where `bouncer.desired` is set
/// from a citizen's route: this overwrites it rather than reaching into the
/// walk itself, so a busker who stops being one simply carries on down the
/// pavement from the next frame with nothing to undo.
fn hold_still(
    mut commands: Commands,
    time: Res<Time>,
    mut playing: Query<
        (&Transform, &mut Bouncer, &mut WalkCycle),
        (With<Busker>, Without<Listening>),
    >,
    mut standing: Query<
        (Entity, &Transform, &Listening, &mut Bouncer, &mut WalkCycle),
        Without<Busker>,
    >,
    pitches: Query<&Transform, With<Busker>>,
) {
    for (_, mut bouncer, mut cycle) in &mut playing {
        bouncer.desired = Vec2::ZERO;
        cycle.speed = 0.0;
        // Still bouncing, and bouncing harder than a person standing about:
        // this is somebody playing, and in this city that is the only
        // performance available.
        bouncer.hop_scale = 1.35;
    }

    let now = time.elapsed_secs();
    for (entity, transform, listening, mut bouncer, mut cycle) in &mut standing {
        let here = transform.translation.xz();
        let to_spot = listening.stand - here;
        if to_spot.length() > 0.35 {
            let walk = to_spot.normalize_or_zero() * 1.1;
            bouncer.desired = walk;
            cycle.speed = walk.length();
        } else {
            bouncer.desired = Vec2::ZERO;
            cycle.speed = 0.0;
        }
        // Watch the act. A ring of people standing in a circle facing
        // nothing in particular is scenery; a ring of heads all turned the
        // same way is an audience, and the difference costs one component.
        if let Ok(pitch) = pitches.get(listening.to) {
            commands.entity(entity).insert(super::figure::Attention::to(
                pitch.translation,
                now,
                1.2,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ring answers to the city and never empties.
    #[test]
    fn a_glum_city_still_leaves_one_person_listening() {
        assert_eq!(audience_for(-1.0), AUDIENCE.0);
        assert_eq!(audience_for(1.0), AUDIENCE.1);
        assert!(AUDIENCE.0 >= 1, "a busker playing to nobody reads as a bug");
        // Monotone, or the crowd swells as the city sours.
        let mut previous = 0;
        for step in 0..=40 {
            let mood = -1.0 + step as f32 / 20.0;
            let now = audience_for(mood);
            assert!(now >= previous, "the ring shrank as the city cheered up");
            previous = now;
        }
        // And out-of-range moods do not produce an out-of-range ring.
        assert_eq!(audience_for(-9.0), AUDIENCE.0);
        assert_eq!(audience_for(9.0), AUDIENCE.1);
    }

    /// The guitar is held in front of the player rather than through them.
    #[test]
    fn the_guitar_is_where_a_pair_of_hands_would_be() {
        let (across, neck) = guitar();
        for part in [across, neck] {
            // In front. The figure faces -Z, so a held instrument has to be on
            // the negative side of the torso and clear of it.
            assert!(part.z < -0.12, "the guitar is inside the ribcage");
            assert!(part.z > -0.45, "it is being held at arm's length");
            // Below the chin and above the hip.
            assert!(
                part.y < body::HEAD_CENTRE - body::HEAD_RADIUS,
                "the guitar is under their nose"
            );
            assert!(part.y > body::HIP, "it is being played round the knees");
        }
        // And the neck is up and to one side of the body, which is what a neck
        // is: the same height and the same place is a guitar in two halves.
        assert!(neck.y > across.y, "the neck points down");
        assert!(
            (neck.x - across.x).abs() > 0.2,
            "the neck is inside the body"
        );
    }

    /// The audience stands round the pitch rather than on it.
    #[test]
    fn nobody_stands_on_the_hat() {
        // Inside the ring is where the busker is, and past earshot is where
        // somebody has walked away — so the ring has to sit between a person's
        // own width and the distance the music carries.
        assert!(RING.0 > 1.0, "the front row is inside the guitar");
        assert!(RING.1 < EARSHOT, "the back row cannot hear it");
        // And a listener heading for their spot has somewhere to walk from:
        // earshot has to be comfortably wider than the ring itself.
        assert!(EARSHOT > RING.1 * 1.8);
    }

    /// A pitch is taken at conversational distance, not across the city.
    #[test]
    fn a_busker_is_close_enough_to_walk_to() {
        assert!(PICK.0 > EARSHOT, "one could appear inside its own audience");
        assert!(PICK.1 < FORGET, "a busker would be forgotten as it appears");
    }
}
