//! Rubbish, and what happens to it when somebody walks through it.
//!
//! A newspaper page skating along the gutter is one of the two or three things
//! that most reliably says *city* — and in a game whose whole verb is barging
//! about, litter is the cheapest interactive object there is. You cannot pick
//! it up, break it or do anything with it at all except send it everywhere,
//! which is exactly the amount of interaction it deserves.
//!
//! ## Hand-animated, like the geyser
//!
//! None of this is given to the solver. `world::mayhem` already makes the
//! argument for its droplets — a hundred a second, none of which needs to push
//! anything — and it applies twice over here: there are a thousand pieces of
//! litter resident at any time, and a can that could be leaned on is a can that
//! costs a contact manifold for the rest of the game.
//!
//! Hand animation also buys the thing a solver would have made hard. Paper does
//! not fall; paper *flutters*, at a quarter of gravity, wandering sideways on
//! the way down. A rigid body with a paper-shaped collider falls like a
//! flagstone, and no amount of drag tuning fixes that because the effect comes
//! from a sheet's whole surface, not from its mass.
//!
//! ## It gathers
//!
//! Litter is placed in drifts rather than scattered evenly: one street edge in
//! three gets a huddle of four or five pieces in one spot, and the rest get
//! nothing. That is both how rubbish actually behaves — it collects in corners
//! and doorways and against kerbs — and the difference between a thousand
//! pieces that read as a dirty city and four thousand that read as confetti.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::roadgraph::RoadEdge;
use crate::core::schedule::GameSet;

/// How far litter is drawn. Nearer than almost anything else in the world: a
/// crisp packet is ten centimetres across and there is no distance at which a
/// missing one is noticed.
pub const RANGE: f32 = 55.0;

/// Chance that one street edge has a drift on it at all.
const DRIFT_CHANCE: f32 = 0.34;
/// And how many pieces are in one when it does.
const DRIFT_SIZE: (usize, usize) = (3, 7);
/// How far a drift spreads from its middle, in metres.
const DRIFT_SPREAD: f32 = 1.5;

/// How close something has to pass to stir a piece, in metres.
const KICK: f32 = 0.85;
/// And a car, which is both wider than a person and travelling.
const KICK_VEHICLE: f32 = 2.6;

/// Metres per second a stirred piece leaves with, before its kind's own
/// lightness is applied.
const STIR: f32 = 2.4;
/// And the multiplier a car gets over a person. A car does not brush past a
/// drift, it displaces the air the drift is sitting in.
const WAKE: f32 = 2.6;

// A car disturbs a much wider swathe than a person — it does not brush past a
// drift, it displaces the air the drift is sitting in — and a person has to be
// able to walk *past* one without emptying it, or no street in the city stays
// dirty for longer than one lap. And a drift two metres across is not a drift,
// it is a scatter. Compile-time facts, so they are asserted as such.
const _: () = assert!(KICK_VEHICLE > KICK * 2.0);
const _: () = assert!(KICK < 1.2);
const _: () = assert!(DRIFT_SPREAD < 2.0);

/// What a piece of litter is.
///
/// The kinds differ in exactly two things that matter — how heavy they fall and
/// how much they wander doing it — and that is the whole model. A can is
/// basically a pebble; a sheet of newspaper is basically a leaf.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scrap {
    /// A page of newsprint. The one everybody pictures.
    Paper,
    /// A takeaway cup, which rolls.
    Cup,
    /// A drinks can, which rolls further and catches the light.
    Can,
    /// A carrier bag, crumpled. The heaviest thing here and still nothing.
    Bag,
}

impl Scrap {
    const ALL: [Scrap; 4] = [Scrap::Paper, Scrap::Cup, Scrap::Can, Scrap::Bag];

    /// Weighted the way a gutter is: mostly paper, some packaging, the
    /// occasional bag.
    fn pick(rng: &mut ChaCha8Rng) -> Scrap {
        match rng.random_range(0..10) {
            0..=4 => Scrap::Paper,
            5..=6 => Scrap::Cup,
            7..=8 => Scrap::Can,
            _ => Scrap::Bag,
        }
    }

    /// Metres per second squared. Not gravity: the *effective* fall, with the
    /// air already in it, because nothing here is heavy enough for the two to
    /// be worth separating.
    fn fall(self) -> f32 {
        match self {
            // A sheet of paper falls at about a quarter of a stone.
            Scrap::Paper => 2.4,
            Scrap::Bag => 4.5,
            Scrap::Cup => 7.0,
            Scrap::Can => 8.5,
        }
    }

    /// How far it wanders sideways on the way down, in metres per second.
    /// This is the whole difference between fluttering and dropping.
    fn wander(self) -> f32 {
        match self {
            Scrap::Paper => 1.5,
            Scrap::Bag => 0.7,
            _ => 0.0,
        }
    }

    /// How a piece lies when nobody has touched it, and how high its middle
    /// sits above the ground while it does.
    ///
    /// The two are one decision, and they have to be made per kind. A cup is a
    /// cylinder standing on its end and has to be tipped over; a sheet of paper
    /// is already a flat plate, and tipping *that* stands it on edge like a
    /// headstone — which is exactly what one rotation for everything produced
    /// the first time, a gutter full of little grey gravestones.
    fn lying(self, size: Vec3) -> (Quat, f32) {
        match self {
            // Cylinders, onto the flank: what was the height now runs along
            // the ground, and the radius is what holds it up.
            Scrap::Cup | Scrap::Can => (Quat::from_rotation_x(std::f32::consts::FRAC_PI_2), size.x),
            // Flat already, and a crumpled bag has no up.
            Scrap::Paper | Scrap::Bag => (Quat::IDENTITY, size.y),
        }
    }

    /// How hard a given stir throws it.
    fn lightness(self) -> f32 {
        match self {
            Scrap::Paper => 1.6,
            Scrap::Bag => 1.1,
            Scrap::Cup => 0.9,
            Scrap::Can => 0.7,
        }
    }
}

/// One piece, and everything about where it is going.
#[derive(Component)]
pub struct Litter {
    kind: Scrap,
    /// The height it lies at when it is lying still.
    ground: f32,
    velocity: Vec3,
    /// Radians per second about each axis while it is in the air.
    tumble: Vec3,
    /// Seconds it has been up. Zero means it is lying where it landed.
    aloft: f32,
    /// Its own offset into the wander, so a drift does not flutter in unison.
    phase: f32,
}

#[derive(Resource)]
pub struct LitterKit {
    sheet: Handle<Mesh>,
    tube: Handle<Mesh>,
    lump: Handle<Mesh>,
    newsprint: Handle<StandardMaterial>,
    card: Handle<StandardMaterial>,
    tin: Handle<StandardMaterial>,
    plastic: Handle<StandardMaterial>,
}

impl LitterKit {
    fn parts(&self, kind: Scrap) -> (&Handle<Mesh>, &Handle<StandardMaterial>, Vec3) {
        match kind {
            // A page of newspaper, near enough A3 folded once.
            Scrap::Paper => (&self.sheet, &self.newsprint, Vec3::new(0.21, 0.002, 0.29)),
            Scrap::Cup => (&self.tube, &self.card, Vec3::new(0.040, 0.105, 0.040)),
            Scrap::Can => (&self.tube, &self.tin, Vec3::new(0.033, 0.122, 0.033)),
            Scrap::Bag => (&self.lump, &self.plastic, Vec3::new(0.105, 0.07, 0.09)),
        }
    }
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> LitterKit {
    LitterKit {
        sheet: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        // Six sides is a can at ten centimetres. Nobody has ever counted them.
        tube: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(6).build()),
        lump: meshes.add(
            Sphere::new(1.0)
                .mesh()
                .ico(1)
                .expect("an icosphere at one subdivision"),
        ),
        newsprint: materials.add(StandardMaterial {
            base_color: Color::srgb(0.80, 0.78, 0.72),
            perceptual_roughness: 0.95,
            ..default()
        }),
        card: materials.add(StandardMaterial {
            base_color: Color::srgb(0.72, 0.66, 0.56),
            perceptual_roughness: 0.90,
            ..default()
        }),
        tin: materials.add(StandardMaterial {
            base_color: Color::srgb(0.68, 0.70, 0.73),
            perceptual_roughness: 0.30,
            metallic: 0.85,
            ..default()
        }),
        plastic: materials.add(StandardMaterial {
            base_color: Color::srgb(0.66, 0.68, 0.70),
            perceptual_roughness: 0.55,
            ..default()
        }),
    }
}

/// Scatters a drift of rubbish somewhere along one street, if this street has
/// one.
#[allow(clippy::too_many_arguments)]
pub fn spawn_edge(
    commands: &mut Commands,
    kit: &LitterKit,
    rng: &mut ChaCha8Rng,
    edge: &RoadEdge,
    from: Vec2,
    to: Vec2,
    chunk: IVec2,
    range: f32,
) {
    if rng.random_range(0.0..1.0) > DRIFT_CHANCE {
        return;
    }
    let Ok(direction) = Dir2::new(to - from) else {
        return;
    };
    let normal = Vec2::new(-direction.y, direction.x);
    let side = if rng.random_range(0.0..1.0) < 0.5 {
        1.0
    } else {
        -1.0
    };

    // Rubbish gathers where the wind stops pushing it: in the gutter against
    // the kerb, or back against the buildings. The middle of a pavement is the
    // one place it is never found, which is exactly where an even scatter
    // would put most of it.
    let against_the_wall = rng.random_range(0.0..1.0) < 0.45;
    let (offset, ground) = if against_the_wall {
        (
            edge.width * 0.5 + rng.random_range(2.4..3.0),
            SIDEWALK_HEIGHT,
        )
    } else {
        (edge.width * 0.5 - rng.random_range(0.15..0.45), 0.0)
    };
    let along = rng.random_range(0.1..0.9) * edge.length;
    let middle = from + *direction * along + normal * (offset * side);

    let visibility = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: range..(range * 1.15),
        use_aabb: false,
    };

    for _ in 0..rng.random_range(DRIFT_SIZE.0..DRIFT_SIZE.1) {
        let kind = Scrap::pick(rng);
        let (mesh, material, size) = kit.parts(kind);
        let angle = rng.random_range(0.0..std::f32::consts::TAU);
        let spread = rng.random_range(0.0..DRIFT_SPREAD);
        let at = middle + Vec2::new(angle.cos(), angle.sin()) * spread;

        // Lying down, not standing up. A can on its end is a can somebody put
        // there; the whole read of litter is that nobody put it anywhere.
        let (lie, clearance) = kind.lying(size);
        let lying = Quat::from_rotation_y(rng.random_range(0.0..std::f32::consts::TAU)) * lie;
        let rest = ground + clearance + 0.004;

        commands.spawn((
            ChunkOf(chunk),
            Litter {
                kind,
                ground: rest,
                velocity: Vec3::ZERO,
                tumble: Vec3::ZERO,
                aloft: 0.0,
                phase: rng.random_range(0.0..std::f32::consts::TAU),
            },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(at.x, rest, at.y)
                .with_rotation(lying)
                .with_scale(size),
            visibility.clone(),
        ));
    }
}

pub struct LitterPlugin;

impl Plugin for LitterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (stir, drift).chain().in_set(GameSet::Simulation));
    }
}

/// Anything that goes past close enough sends it up.
///
/// The filter for people is `Bouncer`, which is what everything in this city
/// that moves under its own steam carries — the player, the crowd, the dogs and
/// the cats — so a cat crossing a drift scatters it and a lamp post does not.
fn stir(
    time: Res<Time>,
    mut litter: Query<(&Transform, &mut Litter)>,
    walkers: Query<&Transform, (Without<Litter>, With<crate::bounce::controller::Bouncer>)>,
    vehicles: Query<
        &Transform,
        (
            With<crate::vehicle::spawn::Vehicle>,
            Without<Litter>,
            Without<crate::bounce::controller::Bouncer>,
        ),
    >,
) {
    // Gathered once, as the pigeons do: a thousand pieces against a street's
    // worth of people is otherwise the most expensive thing in the frame.
    let mut stirrers: Vec<(Vec3, f32, f32)> = Vec::new();
    for transform in &walkers {
        stirrers.push((transform.translation, KICK, 1.0));
    }
    for transform in &vehicles {
        stirrers.push((transform.translation, KICK_VEHICLE, WAKE));
    }
    if stirrers.is_empty() {
        return;
    }

    // A phase that advances with the clock, so two pieces stirred in the same
    // frame do not leave along identical arcs.
    let jitter = time.elapsed_secs();

    for (transform, mut scrap) in &mut litter {
        if scrap.aloft > 0.0 {
            continue;
        }
        let here = transform.translation;
        let Some((at, _, force)) = stirrers
            .iter()
            .copied()
            .find(|(at, reach, _)| at.distance_squared(here) < reach * reach)
        else {
            continue;
        };

        // Away from whatever came past, and up. The upward part is most of it:
        // rubbish kicked along the ground reads as a bug, and rubbish that goes
        // up and comes down reads as rubbish.
        let away = (here - at).with_y(0.0).normalize_or(Vec3::X);
        let lift = scrap.kind.lightness();
        scrap.velocity = away * (STIR * force * lift) + Vec3::Y * (STIR * lift * 0.75);
        scrap.tumble = Vec3::new(
            (jitter * 3.1).sin() * 7.0,
            (jitter * 2.3).cos() * 5.0,
            (jitter * 4.7).sin() * 9.0,
        ) * lift;
        scrap.aloft = f32::EPSILON;
        scrap.phase = jitter;
    }
}

/// Everything that is currently in the air, and the settling of it.
fn drift(time: Res<Time>, mut litter: Query<(&mut Transform, &mut Litter)>) {
    let dt = time.delta_secs();
    for (mut transform, mut scrap) in &mut litter {
        if scrap.aloft <= 0.0 {
            continue;
        }
        scrap.aloft += dt;

        let kind = scrap.kind;
        // The wander: a sideways drift that reverses, which is what a sheet of
        // paper does on the way down and what nothing else in this game does.
        let sway = kind.wander();
        if sway > 0.0 {
            let t = scrap.phase + scrap.aloft * 3.2;
            scrap.velocity.x += t.sin() * sway * dt * 2.0;
            scrap.velocity.z += (t * 0.77).cos() * sway * dt * 2.0;
        }
        scrap.velocity.y -= kind.fall() * dt;

        let step = scrap.velocity * dt;
        transform.translation += step;
        let tumble = scrap.tumble * dt;
        transform.rotation =
            Quat::from_euler(EulerRot::XYZ, tumble.x, tumble.y, tumble.z) * transform.rotation;

        if transform.translation.y <= scrap.ground && scrap.velocity.y < 0.0 {
            // Down. It stays exactly where it landed rather than bouncing:
            // this is a crisp packet, and the game has enough things in it
            // that bounce.
            transform.translation.y = scrap.ground;
            scrap.velocity = Vec3::ZERO;
            scrap.tumble = Vec3::ZERO;
            scrap.aloft = 0.0;
            // Flat again, keeping whatever heading the tumble left it on —
            // and flat in this kind's own sense of the word, which for a sheet
            // of paper is not the same quarter turn a can needs.
            let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
            let (lie, _) = kind.lying(transform.scale);
            transform.rotation = Quat::from_rotation_y(yaw) * lie;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paper falls slower than tin, and wanders where tin does not.
    #[test]
    fn a_sheet_of_paper_flutters_and_a_can_does_not() {
        assert!(Scrap::Paper.fall() < Scrap::Can.fall() * 0.4);
        assert!(Scrap::Paper.wander() > 1.0);
        assert_eq!(Scrap::Can.wander(), 0.0);
        // And the order is the order of the real things, all the way down.
        assert!(Scrap::Paper.fall() < Scrap::Bag.fall());
        assert!(Scrap::Bag.fall() < Scrap::Cup.fall());
        assert!(Scrap::Cup.fall() < Scrap::Can.fall());
    }

    /// Nothing here falls at anything like the rate a stone does.
    #[test]
    fn nothing_in_the_gutter_weighs_anything() {
        for kind in Scrap::ALL {
            assert!(
                kind.fall() < 9.81,
                "{kind:?} falls at {}, which is a brick",
                kind.fall()
            );
            assert!(kind.fall() > 1.0, "{kind:?} would hang in the air");
            assert!((0.5..2.0).contains(&kind.lightness()));
        }
    }

    /// The lightest thing goes furthest for the same kick.
    #[test]
    fn the_same_shove_sends_paper_further_than_a_can() {
        // Rise time is velocity over fall, and range goes with the square of
        // it — so this is the whole ballistic model in one line, and it has to
        // come out the way anybody who has kicked a can expects.
        let range = |kind: Scrap| {
            let up = STIR * kind.lightness() * 0.75;
            let air = 2.0 * up / kind.fall();
            air * STIR * 0.6
        };
        assert!(range(Scrap::Paper) > range(Scrap::Bag));
        assert!(range(Scrap::Bag) > range(Scrap::Cup));
        assert!(range(Scrap::Cup) > range(Scrap::Can));
    }

    /// Nothing stands up.
    ///
    /// This is the one that actually went wrong: a single quarter turn for
    /// every kind laid the cylinders down correctly and stood the flat things
    /// on edge, so a gutter came out full of little grey gravestones. The test
    /// is that whatever pose a kind rests in, the thing is wider than it is
    /// tall — which is what "dropped" looks like and "placed" does not.
    #[test]
    fn every_kind_of_rubbish_lies_flatter_than_it_stands() {
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let kit = build_assets(&mut meshes, &mut materials);

        for kind in Scrap::ALL {
            let (_, _, size) = kit.parts(kind);
            let (lie, clearance) = kind.lying(size);
            // The half-extents, turned by the resting pose.
            let up = (lie * Vec3::new(size.x, size.y, size.z)).abs();
            let widest = up.x.max(up.z);
            assert!(
                up.y <= widest,
                "{kind:?} rests {:.3}m tall and {widest:.3}m wide",
                up.y
            );
            // And it rests *on* the ground rather than in it or over it.
            assert!(
                (clearance - up.y).abs() < 1e-4,
                "{kind:?} floats: {clearance:.3}m up on a {:.3}m half-height",
                up.y
            );
        }
    }

    /// A drift is a huddle, not a scatter.
    #[test]
    fn litter_gathers_rather_than_spreading() {
        assert!(DRIFT_SIZE.0 >= 3, "two pieces is not a drift");
        // One edge in three, so the city reads as dirty in places rather than
        // uniformly sprinkled — and so the resident count stays in the
        // hundreds rather than the thousands.
        assert!((0.2..0.5).contains(&DRIFT_CHANCE));
    }
}
