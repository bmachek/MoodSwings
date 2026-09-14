//! Persisted player preferences: rebindable keys, mouse-look direction, audio
//! and graphics — everything the pause menu's settings screens change.
//!
//! `GameConfig` already derives `Serialize`/`Deserialize` (see its module
//! docs), so most of this is just writing it to `saves/options.ron` instead
//! of only ever holding live defaults. Key bindings are the exception: an
//! `InputMap` stores its bindings as trait objects and cannot round-trip
//! through serde on its own, so the handful of keys a player can rebind live
//! in their own small map instead, keyed by [`RebindableAction`] rather than
//! by `Action` so the menu never has to reject an axis or a mouse button as
//! "not rebindable" — the type says so up front.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::config::GameConfig;
use crate::player::input::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RebindableAction {
    Jump,
    Sprint,
    Interact,
    Handbrake,
    Pause,
    Map,
    QuickSave,
    QuickLoad,
    ToggleDebugCamera,
}

impl RebindableAction {
    pub const ALL: [Self; 9] = [
        Self::Jump,
        Self::Sprint,
        Self::Interact,
        Self::Handbrake,
        Self::Pause,
        Self::Map,
        Self::QuickSave,
        Self::QuickLoad,
        Self::ToggleDebugCamera,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Jump => "Springen",
            Self::Sprint => "Sprinten",
            Self::Interact => "Interagieren / Einsteigen",
            Self::Handbrake => "Handbremse",
            Self::Pause => "Menü",
            Self::Map => "Karte",
            Self::QuickSave => "Schnellspeichern",
            Self::QuickLoad => "Schnellladen",
            Self::ToggleDebugCamera => "Freie Kamera (Debug)",
        }
    }

    pub fn action(self) -> Action {
        match self {
            Self::Jump => Action::Jump,
            Self::Sprint => Action::Sprint,
            Self::Interact => Action::Interact,
            Self::Handbrake => Action::Handbrake,
            Self::Pause => Action::Pause,
            Self::Map => Action::Map,
            Self::QuickSave => Action::QuickSave,
            Self::QuickLoad => Action::QuickLoad,
            Self::ToggleDebugCamera => Action::ToggleDebugCamera,
        }
    }

    /// The keyboard binding `Action::default_input_map` gives this action.
    /// [`Action::input_map`] needs this to know exactly which binding to
    /// replace rather than clearing the action's bindings outright, which
    /// would also throw away its gamepad button.
    pub fn default_key(self) -> KeyCode {
        match self {
            Self::Jump => KeyCode::Space,
            Self::Sprint => KeyCode::ShiftLeft,
            Self::Interact => KeyCode::KeyF,
            Self::Handbrake => KeyCode::Space,
            Self::Pause => KeyCode::Escape,
            Self::Map => KeyCode::KeyM,
            Self::QuickSave => KeyCode::F5,
            Self::QuickLoad => KeyCode::F9,
            Self::ToggleDebugCamera => KeyCode::F1,
        }
    }
}

/// Live key bindings for the rebindable actions.
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindings(pub HashMap<RebindableAction, KeyCode>);

impl Default for KeyBindings {
    fn default() -> Self {
        Self(
            RebindableAction::ALL
                .into_iter()
                .map(|action| (action, action.default_key()))
                .collect(),
        )
    }
}

impl KeyBindings {
    pub fn key_for(&self, action: RebindableAction) -> KeyCode {
        self.0
            .get(&action)
            .copied()
            .unwrap_or_else(|| action.default_key())
    }
}

/// The whole of what the settings screens change, together in one file so
/// loading and saving are each a single call.
///
/// `#[serde(default)]` for the same reason every block in [`GameConfig`] has
/// it: a third field added here without one would make every existing
/// `options.ron` unparseable, and [`load`]'s answer to that is to discard the
/// file. Two fields today is exactly when this is cheap to add.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Options {
    config: GameConfig,
    keybindings: KeyBindings,
}

fn options_path() -> PathBuf {
    Path::new("saves").join("options.ron")
}

/// Reads `saves/options.ron` onto `config` and `keybindings`, if it exists
/// and parses. Silent on any failure beyond a log line: a missing or corrupt
/// options file must never stop the game from starting with defaults.
///
/// A file that will not parse is *moved aside* rather than left where it is,
/// because the next [`save`] would otherwise overwrite it and the only copy of
/// the player's settings would be gone before anyone could look at why it
/// stopped parsing. The warning line naming the file is the whole recovery
/// procedure.
fn load(config: &mut GameConfig, keybindings: &mut KeyBindings) {
    let path = options_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    match ron::from_str::<Options>(&text) {
        Ok(options) => {
            *config = options.config;
            *keybindings = options.keybindings;
        }
        Err(error) => {
            let kept = path.with_extension("ron.unreadable");
            match std::fs::rename(&path, &kept) {
                Ok(()) => warn!(
                    "saves/options.ron did not parse ({error}); kept as {} and starting from defaults",
                    kept.display()
                ),
                Err(move_error) => warn!(
                    "saves/options.ron did not parse ({error}) and could not be set aside \
                     ({move_error}); starting from defaults"
                ),
            }
        }
    }
}

/// Writes `config` and `keybindings` to `saves/options.ron`.
pub fn save(config: &GameConfig, keybindings: &KeyBindings) {
    // A server's world must not become the next single-player world's seed.
    if crate::multiplayer::active() {
        return;
    }
    let options = Options {
        config: config.clone(),
        keybindings: keybindings.clone(),
    };
    let text = match ron::ser::to_string_pretty(&options, ron::ser::PrettyConfig::default()) {
        Ok(text) => text,
        Err(error) => {
            error!("could not serialise settings: {error}");
            return;
        }
    };
    let path = options_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&path, text) {
        error!("could not save settings: {error}");
    }
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let mut config = GameConfig::default();
        let mut keybindings = KeyBindings::default();
        load(&mut config, &mut keybindings);
        app.insert_resource(config).insert_resource(keybindings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::AudioConfig;

    /// Every rebindable action is in `ALL` and has a key.
    ///
    /// The match is the real check and it is checked by the compiler: adding a
    /// variant to `RebindableAction` without visiting this test is a build
    /// error, and a variant missing from `ALL` is an action the Tastenbelegung
    /// screen cannot show and the player therefore cannot rebind.
    ///
    /// Note what is *not* asserted: that the default keys are distinct. Jump
    /// and Handbrake are both Space on purpose — one is on foot and the other
    /// is in a car, and they never contend.
    #[test]
    fn every_rebindable_action_is_listed_and_bound() {
        fn listed(action: RebindableAction) -> bool {
            match action {
                RebindableAction::Jump
                | RebindableAction::Sprint
                | RebindableAction::Interact
                | RebindableAction::Handbrake
                | RebindableAction::Pause
                | RebindableAction::Map
                | RebindableAction::QuickSave
                | RebindableAction::QuickLoad
                | RebindableAction::ToggleDebugCamera => true,
            }
        }

        let bindings = KeyBindings::default();
        for action in RebindableAction::ALL {
            assert!(listed(action), "{action:?} is not accounted for");
            assert!(
                !action.label().is_empty(),
                "{action:?} has no label for the menu"
            );
            assert_eq!(
                bindings.key_for(action),
                action.default_key(),
                "{action:?} is missing from the default bindings"
            );
        }

        let mut seen: Vec<_> = RebindableAction::ALL.to_vec();
        let before = seen.len();
        seen.sort_by_key(|a| format!("{a:?}"));
        seen.dedup();
        assert_eq!(before, seen.len(), "ALL lists an action twice");
    }

    #[test]
    fn a_binding_the_file_never_mentioned_falls_back_to_its_default_key() {
        // What an options file from before an action existed leaves behind: a
        // map with everything but that one entry.
        let mut bindings = KeyBindings::default();
        bindings.0.remove(&RebindableAction::Map);
        assert_eq!(
            bindings.key_for(RebindableAction::Map),
            RebindableAction::Map.default_key()
        );
    }

    #[test]
    fn options_survive_a_round_trip_through_ron() {
        let mut keybindings = KeyBindings::default();
        keybindings.0.insert(RebindableAction::Jump, KeyCode::KeyQ);
        let written = Options {
            config: GameConfig {
                world_seed: 0x1234_5678,
                audio: AudioConfig {
                    master: 0.42,
                    ..Default::default()
                },
                ..Default::default()
            },
            keybindings,
        };

        let text = ron::ser::to_string(&written).expect("options should serialise");
        let read: Options = ron::from_str(&text).expect("options should parse back");

        assert_eq!(read.config.audio.master, 0.42);
        assert_eq!(read.config.world_seed, 0x1234_5678);
        assert_eq!(
            read.keybindings.key_for(RebindableAction::Jump),
            KeyCode::KeyQ
        );
    }

    /// An options file missing either half still loads the other.
    ///
    /// This is the failure this module has actually paid for once: the loader
    /// throws away the *whole* file when any part of it will not parse, so a
    /// field added without a default resets the player's city, costume and
    /// keybindings together. `Options` carries `#[serde(default)]` so that a
    /// third field arriving here cannot do it again.
    #[test]
    fn an_options_file_missing_a_half_still_loads_the_other() {
        let only_config = "(config:(world_seed:99))";
        let parsed: Options =
            ron::from_str(only_config).expect("half an options file should parse");
        assert_eq!(parsed.config.world_seed, 99);
        assert_eq!(
            parsed.keybindings.key_for(RebindableAction::Jump),
            RebindableAction::Jump.default_key()
        );

        let only_keys = "(keybindings:({}))";
        let parsed: Options = ron::from_str(only_keys).expect("half an options file should parse");
        assert_eq!(parsed.config.world_seed, GameConfig::default().world_seed);
    }

    /// A field this build no longer knows about is ignored rather than fatal.
    ///
    /// Serde's default behaviour, asserted here because the whole point of the
    /// loader's discipline is that an options file written by a *different*
    /// build still opens — and that cuts both ways, forwards and back.
    #[test]
    fn a_field_this_build_does_not_know_is_ignored() {
        let from_the_future = "(config:(world_seed:7,haircut_gravity:0.5),keybindings:({}))";
        let parsed: Options =
            ron::from_str(from_the_future).expect("an unknown field should not be fatal");
        assert_eq!(parsed.config.world_seed, 7);
    }
}
