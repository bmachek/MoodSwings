//! The name on the corner.
//!
//! `tools/bake-city.py` has been carrying Landshut's street names since the
//! first extract — a hundred and sixty-three of them, Maximilianstraße and
//! Ländtorplatz and Nikolaus-Alexander-Mair-Straße — and nothing has ever read
//! them. They are the whole difference between *a* street plan and *this* one:
//! the geometry says a town, and the names say which.
//!
//! Blue with white lettering, which is what Bavaria puts on a corner.
//!
//! ## One mesh, one material per name
//!
//! Every plate is the same size, so there is one mesh for all of them and the
//! only thing that varies is the texture. That is a hundred and sixty-three
//! materials for a whole town rather than one per sign — the same arithmetic
//! that the parked cars' paint shop is built on, arrived at from the other
//! direction: here the *names* are the closed list.
//!
//! A fixed plate means a long name is squeezed into the same width as a short
//! one, and that is not a compromise. It is what a real sign does, and it is
//! why Nikolaus-Alexander-Mair-Straße is written in narrower letters than
//! Altstadt on the corner it actually stands on.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::buildings::{ChunkOf, SIDEWALK_HEIGHT};
use super::citygen::SIDEWALK_WIDTH;
use super::texture::{byte, encode, fbm, painted_rect, text_band};

/// How far a sign is drawn. Short: the plate is a hand's width tall, and a
/// name nobody can read is a draw call with no picture in it.
pub const RANGE: f32 = 75.0;

/// The plate, in metres. A real one is about this.
const PLATE: Vec2 = Vec2::new(1.55, 0.30);
/// And how high its middle rides above the pavement.
const HEIGHT: f32 = 2.15;

/// Texels across a plate. Enough that a thirty-character name still has three
/// texels per stroke of a letter.
const WIDTH: u32 = 512;
const TALL: u32 = 100;

// A plate hangs above head height and below a first-floor window, and it is a
// plate rather than a hoarding.
const _: () = assert!(HEIGHT > 1.9);
const _: () = assert!(HEIGHT < 2.8);
const _: () = assert!(PLATE.x > PLATE.y * 3.0 && PLATE.x < 2.0);

#[derive(Resource)]
pub struct StreetNameKit {
    plate: Handle<Mesh>,
    post: Handle<Mesh>,
    steel: Handle<StandardMaterial>,
    /// One per distinct name, indexed the way [`Signposts`] indexes them.
    ///
    /// (`Signposts` lives in `world::atlas`, which is where the names come
    /// from; this is only the paint.)
    names: Vec<Handle<StandardMaterial>>,
}

/// A name plate: white letters on a blue field, with a white border.
fn plate(name: &str) -> Image {
    let letters = encode(name);
    painted_rect(WIDTH, TALL, TextureFormat::Rgba8UnormSrgb, |u, v| {
        // The border, and the field inside it. Enamel that has been on a wall
        // for thirty years, so the blue is not flat.
        let edge = u
            .min(1.0 - u)
            .min((v.min(1.0 - v)) * (TALL as f32 / WIDTH as f32));
        let border = edge < 0.006;
        let weathered = fbm(u, v, 9, 3, 0x51c7) * 0.10;

        // The lettering sits in the middle two thirds of the plate's height,
        // which leaves the same margin above and below that a real one has.
        let ink = text_band(&letters, u, (v - 0.24) / 0.52);

        if ink || border {
            let white = 0.92 - weathered * 0.4;
            [byte(white), byte(white), byte(white * 1.01), 255]
        } else {
            [
                byte(0.045 + weathered * 0.5),
                byte(0.145 + weathered * 0.6),
                byte(0.360 + weathered * 0.5),
                255,
            ]
        }
    })
}

pub fn build_assets(
    names: &[String],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> StreetNameKit {
    StreetNameKit {
        plate: meshes.add(Rectangle::new(PLATE.x, PLATE.y)),
        post: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(6).build()),
        steel: materials.add(StandardMaterial {
            base_color: Color::srgb(0.52, 0.53, 0.55),
            perceptual_roughness: 0.55,
            metallic: 0.6,
            ..default()
        }),
        names: names
            .iter()
            .map(|name| {
                materials.add(StandardMaterial {
                    base_color_texture: Some(images.add(plate(name))),
                    // Vitreous enamel: smoother than paint and nothing like a
                    // mirror, which is what makes one catch a headlight.
                    perceptual_roughness: 0.36,
                    ..default()
                })
            })
            .collect(),
    }
}

/// Stands one name on the corner it belongs to.
///
/// `at` is the junction, `towards` is the way the street runs away from it.
/// The plate stands on the pavement on the near corner and faces back up its
/// own street, which is where somebody arriving at the junction is looking.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    kit: &StreetNameKit,
    name: usize,
    at: Vec2,
    towards: Vec2,
    width: f32,
    chunk: IVec2,
    range: f32,
) {
    let Some(paint) = kit.names.get(name) else {
        return;
    };
    let Ok(direction) = Dir2::new(towards - at) else {
        return;
    };
    let normal = Vec2::new(-direction.y, direction.x);
    // On the pavement, a little way down the street from the middle of the
    // junction, so the post is not standing in the crossing.
    let foot =
        at + *direction * (width * 0.5 + 2.0) + normal * (width * 0.5 + SIDEWALK_WIDTH * 0.5);
    // Facing back at the junction: `+Z` towards where somebody is coming from.
    let yaw = (-direction.x).atan2(-direction.y);
    let visibility = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: range..(range * 1.1),
        use_aabb: false,
    };

    commands.spawn((
        ChunkOf(chunk),
        Mesh3d(kit.post.clone()),
        MeshMaterial3d(kit.steel.clone()),
        Transform::from_xyz(foot.x, SIDEWALK_HEIGHT + HEIGHT * 0.5, foot.y)
            .with_scale(Vec3::new(0.030, HEIGHT, 0.030)),
        visibility.clone(),
    ));
    // Off to one side of its own post, the way a plate is bolted on.
    //
    // Two quads back to back rather than one double-sided one, because the
    // back face of a quad shows its texture *mirrored*: a single plate marked
    // `double_sided` reads Karlsbader Straße from in front and ƎSSAЯTS from
    // behind. A real sign is a sheet of enamel with the name on both faces,
    // and this is that, at the cost of one more quad on a corner.
    let plate = foot + normal * (PLATE.x * 0.42);
    for turn in [0.0, std::f32::consts::PI] {
        commands.spawn((
            ChunkOf(chunk),
            Mesh3d(kit.plate.clone()),
            MeshMaterial3d(paint.clone()),
            Transform::from_xyz(plate.x, SIDEWALK_HEIGHT + HEIGHT, plate.y)
                .with_rotation(Quat::from_rotation_y(yaw + turn)),
            visibility.clone(),
            bevy::light::NotShadowCaster,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name in the town can be written.
    #[test]
    fn the_font_can_spell_landshut() {
        // The charset of the real extract: German street names, which means
        // umlauts, an eszett, and the hyphens in a name like
        // Nikolaus-Alexander-Mair-Straße.
        for name in [
            "Maximilianstraße",
            "Ländtorplatz",
            "Nikolaus-Alexander-Mair-Straße",
            "Am Alten Viehmarkt",
            "Bischof-Sailer-Platz",
        ] {
            let letters = encode(name);
            assert_eq!(letters.len(), name.chars().count());
            assert!(
                !letters.contains(&b' ') || name.contains(' ') || name.contains('-'),
                "{name} lost a letter to the font"
            );
        }
    }

    /// A plate is mostly blue with writing on it.
    #[test]
    fn a_sign_is_a_sign_and_not_a_blue_rectangle() {
        let image = plate("Altstadt");
        let data = image.data.as_ref().expect("the plate was not painted");
        let pale = data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|texel| texel[2] > 200 && texel[0] > 200)
            .count() as f32
            / (WIDTH * TALL) as f32;
        assert!(pale > 0.02, "there is no lettering on it: {pale:.3}");
        assert!(pale < 0.35, "it is a white plate: {pale:.3}");
    }
}
