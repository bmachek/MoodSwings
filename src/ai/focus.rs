//! Where the simulation thinks the player is.
//!
//! Three different systems used to answer this question three different ways.
//! Chunk streaming reads the camera rig, because what has to be built is what
//! can be *seen*. The crowd, the traffic and the cyclists all read the
//! `Player`'s transform, because that is who they are keeping company. Those
//! are the same point while somebody is walking around, and they are two
//! kilometres apart the moment the camera leaves — in the free camera, and in
//! every single screenshot the capture harness takes, which poses the camera
//! and leaves the player wherever it spawned.
//!
//! That is why `tools/shoot.sh` produces street framings with no traffic and no
//! crowd on them: the world was built around the lens and the life was kept
//! around somebody standing in a different postcode. A change to the city's
//! liveliness that cannot be seen in a screenshot cannot be judged at all.
//!
//! So there is one focus, written once a frame, preferring the camera and
//! falling back to the player.

use bevy::prelude::*;

use crate::core::schedule::GameSet;

/// The point everything ambient is kept around.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct SimFocus(pub Vec3);

impl SimFocus {
    /// On the ground plane, which is what every population ring measures in.
    pub fn ground(&self) -> Vec2 {
        self.0.xz()
    }
}

pub struct FocusPlugin;

impl Plugin for FocusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimFocus>()
            // Ahead of everything in `Ai` that reads it, and outside the
            // `InGame` gate so a focus exists before the first frame of play.
            .add_systems(Update, follow_the_lens.before(GameSet::Ai));
    }
}

fn follow_the_lens(
    mut focus: ResMut<SimFocus>,
    cameras: Query<&GlobalTransform, With<crate::player::camera::CameraRig>>,
    players: Query<&Transform, With<crate::player::on_foot::Player>>,
) {
    if let Ok(camera) = cameras.single() {
        focus.0 = camera.translation();
    } else if let Ok(player) = players.single() {
        focus.0 = player.translation;
    }
}
