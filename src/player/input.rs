//! Device-agnostic action mapping.
//!
//! Gameplay never reads keys directly; it reads `ActionState<Action>`. That
//! keeps keyboard and gamepad on one path and makes rebinding a data change.
//!
//! `Move` is deliberately shared between on-foot and driving: on foot it is a
//! direction, in a car its Y is throttle/brake and its X is steering. Same for
//! `Jump`/`Handbrake` sharing Space — context decides which one reads it, which
//! is exactly how the games this borrows from behave.
//!
//! [`Action::Taunt`] and [`Action::Cheer`] sit on the two mouse buttons that
//! used to fire and aim a weapon. The bindings are unchanged on purpose: the
//! trigger finger already knows where they are, and a game about being rude to
//! strangers wants its rudeness under the same thumb a shooter puts a gun.

use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::core::schedule::GameSet;
use crate::core::settings::{KeyBindings, RebindableAction};

#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum Action {
    #[actionlike(DualAxis)]
    Move,
    #[actionlike(DualAxis)]
    Look,
    /// A stick is a rate; mouse motion is a displacement. Mixing both into
    /// Look loses the units and makes controller look frame-rate dependent.
    #[actionlike(DualAxis)]
    LookStick,
    Jump,
    Sprint,
    /// Enter/exit vehicle, pick things up.
    Interact,
    /// Be rude at everybody nearby: a raspberry, a fart, a cough, a spit.
    Taunt,
    /// Whistle at them instead.
    Cheer,
    /// Say sorry and throw a flower to whoever holds it against you.
    Apologize,
    Handbrake,
    Pause,
    /// Opens the full-screen map.
    Map,
    QuickSave,
    QuickLoad,
    /// Toggles the free-fly debug camera.
    ToggleDebugCamera,
    ToggleTools,
}

impl Action {
    /// Everything the *game* reads, as against the keys that open a
    /// developer's window or pause the world.
    ///
    /// `ui::debug` disables this set wholesale while the tuning panel holds
    /// the cursor. Without it the panel is unusable as a panel: the cursor is
    /// free so that a slider can be dragged, and every drag also taunts the
    /// street, every WASD keystroke walks the player out of the thing being
    /// tuned, and F5 quick-saves whatever mess that made. Gating each reader
    /// would mean a `run_if` on nine systems in six modules and a tenth one
    /// forgotten; there is one action state and this is the one place.
    ///
    /// `Pause` stays live deliberately — it is the way out — and so do the
    /// two developer toggles, or the panel could not be closed again.
    pub const IN_PLAY: [Self; 13] = [
        Self::Move,
        Self::Look,
        Self::LookStick,
        Self::Jump,
        Self::Sprint,
        Self::Interact,
        Self::Taunt,
        Self::Cheer,
        Self::Apologize,
        Self::Handbrake,
        Self::Map,
        Self::QuickSave,
        Self::QuickLoad,
    ];

    pub fn default_input_map() -> InputMap<Self> {
        let mut map = InputMap::default();

        // Keyboard and mouse.
        map.insert_dual_axis(Self::Move, VirtualDPad::wasd());
        map.insert_dual_axis(Self::Look, MouseMove::default());
        map.insert(Self::Jump, KeyCode::Space);
        map.insert(Self::Sprint, KeyCode::ShiftLeft);
        map.insert(Self::Interact, KeyCode::KeyF);
        map.insert(Self::Taunt, MouseButton::Left);
        map.insert(Self::Cheer, MouseButton::Right);
        map.insert(Self::Apologize, MouseButton::Middle);
        map.insert(Self::Handbrake, KeyCode::Space);
        map.insert(Self::Pause, KeyCode::Escape);
        map.insert(Self::Map, KeyCode::KeyM);
        map.insert(Self::QuickSave, KeyCode::F5);
        map.insert(Self::QuickLoad, KeyCode::F9);
        map.insert(Self::ToggleDebugCamera, KeyCode::F1);
        map.insert(Self::ToggleTools, KeyCode::F3);

        // Gamepad.
        map.insert_dual_axis(Self::Move, GamepadStick::LEFT);
        map.insert_dual_axis(Self::LookStick, GamepadStick::RIGHT);
        map.insert(Self::Jump, GamepadButton::South);
        map.insert(Self::Sprint, GamepadButton::LeftThumb);
        map.insert(Self::Interact, GamepadButton::North);
        map.insert(Self::Taunt, GamepadButton::RightTrigger2);
        map.insert(Self::Cheer, GamepadButton::LeftTrigger2);
        map.insert(Self::Apologize, GamepadButton::West);
        map.insert(Self::Handbrake, GamepadButton::East);
        map.insert(Self::Pause, GamepadButton::Start);
        map.insert(Self::Map, GamepadButton::Select);

        map
    }

    /// Same as [`Self::default_input_map`], except the keyboard side of the
    /// rebindable actions comes from `keybindings` instead of the hard-coded
    /// defaults.
    ///
    /// Built by taking the default map and swapping out just the bindings
    /// that differ, rather than clearing each action outright: an action like
    /// `Jump` also carries a gamepad button, and clearing it to rebind the
    /// key would throw that away too.
    pub fn input_map(keybindings: &KeyBindings) -> InputMap<Self> {
        let mut map = Self::default_input_map();
        for rebindable in RebindableAction::ALL {
            let bound = keybindings.key_for(rebindable);
            let default = rebindable.default_key();
            if bound != default {
                let action = rebindable.action();
                map.remove(&action, default);
                map.insert(action, bound);
            }
        }
        map
    }
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        // The map itself is attached to the player entity in `on_foot`.
        app.add_plugins(InputManagerPlugin::<Action>::default())
            .add_systems(Update, apply_keybindings.in_set(GameSet::Input));
    }
}

/// Rebuilds the player's input map whenever the settings menu changes a key
/// binding. Whole-map rebuild rather than an incremental patch: it is the
/// same construction `input_map` already does for the initial spawn, so
/// there is exactly one place that knows how a `KeyBindings` becomes an
/// `InputMap`.
fn apply_keybindings(
    keybindings: Res<KeyBindings>,
    mut maps: Query<&mut InputMap<Action>, With<crate::player::on_foot::Player>>,
) {
    if !keybindings.is_changed() {
        return;
    }
    for mut map in &mut maps {
        *map = Action::input_map(&keybindings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_is_either_played_or_deliberately_left_live() {
        // The exhaustive match is the point: a new action will not compile
        // until somebody has decided whether the tuning panel should be able
        // to take it away. Left to a bare list, the answer is always "nobody
        // thought about it", which is how a panel ends up with one key still
        // walking the player out of the street being tuned.
        for action in [
            Action::Move,
            Action::Look,
            Action::LookStick,
            Action::Jump,
            Action::Sprint,
            Action::Interact,
            Action::Taunt,
            Action::Cheer,
            Action::Apologize,
            Action::Handbrake,
            Action::Pause,
            Action::Map,
            Action::QuickSave,
            Action::QuickLoad,
            Action::ToggleDebugCamera,
            Action::ToggleTools,
        ] {
            let live = match action {
                Action::Pause | Action::ToggleDebugCamera | Action::ToggleTools => true,
                Action::Move
                | Action::Look
                | Action::LookStick
                | Action::Jump
                | Action::Sprint
                | Action::Interact
                | Action::Taunt
                | Action::Cheer
                | Action::Apologize
                | Action::Handbrake
                | Action::Map
                | Action::QuickSave
                | Action::QuickLoad => false,
            };
            assert_eq!(
                Action::IN_PLAY.contains(&action),
                !live,
                "{action:?} is on the wrong side of the panel"
            );
        }
    }

    #[test]
    fn every_action_the_panel_suppresses_is_actually_bound_to_something() {
        let map = Action::default_input_map();
        for action in Action::IN_PLAY {
            assert!(
                map.get(&action).is_some(),
                "{action:?} is suppressed but nothing presses it"
            );
        }
    }
}
