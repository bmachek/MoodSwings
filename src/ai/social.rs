//! The street's social life: who stops for whom, and why.
//!
//! A crowd that only walks routes is a screensaver. What makes a street read
//! as *lived in* is everything that interrupts the walking: two neighbours
//! stopping for a word, a window worth looking into, everybody turning round
//! when somebody is sent flying, and the Wutbürger getting angrier at the
//! traffic than the traffic could ever deserve. None of it is scripted per
//! citizen — each behaviour is a small rule gated on who somebody is (their
//! [`Archetype`]), how old they are, what the clock says and what the sky is
//! doing, and the city's life falls out of the overlap.
//!
//! Everything here *overrides* the walking intent the same way a grudge does:
//! the systems run after [`Walking`] and write `Bouncer::desired` (and, while
//! somebody is standing still, their facing) on top of what the route said.
//! A panic, a grudge or a launch always wins — sociability is the first thing
//! anybody drops when a car mounts the kerb — which is what
//! [`break_up_chats`] is for.
//!
//! The randomness is playback jitter, not world generation, so it draws from
//! [`AudioRng`] exactly as the grudge rolls do: none of it may touch a
//! generation stream, and none of it needs to be reproducible from the seed.

use avian3d::prelude::*;
use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use rand::RngExt;

use super::archetype::{AgeClass, Archetype};
use super::pedestrian::{Follows, Pedestrian, Walking};
use super::steering::right_of;
use crate::audio::AudioRng;
use crate::bounce::boing::Wallop;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::config::GameConfig;
use crate::core::schedule::GameSet;
use crate::mood::feeling::{Mood, Temperament, caught};
use crate::mood::grudge::{Grudge, Pirouette};
use crate::world::City;

/// How close two citizens have to pass for a chat to strike up, in metres.
const CHAT_RANGE: f32 = 1.7;
/// Chance per second that a pair in range actually stops, before either
/// party's chattiness scales it. Passing strangers overlap for a stride or
/// two, so most passes stay passes — a street where every crossing is a
/// conversation is a cocktail party.
const CHAT_CHANCE: f32 = 0.055;
/// How long a chat runs, either side of the draw.
const CHAT_SECONDS: (f32, f32) = (3.5, 8.5);
/// How hard two chatting moods pull on each other, as a multiple of the
/// street's ordinary contagion rate. Talking to somebody is catching their
/// mood on purpose.
const CHAT_CONTAGION: f32 = 6.0;
/// Comfortable talking distance, and the gentle speed used to close to it.
const CHAT_APART: f32 = 1.1;
const CHAT_STEP: f32 = 0.8;
/// How long the composure after a chat or a loiter lasts: nobody stops twice
/// on the same block.
const COMPOSURE: (f32, f32) = (18.0, 40.0);

/// A wallop worth turning round for, and how far the turning reaches.
const GAWK_AT: f32 = 6.0;
const GAWK_RANGE: f32 = 13.0;
const GAWK_SECONDS: f32 = 2.4;
/// Chance each bystander actually looks. Somebody always misses it, which is
/// what stops the whole street snapping round like a drill team.
const GAWK_CHANCE: f32 = 0.75;

/// A car this close and this fast is, to a Wutbürger, a personal insult.
/// The speed matches `CrowdConfig::scare_speed`: the traffic that worries
/// everybody else is the traffic that outrages him.
const RANT_RANGE: f32 = 9.0;
const RANT_AT_SPEED: f32 = 6.0;
/// Mood lost per second of glaring at it. Against a ragemonger recovery of
/// 0.08 this wins easily, which is the point: traffic keeps the Wutbürger
/// stocked with the anger the rest of the street then catches.
const RANT_STING: f32 = 0.22;

/// An Elvis needs an audience this big, this close, before the show starts.
const AUDIENCE: usize = 3;
const STAGE: f32 = 8.0;
const SHOWOFF_CHANCE: f32 = 0.06;

/// A busker settles for less of a crowd than an Elvis — two listeners is a
/// gig — and holds the pitch for a whole loop of the recording. While he
/// plays, everyone in stage range who can hear drifts gently upward: the
/// one provocation in the city that only ever improves things.
const BUSK_AUDIENCE: usize = 2;
const BUSK_CHANCE: f32 = 0.10;
const BUSK_SECONDS: f32 = 16.0;
const BUSK_LIFT: f32 = 0.035;
const BUSK_GAIN: f32 = 0.55;
const BUSK_EARSHOT: f32 = 26.0;

/// The photographer's decisive moment: range, chance, and what being
/// photographed does to somebody. Most citizens are flattered. The Shy are
/// having the worst moment of their week, which is the joke with the
/// sting left in — er knipst, sie leidet.
const SNAP_RANGE: f32 = 6.5;
const SNAP_CHANCE: f32 = 0.06;
const SNAP_SECONDS: f32 = 2.4;
const SNAP_FLATTERY: f32 = 0.10;
const SNAP_INTRUSION: f32 = 0.18;
const SNAP_GAIN: f32 = 0.6;
const SNAP_EARSHOT: f32 = 14.0;
/// And a skater pops an ollie now and then whatever the gait setting says —
/// a deliberate trick, like a jump, not a way of travelling.
const OLLIE_CHANCE: f32 = 0.07;
const OLLIE: f32 = 1.5;

/// Mid-conversation. Both parties carry one, each ticking its own copy of
/// the same clock, so a partner streaming out mid-sentence strands nobody.
#[derive(Component, Debug)]
pub struct Chatting {
    pub with: Entity,
    pub left: f32,
}

/// Standing still on purpose: a shop window, a rest on the cane, a spot
/// worth begging on. `face` is a world-space direction to hold, if the
/// stop has one — a window-shopper faces the window, a resting senior
/// faces wherever they stopped.
#[derive(Component, Debug)]
pub struct Loitering {
    pub left: f32,
    pub face: Option<Vec2>,
}

/// Turned round to watch somebody else's accident.
#[derive(Component, Debug)]
pub struct Rubbernecking {
    pub at: Vec3,
    pub left: f32,
}

/// Mid-set. The emitter is the looping recording, hung as a child so the
/// music travels with the musician — a launched busker keeps playing all
/// the way through the arc, which is the correct amount of professionalism.
#[derive(Component, Debug)]
pub struct Busking {
    pub left: f32,
    pub emitter: Entity,
}

/// Recently finished being sociable; not about to start again. One cooldown
/// for chats and loiters both, so a citizen does something at most once per
/// block rather than stuttering down the pavement.
#[derive(Component, Debug)]
pub struct Composure {
    pub left: f32,
}

/// Everything that interrupts the walking for a social reason.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Socialising;

pub struct SocialPlugin;

impl Plugin for SocialPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                (
                    tick_composure,
                    break_up_chats,
                    strike_up_chats,
                    start_loitering,
                    rubberneck_at_wallops,
                    sour_at_traffic,
                    show_off,
                    busk,
                    snap,
                ),
                (hold_chats, hold_loiters, gawk),
            )
                .chain()
                .in_set(Socialising)
                .in_set(GameSet::Ai)
                // Overrides of the walking intent, exactly like a grudge:
                // whatever runs later wins, and these run later.
                .after(Walking),
        );
    }
}

// ------------------------------------------------------------- the maths ----

/// How full the pavements are at this hour, as a fraction of the configured
/// population. Busiest mid-afternoon, quietest in the small hours — the city
/// never quite empties, because a city that empties is a level that switched
/// off, but four in the morning should feel like four in the morning.
pub fn crowd_level(hours: f32) -> f32 {
    let angle = (hours - 15.5) / 24.0 * std::f32::consts::TAU;
    0.675 + 0.325 * angle.cos()
}

/// The rain multiplier on walking pace. People hurry through weather; they
/// do not sprint through it, so the cap stays well under a flee.
pub fn hurry(rain: f32) -> f32 {
    1.0 + 0.28 * rain.clamp(0.0, 1.0)
}

/// Chance per second that this particular pair stops to talk. Multiplied
/// rather than averaged, so one unwilling party vetoes the whole thing —
/// the Shy and the Headphones wearers set theirs to zero and are never
/// buttonholed, which is exactly what wearing headphones is *for*.
pub fn chat_chance(a: (Archetype, AgeClass), b: (Archetype, AgeClass), rain: f32) -> f32 {
    CHAT_CHANCE
        * a.0.chattiness()
        * a.1.chattiness()
        * b.0.chattiness()
        * b.1.chattiness()
        * (1.0 - 0.8 * rain.clamp(0.0, 1.0))
}

// ----------------------------------------------------------- the systems ----

fn tick_composure(
    mut commands: Commands,
    time: Res<Time>,
    mut composed: Query<(Entity, &mut Composure)>,
) {
    for (entity, mut composure) in &mut composed {
        composure.left -= time.delta_secs();
        if composure.left <= 0.0 {
            commands.entity(entity).remove::<Composure>();
        }
    }
}

/// Finds pairs passing close enough and rolls whether they stop.
fn strike_up_chats(
    mut commands: Commands,
    time: Res<Time>,
    weather: Res<crate::world::weather::Weather>,
    mut rng: ResMut<AudioRng>,
    candidates: Query<
        (
            Entity,
            &Transform,
            &Pedestrian,
            &Archetype,
            &AgeClass,
            Option<&Follows>,
        ),
        (
            Without<Chatting>,
            Without<Loitering>,
            Without<Rubbernecking>,
            Without<Composure>,
            Without<Grudge>,
            Without<Launched>,
            Without<crate::mood::scuffle::Scuffle>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let eligible: Vec<_> = candidates
        .iter()
        .filter(|(_, _, pedestrian, ..)| pedestrian.panic <= 0.0)
        .collect();

    let mut taken: HashSet<Entity> = HashSet::new();
    for (i, a) in eligible.iter().enumerate() {
        if taken.contains(&a.0) {
            continue;
        }
        for b in eligible.iter().skip(i + 1) {
            if taken.contains(&b.0) {
                continue;
            }
            // Gang members march in each other's pockets all day; the group
            // is company enough, and a column that stops to chat with itself
            // never arrives anywhere.
            let related = |x: Option<&Follows>, other: Entity| x.is_some_and(|f| f.0 == other);
            let same_gang = match (a.5, b.5) {
                (Some(fa), Some(fb)) => fa.0 == fb.0,
                _ => related(a.5, b.0) || related(b.5, a.0),
            };
            if same_gang {
                continue;
            }
            if a.1.translation.distance(b.1.translation) > CHAT_RANGE {
                continue;
            }
            let chance = chat_chance((*a.3, *a.4), (*b.3, *b.4), weather.rain);
            if rng.random::<f32>() > chance * dt {
                continue;
            }
            let left = rng.random_range(CHAT_SECONDS.0..CHAT_SECONDS.1);
            commands.entity(a.0).insert(Chatting { with: b.0, left });
            commands.entity(b.0).insert(Chatting { with: a.0, left });
            taken.insert(a.0);
            taken.insert(b.0);
            break;
        }
    }
}

/// Holds both parties in place, faces them at each other, and lets the moods
/// do what moods do when people talk: converge.
///
/// One query, iterated twice — a read snapshot and then the write pass — the
/// same shape as `mood::feeling::spread_moods` and for the same schedule-trap
/// reason: the partner's `Transform` and `Mood` live in the very query being
/// mutated.
fn hold_chats(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<GameConfig>,
    mut rng: ResMut<AudioRng>,
    mut chatting: Query<(
        Entity,
        &mut Transform,
        &mut Bouncer,
        &mut Mood,
        &Temperament,
        &mut Chatting,
    )>,
) {
    let dt = time.delta_secs();
    let others: HashMap<Entity, (Vec3, f32)> = chatting
        .iter()
        .map(|(entity, transform, _, mood, ..)| (entity, (transform.translation, mood.value)))
        .collect();

    for (entity, mut transform, mut bouncer, mut mood, temper, mut chat) in &mut chatting {
        chat.left -= dt;
        let partner = others.get(&chat.with).copied();
        if chat.left <= 0.0 || partner.is_none() {
            commands
                .entity(entity)
                .remove::<Chatting>()
                .insert(Composure {
                    left: rng.random_range(COMPOSURE.0..COMPOSURE.1),
                });
            continue;
        }
        let (there, their_mood) = partner.unwrap();
        let apart = (there - transform.translation).with_y(0.0);

        // Stand at talking distance: close the gap if it opened, otherwise
        // hold still and let the walk cycle idle out.
        bouncer.desired = if apart.length() > CHAT_APART {
            apart.normalize_or_zero().xz() * CHAT_STEP
        } else {
            Vec2::ZERO
        };
        if let Ok(facing) = Dir2::new(apart.xz()) {
            transform.rotation =
                Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
        }
        // Talking is catching a mood on purpose: the ordinary contagion,
        // several times over, aimed at exactly one person.
        mood.value = caught(
            mood.value,
            their_mood,
            temper,
            config.mood.contagion_rate * CHAT_CONTAGION,
            dt,
        )
        .clamp(-1.0, 1.0);
    }
}

/// Sociability is the first thing anybody drops: a panic, a grudge or a
/// launch ends the conversation mid-word.
fn break_up_chats(
    mut commands: Commands,
    interrupted: Query<
        Entity,
        (
            With<Chatting>,
            Or<(
                With<Grudge>,
                With<Launched>,
                With<crate::mood::scuffle::Scuffle>,
            )>,
        ),
    >,
    panicked: Query<(Entity, &Pedestrian), With<Chatting>>,
) {
    for entity in &interrupted {
        commands.entity(entity).remove::<Chatting>();
    }
    for (entity, pedestrian) in &panicked {
        if pedestrian.panic > 0.0 {
            commands.entity(entity).remove::<Chatting>();
        }
    }
}

/// Starts the archetype-specific stops: the shop window, the begging spot,
/// the rest on the cane, the corner worth holding up.
fn start_loitering(
    mut commands: Commands,
    time: Res<Time>,
    city: Res<City>,
    mut rng: ResMut<AudioRng>,
    candidates: Query<
        (Entity, &Pedestrian, &Archetype),
        (
            Without<Chatting>,
            Without<Loitering>,
            Without<Rubbernecking>,
            Without<Composure>,
            Without<Grudge>,
            Without<Launched>,
        ),
    >,
) {
    let dt = time.delta_secs();
    for (entity, pedestrian, archetype) in &candidates {
        let Some((chance, seconds, window)) = archetype.loiter() else {
            continue;
        };
        if pedestrian.panic > 0.0 || rng.random::<f32>() > chance * dt {
            continue;
        }
        // A window-shopper faces the buildings, which sit outward of the
        // pavement: off the street's direction of travel, on their side.
        let face = window.then(|| {
            let a = city.graph.node(pedestrian.from).pos;
            let b = city.graph.node(pedestrian.to).pos;
            right_of((b - a).normalize_or_zero()) * pedestrian.side
        });
        commands.entity(entity).insert(Loitering {
            left: seconds * rng.random_range(0.7..1.3),
            face,
        });
    }
}

fn hold_loiters(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<AudioRng>,
    mut loitering: Query<(
        Entity,
        &mut Transform,
        &mut Bouncer,
        &Pedestrian,
        &mut Loitering,
    )>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut bouncer, pedestrian, mut stop) in &mut loitering {
        stop.left -= dt;
        // A car ends a window-shop the same way it ends a chat.
        if stop.left <= 0.0 || pedestrian.panic > 0.0 {
            commands
                .entity(entity)
                .remove::<Loitering>()
                .insert(Composure {
                    left: rng.random_range(COMPOSURE.0..COMPOSURE.1),
                });
            continue;
        }
        bouncer.desired = Vec2::ZERO;
        if let Some(face) = stop.face
            && let Ok(facing) = Dir2::new(face)
        {
            transform.rotation =
                Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
        }
    }
}

/// Somebody sent flying is a spectacle, and a street that fails to notice a
/// spectacle is a street of props. Read-only over the crowd — the actual
/// stopping and turning happens in [`gawk`] — so the wallop victim's own
/// transform can be looked up without the two-queries-one-component trap.
fn rubberneck_at_wallops(
    mut commands: Commands,
    mut rng: ResMut<AudioRng>,
    mut wallops: MessageReader<Wallop>,
    positions: Query<&Transform>,
    bystanders: Query<
        (Entity, &Transform),
        (
            With<Pedestrian>,
            Without<Launched>,
            Without<Chatting>,
            Without<Rubbernecking>,
        ),
    >,
) {
    for wallop in wallops.read() {
        if wallop.severity < GAWK_AT {
            continue;
        }
        let Ok(scene) = positions.get(wallop.entity) else {
            continue;
        };
        let at = scene.translation;
        for (entity, transform) in &bystanders {
            if entity == wallop.entity {
                continue;
            }
            if transform.translation.distance(at) > GAWK_RANGE {
                continue;
            }
            if rng.random::<f32>() > GAWK_CHANCE {
                continue;
            }
            commands.entity(entity).insert(Rubbernecking {
                at,
                left: GAWK_SECONDS * rng.random_range(0.7..1.2),
            });
        }
    }
}

fn gawk(
    mut commands: Commands,
    time: Res<Time>,
    mut gawkers: Query<(
        Entity,
        &mut Transform,
        &mut Bouncer,
        &Pedestrian,
        &mut Rubbernecking,
    )>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut bouncer, pedestrian, mut look) in &mut gawkers {
        look.left -= dt;
        if look.left <= 0.0 || pedestrian.panic > 0.0 {
            commands.entity(entity).remove::<Rubbernecking>();
            continue;
        }
        bouncer.desired = Vec2::ZERO;
        let towards = (look.at - transform.translation).xz();
        if let Ok(facing) = Dir2::new(towards) {
            transform.rotation =
                Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
        }
    }
}

/// The Wutbürger and the traffic.
///
/// No jolt, no message, no special case in the mood pipeline: a fast car in a
/// Wutbürger's field of grievance simply drains their mood while they glare
/// at it, and everything after that — the reddening face, the spontaneous
/// taunt, the neighbours catching it — falls out of the systems that already
/// exist. Traffic is how the city keeps its ragemongers stocked with rage.
fn sour_at_traffic(
    time: Res<Time>,
    vehicles: Query<(&Transform, &LinearVelocity), With<crate::vehicle::spawn::Vehicle>>,
    mut grouches: Query<
        (&mut Transform, &mut Mood, &Archetype, &Pedestrian),
        (
            Without<crate::vehicle::spawn::Vehicle>,
            Without<Launched>,
            Without<Chatting>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let passing: Vec<Vec3> = vehicles
        .iter()
        .filter(|(_, velocity)| velocity.length() > RANT_AT_SPEED)
        .map(|(transform, _)| transform.translation)
        .collect();
    if passing.is_empty() {
        return;
    }

    for (mut transform, mut mood, archetype, pedestrian) in &mut grouches {
        if *archetype != Archetype::Wutbuerger || pedestrian.panic > 0.0 {
            continue;
        }
        let here = transform.translation;
        let Some(car) = passing
            .iter()
            .filter(|at| at.distance(here) < RANT_RANGE)
            .min_by(|a, b| a.distance(here).total_cmp(&b.distance(here)))
        else {
            continue;
        };
        mood.value = (mood.value - RANT_STING * dt).clamp(-1.0, 1.0);
        if let Ok(facing) = Dir2::new((*car - here).xz()) {
            transform.rotation =
                Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
        }
    }
}

/// The performers: an Elvis with an audience strikes a spin, and a skater
/// pops an ollie whatever the gait setting says — a trick is a jump, not a
/// way of travelling, so it survives the walking city.
fn show_off(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<AudioRng>,
    mut performers: Query<
        (Entity, &Transform, &mut Bouncer, &Archetype, &Mood),
        (
            With<Pedestrian>,
            Without<Launched>,
            Without<Chatting>,
            Without<Pirouette>,
            Without<Grudge>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let crowd: Vec<Vec3> = performers
        .iter()
        .map(|(_, transform, ..)| transform.translation)
        .collect();

    for (entity, transform, mut bouncer, archetype, mood) in &mut performers {
        match archetype {
            Archetype::Elvis => {
                if mood.value < 0.0 || rng.random::<f32>() > SHOWOFF_CHANCE * dt {
                    continue;
                }
                let here = transform.translation;
                let audience = crowd
                    .iter()
                    .filter(|at| {
                        let apart = at.distance(here);
                        apart > f32::EPSILON && apart < STAGE
                    })
                    .count();
                if audience >= AUDIENCE {
                    commands.entity(entity).insert(Pirouette {
                        left: 1.2,
                        towards: None,
                    });
                }
            }
            Archetype::Skater => {
                if rng.random::<f32>() > OLLIE_CHANCE * dt {
                    continue;
                }
                // Spent on the next landing, exactly like a player's jump.
                bouncer.hop_scale = OLLIE;
            }
            _ => {}
        }
    }
}

/// The busker's set: given a modest audience he stops, plays the recording,
/// and gently lifts every mood in earshot for as long as the set runs. The
/// music is a child entity, so it flies with him if somebody launches him
/// mid-song.
#[allow(clippy::type_complexity)]
fn busk(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<AudioRng>,
    config: Res<GameConfig>,
    bank: Option<Res<crate::audio::bank::SoundBank>>,
    mut buskers: Query<
        (
            Entity,
            &Transform,
            &Archetype,
            Option<&Loitering>,
            Option<&Composure>,
            Option<&Chatting>,
            Option<&Grudge>,
            Option<&Launched>,
            Option<&mut Busking>,
        ),
        With<Pedestrian>,
    >,
    mut street: Query<(&Transform, &Archetype, &mut Mood), With<Pedestrian>>,
) {
    let dt = time.delta_secs();

    // The running sets: tick them down, and strike the stage when the set
    // ends or something ended the stop for him — a panic clears Loitering,
    // and a launch is its own kind of encore.
    let mut stages: Vec<Vec3> = Vec::new();
    for (entity, transform, _, loitering, .., launched, busking) in &mut buskers {
        let Some(mut busking) = busking else { continue };
        busking.left -= dt;
        if busking.left <= 0.0 || loitering.is_none() || launched.is_some() {
            commands.entity(busking.emitter).despawn();
            commands.entity(entity).remove::<Busking>();
            continue;
        }
        stages.push(transform.translation);
    }

    // New sets, for any busker at liberty with a crowd worth playing to.
    let crowd: Vec<Vec3> = street.iter().map(|(t, ..)| t.translation).collect();
    for (entity, transform, archetype, loitering, composure, chatting, grudge, launched, busking) in
        &buskers
    {
        if *archetype != Archetype::Busker
            || busking.is_some()
            || loitering.is_some()
            || composure.is_some()
            || chatting.is_some()
            || grudge.is_some()
            || launched.is_some()
        {
            continue;
        }
        if rng.random::<f32>() > BUSK_CHANCE * dt {
            continue;
        }
        let here = transform.translation;
        let audience = crowd
            .iter()
            .filter(|at| {
                let apart = at.distance(here);
                apart > f32::EPSILON && apart < STAGE
            })
            .count();
        if audience < BUSK_AUDIENCE {
            continue;
        }
        let Some(bank) = bank.as_ref() else { continue };
        let emitter = commands
            .spawn((
                bevy::audio::AudioPlayer(bank.busking.clone()),
                bevy::audio::PlaybackSettings::LOOP
                    .with_volume(bevy::audio::Volume::Linear(crate::audio::effect_gain(
                        &config, BUSK_GAIN,
                    )))
                    .with_spatial(true)
                    .with_spatial_scale(bevy::audio::SpatialScale::new(1.0 / BUSK_EARSHOT)),
                ChildOf(entity),
            ))
            .id();
        commands.entity(entity).insert((
            Busking {
                left: BUSK_SECONDS,
                emitter,
            },
            Loitering {
                left: BUSK_SECONDS,
                face: None,
            },
        ));
        stages.push(here);
    }

    // The lift. Deafness works on music exactly as it works on cheers —
    // headphones are a commitment.
    if !stages.is_empty() {
        for (transform, archetype, mut mood) in &mut street {
            if archetype.deaf() {
                continue;
            }
            if stages
                .iter()
                .any(|stage| stage.distance(transform.translation) < STAGE)
            {
                mood.value = (mood.value + BUSK_LIFT * dt).clamp(-1.0, 1.0);
            }
        }
    }
}

/// The street photographer: point-blank portraits of strangers. The subject
/// is flattered — being seen is most of what anybody wants — except the
/// Shy, for whom this is the worst moment of the week.
#[allow(clippy::type_complexity)]
fn snap(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<AudioRng>,
    config: Res<GameConfig>,
    bank: Option<Res<crate::audio::bank::SoundBank>>,
    photographers: Query<
        (Entity, &Transform, &Archetype),
        (
            With<Pedestrian>,
            Without<Launched>,
            Without<Chatting>,
            Without<Grudge>,
            Without<Loitering>,
            Without<Composure>,
        ),
    >,
    mut subjects: Query<(Entity, &Transform, &Archetype, &mut Mood), With<Pedestrian>>,
) {
    let dt = time.delta_secs();
    for (photographer, transform, archetype) in &photographers {
        if *archetype != Archetype::Photographer || rng.random::<f32>() > SNAP_CHANCE * dt {
            continue;
        }
        let here = transform.translation;
        let Some((subject, apart)) = subjects
            .iter()
            .filter(|(entity, ..)| *entity != photographer)
            .map(|(entity, t, ..)| (entity, t.translation.distance(here)))
            .filter(|(_, apart)| *apart < SNAP_RANGE)
            .min_by(|a, b| a.1.total_cmp(&b.1))
        else {
            continue;
        };
        let _ = apart;
        let Ok((_, towards, subject_archetype, mut mood)) = subjects.get_mut(subject) else {
            continue;
        };
        let sting = if subject_archetype.shy() {
            -SNAP_INTRUSION
        } else {
            SNAP_FLATTERY
        };
        mood.value = (mood.value + sting).clamp(-1.0, 1.0);

        let face = (towards.translation - here).xz();
        commands.entity(photographer).insert((
            Loitering {
                left: SNAP_SECONDS,
                face: (face.length_squared() > f32::EPSILON).then_some(face),
            },
            Composure {
                left: rng.random_range(8.0..20.0),
            },
        ));
        if let Some(bank) = bank.as_ref() {
            commands.spawn((
                bevy::audio::AudioPlayer(bank.camera.clone()),
                crate::audio::spatial_once(
                    crate::audio::effect_gain(&config, SNAP_GAIN),
                    SNAP_EARSHOT,
                ),
                Transform::from_translation(here + Vec3::Y * 0.6),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_small_hours_are_quiet_and_the_afternoon_is_full() {
        assert!((crowd_level(15.5) - 1.0).abs() < 1e-3);
        assert!(crowd_level(3.5) < 0.4, "four a.m. should feel like it");
        for step in 0..48 {
            let level = crowd_level(step as f32 * 0.5);
            assert!(
                (0.3..=1.0).contains(&level),
                "at {:.1}h the street holds {level:.2} of its crowd",
                step as f32 * 0.5
            );
        }
    }

    #[test]
    fn rain_hurries_nobody_into_a_sprint() {
        assert_eq!(hurry(0.0), 1.0);
        assert!(
            hurry(1.0) > 1.1,
            "a downpour should visibly quicken the street"
        );
        // The crowd's speed cap times the worst hurry must stay under a flee,
        // or the rain turns every stroller into an escape artist.
        let crowd = GameConfig::default().crowd;
        let fastest = crowd.walk_speed * 1.3 * 1.5 * hurry(1.0);
        assert!(fastest < crowd.flee_speed + 1.6);
    }

    #[test]
    fn headphones_and_the_shy_are_never_buttonholed() {
        // The veto multiplies through: it must not matter how chatty the
        // other party is.
        let chatterbox = (Archetype::Missionary, AgeClass::Senior);
        for wall in [Archetype::Headphones, Archetype::Shy] {
            assert_eq!(
                chat_chance(chatterbox, (wall, AgeClass::Adult), 0.0),
                0.0,
                "{wall:?} was talked at"
            );
        }
    }

    #[test]
    fn seniors_chat_more_than_hooligans_and_rain_shuts_everybody_up() {
        let senior = chat_chance(
            (Archetype::Everyday, AgeClass::Senior),
            (Archetype::Everyday, AgeClass::Senior),
            0.0,
        );
        let hooligans = chat_chance(
            (Archetype::Hooligan, AgeClass::Adult),
            (Archetype::Hooligan, AgeClass::Adult),
            0.0,
        );
        assert!(senior > hooligans * 2.0);
        let dry = chat_chance(
            (Archetype::Everyday, AgeClass::Adult),
            (Archetype::Everyday, AgeClass::Adult),
            0.0,
        );
        let wet = chat_chance(
            (Archetype::Everyday, AgeClass::Adult),
            (Archetype::Everyday, AgeClass::Adult),
            1.0,
        );
        assert!(wet < dry * 0.5, "nobody stops for small talk in a downpour");
        assert!(wet > 0.0, "except the ones who do, occasionally");
    }

    #[test]
    fn a_rant_outdraws_a_ragemongers_recovery() {
        // The Wutbürger glaring at traffic must actually lose the argument
        // with themselves: if recovery out-pulls the sting near the baseline,
        // the whole quirk is a facing animation and no rage ever ships.
        let temper = Temperament::ragemonger();
        let near_baseline = temper.baseline - 0.05;
        let drift_back = (temper.baseline - near_baseline) * temper.recovery;
        assert!(RANT_STING > drift_back * 3.0);
    }
}
