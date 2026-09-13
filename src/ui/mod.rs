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
        // left and a face over its top right.
        //
        // `--map` asks for the minimap by name. What it actually gets is the
        // *camera* and not the widget: the HUD is what displays the render
        // target, and the HUD is held back with everything else here, so a
        // shot taken with `--map` shows no map. Worth knowing rather than
        // worth removing — it makes the flag an exact A/B for what the second
        // view costs, with nothing else changing between the two runs, which
        // is the one measurement README.md's frame-time section is still
        // missing. A shot that genuinely wants a map drawn in it needs the HUD
        // as well, and that is a bigger decision than this line: it would put
        // a speedometer and a clock in the frame too.
        if crate::core::capture::is_capture_mode() {
            if crate::core::capture::wants_map() {
                app.add_plugins(minimap::MinimapPlugin);
            }
        } else {
            app.add_plugins((minimap::MinimapPlugin, hud::HudPlugin, debug::DebugUiPlugin));
        }
    }
}
