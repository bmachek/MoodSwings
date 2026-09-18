//! Getting about by bouncing.
//!
//! Everybody in this city is a rubber ball with legs, and this is the part that
//! keeps them in the air. It replaces a floating character controller, which
//! could not be made to work here for a reason worth writing down: a floating
//! controller holds the body a fixed distance above the ground with a spring,
//! so the collider never touches anything and never forms a contact manifold.
//! Restitution is a property of a contact. A body that has no contacts cannot
//! bounce, however elastic you declare it to be.
//!
//! So the ground is found with a ray and the rebound is applied by hand. The
//! vertical speed is *assigned* at the bottom of each hop rather than added to,
//! which is what stops the solver's own restitution and this system compounding
//! into a body that climbs out of the world. Whatever the last bounce gave back,
//! the next hop leaves at the same speed — so a flummi crossing a flat street
//! keeps a steady rhythm, and one thrown off a roof still lands like rubber.
//!
//! The ground probe reaches well past the soles on purpose. A bouncing body is
//! airborne for most of its cycle, and a controller that only steers while
//! strictly touching the ground gives the player about three frames of control
//! per second. Reaching down means the lower part of every arc counts as
//! grounded, which is where the steering that matters happens anyway.
//!
//! What that reach must *not* decide is where a body lands, and for a long time
//! it did. The rebound is assigned rather than added, so a landing that fires at
//! the top of the probe's slack is a floor the body hops off without ever
//! touching the pavement — permanently, and at no cost in energy. The headless
//! harness measured both halves of it: a body's soles oscillating between 41 and
//! 85 centimetres above the street, and — once the walking gait made the resting
//! hop zero — one stepping off a 28 cm kerb sinking the last stretch at about
//! 15 cm/s, nearly three seconds to arrive, because every frame reassigned the
//! fall to nothing. So `grounded` is still the wide reach and steering still
//! reads it; `touching` is the narrow one, and a landing is the frame a body
//! arrives on it.
//!
//! A walking body has no hop to leave with, and two things follow. It bounces
//! off what it lands on at the solver's own restitution instead — applied here
//! rather than left to the solver, so that it is booked like every other
//! assignment and a jump still cannot read as an assault — and it has to pick
//! its feet up for a kerb, because the hop it used to clear one with is gone.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::boing::{Landed, PreviousVelocity};
use crate::core::config::GameConfig;
use crate::core::schedule::GameSet;

/// How far below the soles the ground probe still counts as contact, as a
/// fraction of the body's own standing height.
///
/// A fraction and not a constant, and the difference is a dog. At a flat 45cm
/// — which is what a person's soles get, and is where this number came from —
/// a dog that stands 26cm high counted as grounded while it was nearly two of
/// itself off the pavement. The rebound is *assigned* rather than added, so it
/// left the ground at full hop speed from up there, fell back to 45cm, and was
/// relaunched: no energy ever leaves the loop, and the dog spent the whole walk
/// hovering about two metres up on the end of its leash. Scaled to the body,
/// everybody on two feet keeps the reach they had — a flummi stands 85 to 90cm
/// to the soles and half of that is the constant this replaces — and something
/// knee-high gets a slack in proportion to its knees.
const GROUND_REACH: f32 = 0.5;
/// The probe starts a little above the origin so it cannot begin inside a kerb
/// the body is already standing on.
const PROBE_LIFT: f32 = 0.1;
/// How far below the soles still counts as *contact*, in metres.
///
/// Flat rather than scaled, unlike [`GROUND_REACH`], because this one is not a
/// proportion of anything: it is the slop between where the solver holds a
/// resting body and where the ray says the ground is. A few centimetres covers
/// that on a dog and on a flummi alike, and anything wider starts deciding
/// where a landing happens again.
const CONTACT_SLACK: f32 = 0.06;
/// Multiplier on the hop when somebody deliberately jumps.
pub const JUMP_SCALE: f32 = 2.6;
/// The tallest thing a body will pick its feet up for, as a fraction of its own
/// standing height.
///
/// A kerb is 0.28 m, which is a third of an adult flummi and over half a child,
/// so this has to be generous or the children of this city cannot get back onto
/// the pavement they crossed a road from. Above it a body is walking into a
/// wall, not up a step, and walking into a wall is allowed to fail.
const STEP_MAX: f32 = 0.6;
/// And the smallest worth leaving the ground for. Below this the capsule's own
/// bottom cap rides over it.
const STEP_MIN: f32 = 0.05;
/// How much air to clear a step by, in metres.
const STEP_CLEARANCE: f32 = 0.05;
/// Arrival speed, in m/s, below which a landing is a body settling rather than
/// a body falling.
///
/// A walking body in permanent contact gathers a tick of gravity between
/// frames; giving that back at the solver's restitution would be a street of
/// citizens shivering. A real fall — off a kerb is 2.3, off a jump is 7.3 —
/// is well clear of it.
const SETTLE_SPEED: f32 = 1.5;

/// A body that gets about by bouncing.
///
/// Written by whoever owns it — the input handler for the player, the pavement
/// AI for the crowd — so this module does not need to know which it is looking
/// at, in the same way [`crate::ai::figure::WalkCycle`] does not.
#[derive(Component)]
pub struct Bouncer {
    /// Ground velocity this body is trying to reach, in m/s.
    pub desired: Vec2,
    /// Scales the next hop. 1.0 is travelling; more is a jump.
    ///
    /// Written every frame by whoever owns the body, because the controller
    /// spends it at every landing — which is also why it cannot carry a
    /// one-shot; see [`Bouncer::pending`].
    pub hop_scale: f32,
    /// A single hop asked for once, held until there is a landing to spend it
    /// on.
    ///
    /// `hop_scale` cannot do this. It is rewritten every frame by the pavement
    /// AI and by `drive_player`, and a landing falls in one frame out of the
    /// twenty an arc lasts, so a scale written once is overwritten long before
    /// it is spent: the skater's ollie is documented as "spent on the next
    /// landing, exactly like a player's jump" and fired about one time in
    /// twenty. A player's jump only works because the key is *held*, so the
    /// frame the landing happens in has it set too.
    pub pending: Option<f32>,
    /// Distance from the body origin down to the soles.
    pub stand_height: f32,
    /// Whether the probe found ground under it this tick — anywhere inside its
    /// generous reach, which is most of an arc. This is what steering and a
    /// jump ask.
    pub grounded: bool,
    /// Whether the ground is actually under the soles. What a *landing* asks,
    /// and kept from one frame to the next so that arriving is an event rather
    /// than a state: a body resting on the pavement is touching it every frame
    /// and has landed on it once.
    pub touching: bool,
    /// Seconds since the last landing, and how long that whole arc lasted.
    /// Together they say where in its hop a figure is, which is what the
    /// squash and stretch is posed from.
    pub since_landing: f32,
    pub last_arc: f32,
    /// Downward speed at the last landing, in m/s. What the landing sound is
    /// pitched off.
    pub landing_speed: f32,
    /// How fast the body was falling last frame.
    ///
    /// Remembered because by the time this system sees a contact the solver
    /// may already have had the landing: it runs in `FixedPostUpdate`, after
    /// this, so a body that was coming down at seven metres a second can be
    /// sitting still — or already on its way back up — the first frame the
    /// probe calls it touching. Read off the velocity at that point, a roof
    /// drop and a kerb step are the same landing.
    pub fall_speed: f32,
}

impl Bouncer {
    pub fn new(stand_height: f32) -> Self {
        Self {
            desired: Vec2::ZERO,
            hop_scale: 1.0,
            pending: None,
            stand_height,
            grounded: false,
            touching: false,
            since_landing: 0.0,
            // Not zero: a figure spawned mid-air would otherwise be posed as if
            // it had just landed, and pop when it actually does.
            last_arc: 0.5,
            landing_speed: 0.0,
            fall_speed: 0.0,
        }
    }

    /// How far through its current hop this body is, 0 at the bottom and 1 at
    /// the next landing. Clamped, because an arc can always run long.
    pub fn hop_phase(&self) -> f32 {
        (self.since_landing / self.last_arc.max(0.05)).clamp(0.0, 1.0)
    }
}

/// A body temporarily not in charge of itself: thrown, and tumbling.
///
/// While this is on an entity the controller leaves it alone entirely, so the
/// throw carries and the solver's restitution is the only thing acting on it.
#[derive(Component)]
pub struct Launched;

pub struct BounceControllerPlugin;

impl Plugin for BounceControllerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, bounce_bodies.in_set(GameSet::Simulation));
    }
}

/// Steering a grounded body towards the speed it wants, in m/s².
///
/// A pure function of what the caller already knows, so the feel of the thing
/// is testable without a physics world: at rest it accelerates hardest, and as
/// the gap closes it eases off rather than overshooting and oscillating.
pub fn steer(current: Vec2, desired: Vec2, accel: f32, dt: f32) -> Vec2 {
    let gap = desired - current;
    let step = accel * dt;
    if gap.length() <= step {
        desired
    } else {
        current + gap.normalize() * step
    }
}

/// How far down the ground probe reaches from the body's origin.
///
/// Pure, because the thing worth pinning about it is a proportion rather than a
/// frame of physics: how much air a body is allowed to call ground.
pub fn probe_reach(stand_height: f32) -> f32 {
    stand_height * (1.0 + GROUND_REACH) + PROBE_LIFT
}

/// How far the probe may report before the body has stopped falling.
///
/// The distance at which the ray is looking straight at the soles, plus the
/// slack. Pure and separate from [`probe_reach`] because the whole point is
/// that the two are different numbers: one says how much air a body may steer
/// through, the other says where the ground is.
pub fn contact_reach(stand_height: f32) -> f32 {
    stand_height + PROBE_LIFT + CONTACT_SLACK
}

/// How far ahead of itself a body looks for a step, in metres.
///
/// A body's own width is not on the [`Bouncer`], and it is near enough a fixed
/// fraction of its height for everything that walks here — a flummi is 0.32
/// across the shoulders and 0.845 to the soles — so it is taken from the height
/// with a stride's worth of run-up added. Looking further would lift a body for
/// a kerb it then turns away from; looking closer lifts it with its nose
/// already against the stone.
pub fn step_lookahead(stand_height: f32) -> f32 {
    stand_height * 0.45 + 0.2
}

/// Upward speed, in m/s, for picking one's feet up over a step of this height.
///
/// Zero for anything too low to be worth leaving the ground for and anything
/// too tall to be a step at all — a wall reads as a step of a whole body
/// height, and walking into a wall is allowed to fail. Pure: what a kerb costs
/// is arithmetic, and it is the arithmetic a test can hold.
pub fn step_lift(step: f32, stand_height: f32) -> f32 {
    if step < STEP_MIN || step > stand_height * STEP_MAX {
        return 0.0;
    }
    (2.0 * 9.81 * (step + STEP_CLEARANCE)).sqrt()
}

pub fn bounce_bodies(
    time: Res<Time>,
    config: Res<GameConfig>,
    spatial: SpatialQuery,
    mut landings: MessageWriter<Landed>,
    bodies: Query<
        (
            Entity,
            &Transform,
            &mut LinearVelocity,
            &mut Bouncer,
            Option<&mut PreviousVelocity>,
        ),
        Without<Launched>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let tune = &config.bounce;

    for (entity, transform, mut velocity, mut bouncer, previous) in bodies {
        // Excluding the body itself is load-bearing. An unfiltered ray started
        // above the origin hits the body's own collider first, which reads as
        // ground a body-height up — and it climbs, a metre and a half a frame.
        let from = transform.translation + Vec3::Y * PROBE_LIFT;
        let reach = probe_reach(bouncer.stand_height);
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);
        let under = spatial.cast_ray(from, Dir3::NEG_Y, reach, true, &filter);
        bouncer.grounded = under.is_some();
        // The same ray read twice, at two distances. The wide one is what a
        // body may steer and jump from; the narrow one is the ground.
        let touching = under.is_some_and(|hit| hit.distance <= contact_reach(bouncer.stand_height));

        let accel = if bouncer.grounded {
            tune.ground_accel
        } else {
            tune.air_accel
        };
        let steered = steer(velocity.0.xz(), bouncer.desired, accel, dt);
        // Every assignment this controller makes is booked into the wallop
        // detector's memory, the vertical ones below included. Without it a
        // frame long enough to close the whole gap to `desired` in one step —
        // a chunk streaming in, measured at 80ms and up — reads as a knock of
        // the same size: the street boings, moods shift with nobody touching
        // anybody, and somebody starts a feud over a stutter.
        let mut booked = Vec3::new(steered.x - velocity.x, 0.0, steered.y - velocity.z);
        velocity.x = steered.x;
        velocity.z = steered.y;

        bouncer.since_landing += dt;

        // How fast this body arrived: the worse of what it is doing now and
        // what it was doing last frame, because the solver may have absorbed or
        // reflected the landing before this system saw the contact at all.
        let arriving = bouncer.fall_speed.max(-velocity.y);

        // A landing is a fall that stopped, and it has to be recognised two
        // ways. Slowly, a body is seen sitting in the contact shell and the
        // transition into it is the landing. Quickly, it is not seen there at
        // all: the shell is six centimetres and a body falling faster than
        // about four metres a second crosses the whole of it between two
        // frames, so by the time this system looks the solver has had the
        // landing and turned the body round. That is the case that matters —
        // it is every jump and every roof — so the reversal counts as
        // arriving even with no contact to show for it.
        let stopped_falling =
            bouncer.fall_speed > SETTLE_SPEED && velocity.y > -bouncer.fall_speed * 0.5;
        let landed = !bouncer.touching && bouncer.grounded && (touching || stopped_falling);
        // Having landed counts as having been in contact, whether or not the
        // probe ever saw it there. Otherwise a body caught by the reversal is
        // still inside the shell on the next frame and lands a second time on
        // the way back up, which resets the arc the squash is posed from.
        bouncer.touching = touching || landed;

        if landed {
            bouncer.landing_speed = arriving;
            bouncer.last_arc = bouncer.since_landing;
            bouncer.since_landing = 0.0;
            if arriving > SETTLE_SPEED {
                landings.write(Landed {
                    entity,
                    position: transform.translation,
                    speed: arriving,
                });
            }
        }

        // What the body leaves the ground with, and there are three ways to
        // earn it. The hop is the travelling bounce, spent at a landing. The
        // step is a kerb, and is asked for every frame a body stands against
        // one rather than only on arrival, because a walking body is already in
        // contact when it gets there. The rubber is what a gait with no hop of
        // its own still owes a real fall: the solver would apply it at the
        // contact, and applying it here instead keeps the one rule this module
        // is built on — the vertical speed is *assigned*, never added, so
        // nothing can compound into a body that climbs out of the world.
        let hop = if landed {
            let asked = bouncer.hop_scale.max(bouncer.pending.take().unwrap_or(0.0));
            tune.hop_speed * asked
        } else {
            0.0
        };
        let step = if touching {
            let ahead = Vec2::new(bouncer.desired.x, bouncer.desired.y);
            match Dir2::new(ahead) {
                Ok(facing) if ahead.length() > 0.2 => {
                    let toe = from
                        + Vec3::new(facing.x, 0.0, facing.y) * step_lookahead(bouncer.stand_height);
                    spatial
                        .cast_ray(toe, Dir3::NEG_Y, reach, true, &filter)
                        .map(|hit| {
                            step_lift(
                                bouncer.stand_height + PROBE_LIFT - hit.distance,
                                bouncer.stand_height,
                            )
                        })
                        .unwrap_or(0.0)
                }
                _ => 0.0,
            }
        } else {
            0.0
        };
        let rubber = if landed && hop <= 0.0 && arriving > SETTLE_SPEED {
            arriving * tune.restitution
        } else {
            0.0
        };

        let lift = hop.max(step).max(rubber);
        if lift > 0.0 {
            booked.y += lift - velocity.y;
            velocity.y = lift;
        } else if landed {
            // Arrived with nothing to give back: stop, rather than leave a
            // tick of gravity for the solver to argue with.
            booked.y += -velocity.y;
            velocity.y = 0.0;
        }
        if landed {
            // A jump is asked for once and spent once; holding the key down
            // must not turn into a pogo stick to the roofline.
            bouncer.hop_scale = 1.0;
        }

        // Last thing, so that next frame's landing can ask how fast this body
        // was falling before anything got hold of it. After a lift it is zero,
        // which is the truth: a rising body is not falling.
        bouncer.fall_speed = (-velocity.y).max(0.0);

        if let Some(mut previous) = previous {
            previous.0 += booked;
            if landed {
                // The whole vertical change at a landing, not only this
                // system's share of it: between two frames the solver may have
                // reflected the arrival itself, and a body coming down off its
                // own jump has not been assaulted by the pavement. What a
                // landing is worth is said by `Landed`, which makes a sound
                // and moves nobody's mood. Being *thrown* still registers,
                // because a launched body is skipped by this system entirely
                // and nothing books anything for it.
                previous.0.y = velocity.y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steering_closes_the_gap_without_overshooting_it() {
        // One step larger than the gap must land exactly on the target, not
        // sail past it and come back — that is an oscillation, and it reads as
        // a body that cannot decide where it is going.
        let landed = steer(Vec2::ZERO, Vec2::new(3.0, 0.0), 100.0, 1.0);
        assert_eq!(landed, Vec2::new(3.0, 0.0));
    }

    #[test]
    fn steering_takes_a_bounded_step_when_the_gap_is_wide() {
        let step = steer(Vec2::ZERO, Vec2::new(40.0, 0.0), 10.0, 0.1);
        assert!(
            (step.length() - 1.0).abs() < 1e-4,
            "took a {} m/s step where the budget was 1.0",
            step.length()
        );
    }

    #[test]
    fn a_body_already_at_speed_is_left_alone() {
        let held = Vec2::new(0.0, 6.0);
        assert_eq!(steer(held, held, 42.0, 0.016), held);
    }

    #[test]
    fn a_hop_is_a_fraction_of_a_second_rather_than_a_moon_jump() {
        // Time to fall back from the top of one hop, under Earth gravity. Long
        // hops read as low gravity, which is a different joke from rubber.
        let hop = GameConfig::default().bounce.hop_speed;
        let arc = 2.0 * hop / 9.81;
        assert!(
            (0.25..0.85).contains(&arc),
            "a hop lasting {arc:.2}s is not a bounce"
        );
    }

    /// Nobody may call ground something they are their own height above.
    ///
    /// The rebound is assigned rather than added, so whatever slack the probe
    /// allows is a floor the body hops off *without touching* — permanently, at
    /// no cost in energy. Held under half a body height it reads as the lower
    /// part of an arc, which is what it is for; at more than a whole one it is
    /// a hover, and that is what a dog was doing on the end of its leash.
    #[test]
    fn the_probe_reaches_past_the_soles_in_proportion_to_the_body() {
        for stand in [0.18f32, 0.26, 0.845, 0.905] {
            let slack = probe_reach(stand) - stand - PROBE_LIFT;
            assert!(
                slack < stand * 0.75,
                "a body standing {stand}m counts {slack}m of air as ground"
            );
            assert!(slack > stand * 0.25, "{stand}m has nothing to steer with");
        }
    }

    #[test]
    fn a_jump_clears_more_than_a_kerb_and_less_than_a_storey() {
        let hop = GameConfig::default().bounce.hop_speed * JUMP_SCALE;
        let apex = hop * hop / (2.0 * 9.81);
        assert!(apex > 1.0, "a jump of {apex:.2}m clears nothing");
        assert!(apex < 4.0, "a jump of {apex:.2}m is a helicopter");
    }
}
