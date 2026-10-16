//! The crowd watches, and it has opinions.
//!
//! The city's biggest comedy events used to play to a house that only turned
//! round: a flummi cartwheeling over a bonnet moved its own mood
//! (`feeling::feel_wallops`) and made the street look (`ai::social`'s
//! rubbernecking), but nobody who watched it *felt* anything; a crash was a
//! sound and a fling; a parking meter leaving its bolts was one sproing. Now
//! the street reacts. Who finds a launch funny and who takes against it is
//! decided by the same two numbers everything else here runs on — the mood
//! and the temperament — so a cheerful street giggles itself happier while a
//! sour one talks itself into a feud, which is exactly the two-contagion city
//! `grudge` already built, fed from a new direction.
//!
//! Everything is a listener. `Wallop`, `VehicleImpact`, `PropSheared` and
//! `TookFright` are all written elsewhere; this module only reads them, which
//! is why it costs the simulation nothing when the street is quiet. The
//! looking stays `ai::social`'s job and this is only the feeling: a gawker
//! here is somebody whose afternoon just changed, not somebody who stopped.
//!
//! The systems sit inside [`Feeling`], after the victim's own jolt and before
//! the room is read, so a witness pushed into the red counts towards a rage
//! wave like anybody else — and the face, which runs after the whole of
//! `Feeling`, shows the witnessed half the same frame.

use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::feeling::{self, Feeling, Mood, Temperament};
use super::grudge::{Grudge, Pirouette, bears_a_grudge};
use super::provoke::carry;
use super::scuffle::Scuffle;
use crate::ai::pedestrian::TookFright;
use crate::bounce::boing::Wallop;
use crate::bounce::controller::Launched;
use crate::core::config::{GameConfig, SchadenfreudeConfig};
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::player::interact::Driving;
use crate::player::on_foot::Player;
use crate::vehicle::impact::VehicleImpact;
use crate::world::mayhem::PropSheared;

/// Wallop severity, in m/s lost, below which nobody in the audience feels
/// anything. Kin to `grudge::WORTH_BLAMING`: a hop off a kerb is not theatre.
const WORTH_A_LOOK: f32 = 5.0;
/// Chance an amused witness is tickled enough to pirouette on the spot.
const GLEE: f32 = 0.5;
/// Mood a witness must already be in for the pirouette; below it they enjoy
/// the show quietly. Same corner of the scale as `grudge::CHUFFED`.
const TICKLED: f32 = 0.55;
/// How far from the landing the "culprit" can be picked, matching the range
/// `grudge::blame_the_nearest` uses for the victim's own guess.
const SUSPECT_RANGE: f32 = 6.0;
/// A crash's severity at which the reaction saturates, in m/s lost. The same
/// scale `vehicle::impact` calls a solid crash.
const FULL_CRASH: f32 = 12.0;
/// Prop mass at which the cartwheel is as impressive as it gets, and the
/// speed that gives it full send. The planter is the heaviest thing bolted.
const FULL_HEFT: f32 = 120.0;
const FULL_SEND: f32 = 12.0;

/// Reaction rolls draw from their own stream (`stream::WITNESS`) — runtime
/// chaos in the `MayhemRng` sense, and emphatically not `MoodRng`, whose
/// draw order fixes which temperament every future citizen spawns with.
#[derive(Resource)]
struct WitnessRng(ChaCha8Rng);

pub struct SchadenfreudePlugin;

impl Plugin for SchadenfreudePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, seed_the_stream).add_systems(
            Update,
            (
                witness_wallops,
                witness_crashes,
                enjoy_the_sproings,
                feel_the_fright,
            )
                .chain()
                .in_set(Feeling)
                .in_set(GameSet::Ai)
                // After the victim's own jolt, so a witness's verdict is read
                // off the mood the street is in rather than one a frame old,
                // and before the room is read, so the crossings count.
                .after(feeling::feel_wallops)
                .before(feeling::read_the_room),
        );
    }
}

fn seed_the_stream(mut commands: Commands, config: Res<GameConfig>) {
    commands.insert_resource(WitnessRng(stream_for(config.world_seed, stream::WITNESS)));
}

// ------------------------------------------------------------- the maths ----

/// How funny somebody else's misfortune is to this citizen, signed.
///
/// Positive is amusement, negative is offence taken on the victim's behalf.
/// Two pulls: a good mood inclines anybody towards laughing, and the fuse
/// decides how strongly the same sight reads as an outrage instead — so a
/// serene flummi grins at a pile-up a Wutbürger would join a mob over. The
/// crossover sits near the ordinary temperament in an ordinary mood, which
/// keeps the average street evenly split, and the split is the theatre.
pub fn verdict(mood: f32, temper: &Temperament) -> f32 {
    let cheer = 0.5 + 0.5 * mood.clamp(-1.0, 1.0);
    let humour = cheer * (1.2 - temper.fuse);
    let offence = (1.0 - cheer) * temper.fuse * 0.9;
    (humour - offence).clamp(-1.0, 1.0)
}

/// What a crash does to somebody standing `distance` from it, signed.
///
/// The same event is two different experiences: inside `alarm_radius` it is
/// happening *to* you and the reading is negative, past it you have a seat in
/// the stalls and it is positive, fading to nothing at `crash_radius`. One
/// continuous curve rather than two zones, so a step backwards never flips
/// anybody's afternoon at a boundary.
pub fn crash_stirs(distance: f32, severity: f32, tune: &SchadenfreudeConfig) -> f32 {
    if severity < tune.crash_stir {
        return 0.0;
    }
    let heft = (severity / FULL_CRASH).clamp(0.0, 1.0);
    // -1 at the bumper, +1 from the alarm line outwards.
    let seat = ((distance / tune.alarm_radius.max(0.1)) * 2.0 - 1.0).clamp(-1.0, 1.0);
    carry(distance, tune.crash_radius) * heft * seat
}

/// The street's delight at one piece of furniture leaving its bolts.
///
/// Always positive: world damage is the only damage this game has, and the
/// crowd is unambiguously *for* it — a sheared parking meter costs nobody
/// anything and cartwheels beautifully. Heavier and faster is funnier, up to
/// a cap, because delight is not a physics quantity.
pub fn sproing_glee(speed: f32, mass: f32, tune: &SchadenfreudeConfig) -> f32 {
    let heft = (mass / FULL_HEFT).clamp(0.0, 1.0);
    let send = (speed / FULL_SEND).clamp(0.0, 1.0);
    tune.sproing_delight * (0.4 + 0.6 * heft) * send
}

/// What taking fright costs: a little, scaled up the shorter the fuse —
/// being made to run is the sort of thing a Wutbürger remembers.
pub fn fright_cost(temper: &Temperament, tune: &SchadenfreudeConfig) -> f32 {
    tune.fright_dip * (0.5 + 0.5 * temper.fuse)
}

/// One mood moved by a signed reading: the laugh scaled by `amusement`, the
/// offence by `indignation`, which is the slightly stronger of the two so a
/// street that watches a massacre trends sour rather than giddy.
fn shifted(mood: f32, reading: f32, tune: &SchadenfreudeConfig) -> f32 {
    let weight = if reading >= 0.0 {
        tune.amusement
    } else {
        tune.indignation
    };
    (mood + reading * weight).clamp(-1.0, 1.0)
}

// ----------------------------------------------------------- the systems ----

/// Everybody who sees a launch reacts to it, each according to their nature.
///
/// The victim is skipped — `feel_wallops`, the gasp and `blame_the_nearest`
/// already own their side, and nobody is their own audience. Witness moods
/// shift by the verdict; a delighted witness may pirouette on the spot, an
/// appalled one may take up a grudge against whoever is standing nearest the
/// landing — the same deliberately unreliable attribution the victim uses,
/// and wrong the same satisfying half of the time.
fn witness_wallops(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut rng: ResMut<WitnessRng>,
    mut wallops: MessageReader<Wallop>,
    mut flummis: Query<
        (
            Entity,
            &Transform,
            &mut Mood,
            &Temperament,
            Has<Player>,
            Has<Scuffle>,
        ),
        Without<Launched>,
    >,
) {
    let tune = &config.schadenfreude;
    for wallop in wallops.read() {
        if wallop.severity < WORTH_A_LOOK {
            continue;
        }
        // Positions first (the `spread_moods` discipline): the suspect is
        // picked from the same query the witnesses are walked out of.
        let suspect = flummis
            .iter()
            .filter(|(other, ..)| *other != wallop.entity)
            .map(|(other, at, ..)| (at.translation.distance(wallop.position), other))
            .filter(|(apart, _)| *apart < SUSPECT_RANGE)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, other)| other);

        for (entity, transform, mut mood, temper, is_player, scuffling) in &mut flummis {
            if entity == wallop.entity {
                continue;
            }
            let apart = transform.translation.distance(wallop.position);
            if apart > tune.watch_radius {
                continue;
            }
            let funny = verdict(mood.value, temper);
            mood.value = shifted(mood.value, funny * carry(apart, tune.watch_radius), tune);

            // One roll per witness either way, so the amused and the appalled
            // cost the stream the same and the split stays deterministic.
            let roll = rng.0.random::<f32>();
            // The player's body is the player's own; only their mood watches.
            // And somebody with a collar in their fist has their hands full —
            // a spin or a fresh chase would be two systems steering one body.
            if is_player || scuffling {
                continue;
            }
            if funny > 0.0 {
                if mood.value >= TICKLED && roll < GLEE {
                    commands.entity(entity).try_insert(Pirouette::solo());
                }
            } else if bears_a_grudge(mood.value, temper, roll)
                && let Some(suspect) = suspect
                && suspect != entity
            {
                commands.entity(entity).try_insert(Grudge {
                    against: suspect,
                    left: config.mood.grudge_seconds,
                });
            }
        }
    }
}

/// A crash ripples through the pavement: alarming at the kerb it mounts,
/// comedy from across the street.
///
/// No grudge comes of it — nobody chases a car — and no pirouette is needed:
/// a street pushed cheerful enough starts giggling by itself through
/// `voice::speak_up`, which is the cheapest audience reaction there is. The
/// player at the wheel is skipped: they are in the crash, not watching it,
/// and souring the one mood the HUD shows for every kerb they clip would be
/// the game telling them off for playing it.
fn witness_crashes(
    config: Res<GameConfig>,
    mut impacts: MessageReader<VehicleImpact>,
    mut flummis: Query<(&Transform, &mut Mood), (With<Temperament>, Without<Driving>)>,
) {
    let tune = &config.schadenfreude;
    for impact in impacts.read() {
        for (transform, mut mood) in &mut flummis {
            let stir = crash_stirs(
                transform.translation.distance(impact.position),
                impact.severity,
                tune,
            );
            if stir != 0.0 {
                mood.value = shifted(mood.value, stir, tune);
            }
        }
    }
}

/// Street furniture cartwheeling is for everybody — the driver who sent it
/// included.
fn enjoy_the_sproings(
    config: Res<GameConfig>,
    mut sheared: MessageReader<PropSheared>,
    mut flummis: Query<(&Transform, &mut Mood), With<Temperament>>,
) {
    let tune = &config.schadenfreude;
    for sproing in sheared.read() {
        let glee = sproing_glee(sproing.speed, sproing.mass, tune);
        for (transform, mut mood) in &mut flummis {
            let apart = transform.translation.distance(sproing.position);
            if apart > tune.crash_radius {
                continue;
            }
            mood.value = (mood.value + carry(apart, tune.crash_radius) * glee).clamp(-1.0, 1.0);
        }
    }
}

/// Taking fright costs a little mood. The squeal is `voice`'s; this is only
/// the feeling.
fn feel_the_fright(
    config: Res<GameConfig>,
    mut frights: MessageReader<TookFright>,
    mut flummis: Query<(&mut Mood, &Temperament)>,
) {
    for fright in frights.read() {
        let Ok((mut mood, temper)) = flummis.get_mut(fright.entity) else {
            continue;
        };
        let dip = fright_cost(temper, &config.schadenfreude);
        mood.value = (mood.value - dip).clamp(-1.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Initialising a system is enough to trip Bevy's B0001 query-conflict
    /// check, which otherwise only fires at first run in the real app.
    #[test]
    fn no_query_of_a_system_here_fights_another() {
        use crate::bounce::testing::initialises;
        initialises(witness_wallops);
        initialises(witness_crashes);
        initialises(enjoy_the_sproings);
        initialises(feel_the_fright);
    }

    /// The ordering against `feeling` is by system, and a schedule that
    /// cannot be built — a cycle, an ambiguous reference — fails at the first
    /// frame rather than at compile time. This is that frame, with nothing in
    /// the world but the two plugins and the messages they read.
    #[test]
    fn the_audience_slots_into_the_feeling_schedule() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<GameConfig>()
            .add_message::<Wallop>()
            .add_message::<VehicleImpact>()
            .add_message::<PropSheared>()
            .add_message::<TookFright>()
            .add_plugins((feeling::FeelingPlugin, SchadenfreudePlugin));
        app.update();
        app.update();
        assert!(app.world().contains_resource::<WitnessRng>());
    }

    #[test]
    fn a_serene_onlooker_finds_a_launch_funnier_than_a_wutburger() {
        // Same mood, opposite verdicts: the split is the temperament's.
        let serene = verdict(0.0, &Temperament::serene());
        let rager = verdict(0.0, &Temperament::ragemonger());
        assert!(serene > 0.3, "serene verdict {serene}");
        assert!(rager < -0.3, "Wutbürger verdict {rager}");
        // And the mood still matters to an ordinary citizen.
        assert!(verdict(0.9, &Temperament::ordinary()) > 0.0);
        assert!(verdict(-0.9, &Temperament::ordinary()) < 0.0);
    }

    #[test]
    fn an_ordinary_street_in_an_ordinary_mood_is_split_rather_than_unanimous() {
        // The crossover sits near the ordinary citizen at their own baseline:
        // a street that all laughed, or all took offence, would have one joke.
        let ordinary = Temperament::ordinary();
        let at_rest = verdict(ordinary.baseline, &ordinary);
        assert!(at_rest.abs() < 0.2, "ordinary verdict at rest {at_rest}");
        assert!(verdict(Temperament::easygoing().baseline, &Temperament::easygoing()) > 0.0);
        assert!(verdict(Temperament::touchy().baseline, &Temperament::touchy()) < 0.0);
    }

    #[test]
    fn indignation_outweighs_the_laugh() {
        let tune = GameConfig::default().schadenfreude;
        let up = shifted(0.0, 0.5, &tune);
        let down = shifted(0.0, -0.5, &tune);
        assert!(-down > up, "a laugh of {up} against offence of {down}");
    }

    #[test]
    fn a_crash_is_funny_from_across_the_street_and_alarming_from_the_kerb() {
        let tune = GameConfig::default().schadenfreude;
        assert!(crash_stirs(1.0, 10.0, &tune) < 0.0);
        assert!(crash_stirs(12.0, 10.0, &tune) > 0.0);
        // And nothing at all past the far edge of the audience.
        assert_eq!(crash_stirs(tune.crash_radius + 1.0, 10.0, &tune), 0.0);
    }

    #[test]
    fn stepping_back_from_a_crash_never_flips_the_reading_abruptly() {
        // One curve rather than two zones: walk away from the bumper in small
        // steps and no step changes the reading by much.
        let tune = GameConfig::default().schadenfreude;
        let mut last = crash_stirs(0.0, 10.0, &tune);
        let mut distance = 0.0;
        while distance < tune.crash_radius {
            distance += 0.25;
            let here = crash_stirs(distance, 10.0, &tune);
            assert!(
                (here - last).abs() < 0.1,
                "a step to {distance} m jumped from {last} to {here}"
            );
            last = here;
        }
    }

    #[test]
    fn nobody_reacts_to_a_crash_too_soft_to_hear() {
        let tune = GameConfig::default().schadenfreude;
        assert_eq!(crash_stirs(3.0, tune.crash_stir - 0.1, &tune), 0.0);
    }

    #[test]
    fn a_heavy_meter_at_speed_delights_more_than_a_cone_nudged_over() {
        let tune = GameConfig::default().schadenfreude;
        let meter = sproing_glee(11.0, 60.0, &tune);
        let nudge = sproing_glee(4.0, 14.0, &tune);
        assert!(meter > nudge);
        assert!(nudge > 0.0);
        // Capped: a lorry-launched planter is not funnier than the cap.
        assert!(sproing_glee(100.0, 1000.0, &tune) <= tune.sproing_delight);
    }

    #[test]
    fn a_fright_stings_a_short_fuse_more_and_never_much() {
        let tune = GameConfig::default().schadenfreude;
        let serene = fright_cost(&Temperament::serene(), &tune);
        let rager = fright_cost(&Temperament::ragemonger(), &tune);
        assert!(rager > serene);
        // Small next to a taunt: running from a car is not an insult, and a
        // crowd that went red from being startled would be a city of cowards.
        let taunt = GameConfig::default().mood.taunt_bite;
        assert!(rager < taunt * 0.25, "a fright costs {rager}");
    }
}
