//! The sample buffer the bank is built of, and the load-time discipline.
//!
//! This module used to be a synthesiser — oscillators, resonators, formants,
//! the lot — and the bank used to be written in it. The synthesis is gone (the
//! recordings won, see `audio::bank`), and what remains is the part that was
//! never about synthesis at all: [`SynthSound`], the in-memory audio asset
//! every recording is loaded into, and the three functions that hold any
//! buffer to the bank's rules — [`fade_edges`], [`normalize`] and
//! [`wrap_seam`]. The name stays because the asset type is registered under it
//! and half the codebase says `SynthSound`; renaming it would be a big diff
//! about nothing.
//!
//! The decoder deliberately holds an `Arc` of the samples rather than a `Vec`.
//! Every sink that starts playing calls `decoder()`, and forty cars sharing
//! one engine should share one buffer, not copy a second of audio each.

use std::sync::Arc;
use std::time::Duration;

use bevy::audio::{ChannelCount, Decodable, Sample, SampleRate, Source};
use bevy::prelude::*;

pub const SAMPLE_RATE: u32 = 44_100;

const RATE: SampleRate = match SampleRate::new(SAMPLE_RATE) {
    Some(rate) => rate,
    None => unreachable!(),
};

/// Everything is mono: spatial panning is rodio's job, and a stereo source
/// cannot be positioned in the world.
const MONO: ChannelCount = match ChannelCount::new(1) {
    Some(count) => count,
    None => unreachable!(),
};

/// A block of loaded samples, playable as an audio asset.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct SynthSound {
    samples: Arc<[f32]>,
}

impl SynthSound {
    pub fn new(samples: Vec<f32>) -> Self {
        Self {
            samples: samples.into(),
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn duration(&self) -> Duration {
        Duration::from_secs_f32(self.samples.len() as f32 / SAMPLE_RATE as f32)
    }
}

/// Plays one [`SynthSound`] once, from the start.
pub struct SynthDecoder {
    samples: Arc<[f32]>,
    position: usize,
}

impl Iterator for SynthDecoder {
    type Item = Sample;

    #[inline]
    fn next(&mut self) -> Option<Sample> {
        let sample = *self.samples.get(self.position)?;
        self.position += 1;
        Some(sample)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.samples.len() - self.position;
        (left, Some(left))
    }
}

impl ExactSizeIterator for SynthDecoder {}

impl Source for SynthDecoder {
    fn current_span_len(&self) -> Option<usize> {
        // Rodio's contract: `Some(0)` exactly when the source is spent.
        if self.position >= self.samples.len() {
            Some(0)
        } else {
            Some(self.samples.len())
        }
    }

    fn channels(&self) -> ChannelCount {
        MONO
    }

    fn sample_rate(&self) -> SampleRate {
        RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(
            self.samples.len() as f32 / SAMPLE_RATE as f32,
        ))
    }
}

impl Decodable for SynthSound {
    type Decoder = SynthDecoder;

    fn decoder(&self) -> Self::Decoder {
        SynthDecoder {
            samples: self.samples.clone(),
            position: 0,
        }
    }
}

// ----------------------------------------------------------- discipline ----

/// Samples in `seconds` of audio.
pub fn samples(seconds: f32) -> usize {
    (seconds * SAMPLE_RATE as f32).round().max(1.0) as usize
}

/// Folds a buffer's surplus tail back over its head so it loops without a click.
///
/// `samples` must be `fade` longer than the loop you want; the result is that
/// shorter. The last sample of the result and the first are adjacent in the
/// original buffer, so the join is continuous by construction. This is what
/// makes an arbitrary field recording loopable: noise can never be made
/// periodic on the cheap, but it can be crossfaded into itself.
pub fn wrap_seam(mut samples: Vec<f32>, fade: usize) -> Vec<f32> {
    let length = samples.len().saturating_sub(fade);
    if length == 0 || fade == 0 {
        return samples;
    }
    for index in 0..fade {
        let blend = index as f32 / fade as f32;
        samples[index] = samples[index] * blend + samples[length + index] * (1.0 - blend);
    }
    samples.truncate(length);
    samples
}

/// Ramps the first and last few milliseconds to zero, so starting or stopping
/// a one-shot does not click.
pub fn fade_edges(samples: &mut [f32], seconds: f32) {
    let edge = self::samples(seconds).min(samples.len() / 2);
    if edge == 0 {
        return;
    }
    let length = samples.len();
    for index in 0..edge {
        let blend = index as f32 / edge as f32;
        samples[index] *= blend;
        samples[length - 1 - index] *= blend;
    }
}

/// Scales a buffer so its loudest sample sits at `peak`.
///
/// Every recording arrives at whatever loudness its uploader mastered it to;
/// normalising here means the mix is set by one number per sound in the
/// bank's register instead of by accident.
pub fn normalize(samples: &mut [f32], peak: f32) {
    let loudest = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if loudest <= f32::EPSILON {
        return;
    }
    let scale = peak / loudest;
    for sample in samples {
        *sample *= scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(sound: &SynthSound) -> Vec<f32> {
        sound.decoder().collect()
    }

    /// Deterministic hash noise, standing in for the filtered noise the old
    /// synthesiser made: the seam test needs material with no period in it.
    fn noise(length: usize) -> Vec<f32> {
        (0..length)
            .map(|i| {
                let mut h = (i as u32).wrapping_mul(0x9E37_79B1);
                h ^= h >> 15;
                h = h.wrapping_mul(0x2545_F491);
                (h as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn a_decoder_replays_the_whole_buffer_and_then_reports_empty() {
        let sound = SynthSound::new(vec![0.25, -0.5, 0.75]);
        assert_eq!(decode(&sound), vec![0.25, -0.5, 0.75]);

        let mut decoder = sound.decoder();
        assert_ne!(decoder.current_span_len(), Some(0));
        for _ in 0..3 {
            decoder.next();
        }
        // Rodio uses this to decide the sink is done; get it wrong and one-shot
        // sounds never despawn.
        assert_eq!(decoder.current_span_len(), Some(0));
        assert_eq!(decoder.next(), None);
    }

    #[test]
    fn two_decoders_of_one_sound_do_not_copy_the_samples() {
        let sound = SynthSound::new(vec![0.0; 1024]);
        let first = sound.decoder();
        let second = sound.decoder();
        assert!(
            Arc::ptr_eq(&first.samples, &second.samples),
            "every sink playing an engine would otherwise clone a second of audio"
        );
    }

    #[test]
    fn wrapping_joins_a_noise_loop_at_an_adjacent_pair() {
        let raw = noise(4_096);
        let wrapped = wrap_seam(raw.clone(), 1_024);
        assert_eq!(wrapped.len(), 3_072);

        // The guarantee: the loop's last sample and its first are neighbours in
        // the source, so playing round the join is playing the source forwards.
        assert_eq!(wrapped[wrapped.len() - 1], raw[3_071]);
        assert_eq!(wrapped[0], raw[3_072]);
    }

    #[test]
    fn fading_pins_both_edges_to_zero() {
        let mut buffer = vec![1.0; samples(0.5)];
        fade_edges(&mut buffer, 0.004);
        assert_eq!(buffer[0], 0.0);
        assert!(buffer[buffer.len() - 1].abs() < 1e-3);
        assert_eq!(buffer[buffer.len() / 2], 1.0, "the middle is untouched");
    }

    #[test]
    fn normalising_lands_exactly_on_the_requested_peak() {
        let mut buffer = vec![0.1, -0.4, 0.2];
        normalize(&mut buffer, 0.8);
        assert!((buffer.iter().fold(0.0f32, |m, s| m.max(s.abs())) - 0.8).abs() < 1e-6);

        // Silence must not turn into a division by zero.
        let mut silent = vec![0.0; 4];
        normalize(&mut silent, 0.8);
        assert!(silent.iter().all(|s| *s == 0.0));
    }
}
