//! The heads-up display.
//!
//! There is no health, no money and no wanted level to show, because none of
//! those exist any more. What is left is the only number the game is about: how
//! the city feels. It is shown three ways — the player's own face, their own
//! mood, and the average of everyone resident — because the joke is the gap
//! between them. A delighted face in a furious street is funnier than either.
//!
//! The words on screen are German, matching `ui::menu`. Everything a player
//! reads is; everything a developer reads — the dev panel, the logs, the code
//! itself — is English.

use bevy::prelude::*;
use bevy::text::FontSize;
use leafwing_input_manager::prelude::ActionState;

use super::minimap::{MapOpen, MinimapImage};
use crate::core::schedule::GameSet;
use crate::mood::face::{self, FaceAssets};
use crate::mood::feeling::CityMood;
use crate::player::input::Action;

const PANEL: Color = Color::srgba(0.05, 0.06, 0.09, 0.62);
const INK: Color = Color::srgb(0.93, 0.95, 0.98);
/// The bar colours at furious, indifferent and delighted.
const SOUR: Color = Color::srgb(0.86, 0.19, 0.16);
const FLAT: Color = Color::srgb(0.95, 0.72, 0.16);
const SWEET: Color = Color::srgb(0.36, 0.78, 0.34);

#[derive(Component)]
struct MinimapFrame;

/// Which mood a bar is showing. One marker with a discriminant rather than two
/// marker types, so the widget code is one loop.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Meter {
    Own,
    City,
}

/// On the caption above a bar. Carried so the text is addressable later; the
/// bars themselves are found by their [`Meter`].
#[derive(Component)]
struct Caption;

#[derive(Component)]
struct FacePortrait;

#[derive(Component)]
struct RageBanner;

/// The banner announcing the day's scheduled event, under the rage banner —
/// a demo can put both up at once, which is exactly right.
#[derive(Component)]
struct EventShout;

#[derive(Component)]
enum StreetReadout {
    Place,
    Moments,
    Controls,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_hud).add_systems(
            Update,
            (
                toggle_map,
                size_map_frame,
                show_the_mood,
                show_the_event,
                show_the_street,
            )
                .chain()
                .in_set(GameSet::Ui),
        );
    }
}

/// A proportion, drawn as a filled track.
fn bar(fill: Color, marker: impl Component) -> impl Bundle {
    (
        Node {
            width: Val::Px(168.0),
            height: Val::Px(10.0),
            border: UiRect::all(Val::Px(1.0)),
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.18)),
        children![(
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(fill),
            marker,
        )],
    )
}

fn label(text: &str, size: f32, color: Color, marker: impl Component) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        marker,
    )
}

/// The colour a mood reads at, red through amber to green.
fn tint(mood: f32) -> Color {
    let mood = mood.clamp(-1.0, 1.0);
    let (from, to, t) = if mood < 0.0 {
        (SOUR, FLAT, mood + 1.0)
    } else {
        (FLAT, SWEET, mood)
    };
    from.mix(&to, t)
}

/// How much of a bar a mood fills. The track runs the whole range, so
/// indifference is a half-full bar rather than an empty one — an empty bar
/// reads as "nothing here" and a mood of zero is not nothing.
fn fill_fraction(mood: f32) -> f32 {
    (mood.clamp(-1.0, 1.0) + 1.0) * 0.5
}

fn spawn_hud(mut commands: Commands, minimap: Res<MinimapImage>, faces: Res<FaceAssets>) {
    // Separate roots anchor to the viewport rather than to the mood column.
    // A compact line gives the city a time and a place without a debug panel.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(18.0),
            left: Val::Px(20.0),
            max_width: Val::Percent(45.0),
            padding: UiRect::all(Val::Px(10.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(PANEL),
        Pickable::IGNORE,
        GlobalZIndex(11),
        children![label("", 16.0, INK, StreetReadout::Place)],
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(20.0),
            bottom: Val::Px(20.0),
            width: Val::Px(330.0),
            max_width: Val::Percent(44.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(12.0),
            padding: UiRect::all(Val::Px(14.0)),
            border_radius: BorderRadius::all(Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(PANEL),
        Pickable::IGNORE,
        GlobalZIndex(11),
        children![
            label("", 15.0, INK, StreetReadout::Moments),
            label(
                "",
                13.0,
                Color::srgb(0.7, 0.82, 0.85),
                StreetReadout::Controls
            ),
        ],
    ));
    commands.spawn((
        Name::new("HUD"),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(16.0)),
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
        // The HUD is decoration; it must never eat clicks meant for the world.
        Pickable::IGNORE,
        GlobalZIndex(10),
        children![
            (
                // --- left column: the minimap, anchored to the bottom ---
                Node {
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::FlexEnd,
                    height: Val::Percent(100.0),
                    ..default()
                },
                children![(
                    Node {
                        width: Val::Px(170.0),
                        height: Val::Px(170.0),
                        border: UiRect::all(Val::Px(2.0)),
                        overflow: Overflow::clip(),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.22)),
                    BackgroundColor(PANEL),
                    MinimapFrame,
                    children![(
                        ImageNode::new(minimap.0.clone()),
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                    )],
                )],
            ),
            // --- right column: the face, and the two moods it sits between ---
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    ..default()
                },
                children![(
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        padding: UiRect::all(Val::Px(10.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(PANEL),
                    children![
                        (
                            ImageNode::new(faces.portrait(face::LEVELS / 2)),
                            Node {
                                width: Val::Px(52.0),
                                height: Val::Px(52.0),
                                ..default()
                            },
                            FacePortrait,
                        ),
                        (
                            Node {
                                flex_direction: FlexDirection::Column,
                                ..default()
                            },
                            children![
                                label("Du", 12.0, INK, Caption),
                                bar(FLAT, Meter::Own),
                                label("Die Stadt", 12.0, INK, Caption),
                                bar(FLAT, Meter::City),
                            ],
                        ),
                    ],
                )],
            ),
            // --- and the announcement when the street turns on itself ---
            (
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(40.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Visibility::Hidden,
                RageBanner,
                children![(
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(30.0),
                        ..default()
                    },
                    TextColor(SOUR),
                )],
            ),
            // --- and the day's event, while one is on the streets ---
            (
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(78.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Visibility::Hidden,
                EventShout,
                children![(
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(24.0),
                        ..default()
                    },
                    TextColor(INK),
                )],
            ),
        ],
    ));
}

/// Puts the city's temperature on screen: the player's own face and mood, the
/// average of everybody resident, and a shout when a lot of them go red at once.
fn show_the_mood(
    city: Res<CityMood>,
    faces: Res<FaceAssets>,
    mut worn: Local<Option<usize>>,
    mut portraits: Query<&mut ImageNode, With<FacePortrait>>,
    mut fills: Query<(&Meter, &mut Node, &mut BackgroundColor)>,
    mut banners: Query<(&mut Visibility, &Children), With<RageBanner>>,
    mut shouts: Query<&mut Text>,
) {
    for (meter, mut node, mut colour) in &mut fills {
        let mood = match meter {
            Meter::Own => city.player,
            Meter::City => city.average,
        };
        node.width = Val::Percent(fill_fraction(mood) * 100.0);
        colour.0 = tint(mood);
    }

    let level = face::level_of(city.player);
    if *worn != Some(level) {
        *worn = Some(level);
        for mut portrait in &mut portraits {
            portrait.image = faces.portrait(level);
        }
    }

    for (mut visibility, children) in &mut banners {
        let showing = city.wave > 0.0;
        *visibility = if showing {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !showing {
            continue;
        }
        for &child in children {
            if let Ok(mut text) = shouts.get_mut(child) {
                let count = city.wave_size;
                **text = format!("Wut-Welle! {count} Bürger");
            }
        }
    }
}

/// Shows whatever `events` says is on the streets right now. The colour
/// follows the kind: a CSD announces itself in the HUD's own ink, a demo in
/// the same sour red the rage wave uses.
fn show_the_event(
    banner: Res<crate::events::EventBanner>,
    mut shouts: Query<(&mut Visibility, &Children), With<EventShout>>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    if !banner.is_changed() {
        return;
    }
    for (mut visibility, children) in &mut shouts {
        *visibility = if banner.0.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let Some((line, kind)) = &banner.0 else {
            continue;
        };
        for &child in children {
            if let Ok((mut text, mut colour)) = texts.get_mut(child) {
                **text = line.clone();
                colour.0 = match kind {
                    crate::events::EventKind::Csd => INK,
                    crate::events::EventKind::Demo => SOUR,
                };
            }
        }
    }
}

fn toggle_map(mut map_open: ResMut<MapOpen>, actions: Query<&ActionState<Action>>) {
    let Ok(action_state) = actions.single() else {
        return;
    };
    if action_state.just_pressed(&Action::Map) {
        map_open.0 = !map_open.0;
    }
}

/// Sizes the map panel from the state rather than from the keypress, so
/// anything that sets `MapOpen` — a menu, a script, the capture tool — gets the
/// full-size map instead of a zoomed-out city crammed into a minimap frame.
fn size_map_frame(
    map_open: Res<MapOpen>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut frames: Query<&mut Node, With<MinimapFrame>>,
) {
    // One texture serves both views: the camera zooms out, the frame grows.
    let available = windows
        .single()
        .map_or(640.0, |w| (w.width().min(w.height()) - 48.0).max(100.0));
    let size = if map_open.0 {
        available.min(640.0)
    } else {
        available.min(170.0)
    };
    for mut node in &mut frames {
        node.width = Val::Px(size);
        node.height = Val::Px(size);
    }
}

/// Four updates a second is enough for readable instruments. In particular,
/// do not lay out new text sixty times a second just because the car moved.
fn show_the_street(
    time: Res<Time>,
    mut elapsed: Local<f32>,
    config: Res<crate::core::config::GameConfig>,
    clock: Res<crate::world::timeofday::TimeOfDay>,
    weather: Res<crate::world::weather::Weather>,
    bindings: Res<crate::core::settings::KeyBindings>,
    moments: Res<crate::player::stroll::StreetMoments>,
    map: Res<MapOpen>,
    players: Query<
        (&Transform, Option<&crate::player::interact::Driving>),
        With<crate::player::on_foot::Player>,
    >,
    vehicles: Query<
        (
            Entity,
            &Transform,
            &avian3d::prelude::LinearVelocity,
            Option<&crate::player::interact::DrivenBy>,
        ),
        With<crate::vehicle::spawn::Vehicle>,
    >,
    mut readouts: Query<(&StreetReadout, &mut Text)>,
) {
    *elapsed -= time.delta_secs();
    if *elapsed > 0.0 {
        return;
    }
    *elapsed = 0.25;
    let Ok((player, driving)) = players.single() else {
        return;
    };
    let minutes = (clock.hours.rem_euclid(24.0) * 60.0) as u32;
    let sky = if weather.rain > 0.15 {
        "Regen"
    } else if weather.cover > 0.65 {
        "Bewölkt"
    } else if crate::world::timeofday::daylight(clock.hours) < 0.1 {
        "Nacht"
    } else {
        "Heiter"
    };
    let place = format!(
        "{}  ·  {:02}:{:02}  ·  {}",
        config.city.label(),
        minutes / 60,
        minutes % 60,
        sky
    );
    let tune = &config.stroll;
    let check = |done: bool| if done { "✓" } else { "·" };
    let invitation = if !tune.enabled || map.0 {
        String::new()
    } else if moments.notice_left > 0.0 {
        moments.notice.clone()
    } else {
        format!(
            "DEINE STADTMOMENTE\nGanz ohne Eile. Für diesen Ausflug.\n\n{} Frische Luft: {:.0}/{:.0} m zu Fuß\n{} Gute Nachbarschaft: {}/{} aufgemuntert\n{} Erste Reihe: {:.0}/{:.0} s Straßenmusik",
            check(moments.completed[0]),
            moments.walked,
            tune.walk_metres.max(1.0),
            check(moments.completed[1]),
            moments.cheered.len(),
            tune.cheer_people.clamp(1, 100),
            check(moments.completed[2]),
            moments.listened,
            tune.listeners_seconds.max(1.0)
        )
    };
    let key = |action| {
        let raw = format!("{:?}", bindings.key_for(action));
        raw.strip_prefix("Key").unwrap_or(&raw).to_string()
    };
    use crate::core::settings::RebindableAction as Binding;
    let controls = if let Some(driving) = driving {
        let speed = vehicles
            .get(driving.0)
            .map_or(0.0, |(_, _, v, _)| v.0.xz().length() * 3.6);
        format!(
            "{speed:.0} km/h\n{} / B: Handbremse · {} / Y: Aussteigen",
            key(Binding::Handbrake),
            key(Binding::Interact)
        )
    } else {
        let car_near = vehicles.iter().any(|(_, car, _, driver)| {
            driver.is_none()
                && car.translation.distance(player.translation)
                    <= crate::player::interact::ENTER_RANGE
        });
        let hint = if car_near {
            format!("{} / Y: Einsteigen", key(Binding::Interact))
        } else {
            "Rechtsklick / LT: Aufmuntern".to_string()
        };
        format!(
            "{hint}\nMittelklick / X: Blume verschenken\n{} / Select: Karte · F3: Entwicklerfenster",
            key(Binding::Map)
        )
    };
    for (kind, mut text) in &mut readouts {
        let value = match kind {
            StreetReadout::Place => &place,
            StreetReadout::Moments => &invitation,
            StreetReadout::Controls => &controls,
        };
        if text.0 != *value {
            text.0.clone_from(value);
        }
    }
}
