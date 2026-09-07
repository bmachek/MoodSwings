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
    /// Faster than a walk, lower than a hop: a glide with a kick in it.
    Skater,
    /// A stick, a shuffle, and all the time in the world.
    CaneUser,
    /// Wheels instead of a hop. Steadfast: nothing tips a wheelchair user
    /// over — they glide, they get punted like everybody else, they land
    /// on their wheels.
    Wheelchair,
}

impl Archetype {
    pub const ALL: [Archetype; 11] = [
        Archetype::Everyday,
        Archetype::Wutbuerger,
        Archetype::Punk,
        Archetype::Elvis,
        Archetype::Headphones,
        Archetype::Beggar,
        Archetype::Shy,
        Archetype::Missionary,
        Archetype::Skater,
        Archetype::CaneUser,
        Archetype::Wheelchair,
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
            Archetype::Skater => "Skater",
            Archetype::CaneUser => "Mit Gehstock",
            Archetype::Wheelchair => "Im Rollstuhl",
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
            Archetype::Skater => 1.4,
            Archetype::CaneUser => 0.68,
            _ => 1.0,
        }
    }

    /// Multiplier on the hop the bounce controller takes: a skater glides
    /// more than it bounces, and a wheelchair does not bounce at all — the
    /// controller's steering still works with the hop at zero, it just rolls.
    pub fn hop(self) -> f32 {
        match self {
            Archetype::Skater => 0.55,
            Archetype::Wheelchair => 0.0,
            _ => 1.0,
        }
    }

    /// Never knocked head over heels — see `bounce::launch::NeverTumbles`.
    /// A launched wheelchair flies level and lands rolling, which is both
    /// kinder and funnier than the alternative.
    pub fn steadfast(self) -> bool {
        self == Archetype::Wheelchair
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

/// How old somebody is, as far as a rubber figure can be: a size, a pace, a
/// pitch and a spring. The second axis of who a citizen is, orthogonal to
/// the archetype — a child punk and a senior Elvis are both legal and both
/// jokes — drawn from the same `stream::CROWD`.
///
/// The scale is applied to the collider capsule and to every part's `Rest`
/// pose, never to the body entity's transform: Avian scales a collider by
/// its transform, and a shrunken child would fall through the pavement.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AgeClass {
    Child,
    #[default]
    Adult,
    Senior,
}

impl AgeClass {
    /// Height relative to an adult.
    pub fn size(self) -> f32 {
        match self {
            AgeClass::Child => 0.62,
            AgeClass::Adult => 1.0,
            AgeClass::Senior => 0.94,
        }
    }

    /// Multiplier on the walking pace. Children scurry, seniors take the
    /// time the street owes them.
    pub fn pace(self) -> f32 {
        match self {
            AgeClass::Child => 1.1,
            AgeClass::Adult => 1.0,
            AgeClass::Senior => 0.7,
        }
    }

    /// Multiplier on the voice's pitch.
    pub fn pitch(self) -> f32 {
        match self {
            AgeClass::Child => 1.45,
            AgeClass::Adult => 1.0,
            AgeClass::Senior => 0.9,
        }
    }

    /// Multiplier on the hop. Childhood is mostly spring.
    pub fn spring(self) -> f32 {
        match self {
            AgeClass::Child => 1.25,
            AgeClass::Adult => 1.0,
            AgeClass::Senior => 0.85,
        }
    }

    /// Whether this age wears that archetype. Deliberately permissive — a
    /// child punk and a child Wutbürger are the tie broken towards the joke
    /// — with one exception that would not be one.
    pub fn suits(self, archetype: Archetype) -> bool {
        !(self == AgeClass::Child && archetype == Archetype::Beggar)
    }

    /// Draws an age from the fixed pyramid. A method on the class rather
    /// than a tunable table: the mix of ages is scenery, not a dial the
    /// game's feel hangs on.
    pub fn draw(rng: &mut ChaCha8Rng) -> AgeClass {
        let roll: f32 = rng.random_range(0.0..1.0);
        if roll < 0.12 {
            AgeClass::Child
        } else if roll < 0.82 {
            AgeClass::Adult
        } else {
            AgeClass::Senior
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
            (Archetype::Everyday, 0.44),
            (Archetype::Wutbuerger, 0.07),
            (Archetype::Punk, 0.07),
            // One in fifty. An Elvis is a sighting, not a demographic.
            (Archetype::Elvis, 0.02),
            (Archetype::Headphones, 0.08),
            (Archetype::Beggar, 0.05),
            (Archetype::Shy, 0.10),
            (Archetype::Missionary, 0.06),
            (Archetype::Skater, 0.05),
            (Archetype::CaneUser, 0.04),
            (Archetype::Wheelchair, 0.02),
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
