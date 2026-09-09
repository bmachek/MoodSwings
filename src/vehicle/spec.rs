//! Vehicle definitions.
//!
//! Handling is arcade, not simulation. The games this borrows from never
//! modelled a differential or a tyre slip curve, and trying to would make the
//! cars *worse*: what matters is that they turn in predictably, break traction
//! when provoked, and recover without punishing the player. Every constant here
//! is therefore a feel knob, exposed to the dev panel.

use bevy::prelude::*;

/// Wheel ordering used everywhere: front-left, front-right, rear-left, rear-right.
pub const WHEEL_COUNT: usize = 4;

/// How much daylight a settled car's collider leaves over a kerb.
///
/// Enough that a kerb streaming in under a parked car is not a body spawned
/// inside a static box — which Avian resolves the only way it can, and which
/// the patrol once caught as a parked car leaving the ground at thirteen
/// metres a second.
///
/// Six centimetres rather than the two it started at. Two is enough on paper —
/// the box floor lands at 0.30 against a 0.28 kerb — and it is not enough in
/// practice, because a car settles onto whatever is under it and a couple of
/// centimetres of suspension travel is all it takes to close the gap. The
/// patrol caught exactly that, once, a hundred seconds into a run.
const KERB_DAYLIGHT: f32 = 0.06;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VehicleClass {
    Sedan,
    Coupe,
    Sports,
    Pickup,
    Truck,
}

impl VehicleClass {
    /// Everything that turns up as traffic or parked at a kerb.
    pub const CIVILIAN: [VehicleClass; 5] = [
        VehicleClass::Sedan,
        VehicleClass::Coupe,
        VehicleClass::Sports,
        VehicleClass::Pickup,
        VehicleClass::Truck,
    ];

    /// Every class there is. Used to build the meshes and to make sure a test
    /// covers all of them.
    pub const ALL: [VehicleClass; 5] = VehicleClass::CIVILIAN;

    pub fn spec(self) -> VehicleSpec {
        match self {
            VehicleClass::Sedan => VehicleSpec::sedan(),
            VehicleClass::Coupe => VehicleSpec::coupe(),
            VehicleClass::Sports => VehicleSpec::sports(),
            VehicleClass::Pickup => VehicleSpec::pickup(),
            VehicleClass::Truck => VehicleSpec::truck(),
        }
    }
}

#[derive(Component, Debug, Clone)]
pub struct VehicleSpec {
    pub class: VehicleClass,
    pub display_name: &'static str,

    // --- body ---
    /// How big the bodywork is: half width, half height, half length.
    ///
    /// Read by `body::build` as the scale of the three lofts, by
    /// `bounce::launch` as the volume to sweep, and — through
    /// [`collider_half_extents`](Self::collider_half_extents) — as the size of
    /// the box the car collides as. It is *not* the collider directly any more;
    /// see there for why.
    pub half_extents: Vec3,
    pub mass: f32,
    /// Offset of the centre of mass from the body origin. Lowering it is the
    /// single most effective thing preventing the car from tipping in corners.
    ///
    /// Relative to the origin, so it moves when the ride height does. When the
    /// ride heights were dropped to put the wheel arches back on the tyres,
    /// every one of these was raised by exactly as much, so the centre of mass
    /// stayed where it was above the road and no car's handling changed.
    pub center_of_mass: Vec3,

    // --- wheels ---
    pub wheel_base: f32,
    pub track: f32,
    pub wheel_radius: f32,
    /// Height of the wheel anchors relative to the body origin.
    ///
    /// This and [`suspension_rest`](Self::suspension_rest) together decide the
    /// ride height, and therefore where the tyre sits inside the wheel arch —
    /// which is a fact about the *bodywork*, a table away in `vehicle::body`.
    /// The two drifted apart, and what that looked like is a car hanging a
    /// quarter of a metre of daylight between the top of every tyre and the
    /// arch above it: every car in the city visibly floating, on wheels that
    /// read as detached. `the_arch_sits_on_the_tyre` in `vehicle::body` is the
    /// invariant that ties them together now, and it is the reason these are
    /// three decimal places rather than two.
    pub axle_height: f32,

    // --- suspension ---
    /// How far the spring is extended with no load on it.
    ///
    /// Minus the compression the car's own weight causes, this is the travel
    /// left before the body bottoms out — a fifth of a metre or so, which is
    /// soft for a road car and deliberately so.
    pub suspension_rest: f32,
    pub spring_strength: f32,
    /// Resistance to the spring's own motion, in newtons per m/s.
    ///
    /// Set at roughly a fifth of critical, which on a real car would be a
    /// fault. Here it is the point: the suspension is meant to give the body
    /// back most of what a kerb puts into it, so that a car crossing a junction
    /// at speed leaves the ground and lands like everything else in this city.
    pub damping: f32,
    /// Resists body roll by coupling the two wheels on an axle.
    pub anti_roll: f32,

    // --- drivetrain ---
    pub engine_force: f32,
    pub brake_force: f32,
    pub reverse_force: f32,
    pub max_speed: f32,
    pub drag: f32,
    /// Downforce coefficient; scales with speed squared.
    pub downforce: f32,

    // --- steering ---
    pub max_steer: f32,
    /// How fast the steering angle chases the input, in 1/s. The felt lag is
    /// roughly 2/rate to settle: 13 → ~150ms, which reads as direct with
    /// digital keys. The first round of values (5.5–10) read as towing a
    /// caravan — every class was raised once players called it laggy.
    pub steer_rate: f32,
    /// Fraction of steering lock still available at top speed.
    pub high_speed_steer: f32,

    // --- grip ---
    /// Friction coefficients against wheel load. Rear below front gives mild
    /// oversteer, which is what makes a car feel lively rather than inert.
    pub front_grip: f32,
    pub rear_grip: f32,
    /// Rear grip multiplier while the handbrake is held; this is the drift.
    pub handbrake_grip: f32,
    /// How much of the tyre's lateral force is allowed to roll the body.
    ///
    /// Applying cornering force at the true contact patch is honest and puts
    /// the car on its roof: arcade grip levels sit well above the rollover
    /// threshold that a real car's tyres would slide at first. Applying it
    /// nearer the centre of mass keeps the bite and drops the roll moment.
    /// 0 = no body roll at all, 1 = full physical roll.
    pub roll_couple: f32,

    pub body_color: Color,
    /// How much of the paint is flake rather than pigment. 0 is a solid
    /// colour, 1 a full metallic. Distinct from the clearcoat, which every car
    /// has: this is what is *under* the lacquer.
    pub body_metallic: f32,
    /// How long this one has been parked outside, 0 to 1. Not damage — a
    /// rubber car cannot be damaged — but the finish: see `paint::finish`.
    pub body_age: f32,
}

impl VehicleSpec {
    /// World-space-agnostic wheel anchor positions in body-local coordinates.
    /// Bevy's forward is -Z, so the front axle sits at negative Z.
    pub fn wheel_anchors(&self) -> [Vec3; WHEEL_COUNT] {
        let half_track = self.track * 0.5;
        let half_base = self.wheel_base * 0.5;
        [
            Vec3::new(-half_track, self.axle_height, -half_base),
            Vec3::new(half_track, self.axle_height, -half_base),
            Vec3::new(-half_track, self.axle_height, half_base),
            Vec3::new(half_track, self.axle_height, half_base),
        ]
    }

    pub fn is_front(index: usize) -> bool {
        index < 2
    }

    /// Share of the vehicle's mass carried by one wheel.
    pub fn wheel_mass_share(&self) -> f32 {
        self.mass / WHEEL_COUNT as f32
    }

    /// Suspension travel available before the body bottoms out.
    pub fn max_ray_length(&self) -> f32 {
        self.suspension_rest + self.wheel_radius
    }

    /// How far the springs are squashed by the car's own weight.
    ///
    /// One function rather than the four copies of `mass · g / (4 · k)` that
    /// were spread across the spawner, the controller and two tests. They
    /// agreed, which is the only reason it never showed.
    pub fn rest_compression(&self) -> f32 {
        self.wheel_mass_share() * 9.81 / self.spring_strength
    }

    /// Height the body origin settles at once the springs balance the weight.
    pub fn resting_height(&self) -> f32 {
        (self.max_ray_length() - self.rest_compression()) - self.axle_height
    }

    /// The box the car is *felt* as, which is not the box it is drawn as.
    ///
    /// Two different jobs used to share [`half_extents`](Self::half_extents),
    /// and dropping the ride heights is what pulled them apart. Centred on the
    /// body origin, a box a car's height reached from well above the roof to
    /// well below the sill — at the old ride height its floor was forty
    /// centimetres up, comfortably over every kerb in the city, which is why
    /// cars glided across kerbs without ever touching one and why the parked
    /// ones survived a kerb streaming in underneath them. Lower the car by a
    /// third of a metre with that box still centred and the floor drops under
    /// the kerb: every parked car in Landshut would be ejected by the kerb
    /// arriving inside it, which is the thirteen-metres-a-second launch the
    /// spawner already carries a comment about.
    ///
    /// So the box is fitted between two heights instead: its floor clears a
    /// kerb by a couple of centimetres, and its ceiling stays where the
    /// bodywork's own half-height puts it. A car mounts a kerb the way a real
    /// one does — on its wheels, through the suspension — rather than by
    /// hovering over it.
    pub fn collider_half_extents(&self) -> Vec3 {
        let (floor, ceiling) = self.collider_span();
        Vec3::new(
            self.half_extents.x,
            (ceiling - floor) * 0.5,
            self.half_extents.z,
        )
    }

    /// Where that box sits, relative to the body origin.
    pub fn collider_offset(&self) -> f32 {
        let (floor, ceiling) = self.collider_span();
        (floor + ceiling) * 0.5 - self.resting_height()
    }

    /// The two heights above the road the collider is fitted between.
    fn collider_span(&self) -> (f32, f32) {
        (
            crate::world::buildings::SIDEWALK_HEIGHT + KERB_DAYLIGHT,
            self.resting_height() + self.half_extents.y,
        )
    }

    fn sedan() -> Self {
        Self {
            class: VehicleClass::Sedan,
            display_name: "Sedan",
            half_extents: Vec3::new(0.90, 0.58, 2.20),
            mass: 1400.0,
            center_of_mass: Vec3::new(0.0, -0.135, 0.0),
            wheel_base: 2.75,
            track: 1.56,
            wheel_radius: 0.34,
            axle_height: -0.145,
            suspension_rest: 0.32,
            spring_strength: 32_000.0,
            damping: 1_280.0,
            anti_roll: 9_000.0,
            engine_force: 13_500.0,
            brake_force: 26_000.0,
            reverse_force: 7_000.0,
            max_speed: 40.0,
            drag: 3.2,
            downforce: 9.0,
            max_steer: 0.56,
            steer_rate: 13.0,
            high_speed_steer: 0.40,
            front_grip: 1.60,
            rear_grip: 1.45,
            handbrake_grip: 0.22,
            roll_couple: 0.30,
            body_color: Color::srgb(0.72, 0.24, 0.22),
            body_metallic: 0.35,
            body_age: 0.0,
        }
    }

    /// Long bonnet, short deck, far too much engine. Quick in a straight line
    /// and unwilling to change direction, which is the entire character.
    fn coupe() -> Self {
        Self {
            class: VehicleClass::Coupe,
            display_name: "Coupe",
            half_extents: Vec3::new(0.94, 0.55, 2.45),
            mass: 1620.0,
            center_of_mass: Vec3::new(0.0, -0.225, 0.0),
            wheel_base: 2.95,
            track: 1.62,
            wheel_radius: 0.36,
            axle_height: -0.185,
            suspension_rest: 0.34,
            spring_strength: 34_000.0,
            damping: 1_240.0,
            anti_roll: 8_000.0,
            engine_force: 22_000.0,
            brake_force: 25_000.0,
            reverse_force: 8_000.0,
            max_speed: 50.0,
            drag: 3.4,
            downforce: 7.0,
            max_steer: 0.50,
            steer_rate: 11.0,
            high_speed_steer: 0.36,
            front_grip: 1.55,
            // Well under the front: this is a car that leaves in a cloud of
            // its own tyre smoke if you ask it to.
            rear_grip: 1.28,
            handbrake_grip: 0.16,
            roll_couple: 0.38,
            body_color: Color::srgb(0.62, 0.18, 0.16),
            body_metallic: 0.35,
            body_age: 0.0,
        }
    }

    fn sports() -> Self {
        Self {
            class: VehicleClass::Sports,
            display_name: "Sports",
            half_extents: Vec3::new(0.92, 0.46, 2.15),
            mass: 1150.0,
            center_of_mass: Vec3::new(0.0, -0.192, 0.0),
            wheel_base: 2.60,
            track: 1.64,
            wheel_radius: 0.33,
            axle_height: -0.152,
            suspension_rest: 0.28,
            spring_strength: 36_000.0,
            damping: 1_440.0,
            anti_roll: 14_000.0,
            engine_force: 19_000.0,
            brake_force: 30_000.0,
            reverse_force: 7_500.0,
            max_speed: 55.0,
            drag: 2.6,
            downforce: 16.0,
            max_steer: 0.60,
            steer_rate: 15.0,
            high_speed_steer: 0.34,
            front_grip: 1.85,
            rear_grip: 1.62,
            handbrake_grip: 0.18,
            roll_couple: 0.22,
            body_color: Color::srgb(0.90, 0.72, 0.16),
            body_metallic: 0.35,
            body_age: 0.0,
        }
    }

    /// Body-on-frame pickup: a cab and an open bed. Rides high, leans, and
    /// carries its weight where a saloon does not.
    fn pickup() -> Self {
        Self {
            class: VehicleClass::Pickup,
            display_name: "Pickup",
            half_extents: Vec3::new(1.00, 0.78, 2.65),
            mass: 2300.0,
            center_of_mass: Vec3::new(0.0, -0.095, 0.0),
            wheel_base: 3.20,
            track: 1.72,
            wheel_radius: 0.42,
            axle_height: -0.175,
            suspension_rest: 0.38,
            spring_strength: 44_000.0,
            damping: 1_760.0,
            anti_roll: 11_000.0,
            engine_force: 18_000.0,
            brake_force: 32_000.0,
            reverse_force: 9_000.0,
            max_speed: 36.0,
            drag: 4.4,
            downforce: 6.0,
            max_steer: 0.50,
            steer_rate: 10.0,
            high_speed_steer: 0.46,
            front_grip: 1.46,
            // An empty bed over the driven axle is the reason a pickup steps
            // out in the wet.
            rear_grip: 1.30,
            handbrake_grip: 0.26,
            roll_couple: 0.42,
            body_color: Color::srgb(0.32, 0.46, 0.38),
            body_metallic: 0.20,
            body_age: 0.0,
        }
    }

    fn truck() -> Self {
        Self {
            class: VehicleClass::Truck,
            display_name: "Truck",
            half_extents: Vec3::new(1.05, 0.95, 2.85),
            mass: 3200.0,
            center_of_mass: Vec3::new(0.0, -0.115, 0.0),
            wheel_base: 3.40,
            track: 1.80,
            wheel_radius: 0.46,
            axle_height: -0.185,
            suspension_rest: 0.40,
            spring_strength: 62_000.0,
            damping: 2_480.0,
            anti_roll: 16_000.0,
            engine_force: 24_000.0,
            brake_force: 42_000.0,
            reverse_force: 11_000.0,
            max_speed: 30.0,
            drag: 5.5,
            downforce: 5.0,
            max_steer: 0.46,
            steer_rate: 8.0,
            high_speed_steer: 0.50,
            front_grip: 1.40,
            rear_grip: 1.34,
            handbrake_grip: 0.30,
            roll_couple: 0.45,
            body_color: Color::srgb(0.35, 0.42, 0.52),
            body_metallic: 0.35,
            body_age: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_anchors_are_symmetric_and_correctly_ordered() {
        for class in VehicleClass::ALL {
            let spec = class.spec();
            let [fl, fr, rl, rr] = spec.wheel_anchors();

            assert!(fl.z < 0.0 && fr.z < 0.0, "front axle must be at -Z");
            assert!(rl.z > 0.0 && rr.z > 0.0, "rear axle must be at +Z");
            assert!(fl.x < 0.0 && rl.x < 0.0, "left wheels must be at -X");
            assert_eq!(fl.x, -fr.x, "track must be symmetric");
            assert!(VehicleSpec::is_front(0) && VehicleSpec::is_front(1));
            assert!(!VehicleSpec::is_front(2) && !VehicleSpec::is_front(3));
        }
    }

    #[test]
    fn suspension_can_carry_the_vehicle() {
        // If the springs cannot hold the car up at a sane ride height it will
        // sit on its belly, which reads as "the physics is broken".
        for class in VehicleClass::ALL {
            let spec = class.spec();
            let load_per_wheel = spec.mass * 9.81 / WHEEL_COUNT as f32;
            let compression = load_per_wheel / spec.spring_strength;
            assert!(
                compression > 0.02 && compression < spec.suspension_rest * 0.6,
                "{}: rest compression {compression:.3}m is not sane for {}m of travel",
                spec.display_name,
                spec.suspension_rest
            );
        }
    }

    #[test]
    fn rear_grip_never_exceeds_front() {
        // Front grip above rear gives understeer, which feels dead. Keep every
        // preset on the lively side of neutral.
        for class in VehicleClass::ALL {
            let spec = class.spec();
            assert!(spec.rear_grip <= spec.front_grip, "{}", spec.display_name);
            assert!(spec.handbrake_grip < spec.rear_grip * 0.5);
        }
    }
}
