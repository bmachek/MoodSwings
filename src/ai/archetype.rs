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
    /// Five abreast, one football chant short of a mood. The taunt-spamming
    /// mirror of the missionaries.
    Hooligan,
    /// Four leather jackets that arrive together and hold a grudge together.
    Rocker,
    /// Faster than a walk, lower than a hop: a glide with a kick in it.
    Skater,
    /// A stick, a shuffle, and all the time in the world.
    CaneUser,
    /// Wheels instead of a hop. Steadfast: nothing tips a wheelchair user
    /// over — they glide, they get punted like everybody else, they land
    /// on their wheels.
    Wheelchair,
    /// A guitar and an unshakeable conviction that this corner is a venue.
    /// Given an audience, he plays, and the street's mood actually lifts —
    /// see `ai::social::busk`.
    Busker,
    /// A camera where a face should be pointed. Photographs strangers at
    /// point-blank range; most are flattered, the Shy emphatically are not.
    Photographer,
    /// A tray of unspecified wares and all the patter in the world. No
    /// mechanic of his own: a fixed sunny temper and the city's highest
    /// chattiness *are* the business model.
    Vendor,
}

impl Archetype {
    pub const ALL: [Archetype; 16] = [
        Archetype::Everyday,
        Archetype::Wutbuerger,
        Archetype::Punk,
        Archetype::Elvis,
        Archetype::Headphones,
        Archetype::Beggar,
        Archetype::Shy,
        Archetype::Missionary,
        Archetype::Hooligan,
        Archetype::Rocker,
        Archetype::Skater,
        Archetype::CaneUser,
        Archetype::Wheelchair,
        Archetype::Busker,
        Archetype::Photographer,
        Archetype::Vendor,
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
            Archetype::Hooligan => "Hooligan",
            Archetype::Rocker => "Rocker",
            Archetype::Skater => "Skater",
            Archetype::CaneUser => "Mit Gehstock",
            Archetype::Wheelchair => "Im Rollstuhl",
            Archetype::Busker => "Straßenmusiker",
            Archetype::Photographer => "Streetfotograf",
            Archetype::Vendor => "Fliegender Händler",
        }
    }

    /// A disposition the archetype fixes, or `None` to draw from the mix.
    pub fn temper(self) -> Option<Temperament> {
        match self {
            Archetype::Wutbuerger => Some(Temperament::ragemonger()),
            Archetype::Shy => Some(Temperament::serene()),
            Archetype::Missionary => Some(Temperament::easygoing()),
            Archetype::Hooligan => Some(Temperament::touchy()),
            Archetype::Rocker => Some(Temperament::easygoing()),
            // Playing music all day and selling things to strangers are both
            // jobs you keep only if nothing much dents you.
            Archetype::Busker => Some(Temperament::easygoing()),
            Archetype::Vendor => Some(Temperament::easygoing()),
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
            Archetype::Hooligan => Some(Color::srgb(0.88, 0.88, 0.90)),
            Archetype::Rocker => Some(Color::srgb(0.09, 0.08, 0.08)),
            // A corduroy sort of brown; every busker owns exactly one coat.
            Archetype::Busker => Some(Color::srgb(0.42, 0.30, 0.18)),
            // The many-pocketed khaki vest, worn as a uniform worldwide.
            Archetype::Photographer => Some(Color::srgb(0.52, 0.48, 0.34)),
            // Mustard: loud enough to be its own advertising.
            Archetype::Vendor => Some(Color::srgb(0.78, 0.60, 0.14)),
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
            // A guitar is luggage; a tray of wares is furniture.
            Archetype::Busker => 0.85,
            Archetype::Vendor => 0.75,
            // Always hurrying after the next shot.
            Archetype::Photographer => 1.15,
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

    /// A rudeness this archetype produces spontaneously whatever its mood:
    /// the missionaries cheer, the hooligans chant. Everybody else needs to
    /// actually feel something before they say so.
    pub fn spam(self) -> Option<crate::mood::provoke::Rudeness> {
        match self {
            Archetype::Missionary => Some(crate::mood::provoke::Rudeness::Cheer),
            Archetype::Hooligan => Some(crate::mood::provoke::Rudeness::Taunt),
            _ => None,
        }
    }

    /// A factor on the voice. The gangs run deep.
    pub fn pitch(self) -> f32 {
        match self {
            Archetype::Hooligan => 0.88,
            Archetype::Rocker => 0.78,
            _ => 1.0,
        }
    }

    /// How readily this archetype stops for small talk, as a multiplier on
    /// the street's base chance. Zero is a veto — see
    /// [`crate::ai::social::chat_chance`], which multiplies both parties
    /// through, so the Shy are never cornered and headphones actually work.
    pub fn chattiness(self) -> f32 {
        match self {
            Archetype::Shy | Archetype::Headphones => 0.0,
            Archetype::Wutbuerger => 0.3,
            Archetype::Hooligan => 0.4,
            Archetype::Skater => 0.5,
            Archetype::Punk => 0.7,
            Archetype::Rocker => 0.8,
            Archetype::Beggar => 1.2,
            Archetype::CaneUser => 1.4,
            // Recruiting is talking. Of course they stop.
            Archetype::Missionary => 1.6,
            // Mid-set there is no small talk; the guitar does the talking.
            Archetype::Busker => 0.5,
            Archetype::Photographer => 0.6,
            // Selling is talking with a tray. Outranks even the recruiters.
            Archetype::Vendor => 1.8,
            _ => 1.0,
        }
    }

    /// An excuse this archetype has to stand still: (chance per second,
    /// how long, whether they face the buildings while they do it).
    ///
    /// The window-shopper faces the shopfronts; everybody else faces
    /// wherever they happened to stop, which for a beggar with a cup out
    /// and a punk holding up a corner is exactly right.
    pub fn loiter(self) -> Option<(f32, f32, bool)> {
        match self {
            Archetype::Everyday => Some((0.010, 4.0, true)),
            Archetype::Beggar => Some((0.05, 9.0, false)),
            Archetype::Punk => Some((0.03, 7.0, false)),
            Archetype::CaneUser => Some((0.04, 5.0, false)),
            // A busker between sets is a busker scouting the next pitch,
            // and a vendor's whole trade is standing somewhere promising.
            Archetype::Busker => Some((0.04, 8.0, false)),
            Archetype::Vendor => Some((0.05, 10.0, false)),
            _ => None,
        }
    }

    /// How many of them arrive together.
    pub fn group_size(self) -> usize {
        match self {
            Archetype::Missionary => 4,
            Archetype::Hooligan => 5,
            Archetype::Rocker => 4,
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

    /// Multiplier on the chance of stopping for a chat. Children have
    /// somewhere to be; seniors have all the time the street owes them.
    pub fn chattiness(self) -> f32 {
        match self {
            AgeClass::Child => 0.7,
            AgeClass::Adult => 1.0,
            AgeClass::Senior => 1.5,
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
            (Archetype::Everyday, 0.41),
            (Archetype::Wutbuerger, 0.06),
            (Archetype::Punk, 0.06),
            // One in fifty. An Elvis is a sighting, not a demographic.
            (Archetype::Elvis, 0.02),
            (Archetype::Headphones, 0.07),
            (Archetype::Beggar, 0.04),
            (Archetype::Shy, 0.09),
            (Archetype::Missionary, 0.04),
            (Archetype::Hooligan, 0.03),
            (Archetype::Rocker, 0.02),
            (Archetype::Skater, 0.04),
            (Archetype::CaneUser, 0.04),
            (Archetype::Wheelchair, 0.02),
            // The street performers, paid for with a sliver off everybody
            // else's share rather than off the Everyday crowd — a city needs
            // its plain majority more than it needs a fourth punk per block.
            (Archetype::Busker, 0.02),
            (Archetype::Photographer, 0.02),
            (Archetype::Vendor, 0.02),
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
