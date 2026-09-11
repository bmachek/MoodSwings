//! Shared steering maths for anything that drives or walks a route.
//!
//! Kept as free functions over plain vectors rather than systems over
//! components, so the geometry that decides whether a car turns the right way
//! can be tested directly instead of inferred from watching traffic.

use bevy::prelude::*;

/// Which side of the road traffic drives on. Right-hand rule.
pub const RIGHT_HAND_TRAFFIC: bool = true;

/// Unit normal pointing to the right of `direction` in the XZ plane.
///
/// Derived from `cross(forward, up)`, which is how Bevy defines a transform's
/// right axis: for forward `(dx, 0, dz)` and up `+Y` that is `(-dz, 0, dx)`.
/// Getting this backwards is invisible in a static screenshot and puts every
/// car in the oncoming lane, so it has its own test.
pub fn right_of(direction: Vec2) -> Vec2 {
    Vec2::new(-direction.y, direction.x)
}

/// How much of a kerb a parked row takes out of a carriageway.
///
/// Nothing about the *travel* lane can be worked out without this, which is
/// exactly the mistake this replaces: the lane was a quarter of the width, the
/// parked row was measured from the kerb inwards, and the two were never
/// compared. On Landshut's median street — seven and a half metres, and 82% of
/// the town is exactly that — they overlapped by more than a metre for the
/// whole length of every street, so the traffic drove down the middle of the
/// parked cars, the cyclists rode through them, and every traffic car that
/// spawned inside one was fired out of it by the solver.
///
/// Two depths, because there are two kinds of street. On a wide one anything
/// parks and the row is as deep as the widest thing in the city; on a narrow
/// one only the saloons fit and the row is shallower.
pub const PARKED_ROW_WIDE: f32 = 2.50;
pub const PARKED_ROW_NARROW: f32 = 2.25;

/// The widest half-width allowed to park on a one-sided street.
pub const NARROW_STREET_HALF_WIDTH: f32 = 0.95;

/// And the widest half-width of anything that *drives*, which is what a travel
/// lane has to leave room for.
const LANE_CAR_HALF: f32 = 1.05;

/// Where a street of a given width parks.
///
/// The thresholds are arithmetic rather than taste. Two parked rows and two
/// lanes need about ten metres; one row and two lanes need about seven. Under
/// that a street is an alley and nobody parks in it — which the game used to do
/// anyway, on carriageways with no room, putting the parked row past its own
/// centreline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Parking {
    None,
    /// One kerb only, and which one is a property of the street — see
    /// [`parked_kerb`]. This is what makes a real old town work: 82% of
    /// Landshut is 7.5 m across, which is one parked row and two tight lanes,
    /// and treating those streets as unparkable emptied the whole Altstadt of
    /// cars.
    OneSide,
    BothSides,
}

pub const ONE_SIDE_WIDTH: f32 = 6.9;
pub const BOTH_SIDES_WIDTH: f32 = 9.6;

pub fn parking_for(width: f32) -> Parking {
    if width >= BOTH_SIDES_WIDTH {
        Parking::BothSides
    } else if width >= ONE_SIDE_WIDTH {
        Parking::OneSide
    } else {
        Parking::None
    }
}

/// How deep the parked row on this street is.
pub fn parked_depth(width: f32) -> f32 {
    match parking_for(width) {
        Parking::None => 0.0,
        Parking::OneSide => PARKED_ROW_NARROW,
        Parking::BothSides => PARKED_ROW_WIDE,
    }
}

/// The world-space normal pointing at the kerb a one-sided row stands on.
///
/// A property of the *street*, not of the direction it is being driven, so it
/// has to give the same answer to the spawner that parks the cars, to the
/// traffic deciding which half of the road is left, and to the cyclist tucking
/// in beside them. Hence a hash of where the street is rather than a draw from
/// an RNG stream: none of the three visits the streets in the same order, and
/// a stream would hand them three different answers.
pub fn parked_kerb(a: Vec2, b: Vec2) -> Vec2 {
    // Canonical end first, so reversing the street cannot flip the kerb.
    let (p, q) = if (a.x, a.y) <= (b.x, b.y) {
        (a, b)
    } else {
        (b, a)
    };
    let Ok(direction) = Dir2::new(q - p) else {
        return Vec2::ZERO;
    };
    let mid = p.midpoint(q);
    let mut key = ((mid.x * 10.0) as i64).wrapping_mul(73_856_093)
        ^ ((mid.y * 10.0) as i64).wrapping_mul(19_349_663);
    key ^= key >> 17;
    key = key.wrapping_mul(0x9E37_79B9_7F4A_7C15u64 as i64);
    key ^= key >> 31;
    right_of(*direction) * if key & 1 == 0 { 1.0 } else { -1.0 }
}

/// How far from the centreline the travel lane runs.
///
/// `parked_beside` says whether the row is on *this* driver's side. It matters:
/// on a one-sided street the half with the parked cars has barely a lane left
/// and the other half has a wide one, so both drivers shift away from the row
/// together and the road still holds two directions.
pub fn lane_offset(width: f32, parked_beside: bool) -> f32 {
    let row = parked_depth(width);
    let (mine, theirs) = match parking_for(width) {
        Parking::None => (0.0, 0.0),
        Parking::BothSides => (row, row),
        Parking::OneSide if parked_beside => (row, 0.0),
        Parking::OneSide => (0.0, row),
    };
    // What is left of the carriageway once both rows have taken their share,
    // and where the middle of it has moved to.
    let free = (width - mine - theirs).max(0.0);
    let centre = (theirs - mine) * 0.5;
    let ideal = centre + free * 0.25;
    // Never so far out that the car's own flank reaches the row it is passing.
    let limit = width * 0.5 - mine - LANE_CAR_HALF;
    ideal.min(limit).max(0.0)
}

/// How far apart two cars passing each other on this street are.
///
/// The two lanes are not the same distance from the centreline on a street
/// parked down one kerb — the driver beside the row is squeezed against the
/// middle of the road and the other one has the rest of it — so this adds the
/// two rather than doubling either, and each is measured to its own side.
pub fn passing_gap(width: f32) -> f32 {
    match parking_for(width) {
        Parking::None => lane_offset(width, false) * 2.0,
        Parking::BothSides => lane_offset(width, true) * 2.0,
        Parking::OneSide => lane_offset(width, true) + lane_offset(width, false),
    }
}

/// Daylight left between two cars passing, over and above the metal.
///
/// A hand's breadth either side. Not comfort: a car that is *exactly* as wide
/// as its half of the road is one wobble from a contact, and the wobble is
/// guaranteed — these are steered by a pure-pursuit controller on rubber
/// suspension, not railed.
const PASSING_CLEARANCE: f32 = 0.15;

/// Is this carriageway too narrow for two cars to pass at all?
///
/// Landshut's answer to this is the whole reason it exists. The bake's
/// narrowest street is four metres — 45% of the town's 45 km of road is under
/// four and a half — and on four metres `lane_offset` puts the two lanes 1.90 m
/// apart, while the cars in this game are 1.80 m across at the smallest and
/// 2.10 m at the largest. So two vans meeting in a Gasse overlap by twenty
/// centimetres, and a van meeting a hatchback by five: they touch, they stop,
/// and neither one's forward ray can even see the other, because a single ray
/// down the middle passes a car offset by 1.90 m with 85 cm to spare. What the
/// traffic recovery timer then deletes is not a navigation failure, it is two
/// cars obeying geometry.
///
/// A street this narrow is single file, and the town is full of them. It is
/// also what the real Altstadt is: you wait at the mouth of the Gasse for the
/// one coming the other way.
pub fn single_file(width: f32) -> bool {
    passing_gap(width) < LANE_CAR_HALF * 2.0 + PASSING_CLEARANCE
}

/// And how far out a bicycle rides: the kerb side of the same lane, tucked
/// just inside whatever is parked there.
pub fn cycle_offset(width: f32, parked_beside: bool) -> f32 {
    // A street parked on both kerbs has a row beside every rider, whatever the
    // caller thought. Deciding that here rather than trusting three call sites
    // to agree is the same reasoning `parked_kerb` is written for.
    let beside = parked_beside || parking_for(width) == Parking::BothSides;
    let row = if beside { parked_depth(width) } else { 0.6 };
    (width * 0.5 - row - 0.55).max(lane_offset(width, beside))
}

/// Is the parked row on the right of somebody travelling `a -> b`?
///
/// Always, on a street parked both sides; never, on one parked on neither.
pub fn parked_on_the_right(a: Vec2, b: Vec2, width: f32) -> bool {
    match parking_for(width) {
        Parking::None => false,
        Parking::BothSides => true,
        Parking::OneSide => match Dir2::new(b - a) {
            Ok(direction) => parked_kerb(a, b).dot(right_of(*direction)) > 0.0,
            Err(_) => false,
        },
    }
}

/// A point in the correct travel lane along the segment `a -> b`.
///
/// `t` runs 0..1 along the segment; the result is offset sideways so vehicles
/// keep to their own half of the carriageway rather than driving the centreline
/// head-on into oncoming traffic — and clear of whatever is parked there.
pub fn lane_point(a: Vec2, b: Vec2, width: f32, t: f32) -> Vec2 {
    let offset = lane_offset(width, parked_on_the_right(a, b, width));
    offset_point(a, b, offset, t)
}

/// A point at a given offset to the correct side of the segment `a -> b`.
pub fn offset_point(a: Vec2, b: Vec2, offset: f32, t: f32) -> Vec2 {
    let Ok(direction) = Dir2::new(b - a) else {
        return a;
    };
    let side = right_of(*direction) * offset;
    let centre = a.lerp(b, t);
    if RIGHT_HAND_TRAFFIC {
        centre + side
    } else {
        centre - side
    }
}

/// Flattened forward and right axes of a transform, in the XZ plane.
pub fn ground_axes(transform: &Transform) -> (Vec2, Vec2) {
    let forward = transform.forward();
    let right = transform.right();
    (
        Vec2::new(forward.x, forward.z).normalize_or_zero(),
        Vec2::new(right.x, right.z).normalize_or_zero(),
    )
}

/// A push away from every neighbour inside `radius`, in the XZ plane.
///
/// Inverse-linear falloff: a neighbour at the edge of the radius contributes
/// nothing, one nose-to-nose contributes a full unit of push, and several
/// neighbours sum — a citizen in a knot is pushed harder than one meeting a
/// stranger. Clamped to unit length so the sum can bend a path but never
/// catapult anybody, and a coincident neighbour contributes nothing rather
/// than a NaN: the solver owns actual depenetration, this only owns intent.
pub fn separation(me: Vec2, neighbours: &[Vec2], radius: f32) -> Vec2 {
    let mut push = Vec2::ZERO;
    for other in neighbours {
        let away = me - *other;
        let distance = away.length();
        if distance >= radius || distance <= 1e-4 {
            continue;
        }
        push += away / distance * (1.0 - distance / radius);
    }
    push.clamp_length_max(1.0)
}

/// Steering input in -1..1 that turns towards `to_target`.
///
/// Positive is right, matching `VehicleInput::steer`.
pub fn steer_towards(forward: Vec2, right: Vec2, to_target: Vec2) -> f32 {
    let Ok(direction) = Dir2::new(to_target) else {
        return 0.0;
    };
    let lateral = direction.dot(right);
    let ahead = direction.dot(forward);

    if ahead <= 0.0 {
        // Target is behind us; commit to full lock on the side it lies on
        // rather than letting a near-zero lateral component dither.
        if lateral >= 0.0 { 1.0 } else { -1.0 }
    } else {
        (lateral * 2.5).clamp(-1.0, 1.0)
    }
}

/// Throttle in -1..1 to converge on `desired` from `current` speed (m/s).
pub fn throttle_for_speed(current: f32, desired: f32) -> f32 {
    let error = desired - current;
    // Deadband stops the AI oscillating on and off the power at cruise.
    if error.abs() < 0.4 {
        0.0
    } else {
        (error * 0.35).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_of_matches_bevys_right_axis() {
        // Facing +Z, a transform's right axis points towards -X.
        let forward = Vec3::new(0.0, 0.0, 1.0);
        let expected = forward.cross(Vec3::Y).normalize();
        let got = right_of(Vec2::new(0.0, 1.0));
        assert!(
            (got - Vec2::new(expected.x, expected.z)).length() < 1e-5,
            "right_of gave {got:?}, cross product says {expected:?}"
        );
    }

    #[test]
    fn lane_point_keeps_right_of_the_centreline() {
        let a = Vec2::ZERO;
        let b = Vec2::new(0.0, 100.0); // heading towards +Z
        let point = lane_point(a, b, 10.0, 0.5);

        // Travelling +Z, "right" is -X.
        assert!(point.x < 0.0, "lane point {point:?} is on the wrong side");
        assert!((point.y - 50.0).abs() < 1e-4);
        // Not a quarter of the width any more: a ten-metre street is parked
        // on, and the lane is the middle of what the parked row leaves.
        assert!(
            (point.x.abs() - lane_offset(10.0, true)).abs() < 1e-4,
            "lane point {point:?} does not sit in the lane"
        );
        assert!(
            point.x.abs() < 2.5,
            "the lane is still where it was before the parked row was accounted for"
        );
    }

    #[test]
    fn opposing_directions_use_opposite_lanes() {
        let a = Vec2::ZERO;
        let b = Vec2::new(100.0, 0.0);
        let outbound = lane_point(a, b, 12.0, 0.5);
        let inbound = lane_point(b, a, 12.0, 0.5);
        assert!(
            (outbound.y - inbound.y).abs() > 1.0,
            "traffic in both directions landed in the same lane: {outbound:?} / {inbound:?}"
        );
    }

    #[test]
    fn steering_turns_the_shorter_way() {
        let forward = Vec2::new(0.0, -1.0);
        let right = Vec2::new(1.0, 0.0);

        assert!(
            steer_towards(forward, right, Vec2::new(10.0, -10.0)) > 0.0,
            "right"
        );
        assert!(
            steer_towards(forward, right, Vec2::new(-10.0, -10.0)) < 0.0,
            "left"
        );
        assert!(
            steer_towards(forward, right, Vec2::new(0.0, -10.0)).abs() < 1e-3,
            "straight ahead needs no steering"
        );
    }

    #[test]
    fn a_target_behind_gets_full_lock() {
        let forward = Vec2::new(0.0, -1.0);
        let right = Vec2::new(1.0, 0.0);
        assert_eq!(steer_towards(forward, right, Vec2::new(1.0, 10.0)), 1.0);
        assert_eq!(steer_towards(forward, right, Vec2::new(-1.0, 10.0)), -1.0);
    }

    #[test]
    fn separation_pushes_away_from_a_near_neighbour() {
        let push = separation(Vec2::ZERO, &[Vec2::new(0.3, 0.0)], 1.0);
        assert!(push.x < 0.0, "should push away from +X, got {push:?}");
        assert!(push.y.abs() < 1e-5);
    }

    #[test]
    fn separation_is_zero_when_alone_or_at_arms_length() {
        assert_eq!(separation(Vec2::ZERO, &[], 1.0), Vec2::ZERO);
        let at_radius = separation(Vec2::ZERO, &[Vec2::new(1.0, 0.0)], 1.0);
        assert_eq!(at_radius, Vec2::ZERO, "the edge of the radius is peace");
    }

    #[test]
    fn a_knot_pushes_harder_than_a_stranger_but_never_catapults() {
        let one = separation(Vec2::ZERO, &[Vec2::new(0.5, 0.0)], 1.0).length();
        let two = separation(
            Vec2::ZERO,
            &[Vec2::new(0.5, 0.1), Vec2::new(0.5, -0.1)],
            1.0,
        )
        .length();
        assert!(two > one, "two neighbours should push harder than one");
        let crush = separation(
            Vec2::ZERO,
            &[
                Vec2::new(0.1, 0.0),
                Vec2::new(0.1, 0.05),
                Vec2::new(0.1, -0.05),
            ],
            1.0,
        );
        assert!(crush.length() <= 1.0 + 1e-5, "the push is capped at a unit");
    }

    #[test]
    fn standing_inside_somebody_is_the_solvers_problem() {
        // Two flummis at the same spot: depenetration belongs to physics, and
        // a direction invented from noise would fling them somewhere random.
        assert_eq!(separation(Vec2::ZERO, &[Vec2::ZERO], 1.0), Vec2::ZERO);
    }

    #[test]
    fn throttle_closes_on_the_target_speed() {
        assert!(
            throttle_for_speed(0.0, 12.0) > 0.5,
            "should accelerate hard"
        );
        assert!(throttle_for_speed(20.0, 12.0) < 0.0, "should back off");
        assert_eq!(throttle_for_speed(12.0, 12.0), 0.0, "cruise is hands-off");
        assert!(throttle_for_speed(11.8, 12.0).abs() < 1e-6, "deadband");
    }

    /// Nothing that moves drives through what is parked.
    ///
    /// The lane used to be a quarter of the carriageway and the parked row was
    /// measured inwards from the kerb, and the two were never compared. On the
    /// widths a real town actually has — 82% of Landshut is exactly seven and
    /// a half metres — they overlapped by more than a metre for the whole
    /// length of every street, so the ambient traffic drove down the middle of
    /// the parked cars and the cyclists rode through them.
    #[test]
    fn a_moving_car_never_shares_ground_with_a_parked_one() {
        // The widths the game actually builds: Landshut's narrowest, the one
        // four streets in five are, its main roads, and the generator's table.
        for width in [4.8f32, 6.0, 7.5, 9.5, 12.0, 13.9, 17.0] {
            let row = parked_depth(width);
            for parked_beside in [false, true] {
                if parking_for(width) == Parking::OneSide && !parked_beside {
                    // The far half of a one-sided street has no row in it, so
                    // there is nothing to clash with; it only has to stay on
                    // the carriageway.
                    assert!(
                        lane_offset(width, false) + LANE_CAR_HALF <= width * 0.5 + 1e-3,
                        "a {width}m street ran its clear lane onto the pavement"
                    );
                    continue;
                }
                if row == 0.0 {
                    assert!(
                        lane_offset(width, parked_beside) > 0.0,
                        "a {width}m street put its lane on the centreline"
                    );
                    continue;
                }
                // Where the row's inner flank is, from the centreline. Measured
                // against the widest thing allowed to stand in it.
                let widest = match parking_for(width) {
                    Parking::OneSide => NARROW_STREET_HALF_WIDTH,
                    _ => 1.05,
                };
                let flank = width * 0.5 - 2.0 * widest - 0.34;
                let lane = lane_offset(width, parked_beside) + LANE_CAR_HALF;
                assert!(
                    lane <= flank + 1e-3,
                    "on a {width}m street traffic reaches {lane:.2}m and the parked row starts at {flank:.2}m"
                );
                let bike = cycle_offset(width, parked_beside) + 0.4;
                assert!(
                    bike <= flank + 1e-3,
                    "on a {width}m street a bike reaches {bike:.2}m into a row starting at {flank:.2}m"
                );
            }
        }
    }

    /// A bike rides nearer the kerb than the cars do.
    #[test]
    fn a_bike_keeps_out_of_the_middle_of_the_lane() {
        for width in [4.8f32, 7.5, 9.5, 13.9, 17.0] {
            for beside in [false, true] {
                assert!(
                    cycle_offset(width, beside) >= lane_offset(width, beside),
                    "a bike on a {width}m street rides inside the traffic"
                );
                assert!(
                    cycle_offset(width, beside) < width * 0.5,
                    "a bike on a {width}m street rides on the pavement"
                );
            }
        }
    }

    /// Which kerb a one-sided street parks on is a fact about the street.
    ///
    /// Three separate systems ask it — the spawner that parks the cars, the
    /// traffic working out which half of the road is left, and the cyclist
    /// tucking in beside them — and two of them see the street the other way
    /// round. Any disagreement puts moving cars through parked ones on half
    /// the streets in the town, which is the failure this whole model exists
    /// to close.
    #[test]
    fn a_street_parks_on_the_same_kerb_whichever_way_it_is_driven() {
        for (a, b) in [
            (Vec2::new(-40.0, 12.0), Vec2::new(35.0, -8.0)),
            (Vec2::new(3.0, 0.0), Vec2::new(3.0, 90.0)),
            (Vec2::new(-120.5, -60.25), Vec2::new(-60.0, -61.0)),
        ] {
            let there = parked_kerb(a, b);
            let back = parked_kerb(b, a);
            assert!(
                there.distance(back) < 1e-4,
                "the row moved kerbs when the street was driven the other way: {there:?} / {back:?}"
            );
            // And the two directions of travel disagree about whether it is on
            // *their* right, which is the whole point of asking.
            let width = 7.5;
            assert_ne!(
                parked_on_the_right(a, b, width),
                parked_on_the_right(b, a, width),
                "both directions think the parked row is on their right"
            );
        }
    }

    #[test]
    fn a_four_metre_gasse_cannot_hold_two_cars_abreast() {
        // The measured case: 4.0 m is the bake's narrowest street and 45% of
        // Landshut is under 4.5. The widest car is 2.10 m across.
        assert!((passing_gap(4.0) - 1.90).abs() < 1e-5);
        assert!(single_file(4.0));
        // And the smallest street two of them do fit on.
        assert!(!single_file(5.0));
        assert!(passing_gap(5.0) >= LANE_CAR_HALF * 2.0);
    }

    #[test]
    fn a_parked_row_does_not_make_an_ordinary_street_single_file() {
        // The one that would quietly empty the town: the moment a street is
        // wide enough to park on, a row eats a lane's worth of it, and a rule
        // written as "how wide is the street" rather than "what is left of it"
        // reads 7.5 m — 82% of Landshut — as an alley.
        for width in [6.9, 7.5, 8.0, 9.6, 13.6, 15.1] {
            assert!(
                !single_file(width),
                "{width} m came out single file, gap {}",
                passing_gap(width)
            );
        }
    }

    #[test]
    fn passing_adds_the_two_lanes_rather_than_doubling_one() {
        // A street parked down one kerb is the asymmetric case, and doubling
        // either lane gets it wrong in a different direction.
        let width = 7.5;
        let squeezed = lane_offset(width, true);
        let roomy = lane_offset(width, false);
        assert!(squeezed < roomy);
        assert!((passing_gap(width) - (squeezed + roomy)).abs() < 1e-5);
    }
}
