//! Hooking the sound bank up to the game.
//!
//! Two shapes of sound, handled differently on purpose:
//!
//! * **One-shots** are spawned per event and despawn themselves. A crash, a
//!   door, a footfall. Cheap, fire and forget.
//! * **Voices** are loops owned by an entity, living as a child of it so the
//!   spatial mixer follows the car around, and modulated every frame — engine
//!   pitch from road speed, tyre squeal from how far the tyres are actually
//!   sliding. Spawning and despawning these per event would click; they run
//!   continuously and change volume instead.
//!
//! Nothing in here writes to the simulation. If a system in this file were
//! deleted the game would play identically, in silence.

use bevy::audio::{AudioSinkPlayback, SpatialAudioSink, Volume};
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use rand::RngExt;

use super::bank::SoundBank;
use super::synth::SynthSound;
use super::{AudioRng, close_once, effect_gain, spatial_once};
use crate::ai::animal::{Cat, Dog};
use crate::bounce::launch::KnockedDown;
use crate::core::config::GameConfig;
use crate::core::schedule::GameSet;
use crate::mood::feeling::{CityMood, Mood};
use crate::player::interact::{DrivenBy, Driving};
use crate::player::on_foot::Player;
use crate::vehicle::controller::{VehicleInput, VehicleState};
use crate::vehicle::impact::VehicleImpact;
use crate::vehicle::spawn::{AlwaysSimulated, Vehicle};
use crate::world::mayhem::{Geyser, PropSheared, pressure};

/// Metres of pavement per footfall. Distance rather than time, so sprinting
/// speeds up the cadence for free.
const STRIDE: f32 = 1.75;

/// Sideways sliding speed, in m/s, at which the tyres are screaming.
///
/// `VehicleState::slip` is uncancelled lateral speed, so this is readable as a
/// physical claim: three metres a second sideways is a full-lock slide.
const FULL_SQUEAL: f32 = 3.0;
/// Below this road speed a sliding car is being shoved, not drifting.
const SQUEAL_FLOOR_KPH: f32 = 9.0;

/// Impact severity, in m/s of velocity lost, that counts as a proper crash.
const CRASH_FLOOR: f32 = 1.5;
const CRASH_FULL: f32 = 16.0;

/// Per-sound gains, so the mix is one block of numbers rather than a constant
/// buried in each system.
pub mod gain {
    pub const CRASH: f32 = 0.9;
    pub const HONK: f32 = 0.7;
    pub const WHEEE: f32 = 0.6;
    pub const SPROING: f32 = 0.75;
    pub const SPRAY: f32 = 0.5;
    pub const FOOTSTEP: f32 = 0.30;
    pub const DOOR: f32 = 0.6;
    pub const ENGINE: f32 = 0.55;
    pub const SCREECH: f32 = 0.55;
    /// The always-on ambient bed is filtered noise, and at full level the ear
    /// stops reading it as a city behind the buildings and starts reading it
    /// as the mixer hissing. Felt more than heard, as its synth promises.
    pub const TRAFFIC_BED: f32 = 0.4;
    /// The mood beds proper. Birdsong and uproar used to ride at unity, and
    /// at unity a delighted city's birds sat *on top of* everything instead
    /// of behind it — and fed the same summing distortion the traffic did.
    /// A bed is the room tone of the city; it must never compete with an
    /// event happening in it.
    pub const BIRDS_BED: f32 = 0.6;
    pub const UPROAR_BED: f32 = 0.7;
    // The zone emitters. These went *up* when the traffic band came in: with
    // engines local and the far city quiet, the places themselves — chatter
    // on a frontage, a ball on a court, the factory drone — are what carries
    // a street's character, and they were tuned to hide under noise that is
    // no longer there.
    pub const CHATTER: f32 = 0.6;
    pub const FORECOURT: f32 = 0.5;
    pub const PARK_BIRDS: f32 = 0.55;
    pub const COURT: f32 = 0.5;
    pub const INDUSTRY: f32 = 0.4;
    /// The cultural quarters' street music. Quieter than the chatter on
    /// purpose: a quarter's tune is weather, not a performance — the busker
    /// is the performance, and he must still win his own street.
    pub const QUARTER: f32 = 0.35;
    pub const BARK: f32 = 0.55;
    pub const MEOW: f32 = 0.5;
}

/// How much of a vehicle's voice survives its distance from the player.
///
/// Rodio attenuates by inverse square and never actually reaches zero, and
/// with a district's worth of traffic simulated at once those leftover tails
/// sum into a permanent grey wash under everything. So distance gets a second,
/// steeper hand on the fader: full inside `CLEAR`, genuinely nothing past
/// `GONE`, cubed in between so the drop accelerates on the way out.
///
/// The band used to run 30..55 with a square. That was quiet enough per
/// engine, but rodio has no limiter: every audible source sums linearly, and
/// a junction's worth of engines plus the beds pushed the sum past full scale
/// — heard as crackling distortion, not as loudness. The cure for clipping is
/// fewer things audible at once, so the band came in hard and the curve got
/// a third power. An engine is now a *local* fact; the city at large is the
/// ambience beds' job, which is what they are for.
pub fn hush(distance: f32) -> f32 {
    /// Inside this, the mixer's own attenuation is the whole story.
    const CLEAR: f32 = 14.0;
    /// Beyond this a running engine is scenery, not sound.
    const GONE: f32 = 38.0;
    let fade = ((GONE - distance) / (GONE - CLEAR)).clamp(0.0, 1.0);
    fade * fade * fade
}

/// The same second fader for a *place*, on a longer leash than `hush`.
///
/// The zone emitters are deliberately not held to the vehicle band: a park
/// should already sound like a park from across the street, because the
/// place's mood arriving before the place is most of what an ambience is
/// for. The `EMITTER_CHOIR` cap keeps the long tail from ever piling up the
/// way traffic did.
pub fn linger(distance: f32) -> f32 {
    /// A place fills its own lot at full strength...
    const CLEAR: f32 = 25.0;
    /// ...and has faded to genuinely nothing a long block away.
    const GONE: f32 = 80.0;
    let fade = ((GONE - distance) / (GONE - CLEAR)).clamp(0.0, 1.0);
    fade * fade
}

/// A looping sound belonging to a vehicle.
#[derive(Component)]
struct Voice {
    owner: Entity,
    kind: VoiceKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VoiceKind {
    Engine,
    Screech,
}

/// One of the three ambient beds, spawned once and never despawned. The mixer
/// crossfades between them on the city's average mood: birds when the city is
/// pleased with itself, distant traffic when it is nothing in particular, and
/// a demonstration somewhere behind the buildings when it has had enough.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Ambience {
    Traffic,
    Birdsong,
    Uproar,
}

/// A place's own loop: restaurant chatter on a frontage, forecourt hum under
/// a filling station's canopy, birdsong over a park. Spawned per block by
/// `world::buildings::spawn_block` with `ChunkOf`, so a place's sound streams
/// in and out with its geometry.
#[derive(Component)]
pub struct AmbienceEmitter {
    /// This emitter's slot in the mix, against the `gain` table's scale.
    pub gain: f32,
}

/// How many zone emitters may be audible at once. A street corner with a
/// restaurant, a forecourt and a park all in earshot must not stack five
/// loops — the same reasoning as the voice choir, at the scale of places.
const EMITTER_CHOIR: usize = 4;

/// Keeps the nearest few places audible and the rest muted.
///
/// Same two-pass shape as `mood::voice::speak_up`: rank by distance from the
/// listener, then touch every sink once. Distance is measured from the
/// camera, where the `SpatialListener` actually sits.
fn tend_emitters(
    config: Res<GameConfig>,
    listeners: Query<&GlobalTransform, With<crate::player::camera::CameraRig>>,
    mut emitters: Query<(
        Entity,
        &GlobalTransform,
        &AmbienceEmitter,
        &mut SpatialAudioSink,
    )>,
) {
    let Ok(listener) = listeners.single() else {
        return;
    };
    let ears = listener.translation();
    let base = config.audio.master * config.audio.ambience;

    let mut near: Vec<(f32, Entity)> = emitters
        .iter()
        .map(|(entity, at, ..)| (at.translation().distance(ears), entity))
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.truncate(EMITTER_CHOIR);
    let audible: Vec<Entity> = near.into_iter().map(|(_, entity)| entity).collect();

    for (entity, at, emitter, mut sink) in &mut emitters {
        let level = if audible.contains(&entity) {
            base * emitter.gain * linger(at.translation().distance(ears))
        } else {
            0.0
        };
        sink.set_volume(Volume::Linear(level));
        // A muted sink remembers its volume, so unmuting lands on the level
        // just set rather than on last week's.
        if level > 0.001 {
            if sink.is_muted() {
                sink.unmute();
            }
        } else if !sink.is_muted() {
            sink.mute();
        }
    }
}

/// The animals speak. A dog barks about how it feels — the further its mood
/// is from level, the more it has to say, delighted or furious alike. A cat
/// almost never says anything, which is the correct amount; when it does,
/// the meow is addressed to nobody and means nothing.
fn play_animal_voices(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    mut rng: ResMut<AudioRng>,
    dogs: Query<(&Dog, &Transform, &Mood)>,
    cats: Query<(&Cat, &Transform)>,
) {
    let dt = time.delta_secs();
    for (dog, here, mood) in &dogs {
        let eagerness = 0.03 + 0.22 * mood.value.abs();
        if rng.random::<f32>() < eagerness * dt {
            at(
                &mut commands,
                bank.bark.clone(),
                here.translation,
                spatial_once(effect_gain(&config, gain::BARK), 18.0).with_speed(dog.pitch),
            );
        }
    }
    for (cat, here) in &cats {
        if rng.random::<f32>() < 0.008 * dt {
            at(
                &mut commands,
                bank.meow.clone(),
                here.translation,
                spatial_once(effect_gain(&config, gain::MEOW), 12.0).with_speed(cat.pitch),
            );
        }
    }
}

pub struct SfxPlugin;

impl Plugin for SfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                start_ambience,
                (
                    play_impacts,
                    play_honks,
                    play_wheees,
                    play_sproings,
                    voice_geysers,
                    play_doors,
                    play_footsteps,
                    manage_vehicle_voices,
                    update_vehicle_voices,
                    update_ambience,
                    tend_emitters,
                    play_animal_voices,
                )
                    .in_set(GameSet::Simulation),
                // After the lot of them, and after `mood::voice`, which
                // spawns its one-shots in `Ai`.
                cap_one_shots.in_set(GameSet::Ui),
            )
                // The bank is loaded in `Startup`; nothing here can run
                // before it lands.
                .run_if(resource_exists::<SoundBank>),
        );
    }
}

// ------------------------------------------------------------- one-shots ----

/// How many one-shots may be sounding at the same time.
///
/// The loops are all bounded now — three beds, four places, three engines,
/// eight opinions — and one-shots were the last category with no ceiling on
/// it at all. They are spawned from seven modules and twenty-odd call sites,
/// and none of them can know what the other six are doing: a car landing in
/// a crowd is a crash, a honk, four sproings, a dozen gasps and everybody's
/// footsteps, all inside the same tenth of a second.
///
/// Capped here rather than at the call sites, because here is the one place
/// every one-shot in the game is guaranteed to pass. The newest are the ones
/// dropped, which for a pile-up is the right end: what is already sounding
/// is what the player has already started hearing.
const ONE_SHOT_CHOIR: usize = 10;

/// Refuses a one-shot that would be the eleventh thing going off at once.
///
/// Bevy starts queued playback in `PostUpdate`, so within a frame the ones
/// spawned this tick have no sink yet and the ones already sounding do —
/// which is exactly the distinction this needs, and is why the system can
/// run anywhere in `Update`.
fn cap_one_shots(
    mut commands: Commands,
    sounding: Query<
        &PlaybackSettings,
        Or<(
            With<bevy::audio::AudioSink>,
            With<bevy::audio::SpatialAudioSink>,
        )>,
    >,
    fresh: Query<(Entity, &PlaybackSettings), Added<AudioPlayer<SynthSound>>>,
) {
    use bevy::audio::PlaybackMode;

    // `PlaybackMode` carries no `PartialEq`, hence the match.
    let one_shot = |settings: &PlaybackSettings| matches!(settings.mode, PlaybackMode::Despawn);
    let playing = sounding.iter().filter(|s| one_shot(s)).count();
    let mut budget = ONE_SHOT_CHOIR.saturating_sub(playing);
    for (entity, settings) in &fresh {
        if !one_shot(settings) {
            continue;
        }
        match budget {
            0 => commands.entity(entity).despawn(),
            _ => budget -= 1,
        }
    }
}

fn at(
    commands: &mut Commands,
    sound: Handle<SynthSound>,
    position: Vec3,
    settings: PlaybackSettings,
) {
    commands.spawn((
        AudioPlayer(sound),
        settings,
        Transform::from_translation(position),
    ));
}

fn play_impacts(
    mut commands: Commands,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    mut impacts: MessageReader<VehicleImpact>,
) {
    for impact in impacts.read() {
        if impact.severity < CRASH_FLOOR {
            continue;
        }
        // Loudness follows how hard the car actually stopped, so a scrape and a
        // head-on are the same sound played with different force.
        let force = (impact.severity / CRASH_FULL).clamp(0.25, 1.0);
        at(
            &mut commands,
            bank.crash.clone(),
            impact.position,
            spatial_once(effect_gain(&config, gain::CRASH * force), 22.0)
                // Heavier hits ring lower.
                .with_speed(1.15 - force * 0.3),
        );
    }
}

/// Impact severity above which the offended car honks about it.
const HONK_FLOOR: f32 = 3.0;

/// The indignant honk after a crash.
///
/// Not every crash: a horn that answers every scrape is a metronome, and the
/// joke needs room to land. The pause between the bang and the honk is baked
/// into the sound itself — see `bank::honk`.
fn play_honks(
    mut commands: Commands,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    mut rng: ResMut<AudioRng>,
    mut impacts: MessageReader<VehicleImpact>,
) {
    for impact in impacts.read() {
        if impact.severity < HONK_FLOOR || rng.random::<f32>() > 0.6 {
            continue;
        }
        at(
            &mut commands,
            bank.honk.clone(),
            impact.position,
            spatial_once(effect_gain(&config, gain::HONK), 20.0)
                // Every car has its own voice, near enough.
                .with_speed(0.85 + rng.random::<f32>() * 0.35),
        );
    }
}

/// The twang of street furniture leaving its footing.
fn play_sproings(
    mut commands: Commands,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    mut rng: ResMut<AudioRng>,
    mut sheared: MessageReader<PropSheared>,
) {
    for shear in sheared.read() {
        at(
            &mut commands,
            bank.sproing.clone(),
            shear.position,
            spatial_once(effect_gain(&config, gain::SPROING), 18.0)
                // A parking meter and a phone box do not twang at the same
                // pitch, and the ear notices even if it cannot say why.
                .with_speed(0.85 + rng.random::<f32>() * 0.4),
        );
    }
}

/// Puts the spray loop on every geyser, and lets it die with the pressure.
///
/// The sink rides the geyser entity itself, so the spatial mixer follows the
/// stump and the loop stops the moment the geyser despawns — with the chunk
/// or with its own timer, either way for free.
fn voice_geysers(
    mut commands: Commands,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    fresh: Query<Entity, (With<Geyser>, Without<SpatialAudioSink>)>,
    mut running: Query<(&Geyser, &mut SpatialAudioSink)>,
) {
    for geyser in &fresh {
        commands.entity(geyser).insert((
            AudioPlayer(bank.spray.clone()),
            // Muted for the same reason the vehicle voices start muted: the
            // first frame must not blare before the level below has run once.
            PlaybackSettings::LOOP.with_spatial(true).muted(),
        ));
    }
    for (geyser, mut sink) in &mut running {
        let level = effect_gain(&config, gain::SPRAY) * pressure(geyser.life.fraction());
        sink.set_volume(Volume::Linear(level));
        if level > 0.001 && sink.is_muted() {
            sink.unmute();
        }
    }
}

/// The slide whistle for anybody who has just been put in the air.
///
/// `Added<KnockedDown>` rather than the launch itself, so it covers every way
/// a body leaves the ground unwillingly — bumpers, grudges, whatever comes
/// next — without each of them having to remember the orchestra.
fn play_wheees(
    mut commands: Commands,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    mut rng: ResMut<AudioRng>,
    launched: Query<&Transform, Added<KnockedDown>>,
) {
    for transform in &launched {
        at(
            &mut commands,
            bank.wheee.clone(),
            transform.translation,
            spatial_once(effect_gain(&config, gain::WHEEE), 20.0)
                .with_speed(0.9 + rng.random::<f32>() * 0.3),
        );
    }
}

fn play_doors(
    mut commands: Commands,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    entered: Query<&Transform, Added<DrivenBy>>,
    mut vacated: RemovedComponents<DrivenBy>,
    transforms: Query<&Transform>,
) {
    let slam = |position: Vec3, commands: &mut Commands| {
        at(
            commands,
            bank.car_door.clone(),
            position,
            spatial_once(effect_gain(&config, gain::DOOR), 14.0),
        );
    };

    for transform in &entered {
        slam(transform.translation, &mut commands);
    }
    for vehicle in vacated.read() {
        // A car can lose its driver by being despawned out from under them —
        // streaming, mostly. A despawn is not a door.
        if let Ok(transform) = transforms.get(vehicle) {
            slam(transform.translation, &mut commands);
        }
    }
}

fn play_footsteps(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<GameConfig>,
    bank: Res<SoundBank>,
    mut rng: ResMut<AudioRng>,
    mut travelled: Local<f32>,
    players: Query<&avian3d::prelude::LinearVelocity, (With<Player>, Without<Driving>)>,
) {
    let Ok(velocity) = players.single() else {
        *travelled = 0.0;
        return;
    };

    let pace = velocity.0.xz().length();
    // Airborne, or barely moving: no feet on the ground to hear.
    if pace < 0.6 || velocity.0.y.abs() > 2.5 {
        return;
    }

    *travelled += pace * time.delta_secs();
    if *travelled < STRIDE {
        return;
    }
    *travelled -= STRIDE;

    commands.spawn((
        AudioPlayer(bank.footstep.clone()),
        close_once(effect_gain(&config, gain::FOOTSTEP))
            // Every footfall lands slightly differently. Without this the walk
            // cycle turns into a drum machine within three paces.
            .with_speed(0.90 + rng.random::<f32>() * 0.22),
    ));
}

// ---------------------------------------------------------------- voices ----

/// Engine pitch from road speed and throttle, as a playback speed multiplier.
///
/// A single loop pitched by road speed alone would climb to a scream on the
/// motorway and sit there. Real cars change gear, and faking the gearbox —
/// revs rising through a band, then dropping back as the next ratio takes over
/// — is most of what makes a car sound like it is being *driven* rather than
/// merely moving.
pub fn engine_pitch(speed_kph: f32, throttle: f32) -> f32 {
    /// Road speed each gear runs out at, in km/h.
    const GEARS: [f32; 5] = [26.0, 50.0, 82.0, 122.0, 180.0];

    let speed = speed_kph.abs();
    let gear = GEARS
        .iter()
        .position(|&top| speed < top)
        .unwrap_or(GEARS.len() - 1);
    let bottom = if gear == 0 { 0.0 } else { GEARS[gear - 1] };
    let through = ((speed - bottom) / (GEARS[gear] - bottom)).clamp(0.0, 1.0);

    // Idle, plus the revs earned within this gear, plus a lift for load: a car
    // held on the throttle sounds busier than one coasting at the same speed.
    0.62 + through * 0.92 + throttle.max(0.0) * 0.14
}

/// Adds a loop set to every vehicle that should be making noise, and takes them
/// away again when it stops.
///
/// A car qualifies if something is driving it: the player or traffic, both of
/// which are exempt from distance culling. Several hundred cars are parked
/// around the city and none of them has its engine running.
fn manage_vehicle_voices(
    mut commands: Commands,
    bank: Res<SoundBank>,
    driven: Query<Entity, (With<Vehicle>, Or<(With<DrivenBy>, With<AlwaysSimulated>)>)>,
    voices: Query<(Entity, &Voice)>,
) {
    let running: HashSet<Entity> = driven.iter().collect();

    let mut voiced: HashSet<Entity> = HashSet::default();
    for (entity, voice) in &voices {
        if running.contains(&voice.owner) {
            voiced.insert(voice.owner);
        } else {
            commands.entity(entity).despawn();
        }
    }

    for vehicle in &driven {
        if voiced.contains(&vehicle) {
            continue;
        }
        // Started muted rather than silent-by-volume so the first frame cannot
        // blare before the modulation systems have run.
        let looping = PlaybackSettings::LOOP.with_spatial(true).muted();
        // The identity transform is load-bearing: it brings `GlobalTransform`
        // with it, and a spatial source without one is mixed at the origin.
        let place = Transform::default();
        commands.entity(vehicle).with_children(|car| {
            car.spawn((
                Voice {
                    owner: vehicle,
                    kind: VoiceKind::Engine,
                },
                AudioPlayer(bank.engine.clone()),
                looping,
                place,
            ));
            car.spawn((
                Voice {
                    owner: vehicle,
                    kind: VoiceKind::Screech,
                },
                AudioPlayer(bank.screech.clone()),
                looping,
                place,
            ));
        });
    }
}

/// How many cars may be heard at once.
///
/// `hush` already makes an engine a local fact, but "local" is not "one":
/// a junction can put six cars inside the band at the same moment, and six
/// engines at their working level sum past full scale on their own. So the
/// nearest few are the traffic and the rest are the ambience bed's problem
/// — the same bargain `EMITTER_CHOIR` strikes for places and `voice::CHOIR`
/// for opinions.
const ENGINE_CHOIR: usize = 3;

fn update_vehicle_voices(
    config: Res<GameConfig>,
    listeners: Query<&GlobalTransform, With<crate::player::camera::CameraRig>>,
    vehicles: Query<(Entity, &VehicleState, &VehicleInput, &Transform)>,
    mut voices: Query<(&Voice, &mut SpatialAudioSink)>,
) {
    // Measured from the camera, because that is where the `SpatialListener`
    // sits — this used to measure from the player, and in the free camera the
    // two faders disagreed about which cars were near.
    let ears = listeners
        .single()
        .map(|listener| listener.translation())
        .unwrap_or_default();

    // Two passes, the same shape as `tend_emitters`: rank, then touch every
    // sink once. Both of a car's voices ride on one decision, so an engine
    // and its tyres never disagree about whether that car is being heard.
    let mut near: Vec<(f32, Entity)> = vehicles
        .iter()
        .map(|(entity, _, _, at)| (at.translation.distance(ears), entity))
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.truncate(ENGINE_CHOIR);
    let audible: HashSet<Entity> = near.into_iter().map(|(_, entity)| entity).collect();

    for (voice, mut sink) in &mut voices {
        let Ok((_, state, input, at)) = vehicles.get(voice.owner) else {
            continue;
        };
        let heard = if audible.contains(&voice.owner) {
            hush(at.translation.distance(ears))
        } else {
            0.0
        };
        let speed_kph = state.speed_kph();

        let level = match voice.kind {
            VoiceKind::Engine => {
                sink.set_speed(engine_pitch(speed_kph, input.throttle));
                // Idling is audible; working is loud.
                let load = input.throttle.abs().max((speed_kph / 70.0).min(1.0) * 0.6);
                effect_gain(&config, gain::ENGINE) * (0.35 + 0.65 * load)
            }
            VoiceKind::Screech => {
                let sliding = if speed_kph < SQUEAL_FLOOR_KPH {
                    0.0
                } else {
                    (state.slip / FULL_SQUEAL).clamp(0.0, 1.0)
                };
                // Tyres go up in pitch as they let go, not just up in volume.
                sink.set_speed(0.88 + sliding * 0.3);
                effect_gain(&config, gain::SCREECH) * sliding
            }
        };

        // A muted sink still remembers its volume, so unmuting lands on the
        // right level rather than on whatever it was before.
        let level = level * heard;
        sink.set_volume(Volume::Linear(level));
        if level > 0.001 {
            if sink.is_muted() {
                sink.unmute();
            }
        } else if !sink.is_muted() {
            sink.mute();
        }
    }
}

// -------------------------------------------------------------- ambience ----

fn start_ambience(
    mut commands: Commands,
    bank: Res<SoundBank>,
    existing: Query<(), With<Ambience>>,
) {
    if !existing.is_empty() {
        return;
    }
    for (name, bed, sound) in [
        ("City ambience", Ambience::Traffic, &bank.ambience),
        ("Birdsong", Ambience::Birdsong, &bank.birdsong),
        ("Uproar", Ambience::Uproar, &bank.uproar),
    ] {
        commands.spawn((
            Name::new(name),
            bed,
            AudioPlayer(sound.clone()),
            // Muted until the first mix pass, so no bed blares at full
            // synthesis level for a frame before the mood is read.
            PlaybackSettings::LOOP.muted(),
        ));
    }
}

/// How loud each ambient bed is at a given city mood, −1 to 1: (traffic,
/// birdsong, uproar).
///
/// Pure, so the crossfade can be argued about without ears. The dead band
/// around neutral is deliberate: an ordinary day is traffic and nothing else,
/// and the first birds arriving are *news* — they say the street has actually
/// warmed up, not that the average twitched past zero.
pub fn ambience_mix(mood: f32) -> (f32, f32, f32) {
    let mood = mood.clamp(-1.0, 1.0);
    let birds = ((mood - 0.1) / 0.7).clamp(0.0, 1.0);
    let uproar = ((-mood - 0.1) / 0.7).clamp(0.0, 1.0);
    // The rumble never quite leaves — the city is still a city under the
    // birds — but it makes room for whichever pole is playing.
    let traffic = 1.0 - 0.6 * birds.max(uproar);
    (traffic, birds, uproar)
}

fn update_ambience(
    config: Res<GameConfig>,
    city: Res<CityMood>,
    mut beds: Query<(&Ambience, &mut bevy::audio::AudioSink)>,
) {
    let (traffic, birds, uproar) = ambience_mix(city.average);
    let base = config.audio.master * config.audio.ambience;
    for (bed, mut sink) in &mut beds {
        let level = match bed {
            Ambience::Traffic => traffic * gain::TRAFFIC_BED,
            Ambience::Birdsong => birds * gain::BIRDS_BED,
            Ambience::Uproar => uproar * gain::UPROAR_BED,
        };
        sink.set_volume(Volume::Linear(base * level));
        // A muted sink remembers its volume, so unmuting lands on the level
        // just set rather than on last week's.
        if base * level > 0.001 {
            if sink.is_muted() {
                sink.unmute();
            }
        } else if !sink.is_muted() {
            sink.mute();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revs_climb_through_a_gear_and_drop_at_the_change() {
        let pitch = |kph| engine_pitch(kph, 1.0);
        assert!(
            pitch(20.0) > pitch(5.0),
            "revs should rise within first gear"
        );
        assert!(
            pitch(28.0) < pitch(24.0),
            "the shift into second should drop the revs"
        );
        assert!(
            pitch(52.0) < pitch(48.0),
            "and so should the shift into third"
        );
    }

    #[test]
    fn revs_stay_in_a_playable_band_at_any_speed() {
        // Outside roughly half to double speed, resampling a loop stops
        // sounding like an engine and starts sounding like a fault.
        for kph in 0..400 {
            for throttle in [-1.0, 0.0, 1.0] {
                let pitch = engine_pitch(kph as f32, throttle);
                assert!(
                    (0.5..=2.0).contains(&pitch),
                    "{kph}km/h at throttle {throttle} gives {pitch}"
                );
            }
        }
    }

    #[test]
    fn the_ambience_follows_the_city_from_birds_to_barricades() {
        let (traffic, birds, uproar) = ambience_mix(0.0);
        assert_eq!(
            (birds, uproar),
            (0.0, 0.0),
            "an ordinary day is traffic and nothing else"
        );
        assert_eq!(traffic, 1.0);

        let (_, birds, uproar) = ambience_mix(0.9);
        assert!(birds > 0.9, "a delighted city should be full of birds");
        assert_eq!(uproar, 0.0, "and demonstrating about nothing");

        let (_, birds, uproar) = ambience_mix(-0.9);
        assert!(uproar > 0.9, "a furious city should be on the barricades");
        assert_eq!(birds, 0.0, "with every bird long gone");

        for mood in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            assert!(
                ambience_mix(mood).0 > 0.3,
                "the city is still a city at mood {mood}"
            );
        }
    }

    #[test]
    fn traffic_a_street_away_is_silent_rather_than_a_permanent_hiss() {
        // Inverse square alone leaves every simulated engine a few percent
        // audible forever, and thirty of those sum to a noise floor. The hush
        // has to reach an actual zero, and reach it faster than a straight
        // line so leaving earshot sounds like leaving rather than dimming.
        assert_eq!(hush(0.0), 1.0);
        assert_eq!(hush(12.0), 1.0, "close traffic is the mixer's business");
        assert_eq!(hush(40.0), 0.0);
        assert_eq!(hush(300.0), 0.0);
        let midway = hush(26.0);
        assert!(
            midway > 0.0 && midway < 0.25,
            "halfway out should be well under a quarter as loud, got {midway}"
        );
        // Rodio sums every audible source with no limiter, so the sum of a
        // junction's worth of engines is bounded by how many the band lets
        // through at all — at 30m, where the old band still passed a third,
        // an engine must now be nearly gone.
        assert!(hush(30.0) < 0.05, "got {}", hush(30.0));
    }

    #[test]
    fn a_place_carries_further_than_an_engine() {
        // The whole point of splitting `linger` off `hush`: the mood of a
        // place should arrive before the place does, while traffic stays a
        // local fact.
        assert_eq!(linger(0.0), 1.0);
        assert!(linger(35.0) > hush(35.0), "a park outlasts an engine");
        assert!(linger(35.0) > 0.3, "audible from across the street");
        assert_eq!(linger(85.0), 0.0, "but a long block away it is gone");
    }

    #[test]
    fn a_stationary_car_still_idles() {
        assert!(engine_pitch(0.0, 0.0) > 0.5, "the engine is still running");
        // Reverse is still the engine turning forwards.
        assert_eq!(engine_pitch(-20.0, 0.0), engine_pitch(20.0, 0.0));
    }
}
