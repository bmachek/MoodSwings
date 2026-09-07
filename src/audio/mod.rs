//! Audio.
//!
//! Every sound in the game is a recording loaded into a buffer at startup —
//! `files` is how, `bank` is what — and this module is the wiring: it
//! registers the in-memory asset with Bevy's audio backend, puts the listener
//! on the camera, and decides how big the world sounds.

pub mod audition;
pub mod bank;
pub mod files;
pub mod sfx;
pub mod synth;

use bevy::audio::{AddAudioSource, DefaultSpatialScale, GlobalVolume, SpatialScale, Volume};
use bevy::prelude::*;
use rand_chacha::ChaCha8Rng;

use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::player::camera::CameraRig;

/// Distance, in metres, out to which a spatial sound plays at full strength.
///
/// Rodio attenuates by the inverse square of the *scaled* distance and clamps
/// at unity, so this one number is really "how big does the world sound". Too
/// small and a siren one street over is inaudible; too large and every car in
/// the district is sitting in your lap.
const EARSHOT: f32 = 9.0;

/// Explosions are the one thing that should carry across a district.
pub const BLAST_EARSHOT: f32 = 55.0;

/// Random source for playback jitter: the small pitch differences that stop a
/// repeated sound turning into a metronome.
#[derive(Resource, Deref, DerefMut)]
pub struct AudioRng(pub ChaCha8Rng);

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<synth::SynthSound>()
            .insert_resource(DefaultSpatialScale(SpatialScale::new(1.0 / EARSHOT)))
            .add_systems(Startup, build_bank)
            .add_systems(Update, attach_listener);

        // A screenshot is taken by a process nobody is listening to, and the
        // capture run is scripted rather than played.
        if crate::core::capture::is_capture_mode() {
            app.insert_resource(GlobalVolume::new(Volume::SILENT));
            return;
        }

        app.init_resource::<Limiter>()
            .add_systems(
                Update,
                // In `Ui`, which is the one set that runs after everything
                // that sets a level — and, unlike the gameplay sets, keeps
                // running while the pause menu is open.
                ride_the_gain.in_set(crate::core::schedule::GameSet::Ui),
            )
            .add_plugins(sfx::SfxPlugin);
    }
}

fn build_bank(mut commands: Commands, mut sounds: ResMut<Assets<synth::SynthSound>>) {
    let started = std::time::Instant::now();
    let bank = bank::build(&mut sounds);
    commands.insert_resource(bank);
    commands.insert_resource(AudioRng(stream_for(0, stream::AUDIO)));
    info!(
        "sound bank loaded in {:.1}ms",
        started.elapsed().as_secs_f32() * 1000.0
    );
}

/// The camera is where the player's ears are, so it carries the listener.
fn attach_listener(
    mut commands: Commands,
    cameras: Query<Entity, (With<CameraRig>, Without<bevy::audio::SpatialListener>)>,
) {
    for camera in &cameras {
        // Roughly a head across. Rodio pans on which ear is nearer, so the gap
        // only has to be non-zero and honest about the scale of the world.
        commands
            .entity(camera)
            .insert(bevy::audio::SpatialListener::new(0.25));
    }
}

/// Playback for a one-shot heard at a place in the world.
pub fn spatial_once(volume: f32, earshot: f32) -> PlaybackSettings {
    PlaybackSettings::DESPAWN
        .with_volume(Volume::Linear(volume))
        .with_spatial(true)
        .with_spatial_scale(SpatialScale::new(1.0 / earshot))
}

/// Playback for a one-shot that happens to the player rather than near them —
/// their own weapon, their own feet. Positioning those would only make the
/// player's own actions quieter when they turn their head.
pub fn close_once(volume: f32) -> PlaybackSettings {
    PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume))
}

/// Effect volume after the mixer settings, for a sound with the given gain.
pub fn effect_gain(config: &GameConfig, gain: f32) -> f32 {
    config.audio.master * config.audio.effects * gain
}

/// The summed level the master fader is allowed to let through.
///
/// Above one, because summing the sinks' own volumes is a pessimistic
/// estimate of what actually comes out: the sources are uncorrelated, so
/// their peaks land in different places, and every spatial sink is attenuated
/// again by the mixer for its distance after this has read it. Set at the
/// point where a junction full of traffic is held down and a single crash
/// still lands at full force.
const CEILING: f32 = 1.5;

/// Master gain, kept moving by [`ride_the_gain`].
///
/// Both numbers are on the dev panel, because "is it still too much at once?"
/// is a question about a moment on a particular street and cannot be answered
/// by reading the gain tables.
#[derive(Resource)]
pub struct Limiter {
    /// Summed level of everything audible, as of the last frame.
    pub loud: f32,
    /// And the master gain that is holding it down.
    pub gain: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        // Opens fully and is pulled down from there, so the first frame of a
        // quiet street is not a fade-in.
        Self {
            loud: 0.0,
            gain: 1.0,
        }
    }
}

/// The ceiling the panel reads back, so it can say how close the mix is.
pub const fn ceiling() -> f32 {
    CEILING
}

/// The gain that holds a summed level down to the ceiling.
///
/// Pure, because the whole argument about this is "what happens when eight
/// things are loud at once", and that is an argument to have in a test rather
/// than by standing on a street corner.
pub fn duck(loud: f32, ceiling: f32) -> f32 {
    if loud <= ceiling || loud <= 0.0 {
        1.0
    } else {
        ceiling / loud
    }
}

/// One frame of the fader moving towards where it should be.
///
/// Asymmetric on purpose, the way every compressor is: down fast enough to
/// already be there when the crash lands, up slowly enough that a street of
/// footsteps does not make the whole city breathe in and out.
pub fn ride(current: f32, target: f32, dt: f32) -> f32 {
    const ATTACK: f32 = 0.05;
    const RELEASE: f32 = 0.9;
    let tau = if target < current { ATTACK } else { RELEASE };
    current + (target - current) * (1.0 - (-dt / tau).exp())
}

/// The limiter rodio does not have.
///
/// Rodio sums every audible source and hands the result to the device with
/// nothing standing between them, so the moment the sum passes full scale it
/// clips — and clipping is heard as crackle and grit, not as loudness. No
/// amount of tuning the per-sound gains prevents it, because what overflows
/// is not any one sound but *how many* of them a street happens to be doing
/// at the same second, and that is a property of the city rather than of the
/// bank. The choirs in `sfx` bound each category; this bounds the total.
///
/// Reads the sinks rather than being told by the systems that set them: they
/// are the one place every loop, one-shot, bed and emitter is guaranteed to
/// turn up, whoever spawned it and whether or not they remembered to book it
/// in anywhere.
fn ride_the_gain(
    time: Res<Time>,
    mut limiter: ResMut<Limiter>,
    mut global: ResMut<GlobalVolume>,
    plain: Query<&bevy::audio::AudioSink>,
    spatial: Query<&bevy::audio::SpatialAudioSink>,
) {
    use bevy::audio::AudioSinkPlayback;

    let mut loud = 0.0;
    for sink in &plain {
        if !sink.is_muted() {
            loud += sink.volume().to_linear();
        }
    }
    for sink in &spatial {
        if !sink.is_muted() {
            loud += sink.volume().to_linear();
        }
    }
    limiter.loud = loud;
    limiter.gain = ride(limiter.gain, duck(loud, CEILING), time.delta_secs());
    global.volume = Volume::Linear(limiter.gain);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_street_is_not_turned_down_at_all() {
        assert_eq!(duck(0.0, CEILING), 1.0);
        assert_eq!(duck(CEILING, CEILING), 1.0);
    }

    #[test]
    fn a_sum_over_the_ceiling_comes_back_to_the_ceiling() {
        for loud in [1.6f32, 3.0, 12.0] {
            let gain = duck(loud, CEILING);
            assert!(
                (loud * gain - CEILING).abs() < 1e-4,
                "{loud} ducked to {}",
                loud * gain
            );
        }
    }

    #[test]
    fn the_fader_falls_faster_than_it_rises() {
        // The whole point of an asymmetric envelope: a crash must be caught
        // before it clips, and let go of slowly enough that nobody hears the
        // letting go.
        let dt = 1.0 / 60.0;
        let down = 1.0 - ride(1.0, 0.4, dt);
        let up = ride(0.4, 1.0, dt) - 0.4;
        assert!(
            down > up * 4.0,
            "attack moved {down:.4} where release moved {up:.4}"
        );
    }

    #[test]
    fn the_fader_arrives_and_stays() {
        let mut gain = 1.0;
        for _ in 0..600 {
            gain = ride(gain, 0.3, 1.0 / 60.0);
        }
        assert!((gain - 0.3).abs() < 1e-3, "settled at {gain}");
    }
}
