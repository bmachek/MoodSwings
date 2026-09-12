//! Shared exploration. Remote figures are visual replicas, never local AI or
//! physics bodies. The network thread stays alive during loading and pause.
use crate::{
    core::config::{CityStyle, GameConfig},
    player::on_foot::Player,
};
use bevy::prelude::*;
use mood_multiplayer::{
    ClientMessage, Connection, Pose, ServerMessage, TICK, TIMEOUT, VERSION, World,
};
use std::{
    net::{TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

#[derive(Default)]
struct Shared {
    local: Option<Pose>,
    players: Vec<mood_multiplayer::Player>,
    hour: f32,
    error: Option<String>,
    actors: Vec<mood_multiplayer::Actor>,
}
#[derive(Resource)]
pub struct Client {
    id: u64,
    world: World,
    shared: Arc<Mutex<Shared>>,
}
impl Client {
    pub fn requested() -> Result<Option<Self>, String> {
        let args: Vec<_> = std::env::args().collect();
        let Some(index) = args.iter().position(|a| a == "--connect") else {
            return Ok(None);
        };
        let address = args
            .get(index + 1)
            .filter(|a| !a.starts_with("--"))
            .ok_or("--connect braucht HOST:PORT")?;
        let name = match args.iter().position(|a| a == "--name") {
            Some(i) => args
                .get(i + 1)
                .ok_or("--name braucht einen Spielernamen")?
                .clone(),
            None => "Flummi".into(),
        };
        if !mood_multiplayer::valid_name(&name) {
            return Err("Spielername: 1–24 Zeichen, keine Steuerzeichen".into());
        }
        for flag in [
            "--city",
            "--hour",
            "--cover",
            "--wet",
            "--survey",
            "--audition",
        ] {
            if args.iter().any(|a| a == flag) {
                return Err(format!("{flag} lässt sich nicht mit --connect kombinieren"));
            }
        }
        let result = (|| -> std::io::Result<Self> {
            let addresses: Vec<_> = address.to_socket_addrs()?.collect();
            let mut stream = None;
            for addr in addresses {
                if let Ok(socket) = TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                    stream = Some(socket);
                    break;
                }
            }
            let mut connection = Connection::new(
                stream.ok_or_else(|| mood_multiplayer::invalid("Server nicht erreichbar"))?,
            )?;
            connection.send(&ClientMessage::Hello {
                version: VERSION,
                name,
            })?;
            let start = Instant::now();
            let (id, world) = 'handshake: loop {
                for message in connection.receive::<ServerMessage>()? {
                    if let ServerMessage::Welcome { version, id, world } = message {
                        if version != VERSION || !world.valid() {
                            return Err(mood_multiplayer::invalid("Inkompatibler Server"));
                        }
                        break 'handshake (id, world);
                    }
                }
                if start.elapsed() > Duration::from_secs(5) {
                    return Err(mood_multiplayer::invalid("Keine Antwort vom Server"));
                }
                connection.flush()?;
                std::thread::sleep(Duration::from_millis(10));
            };
            let shared = Arc::new(Mutex::new(Shared {
                hour: world.hour,
                ..Default::default()
            }));
            // A weak reference makes app shutdown stop the worker and close TCP.
            let weak = Arc::downgrade(&shared);
            std::thread::Builder::new()
                .name("multiplayer".into())
                .spawn(move || {
                    let mut heard = Instant::now();
                    while let Some(shared) = weak.upgrade() {
                        let result = (|| -> std::io::Result<()> {
                            let local = shared.lock().unwrap().local.clone();
                            connection.send(&ClientMessage::Update(local))?;
                            for message in connection.receive::<ServerMessage>()? {
                                if let ServerMessage::Snapshot {
                                    hour,
                                    players,
                                    actors,
                                } = message
                                {
                                    if !hour.is_finite()
                                        || !(0.0..24.0).contains(&hour)
                                        || players.len() > mood_multiplayer::MAX_PLAYERS
                                        || actors.len() > 4096
                                        || players.iter().any(|p| {
                                            !p.pose.valid()
                                                || !mood_multiplayer::valid_name(&p.name)
                                        })
                                        || actors.iter().any(|a| {
                                            !a.position
                                                .iter()
                                                .all(|x| x.is_finite() && x.abs() < 100_000.0)
                                                || !a
                                                    .velocity
                                                    .iter()
                                                    .all(|x| x.is_finite() && x.abs() < 100.0)
                                                || !a.mood.is_finite()
                                        })
                                    {
                                        return Err(mood_multiplayer::invalid(
                                            "Ungültiger Serverzustand",
                                        ));
                                    }
                                    let mut data = shared.lock().unwrap();
                                    data.hour = hour;
                                    data.players = players;
                                    data.actors = actors;
                                    heard = Instant::now();
                                }
                            }
                            if heard.elapsed() > TIMEOUT {
                                return Err(mood_multiplayer::invalid(
                                    "Server antwortet nicht mehr",
                                ));
                            }
                            Ok(())
                        })();
                        if let Err(error) = result {
                            let mut data = shared.lock().unwrap();
                            data.error = Some(error.to_string());
                            data.players.clear();
                            break;
                        }
                        drop(shared);
                        std::thread::sleep(TICK);
                    }
                })?;
            Ok(Self { id, world, shared })
        })()
        .map_err(|e| format!("Multiplayer-Verbindung fehlgeschlagen: {e}"))?;
        ACTIVE.store(true, Ordering::Relaxed);
        Ok(Some(result))
    }
}

pub struct MultiplayerPlugin(pub Option<Client>);
// Plugins are borrowed by Bevy; the socket worker state is cheaply shared.
impl Plugin for MultiplayerPlugin {
    fn build(&self, app: &mut App) {
        let Some(client) = &self.0 else { return };
        app.insert_resource(Client {
            id: client.id,
            world: client.world.clone(),
            shared: client.shared.clone(),
        })
        .add_systems(
            PreStartup,
            configure.after(crate::core::capture::apply_capture_overrides),
        )
        .add_systems(Startup, status_label)
        .add_systems(PreUpdate, receive)
        .add_systems(PostUpdate, (publish, paint_remote_faces));
    }
}
fn configure(client: Res<Client>, mut config: ResMut<GameConfig>) {
    // Local generation tunables must not change the layout of a shared town.
    // Keep presentation and controls, take world generation from code defaults.
    let stream_radius = config.world.stream_radius;
    config.world = GameConfig::default().world;
    config.world.stream_radius = stream_radius;
    config.world_seed = client.world.seed;
    config.city = CityStyle::ALL
        .into_iter()
        .find(|s| format!("{s:?}") == client.world.city)
        .unwrap();
    config.world.start_hour = client.world.hour;
    config.world.day_length_seconds = client.world.day_seconds;
}
#[derive(Component)]
struct Remote {
    id: u64,
    character: String,
}
#[derive(Component)]
struct RemoteActor(u64);
#[derive(Component)]
struct Status;
fn status_label(mut commands: Commands) {
    commands.spawn((
        Status,
        Text::new("Multiplayer verbunden"),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(14.0),
            left: Val::Px(16.0),
            ..default()
        },
    ));
}
fn receive(
    client: Res<Client>,
    time: Res<Time>,
    mut commands: Commands,
    mut clock: ResMut<crate::world::timeofday::TimeOfDay>,
    mut locals: Query<&mut Transform, With<Player>>,
    mut remotes: Query<(
        Entity,
        &Remote,
        &mut Transform,
        &mut crate::ai::figure::WalkCycle,
    )>,
    mut remotes_actor: Query<(Entity, &RemoteActor, &mut Transform)>,
    mut status: Query<&mut Text, With<Status>>,
    figures: Res<crate::ai::figure::FigureAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut coat: Local<Option<Handle<StandardMaterial>>>,
    mut actor_mesh: Local<Option<Handle<Mesh>>>,
    mut actor_materials: Local<Option<(Handle<StandardMaterial>, Handle<StandardMaterial>)>>,
) {
    let data = client.shared.lock().unwrap();
    clock.hours = data.hour;
    if let Ok(mut text) = status.single_mut() {
        text.0 = if data.error.is_some() {
            "Verbindung getrennt - Zum Verbinden neu starten".to_string()
        } else {
            format!(
                "Multiplayer: {} Spieler, {} NPC/Fahrzeuge | Du bist #{}",
                data.players.len(),
                data.actors.len(),
                client.id
            )
        };
    }
    for (entity, remote, mut transform, mut walk) in &mut remotes {
        let Some(player) = data
            .players
            .iter()
            .find(|p| p.id == remote.id && p.pose.character == remote.character)
        else {
            commands.entity(entity).despawn();
            continue;
        };
        let target = Vec3::from_array(player.pose.position);
        let distance = transform.translation.distance(target);
        walk.speed = (distance / TICK.as_secs_f32()).min(15.0);
        let blend = if distance > 30.0 {
            1.0
        } else {
            1.0 - (-15.0 * time.delta_secs()).exp()
        };
        transform.translation = transform.translation.lerp(target, blend);
        transform.rotation = transform
            .rotation
            .slerp(Quat::from_array(player.pose.rotation).normalize(), blend);
    }
    // Reconcile the local body against the authoritative pose. A small blend
    // hides packet jitter; a large correction prevents a client from drifting
    // indefinitely when its local frame rate differs from the server tick.
    if let Some(player) = data.players.iter().find(|p| p.id == client.id)
        && let Ok(mut local) = locals.single_mut()
    {
        let target = Vec3::from_array(player.pose.position);
        let error = local.translation.distance(target);
        local.translation = local
            .translation
            .lerp(target, (error * 0.15).clamp(0.05, 1.0));
    }
    for player in data.players.iter().filter(|p| p.id != client.id) {
        if remotes
            .iter()
            .any(|(_, r, _, _)| r.id == player.id && r.character == player.pose.character)
        {
            continue;
        }
        let character = ron::from_str::<crate::ai::archetype::Archetype>(&player.pose.character)
            .unwrap_or_default();
        let coat = coat
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.12, 0.65, 0.95),
                    ..default()
                })
            })
            .clone();
        let mut entity = commands.spawn((
            Name::new(format!("{} #{}", player.name, player.id)),
            Remote {
                id: player.id,
                character: player.pose.character.clone(),
            },
            Transform {
                translation: Vec3::from_array(player.pose.position),
                rotation: Quat::from_array(player.pose.rotation).normalize(),
                ..default()
            },
            Visibility::default(),
        ));
        let mut rng = crate::core::rng::stream_for(
            client.world.seed ^ player.id,
            crate::core::rng::stream::MULTIPLAYER,
        );
        crate::ai::figure::dress(
            &mut entity,
            &figures,
            coat,
            crate::mood::face::level_of(player.pose.mood),
            character,
            &mut rng,
        );
    }
    let mesh = actor_mesh
        .get_or_insert_with(|| meshes.add(Sphere::new(0.38)))
        .clone();
    let mats = actor_materials
        .get_or_insert_with(|| {
            (
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.95, 0.65, 0.12),
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.18, 0.20, 0.24),
                    ..default()
                }),
            )
        })
        .clone();
    let actors = data.actors.clone();
    drop(data);
    for (entity, remote, mut transform) in &mut remotes_actor {
        let Some(actor) = actors.iter().find(|a| a.id == remote.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        transform.translation = Vec3::from_array(actor.position);
        transform.rotation = Quat::from_rotation_y(actor.velocity[0].atan2(actor.velocity[2]));
    }
    for actor in &actors {
        if remotes_actor.iter().any(|(_, r, _)| r.0 == actor.id) {
            continue;
        }
        commands.spawn((
            RemoteActor(actor.id),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(match actor.kind {
                mood_multiplayer::ActorKind::Vehicle => mats.1.clone(),
                _ => mats.0.clone(),
            }),
            Transform::from_translation(Vec3::from_array(actor.position)),
            Visibility::default(),
        ));
    }
}
fn publish(
    client: Res<Client>,
    config: Res<GameConfig>,
    players: Query<
        (
            &Transform,
            &crate::mood::feeling::Mood,
            &leafwing_input_manager::prelude::ActionState<crate::player::input::Action>,
        ),
        With<Player>,
    >,
) {
    let Ok((transform, mood, actions)) = players.single() else {
        return;
    };
    client.shared.lock().unwrap().local = Some(Pose {
        position: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
        character: format!("{:?}", config.character),
        mood: mood.value.clamp(-1.0, 1.0),
        input: actions
            .clamped_axis_pair(&crate::player::input::Action::Move)
            .to_array(),
        buttons: [
            crate::player::input::Action::Jump,
            crate::player::input::Action::Sprint,
            crate::player::input::Action::Handbrake,
            crate::player::input::Action::Interact,
        ]
        .into_iter()
        .enumerate()
        .fold(0u16, |bits, (i, action)| {
            bits | (u16::from(actions.pressed(&action)) << i)
        }),
    });
}

fn paint_remote_faces(
    client: Res<Client>,
    figures: Res<crate::ai::figure::FigureAssets>,
    remotes: Query<(Entity, &Remote, Option<&crate::ai::appearance::Appearance>)>,
    joints: Query<&Children>,
    mut parts: Query<(
        &mut MeshMaterial3d<StandardMaterial>,
        Option<&crate::ai::figure::Head>,
        Option<&crate::ai::figure::Bare>,
    )>,
) {
    // Do not add Mood to replicas: local contagion and provocation would then
    // mutate a remote player's face independently of its owner's simulation.
    let data = client.shared.lock().unwrap();
    for (entity, remote, appearance) in &remotes {
        let Some(player) = data.players.iter().find(|p| p.id == remote.id) else {
            continue;
        };
        let level = crate::mood::face::level_of(player.pose.mood);
        for child in joints.iter_descendants(entity) {
            if let Ok((mut material, head, bare)) = parts.get_mut(child) {
                if head.is_some() {
                    if let Some(appearance) = appearance {
                        material.0 = figures.humans.face(appearance.skin, appearance.face, level);
                    }
                } else if bare.is_some()
                    && let Some(appearance) = appearance
                {
                    material.0 = figures.humans.skin(appearance.skin);
                }
            }
        }
    }
}
