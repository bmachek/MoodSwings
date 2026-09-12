//! Appearance is independent of temperament and occupation. A stable resident
//! keeps their complexion, build and clothes when their streamed body returns.

use bevy::prelude::*;
use rand::RngExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    Masculine,
    Feminine,
    Androgynous,
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    pub presentation: Presentation,
    pub skin: usize,
    pub face: usize,
    pub hair: usize,
    pub hairstyle: usize,
    pub outfit: usize,
    pub trousers: usize,
    pub shoulders: f32,
    pub hips: f32,
    pub depth: f32,
    pub glasses: bool,
}

impl Appearance {
    pub fn from_seed(seed: u64) -> Self {
        let mut rng = crate::core::rng::stream_for(seed, crate::core::rng::stream::APPEARANCE);
        let presentation = match rng.random_range(0..3) {
            0 => Presentation::Masculine,
            1 => Presentation::Feminine,
            _ => Presentation::Androgynous,
        };
        let build = rng.random_range(0.90..1.13);
        let (shoulders, hips) = match presentation {
            Presentation::Masculine => (1.08, 0.97),
            Presentation::Feminine => (0.91, 1.08),
            Presentation::Androgynous => (1.0, 1.02),
        };
        Self {
            presentation,
            skin: rng.random_range(0..6),
            face: rng.random_range(0..3),
            hair: rng.random_range(0..6),
            hairstyle: rng.random_range(0..5),
            outfit: rng.random_range(0..4),
            trousers: rng.random_range(0..3),
            shoulders: shoulders * build,
            hips: hips * build,
            depth: rng.random_range(0.88..1.18),
            glasses: rng.random::<f32>() < 0.22,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_returning_resident_keeps_their_appearance() {
        for id in 0..200 {
            assert_eq!(Appearance::from_seed(id), Appearance::from_seed(id));
        }
    }

    #[test]
    fn the_crowd_has_every_presentation_complexion_and_outfit() {
        let people: Vec<_> = (0..600).map(Appearance::from_seed).collect();
        for look in [
            Presentation::Masculine,
            Presentation::Feminine,
            Presentation::Androgynous,
        ] {
            assert!(people.iter().any(|p| p.presentation == look));
        }
        for skin in 0..6 {
            assert!(people.iter().any(|p| p.skin == skin));
        }
        for outfit in 0..4 {
            assert!(people.iter().any(|p| p.outfit == outfit));
        }
        assert!(people.iter().all(|p| p.shoulders < 1.23 && p.hips < 1.23));
    }
}
