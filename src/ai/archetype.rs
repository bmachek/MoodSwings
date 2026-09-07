//! Who a citizen is, beyond how they feel.
//!
//! The `Tempers` table answers one question — how quickly does this flummi
//! turn red — and this module answers everything else the pavement can read:
//! what they wear, what sits on their head, how fast they amble, whether they
//! can hear you at all. The two compose rather than compete: an archetype may
//! *fix* the temperament (a Wutbürger is always a ragemonger), but it never
//! invents a new one, so every invariant the five temperaments carry keeps
//! holding.
//!
//! Drawn from `stream::CROWD`, its own stream, for the same reason tempers
//! have theirs: retuning the cast's shares must move neither anybody's spawn
//! point (`PEDESTRIANS`) nor anybody's disposition (`MOOD`). To keep those
//! streams independent the spawn code still *consumes* its temper and
//! wardrobe draws for every citizen and only then overrides them — a fixed
//! answer is not a skipped question.

use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::mood::feeling::Temperament;

/// The cast. `Everyday` is the crowd the rest stand out against.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum Archetype {
    #[default]
    Everyday,
    Wutbuerger,
    Punk,
    Elvis,
    /// Wearing headphones, and therefore — the joke writes its own mechanic —
    /// immune to taunts and cheers alike. Bops still land; you cannot wear
    /// headphones against a shove.
    Headphones,
    Beggar,
    Shy,
    /// They walk in a saffron line and cheer at everybody. The only gang in
    /// the city whose drive-by raises your mood.
    Missionary,
}

impl Archetype {
    pub const ALL: [Archetype; 8] = [
        Archetype::Everyday,
        Archetype::Wutbuerger,
        Archetype::Punk,
        Archetype::Elvis,
        Archetype::Headphones,
        Archetype::Beggar,
        Archetype::Shy,
        Archetype::Missionary,
    ];

    /// The name the character menu shows. Player-facing, so German.
    pub fn label(self) -> &'static str {
        match self {
            Archetype::Everyday => "Bürger",
            Archetype::Wutbuerger => "Wutbürger",
            Archetype::Punk => "Punk",
            Archetype::Elvis => "Elvis",
            Archetype::Headphones => "Kopfhörerträger",
            Archetype::Beggar => "Bettler",
            Archetype::Shy => "Schüchterne",
            Archetype::Missionary => "Missionar",
        }
    }

    /// A disposition the archetype fixes, or `None` to draw from the mix.
    pub fn temper(self) -> Option<Temperament> {
        match self {
            Archetype::Wutbuerger => Some(Temperament::ragemonger()),
            Archetype::Shy => Some(Temperament::serene()),
            Archetype::Missionary => Some(Temperament::easygoing()),
            _ => None,
        }
    }

    /// A coat the archetype always wears, or `None` for the street palette.
    pub fn coat(self) -> Option<Color> {
        match self {
            // Red enough to see coming, which is the point: the face says how
            // he feels, the coat says how he is going to feel.
            Archetype::Wutbuerger => Some(Color::srgb(0.58, 0.15, 0.11)),
            Archetype::Punk => Some(Color::srgb(0.10, 0.09, 0.10)),
            Archetype::Elvis => Some(Color::srgb(0.92, 0.90, 0.84)),
            Archetype::Beggar => Some(Color::srgb(0.33, 0.28, 0.21)),
            Archetype::Shy => Some(Color::srgb(0.56, 0.58, 0.61)),
            Archetype::Missionary => Some(Color::srgb(0.87, 0.49, 0.10)),
            _ => None,
        }
    }

    /// Multiplier on the walking pace, over whatever the mood does to it.
    pub fn pace(self) -> f32 {
        match self {
            Archetype::Beggar => 0.6,
            Archetype::Shy => 0.85,
            Archetype::Missionary => 0.9,
            _ => 1.0,
        }
    }

    /// Cannot hear a taunt or a cheer.
    pub fn deaf(self) -> bool {
        self == Archetype::Headphones
    }

    /// Keeps a wide berth around the player.
    pub fn shy(self) -> bool {
        self == Archetype::Shy
    }

    /// No hair cap: a shaved head is most of the costume.
    pub fn bald(self) -> bool {
        self == Archetype::Missionary
    }

    /// Cheers spontaneously whatever the mood, rather than only in delight.
    pub fn cheer_spam(self) -> bool {
        self == Archetype::Missionary
    }

    /// How many of them arrive together.
    pub fn group_size(self) -> usize {
        match self {
            Archetype::Missionary => 4,
            _ => 1,
        }
    }
}

/// The mix, as (archetype, share). A resource for the same reason `Tempers`
/// is: the only way to find out how many Elvises a city can support is to
/// drag a slider and watch the street.
#[derive(Resource, Clone, Debug)]
pub struct Cast(pub Vec<(Archetype, f32)>);

impl Default for Cast {
    fn default() -> Self {
        Self(vec![
            (Archetype::Everyday, 0.55),
            (Archetype::Wutbuerger, 0.07),
            (Archetype::Punk, 0.07),
            // One in fifty. An Elvis is a sighting, not a demographic.
            (Archetype::Elvis, 0.02),
            (Archetype::Headphones, 0.08),
            (Archetype::Beggar, 0.05),
            (Archetype::Shy, 0.10),
            (Archetype::Missionary, 0.06),
        ])
    }
}

impl Cast {
    /// Draws who somebody is. The same ticket walk as `Tempers::draw`.
    pub fn draw(&self, rng: &mut ChaCha8Rng) -> Archetype {
        let total: f32 = self.0.iter().map(|(_, share)| share.max(0.0)).sum();
        if total <= 0.0 {
            return Archetype::Everyday;
        }
        let mut ticket = rng.random_range(0.0..total);
        for (archetype, share) in &self.0 {
            ticket -= share.max(0.0);
            if ticket <= 0.0 {
                return *archetype;
            }
        }
        Archetype::Everyday
    }
}

/// The crowd's own stream — see the module doc for why it is neither
/// `PEDESTRIANS` nor `MOOD`.
#[derive(Resource)]
pub struct CrowdRng(pub ChaCha8Rng);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::rng::{stream, stream_for};

    #[test]
    fn wutbuerger_is_always_a_ragemonger() {
        let temper = Archetype::Wutbuerger.temper().unwrap();
        assert_eq!(temper.name(), "ragemonger");
    }

    #[test]
    fn fixed_tempers_answer_to_existing_names() {
        // Archetypes select among the five, never mint a sixth: every fixed
        // temper must round-trip through the name the fuse implies.
        for archetype in Archetype::ALL {
            if let Some(temper) = archetype.temper() {
                assert!(
                    ["serene", "easygoing", "ordinary", "touchy", "ragemonger"]
                        .contains(&temper.name()),
                    "{archetype:?} invented a temperament"
                );
            }
        }
    }

    #[test]
    fn everybody_in_the_cast_turns_up_eventually() {
        let mut rng = stream_for(3, stream::CROWD);
        let cast = Cast::default();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..3000 {
            seen.insert(cast.draw(&mut rng));
        }
        assert_eq!(seen.len(), Archetype::ALL.len());
    }

    #[test]
    fn the_default_cast_is_mostly_everyday() {
        let cast = Cast::default();
        let everyday = cast
            .0
            .iter()
            .find(|(a, _)| *a == Archetype::Everyday)
            .unwrap()
            .1;
        let total: f32 = cast.0.iter().map(|(_, s)| s).sum();
        assert!(
            everyday / total > 0.4,
            "a city of nothing but characters has none"
        );
        assert!((total - 1.0).abs() < 1e-5, "shares should sum to one");
    }

    #[test]
    fn a_group_walks_in_and_a_loner_walks_alone() {
        assert!(Archetype::Missionary.group_size() > 1);
        assert_eq!(Archetype::Everyday.group_size(), 1);
    }
}
