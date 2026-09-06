//! Painted building signs.
//!
//! A building kind is worth nothing the player cannot read from the pavement,
//! and the cheapest legible thing in this codebase is a painted texture on a
//! quad — the number plates proved it. So every signed kind gets one board:
//! one texture, one material, one mesh, shared city-wide and hung per
//! building in `buildings::spawn_building`. A supermarket's sign is the same
//! board on every supermarket, which is exactly how chain shopfronts work.
//!
//! The names are invented and the words are chosen to live inside the shared
//! 5×7 font, which has no umlauts — so the hotel is HOTEL BOING rather than
//! Hotel Hüpfer, which is funnier anyway. The one sign with something to
//! explain gets a subline: the police station is closed, wegen anhaltender
//! Freundlichkeit.

use bevy::image::Image;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::citygen::BuildingKind;
use super::texture::{glyph, painted_rect};

/// How far a sign board reads, before the level-of-detail scale.
pub const RANGE: f32 = 320.0;
/// How far the board stands proud of the wall face. Enough to clear the
/// facade's recessed reveals; awnings can still brush it on a shopfront and
/// that is a tolerable collision of good intentions.
pub const PROUD: f32 = 0.30;

/// One kind's board, before it is painted.
struct Plaque {
    title: &'static str,
    subline: Option<&'static str>,
    ink: [u8; 4],
    field: [u8; 4],
    /// Height of a title letter, in metres — the whole board scales from it.
    letter: f32,
}

/// The kinds that hang a sign at all. Flats and offices are anonymous on
/// purpose: a city where every building announces itself is an airport.
fn plaque_for(kind: BuildingKind) -> Option<Plaque> {
    use BuildingKind::*;
    let plaque = match kind {
        Supermarket => Plaque {
            title: "SUPERMARKT",
            subline: None,
            ink: [245, 248, 244, 255],
            field: [30, 104, 48, 255],
            letter: 0.55,
        },
        Restaurant => Plaque {
            title: "RESTAURANT",
            subline: None,
            ink: [240, 226, 200, 255],
            field: [98, 26, 22, 255],
            letter: 0.50,
        },
        Hotel => Plaque {
            title: "HOTEL BOING",
            subline: None,
            ink: [236, 198, 112, 255],
            field: [20, 32, 62, 255],
            letter: 0.55,
        },
        TownHall => Plaque {
            title: "RATHAUS",
            subline: None,
            ink: [42, 38, 32, 255],
            field: [212, 196, 166, 255],
            letter: 0.72,
        },
        FireStation => Plaque {
            title: "FEUERWEHR",
            subline: None,
            ink: [248, 244, 240, 255],
            field: [186, 30, 26, 255],
            letter: 0.62,
        },
        PoliceStation => Plaque {
            title: "POLIZEI",
            subline: Some("WEGEN ANHALTENDER FREUNDLICHKEIT GESCHLOSSEN"),
            ink: [246, 247, 250, 255],
            field: [22, 58, 118, 255],
            letter: 0.62,
        },
        Barracks => Plaque {
            title: "KASERNE",
            subline: None,
            ink: [228, 226, 216, 255],
            field: [72, 80, 52, 255],
            letter: 0.55,
        },
        ParkingGarage => Plaque {
            title: "PARKHAUS",
            subline: None,
            ink: [246, 247, 250, 255],
            field: [28, 62, 136, 255],
            letter: 0.60,
        },
        Apartments | Offices => return None,
    };
    Some(plaque)
}

/// Every kind that gets a board, for building the kit and for tests.
const SIGNED: [BuildingKind; 8] = [
    BuildingKind::Supermarket,
    BuildingKind::Restaurant,
    BuildingKind::Hotel,
    BuildingKind::TownHall,
    BuildingKind::FireStation,
    BuildingKind::PoliceStation,
    BuildingKind::Barracks,
    BuildingKind::ParkingGarage,
];

/// Width of one glyph cell relative to the height of its letters. The glyph
/// itself is 5×7 with breathing room either side, same as the plates.
const CELL_ASPECT: f32 = 0.95;

/// Board size in metres for a plaque, title padding included.
fn board_size(plaque: &Plaque) -> Vec2 {
    let title_cells = (plaque.title.len() + 2) as f32;
    // Subline letters are painted at half size, so two of them fit a cell.
    let sub_cells = plaque
        .subline
        .map(|s| (s.len() + 2) as f32 * 0.5)
        .unwrap_or(0.0);
    let cells = title_cells.max(sub_cells);
    let height = plaque.letter * if plaque.subline.is_some() { 2.1 } else { 1.6 };
    Vec2::new(cells * plaque.letter * CELL_ASPECT, height)
}

/// Whether (u, v) inside a text band lands on ink. `v` runs 0..1 over the
/// glyph height; `u` runs 0..1 over the whole band.
fn on_ink(text: &[u8], u: f32, v: f32) -> bool {
    let cells = (text.len() + 2) as f32;
    let column = u * cells - 1.0;
    let index = column.floor();
    if index < 0.0 || index >= text.len() as f32 {
        return false;
    }
    let inside_x = (column - index - 0.14) / 0.72;
    if !(0.0..1.0).contains(&inside_x) || !(0.0..1.0).contains(&v) {
        return false;
    }
    let rows = glyph(text[index as usize]);
    let bit = (inside_x * 5.0) as usize;
    let row = (v * 7.0) as usize;
    rows[row.min(6)] & (1 << (4 - bit.min(4))) != 0
}

/// Paints one board.
fn board_texture(plaque: &Plaque) -> Image {
    let size = board_size(plaque);
    // Pixels per metre, capped so the police novel stays a sane texture.
    let width = ((size.x * 56.0) as u32).clamp(64, 2048);
    let height = ((size.y * 56.0) as u32).clamp(32, 512);
    let title: &[u8] = plaque.title.as_bytes();
    let subline = plaque.subline.map(str::as_bytes);
    let (ink, field) = (plaque.ink, plaque.field);

    painted_rect(width, height, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        // A pressed rim, same trick as the plates: it is what you see of a
        // sign at any distance where the letters have stopped resolving.
        if u < 0.012 || u > 0.988 || v < 0.05 || v > 0.95 {
            return [
                (field[0] / 2).saturating_sub(8),
                (field[1] / 2).saturating_sub(8),
                (field[2] / 2).saturating_sub(8),
                255,
            ];
        }
        let lit = match subline {
            None => on_ink(title, u, (v - 0.18) / 0.64),
            Some(sub) => on_ink(title, u, (v - 0.10) / 0.48) || on_ink(sub, u, (v - 0.66) / 0.24),
        };
        if lit { ink } else { field }
    })
}

#[derive(Resource)]
pub struct SignKit {
    boards: Vec<(BuildingKind, Handle<Mesh>, Handle<StandardMaterial>, Vec2)>,
}

impl SignKit {
    pub fn get(
        &self,
        kind: BuildingKind,
    ) -> Option<(&Handle<Mesh>, &Handle<StandardMaterial>, Vec2)> {
        self.boards
            .iter()
            .find(|(k, ..)| *k == kind)
            .map(|(_, mesh, material, size)| (mesh, material, *size))
    }
}

pub fn build_assets(
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    meshes: &mut Assets<Mesh>,
) -> SignKit {
    let boards = SIGNED
        .iter()
        .map(|&kind| {
            let plaque = plaque_for(kind).expect("every SIGNED kind has a plaque");
            let size = board_size(&plaque);
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(images.add(board_texture(&plaque))),
                perceptual_roughness: 0.72,
                ..default()
            });
            (
                kind,
                meshes.add(Rectangle::new(size.x, size.y)),
                material,
                size,
            )
        })
        .collect();
    SignKit { boards }
}

/// The bundle for one hung board, minus its transform.
pub fn board(
    kit: &SignKit,
    kind: BuildingKind,
) -> Option<(
    Mesh3d,
    MeshMaterial3d<StandardMaterial>,
    NotShadowCaster,
    Vec2,
)> {
    kit.get(kind).map(|(mesh, material, size)| {
        (
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            NotShadowCaster,
            size,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sign_fits_the_font() {
        for kind in SIGNED {
            let plaque = plaque_for(kind).unwrap();
            for text in std::iter::once(plaque.title).chain(plaque.subline) {
                for character in text.bytes() {
                    assert!(
                        character == b' ' || glyph(character) != [0; 7],
                        "the sign for {kind:?} needs a '{}' and the font has none",
                        character as char
                    );
                }
            }
        }
    }

    #[test]
    fn flats_and_offices_stay_anonymous() {
        assert!(plaque_for(BuildingKind::Apartments).is_none());
        assert!(plaque_for(BuildingKind::Offices).is_none());
    }

    #[test]
    fn a_board_is_mostly_field_with_ink_on_it() {
        // The failure this catches is a band-arithmetic slip that paints the
        // whole board in ink, or none of it — both build, both look like a
        // coloured rectangle from the street.
        for kind in SIGNED {
            let plaque = plaque_for(kind).unwrap();
            let image = board_texture(&plaque);
            let data = image.data.as_ref().expect("the board was not painted");
            let ink = data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[..3] == plaque.ink[..3])
                .count() as f32
                / (data.len() / 4) as f32;
            assert!(
                (0.02..0.45).contains(&ink),
                "{kind:?}: {ink:.3} of the board is ink"
            );
        }
    }

    #[test]
    fn the_police_station_confesses_why_it_is_shut() {
        let plaque = plaque_for(BuildingKind::PoliceStation).unwrap();
        assert!(plaque.subline.unwrap().contains("FREUNDLICHKEIT"));
    }
}
