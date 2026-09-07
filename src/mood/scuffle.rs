//! Grabbing somebody by the collar and shaking them.
//!
//! A grudge used to end one way: the ram. That is the right ending when the
//! score is settled at speed, but two citizens who are merely *arguing* — a
//! pursuer arriving at a walking pace, on a neighbour who is also cross —
//! deserve the older piece of street theatre: one grabs the other by the
//! collar and shakes them until both have had enough, and then they shove
//! apart and cartwheel backwards. Nobody is hurt, because nobody here ever
//! is; both leave angrier than they arrived, which is what an argument does.
//!
//! The mechanics reuse everything the city already has. The shake is a
//! velocity the bounce controller steers to (never a written position, which
//! would fight the solver), the arms are a [`Posture`] the figure system
//! poses, the parting shove goes through [`launch`] like every other exit —
//! and because both parties leave `Launched`, the blame system ignores the
//! shove and the feud does not restart itself on the spot. The wallop of the
//! parting is real, though: both moods take it as the insult it is, and the
//! street turns round to watch, through exactly the systems that already
//! handle being hit by a car.
//!
//! Who gets grabbed and who gets rammed is decided where the grudge lands —
//! see `grudge::settle_scores`: the player and the steadfast are rammed (a
//! camera must not be shaken and a wheelchair must not be grabbed), citizens
//! are collared.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;

use super::feeling::Mood;
use crate::ai::figure::Posture;
use crate::audio::bank::{SoundBank, VARIANTS};
use crate::audio::{AudioRng, effect_gain, spatial_once};
use crate::bounce::controller::Bouncer;
use crate::bounce::launch::{THROW_UP, launch};
use crate::core::schedule::GameSet;
use crate::mood::voice::Voicebox;

/// How long the shaking goes on before both have had enough.
pub const SCUFFLE_SECONDS: f32 = 2.6;
/// Collar distance: the grip closes to this and holds it.
const GRIP: f32 = 0.75;
/// The shake, as the sideways velocity the victim is steered to. 3.5 Hz at
/// this speed stays within `ground_accel`'s authority, so the judder is real
/// motion rather than an asked-for motion the steering never reaches.
const SHAKE_HZ: f32 = 3.5;
const SHAKE_SPEED: f32 = 1.8;
/// The parting shove, in m/s along the line between them. Deliberately past
/// `MoodConfig::bop_limit`: the end of a fight must land as an insult on
/// both, not as a friendly bop that cheers everybody up.
const SHOVE: f32 = 7.0;
/// Mood lost per second of being in the argument. Both sides pay it.
const SOUR: f32 = 0.10;
/// How often somebody says what they think of the other one, in seconds.
const CURSE_EVERY: f32 = 1.1;
const CURSE_GAIN: f32 = 0.7;
const CURSE_EARSHOT: f32 = 24.0;

/// One half of a collar-grabbing. Both parties wear one, pointed at each
/// other, each ticking its own copy of the same clock — so a partner
/// streaming out mid-shake strands nobody.
#[derive(Component, Debug)]
pub struct Scuffle {
    pub with: Entity,
    pub left: f32,
    /// Whether this one holds the collar or hangs from it. The grabber
    /// plants their feet and drives the shake; the victim takes it.
    pub grabber: bool,
    /// Countdown to the next curse.
    mutter: f32,
}

impl Scuffle {
    pub fn new(with: Entity, grabber: bool) -> Self {
        Self {
            with,
            left: SCUFFLE_SECONDS,
            grabber,
            // The grabber opens the argument, the victim answers: staggered
            // so the two curses never land as one garbled chord.
            mutter: if grabber { 0.2 } else { 0.75 },
        }
    }
}

pub struct ScufflePlugin;

impl Plugin for ScufflePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            shake
                .in_set(GameSet::Ai)
                // An override of the walking intent, like the grudge pursuit
                // that starts it: it must write `Bouncer::desired` after the
                // pavement AI does.
                .after(crate::ai::pedestrian::Walking),
        );
    }
}

/// The sideways speed the shaken body is steered to at `seconds` into the
/// scuffle. A pure function so the judder can be argued about in a test:
/// it must oscillate fast enough to read as shaking and slow enough that
/// `ground_accel` can actually follow it.
pub fn shake_speed(seconds: f32) -> f32 {
    (seconds * SHAKE_HZ * std::f32::consts::TAU).sin() * SHAKE_SPEED
}

/// Runs both halves of every scuffle: grip, judder, cursing, souring, and
/// the parting shove.
///
/// One query, iterated twice — read snapshot then write pass — because the
/// partner's `Transform` lives in the very query being mutated; the same
/// shape as `feeling::spread_moods`, for the same schedule-trap reason.
fn shake(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<crate::core::config::GameConfig>,
    bank: Option<Res<SoundBank>>,
    mut rng: ResMut<AudioRng>,
    mut scufflers: Query<(
        Entity,
        &mut Transform,
        &mut LinearVelocity,
        &mut Bouncer,
        &mut Mood,
        &Voicebox,
        &mut Scuffle,
    )>,
) {
    let dt = time.delta_secs();
    let elapsed = time.elapsed_secs();
    let others: Vec<(Entity, Vec3)> = scufflers
        .iter()
        .map(|(entity, transform, ..)| (entity, transform.translation))
        .collect();
    let where_is = |who: Entity| others.iter().find(|(e, _)| *e == who).map(|(_, at)| *at);

    for (entity, mut transform, mut velocity, mut bouncer, mut mood, voice, mut scuffle) in
        &mut scufflers
    {
        scuffle.left -= dt;
        let partner = where_is(scuffle.with);

        // Over — either the clock ran out or the partner streamed away. The
        // shove goes through `launch` like every exit in this city, and both
        // leave `Launched`, which is what keeps the blame system out of it.
        if scuffle.left <= 0.0 || partner.is_none() {
            commands.entity(entity).remove::<(Scuffle, Posture)>();
            if let Some(there) = partner {
                let apart = (transform.translation - there).with_y(0.0);
                let away = apart.normalize_or_zero();
                launch(
                    &mut commands,
                    entity,
                    &mut velocity,
                    away * SHOVE + Vec3::Y * THROW_UP,
                    true,
                );
            }
            continue;
        }
        let there = partner.unwrap();
        let apart = (there - transform.translation).with_y(0.0);

        // Face the argument. Nothing else will turn a stopped body.
        if let Ok(facing) = Dir2::new(apart.xz()) {
            transform.rotation =
                Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
        }

        // The grip and the judder. Sideways is perpendicular to the line
        // between them; the victim is thrown along it and the grabber sways
        // against it, half as hard — holding on costs footing too.
        let lateral = Vec2::new(-apart.z, apart.x).normalize_or_zero();
        let judder = shake_speed(elapsed);
        let hold = if apart.length() > GRIP {
            apart.normalize_or_zero().xz() * 1.2
        } else {
            Vec2::ZERO
        };
        bouncer.desired = if scuffle.grabber {
            hold - lateral * judder * 0.5
        } else {
            hold + lateral * judder
        };

        // Arguing sours both sides, and both say so, in their own voice.
        mood.value = (mood.value - SOUR * dt).clamp(-1.0, 1.0);
        scuffle.mutter -= dt;
        if scuffle.mutter <= 0.0 {
            scuffle.mutter = CURSE_EVERY * rng.random_range(0.8..1.3);
            if let Some(bank) = bank.as_deref() {
                let take = rng.random_range(0..VARIANTS);
                commands.spawn((
                    AudioPlayer(bank.curse[take].clone()),
                    spatial_once(effect_gain(&config, CURSE_GAIN), CURSE_EARSHOT)
                        .with_speed(voice.pitch),
                    Transform::from_translation(transform.translation + Vec3::Y * 0.6),
                ));
            }
        }
    }
}

/// Whether a settled grudge becomes a collar-grab rather than a ram.
///
/// The player is rammed (the camera is bolted to them; a shaken camera is
/// seasickness), the steadfast are rammed (a wheelchair is punted, never
/// grabbed), somebody already flying cannot be caught, and somebody already
/// in a scuffle has no collar free.
pub fn grabbable(is_player: bool, steadfast: bool, launched: bool, scuffling: bool) -> bool {
    !is_player && !steadfast && !launched && !scuffling
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shake_is_a_shake_the_steering_can_actually_follow() {
        // The judder is only real if `ground_accel` can chase it: the peak
        // demanded acceleration of a sine at f Hz and amplitude v is TAU·f·v.
        let demanded = std::f32::consts::TAU * SHAKE_HZ * SHAKE_SPEED;
        let available = crate::core::config::GameConfig::default()
            .bounce
            .ground_accel;
        assert!(
            demanded < available,
            "the shake asks for {demanded:.0} m/s² against {available:.0} available"
        );
        // And it must actually oscillate — both signs inside one period.
        let period = 1.0 / SHAKE_HZ;
        assert!(shake_speed(period * 0.25) > 1.0);
        assert!(shake_speed(period * 0.75) < -1.0);
    }

    #[test]
    fn the_parting_shove_lands_as_an_insult_rather_than_a_bop() {
        // The shove plus the upward part must clear the bop limit, or the
        // end of every fight cheers both parties up and arguments become a
        // wellness programme.
        let magnitude = (SHOVE * SHOVE + THROW_UP * THROW_UP).sqrt();
        let tune = crate::core::config::GameConfig::default().mood;
        assert!(magnitude > tune.bop_limit + 1.0);
        // But not so hard that a domestic reads like a traffic accident.
        assert!(magnitude < tune.outrage_limit);
    }

    #[test]
    fn only_ordinary_citizens_get_collared() {
        assert!(grabbable(false, false, false, false));
        assert!(
            !grabbable(true, false, false, false),
            "the camera is on the player"
        );
        assert!(
            !grabbable(false, true, false, false),
            "wheelchairs are punted, not grabbed"
        );
        assert!(
            !grabbable(false, false, true, false),
            "cannot catch somebody mid-flight"
        );
        assert!(
            !grabbable(false, false, false, true),
            "one collar per customer"
        );
    }
}
