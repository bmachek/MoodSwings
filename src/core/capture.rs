//! Screenshot mode: render a few frames into an offscreen texture, save a PNG, exit.
//!
//! Motivation: "it compiled and didn't panic" is not evidence that the world
//! renders correctly. This gives every milestone a visual check that needs no
//! human at the keyboard, and doubles as a way to eyeball city generation from
//! a fixed vantage point when tuning it.
//!
//! It renders to an offscreen image rather than to the window on purpose:
//! capturing the window surface returns black whenever the OS has not actually
//! composited the window (backgrounded, occluded, or just never focused), which
//! makes window capture useless for unattended runs.
//!
//! Usage:
//!   cargo run -- --screenshot shots/city.png
//!   cargo run -- --screenshot shots/city.png --at 0,400,600 --look 0,0,0
//!   cargo run -- --screenshot shots/city.png --frames 120
//!   cargo run -- --screenshot shots/city.png --quality ultra --fps-log
//!   cargo run -- --screenshot shots/city.png --hour 21.5 --cover 1 --wet 0.9
//!   cargo run -- --screenshot shots/faces.png --at-node 300 --eye 1.4 --mood -1

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use avian3d::prelude::{ColliderDisabled, RigidBodyDisabled};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::time::Real;

use crate::player::camera::{CameraMode, CameraRig};
use crate::player::interact::{DrivenBy, Driving};
use crate::player::on_foot::Player;
use crate::render::quality::QualityPreset;
use crate::vehicle::controller::VehicleInput;
use crate::vehicle::spawn::Vehicle;

const CAPTURE_WIDTH: u32 = 1600;
const CAPTURE_HEIGHT: u32 = 900;

#[derive(Resource, Debug, Clone)]
pub struct CaptureRequest {
    pub path: PathBuf,
    /// Frames to render before capturing. Shadow maps, GPU culling and the
    /// streaming systems all need a few frames to settle first.
    pub warmup_frames: u32,
    pub eye: Option<Vec3>,
    pub look_at: Option<Vec3>,
    /// Overrides `world.stream_radius`, so an aerial shot can load more of the
    /// city than a player would ever have resident at once.
    pub stream_radius: Option<f32>,
    /// Stand at this road-graph intersection, looking down a connected street.
    /// Far easier than guessing coordinates that are not inside a building.
    pub node: Option<usize>,
    pub eye_height: f32,
    /// Freezes the clock at this hour, for checking the lighting cycle.
    pub hour: Option<f32>,
    /// Leaves the rig in follow mode, so the shot exercises the real
    /// third-person camera instead of a posed free camera.
    pub follow: bool,
    /// Opens the full-screen map.
    pub map: bool,
    /// Poses the camera three-quarters on to the nearest parked car.
    ///
    /// Bodywork is the one thing a street-level shot never shows properly: from
    /// the pavement a car is a silhouette, and from behind the wheels are
    /// hidden by its own bumper. Judging a body change needs this view.
    pub at_car: bool,
    /// Shears the nearest hydrant and frames the geyser. The only way to
    /// shoot the water: waiting for the traffic to find a hydrant is not a
    /// screenshot. Pair with `--frames` — the column needs a second or two
    /// of droplets in the air before it reads as a column.
    pub geyser: bool,
    /// Lines one of every archetype up down the street and shoots the row.
    /// The only way to compare bodywork without hunting the city for a pickup.
    pub showroom: bool,
    /// Stands one of every archetype in a row, the cast's answer to
    /// `--showroom`.
    ///
    /// The rare archetypes are the ones whose costume goes wrong, and they
    /// are exactly the ones a street framing cannot be relied on to contain:
    /// a wheelchair is four hundredths of the draw, so "shoot a street and
    /// hope" is not a check. This puts the whole cast on one pavement,
    /// standing still, in draw order.
    pub lineup: bool,
    /// Soaks the ground, 0 to 1.
    pub wetness: f32,
    /// Puts this much cloud over the city, 0 to 1. Above about seven tenths it
    /// also rains — which is the only way to shoot rain, now that rainfall comes
    /// out of the sky rather than out of the ground being wet.
    pub cover: f32,
    /// Which renderer tier to shoot at. The whole point of a preset ladder is
    /// being able to put two tiers side by side in the same framing, and that
    /// needs the choice on the command line rather than in a config file.
    pub quality: QualityPreset,
    /// Holds every face in the city at this mood, −1 to 1.
    ///
    /// The faces are the one thing in the game that cannot be shot by finding
    /// the right corner to stand on: a city in a mood is a city that has to be
    /// *put* in one first. This forces it, so the whole ladder of faces can be
    /// photographed from the same spot in three runs.
    pub mood: Option<f32>,
    /// Logs frame times over the warmup run alongside the capture.
    ///
    /// A screenshot proves a change looks right; it says nothing about whether
    /// it can be afforded. From the point geometry density starts moving, this
    /// is the other half of the evidence.
    pub fps_log: bool,
    /// Puts the player in the nearest car and holds the throttle down.
    /// An end-to-end smoke test of enter -> drive -> chase camera that needs
    /// nobody at the keyboard.
    pub drive: bool,
    /// Builds this `CityStyle` instead of the persisted one, so a style can
    /// be photographed without editing anybody's options file. Accepts the
    /// enum name or the label ("--city landshuepf", "--city Landshüpf").
    pub city: Option<crate::core::config::CityStyle>,
    /// Writes a numbered frame into this directory every so many frames, for
    /// as long as the run lasts, instead of one still.
    ///
    /// A still cannot be wrong about a shadow and cannot be wrong about
    /// anything that only happens while the game is *running*: a citizen
    /// walking through a wall, a car climbing a kerb it should have stopped
    /// at, a crowd all stepping off a kerb in formation, a shadow that swims,
    /// a chunk popping in. Those are the defects a posed frame is structurally
    /// unable to catch, and they are most of what is left once the still
    /// framings are clean.
    ///
    /// Paired with `--patrol`, which drives the player, this is the game
    /// playing itself with a camera running — and the frames assemble into a
    /// video with `ffmpeg`.
    pub film: Option<Film>,
}

/// Where a filmed run writes, and how often.
#[derive(Debug, Clone)]
pub struct Film {
    pub dir: PathBuf,
    /// One frame written every this many rendered.
    pub every: u32,
    /// And how many to write before quitting.
    pub count: u32,
}

#[derive(Resource)]
struct CaptureTarget(Handle<Image>);

#[derive(Resource)]
struct CaptureProgress {
    frame: u32,
    triggered: bool,
    saved: Arc<AtomicBool>,
    /// Frames written by a filmed run, and whether one is still being written.
    ///
    /// One at a time: `save_to_disk` encodes and writes a 1600x900 PNG inside
    /// its observer, which takes rather longer than a frame, and issuing the
    /// next screenshot before the last has landed simply queues encodes until
    /// the process runs out of memory. Waiting costs a filmed run its frame
    /// rate and nothing else — the clock is the game's, not the recorder's.
    filmed: u32,
    writing: Arc<AtomicBool>,
    /// Frame durations in milliseconds, oldest first.
    ///
    /// Kept whole rather than reduced to a running mean because the number that
    /// matters for a sixty-a-second budget is not the average, it is how bad
    /// the slow frames get — a mean hides exactly the stutter that is worth
    /// knowing about.
    frame_times: Vec<f32>,
}

/// True when the process was launched to take a screenshot. Used to strip the
/// dev UI so captures show the world and nothing else.
pub fn is_capture_mode() -> bool {
    std::env::args().any(|a| a == "--screenshot")
}

/// True when a capture asked for the map to be open.
///
/// Read off the command line rather than off the parsed [`CaptureRequest`],
/// for the same reason [`is_capture_mode`] is: the answer is wanted while the
/// plugins are being installed, which is before any resource exists.
pub fn wants_map() -> bool {
    std::env::args().any(|a| a == "--map")
}

/// Parses capture arguments. Returns `None` for a normal interactive run.
pub fn parse_args() -> Option<CaptureRequest> {
    let args: Vec<String> = std::env::args().collect();
    let idx = args.iter().position(|a| a == "--screenshot")?;
    let path = PathBuf::from(args.get(idx + 1)?);

    let value_of = |flag: &str| -> Option<String> {
        let i = args.iter().position(|a| a == flag)?;
        args.get(i + 1).cloned()
    };
    let vec3_of = |flag: &str| -> Option<Vec3> {
        let raw = value_of(flag)?;
        let parts: Vec<f32> = raw
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        match parts[..] {
            [x, y, z] => Some(Vec3::new(x, y, z)),
            _ => None,
        }
    };

    Some(CaptureRequest {
        path,
        warmup_frames: value_of("--frames")
            .and_then(|f| f.parse().ok())
            .unwrap_or(60),
        eye: vec3_of("--at"),
        look_at: vec3_of("--look"),
        stream_radius: value_of("--stream-radius").and_then(|v| v.parse().ok()),
        node: value_of("--at-node").and_then(|v| v.parse().ok()),
        eye_height: value_of("--eye")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.7),
        hour: value_of("--hour").and_then(|v| v.parse().ok()),
        at_car: args.iter().any(|a| a == "--at-car"),
        geyser: args.iter().any(|a| a == "--geyser"),
        showroom: args.iter().any(|a| a == "--showroom"),
        lineup: args.iter().any(|a| a == "--lineup"),
        wetness: value_of("--wet")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0),
        // A fair day unless asked otherwise. Deliberately *not* the seed's own
        // weather: every framing in the battery has to mean the same thing from
        // one run to the next, and "whatever the sky happened to be doing"
        // would make every shot an argument about the weather.
        cover: value_of("--cover")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.18),
        mood: value_of("--mood").and_then(|v| v.parse().ok()),
        quality: crate::render::preset_from_arg(value_of("--quality").as_deref()),
        fps_log: args.iter().any(|a| a == "--fps-log"),
        follow: args.iter().any(|a| a == "--follow"),
        drive: args.iter().any(|a| a == "--drive"),
        map: args.iter().any(|a| a == "--map"),
        film: value_of("--film").map(|dir| Film {
            dir: PathBuf::from(dir),
            every: value_of("--film-every")
                .and_then(|v| v.parse().ok())
                .unwrap_or(6),
            count: value_of("--film-frames")
                .and_then(|v| v.parse().ok())
                .unwrap_or(240),
        }),
        city: value_of("--city").map(|name| {
            crate::core::config::CityStyle::ALL
                .into_iter()
                .find(|style| {
                    style.label().eq_ignore_ascii_case(&name)
                        || format!("{style:?}").eq_ignore_ascii_case(&name)
                })
                .unwrap_or_else(|| panic!("--city {name}: no such style"))
        }),
    })
}

pub struct CapturePlugin;

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        let Some(request) = parse_args() else {
            return;
        };

        if let Some(parent) = request.path.parent()
            && !parent.as_os_str().is_empty()
        {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Some(film) = &request.film {
            let _ = std::fs::create_dir_all(&film.dir);
        }

        app.insert_resource(request)
            .insert_resource(CaptureProgress {
                frame: 0,
                triggered: false,
                saved: Arc::new(AtomicBool::new(false)),
                filmed: 0,
                writing: Arc::new(AtomicBool::new(false)),
                frame_times: Vec::new(),
            })
            .add_systems(PreStartup, apply_capture_overrides)
            .add_systems(PostStartup, retarget_camera_offscreen)
            .add_systems(
                Update,
                (pose_at_car, stage_geyser, line_up_showroom, line_up_cast),
            )
            .add_systems(
                FixedUpdate,
                autodrive.before(crate::vehicle::controller::drive_vehicles),
            )
            .add_systems(Last, drive_capture);
    }
}

fn apply_capture_overrides(
    request: Res<CaptureRequest>,
    mut config: ResMut<crate::core::config::GameConfig>,
    mut map_open: ResMut<crate::ui::minimap::MapOpen>,
) {
    config.graphics = request.quality.settings();

    if let Some(radius) = request.stream_radius {
        config.world.stream_radius = radius;
    }
    if request.map {
        map_open.0 = true;
    }
    config.world.start_wetness = request.wetness;
    config.world.start_cover = request.cover;
    if let Some(city) = request.city {
        config.city = city;
    }
    if let Some(hour) = request.hour {
        config.world.start_hour = hour;
        // Freeze it, so the warmup frames do not drift the sky. Weather runs on
        // the same clock, so this holds the cloud and the wetness with it.
        config.world.day_length_seconds = 0.0;
    }
}

/// Points the debug camera at an offscreen texture and applies the requested pose.
fn retarget_camera_offscreen(
    mut commands: Commands,
    request: Res<CaptureRequest>,
    mut images: ResMut<Assets<Image>>,
    city: Option<Res<crate::world::City>>,
    mut cameras: Query<(&mut RenderTarget, &mut Transform, &mut CameraRig), With<Camera>>,
) {
    let size = Extent3d {
        width: CAPTURE_WIDTH,
        height: CAPTURE_HEIGHT,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(image);

    for (mut render_target, mut transform, mut rig) in &mut cameras {
        *render_target = RenderTarget::Image(ImageRenderTarget {
            handle: handle.clone(),
            scale_factor: 1.0,
        });

        if request.follow {
            // Let the real camera do its job; only nudge the orbit angle.
            rig.mode = CameraMode::Follow;
            if let Some(eye) = request.eye {
                rig.yaw = eye.x;
                rig.pitch = eye.y;
            }
            continue;
        }
        // Detached: otherwise the follow system drags the camera back to the
        // player the frame after the requested pose is applied.
        rig.mode = CameraMode::Free;

        // --at-node wins over --at, since it is the more specific request.
        let posed = request.node.zip(city.as_ref()).and_then(|(index, city)| {
            let graph = &city.graph;
            let id = crate::world::roadgraph::NodeId(index as u32 % graph.node_count() as u32);
            let here = graph.node(id).pos;
            let (down_street, _) = graph.neighbors(id).next()?;
            let there = graph.node(down_street).pos;
            Some((
                Vec3::new(here.x, request.eye_height, here.y),
                Vec3::new(there.x, request.eye_height, there.y),
            ))
        });

        if let Some((eye, target)) = posed {
            *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
            let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
            rig.yaw = yaw;
            rig.pitch = pitch;
        } else if let Some(eye) = request.eye {
            let target = request.look_at.unwrap_or(Vec3::ZERO);
            *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
            // Keep the controller's own angles in sync so it does not snap back.
            let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
            rig.yaw = yaw;
            rig.pitch = pitch;
        }
    }

    commands.insert_resource(CaptureTarget(handle));
}

fn drive_capture(
    mut commands: Commands,
    request: Res<CaptureRequest>,
    target: Res<CaptureTarget>,
    mut progress: ResMut<CaptureProgress>,
    time: Res<Time<Real>>,
    mut exit: MessageWriter<AppExit>,
    drawables: Query<(), With<Mesh3d>>,
    cameras: Query<&Transform, With<CameraRig>>,
    subjects: Query<&Transform, With<Player>>,
) {
    progress.frame += 1;
    if request.fps_log && !progress.triggered {
        // `Time<Real>`, not the default virtual clock: that one clamps its
        // delta at 250 ms so a long frame cannot make the simulation take a
        // huge step. Perfectly correct for gameplay, and exactly wrong here —
        // it saturates on precisely the slow frames a budget is decided by, and
        // reports them all as an identical 250.00 ms.
        progress.frame_times.push(time.delta_secs() * 1000.0);
    }

    // A filmed run writes frames for as long as it lasts and never takes the
    // single still. It is the same offscreen target and the same encoder; what
    // changes is that nothing is posed and nothing is frozen, so what lands on
    // disk is the game as it actually runs.
    if let Some(film) = &request.film {
        if progress.frame < request.warmup_frames {
            return;
        }
        if progress.filmed >= film.count {
            info!(
                "film complete: {} frames in {}",
                progress.filmed,
                film.dir.display()
            );
            exit.write(AppExit::Success);
            return;
        }
        if !progress.frame.is_multiple_of(film.every.max(1)) {
            return;
        }
        if progress.writing.load(Ordering::SeqCst) {
            return;
        }
        let path = film.dir.join(format!("{:05}.png", progress.filmed));
        progress.filmed += 1;
        progress.writing.store(true, Ordering::SeqCst);
        let done = progress.writing.clone();
        commands
            .spawn(Screenshot::image(target.0.clone()))
            .observe(save_to_disk(path))
            .observe(move |_: On<ScreenshotCaptured>| done.store(false, Ordering::SeqCst));
        return;
    }

    if !progress.triggered && progress.frame >= request.warmup_frames {
        progress.triggered = true;

        if request.fps_log {
            info!("{}", frame_time_summary(&progress.frame_times));
        }

        // Logged so a blank image can be told apart from an empty scene.
        let camera = cameras
            .single()
            .map(|t| format!("{:?}", t.translation))
            .unwrap_or_else(|_| "<none>".into());
        let player = subjects
            .single()
            .map(|t| format!("{:?}", t.translation))
            .unwrap_or_else(|_| "<none>".into());
        info!(
            "capturing: {} meshes, camera at {}, player at {}",
            drawables.iter().count(),
            camera,
            player
        );

        let flag = progress.saved.clone();
        commands
            .spawn(Screenshot::image(target.0.clone()))
            .observe(save_to_disk(request.path.clone()))
            .observe(move |_: On<ScreenshotCaptured>| flag.store(true, Ordering::SeqCst));
        return;
    }

    // `save_to_disk` writes synchronously inside its observer, so once the flag
    // is set the file is already on disk and it is safe to quit.
    if progress.saved.load(Ordering::SeqCst) {
        info!("capture complete: {}", request.path.display());
        exit.write(AppExit::Success);
    }
}

/// Drives the player into the nearest car and floors it.
///
/// Deliberately bypasses the normal interact range check: this is a smoke test
/// that the enter -> drive -> chase-camera path works, not a test of how close
/// you have to stand to a door.
fn autodrive(
    mut commands: Commands,
    request: Res<CaptureRequest>,
    players: Query<(Entity, &Transform, Option<&Driving>), With<Player>>,
    parked: Query<(Entity, &Transform), (With<Vehicle>, Without<DrivenBy>)>,
    mut inputs: Query<&mut VehicleInput>,
    states: Query<(&Transform, &crate::vehicle::controller::VehicleState)>,
    mut ticks: Local<u32>,
) {
    if !request.drive {
        return;
    }
    *ticks += 1;
    let Ok((player, transform, driving)) = players.single() else {
        return;
    };

    let Some(Driving(vehicle)) = driving else {
        // Let the city populate first. Stealing a car in the opening frames is
        // genuinely unwitnessed, which is correct behaviour but tests nothing.
        if *ticks < 200 {
            return;
        }
        let nearest = parked
            .iter()
            .map(|(entity, other)| (entity, other.translation.distance(transform.translation)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((vehicle, distance)) = nearest {
            info!("autodrive: taking a car {distance:.1}m away");
            commands.entity(player).insert((
                Driving(vehicle),
                RigidBodyDisabled,
                ColliderDisabled,
                Visibility::Hidden,
            ));
            commands.entity(vehicle).insert(DrivenBy(player));
        }
        return;
    };

    if let Ok(mut input) = inputs.get_mut(*vehicle) {
        // Flee, then stop. Driving away forever only demonstrates the escape
        // half of the loop; pulling up lets the pursuit catch up and exercises
        // sighting, chasing and re-escalation too.
        //
        // Deliberately not routed to the mission marker: doing that well needs
        // a competent autonomous driver, and a half-competent one just wrecks
        // the car against a building. Mission completion is covered by tests in
        // `mission` instead.
        let fleeing = *ticks < 700;
        input.throttle = if fleeing { 1.0 } else { -1.0 };
        input.steer = 0.0;
        input.handbrake = !fleeing;
    }

    if (*ticks).is_multiple_of(48)
        && let Ok((_, state)) = states.get(*vehicle)
    {
        info!(
            "t={:>4} {:>5.1} km/h  wheels {}/4",
            *ticks,
            state.speed_kph(),
            state.grounded_wheels(),
        );
    }
}

/// Frames the nearest parked car, once the parked cars exist.
///
/// Deferred to `Update` rather than done with the rest of the pose because
/// `spawn_parked_vehicles` runs in `PostStartup` alongside it, and there is no
/// ordering between them worth asserting for a debug flag.
/// Breaks the nearest hydrant and frames the resulting geyser.
///
/// Deferred to `Update` for the same reason `pose_at_car` is: the props do
/// not exist until streaming has run. The shear itself goes through the same
/// `mayhem::shear` the traffic uses, so the shot shows the real thing.
fn stage_geyser(
    request: Res<CaptureRequest>,
    mut done: Local<bool>,
    mut commands: Commands,
    mut sheared: MessageWriter<crate::world::mayhem::PropSheared>,
    hydrants: Query<(
        Entity,
        &Transform,
        &crate::world::mayhem::Breakaway,
        Option<&crate::world::buildings::ChunkOf>,
    )>,
    mut cameras: Query<(&mut Transform, &mut CameraRig), Without<crate::world::mayhem::Breakaway>>,
) {
    if *done || !request.geyser {
        return;
    }
    let Some((entity, stump, breakaway, chunk)) = hydrants
        .iter()
        .filter(|(_, _, breakaway, _)| breakaway.geyser)
        .min_by(|a, b| {
            a.1.translation
                .length_squared()
                .total_cmp(&b.1.translation.length_squared())
        })
        .map(|(entity, transform, breakaway, chunk)| (entity, *transform, *breakaway, chunk))
    else {
        return;
    };

    // Hit as if by a car doing 40 km/h along the street.
    crate::world::mayhem::shear(
        &mut commands,
        &mut sheared,
        entity,
        &stump,
        &breakaway,
        chunk,
        Vec3::new(11.0, 0.0, 0.0),
        Vec3::Z,
    );

    // Framed from across the street, high enough to see the top of the
    // column against the facade rather than against the sky.
    let eye = stump.translation + Vec3::new(7.5, 3.2, 6.0);
    let target = stump.translation + Vec3::Y * 3.5;
    for (mut transform, mut rig) in &mut cameras {
        rig.mode = CameraMode::Free;
        *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
        let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        rig.yaw = yaw;
        rig.pitch = pitch;
    }
    *done = true;
    info!(
        "sheared a hydrant for the camera at {:?}",
        stump.translation
    );
}

fn pose_at_car(
    request: Res<CaptureRequest>,
    mut done: Local<bool>,
    vehicles: Query<&Transform, (With<Vehicle>, Without<CameraRig>)>,
    mut cameras: Query<(&mut Transform, &mut CameraRig)>,
) {
    if *done || !request.at_car {
        return;
    }
    let nearest = vehicles.iter().min_by(|a, b| {
        a.translation
            .length_squared()
            .total_cmp(&b.translation.length_squared())
    });
    let Some(car) = nearest else {
        return;
    };
    let car = *car;

    // Off the front three-quarter, at about eye height for someone standing
    // beside it: the angle every car photograph is taken from, because it shows
    // the nose, one flank and both wheels on that side at once.
    // Far enough out that the lens is not adding drama of its own: from three
    // metres a nose fills the frame and every proportion is a lie.
    let eye = car.transform_point(Vec3::new(5.2, 1.75, -6.8));
    let target = car.translation + Vec3::Y * 0.35;

    for (mut transform, mut rig) in &mut cameras {
        rig.mode = CameraMode::Free;
        *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
        let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        rig.yaw = yaw;
        rig.pitch = pitch;
    }
    *done = true;
}

/// Parks one of every archetype in a row and frames them.
///
/// Anchored to the car the world guarantees at the player's start, because that
/// is a spot the generator has already established is a street rather than the
/// inside of a building.
fn line_up_showroom(
    mut commands: Commands,
    request: Res<CaptureRequest>,
    mut done: Local<bool>,
    assets: Option<Res<crate::vehicle::spawn::VehicleAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    vehicles: Query<&Transform, (With<Vehicle>, Without<CameraRig>)>,
    mut cameras: Query<(&mut Transform, &mut CameraRig)>,
) {
    if *done || !request.showroom {
        return;
    }
    let (Some(assets), Some(anchor)) = (
        assets,
        vehicles
            .iter()
            .min_by(|a, b| {
                a.translation
                    .length_squared()
                    .total_cmp(&b.translation.length_squared())
            })
            .copied(),
    ) else {
        return;
    };

    let classes = crate::vehicle::spec::VehicleClass::ALL;
    let spacing = 6.2;
    for (i, class) in classes.iter().enumerate() {
        let spec = class.spec();
        let along = anchor.forward() * (i as f32 * spacing);
        let at = anchor.translation + along;
        let transform =
            Transform::from_xyz(at.x, crate::vehicle::spawn::resting_height(&spec), at.z)
                .with_rotation(anchor.rotation);
        crate::vehicle::spawn::spawn_vehicle(
            &mut commands,
            &assets,
            &mut materials,
            spec,
            transform,
        );
    }

    // Off to one side and slightly up, far enough back that the row is not all
    // perspective.
    let middle = anchor.translation + anchor.forward() * (classes.len() as f32 * spacing * 0.5);
    let eye = middle + *anchor.right() * 15.0 + Vec3::Y * 6.0 - anchor.forward() * 6.0;

    for (mut transform, mut rig) in &mut cameras {
        rig.mode = CameraMode::Free;
        *transform = Transform::from_translation(eye).looking_at(middle + Vec3::Y * 0.4, Vec3::Y);
        let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        rig.yaw = yaw;
        rig.pitch = pitch;
    }
    *done = true;
}

/// Stands one of every archetype in a row and points the camera down it.
///
/// The cast's `--showroom`. Deliberately *not* the pedestrian spawner: these
/// stand still, in a known order, so that two runs of this framing differ only
/// by the change being judged. They are dressed by the same `figure::dress`
/// the crowd uses, which is the whole point — a costume bug shows up here
/// because this is the same costume.
fn line_up_cast(
    mut commands: Commands,
    request: Res<CaptureRequest>,
    mut done: Local<bool>,
    figures: Option<Res<crate::ai::figure::FigureAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    players: Query<&Transform, With<Player>>,
    mut cameras: Query<(&mut Transform, &mut CameraRig), Without<Player>>,
) {
    use crate::ai::archetype::Archetype;

    if *done || !request.lineup {
        return;
    }
    let (Some(figures), Ok(anchor)) = (figures, players.single()) else {
        return;
    };

    // Their own stream, keyed off nothing: a lineup is a fixture, not part of
    // the world, and drawing from a real stream would move the city.
    let mut rng = crate::core::rng::stream_for(0, crate::core::rng::stream::CROWD);
    let spacing = 1.6;
    let right = *anchor.right();
    let along = *anchor.forward();
    // Above the roofline, not on the pavement where the player happens to
    // stand. Sixteen archetypes at 1.6m is a row twenty-four metres wide, and
    // photographing a row that wide needs twenty metres of clear standoff — a
    // thing an old town does not have anywhere. Every lineup ever shot here
    // was therefore a photograph of the inside of a wall, including the ones
    // used to sign off costume work. The fixture is not part of the world, so
    // it can stand where there is room: the town's own light and sky, a
    // guaranteed sight line, and nothing in the frame that is not the cast.
    // Well clear of the height cap (23m in Landshüpf) and of the gables on
    // top of it, though not of a cathedral spire — which is why it also steps
    // sideways, away from whatever the player was standing next to.
    let stage = anchor.translation + Vec3::Y * 42.0 + right * 30.0;

    // Six to a row rather than all sixteen in one. A frame wide enough to hold
    // a twenty-four metre line puts every face under a hundred pixels, which
    // photographs a crowd and not a costume. Three shorter rows, each staggered
    // half a place so nobody stands directly behind anybody, cost a little
    // depth and return about two and a half times the figure. The arithmetic
    // is written off `ALL.len()` throughout so a seventeenth archetype lands
    // in frame rather than off the end of it.
    const PER_ROW: usize = 6;
    const ROW_DEPTH: f32 = 3.2;
    let rows = Archetype::ALL.len().div_ceil(PER_ROW);
    for (i, archetype) in Archetype::ALL.iter().enumerate() {
        let (row, column) = (i / PER_ROW, i % PER_ROW);
        // The last row is usually short, and centring it on a full row's width
        // would hang it off one side.
        let wide = PER_ROW.min(Archetype::ALL.len() - row * PER_ROW);
        let middle_index = (wide as f32 - 1.0) * 0.5;
        let stagger = if row % 2 == 1 { spacing * 0.5 } else { 0.0 };
        let at = stage
            + along * (6.0 + row as f32 * ROW_DEPTH)
            + right * ((column as f32 - middle_index) * spacing + stagger);
        let level = crate::mood::face::level_of(0.0);
        let coat = materials.add(StandardMaterial {
            // The fixed wardrobe where the archetype has one, and one neutral
            // street coat where it does not — the street palette is the
            // pedestrian spawner's, and reaching into it would be reaching
            // across a module to make a fixture look prettier.
            base_color: archetype.coat().unwrap_or(Color::srgb(0.42, 0.44, 0.48)),
            perceptual_roughness: 0.85,
            ..default()
        });
        let mut person = commands.spawn((
            Name::new("Lineup"),
            *archetype,
            crate::mood::feeling::Mood::new(0.0),
            crate::mood::face::FaceLevel(level),
            // Facing the camera, which stands where the player does.
            Transform::from_translation(at)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI) * anchor.rotation),
            Visibility::default(),
        ));
        crate::ai::figure::dress(&mut person, &figures, coat, level, *archetype, &mut rng);
    }

    // Three-quarters rather than dead ahead, and above head height. Half
    // this cast's costume is at ground level and edge-on — a board, a cane,
    // a pair of wheels — and a frontal shot is the one angle from which
    // none of it can be told apart.
    // Backed off proportionally to the row's actual width, so the frame
    // keeps holding the whole cast as it grows.
    let span = (PER_ROW as f32 - 1.0) * spacing;
    let depth = (rows as f32 - 1.0) * ROW_DEPTH;
    let middle = stage + along * (6.0 + depth * 0.5);
    let eye = middle - along * (span * 0.65 + depth * 0.5) + right * (span * 0.36) + Vec3::Y * 2.2;
    for (mut transform, mut rig) in &mut cameras {
        rig.mode = CameraMode::Free;
        *transform = Transform::from_translation(eye).looking_at(middle + Vec3::Y * 0.75, Vec3::Y);
        let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        rig.yaw = yaw;
        rig.pitch = pitch;
    }
    *done = true;
}

/// Reduces a warmup run's frame times to the three numbers worth reporting.
///
/// The first frames of any run are pipeline compilation and streaming, not
/// rendering, and including them would make every measurement look terrible
/// regardless of the change being judged — so the opening quarter is dropped.
/// What is left is reported as a median and a 95th percentile, because a budget
/// is kept or missed by the slow frames rather than by the typical one.
fn frame_time_summary(samples: &[f32]) -> String {
    // A handful of frames is not a measurement, and a percentile over three
    // samples is arithmetic rather than evidence. Say so instead.
    const MIN_SAMPLES: usize = 8;
    if samples.len() < MIN_SAMPLES {
        return "frame times: too few frames to report".into();
    }

    let mut sorted: Vec<f32> = samples[samples.len() / 4..].to_vec();
    sorted.sort_by(f32::total_cmp);
    let at = |fraction: f32| {
        let last = sorted.len() - 1;
        sorted[((last as f32) * fraction).round() as usize]
    };

    let median = at(0.5);
    let p95 = at(0.95);
    let worst = sorted[sorted.len() - 1];
    format!(
        "frame times over {} frames: median {median:.2} ms ({:.0} fps), p95 {p95:.2} ms, worst {worst:.2} ms",
        sorted.len(),
        1000.0 / median.max(f32::EPSILON),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_opening_frames_are_left_out_of_the_summary() {
        // Four catastrophic startup frames, then twelve good ones. A mean over
        // the lot would read 30 ms; the number that matters reads 10.
        let mut samples = vec![200.0; 4];
        samples.extend(std::iter::repeat_n(10.0, 12));

        let summary = frame_time_summary(&samples);
        assert!(summary.contains("median 10.00 ms"), "{summary}");
        assert!(summary.contains("over 12 frames"), "{summary}");
    }

    #[test]
    fn the_slow_frames_are_reported_rather_than_averaged_away() {
        let mut samples = vec![8.0; 96];
        samples[90] = 42.0;

        let summary = frame_time_summary(&samples);
        assert!(summary.contains("worst 42.00 ms"), "{summary}");
        assert!(summary.contains("median 8.00 ms"), "{summary}");
    }

    #[test]
    fn a_run_too_short_to_measure_says_so_rather_than_dividing_by_zero() {
        assert!(frame_time_summary(&[]).contains("too few"));
        assert!(frame_time_summary(&[16.0]).contains("too few"));
    }

    /// The headline is frames per second, and getting the reciprocal backwards
    /// is the kind of thing that survives review because both numbers look
    /// plausible.
    #[test]
    fn the_frame_rate_is_the_reciprocal_of_the_median() {
        let summary = frame_time_summary(&[16.666_667; 40]);
        assert!(summary.contains("60 fps"), "{summary}");
    }
}
