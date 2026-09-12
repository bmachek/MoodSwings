//! Painted building signs.
//!
//! A building kind is worth nothing the player cannot read from the pavement,
//! and the cheapest legible thing in this codebase is a painted texture on a
//! quad — the number plates proved it. So every signed kind gets one board:
//! one texture, one material, one mesh, shared city-wide and hung per
//! building in `buildings::spawn_building`. A supermarket's sign is the same
//! board on every supermarket, which is exactly how chain shopfronts work.
//!
//! The names are invented. The font grew umlauts and punctuation when the
//! city wanted jokes — HOTEL BOING predates both and stays, because it was
//! funnier than Hotel Hüpfer anyway. A kind may carry several plaques now:
//! the building's own seed picks one, so the city reads as three or four
//! competing chains instead of one monopolist, at the price of a handful of
//! extra shared boards.
//!
//! The same kit paints the advertising posters that hang on the blind side
//! walls of the anonymous buildings. Flats and offices refuse to say what
//! they are, but they will absolutely tell you what to buy — that is the
//! joke, and it is also how real gable walls work.

use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::citygen::BuildingKind;
use super::texture::{encode, painted_rect, text_band};

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

/// The kinds that hang a sign at all, each with its plaque variants. Flats
/// and offices are anonymous on purpose: a city where every building
/// announces itself is an airport.
///
/// The chains get competitors; the civic kinds stay singular — there is one
/// Rathaus and it has exactly one sense of humour. The sublines are where
/// the jokes live, and the rule for writing one is the police station's:
/// deadpan, plausible, and told entirely in the register of the institution
/// telling it.
fn plaques_for(kind: BuildingKind) -> &'static [Plaque] {
    use BuildingKind::*;
    match kind {
        Supermarket => &[
            Plaque {
                title: "SUPERMARKT",
                subline: Some("HEUTE IM ANGEBOT: GEDULD"),
                ink: [245, 248, 244, 255],
                field: [30, 104, 48, 255],
                letter: 0.55,
            },
            Plaque {
                title: "HÜPF & GUT",
                subline: Some("PREISE IM FREIEN FALL"),
                ink: [252, 246, 232, 255],
                field: [206, 112, 20, 255],
                letter: 0.55,
            },
            Plaque {
                title: "FRISCHE-ECK",
                subline: Some("ÄPFEL MIT HALTUNG"),
                ink: [240, 248, 246, 255],
                field: [14, 92, 84, 255],
                letter: 0.55,
            },
            Plaque {
                title: "LADEN OHNE NAMEN",
                subline: Some("DER NAME WAR ZU TEUER"),
                ink: [36, 38, 40, 255],
                field: [214, 212, 204, 255],
                letter: 0.52,
            },
            Plaque {
                title: "TANTE EMMA & SÖHNE",
                subline: Some("EMMA IST IM URLAUB"),
                ink: [250, 244, 228, 255],
                field: [122, 44, 60, 255],
                letter: 0.50,
            },
            Plaque {
                title: "KAUFHALLE RUND",
                subline: Some("ALLES DA. FAST ALLES."),
                ink: [28, 34, 46, 255],
                field: [232, 196, 60, 255],
                letter: 0.54,
            },
        ],
        Restaurant => &[
            Plaque {
                title: "RESTAURANT",
                subline: Some("WARME KÜCHE, KÜHLE BLICKE"),
                ink: [240, 226, 200, 255],
                field: [98, 26, 22, 255],
                letter: 0.50,
            },
            Plaque {
                title: "PIZZERIA LUIGI LUIGI",
                subline: Some("DOPPELT HÄLT BESSER"),
                ink: [242, 236, 224, 255],
                field: [24, 84, 40, 255],
                letter: 0.50,
            },
            Plaque {
                title: "GASTHAUS ZUR BEULE",
                subline: Some("GUT ABGEHANGEN"),
                ink: [232, 214, 182, 255],
                field: [74, 50, 30, 255],
                letter: 0.50,
            },
            // Index three, and the Fernost-Viertel forces it — see the
            // sign-variant override in `buildings::spawn_building`. Anything
            // new goes on the end: a plaque inserted above this line moves
            // the Wok, and every dining room in the quarter changes cuisine.
            Plaque {
                title: "WOK & WEG",
                subline: Some("GLÜCKSKEKS SAGT: KOMMEN SIE WIEDER"),
                ink: [250, 226, 160, 255],
                field: [122, 24, 20, 255],
                letter: 0.50,
            },
            Plaque {
                title: "WIRTSHAUS ZUM ABPRALL",
                subline: Some("KÜCHE BIS ES REICHT"),
                ink: [238, 224, 198, 255],
                field: [58, 72, 44, 255],
                letter: 0.48,
            },
            Plaque {
                title: "IMBISS HALT",
                subline: Some("ESSEN IM STEHEN, FALLEN IM SITZEN"),
                ink: [32, 30, 28, 255],
                field: [226, 176, 46, 255],
                letter: 0.52,
            },
            Plaque {
                title: "KAFFEEHAUS ZUR UNRUHE",
                subline: Some("EINMAL UMRÜHREN, BITTE"),
                ink: [244, 236, 222, 255],
                field: [84, 52, 34, 255],
                letter: 0.48,
            },
        ],
        Hotel => &[
            Plaque {
                title: "HOTEL BOING",
                subline: Some("JEDES BETT EIN TRAMPOLIN"),
                ink: [236, 198, 112, 255],
                field: [20, 32, 62, 255],
                letter: 0.55,
            },
            Plaque {
                title: "PENSION SORGENFREI",
                subline: Some("SORGEN BITTE AN DER REZEPTION ABGEBEN"),
                ink: [236, 226, 240, 255],
                field: [86, 60, 92, 255],
                letter: 0.55,
            },
            Plaque {
                title: "HOTEL WEICHE LANDUNG",
                subline: Some("FRÜHSTÜCK AB SIEBEN, AUFSTEHEN NIE"),
                ink: [246, 240, 226, 255],
                field: [26, 78, 74, 255],
                letter: 0.50,
            },
            Plaque {
                title: "ZIMMER FREI",
                subline: Some("MEISTENS"),
                ink: [32, 30, 34, 255],
                field: [224, 206, 150, 255],
                letter: 0.58,
            },
        ],
        TownHall => &[Plaque {
            title: "RATHAUS",
            subline: Some("BITTE ZIEHEN SIE EINE NUMMER: 7183"),
            ink: [42, 38, 32, 255],
            field: [212, 196, 166, 255],
            letter: 0.72,
        }],
        FireStation => &[Plaque {
            title: "FEUERWEHR",
            subline: Some("WIR BRENNEN FÜR SIE"),
            ink: [248, 244, 240, 255],
            field: [186, 30, 26, 255],
            letter: 0.62,
        }],
        PoliceStation => &[Plaque {
            title: "POLIZEI",
            subline: Some("WEGEN ANHALTENDER FREUNDLICHKEIT GESCHLOSSEN"),
            ink: [246, 247, 250, 255],
            field: [22, 58, 118, 255],
            letter: 0.62,
        }],
        Barracks => &[Plaque {
            title: "KASERNE",
            subline: Some("TAG DER OFFENEN TÜR: NIE"),
            ink: [228, 226, 216, 255],
            field: [72, 80, 52, 255],
            letter: 0.55,
        }],
        ParkingGarage => &[Plaque {
            title: "PARKHAUS",
            subline: Some("RESTPLÄTZE: JA"),
            ink: [246, 247, 250, 255],
            field: [28, 62, 136, 255],
            letter: 0.60,
        }],
        Museum => &[Plaque {
            title: "MUSEUM",
            subline: Some("EINTRITT FREI, AUSGANG UNGEWISS"),
            ink: [40, 36, 30, 255],
            field: [200, 186, 158, 255],
            letter: 0.68,
        }],
        School => &[Plaque {
            title: "SCHULE",
            subline: Some("BITTE LEISE HÜPFEN"),
            ink: [250, 248, 242, 255],
            field: [164, 88, 22, 255],
            letter: 0.58,
        }],
        Church => &[Plaque {
            title: "SANKT BOING",
            subline: Some("TURMSPRINGEN VERBOTEN"),
            ink: [236, 226, 206, 255],
            field: [58, 48, 40, 255],
            letter: 0.50,
        }],
        Stadium => &[Plaque {
            title: "STADION AN DER BEULE",
            subline: Some("HEIMSPIEL: JEDEN TAG"),
            ink: [244, 214, 74, 255],
            field: [24, 40, 78, 255],
            letter: 0.62,
        }],
        Cathedral => &[Plaque {
            title: "KATHEDRALE ST. MARTIN",
            subline: Some("HÖCHSTER BACKSTEINTURM DER WELT - NICHT ANLEHNEN"),
            ink: [240, 230, 210, 255],
            field: [96, 40, 30, 255],
            letter: 0.48,
        }],
        // A tower and a gate say nothing, and that is right: they were here
        // six hundred years before anybody thought a building should have a
        // name on it. Their plate is the street-name plate on the corner.
        Apartments | Offices | Tower | Gate => &[],
    }
}

/// Every kind that gets a board, for building the kit and for tests.
const SIGNED: [BuildingKind; 13] = [
    BuildingKind::Supermarket,
    BuildingKind::Restaurant,
    BuildingKind::Hotel,
    BuildingKind::TownHall,
    BuildingKind::FireStation,
    BuildingKind::PoliceStation,
    BuildingKind::Barracks,
    BuildingKind::ParkingGarage,
    BuildingKind::Museum,
    BuildingKind::School,
    BuildingKind::Church,
    BuildingKind::Stadium,
    BuildingKind::Cathedral,
];

/// Width of one glyph cell relative to the height of its letters. The glyph
/// itself is 5×7 with breathing room either side, same as the plates.
const CELL_ASPECT: f32 = 0.95;

/// Board size in metres for a plaque, title padding included.
///
/// Counted in characters, not bytes — an umlaut is two bytes and one cell,
/// and a board sized by bytes would carry a blank cell for every one.
fn board_size(plaque: &Plaque) -> Vec2 {
    let title_cells = (plaque.title.chars().count() + 2) as f32;
    // Subline letters are painted at half size, so two of them fit a cell.
    let sub_cells = plaque
        .subline
        .map(|s| (s.chars().count() + 2) as f32 * 0.5)
        .unwrap_or(0.0);
    let cells = title_cells.max(sub_cells);
    let height = plaque.letter * if plaque.subline.is_some() { 2.1 } else { 1.6 };
    Vec2::new(cells * plaque.letter * CELL_ASPECT, height)
}

/// Paints one board.
fn board_texture(plaque: &Plaque) -> Image {
    let size = board_size(plaque);
    // Pixels per metre, capped so the police novel stays a sane texture.
    let width = ((size.x * 56.0) as u32).clamp(64, 2048);
    let height = ((size.y * 56.0) as u32).clamp(32, 512);
    let title = encode(plaque.title);
    let subline = plaque.subline.map(encode);
    let (ink, field) = (plaque.ink, plaque.field);

    painted_rect(width, height, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        // A pressed rim, same trick as the plates: it is what you see of a
        // sign at any distance where the letters have stopped resolving.
        if !(0.012..=0.988).contains(&u) || !(0.05..=0.95).contains(&v) {
            return [
                (field[0] / 2).saturating_sub(8),
                (field[1] / 2).saturating_sub(8),
                (field[2] / 2).saturating_sub(8),
                255,
            ];
        }
        let lit = match &subline {
            None => text_band(&title, u, (v - 0.18) / 0.64),
            Some(sub) => {
                text_band(&title, u, (v - 0.10) / 0.48) || text_band(sub, u, (v - 0.66) / 0.24)
            }
        };
        if lit { ink } else { field }
    })
}

// ------------------------------------------------------------- frontages ----

/// The painted ground storey a civic kind wears instead of shop glass.
///
/// The enterable kinds show who they are through an open door and a lit
/// room; the civic ones are sealed, so their identity has to be painted on.
/// One texture per kind, stretched across the front face's ground storey —
/// a fire station's roller doors stretch a little wider on a wider
/// building, which is also how real fire stations work.
///
/// The paint reads `v = 0` at the top of the strip, `v = 1` at the
/// pavement, matching the quad's UV layout — every gate below is painted to
/// *reach* v = 1, so a flipped convention would show doors hanging from the
/// ceiling and be caught by the first capture.
fn frontage_texture(kind: BuildingKind) -> Option<Image> {
    use BuildingKind::*;
    let paint: fn(f32, f32) -> [u8; 4] = match kind {
        FireStation => |u, v| {
            // Three roller doors in the red, white trim, slats every so far.
            let field = [150, 28, 24, 255];
            for centre in [0.2f32, 0.5, 0.8] {
                let inside = (u - centre).abs();
                if inside < 0.105 && v > 0.18 {
                    if inside > 0.095 || v < 0.20 {
                        return [235, 230, 224, 255];
                    }
                    let slat = (v * 11.0).fract() < 0.16;
                    return if slat {
                        [126, 22, 18, 255]
                    } else {
                        [196, 48, 38, 255]
                    };
                }
            }
            field
        },
        TownHall => |u, v| {
            // Sandstone with pilasters, and one civic double door.
            if (u - 0.5).abs() < 0.055 && v > 0.28 {
                return if (u - 0.5).abs() > 0.048 {
                    [120, 100, 76, 255]
                } else {
                    [74, 52, 38, 255]
                };
            }
            let pilaster = (u * 7.0).fract() < 0.12;
            if pilaster {
                [226, 212, 184, 255]
            } else if v < 0.10 {
                [188, 172, 144, 255]
            } else {
                [208, 192, 162, 255]
            }
        },
        PoliceStation => |u, v| {
            // Institutional grey-blue, barred windows, and a door taped
            // shut — the tape is the subline made architecture.
            let door = (u - 0.5).abs() < 0.06 && v > 0.25;
            if door {
                let tape = ((u - 0.44) * 6.0 - (v - 0.25)).rem_euclid(0.5) < 0.11;
                return if tape {
                    [214, 40, 34, 255]
                } else {
                    [26, 44, 88, 255]
                };
            }
            for centre in [0.16f32, 0.32, 0.68, 0.84] {
                if (u - centre).abs() < 0.05 && (0.2..0.62).contains(&v) {
                    let bar = ((u - centre) * 40.0).rem_euclid(1.0) < 0.3;
                    return if bar {
                        [200, 206, 214, 255]
                    } else {
                        [42, 50, 62, 255]
                    };
                }
            }
            [178, 188, 200, 255]
        },
        Barracks => |u, v| {
            // Olive drab around one steel gate in hazard chevrons.
            if (u - 0.5).abs() < 0.16 && v > 0.2 {
                if (u - 0.5).abs() > 0.15 {
                    return [60, 64, 46, 255];
                }
                let chevron = ((u * 14.0) + v * 2.0).rem_euclid(1.0) < 0.5;
                return if chevron && v > 0.72 {
                    [196, 168, 48, 255]
                } else {
                    [116, 118, 112, 255]
                };
            }
            if (v * 6.0).fract() < 0.06 {
                [76, 84, 56, 255]
            } else {
                [92, 100, 66, 255]
            }
        },
        Museum => |u, v| {
            // Pale stone, a colonnade, and one dark doorway up a band of
            // steps. The columns are what sells "museum" at any distance.
            if v > 0.9 {
                // The steps, by way of three grey bands.
                let tread = (v * 30.0).fract() < 0.5;
                return if tread {
                    [172, 162, 142, 255]
                } else {
                    [198, 188, 166, 255]
                };
            }
            if (u - 0.5).abs() < 0.045 && v > 0.30 {
                return [52, 42, 34, 255];
            }
            let column = ((u * 9.0).fract() - 0.5).abs() < 0.16;
            if column && v > 0.08 {
                let flute = ((u * 9.0).fract() - 0.5).abs() > 0.12;
                return if flute {
                    [186, 176, 154, 255]
                } else {
                    [214, 204, 182, 255]
                };
            }
            if v < 0.08 {
                return [168, 158, 138, 255];
            }
            [204, 192, 168, 255]
        },
        School => |u, v| {
            // Warm plaster, a row of tall windows, a double door — and a
            // band of coloured tiles at child height, because somebody let
            // the pupils decorate and never regretted it.
            if (u - 0.5).abs() < 0.06 && v > 0.30 {
                return if (u - 0.5).abs() > 0.052 {
                    [122, 74, 30, 255]
                } else {
                    [88, 52, 24, 255]
                };
            }
            for centre in [0.14f32, 0.30, 0.70, 0.86] {
                if (u - centre).abs() < 0.055 && (0.15..0.60).contains(&v) {
                    let frame = (u - centre).abs() > 0.048 || !(0.17..=0.58).contains(&v);
                    return if frame {
                        [236, 230, 218, 255]
                    } else {
                        [64, 78, 92, 255]
                    };
                }
            }
            if (0.72..0.88).contains(&v) {
                // The mosaic: one bright tile per hand-width, no two
                // neighbours alike because the hash stripes them.
                let tile = (u * 40.0) as u32;
                let palette = [
                    [206, 82, 60, 255u8],
                    [232, 178, 48, 255],
                    [82, 148, 88, 255],
                    [70, 118, 182, 255],
                ];
                return palette[(tile.wrapping_mul(2654435761) >> 8) as usize % 4];
            }
            [226, 196, 148, 255]
        },
        _ => return None,
    };
    Some(painted_rect(
        1024,
        256,
        TextureFormat::Rgba8UnormSrgb,
        paint,
    ))
}

/// The kinds that paint their ground storey, for the kit and the tests.
const FRONTED: [BuildingKind; 6] = [
    BuildingKind::FireStation,
    BuildingKind::TownHall,
    BuildingKind::PoliceStation,
    BuildingKind::Barracks,
    BuildingKind::Museum,
    BuildingKind::School,
];

/// The filling station's boards. Not a [`BuildingKind`] — a Tankstelle is a
/// vacant-lot occupant with a canopy rather than a building — but it hangs
/// the same kind of painted sign, so it lives in the same kit, and it gets
/// competitors for the same reason the supermarkets do: one brand on every
/// forecourt in the town is a monopoly, and a monopoly is not a joke.
const TANKSTELLEN: [Plaque; 3] = [
    Plaque {
        title: "TANKSTELLE",
        subline: Some("BENZIN, BRAUSE & BEULENSPRAY"),
        ink: [252, 250, 244, 255],
        field: [206, 96, 22, 255],
        letter: 0.58,
    },
    Plaque {
        title: "SPRIT & SPASS",
        subline: Some("SCHLAUCH BITTE ZURÜCKHÄNGEN"),
        ink: [36, 40, 46, 255],
        field: [238, 214, 66, 255],
        letter: 0.56,
    },
    Plaque {
        title: "TANKHOF ROLLE",
        subline: Some("LUFT KOSTET NICHTS. NOCH."),
        ink: [244, 246, 248, 255],
        field: [30, 70, 132, 255],
        letter: 0.56,
    },
];

/// The market's boards, over the stalls on a market lot. Same arrangement as
/// the Tankstelle: a lot occupant, not a [`BuildingKind`], same kit.
const MAERKTE: [Plaque; 3] = [
    Plaque {
        title: "MARKT",
        subline: Some("HEUTE: ALLES MUSS WEG"),
        ink: [250, 246, 236, 255],
        field: [44, 108, 52, 255],
        letter: 0.60,
    },
    Plaque {
        title: "WOCHENMARKT",
        subline: Some("REGIONAL, SAISONAL, EGAL"),
        ink: [40, 36, 30, 255],
        field: [216, 186, 118, 255],
        letter: 0.58,
    },
    Plaque {
        title: "BAUERNMARKT",
        subline: Some("DIREKT VOM BAUERN, ANGEBLICH"),
        ink: [246, 242, 232, 255],
        field: [110, 58, 44, 255],
        letter: 0.58,
    },
];

// -------------------------------------------------------------- adverts ----

/// One parody poster for the blind gable walls.
///
/// The register is the whole gag: every poster speaks fluent advertising and
/// says nothing, which is only a slight exaggeration of the medium. Kept
/// gentle on purpose — the city laughs at products, not at people.
struct Advert {
    title: &'static str,
    lines: &'static [&'static str],
    ink: [u8; 4],
    field: [u8; 4],
    accent: [u8; 4],
    /// A disc behind the title instead of a bar — the "product shot".
    disc: bool,
}

/// The poster run. One texture and one material each, shared city-wide, the
/// same economy as the plaques — a city with eight adverts on rotation is
/// still truer than a city with none.
const ADVERTS: [Advert; 16] = [
    Advert {
        title: "LAUNENBRAUSE",
        lines: &["JETZT MIT NOCH MEHR GEFÜHL", "OHNE ALLES, DAFÜR VIEL"],
        ink: [252, 248, 240, 255],
        field: [188, 44, 32, 255],
        accent: [240, 196, 48, 255],
        disc: true,
    },
    Advert {
        title: "VERSICHERUNG HOPPLA",
        lines: &["WIR ZAHLEN. IRGENDWANN."],
        ink: [238, 242, 248, 255],
        field: [24, 52, 96, 255],
        accent: [96, 148, 210, 255],
        disc: false,
    },
    Advert {
        title: "NEU: NICHTS",
        lines: &["JETZT AUCH IN BUNT"],
        ink: [34, 32, 36, 255],
        field: [236, 232, 224, 255],
        accent: [214, 108, 160, 255],
        disc: true,
    },
    Advert {
        title: "FLUMMI-FITNESS",
        lines: &["IN 3 WOCHEN RUNDUM RUND"],
        ink: [244, 250, 244, 255],
        field: [28, 112, 60, 255],
        accent: [148, 208, 80, 255],
        disc: true,
    },
    Advert {
        title: "ZAHNCREME STRAHL",
        lines: &["LÄCHELN WIE FRISCH GEWISCHT"],
        ink: [252, 252, 254, 255],
        field: [22, 118, 138, 255],
        accent: [180, 236, 244, 255],
        disc: false,
    },
    Advert {
        title: "URLAUB DAHEIM",
        lines: &["SIE SIND SCHON DA", "KOFFER BLEIBT, SIE AUCH"],
        ink: [250, 246, 234, 255],
        field: [66, 138, 190, 255],
        accent: [244, 208, 72, 255],
        disc: true,
    },
    Advert {
        title: "KAUFEN SIE GLÜCK",
        lines: &["JETZT IM 6ER-PACK"],
        ink: [244, 238, 248, 255],
        field: [96, 52, 128, 255],
        accent: [206, 160, 232, 255],
        disc: true,
    },
    Advert {
        title: "MÖBELHAUS WACKEL",
        lines: &["STEHT. MEISTENS."],
        ink: [242, 234, 220, 255],
        field: [104, 66, 36, 255],
        accent: [196, 148, 92, 255],
        disc: false,
    },
    Advert {
        title: "BEULENSPRAY PLUS",
        lines: &["WIRKT. ANGEBLICH.", "FRAGEN SIE IHRE DELLE"],
        ink: [250, 250, 246, 255],
        field: [16, 118, 128, 255],
        accent: [238, 226, 96, 255],
        disc: true,
    },
    Advert {
        title: "GUMMIWERK OST",
        lines: &["SEIT 1902 ELASTISCH"],
        ink: [232, 228, 216, 255],
        field: [52, 56, 62, 255],
        accent: [198, 92, 38, 255],
        disc: false,
    },
    Advert {
        title: "BRAUSE OHNE ZUCKER",
        lines: &["UND OHNE BRAUSE"],
        ink: [28, 30, 34, 255],
        field: [196, 232, 240, 255],
        accent: [246, 250, 252, 255],
        disc: true,
    },
    Advert {
        title: "ABO FÜR ALLES",
        lines: &["MONATLICH KÜNDBAR", "AB DEM ZWEITEN JAHR"],
        ink: [248, 246, 250, 255],
        field: [72, 40, 106, 255],
        accent: [172, 132, 226, 255],
        disc: false,
    },
    Advert {
        title: "MATRATZEN SANFT",
        lines: &["HÄRTEGRAD: EGAL", "SIE FEDERN JA SELBST"],
        ink: [36, 34, 30, 255],
        field: [234, 220, 196, 255],
        accent: [140, 184, 206, 255],
        disc: true,
    },
    // The one poster in the city with a candidate on it, and the candidate
    // is a list number with no face, no name and no position — which is the
    // only way to parody an election poster without parodying anybody.
    Advert {
        title: "LISTE 7: DIE MITTE",
        lines: &["WIR SIND DAFÜR", "UND NATÜRLICH DAGEGEN"],
        ink: [250, 250, 252, 255],
        field: [38, 76, 54, 255],
        accent: [226, 214, 96, 255],
        disc: false,
    },
    Advert {
        title: "STADTWERKE",
        lines: &["STROM KOMMT AUS DER WAND", "DAS WAR UNSER BEITRAG"],
        ink: [30, 40, 52, 255],
        field: [212, 226, 236, 255],
        accent: [64, 148, 196, 255],
        disc: false,
    },
    Advert {
        title: "DER NEUE HÜPFER",
        lines: &["14 PROZENT MEHR", "MEHR WOVON? MEHR."],
        ink: [246, 244, 240, 255],
        field: [156, 30, 66, 255],
        accent: [246, 244, 240, 255],
        disc: true,
    },
];

/// Poster size in metres. One size for the whole run: posters are printed
/// things, and a print run has a format.
const POSTER: Vec2 = Vec2::new(2.7, 3.8);

/// One line of poster text: where its band sits and how tall its letters
/// are, sized so the glyph cells keep the plaque proportions on the poster's
/// fixed aspect instead of stretching to fill it.
fn poster_line(text: &str, centre_v: f32, tallest_v: f32) -> (Vec3, Vec<u8>) {
    let cells = (text.chars().count() + 2) as f32;
    // A letter `h` of the poster tall is `h * (H/W) * CELL_ASPECT` of the
    // poster wide; the whole line has to fit inside 92% of the width.
    let letter_v = tallest_v.min(0.92 / (cells * CELL_ASPECT * (POSTER.y / POSTER.x)));
    let width_u = cells * letter_v * CELL_ASPECT * (POSTER.y / POSTER.x);
    (Vec3::new(centre_v, letter_v, width_u), encode(text))
}

/// Paints one poster.
fn poster_texture(advert: &Advert) -> Image {
    let mut lines = vec![poster_line(advert.title, 0.50, 0.075)];
    for (index, line) in advert.lines.iter().enumerate() {
        lines.push(poster_line(line, 0.66 + index as f32 * 0.10, 0.042));
    }
    let (ink, field, accent, disc) = (advert.ink, advert.field, advert.accent, advert.disc);

    painted_rect(384, 540, TextureFormat::Rgba8UnormSrgb, move |u, v| {
        // A pale paper margin, so the poster reads as pasted on rather than
        // painted into the wall.
        if !(0.02..=0.98).contains(&u) || !(0.015..=0.985).contains(&v) {
            return [230, 226, 216, 255];
        }
        for (band, text) in &lines {
            let (centre_v, letter_v, width_u) = (band.x, band.y, band.z);
            let band_u = (u - (0.5 - width_u * 0.5)) / width_u;
            let band_v = (v - (centre_v - letter_v * 0.5)) / letter_v;
            if (0.0..1.0).contains(&band_u) && text_band(text, band_u, band_v) {
                return ink;
            }
        }
        // The art direction, such as it is: a product disc or a slogan bar
        // in the upper third, behind nothing, meaning everything.
        if disc {
            let d = Vec2::new((u - 0.5) * POSTER.x, (v - 0.26) * POSTER.y);
            if d.length() < 0.52 {
                return accent;
            }
        } else if (0.16..0.36).contains(&v) {
            return accent;
        }
        field
    })
}

#[derive(Resource)]
pub struct SignKit {
    boards: Vec<(
        BuildingKind,
        Vec<(Handle<Mesh>, Handle<StandardMaterial>, Vec2)>,
    )>,
    tankstelle: Vec<(Handle<Mesh>, Handle<StandardMaterial>, Vec2)>,
    markt: Vec<(Handle<Mesh>, Handle<StandardMaterial>, Vec2)>,
    /// A shared unit quad, scaled per building to its ground storey.
    strip: Handle<Mesh>,
    frontages: Vec<(BuildingKind, Handle<StandardMaterial>)>,
    /// The poster mesh and the run of poster materials.
    poster: Handle<Mesh>,
    adverts: Vec<Handle<StandardMaterial>>,
}

impl SignKit {
    /// The board a particular building hangs: its kind's plaque run, picked
    /// into by the building's own variant so the choice survives the chunk
    /// respawning.
    pub fn get(
        &self,
        kind: BuildingKind,
        variant: u32,
    ) -> Option<(&Handle<Mesh>, &Handle<StandardMaterial>, Vec2)> {
        self.boards
            .iter()
            .find(|(k, ..)| *k == kind)
            .and_then(|(_, run)| run.get(variant as usize % run.len().max(1)))
            .map(|(mesh, material, size)| (mesh, material, *size))
    }

    /// One poster off the run, and the size every poster shares.
    pub fn advert(&self, pick: u32) -> (&Handle<Mesh>, &Handle<StandardMaterial>, Vec2) {
        let material = &self.adverts[pick as usize % self.adverts.len()];
        (&self.poster, material, POSTER)
    }

    /// One forecourt's board, off the run. The variant arrives as raw hash
    /// bits from wherever the lot is, so it wraps, exactly as [`Self::get`]
    /// does.
    pub fn tankstelle(&self, variant: u32) -> (&Handle<Mesh>, &Handle<StandardMaterial>, Vec2) {
        let (mesh, material, size) = &self.tankstelle[variant as usize % self.tankstelle.len()];
        (mesh, material, *size)
    }

    pub fn markt(&self, variant: u32) -> (&Handle<Mesh>, &Handle<StandardMaterial>, Vec2) {
        let (mesh, material, size) = &self.markt[variant as usize % self.markt.len()];
        (mesh, material, *size)
    }

    /// The painted ground storey for a civic kind, as a unit quad and its
    /// material — the caller stretches it across the front face.
    pub fn frontage(
        &self,
        kind: BuildingKind,
    ) -> Option<(&Handle<Mesh>, &Handle<StandardMaterial>)> {
        self.frontages
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, material)| (&self.strip, material))
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
            let run = plaques_for(kind)
                .iter()
                .map(|plaque| {
                    let size = board_size(plaque);
                    let material = materials.add(StandardMaterial {
                        base_color_texture: Some(images.add(board_texture(plaque))),
                        perceptual_roughness: 0.72,
                        ..default()
                    });
                    (meshes.add(Rectangle::new(size.x, size.y)), material, size)
                })
                .collect::<Vec<_>>();
            assert!(!run.is_empty(), "{kind:?} is SIGNED but has no plaque");
            (kind, run)
        })
        .collect();

    let mut lot_board = |plaque: &Plaque| {
        let size = board_size(plaque);
        (
            meshes.add(Rectangle::new(size.x, size.y)),
            materials.add(StandardMaterial {
                base_color_texture: Some(images.add(board_texture(plaque))),
                perceptual_roughness: 0.72,
                ..default()
            }),
            size,
        )
    };
    let tankstelle = TANKSTELLEN.iter().map(&mut lot_board).collect();
    let markt = MAERKTE.iter().map(&mut lot_board).collect();

    let frontages = FRONTED
        .iter()
        .map(|&kind| {
            let image = frontage_texture(kind).expect("every FRONTED kind paints one");
            (
                kind,
                materials.add(StandardMaterial {
                    base_color_texture: Some(images.add(image)),
                    perceptual_roughness: 0.85,
                    ..default()
                }),
            )
        })
        .collect();

    let adverts = ADVERTS
        .iter()
        .map(|advert| {
            materials.add(StandardMaterial {
                base_color_texture: Some(images.add(poster_texture(advert))),
                perceptual_roughness: 0.92,
                ..default()
            })
        })
        .collect();

    SignKit {
        boards,
        tankstelle,
        markt,
        strip: meshes.add(Rectangle::new(1.0, 1.0)),
        frontages,
        poster: meshes.add(Rectangle::new(POSTER.x, POSTER.y)),
        adverts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every word the city paints, for tests that walk all of them.
    fn every_text() -> Vec<(&'static str, String)> {
        let mut texts = Vec::new();
        for kind in SIGNED {
            for plaque in plaques_for(kind) {
                texts.push(("plaque", plaque.title.to_string()));
                if let Some(sub) = plaque.subline {
                    texts.push(("plaque", sub.to_string()));
                }
            }
        }
        for plaque in TANKSTELLEN.iter().chain(&MAERKTE) {
            texts.push(("lot board", plaque.title.to_string()));
            if let Some(sub) = plaque.subline {
                texts.push(("lot board", sub.to_string()));
            }
        }
        for advert in &ADVERTS {
            texts.push(("advert", advert.title.to_string()));
            for line in advert.lines {
                texts.push(("advert", line.to_string()));
            }
        }
        texts
    }

    #[test]
    fn every_sign_fits_the_font() {
        use crate::world::texture::glyph;
        for (kind, text) in every_text() {
            for code in encode(&text) {
                assert!(
                    code == b' ' || glyph(code) != [0; 7],
                    "a {kind} says {text:?} and the font cannot draw {:?}",
                    code as char
                );
            }
        }
    }

    #[test]
    fn flats_and_offices_stay_anonymous() {
        assert!(plaques_for(BuildingKind::Apartments).is_empty());
        assert!(plaques_for(BuildingKind::Offices).is_empty());
    }

    #[test]
    fn a_board_is_mostly_field_with_ink_on_it() {
        // The failure this catches is a band-arithmetic slip that paints the
        // whole board in ink, or none of it — both build, both look like a
        // coloured rectangle from the street.
        for kind in SIGNED {
            for plaque in plaques_for(kind) {
                let image = board_texture(plaque);
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
                    "{kind:?} {:?}: {ink:.3} of the board is ink",
                    plaque.title
                );
            }
        }
    }

    #[test]
    fn a_poster_is_mostly_field_with_ink_on_it() {
        // Same slip, same disguise: a poster that paints all ink or no ink
        // still builds and still hangs, it just stops being an advert.
        for advert in &ADVERTS {
            let image = poster_texture(advert);
            let data = image.data.as_ref().expect("the poster was not painted");
            let ink = data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[..3] == advert.ink[..3])
                .count() as f32
                / (data.len() / 4) as f32;
            assert!(
                (0.005..0.30).contains(&ink),
                "{:?}: {ink:.3} of the poster is ink",
                advert.title
            );
        }
    }

    #[test]
    fn every_poster_line_fits_on_the_poster() {
        // The line-fitting maths clamps the letter height so the widest line
        // stays inside the sheet; a regression here paints letters off the
        // edge, which crops the punchline.
        for advert in &ADVERTS {
            for text in std::iter::once(&advert.title).chain(advert.lines) {
                let (band, _) = poster_line(text, 0.5, 0.075);
                assert!(
                    band.z <= 0.921,
                    "{text:?} runs {:.2} of the poster wide",
                    band.z
                );
                assert!(band.y > 0.0, "{text:?} has no letter height at all");
            }
        }
    }

    #[test]
    fn only_the_sealed_civic_kinds_paint_a_frontage() {
        for kind in FRONTED {
            assert!(frontage_texture(kind).is_some(), "{kind:?}");
            assert!(
                !kind.enterable(),
                "{kind:?} has an open door and a painted one"
            );
        }
        // The enterable kinds show who they are through the doorway instead.
        assert!(frontage_texture(BuildingKind::Supermarket).is_none());
        assert!(frontage_texture(BuildingKind::Apartments).is_none());
    }

    #[test]
    fn the_fire_station_doors_reach_the_pavement() {
        // An orientation guard: the strip's last texture row is the
        // pavement line. A door painted at the wrong end of `v` would hang
        // from the lintel, and every painter below inherits the mistake.
        let image = frontage_texture(BuildingKind::FireStation).unwrap();
        let data = image.data.as_ref().unwrap();
        let width = image.texture_descriptor.size.width as usize;
        let height = image.texture_descriptor.size.height as usize;
        let at = |x: usize, y: usize| {
            let i = (y * width + x) * 4;
            [data[i], data[i + 1], data[i + 2]]
        };
        let field = [150, 28, 24];
        assert_eq!(
            at(width / 2, 2),
            field,
            "the lintel over the middle door is missing"
        );
        assert_ne!(
            at(width / 2, height - 2),
            field,
            "the middle door never reaches the ground"
        );
    }

    #[test]
    fn the_police_station_confesses_why_it_is_shut() {
        let [plaque] = plaques_for(BuildingKind::PoliceStation) else {
            panic!("the police station hangs exactly one sign");
        };
        assert!(plaque.subline.unwrap().contains("FREUNDLICHKEIT"));
    }

    #[test]
    fn the_quarters_still_get_the_cuisine_they_were_promised() {
        // `buildings::spawn_building` forces the restaurant sign by index in
        // Klein-Neapel and the Fernost-Viertel. Adding a plaque *above*
        // either of them compiles, passes every other test here, and
        // silently re-cuisines two whole quarters — so the indices are
        // pinned where the names are, rather than where the override is.
        let run = plaques_for(BuildingKind::Restaurant);
        assert_eq!(run[1].title, "PIZZERIA LUIGI LUIGI", "Klein-Neapel");
        assert_eq!(run[3].title, "WOK & WEG", "the Fernost-Viertel");
    }

    #[test]
    fn the_chains_have_real_competition() {
        // Three of anything is a chain; two is a duopoly and one is a
        // monopoly wearing a sign. The kinds a street is actually made of
        // have to carry enough plaques that two neighbouring shops of the
        // same kind are usually different shops.
        for kind in [
            BuildingKind::Supermarket,
            BuildingKind::Restaurant,
            BuildingKind::Hotel,
        ] {
            assert!(
                plaques_for(kind).len() >= 4,
                "{kind:?} has only {} shopfronts in the whole city",
                plaques_for(kind).len()
            );
        }
    }

    #[test]
    fn no_two_signs_in_the_city_say_the_same_thing() {
        // Two plaques with the same title are one plaque and a typo, and a
        // poster run with a duplicate in it wastes a texture on saying
        // something the wall already says.
        let mut seen = std::collections::HashSet::new();
        for kind in SIGNED {
            for plaque in plaques_for(kind) {
                assert!(seen.insert(plaque.title), "{:?} hangs twice", plaque.title);
            }
        }
        for advert in &ADVERTS {
            assert!(
                seen.insert(advert.title),
                "{:?} is pasted twice",
                advert.title
            );
        }
        for plaque in TANKSTELLEN.iter().chain(&MAERKTE) {
            assert!(seen.insert(plaque.title), "{:?} hangs twice", plaque.title);
        }
    }

    #[test]
    fn any_variant_number_lands_on_a_lot_board() {
        // Same trap as the building boards, and the same wrap: the variant
        // is raw hash bits from wherever the lot is, and an unwrapped index
        // panics on the first forecourt past the end of the run.
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut meshes = Assets::<Mesh>::default();
        let kit = build_assets(&mut images, &mut materials, &mut meshes);
        for variant in [0u32, 1, 2, 3, u32::MAX, 3_185_463_605] {
            kit.tankstelle(variant);
            kit.markt(variant);
        }
        assert_ne!(
            kit.markt(0).1.clone(),
            kit.markt(1).1.clone(),
            "every market in the city has the same name over it"
        );
    }

    #[test]
    fn the_civic_kinds_stay_singular() {
        // One Rathaus, one sense of humour. The chains are where the
        // variants live; a civic kind growing a second plaque means two
        // town halls disagreeing about their own name across one city.
        for kind in SIGNED {
            let count = plaques_for(kind).len();
            if kind.is_civic() {
                assert_eq!(count, 1, "{kind:?} hangs {count} different signs");
            } else {
                assert!(count >= 2, "{kind:?} deserves competitors by now");
            }
        }
    }

    #[test]
    fn any_variant_number_lands_on_a_board() {
        // The variant arrives as raw seed bits, so `get` must wrap it into
        // the run — an unwrapped index is a supermarket with no sign, which
        // does not fail, it just quietly unhangs most of the city.
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut meshes = Assets::<Mesh>::default();
        let kit = build_assets(&mut images, &mut materials, &mut meshes);
        for kind in SIGNED {
            for variant in [0u32, 1, 2, 3, u32::MAX, 3_185_463_605] {
                assert!(
                    kit.get(kind, variant).is_some(),
                    "{kind:?} v{variant} has no board"
                );
            }
        }
        assert!(kit.get(BuildingKind::Apartments, 0).is_none());
        // And the run actually varies: two adjacent supermarkets with
        // different variants hang different boards.
        let a = kit.get(BuildingKind::Supermarket, 0).unwrap().1.clone();
        let b = kit.get(BuildingKind::Supermarket, 1).unwrap().1.clone();
        assert_ne!(a, b, "every supermarket wears the same sign");
    }
}
