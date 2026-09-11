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
use bevy::render::render_resource::TextureFormat;

use crate::ai::archetype::Archetype;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::config::GameConfig;
use crate::core::rng::{key_for, stream};
use crate::core::schedule::GameSet;
use crate::mood::face::FaceLevel;
use crate::mood::feeling::{Mood, Temperament};
use crate::mood::provoke::Provoker;
use crate::mood::voice::Voicebox;
use crate::world::City;
use crate::world::buildings::SIDEWALK_HEIGHT;
use crate::world::roadgraph::NodeId;
use crate::world::texture::{encode, painted_rect, text_band};
use crate::world::timeofday::TimeOfDay;

// ------------------------------------------------------------- placards ----

/// One sign somebody is carrying.
///
/// A parade with no signs on it is a queue that happens to be in the road,
/// and the signs are where a march says what it is *about* — which in this
/// city is the joke, twice over. The CSD column means every word of its
/// placards and the words are about being round, soft and several colours,
/// which is what everybody here already is. The Demo column is furious about
/// municipal street furniture, because in a town with no weapons, no police
/// and no way to come to harm, the bollard is the last remaining oppressor
/// and it has never lost an argument.
///
/// The rule for writing one is the police station's, and it is the rule for
/// every painted word in this game: deadpan, plausible, and told entirely in
/// the register of whoever is holding it. Nothing here punches at a person.
struct Placard {
    head: &'static str,
    /// The small print underneath, where a real placard puts the bit that
    /// undercuts the big print.
    foot: Option<&'static str>,
    ink: [u8; 4],
    field: [u8; 4],
}

/// What the rainbow column is carrying.
const CSD_PLACARDS: [Placard; 7] = [
    Placard {
        head: "JEDER HÜPFT ANDERS",
        foot: Some("UND ALLE GLEICH HOCH"),
        ink: [30, 26, 34, 255],
        field: [246, 214, 72, 255],
    },
    Placard {
        head: "LIEBE FEDERT MIT",
        foot: None,
        ink: [252, 248, 252, 255],
        field: [206, 54, 120, 255],
    },
    Placard {
        head: "BUNT PRALLT",
        foot: Some("BESSER AB"),
        ink: [252, 250, 244, 255],
        field: [68, 132, 206, 255],
    },
    Placard {
        head: "KEINE ANGST",
        foot: Some("VOR RUNDUNGEN"),
        ink: [36, 30, 26, 255],
        field: [140, 206, 108, 255],
    },
    Placard {
        head: "WIR SIND WEICH",
        foot: Some("UND WIR SIND VIELE"),
        ink: [250, 246, 240, 255],
        field: [128, 62, 172, 255],
    },
    Placard {
        head: "MEHR GLITZER",
        foot: Some("WENIGER KANTE"),
        ink: [40, 34, 30, 255],
        field: [244, 158, 48, 255],
    },
    Placard {
        head: "GLEICHE HÖHE",
        foot: Some("FÜR ALLE"),
        ink: [248, 250, 252, 255],
        field: [44, 154, 150, 255],
    },
];

/// And what the other one is. Brown card and a marker pen, which is what a
/// demonstration about a bollard is actually written on.
const DEMO_PLACARDS: [Placard; 8] = [
    Placard {
        head: "POLLER RAUS!",
        foot: Some("ER HAT ANGEFANGEN"),
        ink: [28, 26, 24, 255],
        field: [214, 190, 148, 255],
    },
    Placard {
        head: "WIR FORDERN",
        foot: Some("WENIGER"),
        ink: [26, 24, 22, 255],
        field: [226, 220, 206, 255],
    },
    Placard {
        head: "NIEDER MIT DEM",
        foot: Some("BORDSTEIN"),
        ink: [30, 26, 22, 255],
        field: [208, 182, 140, 255],
    },
    Placard {
        head: "WIR SIND HIER",
        foot: Some("WIR SIND RUND"),
        ink: [24, 22, 26, 255],
        field: [230, 224, 210, 255],
    },
    Placard {
        head: "HÄRTE IST",
        foot: Some("KEINE HALTUNG"),
        ink: [138, 26, 22, 255],
        field: [232, 226, 212, 255],
    },
    Placard {
        head: "DIE BAUSTELLE",
        foot: Some("MUSS WEG, SEIT 1874"),
        ink: [28, 26, 24, 255],
        field: [212, 186, 144, 255],
    },
    Placard {
        head: "GEGEN ALLES",
        foot: Some("AUSSER GUMMI"),
        ink: [26, 30, 24, 255],
        field: [222, 216, 198, 255],
    },
    Placard {
        head: "SCHLUSS MIT",
        foot: Some("KANTEN"),
        ink: [130, 30, 26, 255],
        field: [216, 192, 152, 255],
    },
];

/// The board, in metres. One format for the whole march: they were made at
/// the same kitchen table.
const PLACARD: Vec2 = Vec2::new(0.62, 0.46);
/// How thick the two back-to-back faces sit apart. A placard is printed on
/// both sides because a parade is watched from in front and from behind, and
/// a sign that vanishes when it goes past is a sign nobody carried.
const PLACARD_LEAF: f32 = 0.011;
/// Where the stick is gripped in the marcher's own frame, how long it is,
/// and how far up it the board is nailed.
///
/// The figure's origin is the middle of its capsule and its feet are at
/// `body::FEET`, so a board centred `PLACARD_RISE` up a stick held at
/// `POLE_CENTRE` rides about a metre and nine tenths above the pavement —
/// which is a sign held up rather than a sign being carried home.
///
/// Everything is measured *along the stick* rather than in the marcher's
/// axes, so the lean stays one number: nail the board to a tilted pole by
/// writing its height and its offset separately and the two part company
/// the first time anybody retunes the tilt.
const POLE_CENTRE: f32 = 0.55;
const POLE_HALF: f32 = 0.42;
const POLE_RADIUS: f32 = 0.018;
const PLACARD_RISE: f32 = 0.50;
/// How far the whole thing stands off the chest, and how far it leans back.
const PLACARD_AHEAD: f32 = -0.15;
const PLACARD_TILT: f32 = 0.22;

// The board has to actually be on the stick: nailed above the grip and no
// higher than the stick reaches, or the marcher is carrying a plank with a
// sign hovering over it.
const _: () = assert!(PLACARD_RISE > 0.0 && PLACARD_RISE < POLE_HALF + PLACARD.y * 0.5);

/// Width of one glyph cell relative to the height of its letters, the same
/// proportion every other painted word in the city is set at — the plates,
/// the plaques and the posters all run on `signage::CELL_ASPECT` and a
/// placard that disagreed would read as a different town's lettering.
const CELL_ASPECT: f32 = 0.95;

/// The smallest a slogan's big line may come out and still be a slogan.
///
/// The clamp in [`placard_line`] keeps every line *on* the card by shrinking
/// it, which means a placard can never be wrong — it can only be quiet, and a
/// slogan whispered is a slogan nobody reads from the pavement. Roughly
/// fifteen characters at this format. The fix is never to raise this number;
/// it is to break the line in two, which is what a person with a marker pen
/// and a piece of card does anyway.
const SHOUTING: f32 = 0.060;

/// One line of placard text: where its band sits, how tall its letters are
/// and how wide the line comes out, clamped so the longest slogan still fits
/// inside the card rather than running off the side of the punchline.
fn placard_line(text: &str, centre_v: f32, tallest_v: f32) -> (Vec3, Vec<u8>) {
    let cells = (text.chars().count() + 2) as f32;
    let letter_v = tallest_v.min(0.88 / (cells * CELL_ASPECT * (PLACARD.y / PLACARD.x)));
    let width_u = cells * letter_v * CELL_ASPECT * (PLACARD.y / PLACARD.x);
    (Vec3::new(centre_v, letter_v, width_u), encode(text))
}

/// Paints one board: a card, a hand-inked border, and the slogan.
fn placard_texture(sign: &Placard) -> Image {
    let lines = match sign.foot {
        // One line, large, in the middle of the card.
        None => vec![placard_line(sign.head, 0.50, 0.26)],
        Some(foot) => vec![
            placard_line(sign.head, 0.40, 0.22),
            placard_line(foot, 0.68, 0.14),
        ],
    };
    let (ink, field) = (sign.ink, sign.field);

    painted_rect(320, 238, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        // The border is drawn *by hand*, which is to say it wobbles: a
        // ruled rectangle reads as a printed sign, and nobody prints these.
        let wobble = 0.012 + 0.006 * (v * 23.0).sin() * (u * 17.0).cos();
        let edge = u.min(1.0 - u).min(v.min(1.0 - v));
        if edge < 0.02 {
            return field;
        }
        if edge < 0.02 + wobble {
            return ink;
        }
        for (band, text) in &lines {
            let (centre_v, letter_v, width_u) = (band.x, band.y, band.z);
            let band_u = (u - (0.5 - width_u * 0.5)) / width_u;
            let band_v = (v - (centre_v - letter_v * 0.5)) / letter_v;
            if (0.0..1.0).contains(&band_u) && text_band(text, band_u, band_v) {
                return ink;
            }
        }
        field
    })
}

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

/// The column's wardrobe and its signwriting, built once.
#[derive(Resource)]
struct MarchAssets {
    rainbow: Vec<Handle<StandardMaterial>>,
    banner_grey: Handle<StandardMaterial>,
    /// One quad for every board in the city and one stick under it: the
    /// parade's whole placard budget is two meshes and fifteen materials,
    /// which is the same economy the shop signs run on.
    board: Handle<Mesh>,
    pole: Handle<Mesh>,
    stick: Handle<StandardMaterial>,
    csd: Vec<Handle<StandardMaterial>>,
    demo: Vec<Handle<StandardMaterial>>,
}

impl MarchAssets {
    fn placard(&self, kind: EventKind, index: usize) -> Option<&Handle<StandardMaterial>> {
        let run = match kind {
            EventKind::Csd => &self.csd,
            EventKind::Demo => &self.demo,
        };
        placard_for(index, run.len()).map(|pick| &run[pick])
    }
}

/// Which board the marcher in a given slot carries, as an index into a run of
/// `len` boards, or nothing at all.
///
/// Every third marcher has their hands in their pockets, because a column in
/// which literally everybody brought a sign reads as a stock photo. The rest
/// walk down the index rather than repeating one slogan, so the column says
/// several things at once — which is the difference between a demonstration
/// and a chorus line, and it is free.
fn placard_for(index: usize, len: usize) -> Option<usize> {
    if len == 0 || index % 3 == 1 {
        return None;
    }
    Some(index % len)
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

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
) {
    let cloth = |materials: &mut Assets<StandardMaterial>, color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.85,
            ..default()
        })
    };
    let printed = |materials: &mut Assets<StandardMaterial>,
                   images: &mut Assets<Image>,
                   run: &'static [Placard]| {
        run.iter()
            .map(|sign| {
                materials.add(StandardMaterial {
                    base_color_texture: Some(images.add(placard_texture(sign))),
                    perceptual_roughness: 0.94,
                    ..default()
                })
            })
            .collect::<Vec<_>>()
    };
    commands.insert_resource(MarchAssets {
        board: meshes.add(Rectangle::new(PLACARD.x, PLACARD.y)),
        pole: meshes.add(Cylinder::new(POLE_RADIUS, POLE_HALF * 2.0)),
        stick: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.42, 0.26),
            perceptual_roughness: 0.9,
            ..default()
        }),
        csd: printed(&mut materials, &mut images, &CSD_PLACARDS),
        demo: printed(&mut materials, &mut images, &DEMO_PLACARDS),
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
    let level = crate::mood::face::level_of(mood);

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
            FaceLevel(level),
            Voicebox::new(0.85 + (index as f32 * 0.61803) % 0.4),
            Provoker::default(),
            Archetype::Everyday,
            Visibility::default(),
        ));
        crate::ai::figure::dress(
            &mut marcher,
            &figures,
            coat,
            level,
            Archetype::Everyday,
            &mut wardrobe,
        );
        if let Some(board) = assets.placard(happening.kind, index) {
            carry_a_placard(&mut marcher, &assets, board);
        }
    }
}

/// Puts a sign in somebody's hands.
///
/// Children with a [`Rest`](crate::ai::figure::Rest) pose, like every other
/// thing the cast carries: a child with no rest is skipped by
/// `figure::animate`, so a placard without one would hang in the air at the
/// marcher's average height while the marcher underneath it squashed and
/// stretched. Two faces back to back rather than one double-sided quad,
/// because a double-sided one shows the slogan mirrored from behind and a
/// parade is watched from both ends of a street.
fn carry_a_placard(
    marcher: &mut EntityCommands,
    assets: &MarchAssets,
    board: &Handle<StandardMaterial>,
) {
    use crate::ai::figure::Rest;

    let lean = Quat::from_rotation_x(PLACARD_TILT);
    let grip = Vec3::new(0.0, POLE_CENTRE, PLACARD_AHEAD);
    // Up the stick, and out of the face of the board: both taken off the
    // lean so the whole assembly tilts as one object.
    let along = lean * Vec3::Y;
    let normal = lean * Vec3::Z;

    marcher.with_children(|parent| {
        parent.spawn((
            Rest::at(grip),
            Mesh3d(assets.pole.clone()),
            MeshMaterial3d(assets.stick.clone()),
            Transform::from_translation(grip).with_rotation(lean),
        ));
        // The figure faces -Z and a `Rectangle`'s own normal is +Z, so the
        // face somebody standing in front of the marcher reads is the one
        // turned half a turn about Y. The lean is applied *outside* that
        // turn, or the two leaves splay apart like an open book.
        for (leaf, turn) in [(-1.0f32, std::f32::consts::PI), (1.0, 0.0)] {
            let at = grip + along * PLACARD_RISE + normal * (leaf * PLACARD_LEAF);
            parent.spawn((
                Rest::at(at),
                Mesh3d(assets.board.clone()),
                MeshMaterial3d(board.clone()),
                Transform::from_translation(at).with_rotation(lean * Quat::from_rotation_y(turn)),
            ));
        }
    });
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
    fn every_placard_fits_the_font() {
        use crate::world::texture::glyph;
        for sign in CSD_PLACARDS.iter().chain(&DEMO_PLACARDS) {
            for text in std::iter::once(sign.head).chain(sign.foot) {
                for code in encode(text) {
                    assert!(
                        code == b' ' || glyph(code) != [0; 7],
                        "a placard says {text:?} and the font cannot draw {:?}",
                        code as char
                    );
                }
            }
        }
    }

    #[test]
    fn every_slogan_fits_on_the_card_and_still_shouts() {
        // Two failures, and the second is the one that hides. A slogan
        // painted off the edge of the card has its punchline cropped off,
        // and the clamp exists to stop that — but the clamp stops it by
        // shrinking the line, so a head written long enough comes out
        // perfectly correct and completely inaudible. Both ends, then.
        for sign in CSD_PLACARDS.iter().chain(&DEMO_PLACARDS) {
            let tallest = if sign.foot.is_some() { 0.22 } else { 0.26 };
            let (head, _) = placard_line(sign.head, 0.5, tallest);
            assert!(
                head.z <= 0.881,
                "{:?} runs {:.2} of the card wide",
                sign.head,
                head.z
            );
            assert!(
                head.y >= SHOUTING,
                "{:?} is set at {:.3} and wants breaking in two",
                sign.head,
                head.y
            );
            if let Some(foot) = sign.foot {
                let (band, _) = placard_line(foot, 0.68, 0.14);
                assert!(band.z <= 0.881, "{foot:?} runs {:.2} wide", band.z);
                assert!(band.y > 0.03, "{foot:?} is written too small to read");
            }
        }
    }

    #[test]
    fn a_placard_is_mostly_card_with_ink_on_it() {
        // The same disguise the posters wear: a band-arithmetic slip that
        // paints the whole card in ink, or none of it, still builds and
        // still gets carried down the street.
        for sign in CSD_PLACARDS.iter().chain(&DEMO_PLACARDS) {
            let image = placard_texture(sign);
            let data = image.data.as_ref().expect("the placard was not painted");
            let ink = data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[..3] == sign.ink[..3])
                .count() as f32
                / (data.len() / 4) as f32;
            assert!(
                (0.02..0.45).contains(&ink),
                "{:?}: {ink:.3} of the card is ink",
                sign.head
            );
        }
    }

    #[test]
    fn the_column_says_several_things_at_once() {
        for run in [CSD_PLACARDS.len(), DEMO_PLACARDS.len()] {
            let carried: Vec<usize> = (0..MARCHERS).filter_map(|i| placard_for(i, run)).collect();
            assert!(
                carried.len() >= MARCHERS / 2,
                "only {} of {MARCHERS} marchers brought a sign",
                carried.len()
            );
            assert!(
                carried.len() < MARCHERS,
                "every single marcher brought a sign"
            );
            let distinct: std::collections::HashSet<_> = carried.iter().collect();
            assert!(
                distinct.len() >= 4,
                "the whole column is carrying {} slogan(s)",
                distinct.len()
            );
        }
        // A column with no boards built is a column with no boards carried,
        // not a panic on an empty run.
        assert_eq!(placard_for(0, 0), None);
    }

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
        let layout = crate::world::citygen::generate(
            0xA17E_5EED,
            1000.0,
            crate::core::config::CityStyle::Generisch,
        );
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
