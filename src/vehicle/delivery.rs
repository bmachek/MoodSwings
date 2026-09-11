//! The van stopped where it should not be, and whoever is unloading it.
//!
//! Every other vehicle in this city is either driving or parked, and both of
//! those are states a street is *in*. A delivery is a thing a street is
//! *doing*: a van at an angle with its hazards going, a rear door open, and
//! somebody walking a box from one to the pavement and back. It is the only
//! piece of the world that is visibly a task half finished, and a task half
//! finished is most of what makes a place look inhabited.
//!
//! ## Not actually double-parked
//!
//! The name is what it is and the geometry is a compromise, deliberately. A van
//! stopped in the running lane is the funnier picture and it is unplayable:
//! `ai::traffic` drives edge to edge with no idea the van is there, so within a
//! minute there is a permanent scrum of cars nosed into it and the street reads
//! as broken rather than as busy — the same trap `world::worksite` sidestepped
//! by staying on the pavement.
//!
//! So the van sits in the parking lane, at an angle, nose out. That is where a
//! courier actually leaves one for four minutes, the traffic behaves, and the
//! read — stopped in a hurry, not parked — comes entirely from the angle and
//! the hazards.
//!
//! ## One blinking material, not forty
//!
//! Every hazard lamp in the city shares one material and one system writes it,
//! so the whole city's vans blink together. That is the same call
//! `world::worksite` made for its warning lamp and it is the same reason: the
//! alternative is a material per van, which is exactly the mistake the parked
//! cars were making before the paint shop.

use avian3d::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::spec::VehicleClass;
use crate::ai::figure::{Rest, WalkCycle, body};
use crate::ai::steering::Parking;
use crate::bounce::controller::Bouncer;
use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::world::City;
use crate::world::buildings::SIDEWALK_HEIGHT;

/// Chance a street has a delivery going on.
///
/// One in eleven, and it used to be one in thirty-odd — the *denominator*
/// changed, not the density.
///
/// The streets with nowhere to stop have dropped out of the draw, which takes
/// Landshut from the six hundred-odd streets long enough for a delivery to the
/// 191 that also have a parking lane. One in thirty-odd of *those* is four
/// vans in a whole city, which is not a city being delivered to, it is a
/// coincidence.
///
/// Chosen so the *expected* number of vans is the number that was tuned —
/// 191 × this is seventeen, against the eighteen it used to be — and not
/// against what this seed happens to draw. At one in eight the same town came
/// out with twenty-six and at one in eleven with twelve, which is one
/// standard deviation either side of the same density: a constant fitted to
/// one seed's Bernoulli draw is fitted to nothing. The scatter logs both
/// halves of the ratio so the next person does not have to guess the
/// denominator the way this comment once did.
const CHANCE: f32 = 0.088;

/// How far off the street's own line the van is left, in radians. Enough to
/// read as abandoned rather than parked, not enough to block a lane.
const SKEW: (f32, f32) = (0.16, 0.34);

/// Blinks per second. Slower than a warning lamp on a hole in the road,
/// because a hazard flasher is a relay and a relay is about this fast.
const BLINK: f32 = 0.75;

/// How far the hazards and the courier are drawn.
const RANGE: f32 = 110.0;

/// How many places along a street are tried before giving up on it, and how
/// much daylight the van wants beyond its own nose.
///
/// A parked row stands bumper to bumper with gaps in it, so the first place
/// tried is often somebody's boot. These are stepped evenly along the middle
/// half of the street, so the search covers it; none of them clear, and the
/// street simply has no delivery on it today.
const PITCHES: usize = 8;
const ROOM: f32 = 2.6;

/// Metres the courier walks between the van's tail and the pavement, and how
/// long they stand at each end.
const HAUL: f32 = 3.2;
const PAUSE: (f32, f32) = (1.4, 3.6);
const PACE: f32 = 1.25;

// The haul has to be long enough to be a walk at the courier's pace: under a
// second and a half each way it reads as a twitch rather than as a trip. And
// nobody turns straight round at either end.
const _: () = assert!(HAUL / PACE > 1.5);
const _: () = assert!(PAUSE.0 > 1.0);
const _: () = assert!(PAUSE.1 > PAUSE.0);

/// A hazard lamp, blinked by [`flash`].
#[derive(Component)]
pub struct Hazard;

/// Somebody carrying a box between a van and a doorway.
#[derive(Component)]
pub struct Courier {
    /// The two ends of the trip, and which one they are heading for.
    tail: Vec2,
    door: Vec2,
    outbound: bool,
    /// Seconds left standing at whichever end they have reached.
    waiting: f32,
}

#[derive(Resource)]
pub struct DeliveryKit {
    lamp: Handle<Mesh>,
    box_: Handle<Mesh>,
    amber: Handle<StandardMaterial>,
    card: Handle<StandardMaterial>,
    /// Courier overalls. One shared colour on purpose: a uniform is what makes
    /// somebody standing beside a van read as delivering rather than loitering.
    overalls: Handle<StandardMaterial>,
}

pub struct DeliveryPlugin;

impl Plugin for DeliveryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            // After the parked cars, which is where the vehicle assets land.
            .add_systems(
                PostStartup,
                scatter.after(super::spawn::spawn_parked_vehicles),
            )
            .add_systems(Update, flash.in_set(GameSet::Simulation))
            .add_systems(
                Update,
                haul.in_set(GameSet::Ai)
                    .after(crate::ai::pedestrian::Walking),
            );
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(DeliveryKit {
        lamp: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        box_: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        amber: materials.add(StandardMaterial {
            base_color: Color::srgb(0.82, 0.48, 0.08),
            emissive: LinearRgba::rgb(4.0, 1.6, 0.1),
            perceptual_roughness: 0.35,
            ..default()
        }),
        card: materials.add(StandardMaterial {
            base_color: Color::srgb(0.66, 0.54, 0.38),
            perceptual_roughness: 0.94,
            ..default()
        }),
        overalls: materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.32, 0.20),
            perceptual_roughness: 0.90,
            ..default()
        }),
    });
}

/// Leaves a van and a courier on one street in thirty.
#[allow(clippy::too_many_arguments)]
fn scatter(
    mut commands: Commands,
    config: Res<GameConfig>,
    city: Res<City>,
    kit: Res<DeliveryKit>,
    vehicles: Res<super::spawn::VehicleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    figures: Option<Res<crate::ai::figure::FigureAssets>>,
    faces: Option<Res<crate::mood::face::FaceAssets>>,
    tempers: Option<Res<crate::mood::feeling::Tempers>>,
    // The row this van is going to stand in. `scatter` is ordered after
    // `spawn_parked_vehicles` and the sync point between them means the cars
    // are real entities by now, not queued commands.
    parked: Query<&Transform, With<super::spawn::Vehicle>>,
) {
    let mut rng = stream_for(config.world_seed, stream::DELIVERIES);
    // Streets that could have had one, so the log says what the chance above
    // is a chance *of*.
    let mut eligible = 0usize;
    // And the vans this run has already put down, which the query above
    // cannot see for the same reason: `Commands` are deferred.
    let mut standing: Vec<Vec2> = Vec::new();
    let range = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: RANGE..(RANGE * 1.1),
        use_aabb: false,
    };
    let mut left = 0usize;

    for edge in city.graph.edges() {
        // The draw happens first whatever else is true, so that adding a
        // reason to skip a street does not reshuffle which of the others get
        // a van.
        let rolled = rng.random_range(0.0..1.0) <= CHANCE;
        // Only where there is a parking lane to stop in. The module note above
        // says the van stands in the parking lane rather than the running one,
        // and on a street with no parking lane there is no such place: the
        // offset this used to carry put the van ten centimetres off the
        // centreline of a four-metre Gasse, which is not a delivery, it is a
        // roadblock — and 45% of Landshut's road length is that narrow. It is
        // the same test `spawn_parked_vehicles` applies, for the same reason.
        let parking = crate::ai::steering::parking_for(edge.width);
        let suitable = edge.length >= 24.0 && parking != Parking::None;
        if suitable {
            eligible += 1;
        }
        if !rolled || !suitable {
            continue;
        }
        let a = city.graph.node(edge.a).pos;
        let b = city.graph.node(edge.b).pos;
        let Ok(direction) = Dir2::new(b - a) else {
            continue;
        };
        let normal = Vec2::new(-direction.y, direction.x);
        // Which kerb. A street parked down one side has its row on the kerb
        // the street itself names, and a van left in the other half is left in
        // the running lane — the one thing the module note says it must not
        // be. `parked_kerb` is the same answer the row, the traffic and the
        // cyclists all get.
        let side = match parking {
            Parking::BothSides => {
                if rng.random_range(0.0..1.0) < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => crate::ai::steering::parked_kerb(a, b).dot(normal).signum(),
        };

        // The box van of this cast. `Truck` is what the generator calls it and
        // it is the only body in the fleet with a back you could get a parcel
        // out of.
        let mut spec = VehicleClass::Truck.spec();
        (spec.body_color, spec.body_metallic, spec.body_age) = super::paint::street_paint(&mut rng);
        let half = spec.half_extents;

        // In the parking lane, at an angle — and *exactly* where a parked one
        // would stand, which is what the number this replaces was not. It was
        // 1.9 m in from the kerb, justified by a comment saying parked cars
        // sit 1.6 m in; they have sat `steering::parked_depth` in since the
        // row learned its own depth, so the van stood further into the
        // carriageway than the row it was meant to be standing in.
        let offset = super::spawn::kerb_offset(edge.width, half.x);
        let facing = if side > 0.0 { *direction } else { -*direction };
        let skew = rng.random_range(SKEW.0..SKEW.1) * side;
        let heading = super::spawn::heading_towards(facing) + skew;

        // Somewhere along the street with nothing already standing in it.
        // The row is parked bumper to bumper with gaps, and a van dropped
        // into one without looking is a van dropped *into a parked car*: two
        // bodies in one place, which Avian resolves by firing one of them
        // over the rooftops. The patrol has a complaint with that exact
        // wording and this was one of the things making it.
        let phase = rng.random_range(0.0..1.0);
        let Some(at) = (0..PITCHES).find_map(|pitch| {
            // Stepped along the street rather than sampled from it. Random
            // pitches re-test the same stretch and miss the gap two bays
            // further on: sampling found three deliveries in the whole city
            // where walking the street finds the same number as before.
            let t = 0.25 + (pitch as f32 + phase) / PITCHES as f32 * 0.5;
            let at = a + *direction * (t * edge.length) + normal * (offset * side);
            let clear = |other: &Vec2| other.distance(at) > half.z + ROOM;
            (parked.iter().all(|other| clear(&other.translation.xz()))
                && standing.iter().all(clear))
            .then_some(at)
        }) else {
            continue;
        };
        standing.push(at);
        let transform = Transform::from_xyz(at.x, super::spawn::resting_height(&spec), at.y)
            .with_rotation(Quat::from_rotation_y(heading));
        let van =
            super::spawn::spawn_vehicle(&mut commands, &vehicles, &mut materials, spec, transform);

        // Two hazards on the back corners. Children of the van, so they go
        // with it when somebody inevitably drives into it.
        commands.entity(van).with_children(|parent| {
            for x in [-1.0f32, 1.0] {
                parent.spawn((
                    Hazard,
                    Mesh3d(kit.lamp.clone()),
                    MeshMaterial3d(kit.amber.clone()),
                    Transform::from_xyz(x * half.x * 0.82, half.y * 0.15, half.z * 0.98)
                        .with_scale(Vec3::new(0.14, 0.16, 0.06)),
                    range.clone(),
                ));
            }
        });

        // And whoever is unloading it, walking between the tail and the kerb.
        // `None` only if the wardrobe has not landed, which cannot happen at
        // PostStartup but is not worth a panic to say so.
        if let (Some(figures), Some(faces), Some(tempers)) = (&figures, &faces, &tempers) {
            let back = at - facing * (half.z + 0.6);
            let door = at + normal * (2.6 * side);
            courier(
                &mut commands,
                &kit,
                figures,
                faces,
                tempers,
                &mut rng,
                back,
                door,
                &range,
            );
        }
        left += 1;
    }

    info!(
        "{left} deliveries in progress around the city, on {eligible} streets \
         long enough and wide enough to have one"
    );
}

/// One courier, dressed like anybody else and carrying a box.
#[allow(clippy::too_many_arguments)]
fn courier(
    commands: &mut Commands,
    kit: &DeliveryKit,
    figures: &crate::ai::figure::FigureAssets,
    faces: &crate::mood::face::FaceAssets,
    tempers: &crate::mood::feeling::Tempers,
    rng: &mut ChaCha8Rng,
    tail: Vec2,
    door: Vec2,
    range: &VisibilityRange,
) {
    use crate::ai::pedestrian::{HEIGHT, RADIUS, STAND_HEIGHT};
    use crate::mood::face::FaceLevel;
    use crate::mood::feeling::Mood;
    use crate::mood::voice::Voicebox;

    let temper = tempers.draw(rng);
    let mood = temper.baseline;
    let worn = faces.wear(mood);

    let mut person = commands.spawn((
        Name::new("Courier"),
        Courier {
            tail,
            door,
            outbound: true,
            waiting: rng.random_range(PAUSE.0..PAUSE.1),
        },
        Transform::from_xyz(tail.x, SIDEWALK_HEIGHT + STAND_HEIGHT, tail.y),
        RigidBody::Dynamic,
        Collider::capsule(RADIUS, HEIGHT),
        LockedAxes::ROTATION_LOCKED,
        Bouncer::new(STAND_HEIGHT),
        temper,
        Mood::new(mood),
        FaceLevel(worn.level),
        Voicebox::new(rng.random_range(0.85..1.20)),
        crate::mood::provoke::Provoker::default(),
        crate::ai::archetype::Archetype::Everyday,
        Visibility::default(),
    ));
    crate::ai::figure::dress(
        &mut person,
        figures,
        kit.overalls.clone(),
        &worn,
        crate::ai::archetype::Archetype::Everyday,
        rng,
    );
    // The box, carried in front at chest height. A `Rest`, so it is placed and
    // squashed by `figure::animate` along with everything else the courier is
    // made of — a parcel that flattens with its carrier is the correct amount
    // of physics for this game.
    let held = Vec3::new(0.0, body::SHOULDER - 0.26, -0.30);
    person.with_child((
        Rest::posed(held, Vec3::new(0.38, 0.30, 0.30)),
        Mesh3d(kit.box_.clone()),
        MeshMaterial3d(kit.card.clone()),
        Transform::from_translation(held),
        range.clone(),
    ));
}

/// The hazards.
///
/// One material for every van in the city, so this is one write a frame and
/// every delivery in sight blinks together — the same trade `world::worksite`
/// makes, and for the same reason.
fn flash(
    time: Res<Time>,
    kit: Option<Res<DeliveryKit>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(kit) = kit else { return };
    // Square, not a throb: a hazard flasher is a relay opening and closing.
    let on = (time.elapsed_secs() * BLINK * std::f32::consts::TAU).sin() > 0.0;
    if let Some(mut material) = materials.get_mut(&kit.amber) {
        let wanted = if on {
            LinearRgba::rgb(4.0, 1.6, 0.1)
        } else {
            LinearRgba::rgb(0.08, 0.035, 0.004)
        };
        if material.emissive != wanted {
            material.emissive = wanted;
        }
    }
}

/// Couriers walk their box to the door and go back for the next one.
///
/// After `pedestrian::Walking` for the same reason `ai::busker` is: this
/// overwrites `bouncer.desired` rather than reaching into anybody's route, so
/// a courier who gets bounced across the junction simply walks back.
fn haul(
    time: Res<Time>,
    mut couriers: Query<(&mut Courier, &Transform, &mut Bouncer, &mut WalkCycle)>,
) {
    let dt = time.delta_secs();
    for (mut courier, transform, mut bouncer, mut cycle) in &mut couriers {
        let here = transform.translation.xz();
        let target = if courier.outbound {
            courier.door
        } else {
            courier.tail
        };

        if courier.waiting > 0.0 {
            courier.waiting -= dt;
            bouncer.desired = Vec2::ZERO;
            cycle.speed = 0.0;
            continue;
        }

        let to_target = target - here;
        if to_target.length() < 0.4 {
            // Arrived. Stand about for a moment — nobody turns straight round
            // — and then head back.
            courier.outbound = !courier.outbound;
            courier.waiting = PAUSE.0 + (here.x.abs() % 1.0) * (PAUSE.1 - PAUSE.0);
            bouncer.desired = Vec2::ZERO;
            cycle.speed = 0.0;
            continue;
        }
        let walk = to_target.normalize_or_zero() * PACE;
        bouncer.desired = walk;
        cycle.speed = walk.length();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The van is out of the running lane.
    #[test]
    fn a_delivery_stands_exactly_where_a_parked_car_stands() {
        // What this replaces asserted that a van 1.9 m in from the kerb was
        // further out of the way than a parked car "1.6 m in". Parked cars
        // have not been 1.6 m in since the row learned its own depth — they
        // stand at `spawn::kerb_offset`, which on Landshut's median street is
        // 2.4 m off the kerb — so the number the van carried put it *into* the
        // running lane, and the test agreed with it because both numbers were
        // written down in the test.
        let half = VehicleClass::Truck.spec().half_extents.x;
        for width in [7.5f32, 9.6, 13.6, 15.1] {
            let van = crate::vehicle::spawn::kerb_offset(width, half);
            let lane = crate::ai::steering::lane_offset(width, true);
            assert!(
                van > lane + half,
                "on a {width} m street the van at {van} m is in the lane at {lane} m"
            );
        }
    }

    #[test]
    fn nobody_delivers_to_a_street_with_nowhere_to_stop() {
        // The one that made it a roadblock. On a four-metre Gasse there is no
        // parking lane at all, and the old offset put the van ten centimetres
        // off the centreline of a street two cars already cannot pass on.
        assert_eq!(
            crate::ai::steering::parking_for(4.0),
            crate::ai::steering::Parking::None
        );
        assert_ne!(
            crate::ai::steering::parking_for(7.5),
            crate::ai::steering::Parking::None
        );
    }

    /// It reads as stopped rather than as parked.
    #[test]
    fn the_van_is_left_at_an_angle() {
        assert!(SKEW.0 > 0.1, "a van square to the kerb is a parked van");
        // And not so far round that it is across the street: a fifth of a
        // right angle is a bad park, half of one is a crash.
        assert!(SKEW.1 < 0.5);
    }

    /// The hazards are on for half the time and off for the other half.
    #[test]
    fn a_hazard_flasher_flashes() {
        // Sampled over a whole number of periods, which is the only way this
        // means anything: over an arbitrary window a square wave's duty cycle
        // reads as whatever fraction of the last period the window happened to
        // cut off, and six seconds of a three-quarter-hertz relay is four and a
        // half of them.
        let period = 1.0 / BLINK;
        let steps = 4000;
        let lit = (0..steps)
            .filter(|step| {
                let t = *step as f32 / steps as f32 * period * 3.0;
                (t * BLINK * std::f32::consts::TAU).sin() > 0.0
            })
            .count() as f32
            / steps as f32;
        assert!(
            (0.49..0.51).contains(&lit),
            "the lamp is lit {lit:.3} of the time"
        );
    }
}
