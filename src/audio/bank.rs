//! Every sound the game can make, loaded from recordings at startup.
//!
//! The bank used to be synthesised — every sound a little DSP program, built
//! from oscillators and filters at boot. That era is over by decision, not by
//! accident: the recordings simply sound better, and once every slot had one
//! the synthesis was eight hundred lines of fallback for a case nobody wanted
//! to hear. `tools/fetch-materials.sh` is therefore a *required* setup step
//! now; a clone that has not run it still starts, but it plays silence where
//! the missing sounds should be and says so loudly in the log, once per gap.
//!
//! What survived the cut is the discipline. Every recording is pushed through
//! the same rules the synthesised bank was tested to — mono, resampled, edges
//! faded, peak normalised, loops seam-wrapped — by `audio::files`, at load.
//! The [`REGISTER`] below is the single list of what the bank contains and how
//! loud each entry is allowed to be; the loader, the audition tool and the
//! fetch-script sync tests all read it, so a sound cannot be added to one of
//! them and forgotten by the others.

use bevy::prelude::*;

use super::files;
use super::synth::{SynthSound, samples};

/// Every sound, loaded once and shared by everything that plays it.
#[derive(Resource)]
pub struct SoundBank {
    pub boing: Handle<SynthSound>,
    /// Nothing triggers this since vehicles stopped being wreckable; it stays
    /// in the bank, auditioned, for the planned world-damage milestone —
    /// things in this city may yet go bang, just not people.
    pub explosion: Handle<SynthSound>,
    pub crash: Handle<SynthSound>,
    pub honk: Handle<SynthSound>,
    pub wheee: Handle<SynthSound>,
    pub sproing: Handle<SynthSound>,
    pub spray: Handle<SynthSound>,
    pub footstep: Handle<SynthSound>,
    pub car_door: Handle<SynthSound>,
    pub engine: Handle<SynthSound>,
    pub screech: Handle<SynthSound>,
    pub ambience: Handle<SynthSound>,
    pub birdsong: Handle<SynthSound>,
    pub uproar: Handle<SynthSound>,

    // --- voices ---
    //
    // Several of each where one would be recognised as a repeat. A flummi
    // saying the same noise every time it is annoyed stops being a citizen
    // and becomes a doorbell, and playback pitches these further apart again
    // per speaker (`Voicebox::pitch`), which is also what keeps a recorded
    // human take from reading as the same human twice.
    pub whistle: [Handle<SynthSound>; VARIANTS],
    pub giggle: Handle<SynthSound>,
    pub grumble: [Handle<SynthSound>; VARIANTS],
    pub curse: [Handle<SynthSound>; VARIANTS],
    /// The taunt rotation: raspberry, fart, cough, spit — and now the burp,
    /// which sat unclaimed in `assets/sounds/` until it earned its slot. The
    /// game's whole verb deserves a repertoire.
    pub raspberry: Handle<SynthSound>,
    pub fart: Handle<SynthSound>,
    pub cough: Handle<SynthSound>,
    pub spit: Handle<SynthSound>,
    pub burp: Handle<SynthSound>,
    /// Making up: a contrite word, thrown with a flower.
    pub sorry: Handle<SynthSound>,
    pub gasp: Handle<SynthSound>,
}

/// How many takes of each spoken sound the bank holds.
pub const VARIANTS: usize = 3;

/// Firing frequency, in hertz, that the engine loop plays at unit speed.
/// Everything driving it scales from here.
pub const ENGINE_REFERENCE_HZ: f32 = 40.0;

/// Whether a bank entry is a one-shot or a bed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Shot,
    Loop,
}

/// The bank's contents: (file name in `assets/sounds/`, peak, shape).
///
/// Relative loudness across the bank is decided here, in one place, exactly
/// as it was when the peaks were handed to the synthesisers. The tests hold
/// this list against both fetch scripts, so an entry cannot exist without a
/// download to satisfy it.
pub const REGISTER: &[(&str, f32, Shape)] = &[
    ("boing", 0.85, Shape::Shot),
    ("explosion", 1.0, Shape::Shot),
    ("crash", 0.9, Shape::Shot),
    ("honk", 0.8, Shape::Shot),
    ("wheee", 0.7, Shape::Shot),
    ("sproing", 0.8, Shape::Shot),
    ("footstep", 0.55, Shape::Shot),
    ("car-door", 0.75, Shape::Shot),
    ("whistle-0", 0.55, Shape::Shot),
    ("whistle-1", 0.55, Shape::Shot),
    ("whistle-2", 0.55, Shape::Shot),
    ("giggle", 0.6, Shape::Shot),
    ("grumble-0", 0.65, Shape::Shot),
    ("grumble-1", 0.65, Shape::Shot),
    ("grumble-2", 0.65, Shape::Shot),
    ("curse-0", 0.7, Shape::Shot),
    ("curse-1", 0.7, Shape::Shot),
    ("curse-2", 0.7, Shape::Shot),
    ("raspberry", 0.85, Shape::Shot),
    ("fart", 0.85, Shape::Shot),
    ("cough", 0.8, Shape::Shot),
    ("spit", 0.7, Shape::Shot),
    ("burp", 0.8, Shape::Shot),
    ("sorry", 0.6, Shape::Shot),
    ("gasp", 0.65, Shape::Shot),
    ("spray", 0.6, Shape::Loop),
    ("engine", 0.85, Shape::Loop),
    ("screech", 0.75, Shape::Loop),
    ("ambience", 0.55, Shape::Loop),
    ("birdsong", 0.5, Shape::Loop),
    ("uproar", 0.6, Shape::Loop),
];

/// Loads one register entry, or the silence that stands in for it.
pub fn load(dir: &std::path::Path, name: &str) -> SynthSound {
    let (_, peak, shape) = REGISTER
        .iter()
        .find(|(n, ..)| *n == name)
        .unwrap_or_else(|| panic!("{name} is not in the bank's register"));
    let loaded = match shape {
        Shape::Shot => files::one_shot(dir, name, *peak),
        Shape::Loop => files::looping(dir, name, *peak),
    };
    loaded.unwrap_or_else(|| {
        // Loud on purpose. The old synthesised fallback made a missing file
        // inaudible in the truest sense; a quarter second of silence with a
        // warning next to it is a gap somebody will actually fill.
        warn!("no recording for {name} — playing silence; run tools/fetch-materials.sh");
        SynthSound::new(vec![0.0; samples(0.25)])
    })
}

pub fn build(sounds: &mut Assets<SynthSound>) -> SoundBank {
    let dir = files::dir();
    let mut add = |name: &str| sounds.add(load(&dir, name));

    SoundBank {
        boing: add("boing"),
        explosion: add("explosion"),
        crash: add("crash"),
        honk: add("honk"),
        wheee: add("wheee"),
        sproing: add("sproing"),
        spray: add("spray"),
        footstep: add("footstep"),
        car_door: add("car-door"),
        engine: add("engine"),
        screech: add("screech"),
        ambience: add("ambience"),
        birdsong: add("birdsong"),
        uproar: add("uproar"),
        whistle: std::array::from_fn(|take| add(&format!("whistle-{take}"))),
        giggle: add("giggle"),
        grumble: std::array::from_fn(|take| add(&format!("grumble-{take}"))),
        curse: std::array::from_fn(|take| add(&format!("curse-{take}"))),
        raspberry: add("raspberry"),
        fart: add("fart"),
        cough: add("cough"),
        spit: add("spit"),
        burp: add("burp"),
        sorry: add("sorry"),
        gasp: add("gasp"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::synth::SAMPLE_RATE;
    use bevy::audio::Decodable;

    #[test]
    fn register_names_are_unique() {
        let mut names: Vec<_> = REGISTER.iter().map(|(name, ..)| name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), REGISTER.len());
    }

    #[test]
    fn every_peak_is_honest() {
        for (name, peak, _) in REGISTER {
            assert!(
                (0.3..=1.0).contains(peak),
                "{name} is normalised to {peak}, outside the bank's range"
            );
        }
    }

    /// The scripts are the download half of the register, and this is the
    /// mechanical version of CLAUDE.md's keep-in-sync rule: a bank entry
    /// without a fetch entry — in *either* twin — fails the build.
    #[test]
    fn every_register_entry_has_a_fetch_entry_in_both_scripts() {
        for script in ["tools/fetch-materials.sh", "tools/fetch-materials.bat"] {
            let text =
                std::fs::read_to_string(script).expect("the fetch script is part of the repo");
            for (name, ..) in REGISTER {
                // The twins spell an entry differently: the .sh as
                // "name|url", the .bat as "fetch_sound name ext" or, for a
                // zip member, as "\name.ext".
                let present = text.contains(&format!("{name}|"))
                    || text.contains(&format!("fetch_sound {name} "))
                    || text.contains(&format!("\\{name}."));
                assert!(present, "{script} has no entry for {name}");
            }
        }
    }

    fn fixture_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("moodswings-bank-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A deliberately rude fixture: starts and ends mid-waveform, peaks well
    /// over the target. What the loader hands back must obey the rules anyway.
    fn rude_sine(seconds: f32) -> Vec<f32> {
        (0..samples(seconds))
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                (t * 220.0 * std::f32::consts::TAU + 1.0).sin() * 1.4
            })
            .collect()
    }

    #[test]
    fn a_recording_is_held_to_the_one_shot_rules_at_load() {
        let dir = fixture_dir("shot");
        std::fs::write(
            dir.join("boing.wav"),
            super::super::audition::wav(&rude_sine(0.5)),
        )
        .unwrap();

        let sound = load(&dir, "boing");
        let buffer: Vec<f32> = sound.decoder().collect();
        assert_eq!(buffer[0], 0.0, "a loaded one-shot starts mid-waveform");
        assert!(
            buffer[buffer.len() - 1].abs() < 1e-3,
            "a loaded one-shot is cut off rather than finished"
        );
        let peak = buffer.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            (0.80..=0.86).contains(&peak),
            "boing should be normalised to 0.85, not {peak}"
        );
    }

    #[test]
    fn a_recorded_loop_closes_on_itself() {
        let dir = fixture_dir("loop");
        std::fs::write(
            dir.join("engine.wav"),
            super::super::audition::wav(&rude_sine(3.0)),
        )
        .unwrap();

        let buffer: Vec<f32> = load(&dir, "engine").decoder().collect();
        let inside = buffer
            .windows(2)
            .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        let seam = (buffer[0] - buffer[buffer.len() - 1]).abs();
        assert!(
            seam <= inside * 1.5,
            "the loop jumps {seam} at the seam, against {inside} within it"
        );
    }

    #[test]
    fn a_missing_recording_is_a_short_silence_rather_than_a_panic() {
        let sound = load(std::path::Path::new("/definitely/not/a/directory"), "gasp");
        let buffer: Vec<f32> = sound.decoder().collect();
        assert!(!buffer.is_empty());
        assert!(buffer.iter().all(|s| *s == 0.0));
    }
}
