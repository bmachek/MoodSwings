//! Live-tunable gameplay constants.
//!
//! Everything that needs feel-tuning lives here rather than as scattered
//! literals, so the egui dev panel can edit it at runtime instead of forcing a
//! recompile. Sections are added as milestones need them.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::render::quality::GraphicsSettings;

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    /// Everything about world layout derives from this. Same seed, same city.
    pub world_seed: u64,
    pub world: WorldConfig,
    /// How much of a rubber ball everything in this city is.
    pub bounce: BounceConfig,
    /// How quick this city's temper is.
    pub mood: MoodConfig,
    /// How many people are on the pavements and how they move around each
    /// other. `#[serde(default)]` so an options file from before the crowd had
    /// dials still parses instead of resetting everything else in it.
    #[serde(default)]
    pub crowd: CrowdConfig,
    pub camera: CameraConfig,
    pub audio: AudioConfig,
    /// What the renderer is allowed to spend. Resolved from a single quality
    /// preset and then walked back to what the GPU actually supports; see
    /// [`crate::render::quality`].
    pub graphics: GraphicsSettings,
    /// The window itself, as opposed to what is rendered into it. Kept out of
    /// [`GraphicsSettings`] because that block is resolved from a quality
    /// preset and restored from saves; how big the window is belongs to the
    /// player and the screen it sits on, not to a preset or a save game.
    ///
    /// `#[serde(default)]` so an options file written before this existed
    /// still parses instead of resetting everything else in it.
    #[serde(default)]
    pub window: WindowConfig,
    /// Which figure the player wears, chosen in the pause menu's character
    /// screen. In the options rather than the save: a costume is a
    /// preference, like a keybinding, not a fact about one city.
    #[serde(default)]
    pub character: crate::ai::archetype::Archetype,
    /// How everybody on foot gets about: walking like a person or hopping
    /// like a flummi. A preference like the character, so it lives in the
    /// options; see [`Gait`] for why both stayed in the game.
    #[serde(default)]
    pub gait: Gait,
    /// Which city the seed builds — see [`CityStyle`]. Applied at startup,
    /// because the city is generated once; the settings screen says so.
    #[serde(default)]
    pub city: CityStyle,
}

/// Which city the generator builds.
///
/// The roadmap's postcard list, as parody: the same grid, the same seed
/// machinery, a different skyline and wardrobe per style. This is
/// deliberately a *style* and not a map — the world is built of axis-aligned
/// rectangles from the kerbs up, and a street plan that matches the real
/// Landshut needs curved blocks the whole pipeline cannot hold yet. What a
/// style *can* honestly deliver is what a postcard delivers: the heights, how
/// narrow the plots are and therefore how the roofline breaks up, whether
/// those roofs are flat or step into the sky, the colours, the number of
/// spires, and the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CityStyle {
    /// The city as it always was: nowhere in particular.
    #[default]
    Generisch,
    /// Landshut. Low pastel townhouses, the Isar, and the world's tallest
    /// brick tower with a plaque asking you not to lean on it.
    Landshuepf,
    /// New York. Everything is taller than it needs to be, including the
    /// advertising.
    NewDork,
    /// London. Brick, more brick, and a skyline that apologises for its
    /// one tall building.
    Londoof,
    /// München. Cream and ochre, more markets than strictly legal, and a
    /// cathedral of its own. The name is what Munich already calls itself.
    Minga,
    /// Paris. Six storeys forever, in cream.
    Paree,
}

impl CityStyle {
    pub const ALL: [Self; 6] = [
        Self::Generisch,
        Self::Landshuepf,
        Self::NewDork,
        Self::Londoof,
        Self::Minga,
        Self::Paree,
    ];

    /// The settings label and the postcard caption. Player-facing.
    pub fn label(self) -> &'static str {
        match self {
            Self::Generisch => "Irgendstadt",
            Self::Landshuepf => "Landshüpf",
            Self::NewDork => "New Dork",
            Self::Londoof => "Londoof",
            Self::Minga => "Minga",
            Self::Paree => "Paree",
        }
    }

    /// Multiplier on every district's building-height range. This is the
    /// single strongest lever a skyline has.
    pub fn height_scale(self) -> f32 {
        match self {
            Self::Generisch => 1.0,
            // Not the two storeys this said at first. Landshut's Altstadt is
            // four-storey townhouses under very tall gables, and at half scale
            // the gable came out taller than the house it was standing on.
            Self::Landshuepf => 0.82,
            Self::NewDork => 1.8,
            Self::Londoof => 0.8,
            Self::Minga => 0.7,
            Self::Paree => 0.75,
        }
    }

    /// How many churches the zoning pass claims, and whether the last of
    /// them is the cathedral — the one with the ridiculous tower.
    pub fn churches(self) -> (usize, bool) {
        match self {
            Self::Generisch => (3, false),
            // St. Martin's silhouette is the whole reason this style exists.
            Self::Landshuepf => (5, true),
            Self::NewDork => (1, false),
            Self::Londoof => (3, true),
            Self::Minga => (4, true),
            Self::Paree => (2, true),
        }
    }

    /// The vacant-lot roll band that becomes a market. Minga's is wide on
    /// purpose; the Viktualienmarkt is a load-bearing cliché.
    pub fn market_band(self) -> std::ops::Range<f32> {
        match self {
            Self::Minga => 0.30..0.40,
            _ => 0.30..0.335,
        }
    }

    /// The baked town this style builds, if it builds a real one.
    ///
    /// `Landshuepf` is the parody name and Landshut is the town, so this is
    /// where the joke stops being one: the style now loads
    /// `assets/cities/landshut.ron` and lays out the actual street plan. Every
    /// other dial on this enum still applies on top of it — the palette, the
    /// height scale, the gables — because those are what a postcard is, and a
    /// map underneath a postcard is still a postcard.
    ///
    /// Everywhere else is `None` and is generated from the seed as it always
    /// was.
    pub fn atlas(self) -> Option<&'static str> {
        match self {
            Self::Landshuepf => Some("landshut"),
            _ => None,
        }
    }

    /// Multiplier on every district's smallest buildable lot.
    ///
    /// The second lever the layout has, and the one a gable needs. A stepped
    /// screen is measured off the *width* of the house it caps — that is what
    /// gives a row of them one pitch and one silhouette — and the generator's
    /// default lot is eleven to twenty-five metres across, which is a
    /// warehouse. Landshut's Altstadt is burgage plots: narrow fronts, deep
    /// backs, four storeys, and the whole street is one roofline because of it.
    ///
    /// Below one this subdivides further, so a block yields more and narrower
    /// buildings out of the same draws — and that is the expensive direction.
    /// At 0.52, which is what real burgage plots would want, the city came out
    /// at fourteen thousand buildings against the default four, and eleven
    /// frames a second. 0.78 is the most narrowness this generator will carry.
    pub fn lot_scale(self) -> f32 {
        match self {
            Self::Landshuepf => 0.78,
            // Haussmann's blocks are long runs of one building, not plots.
            Self::Paree => 1.25,
            _ => 1.0,
        }
    }

    /// Share of low buildings that carry a stepped gable instead of a flat
    /// parapet — a `Giebelhaus`, front wall carried up past the roof as a
    /// stair-stepped screen.
    ///
    /// The single strongest lever a *roofline* has, and the reason it exists at
    /// all: what anybody who has stood in the Landshut Altstadt remembers is
    /// not the street plan and not the colour, it is a row of tall narrow
    /// houses whose fronts step up into the sky at their own heights. Nowhere
    /// else in this list has them — Minga has a few because the Bavarian
    /// old towns share the habit, and everywhere else is nought, because a
    /// stepped gable on a New York block is not a postcard, it is a mistake.
    pub fn gables(self) -> f32 {
        match self {
            Self::Landshuepf => 0.80,
            Self::Minga => 0.22,
            _ => 0.0,
        }
    }

    /// The gable-poster roll ceiling, out of eight. New Dork wants to be
    /// Times Square everywhere at once.
    pub fn advert_appetite(self) -> u64 {
        match self {
            Self::NewDork => 6,
            _ => 3,
        }
    }
}

/// Whether the city walks or bounces.
///
/// The city was built around the perpetual hop, and then it turned out that a
/// street of people *walking* — with the rubber saved for what happens to
/// them — reads better. Rather than deleting the hop (and the work in it),
/// the gait became a setting: `Walking` zeroes the resting hop and the squash
/// cycle that rides on it, `Bouncing` is the city as it was. Everything else
/// — launches, crashes, deliberate jumps, the solver's restitution — is
/// untouched by this switch, because being made of rubber was never the same
/// decision as travelling by bouncing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Gait {
    /// Feet on the ground: the resting hop is zeroed, so the bounce
    /// controller glides bodies the way it already glides wheelchairs and
    /// cyclists, and the walk cycle carries the motion.
    #[default]
    Walking,
    /// The original flummi city: everybody travels by hopping.
    Bouncing,
}

impl Gait {
    pub const ALL: [Self; 2] = [Self::Walking, Self::Bouncing];

    /// Multiplier on the *resting* hop — the travel bounce, not the jump.
    pub fn hop(self) -> f32 {
        match self {
            Self::Walking => 0.0,
            Self::Bouncing => 1.0,
        }
    }

    /// The squash depth that goes with it. Walking bodies are permanently at
    /// the bottom of a hop as far as `Bouncer::hop_phase` can tell, and a
    /// city frozen mid-squash reads as a rendering bug, so the whole cycle
    /// is switched off with the hop.
    pub fn squash(self, amount: f32) -> f32 {
        match self {
            Self::Walking => 0.0,
            Self::Bouncing => amount,
        }
    }

    /// The settings menu's label. Player-facing, so German.
    pub fn label(self) -> &'static str {
        match self {
            Self::Walking => "Gehen (realistisch)",
            Self::Bouncing => "Hüpfen (Flummi)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowConfig {
    /// Only meaningful in windowed mode; fullscreen takes the screen's own
    /// size, and the settings screen greys this out accordingly.
    pub resolution: Resolution,
    /// Borderless fullscreen on the screen the window is currently on —
    /// never the "exclusive" kind, which switches the display's video mode
    /// and takes the whole desktop down with it when the game hiccups.
    ///
    /// `#[serde(default)]` so an options file written when the resolution
    /// switcher was the whole of this section still parses.
    #[serde(default)]
    pub fullscreen: bool,
}

/// The window sizes the settings menu offers, in logical pixels — on a HiDPI
/// screen the OS multiplies by its scale factor, the same as the size the
/// window is opened with in `main`.
///
/// A fixed menu rather than a free width and height, so the options file can
/// never ask for a size the menu could not have produced, and so the settings
/// screen is a switch rather than two drag fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Resolution {
    R1280x720,
    /// The size the game has always opened at; staying the default means a
    /// fresh install and a pre-switcher install open the same window.
    #[default]
    R1600x900,
    R1920x1080,
    R2560x1440,
    R3840x2160,
}

impl Resolution {
    pub const ALL: [Self; 5] = [
        Self::R1280x720,
        Self::R1600x900,
        Self::R1920x1080,
        Self::R2560x1440,
        Self::R3840x2160,
    ];

    pub fn size(self) -> (u32, u32) {
        match self {
            Self::R1280x720 => (1280, 720),
            Self::R1600x900 => (1600, 900),
            Self::R1920x1080 => (1920, 1080),
            Self::R2560x1440 => (2560, 1440),
            Self::R3840x2160 => (3840, 2160),
        }
    }

    pub fn label(self) -> String {
        let (width, height) = self.size();
        format!("{width} × {height}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldConfig {
    /// Half-extent of the city, in metres.
    pub half_extent: f32,
    /// Chunks within this distance of the camera are spawned.
    pub stream_radius: f32,
    /// Real seconds for a full 24h cycle. 0 freezes the clock.
    pub day_length_seconds: f32,
    pub start_hour: f32,
    /// How wet the ground is when the world opens, 0 to 1.
    ///
    /// A starting value rather than a dial. Weather runs on its own from here —
    /// see [`crate::world::weather::Weather`] for the live values — and it runs
    /// on the same clock as the sun, so `day_length_seconds` at zero holds the
    /// whole sky still. That is what a screenshot needs.
    pub start_wetness: f32,
    /// And how much of the sky is under cloud when it opens, 0 to 1.
    pub start_cover: f32,
}

/// The elastic half of the simulation.
///
/// Two unrelated things are tuned from here, and they are kept together because
/// they have to be tuned against each other. `restitution` and `threshold`
/// belong to the solver: they decide how a body that is *not* in charge of
/// itself rebounds. `hop_speed` and the accelerations belong to the character
/// controller: they decide how a body that *is* in charge of itself gets about,
/// which in this city means hopping. A player who bounces off a wall harder
/// than they can hop is a player who has lost control of the game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BounceConfig {
    /// Fraction of closing speed returned by a collision, 0 to 1.
    pub restitution: f32,
    /// Closing speed, in m/s, below which the solver stops bothering to bounce.
    ///
    /// Avian's default is 1.0, which is most of a hop: without lowering this,
    /// every small knock is absorbed and the city reads as rubber only when
    /// something arrives at speed.
    pub threshold: f32,
    /// Upward speed, in m/s, taken on at the bottom of every hop.
    pub hop_speed: f32,
    /// How hard a grounded body pulls itself towards the speed it wants.
    pub ground_accel: f32,
    /// And in the air, where there is nothing to push against. Much lower, so
    /// that a hop commits you to where it is going.
    pub air_accel: f32,
    /// How far a figure squashes at the bottom of a hop, as a fraction of its
    /// height. Zero is a rigid body on a pogo stick; too much is a puddle.
    pub squash: f32,
    /// Extra speed a crashed car takes away from whatever it hit, as a
    /// fraction of the speed it lost arriving. On top of the solver's own
    /// restitution, because the solver's idea of elastic is physically
    /// defensible and therefore not funny.
    ///
    /// The three `crash_*` dials are `#[serde(default)]` so an options file
    /// written before they existed still parses instead of resetting
    /// everything else in it.
    #[serde(default = "default_crash_rebound")]
    pub crash_rebound: f32,
    /// Upward part of that rebound, as a fraction of the speed lost. A car
    /// that only bounces back is a billiard ball; one that also leaves the
    /// ground is a joke.
    #[serde(default = "default_crash_pop")]
    pub crash_pop: f32,
    /// Spin handed to a crashed car, in rad/s per m/s of speed lost. Off-axis
    /// blows pirouette; head-on ones tip.
    #[serde(default = "default_crash_spin")]
    pub crash_spin: f32,
    /// The player's resting hop, as a fraction of `hop_speed`. Deliberately
    /// below the whole crowd's range: the camera rides the player's hop
    /// almost 1:1, so a scale that looks lively on a citizen across the
    /// street reads as seasickness from the navel the view is bolted to.
    /// The others do the bouncing; the player mostly watches them do it.
    #[serde(default = "default_player_hop_scale")]
    pub player_hop_scale: f32,
    /// Ceiling on how high delight scales a citizen's hop. This is where the
    /// bounce the player gave up went.
    #[serde(default = "default_npc_spring_max")]
    pub npc_spring_max: f32,
}

fn default_apology_range() -> f32 {
    GameConfig::default().mood.apology_range
}
fn default_apology_balm() -> f32 {
    GameConfig::default().mood.apology_balm
}

fn default_player_hop_scale() -> f32 {
    GameConfig::default().bounce.player_hop_scale
}
fn default_npc_spring_max() -> f32 {
    GameConfig::default().bounce.npc_spring_max
}
fn default_crash_rebound() -> f32 {
    GameConfig::default().bounce.crash_rebound
}
fn default_crash_pop() -> f32 {
    GameConfig::default().bounce.crash_pop
}
fn default_crash_spin() -> f32 {
    GameConfig::default().bounce.crash_spin
}

/// How the city feels, and how fast it changes its mind.
///
/// Only the numbers that are shared between subsystems live here. A single
/// flummi's disposition is its [`crate::mood::feeling::Temperament`], because that
/// varies from one citizen to the next and a global dial cannot express "most
/// people are fine, one in ten is a menace".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoodConfig {
    /// How far a mood carries to the neighbours, in metres.
    ///
    /// Roughly the width of a street. Wider and the whole city moves as one
    /// block, which is a single mood rather than a crowd of them.
    pub contagion_radius: f32,
    /// How fast a flummi is pulled towards the mood around it, per second at
    /// full susceptibility.
    pub contagion_rate: f32,
    /// Velocity lost in a knock, in m/s, up to which it reads as a friendly
    /// bop rather than as an insult. The joke lives on this line: the same
    /// nudge delights one flummi and starts a feud with the next.
    pub bop_limit: f32,
    /// And the loss that makes the worst impression anybody can make. Harder
    /// knocks than this exist; they are no more insulting.
    pub outrage_limit: f32,
    /// Mood below which a flummi counts as having gone red. What the rage-wave
    /// readout in the HUD counts crossings of.
    pub rage_line: f32,
    /// How far a raspberry carries as an insult, in metres. Deliberately
    /// shorter than a whistle: it should be possible to be rude to one person
    /// without starting a riot, and to cheer up a whole street at once.
    pub taunt_radius: f32,
    pub cheer_radius: f32,
    /// How much mood a taunt takes off somebody standing right next to it, at
    /// a fuse of 1. Further away it is less; see
    /// [`crate::mood::provoke::carry`].
    pub taunt_bite: f32,
    /// And how much a whistle gives back.
    pub cheer_warmth: f32,
    /// Seconds between one flummi's provocations. Long enough that the button
    /// is a decision rather than a drum roll.
    pub provoke_rest: f32,
    /// How far an apology can be thrown, in metres. Shorter than a cheer:
    /// making peace means walking up to somebody, not shouting sorry across
    /// a junction.
    ///
    /// The two `apology_*` dials are `#[serde(default)]` so an options file
    /// written before they existed still parses.
    #[serde(default = "default_apology_range")]
    pub apology_range: f32,
    /// Mood restored to whoever the flower reaches. Deliberately the biggest
    /// single lift in the game: an apology accepted has to actually settle
    /// the matter, or the flower is a decoration on a feud.
    #[serde(default = "default_apology_balm")]
    pub apology_balm: f32,
    /// How long somebody stays after whoever offended them, in seconds.
    pub grudge_seconds: f32,
    /// Ground speed of a flummi with a score to settle, in m/s. Faster than
    /// walking and slower than sprinting: being chased has to be survivable.
    pub grudge_speed: f32,
}

/// The crowd on the pavements.
///
/// These lived as private constants in `ai::pedestrian` until the crowd grew
/// dials worth turning. Only the feel numbers moved here: the capsule sizes
/// and the pavement offset are geometry, and a slider on geometry is a way to
/// clip a crowd through a wall from a panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrowdConfig {
    /// How many pedestrians are kept walking around the player.
    pub population: usize,
    /// New arrivals appear on road edges between these distances, in metres:
    /// far enough not to pop in on camera, near enough to arrive on screen
    /// within a stroll.
    pub spawn_min: f32,
    pub spawn_max: f32,
    /// And past this they are quietly recycled.
    pub despawn: f32,
    /// Metres per second of an unhurried citizen. Individuals vary around it
    /// at spawn, and mood scales it live — see `ai::pedestrian::stride`.
    pub walk_speed: f32,
    /// Flat out, ahead of a car. Panic overrides temperament.
    pub flee_speed: f32,
    /// A vehicle closer than this and faster than `scare_speed` is worth
    /// running from.
    pub scare_radius: f32,
    pub scare_speed: f32,
    /// Personal space, in metres. Inside it a citizen leans its intent away
    /// from the neighbours so the crowd flows instead of stacking. Kept small
    /// on purpose: contact must stay possible, because a small knock is a
    /// friendly bop and the bop is load-bearing comedy — see
    /// `mood::feeling::jolt`.
    pub separation_radius: f32,
    /// How hard the lean is, in m/s at a full push. Against a walk of
    /// 1.5 m/s this bends paths without ever pinning anybody in place.
    pub separation_push: f32,
}

impl Default for CrowdConfig {
    fn default() -> Self {
        GameConfig::default().crowd
    }
}

/// The mixer. Three numbers rather than one, because the background bed and
/// the things that happen in front of it want independent control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Scales everything below it.
    pub master: f32,
    /// Weapons, crashes, engines, sirens: anything an event causes.
    pub effects: f32,
    /// The city's background rumble.
    pub ambience: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    /// Free-fly movement speed.
    pub speed: f32,
    pub boost_multiplier: f32,
    pub mouse_sensitivity: f32,
    /// Flips vertical mouse look. Off by default; some players' hands only
    /// work the other way, and that preference is old enough to predate this
    /// genre.
    pub invert_look_y: bool,
    /// How hard the view swings itself in behind the direction of travel, as
    /// an exponential rate in reciprocal seconds. 0 turns it off entirely.
    pub auto_follow: f32,
    /// Seconds of hands off the mouse before that swing starts. Long enough
    /// that looking somewhere deliberately is never fought, short enough that
    /// a corner taken two-handed does not lose the car off the side of the
    /// screen.
    pub auto_follow_delay: f32,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            world_seed: 0xA17E_5EED,
            world: WorldConfig {
                half_extent: 1000.0,
                stream_radius: 900.0,
                day_length_seconds: 600.0,
                start_hour: 9.5,
                start_wetness: 0.0,
                // A fair day with a little cloud in it. Where the weather drifts
                // from here is the seed's business.
                start_cover: 0.18,
            },
            bounce: BounceConfig {
                // Not 1.0 — a perfectly elastic city never settles — and no
                // longer 0.86 either: at that height walls and kerbs threw
                // the player around harder than the player could steer, and
                // a crashed car pinballed for ten seconds before it could be
                // driven again. The comedy send-offs are applied by hand
                // (`vehicle::impact::fling`, `world::mayhem::send_off`), so
                // the solver's own rebound can afford to be modest.
                restitution: 0.62,
                threshold: 0.05,
                hop_speed: 2.8,
                ground_accel: 42.0,
                air_accel: 22.0,
                squash: 0.35,
                // Together these mean a solid 12 m/s crash throws the car
                // back at ~7 m/s, hops it half a metre and turns it most of
                // the way round — enough that both parties leave the scene.
                crash_rebound: 0.6,
                crash_pop: 0.28,
                crash_spin: 0.35,
                player_hop_scale: 0.6,
                npc_spring_max: 1.5,
            },
            crowd: CrowdConfig {
                population: 45,
                spawn_min: 25.0,
                spawn_max: 110.0,
                despawn: 165.0,
                walk_speed: 1.5,
                flee_speed: 5.4,
                scare_radius: 14.0,
                scare_speed: 6.0,
                separation_radius: 0.9,
                separation_push: 1.2,
            },
            mood: MoodConfig {
                contagion_radius: 9.0,
                contagion_rate: 0.9,
                bop_limit: 6.5,
                outrage_limit: 18.0,
                rage_line: -0.5,
                taunt_radius: 11.0,
                cheer_radius: 15.0,
                taunt_bite: 0.55,
                cheer_warmth: 0.34,
                provoke_rest: 0.8,
                apology_range: 13.0,
                apology_balm: 0.55,
                grudge_seconds: 7.0,
                grudge_speed: 5.2,
            },
            camera: CameraConfig {
                speed: 25.0,
                boost_multiplier: 5.0,
                mouse_sensitivity: 0.002,
                invert_look_y: false,
                auto_follow: 3.0,
                auto_follow_delay: 0.7,
            },
            audio: AudioConfig {
                master: 0.7,
                effects: 1.0,
                ambience: 0.5,
            },
            graphics: GraphicsSettings::default(),
            window: WindowConfig::default(),
            character: crate::ai::archetype::Archetype::default(),
            gait: Gait::default(),
            city: CityStyle::default(),
        }
    }
}

impl Default for CameraConfig {
    fn default() -> Self {
        GameConfig::default().camera
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_resolution_is_the_size_main_opens_the_window_with() {
        assert_eq!(Resolution::default().size(), (1600, 900));
    }

    #[test]
    fn the_resolution_menu_lists_every_size_once_smallest_first() {
        let sizes: Vec<_> = Resolution::ALL.iter().map(|r| r.size()).collect();
        let mut sorted = sizes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sizes, sorted);
    }

    #[test]
    fn a_window_section_from_before_the_fullscreen_switch_still_parses() {
        let parsed: WindowConfig =
            ron::from_str("(resolution:R1920x1080)").expect("old window section should parse");
        assert_eq!(parsed.resolution, Resolution::R1920x1080);
        assert!(!parsed.fullscreen);
    }

    #[test]
    fn an_options_file_without_a_window_section_still_parses() {
        // What `saves/options.ron` looked like before the resolution switcher:
        // the whole config, minus the `window` field.
        let mut old = GameConfig::default();
        old.audio.master = 0.42;
        let mut text = ron::ser::to_string(&old).unwrap();
        let start = text.find("window:").unwrap();
        let end = text.rfind(')').unwrap();
        text.replace_range(start..end, "");
        let parsed: GameConfig = ron::from_str(&text).expect("old options should parse");
        assert_eq!(parsed.audio.master, 0.42);
        assert_eq!(parsed.window.resolution, Resolution::default());
    }

    #[test]
    fn an_options_file_without_a_crowd_section_still_parses() {
        // What `saves/options.ron` looked like before the crowd had dials:
        // the whole config, minus the `crowd` section.
        let mut old = GameConfig::default();
        old.audio.master = 0.42;
        let mut text = ron::ser::to_string(&old).unwrap();
        let start = text.find("crowd:(").unwrap();
        let end = start + text[start..].find(')').unwrap() + 2;
        text.replace_range(start..end, "");
        let parsed: GameConfig = ron::from_str(&text).expect("old options should parse");
        assert_eq!(parsed.audio.master, 0.42);
        assert_eq!(
            parsed.crowd.population,
            GameConfig::default().crowd.population
        );
    }

    #[test]
    fn the_city_walks_by_default_and_can_be_told_to_bounce() {
        // The decision this records: walking became the default, hopping
        // stayed as the option, and no work was thrown away for it.
        assert_eq!(Gait::default(), Gait::Walking);
        assert_eq!(Gait::Walking.hop(), 0.0);
        assert_eq!(Gait::Bouncing.hop(), 1.0);
        // The squash cycle rides on the hop and must go with it, or a
        // walking city is a city frozen mid-squash.
        assert_eq!(Gait::Walking.squash(0.35), 0.0);
        assert_eq!(Gait::Bouncing.squash(0.35), 0.35);
    }

    #[test]
    fn an_options_file_from_before_the_gait_switch_still_parses() {
        let mut old = GameConfig::default();
        old.audio.master = 0.42;
        let mut text = ron::ser::to_string(&old).unwrap();
        let start = text.find("gait:").unwrap();
        let end = start + text[start..].find(')').unwrap();
        text.replace_range(start..end, "");
        let parsed: GameConfig = ron::from_str(&text).expect("old options should parse");
        assert_eq!(parsed.audio.master, 0.42);
        assert_eq!(parsed.gait, Gait::default());
    }

    #[test]
    fn an_options_file_without_the_hop_dials_still_parses() {
        // What a bounce section looked like before `player_hop_scale` and
        // `npc_spring_max` landed: the fields simply absent.
        let mut old = ron::ser::to_string(&GameConfig::default()).unwrap();
        for field in ["player_hop_scale", "npc_spring_max"] {
            let start = old.find(field).unwrap();
            let rest = &old[start..];
            // The section's last field ends at `)` rather than `,`.
            let comma = rest.find(',');
            let paren = rest.find(')').unwrap();
            let end = match comma {
                Some(comma) if comma < paren => start + comma + 1,
                _ => start + paren,
            };
            old.replace_range(start..end, "");
        }
        let parsed: GameConfig = ron::from_str(&old).expect("old options should parse");
        let fresh = GameConfig::default().bounce;
        assert_eq!(parsed.bounce.player_hop_scale, fresh.player_hop_scale);
        assert_eq!(parsed.bounce.npc_spring_max, fresh.npc_spring_max);
    }
}
