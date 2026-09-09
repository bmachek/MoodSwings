//! Umbrellas.
//!
//! The weather has been running since the weather milestone and nobody has ever
//! noticed it. Rain falls, the road goes to a mirror, the crowd quietly thins —
//! and the people still out in it walk about bare-headed as though it were
//! June. One reaction fixes that, and it is the loudest reaction available for
//! the money: a street of black domes is a *different street*, not the same one
//! with a filter over it.
//!
//! ## Why it hangs off the body and not off the hand
//!
//! The obvious place for an umbrella is the hand, and the hand is on the end of
//! an arm that `figure::animate` swings through a full stride. An umbrella
//! bolted there sweeps a metre back and forth over its owner's head at walking
//! pace, which is a thing nobody has ever done in the rain.
//!
//! Doing it properly means a `Posture` that locks the carrying arm up and
//! leaves the other one swinging — and a posture is all four limbs, so it would
//! also have to stop the legs walking. So the umbrella hangs off the *body*
//! with its own [`Rest`], directly over the head, which is where the canopy of
//! a carried umbrella actually stays. The arm swings on underneath it. At any
//! distance a pedestrian is ever seen from, the dome over the head is the whole
//! read, and nobody has once looked at the elbow.
//!
//! Hanging it off the body has a second effect that is not a compromise: a
//! `Rest`-carrying child is squashed with its owner, so a citizen bounced off a
//! bonnet takes their umbrella with them, flattening and springing back on the
//! same frames. That is worth more than a correct elbow.
//!
//! ## Not everybody owns one
//!
//! Three in four. A street where every single person has an umbrella out is as
//! clear a tell as one where nobody has, and the ones hurrying along with
//! nothing are what makes the rest read as prepared rather than as issued.

use bevy::prelude::*;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::figure::{Rest, body};
use super::pedestrian::Pedestrian;
use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::core::schedule::GameSet;
use crate::player::on_foot::Player;
use crate::world::weather::Weather;

/// Rain, nought to one, at which umbrellas go up.
const OPEN: f32 = 0.22;
/// And the level they come down at, which is lower on purpose: one threshold
/// makes a shower on the boundary flicker a whole street of umbrellas open and
/// shut several times a second.
const SHUT: f32 = 0.10;

/// Share of the crowd that owns one.
const CARRIED: f32 = 0.75;

/// Canopy width and height, as fractions of a metre.
///
/// Wide and shallow. The cone this is built from wants to be a pyramid and an
/// umbrella is nearly a saucer: at anything like a cone's own proportions it
/// reads as a witch's hat, and the give-away is that you can see the point.
const SPAN: f32 = 0.95;
const RISE: f32 = 0.26;
/// How high the canopy's *middle* rides above the head's middle.
///
/// Not the rim: a cone hangs its rim half its own height below its middle, and
/// the first pass at this forgot that and put the rim a centimetre inside the
/// wearer's skull. The test below is the one that noticed.
const CLEAR: f32 = 0.34;

/// Where the shaft's middle sits, and how long it is.
const SHAFT_MIDDLE: f32 = body::SHOULDER + 0.26;
const SHAFT_LENGTH: f32 = 0.60;

const _: () = assert!(SHUT < OPEN, "one threshold flickers");

/// On a figure that has been asked whether it has an umbrella.
///
/// A marker rather than a handle to the thing, and it goes on whether the
/// answer was yes or no: it is what stops the roll happening once a frame, and
/// what stops somebody who set off without one acquiring one halfway down the
/// street.
#[derive(Component)]
pub struct Sheltered;

/// On the parts of an umbrella — the canopy and the shaft both.
///
/// An umbrella is two entities and the figure it hangs off does not know that.
/// Marking the parts rather than remembering them on the owner is what makes
/// putting them away a query instead of a list, and the first version stored
/// only the canopy and left every shaft in the city standing in the sunshine.
#[derive(Component)]
pub struct Brolly;

#[derive(Resource)]
struct BrollyKit {
    canopy: Handle<Mesh>,
    shaft: Handle<Mesh>,
    /// Mostly black, because umbrellas mostly are, and then the two that are
    /// not — which are the ones you actually see.
    cloth: Vec<Handle<StandardMaterial>>,
    handle: Handle<StandardMaterial>,
}

#[derive(Resource)]
struct BrollyRng(ChaCha8Rng);

pub struct BrollyPlugin;

impl Plugin for BrollyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, shelter.in_set(GameSet::Ai));
    }
}

fn setup(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Runtime behaviour rather than world generation — it depends on what the
    // sky is doing — so it takes its own stream for the reason `mayhem` does:
    // a shower must not reshuffle anything the seed decided.
    commands.insert_resource(BrollyRng(stream_for(config.world_seed, stream::UMBRELLAS)));

    let fabric = |materials: &mut Assets<StandardMaterial>, color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            // Wet nylon. Rougher than glass and far smoother than cloth, and
            // the sheen is most of what says it is raining on it.
            perceptual_roughness: 0.42,
            // Seen from underneath as often as from above, and a one-sided
            // canopy is a hole in the sky to whoever is standing under it.
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    };
    commands.insert_resource(BrollyKit {
        // Eight panels, which is what an umbrella has.
        canopy: meshes.add(Cone::new(1.0, 1.0).mesh().resolution(8).build()),
        shaft: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(5).build()),
        cloth: [
            Color::srgb(0.055, 0.055, 0.065),
            Color::srgb(0.050, 0.050, 0.058),
            Color::srgb(0.14, 0.20, 0.34),
            Color::srgb(0.42, 0.10, 0.12),
            Color::srgb(0.86, 0.84, 0.30),
        ]
        .into_iter()
        .map(|color| fabric(&mut materials, color))
        .collect(),
        handle: materials.add(StandardMaterial {
            base_color: Color::srgb(0.20, 0.16, 0.13),
            perceptual_roughness: 0.85,
            ..default()
        }),
    });
}

/// Puts them up when it rains and takes them down when it stops.
fn shelter(
    mut commands: Commands,
    weather: Res<Weather>,
    kit: Res<BrollyKit>,
    mut rng: ResMut<BrollyRng>,
    bare: Query<Entity, (Or<(With<Pedestrian>, With<Player>)>, Without<Sheltered>)>,
    covered: Query<Entity, With<Sheltered>>,
    up: Query<Entity, With<Brolly>>,
) {
    if weather.rain < SHUT {
        // Dry. Everything comes down, and everybody is asked again next time.
        for part in &up {
            // `try_despawn`: the part hangs off a citizen, and a citizen
            // despawned by `maintain_population` takes its descendants with
            // it. Reaching for the part afterwards is reaching for an id
            // something else has already reused.
            commands.entity(part).try_despawn();
        }
        for figure in &covered {
            commands.entity(figure).remove::<Sheltered>();
        }
        return;
    }
    if weather.rain < OPEN {
        // Between the two thresholds: whatever is up stays up, and nothing new
        // goes up. This is the whole of the hysteresis.
        return;
    }

    for figure in &bare {
        commands.entity(figure).insert(Sheltered);
        if rng.0.random_range(0.0..1.0) >= CARRIED {
            continue;
        }

        let cloth = kit.cloth[rng.0.random_range(0..kit.cloth.len())].clone();
        // A little off vertical, and a different little for each person.
        // An umbrella held plumb reads as a lamp; every real one is
        // tipped into the weather or away from a neighbour.
        let tilt = Quat::from_rotation_x(rng.0.random_range(-0.16..0.16))
            * Quat::from_rotation_z(rng.0.random_range(-0.16..0.16));
        let over = Vec3::new(0.0, body::HEAD_CENTRE + CLEAR, -0.04);

        let canopy = commands
            .spawn((
                // `Rest` is what puts it in `figure::animate`'s hands: it
                // is placed from the rest pose every frame and squashed
                // with its owner, so a citizen sent over a bonnet takes it
                // along and flattens with it.
                Brolly,
                Rest::posed(over, Vec3::new(SPAN * 0.5, SPAN * 0.42, SPAN * 0.5)),
                Mesh3d(kit.canopy.clone()),
                MeshMaterial3d(cloth),
                Transform::from_translation(over).with_rotation(tilt),
            ))
            .id();
        // The shaft, from under the canopy down towards the hand. Its own
        // child of the figure rather than a child of the canopy, because
        // the canopy is scaled to a dome and a shaft inheriting that scale
        // comes out as a squashed peg.
        let shaft = Vec3::new(0.0, SHAFT_MIDDLE, -0.04);
        let stick = commands
            .spawn((
                Brolly,
                Rest::posed(shaft, Vec3::new(0.012, SHAFT_LENGTH, 0.012)),
                Mesh3d(kit.shaft.clone()),
                MeshMaterial3d(kit.handle.clone()),
                Transform::from_translation(shaft),
            ))
            .id();
        commands.entity(figure).add_children(&[canopy, stick]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canopy clears the head it is over.
    #[test]
    fn nobody_is_wearing_their_umbrella() {
        // A cone hangs its rim half its own height below its middle, and the
        // first pass at this forgot that: the rim came out a centimetre inside
        // the wearer's skull, which reads as a very odd hat. So the rim, not
        // the middle, is what has to clear the crown.
        let rim = body::HEAD_CENTRE + CLEAR - RISE * 0.5;
        let crown = body::HEAD_CENTRE + body::HEAD_RADIUS;
        assert!(
            rim > crown,
            "the rim is at {rim:.3} and the head reaches {crown:.3}"
        );
        // And it is not held at arm's length above them either.
        assert!(rim < crown + 0.35, "the umbrella is a parasol on a pole");
    }

    /// The shaft reaches from the canopy down to about where a hand is.
    #[test]
    fn the_stick_joins_the_canopy_to_the_person() {
        let (top, bottom) = (
            SHAFT_MIDDLE + SHAFT_LENGTH * 0.5,
            SHAFT_MIDDLE - SHAFT_LENGTH * 0.5,
        );
        assert!(
            top > body::HEAD_CENTRE + CLEAR - RISE * 0.5,
            "the shaft stops short of its own canopy"
        );
        assert!(
            bottom < body::SHOULDER,
            "the shaft never comes down as far as a shoulder"
        );
    }

    /// A shower sitting on the threshold must not strobe.
    #[test]
    fn umbrellas_do_not_flicker_at_the_threshold() {
        // Sweeping the rain up and down across the gap has to produce exactly
        // one raising, not one per crossing. This is the case that a single
        // threshold gets wrong, and it gets it wrong loudly: a whole street of
        // umbrellas opening and shutting several times a second.
        let mut up = false;
        let mut changes = 0;
        for step in 0..400 {
            let rain = OPEN + (step as f32 * 0.7).sin() * 0.03;
            let wanted = if rain >= OPEN {
                true
            } else if rain < SHUT {
                false
            } else {
                up
            };
            if wanted != up {
                changes += 1;
                up = wanted;
            }
        }
        assert_eq!(changes, 1, "the street strobed {changes} times");
    }

    /// Most of the street has one, and a visible minority does not.
    #[test]
    fn some_of_the_street_gets_wet() {
        assert!((0.5..0.9).contains(&CARRIED));
    }
}
