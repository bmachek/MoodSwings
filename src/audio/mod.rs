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
            app.insert_resource(GlobalVolume::new(Volume::SILENT))
                .add_systems(Update, hush);
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

/// Throws away every sound a capture run makes, on the frame it is made.
///
/// Silencing the mixer is not enough, and the difference only shows in a run
/// long enough to accumulate: `cap_one_shots` — the choir that keeps the
/// number of live one-shots bounded — lives in [`sfx::SfxPlugin`], and that
/// plugin is exactly what a capture run does not install. So every footfall,
/// grumble, boing and taunt the crowd made was spawned and never capped, and a
/// filmed seventy seconds of Landshut ended with thirty-eight thousand sources
/// handed to a mixer that was rendering all of them at zero volume.
///
/// It went unseen for as long as a capture was one posed frame. `--film` is
/// what put a capture run on the clock for minutes at a time, and the patrol's
/// own watch caught it on the first take — which is what both of them are for.
fn hush(mut commands: Commands, fresh: Query<Entity, Added<PlaybackSettings>>) {
    for sound in &fresh {
        // Forgiving: a one-shot spawned as a child of something that is
        // despawned in the same frame — a citizen walking off the ring while
        // grumbling — is already gone by the time this runs.
        commands.entity(sound).try_despawn();
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

/// The level a sound is asking for, before the limiter has had its say.
///
/// Bevy applies [`GlobalVolume`] when a sink is *created* and never touches it
/// again — its own documented behaviour — so the master fader used to be a
/// fader with nothing on the far end of it. It moved, and everything already
/// sounding carried on at whatever level it happened to start at: a bed that
/// began under a crash stayed ducked for the rest of the session, the crash
/// itself was never turned down at all, and the sum went on clipping, which is
/// the one thing the limiter exists to stop. The gain was even measured off
/// the sinks it had already discounted, so it fought its own last frame.
///
/// So [`ride_the_gain`] rides the sinks by hand, and to do that it has to know
/// what each of them *wanted*. It cannot read that back off the sink: a sink's
/// volume is the ducked one, and ducking it again every frame is a fade to
/// silence. A one-shot's wish never changes and is already on its
/// `PlaybackSettings`. A loop's changes every frame — engine load, distance,
/// how hard a geyser is blowing — and this is where its own system writes it,
/// instead of writing the sink and being overruled.
#[derive(Component, Clone, Copy, Debug)]
pub struct Level(pub f32);

/// The summed level the master fader is allowed to let through.
///
/// Above one, because summing the sinks' own volumes is a pessimistic
/// estimate of what actually comes out: the sources are uncorrelated, so
/// their peaks land in different places, and every spatial sink is attenuated
/// again by the mixer for its distance after this has read it. Set at the
/// point where a junction full of traffic is held down and a single crash
/// still lands at full force.
///
/// Read against the sum with the master fader divided out, so this is a claim
/// about the mix rather than about the setting — see [`ride_the_gain`]. An
/// ordinary Landshut street asks for about three and a half and a busy one for
/// six and a half, so the fader is doing real work most of the time: that is
/// the sum that was going out unattenuated and being heard as crackle.
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
/// in anywhere. What it reads off each of them is the level it *asked* for —
/// see [`Level`] for why that cannot be the sink's own volume, and why this
/// has to write every sink itself rather than leave it to `GlobalVolume`.
///
/// In `Ui`, after everything that sets a level, so a sink is written once per
/// frame and the write is the last word.
fn ride_the_gain(
    time: Res<Time>,
    config: Res<GameConfig>,
    mut limiter: ResMut<Limiter>,
    mut global: ResMut<GlobalVolume>,
    mut plain: Query<(
        &mut bevy::audio::AudioSink,
        &PlaybackSettings,
        Option<&Level>,
    )>,
    mut spatial: Query<(
        &mut bevy::audio::SpatialAudioSink,
        &PlaybackSettings,
        Option<&Level>,
    )>,
) {
    use bevy::audio::AudioSinkPlayback;

    /// What one sink is asking for. A loop says so every frame; a one-shot
    /// said so once, when it was spawned, and its `PlaybackSettings` still
    /// carry it.
    fn wanted(settings: &PlaybackSettings, level: Option<&Level>) -> f32 {
        level.map_or_else(|| settings.volume.to_linear(), |level| level.0)
    }

    // A muted sink is asking for nothing and must not be counted, or the
    // fader spends a quiet street holding down silence.
    let mut loud = 0.0;
    for (sink, settings, level) in &plain {
        if !sink.is_muted() {
            loud += wanted(settings, level);
        }
    }
    for (sink, settings, level) in &spatial {
        if !sink.is_muted() {
            loud += wanted(settings, level);
        }
    }

    // Measured with the master fader divided back out, so the ceiling is a
    // statement about the *mix* and not about the setting. Every level in the
    // game is master times something, so a limiter that ducked the raw sum
    // would cancel the master exactly: turning the game up would raise `loud`,
    // lower the gain by the same factor, and come out at the same loudness.
    // The slider would do nothing at all on a busy street — which is where a
    // player is most likely to reach for it.
    let master = config.audio.master.max(1e-3);
    limiter.loud = loud;
    limiter.gain = ride(
        limiter.gain,
        duck(loud / master, CEILING),
        time.delta_secs(),
    );
    let gain = limiter.gain;

    // New sinks are born ducked, so nothing spawned this frame blares for the
    // one frame before the loop below first reaches it...
    global.volume = Volume::Linear(gain);
    // ...and everything already sounding is held there. Muted sinks are written
    // too: rodio remembers a muted sink's volume, so unmuting has to land on
    // the current gain rather than on whatever was true when it went quiet.
    for (mut sink, settings, level) in &mut plain {
        sink.set_volume(Volume::Linear(wanted(settings, level) * gain));
    }
    for (mut sink, settings, level) in &mut spatial {
        sink.set_volume(Volume::Linear(wanted(settings, level) * gain));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turning the game up must make it louder, ceiling or no ceiling.
    ///
    /// Every level in the bank is the master fader times something, so the
    /// summed level moves with the fader too — and a limiter that ducked the
    /// raw sum would divide out exactly what the player just added. This pins
    /// the arithmetic that stops it: at twice the master, the same street ends
    /// up twice as loud, both settings ducking.
    #[test]
    fn the_master_fader_still_does_something_under_the_ceiling() {
        let asked = 4.0; // what a busy street wants, at master 1.0
        let out = |master: f32| {
            let loud = asked * master;
            loud * duck(loud / master, CEILING)
        };
        assert!(
            out(1.0) > CEILING - 1e-4,
            "a full master is below the ceiling"
        );
        assert!(
            (out(2.0) / out(1.0) - 2.0).abs() < 1e-4,
            "twice the master came out {} times as loud",
            out(2.0) / out(1.0)
        );
        assert!((out(0.5) / out(1.0) - 0.5).abs() < 1e-4);
    }

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
