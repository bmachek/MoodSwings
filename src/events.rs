//! The city throws events.
//!
//! Two regimes were planned and one turned out to already exist: *emergent*
//! street theatre — two touchy citizens locked in a taunt exchange, a grudge
//! chase through a crowd — falls out of `mood::provoke` and `mood::grudge`
//! without a line of code here. What this module adds is the *scheduled*
//! kind: parades, on the calendar, at an hour, down a route.
//!
//! Scheduling follows the weather's determinism recipe rather than mayhem's:
//! the programme is a pure function of (seed, day) via `key_for`, never a
//! drawn stream, so a parade at 14:00 on this seed is a fact about the seed —
//! reproducible, and freezable with `--hour` like everything else the capture
//! harness pins. The marchers themselves are ordinary citizens with full
//! moods, which is the whole trick: a CSD parade *is* a rolling cheer,
//! mechanically — delight radiating from the column into every street it
//! passes — and a Demo is the same column with its sign flipped.
//!
//! Events sit out capture mode: an unattended screenshot must not acquire a
//! parade.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::ai::archetype::Archetype;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::config::GameConfig;
use crate::core::rng::{key_for, stream};
use crate::core::schedule::GameSet;
use crate::mood::face::{FaceAssets, FaceLevel};
use crate::mood::feeling::{Mood, Temperament};
use crate::mood::provoke::Provoker;
use crate::mood::voice::Voicebox;
use crate::world::City;
use crate::world::buildings::SIDEWALK_HEIGHT;
use crate::world::roadgraph::NodeId;
use crate::world::timeofday::TimeOfDay;

/// How long a parade stays on the streets, in game hours.
const DURATION: f32 = 1.6;
/// How many march in the column.
const MARCHERS: usize = 14;
/// The column's pace. Brisker than a stroll, well under a flee: a parade you
/// cannot walk alongside is a chase.
const MARCH_PACE: f32 = 1.7;
/// A marcher this close to its waypoint takes the next one.
const ARRIVED: f32 = 4.0;
/// The column only actually forms if the player is near enough to ever see
/// it; a parade nobody attends stays a line in the listings.
const ATTENDANCE: f32 = 500.0;

/// What kind of day it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// Rainbow coats, top-of-scale moods, and the contagion doing the rest:
    /// mechanically a rolling cheer through every street on the route.
    Csd,
    /// The same column, furious. The uproar bed and the rage wave banner
    /// react on their own, because the marchers are real citizens.
    Demo,
}

impl EventKind {
    /// The HUD line, player-facing and therefore German.
    fn announcement(self) -> &'static str {
        match self {
            EventKind::Csd => "CSD! Die Parade zieht durch die Stadt",
            EventKind::Demo => "Demo! Die Stadt ist auf der Straße",
        }
    }

    fn baseline(self) -> f32 {
        match self {
            EventKind::Csd => 0.85,
            EventKind::Demo => -0.7,
        }
    }
}

/// A well-mixed roll in 0..1 for one question about one day.
///
/// The splitmix64 finaliser, because the first draft used a cheap FNV fold
/// and its answers to salt 1 and salt 2 were correlated enough that sixty
/// days of calendar contained no demo at all. Avalanche is not a luxury.
fn day_roll(seed: u64, day: u32, salt: u64) -> f32 {
    let mut h = key_for(seed, stream::EVENTS) ^ (salt << 32) ^ day as u64;
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// The day's programme: what happens today and when it starts, or a quiet day.
///
/// Pure, so the same seed schedules the same week — the determinism test
/// leans on this directly.
pub fn programme(seed: u64, day: u32) -> Option<(EventKind, f32)> {
    // A little over half the days have something on. Every day would make
    // parades wallpaper; the point of a parade is the day it is not there.
    if day_roll(seed, day, 1) > 0.55 {
        return None;
    }
    let kind = if day_roll(seed, day, 2) < 0.5 {
        EventKind::Csd
    } else {
        EventKind::Demo
    };
    // Between late morning and early evening, when the streets have people
    // in them to infect.
    let start = 10.0 + day_roll(seed, day, 3) * 8.0;
    Some((kind, start))
}

/// The route: an A* path between two far-apart junctions, both derived from
/// the day. The road graph guarantees consecutive nodes share an edge.
pub fn route(city: &City, seed: u64, day: u32) -> Vec<NodeId> {
    // Polar rather than a random square point: the radius floor is what
    // guarantees the march is a march — a start near the origin made day
    // zero's route three hundred metres long.
    let angle = day_roll(seed, day, 4) * std::f32::consts::TAU;
    let radius = city.half_extent * (0.45 + day_roll(seed, day, 5) * 0.3);
    let from = Vec2::new(angle.cos(), angle.sin()) * radius;
    // Marching *across* the city rather than to a random second point: the
    // far side of town is the one destination that guarantees a long route.
    let to = -from;
    let (Some(a), Some(b)) = (city.graph.nearest_node(from), city.graph.nearest_node(to)) else {
        return Vec::new();
    };
    city.graph.path(a, b).unwrap_or_default()
}

/// On one member of the column.
#[derive(Component)]
struct Marcher {
    /// Index into the route.
    next: usize,
    /// Lateral slot, so the column is a block rather than a conga line.
    file: f32,
}

/// The running event, if any.
#[derive(Resource, Default)]
struct ActiveEvent(Option<Happening>);

struct Happening {
    kind: EventKind,
    route: Vec<NodeId>,
    ends: f32,
    formed: bool,
}

/// What the HUD banner should say right now. Read by `ui::hud`.
#[derive(Resource, Default)]
pub struct EventBanner(pub Option<(String, EventKind)>);

/// Which game day it is. The clock only carries hours; days are counted here
/// by watching it wrap.
#[derive(Resource, Default)]
struct EventDay {
    day: u32,
    previous_hours: f32,
}

/// A capture run that *asked* for a parade: `--event csd` / `--event demo`.
///
/// The only door through the events-sit-out-capture rule, and the way a
/// parade gets screenshotted at all: day zero's route, started the moment the
/// harness's pinned clock allows, attendance waived because the camera is
/// the audience.
pub fn forced() -> Option<EventKind> {
    let args: Vec<String> = std::env::args().collect();
    let flag = args.iter().position(|a| a == "--event")?;
    match args.get(flag + 1).map(String::as_str) {
        Some("csd") => Some(EventKind::Csd),
        Some("demo") => Some(EventKind::Demo),
        other => {
            eprintln!("--event wants csd or demo, not {other:?}");
            None
        }
    }
}

/// The column's wardrobe, built once.
#[derive(Resource)]
struct MarchAssets {
    rainbow: Vec<Handle<StandardMaterial>>,
    banner_grey: Handle<StandardMaterial>,
}

pub struct EventsPlugin;

impl Plugin for EventsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveEvent>()
            .init_resource::<EventBanner>()
            .init_resource::<EventDay>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (schedule_events, form_the_column, march)
                    .chain()
                    .in_set(GameSet::Ai)
                    // After the pavement logic so a marching order written
                    // here is the one the body obeys — the module-doc
                    // contract `ai::pedestrian` publishes.
                    .after(crate::ai::pedestrian::Walking),
            );
    }
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let cloth = |materials: &mut Assets<StandardMaterial>, color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.85,
            ..default()
        })
    };
    commands.insert_resource(MarchAssets {
        rainbow: [
            Color::srgb(0.86, 0.18, 0.18),
            Color::srgb(0.92, 0.55, 0.12),
            Color::srgb(0.92, 0.84, 0.18),
            Color::srgb(0.22, 0.68, 0.28),
            Color::srgb(0.20, 0.42, 0.86),
            Color::srgb(0.55, 0.26, 0.72),
        ]
        .into_iter()
        .map(|color| cloth(&mut materials, color))
        .collect(),
        banner_grey: cloth(&mut materials, Color::srgb(0.42, 0.42, 0.45)),
    });
}

/// Watches the calendar. Starting and ending an event is all this does; the
/// marchers are somebody else's problem below.
fn schedule_events(
    clock: Res<TimeOfDay>,
    config: Res<GameConfig>,
    mut commands: Commands,
    mut day: ResMut<EventDay>,
    mut active: ResMut<ActiveEvent>,
    mut banner: ResMut<EventBanner>,
    city: Res<City>,
    marchers: Query<Entity, With<Marcher>>,
) {
    // An unattended screenshot must not acquire a parade — unless it asked.
    if crate::core::capture::is_capture_mode() {
        if let Some(kind) = forced()
            && active.0.is_none()
        {
            let route = route(&city, config.world_seed, 0);
            if route.len() < 4 {
                return;
            }
            let start = city.graph.node(route[0]).pos;
            info!("forced {kind:?} for capture; column forms at {start}");
            banner.0 = Some((kind.announcement().to_string(), kind));
            active.0 = Some(Happening {
                kind,
                route,
                ends: clock.hours + DURATION,
                formed: false,
            });
        }
        return;
    }

    // The clock wrapping past midnight is the page of the calendar turning.
    if clock.hours < day.previous_hours - 12.0 {
        day.day += 1;
    }
    day.previous_hours = clock.hours;

    if let Some(happening) = &active.0 {
        // Over: send everybody home (that is, away) and take the banner down.
        if clock.hours > happening.ends || clock.hours < happening.ends - DURATION - 0.5 {
            for marcher in &marchers {
                commands.entity(marcher).despawn();
            }
            active.0 = None;
            banner.0 = None;
        }
        return;
    }

    let Some((kind, start)) = programme(config.world_seed, day.day) else {
        return;
    };
    if clock.hours < start || clock.hours > start + DURATION {
        return;
    }
    let route = route(&city, config.world_seed, day.day);
    if route.len() < 4 {
        return;
    }
    info!("{kind:?} scheduled for day {} is on the streets", day.day);
    banner.0 = Some((kind.announcement().to_string(), kind));
    active.0 = Some(Happening {
        kind,
        route,
        ends: start + DURATION,
        formed: false,
    });
}

/// Forms the column, once, when there is somebody around to see it.
fn form_the_column(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut active: ResMut<ActiveEvent>,
    city: Res<City>,
    assets: Res<MarchAssets>,
    figures: Res<crate::ai::figure::FigureAssets>,
    faces: Res<FaceAssets>,
    players: Query<&Transform, With<crate::player::on_foot::Player>>,
) {
    let Some(happening) = &mut active.0 else {
        return;
    };
    if happening.formed {
        return;
    }
    let Ok(player) = players.single() else { return };
    let start = city.graph.node(happening.route[0]).pos;
    // The camera is the audience in a forced capture; nobody else need be.
    if !crate::core::capture::is_capture_mode()
        && player.translation.xz().distance(start) > ATTENDANCE
    {
        return;
    }
    happening.formed = true;

    let towards = city.graph.node(happening.route[1]).pos - start;
    let direction = towards.normalize_or_zero();
    let right = Vec2::new(-direction.y, direction.x);
    let mood = happening.kind.baseline();
    let worn = faces.wear(mood);

    for index in 0..MARCHERS {
        // Two abreast, staggered back down the road.
        let file = if index % 2 == 0 { -0.9 } else { 0.9 };
        let rank = (index / 2) as f32 * 1.6;
        let at = start - direction * rank + right * file;

        let coat = match happening.kind {
            EventKind::Csd => assets.rainbow[index % assets.rainbow.len()].clone(),
            EventKind::Demo => assets.banner_grey.clone(),
        };
        // Deterministic per column slot; nothing here draws from a stream.
        let mut wardrobe =
            crate::core::rng::stream_for(config.world_seed ^ index as u64, stream::EVENTS);
        let temper = match happening.kind {
            EventKind::Csd => Temperament::easygoing(),
            EventKind::Demo => Temperament::ragemonger(),
        };

        let mut marcher = commands.spawn((
            Name::new("Marcher"),
            Marcher {
                next: 1,
                file: file * 1.5,
            },
            Transform::from_xyz(at.x, SIDEWALK_HEIGHT + 0.845, at.y),
            RigidBody::Dynamic,
            Collider::capsule(0.32, 1.05),
            LockedAxes::ROTATION_LOCKED,
            Bouncer::new(0.845),
            temper,
            Mood::new(mood),
            FaceLevel(worn.level),
            Voicebox::new(0.85 + (index as f32 * 0.61803) % 0.4),
            Provoker::default(),
            Archetype::Everyday,
            Visibility::default(),
        ));
        crate::ai::figure::dress(
            &mut marcher,
            &figures,
            coat,
            &worn,
            Archetype::Everyday,
            &mut wardrobe,
        );
    }
}

/// Keeps the column marching down its route.
fn march(
    mut commands: Commands,
    config: Res<GameConfig>,
    active: Res<ActiveEvent>,
    city: Res<City>,
    mut marchers: Query<
        (
            Entity,
            &mut Marcher,
            &Transform,
            &mut Bouncer,
            &mut crate::ai::figure::WalkCycle,
            &Mood,
        ),
        Without<Launched>,
    >,
) {
    let Some(happening) = &active.0 else { return };

    for (entity, mut marcher, transform, mut bouncer, mut cycle, mood) in &mut marchers {
        if marcher.next >= happening.route.len() {
            // Reached the end: the parade disperses into the crowd — which
            // here means politely ceasing to exist.
            commands.entity(entity).despawn();
            continue;
        }
        let here = transform.translation.xz();
        let node = city.graph.node(happening.route[marcher.next]).pos;
        let direction = (node - here).normalize_or_zero();
        let right = Vec2::new(-direction.y, direction.x);
        let target = node + right * marcher.file;
        if here.distance(node) < ARRIVED {
            marcher.next += 1;
        }

        let heading = (target - here).normalize_or_zero();
        bouncer.desired = heading * MARCH_PACE;
        bouncer.hop_scale = crate::ai::pedestrian::spring(mood.value, config.bounce.npc_spring_max);
        cycle.speed = MARCH_PACE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_schedules_the_same_week() {
        for day in 0..7 {
            assert_eq!(programme(7, day), programme(7, day));
        }
    }

    #[test]
    fn the_calendar_has_both_parades_and_quiet_days() {
        let mut csd = 0;
        let mut demo = 0;
        let mut quiet = 0;
        for day in 0..60 {
            match programme(0xA17E_5EED, day) {
                Some((EventKind::Csd, _)) => csd += 1,
                Some((EventKind::Demo, _)) => demo += 1,
                None => quiet += 1,
            }
        }
        assert!(csd > 5, "two months should hold a few CSDs: {csd}");
        assert!(demo > 5, "and a few demos: {demo}");
        assert!(quiet > 10, "and plenty of quiet days: {quiet}");
    }

    #[test]
    fn every_event_starts_at_a_civilised_hour() {
        for day in 0..120 {
            if let Some((_, start)) = programme(3, day) {
                assert!(
                    (10.0..=18.0).contains(&start),
                    "day {day} starts at {start}"
                );
            }
        }
    }

    #[test]
    fn a_route_crosses_the_city() {
        let layout = crate::world::citygen::generate(0xA17E_5EED, 1000.0);
        let city = City(layout);
        for day in 0..5 {
            let route = route(&city, 0xA17E_5EED, day);
            assert!(route.len() > 4, "day {day} routes {} nodes", route.len());
            let from = city.graph.node(route[0]).pos;
            let to = city.graph.node(*route.last().unwrap()).pos;
            assert!(
                from.distance(to) > 400.0,
                "day {day} marches only {}m",
                from.distance(to)
            );
        }
    }
}
