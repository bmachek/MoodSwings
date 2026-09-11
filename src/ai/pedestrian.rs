//! Sidewalk pedestrians.
//!
//! Pedestrians walk the same road graph as traffic, offset past the kerb onto
//! the pavement. There is no separate navmesh: the city generator already
//! produces the only walkable topology that exists here, and a Recast navmesh
//! would add a heavy dependency to solve a problem the grid does not have.
//!
//! They used to be kinematic bodies whose motion was authored straight onto
//! `Transform`, which is the cheapest way to move a crowd and the only way to
//! move one that must never be pushed around. Neither property survives a city
//! made of rubber: being knocked flying by a car is the point now, and a
//! kinematic body cannot be. So they are dynamic, and where they walk is
//! expressed as a velocity the bounce controller steers towards rather than as
//! a position written each frame.
//!
//! That also retires the ground-following raycast this module used to need.
//! A dynamic body finds the kerb by landing on it.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::steering::right_of;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::mood::face::{FaceAssets, FaceLevel};
use crate::mood::feeling::{Mood, MoodRng, Tempers};
use crate::mood::provoke::Provoker;
use crate::mood::voice::Voicebox;
use crate::player::on_foot::Player;
use crate::world::City;
use crate::world::buildings::SIDEWALK_HEIGHT;
use crate::world::roadgraph::NodeId;

// Population, spawn ring, speeds and personal space are `GameConfig::crowd`
// now: the dev panel tunes a crowd, and archetype work will want to lean on
// the same dials. What stays here is geometry.

/// How far past the kerb the pavement centre sits.
const PAVEMENT_OFFSET: f32 = 1.9;

/// How often a new arrival steps out of a doorway rather than simply being
/// there, and how far it will look for one.
///
/// A city where nobody ever comes out of a building is a city of people who
/// live on the pavement. It costs nothing: this is the same arrival, placed
/// somewhere better.
const OUT_OF_A_DOOR: f32 = 0.4;
const DOORWAY_REACH: f32 = 18.0;

/// How many citizens may arrive on one refill tick, at most and at least.
///
/// The refill used to drain the whole deficit at once, which on a jump — a
/// chunk reload, a fast camera move, the first frame of a capture — is the
/// entire population spawned in a single frame, fully dressed.
///
/// A flat five was the first answer and it was the wrong one at the other end:
/// eight arrivals a second against a hundred and sixty means twenty seconds
/// before a street is a street, and a screenshot is taken after ninety frames.
/// Every capture came back with an empty pavement, which is a change to the
/// crowd that cannot be seen in the one instrument that judges it.
///
/// So it scales with how empty the ring is: a quarter of the deficit, which
/// fills an empty city in four ticks and settles to a trickle once it is full.
const ARRIVALS: (usize, usize) = (3, 22);
/// The capsule a citizen is.
///
/// Public because anything that spawns a figure has to build the same one — a
/// courier is a pedestrian who works for a living — and a second private copy
/// of these numbers is a second thing to forget when the crowd is resized.
pub const RADIUS: f32 = 0.32;
pub const HEIGHT: f32 = 1.05;
/// Distance from the capsule's centre to its lowest point.
pub const STAND_HEIGHT: f32 = HEIGHT * 0.5 + RADIUS;

/// Pace multiplier at the angry end of the scale: a Wutbürger at rock bottom
/// storms down the pavement half again as fast as they would stroll it.
const STORM_PACE: f32 = 1.5;
/// And at the bottom of an ordinary sulk: dragging the feet.
const TRUDGE_PACE: f32 = 0.78;
/// Mood below which a sulk stops slowing somebody down and starts driving
/// them: the same corner of the scale the rage line and the spontaneous
/// taunt live in.
const STORMING: f32 = -0.5;
/// How much a good mood lengthens the stride. Deliberately smaller than the
/// angry end — contentment is a stroll, not a hurry.
const STROLL_LIFT: f32 = 0.18;

/// How a flummi feels shows in how it walks, not just on its face.
///
/// Multiplier on the walking pace. Piecewise on purpose, because the angry
/// half of the scale is two different bodies: a mild sulk *slows* somebody
/// down — feet dragged, nowhere worth being — and past [`STORMING`] the same
/// scale flips into pace, which is what makes a genuinely furious flummi
/// legible from across the street before it ever taunts anybody. The happy
/// side is one gentle lift; delight lives in the hop, not the stride.
pub fn stride(mood: f32) -> f32 {
    let mood = mood.clamp(-1.0, 1.0);
    if mood <= STORMING {
        let past = (mood - STORMING) / (-1.0 - STORMING);
        TRUDGE_PACE + (STORM_PACE - TRUDGE_PACE) * past
    } else if mood < 0.0 {
        let into = mood / STORMING;
        1.0 + (TRUDGE_PACE - 1.0) * into
    } else {
        1.0 + STROLL_LIFT * mood
    }
}

/// And in how high it bounces: delight literally puts a spring in the step,
/// a bad mood flattens the hop into a stomp. Clamped at the low end so
/// nobody's walk cycle collapses into a shuffle along the ground; the top
/// comes from `BounceConfig::npc_spring_max`, because the crowd is where the
/// game's bounce lives now that the player's own hop is dialled down.
pub fn spring(mood: f32, max: f32) -> f32 {
    (1.0 + 0.5 * mood).clamp(0.85, max)
}

#[derive(Component)]
pub struct Pedestrian {
    pub from: NodeId,
    pub to: NodeId,
    /// Which pavement: +1 right of travel, -1 left.
    pub side: f32,
    pub speed: f32,
    /// Counts down while fleeing; keeps them running a moment after the danger
    /// passes rather than snapping back to a stroll.
    pub panic: f32,
    /// Metres per second this citizen *intends* to cover this frame.
    ///
    /// Not what paces the walk cycle any more — that is read off the body, so
    /// that every override in `ai::social` and `mood` moves the legs without
    /// having to remember to. Kept because it is the intent, and the intent is
    /// what a future crossing or queue has to be able to zero.
    pub current_speed: f32,
}

#[derive(Resource)]
pub struct PedestrianRng(pub ChaCha8Rng);

#[derive(Resource)]
struct PedestrianTimer(Timer);

impl Default for PedestrianTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(0.6, TimerMode::Repeating))
    }
}

#[derive(Resource)]
struct PedestrianAssets {
    clothes: Vec<Handle<StandardMaterial>>,
    /// Archetype-fixed coats. Small: most of the cast dresses off the street
    /// palette above.
    coats: Vec<(super::archetype::Archetype, Handle<StandardMaterial>)>,
}

impl PedestrianAssets {
    fn coat_for(&self, archetype: super::archetype::Archetype) -> Option<Handle<StandardMaterial>> {
        self.coats
            .iter()
            .find(|(a, _)| *a == archetype)
            .map(|(_, handle)| handle.clone())
    }
}

/// A gang member walks where the gang walks.
///
/// Groups spawn down one pavement, but every citizen re-rolls its route at
/// each junction, and five hooligans who each pick their own next street are
/// five pedestrians, not a gang. So a group has a leader — the first member
/// spawned — and the rest copy the leader's route whenever it changes. A
/// leader who despawns (streamed out, mostly) orphans the others into
/// ordinary citizens, which reads as the gang calling it a night.
#[derive(Component)]
pub struct Follows(pub Entity);

/// Everything that decides where the crowd is walking.
///
/// Exported so that anything wanting to override a flummi's intent — somebody
/// with a grudge, somebody too pleased with themselves to walk in a straight
/// line — can simply run after it. Both write `Bouncer::desired`, and whichever
/// runs last is what the body actually does.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Walking;

pub struct PedestrianPlugin;

impl Plugin for PedestrianPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PedestrianTimer>()
            .init_resource::<super::resident::Residents>()
            .init_resource::<super::archetype::Cast>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    reconcile_residents,
                    maintain_population,
                    // Routes copy before anybody steers along them.
                    flock,
                    walk_pavements,
                    // After the intent, before anything reads it: the lean
                    // away from the neighbours is part of walking, not an
                    // override, so it lives inside `Walking` rather than
                    // after it — a grudge or a flee that runs later still
                    // wins outright, which is exactly right for both.
                    give_way,
                    super::figure::pace_pedestrians,
                    super::figure::pace_player,
                    super::figure::animate,
                )
                    .chain()
                    .in_set(Walking)
                    .in_set(GameSet::Ai),
            );
    }
}

fn setup(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(PedestrianRng(stream_for(
        config.world_seed,
        stream::PEDESTRIANS,
    )));
    commands.insert_resource(super::archetype::CrowdRng(stream_for(
        config.world_seed,
        stream::CROWD,
    )));

    let palette = [
        Color::srgb(0.24, 0.30, 0.42),
        Color::srgb(0.48, 0.26, 0.24),
        Color::srgb(0.30, 0.36, 0.28),
        Color::srgb(0.55, 0.50, 0.42),
        Color::srgb(0.20, 0.22, 0.26),
        Color::srgb(0.42, 0.38, 0.52),
    ];
    // The same weave the rest of the cast is cut from — this is where the
    // crowd's own wardrobe is mixed, and it was thirty flat colours.
    let weave = images.add(crate::world::texture::fabric());
    let weave_relief = images.add(crate::world::texture::fabric_normal());
    let cloth = |materials: &mut Assets<StandardMaterial>, color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            base_color_texture: Some(weave.clone()),
            normal_map_texture: Some(weave_relief.clone()),
            uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(super::figure::WEAVE_TILE)),
            perceptual_roughness: 0.85,
            ..default()
        })
    };
    commands.insert_resource(super::figure::build_assets(
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    commands.insert_resource(PedestrianAssets {
        clothes: palette
            .into_iter()
            .map(|color| cloth(&mut materials, color))
            .collect(),
        // The fixed wardrobes, one material per archetype that has one.
        coats: super::archetype::Archetype::ALL
            .into_iter()
            .filter_map(|archetype| {
                archetype
                    .coat()
                    .map(|color| (archetype, cloth(&mut materials, color)))
            })
            .collect(),
    });
}

/// How near the far end of a street a citizen has to get before turning the
/// corner, measured along the street.
///
/// Along it, because across it a pavement is always the best part of a
/// carriageway away from the centreline — see the note in `walk_pavements`.
const ARRIVE: f32 = 2.5;

/// How wide the street between two junctions is.
fn street_width(city: &City, from: NodeId, to: NodeId) -> f32 {
    city.graph
        .neighbors(from)
        .find(|(node, _)| *node == to)
        .map(|(_, edge)| city.graph.edge(edge).width)
        .unwrap_or(9.0)
}

/// Which pavement of the street `at -> ahead` somebody standing at `position`
/// is already on.
///
/// A citizen turning a corner should walk round it, not step off the kerb and
/// cross to the other side of the new street because their `side` happened to
/// be written down as +1. With the pavements mitred at every junction the
/// corner is a continuous surface, so turning it is a matter of picking the
/// half of the new street the citizen is already standing on.
fn same_corner(at: Vec2, ahead: Vec2, position: Vec2) -> f32 {
    match Dir2::new(ahead - at) {
        Ok(direction) => {
            let across = right_of(*direction).dot(position - at);
            if across >= 0.0 { 1.0 } else { -1.0 }
        }
        Err(_) => 1.0,
    }
}

/// Centre of the pavement alongside the segment `a -> b`.
fn pavement_point(a: Vec2, b: Vec2, width: f32, side: f32, t: f32) -> Vec2 {
    let Ok(direction) = Dir2::new(b - a) else {
        return a;
    };
    a.lerp(b, t) + right_of(*direction) * side * (width * 0.5 + PAVEMENT_OFFSET)
}

/// Everything a citizen is dressed out of.
///
/// Bundled because Bevy caps a system at sixteen parameters and this one has
/// been at the ceiling since the archetypes landed — the note in the argument
/// list said so. The doorway query is what went over it, and "what somebody is
/// made of" is one thing anyway.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Wardrobe<'w> {
    assets: Res<'w, PedestrianAssets>,
    figures: Res<'w, super::figure::FigureAssets>,
    faces: Res<'w, FaceAssets>,
}

fn maintain_population(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: ResMut<PedestrianTimer>,
    config: Res<GameConfig>,
    // One tuple param rather than two: the system sits at Bevy's sixteen-
    // parameter ceiling, and clock-and-sky is one thing here anyway.
    sky: (
        Res<crate::world::timeofday::TimeOfDay>,
        Res<crate::world::weather::Weather>,
    ),
    city: Res<City>,
    wardrobe: Wardrobe,
    mut rng: ResMut<PedestrianRng>,
    mut tempers: ResMut<MoodRng>,
    mix: Res<Tempers>,
    mut crowd_rng: ResMut<super::archetype::CrowdRng>,
    cast: Res<super::archetype::Cast>,
    focus: Res<super::focus::SimFocus>,
    pedestrians: Query<(Entity, &Transform), With<Pedestrian>>,
    mut residents: ResMut<super::resident::Residents>,
    // Every shopfront the streaming has put up. A share of every arrival comes
    // out of one instead of appearing in the middle of a pavement — which is
    // the other half of `ai::errands`, and the classic tell of a fake city
    // either way round.
    doors: Query<&Transform, With<crate::world::interior::Shopfront>>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let focus = focus.ground();
    let crowd = &config.crowd;

    // The clock and the sky thin the crowd: the small hours keep a fraction
    // of the afternoon's street, and rain sends a share of everybody home.
    // Residents over the cap are not culled — they walk off the despawn ring
    // in their own time, which reads as the street emptying rather than as
    // the game deleting people.
    let (clock, weather) = sky;
    let level =
        super::social::crowd_level(clock.hours) * (1.0 - 0.4 * weather.rain.clamp(0.0, 1.0));
    let population = ((crowd.population as f32 * level).round() as usize).max(1);
    // After dark the cast changes too: children are in bed and the
    // missionaries knock by day. The draws are still consumed as usual —
    // a fixed answer is not a skipped question.
    let dark = crate::world::timeofday::daylight(clock.hours) < 0.2;

    let mut alive = 0usize;
    for (entity, transform) in &pedestrians {
        if transform.translation.xz().distance(focus) > crowd.despawn {
            commands.entity(entity).despawn();
        } else {
            alive += 1;
        }
    }
    if alive >= population {
        return;
    }

    // The streets in the ring, each with a ticket size.
    //
    // Weighted rather than uniform, and it matters more than the population
    // does: a uniform draw over every edge in the annulus spreads the crowd
    // evenly over two and a half thousand back alleys, so the street the
    // player is actually standing in gets its proportional share of nothing.
    // The weight is length — a long street holds more people than a stub —
    // over distance, so the crowd gathers where the camera is.
    let candidates: Vec<(&crate::world::roadgraph::RoadEdge, f32)> = city
        .graph
        .edges()
        .filter_map(|edge| {
            let midpoint = city
                .graph
                .node(edge.a)
                .pos
                .midpoint(city.graph.node(edge.b).pos);
            let away = midpoint.distance(focus);
            (crowd.spawn_min..crowd.spawn_max)
                .contains(&away)
                .then(|| (edge, edge.length / (1.0 + away / 40.0)))
        })
        .collect();
    let total: f32 = candidates.iter().map(|(_, weight)| weight).sum();
    if candidates.is_empty() || total <= 0.0 {
        return;
    }

    // And filled a few at a time. Draining the whole deficit on one tick is a
    // couple of dozen fully dressed figures in a single frame, which is a
    // visible hitch every time a chunk reloads or the camera jumps.
    let budget = ((population - alive) / 4).clamp(ARRIVALS.0, ARRIVALS.1);
    let mut arriving = 0usize;
    while alive < population && arriving < budget {
        arriving += 1;
        let mut ticket = rng.0.random_range(0.0..total);
        let edge = candidates
            .iter()
            .find(|(_, weight)| {
                ticket -= weight;
                ticket <= 0.0
            })
            .map(|(edge, _)| *edge)
            .unwrap_or(candidates[candidates.len() - 1].0);
        let (from, to) = if rng.0.random_range(0.0..1.0) < 0.5 {
            (edge.a, edge.b)
        } else {
            (edge.b, edge.a)
        };
        let a = city.graph.node(from).pos;
        let b = city.graph.node(to).pos;
        let side = if rng.0.random_range(0.0..1.0) < 0.5 {
            1.0
        } else {
            -1.0
        };
        let t: f32 = rng.0.random_range(0.1..0.9);

        // Who they are, from the crowd's own stream — see `ai::archetype` for
        // why it is neither of the two streams drawn from below. A group
        // shares one draw and arrives in single file down the same pavement.
        let mut archetype = cast.draw(&mut crowd_rng.0);
        if dark && archetype == super::archetype::Archetype::Missionary {
            archetype = super::archetype::Archetype::Everyday;
        }
        // Out of a door, some of the time, if there is one on this street.
        // Measured against the pavement point the citizen would otherwise
        // have appeared at, so nobody is teleported across the town to use a
        // doorway — they come out of the one they would have walked past.
        let doorway = (rng.0.random_range(0.0..1.0) < OUT_OF_A_DOOR)
            .then(|| {
                let would_be = pavement_point(a, b, edge.width, side, t);
                doors
                    .iter()
                    .map(|door| door.translation.xz())
                    .filter(|at| at.distance(would_be) < DOORWAY_REACH)
                    .min_by(|x, y| x.distance(would_be).total_cmp(&y.distance(would_be)))
            })
            .flatten();
        let mut leader: Option<Entity> = None;
        for member in 0..archetype.group_size() {
            // And how old. Drawn per member — a group of missionaries spans
            // the generations — and bent to Adult where the combination
            // would not be a joke, or where the hour says a child is in bed.
            let mut age = super::archetype::AgeClass::draw(&mut crowd_rng.0);
            if !age.suits(archetype) || (dark && age == super::archetype::AgeClass::Child) {
                age = super::archetype::AgeClass::Adult;
            }
            let size = age.size();
            if alive >= population {
                break;
            }
            let t = (t + member as f32 * 0.03).min(0.95);
            // The leader of a group may step out of a doorway; the rest of the
            // group follows them out of it, one behind the other, which is
            // what a family leaving a shop looks like.
            let position = match doorway {
                Some(door) => door + Vec2::new(member as f32 * 0.35, member as f32 * 0.2),
                None => pavement_point(a, b, edge.width, side, t),
            };
            let outfit = rng.0.random_range(0..wardrobe.assets.clothes.len());
            // Consume the wardrobe draw before deciding whether an inactive
            // resident comes back, so a returning face cannot reshuffle the
            // rest of the spawn stream.

            // Drawn from its own stream: a citizen's disposition must not
            // depend on how many of them have been spawned already, and
            // retuning the mix must not move anybody's route.
            let drawn = mix.draw(&mut tempers.0);
            let temper = archetype.temper().unwrap_or(drawn);
            // Their own voice, for as long as they are resident. The same
            // stream as the temperament: how somebody sounds is part of who
            // they are, and both are drawn once and never again.
            let pitch = tempers.0.random_range(0.82..1.28) * age.pitch() * archetype.pitch();
            let pace = rng
                .0
                .random_range(crowd.walk_speed - 0.4..crowd.walk_speed + 0.4);

            let mut person = commands.spawn((
                Name::new("Pedestrian"),
                Pedestrian {
                    from,
                    to,
                    side,
                    speed: pace,
                    panic: 0.0,
                    current_speed: 0.0,
                },
                // Nested: a flat tuple would pass Bevy's fifteen-element
                // bundle ceiling, and who-they-are is one thing anyway.
                (archetype, age, super::figure::Stature(size)),
                Transform::from_xyz(
                    position.x,
                    SIDEWALK_HEIGHT + STAND_HEIGHT * size,
                    position.y,
                ),
                // Dynamic, so a car can send them across the junction.
                RigidBody::Dynamic,
                // The collider scales with the age, the same number the
                // figure's pose is multiplied by — see `figure::Stature`.
                Collider::capsule(RADIUS * size, HEIGHT * size),
                // Upright until something knocks them over; `bounce::launch`
                // takes this off for as long as they are tumbling.
                LockedAxes::ROTATION_LOCKED,
                Bouncer::new(STAND_HEIGHT * size),
                temper,
                Mood::new(temper.baseline),
                FaceLevel(wardrobe.faces.wear(temper.baseline).level),
                Voicebox::new(pitch),
                Provoker::default(),
                Visibility::default(),
            ));
            let candidate = super::resident::ResidentProfile {
                archetype,
                age,
                temperament: temper,
                pitch,
                pace,
                outfit,
                mood: temper.baseline,
            };
            let (citizen, profile) = residents.activate(person.id(), candidate, dark);
            // Reusing a person changes their visible facts only after every
            // current spawn stream has taken the draw it always would. The
            // profile is immutable for now; mood, errands and memories gain
            // their own persistent state as those systems become offscreen.
            let material =
                wardrobe.assets.clothes[profile.outfit % wardrobe.assets.clothes.len()].clone();
            let material = wardrobe
                .assets
                .coat_for(profile.archetype)
                .unwrap_or(material);
            let worn = wardrobe.faces.wear(profile.mood);
            person.insert((
                citizen,
                profile.archetype,
                profile.age,
                super::figure::Stature(profile.age.size()),
                profile.temperament,
                Mood::new(profile.mood),
                FaceLevel(wardrobe.faces.wear(profile.mood).level),
                Voicebox::new(profile.pitch),
            ));
            if archetype.steadfast() {
                person.insert(crate::bounce::launch::NeverTumbles);
            }
            match leader {
                None => leader = Some(person.id()),
                Some(leader) => {
                    person.insert(Follows(leader));
                }
            }
            super::figure::dress_person(
                &mut person,
                &wardrobe.figures,
                material,
                &worn,
                archetype,
                &mut rng.0,
                super::appearance::Appearance::from_seed(config.world_seed ^ u64::from(citizen.0)),
            );
            alive += 1;
        }
    }
}

/// Returns bodies that disappeared through any system — a shop visit, an
/// ordinary streaming despawn, or a future activity — to the persistent
/// resident pool before the next refill considers an arrival.
fn reconcile_residents(
    mut residents: ResMut<super::resident::Residents>,
    visible: Query<(Entity, &super::resident::CitizenId, Option<&Mood>), With<Pedestrian>>,
) {
    residents.reconcile(
        visible
            .iter()
            .map(|(entity, id, mood)| (entity, *id, mood.map(|mood| mood.value))),
    );
}

fn walk_pavements(
    time: Res<Time>,
    mut report: Local<f32>,
    city: Res<City>,
    config: Res<GameConfig>,
    weather: Res<crate::world::weather::Weather>,
    mut rng: ResMut<PedestrianRng>,
    vehicles: Query<(&Transform, &LinearVelocity), With<crate::vehicle::spawn::Vehicle>>,
    mut pedestrians: Query<
        (
            &mut Pedestrian,
            &mut Bouncer,
            &mut Transform,
            &Mood,
            &super::archetype::Archetype,
            &super::archetype::AgeClass,
        ),
        (Without<crate::vehicle::spawn::Vehicle>, Without<Launched>),
    >,
) {
    let dt = time.delta_secs();
    let crowd = &config.crowd;

    // Anything moving fast enough to be worth running from.
    let threats: Vec<(Vec2, f32)> = vehicles
        .iter()
        .filter(|(_, velocity)| velocity.length() > crowd.scare_speed)
        .map(|(transform, velocity)| (transform.translation.xz(), velocity.length()))
        .collect();

    *report += dt;
    let announce = *report > 1.0;
    if announce {
        *report = 0.0;
    }
    let mut sample = None;

    for (mut pedestrian, mut bouncer, mut transform, mood, archetype, age) in &mut pedestrians {
        let position = transform.translation.xz();
        let mut a = city.graph.node(pedestrian.from).pos;
        let mut b = city.graph.node(pedestrian.to).pos;
        let mut width = street_width(&city, pedestrian.from, pedestrian.to);
        let mut segment = b - a;
        let mut length = segment.length().max(1.0);
        let mut travelled = ((position - a).dot(segment) / (length * length)).clamp(0.0, 1.0);

        // Arrived at the far end of the street, measured *along* it.
        //
        // Not by distance to the junction, which is what this used to be and
        // which could never fire: a pavement runs half a carriageway plus two
        // metres out from the centreline the junction node sits on, so the
        // nearest a citizen ever came to their own destination node was four
        // and a third metres on the narrowest street in the game and ten on
        // the widest, against a four-metre test. Every citizen therefore
        // walked to the end of the block it spawned on and then stood there
        // jittering against a target it could not reach, spinning, until the
        // despawn ring recycled it. That, and not the population, is why the
        // city did not look alive.
        if travelled * length > length - ARRIVE {
            let next = city
                .graph
                .neighbors(pedestrian.to)
                .map(|(node, _)| node)
                .filter(|node| *node != pedestrian.from)
                .choose(&mut rng.0)
                .unwrap_or(pedestrian.from);
            let corner = city.graph.node(pedestrian.to).pos;
            let ahead = city.graph.node(next).pos;
            pedestrian.from = pedestrian.to;
            pedestrian.to = next;
            // Turn the corner rather than crossing the road to do it: the new
            // street's pavement on the side the citizen is already standing.
            pedestrian.side = same_corner(corner, ahead, position);
            a = corner;
            b = ahead;
            width = street_width(&city, pedestrian.from, pedestrian.to);
            segment = b - a;
            length = segment.length().max(1.0);
            travelled = ((position - a).dot(segment) / (length * length)).clamp(0.0, 1.0);
        }
        let target = pavement_point(
            a,
            b,
            width,
            pedestrian.side,
            (travelled + 6.0 / length).min(1.0),
        );

        let mut heading = (target - position).normalize_or_zero();

        // Bolt away from anything bearing down on them.
        pedestrian.panic = (pedestrian.panic - dt).max(0.0);
        for (threat, _) in &threats {
            let away = position - *threat;
            if away.length() < crowd.scare_radius {
                pedestrian.panic = 1.6;
                heading = (heading + away.normalize_or_zero() * 2.0).normalize_or_zero();
            }
        }

        let speed = if pedestrian.panic > 0.0 {
            // Panic overrides temperament: a trudge does not outrun a car.
            crowd.flee_speed
        } else {
            // Who they are — and how old they are — scales how they amble,
            // on top of how they feel. Rain hurries the lot of them, with a
            // hard cap under the flee: weather quickens a street, it does
            // not turn strollers into escape artists.
            (pedestrian.speed.min(crowd.walk_speed * 1.3)
                * stride(mood.value)
                * archetype.pace()
                * age.pace()
                * super::social::hurry(weather.rain))
            .min(crowd.flee_speed * 0.95)
        };
        // The mood is in the body as well as on the face: the hop the bounce
        // controller takes at the bottom of every arc is scaled here, every
        // frame, because the controller spends the scale on each landing.
        // Who they are scales it again — a skater glides, a wheelchair rolls.
        // And the gait setting scales the lot: a walking city keeps its feet
        // down and lets the walk cycle carry the motion instead.
        bouncer.hop_scale = spring(mood.value, config.bounce.npc_spring_max)
            * archetype.hop()
            * age.spring()
            * config.gait.hop();

        pedestrian.current_speed = if heading == Vec2::ZERO { 0.0 } else { speed };
        // Asked for rather than applied. The bounce controller owns the body's
        // velocity; writing the position here would fight it, and Avian would
        // hand back whichever of the two ran last.
        bouncer.desired = heading * speed;

        // Rotation is locked, so nothing else will turn them to face the way
        // they are going.
        if heading != Vec2::ZERO {
            transform.rotation =
                Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(heading));
        }

        if sample.is_none() {
            sample = Some((transform.translation, speed, pedestrian.panic));
        }
    }

    if announce && let Some((position, speed, panic)) = sample {
        debug!(
            "pedestrians: {} walking, sample at {:.1},{:.2},{:.1} moving {:.2} m/s panic {:.1}",
            pedestrians.iter().len(),
            position.x,
            position.y,
            position.z,
            speed,
            panic
        );
    }
}

/// Leans every citizen's intent away from the neighbours crowding it.
///
/// The first crowd-on-crowd steering this city has had: without it two
/// flummis walking the same pavement the opposite way met chest to chest and
/// left the solver to grind them past each other. The lean bends paths a
/// stride early instead, and it is deliberately *only* a lean — see
/// `CrowdConfig::separation_radius` for why contact must stay possible.
///
/// One query, iterated twice — a read pass into a snapshot, then the write
/// pass — rather than two queries that both touch `Transform`, which is the
/// panic the schedule trap in CLAUDE.md is about.
/// Copies the leader's route onto everybody following one.
///
/// Disjoint by construction rather than by luck: a leader is exactly a
/// pedestrian `Without<Follows>`, so the read and the write can never alias
/// one component — this is the honest version of the filter trick the
/// schedule traps warn about, because here the filter *is* the semantics.
fn flock(
    leaders: Query<&Pedestrian, Without<Follows>>,
    mut followers: Query<(&mut Pedestrian, &Follows)>,
) {
    for (mut own, follows) in &mut followers {
        // A despawned leader orphans the gang into ordinary citizens.
        let Ok(leader) = leaders.get(follows.0) else {
            continue;
        };
        if own.to != leader.to || own.side != leader.side {
            own.from = leader.from;
            own.to = leader.to;
            own.side = leader.side;
        }
    }
}

/// How wide a berth a shy citizen keeps around the player, in metres.
const SHY_BERTH: f32 = 6.0;

fn give_way(
    config: Res<GameConfig>,
    players: Query<&Transform, (With<Player>, Without<Pedestrian>)>,
    mut pedestrians: Query<
        (&Transform, &super::archetype::Archetype, &mut Bouncer),
        (With<Pedestrian>, Without<Launched>),
    >,
) {
    let crowd = &config.crowd;
    if crowd.separation_push <= 0.0 {
        return;
    }
    let player = players.single().ok().map(|t| t.translation.xz());

    let positions: Vec<Vec2> = pedestrians
        .iter()
        .map(|(transform, ..)| transform.translation.xz())
        .collect();

    for (transform, archetype, mut bouncer) in &mut pedestrians {
        let me = transform.translation.xz();
        let push = super::steering::separation(me, &positions, crowd.separation_radius);
        if push != Vec2::ZERO {
            bouncer.desired += push * crowd.separation_push;
        }
        // The shy give the player a whole street's width of respect — the
        // same lean, from much further out and rather harder. They can still
        // be cornered; that is what makes cheering one up worth the chase.
        if archetype.shy()
            && let Some(player) = player
        {
            let berth = super::steering::separation(me, &[player], SHY_BERTH);
            bouncer.desired += berth * (crowd.separation_push * 2.5);
        }
    }
}

/// Convenience for picking a random element without collecting.
trait ChooseExt: Iterator + Sized {
    fn choose(self, rng: &mut ChaCha8Rng) -> Option<Self::Item> {
        let items: Vec<_> = self.collect();
        if items.is_empty() {
            return None;
        }
        let index = rng.random_range(0..items.len());
        items.into_iter().nth(index)
    }
}
impl<I: Iterator> ChooseExt for I {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mood_is_legible_in_the_walk() {
        // A sulk drags the feet, a proper rage storms, and delight strolls a
        // shade quicker — the readout the face gives at arm's length, the walk
        // has to give from across the street.
        assert!(stride(-0.3) < 1.0, "a sulk should slow somebody down");
        assert!(stride(-1.0) > 1.2, "a Wutbürger at the bottom should storm");
        assert_eq!(stride(0.0), 1.0);
        assert!(stride(0.8) > 1.0);
        // The turn from trudge to storm is a corner, not a cliff: a mood
        // easing past the line must not visibly snap gears.
        assert!(
            (stride(STORMING - 1e-3) - stride(STORMING + 1e-3)).abs() < 0.02,
            "the pace jumps at the storming line"
        );
    }

    #[test]
    fn no_mood_walks_anybody_faster_than_a_flee_or_a_chase() {
        // Storming is a manner, not an escape: a furious stroller must stay
        // catchable by a grudge and slower than somebody actually running.
        let crowd = GameConfig::default().crowd;
        let fastest = (0..=40)
            .map(|step| stride(-1.0 + step as f32 / 20.0))
            .fold(0.0f32, f32::max)
            * crowd.walk_speed
            * 1.3;
        assert!(fastest < crowd.flee_speed);
        assert!(fastest < GameConfig::default().mood.grudge_speed);
    }

    #[test]
    fn delight_puts_a_spring_in_the_step_and_a_sulk_flattens_it() {
        let max = GameConfig::default().bounce.npc_spring_max;
        assert!(spring(1.0, max) > 1.2);
        assert!(spring(-1.0, max) < 1.0);
        assert_eq!(spring(0.0, max), 1.0);
        // Flattened, not grounded: the hop is the walk cycle's clock, and a
        // scale near zero would leave a miserable flummi twitching in place.
        assert!(spring(-1.0, max) >= 0.85);
        // The config ceiling must actually be reachable, or the dial is dead.
        assert_eq!(spring(1.0, max), max);
    }

    #[test]
    fn the_player_hops_lower_than_any_citizen() {
        let bounce = GameConfig::default().bounce;
        // The whole crowd out-bounces the player, even a flummi in the depths
        // of a sulk — the camera rides the player's hop, nobody else's.
        assert!(bounce.player_hop_scale < spring(-1.0, bounce.npc_spring_max));
    }

    #[test]
    fn pavements_sit_outside_the_carriageway() {
        let a = Vec2::ZERO;
        let b = Vec2::new(0.0, 100.0);
        let width = 10.0;

        for side in [1.0, -1.0] {
            let point = pavement_point(a, b, width, side, 0.5);
            let lateral = point.x.abs();
            assert!(
                lateral > width * 0.5,
                "pavement at {lateral:.2}m is still inside a {width}m road"
            );
        }
    }

    #[test]
    fn the_two_pavements_are_on_opposite_sides() {
        let a = Vec2::ZERO;
        let b = Vec2::new(100.0, 0.0);
        let left = pavement_point(a, b, 9.0, -1.0, 0.5);
        let right = pavement_point(a, b, 9.0, 1.0, 0.5);
        assert!(
            left.y * right.y < 0.0,
            "both pavements landed on the same side: {left:?} / {right:?}"
        );
    }
}
