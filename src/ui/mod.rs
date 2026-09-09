//! HUD, minimap, menus, and developer tooling.

pub mod debug;
pub mod hud;
pub mod menu;
pub mod minimap;

use bevy::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(menu::MenuPlugin);

        // Screenshots should show the game, not the instruments. The tuning
        // panel was already held back; the mood panel and the minimap were not,
        // and they sit in the two corners a framing is most likely to want —
        // every comparison shot in `shots/` carries a minimap over its bottom
        // left and a face over its top right. `--map` still asks for the
        // minimap by name, because a shot *of* the map is a thing to want.
        if crate::core::capture::is_capture_mode() {
            if crate::core::capture::wants_map() {
                app.add_plugins(minimap::MinimapPlugin);
            }
        } else {
            app.add_plugins((minimap::MinimapPlugin, hud::HudPlugin, debug::DebugUiPlugin));
        }
    }
}
