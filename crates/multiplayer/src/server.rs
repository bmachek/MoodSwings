use crate::*;
use rand::SeedableRng;
use std::{
    net::{SocketAddr, TcpListener},
    time::Instant,
};

struct Peer {
    connection: Connection,
    id: u64,
    name: Option<String>,
    pose: Option<Pose>,
    heard: Instant,
}
pub struct Server {
    listener: TcpListener,
    world: World,
    started: Instant,
    peers: Vec<Peer>,
    next_id: u64,
    actors: Vec<Actor>,
}
impl Server {
    pub fn bind(address: &str, world: World) -> io::Result<Self> {
        if !world.valid() {
            return Err(invalid("invalid world configuration"));
        }
        let seed = world.seed;
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            world,
            started: Instant::now(),
            peers: Vec::new(),
            next_id: 1,
            actors: seed_actors(seed),
        })
    }
    pub fn address(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }
    pub fn tick(&mut self) -> io::Result<()> {
        for _ in 0..MAX_PLAYERS {
            match self.listener.accept() {
                Ok((stream, _)) if self.peers.len() < MAX_PLAYERS + 8 => {
                    self.peers.push(Peer {
                        connection: Connection::new(stream)?,
                        id: self.next_id,
                        name: None,
                        pose: None,
                        heard: Instant::now(),
                    });
                    self.next_id += 1;
                }
                Ok(_) => {} // Dropping the stream refuses a full session.
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        let hour = self.world.hour_after(self.started.elapsed().as_secs_f32());
        let dt = TICK.as_secs_f32();
        for actor in &mut self.actors {
            actor.position[0] += actor.velocity[0] * dt;
            actor.position[2] += actor.velocity[2] * dt;
            if actor.position[0].abs() > 900.0 {
                actor.velocity[0] = -actor.velocity[0];
            }
            if actor.position[2].abs() > 900.0 {
                actor.velocity[2] = -actor.velocity[2];
            }
        }
        let mut admitted = self.peers.iter().filter(|p| p.name.is_some()).count();
        self.peers.retain_mut(|peer| {
            let result = (|| -> io::Result<()> {
                if peer.heard.elapsed() > TIMEOUT {
                    return Err(invalid("peer timed out"));
                }
                for message in peer.connection.receive::<ClientMessage>()? {
                    match message {
                        ClientMessage::Ping if peer.name.is_none() => {
                            peer.connection.send(&ServerMessage::Pong)?;
                            return Err(invalid("health probe complete"));
                        }
                        ClientMessage::Hello { version, name }
                            if peer.name.is_none()
                                && version == VERSION
                                && valid_name(&name)
                                && admitted < MAX_PLAYERS =>
                        {
                            admitted += 1;
                            peer.name = Some(name);
                            let mut world = self.world.clone();
                            world.hour = hour;
                            peer.connection.send(&ServerMessage::Welcome {
                                version: VERSION,
                                id: peer.id,
                                world,
                            })?;
                        }
                        ClientMessage::Update(pose) if peer.name.is_some() => {
                            if pose.as_ref().is_some_and(|p| !p.valid()) {
                                return Err(invalid("invalid pose"));
                            }
                            if let Some(mut pose) = pose {
                                // The wire position is a report for
                                // interpolation. Movement is the command: the
                                // authority advances it and bounds the result.
                                if pose.input != [0.0, 0.0] {
                                    pose.position[0] += pose.input[0] * 0.18;
                                    pose.position[2] += pose.input[1] * 0.18;
                                    pose.position[0] = pose.position[0].clamp(-1000.0, 1000.0);
                                    pose.position[2] = pose.position[2].clamp(-1000.0, 1000.0);
                                } else if let Some(previous) = &peer.pose {
                                    pose.position = previous.position;
                                }
                                peer.pose = Some(pose);
                            } else {
                                peer.pose = None;
                            }
                        }
                        _ => return Err(invalid("invalid handshake")),
                    }
                    peer.heard = Instant::now();
                }
                Ok(())
            })();
            result.is_ok()
        });
        let players = self
            .peers
            .iter()
            .filter_map(|p| {
                Some(Player {
                    id: p.id,
                    name: p.name.clone()?,
                    pose: p.pose.clone()?,
                })
            })
            .collect();
        let snapshot = ServerMessage::Snapshot {
            hour,
            players,
            actors: self.actors.clone(),
        };
        self.peers
            .retain_mut(|p| p.name.is_none() || p.connection.send(&snapshot).is_ok());
        Ok(())
    }
}

fn seed_actors(seed: u64) -> Vec<Actor> {
    // A deterministic, server-owned ambient population. Clients never invent
    // NPCs or traffic, so reconnects and different GPU frame rates cannot
    // change who is on the street.
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed ^ 0x4d50_4143_544f_5253);
    use rand::RngExt;
    let mut actors = Vec::with_capacity(48);
    for id in 0..32u64 {
        actors.push(Actor {
            id,
            kind: ActorKind::Pedestrian,
            position: [
                rng.random_range(-850.0..850.0),
                0.9,
                rng.random_range(-850.0..850.0),
            ],
            velocity: [
                rng.random_range(-1.2..1.2),
                0.0,
                rng.random_range(-1.2..1.2),
            ],
            mood: rng.random_range(-0.15..0.15),
        });
    }
    for id in 0..16u64 {
        actors.push(Actor {
            id: 10_000 + id,
            kind: ActorKind::Vehicle,
            position: [
                rng.random_range(-850.0..850.0),
                0.5,
                rng.random_range(-850.0..850.0),
            ],
            velocity: [
                rng.random_range(-8.0..8.0),
                0.0,
                rng.random_range(-8.0..8.0),
            ],
            mood: 0.0,
        });
    }
    actors
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settle(server: &mut Server) {
        for _ in 0..10 {
            server.tick().unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn connect(server: &mut Server, name: &str) -> Connection {
        let mut client =
            Connection::new(TcpStream::connect(server.address().unwrap()).unwrap()).unwrap();
        client
            .send(&ClientMessage::Hello {
                version: VERSION,
                name: name.into(),
            })
            .unwrap();
        settle(server);
        client
    }
    fn pump(server: &mut Server, client: &mut Connection) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        for _ in 0..10 {
            server.tick().unwrap();
            messages.extend(client.receive::<ServerMessage>().unwrap());
            std::thread::sleep(Duration::from_millis(2));
        }
        messages
    }
    fn pose() -> Pose {
        Pose {
            position: [12.0, 2.0, -4.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            character: "Punk".into(),
            mood: -0.5,
            input: [0.0, 0.0],
            buttons: 0,
        }
    }
    #[test]
    fn two_clients_share_world_and_pose_then_observe_disconnect() {
        let world = World {
            day_seconds: 0.0,
            ..World::default()
        };
        let mut server = Server::bind("127.0.0.1:0", world.clone()).unwrap();
        let mut alice = connect(&mut server, "Alice");
        let welcome = pump(&mut server, &mut alice);
        assert!(
            welcome.iter().any(
                |m| matches!(m, ServerMessage::Welcome { world: w, id: 1, .. } if *w == world)
            )
        );
        let mut bob = connect(&mut server, "Bob");
        alice.send(&ClientMessage::Update(Some(pose()))).unwrap();
        let messages = pump(&mut server, &mut bob);
        assert!(messages.iter().any(|m| matches!(m, ServerMessage::Snapshot { players, .. } if players.iter().any(|p| p.name == "Alice" && p.pose == pose()))));
        drop(alice);
        let messages = pump(&mut server, &mut bob);
        assert!(
            matches!(messages.last(), Some(ServerMessage::Snapshot { players, .. }) if players.is_empty())
        );
    }
    #[test]
    fn wrong_versions_and_updates_before_hello_are_disconnected() {
        for message in [
            ClientMessage::Hello {
                version: VERSION + 1,
                name: "Alice".into(),
            },
            ClientMessage::Update(Some(pose())),
        ] {
            let mut server = Server::bind("127.0.0.1:0", World::default()).unwrap();
            let mut client =
                Connection::new(TcpStream::connect(server.address().unwrap()).unwrap()).unwrap();
            client.send(&message).unwrap();
            settle(&mut server);
            assert!(server.peers.is_empty());
        }
    }
    #[test]
    fn invalid_poses_and_idle_connections_are_removed() {
        let mut server = Server::bind("127.0.0.1:0", World::default()).unwrap();
        let mut alice = connect(&mut server, "Alice");
        let mut invalid_pose = pose();
        invalid_pose.rotation = [0.0; 4];
        alice
            .send(&ClientMessage::Update(Some(invalid_pose)))
            .unwrap();
        settle(&mut server);
        assert!(server.peers.is_empty());
        let _bob = connect(&mut server, "Bob");
        server.peers[0].heard = Instant::now() - TIMEOUT - TICK;
        settle(&mut server);
        assert!(server.peers.is_empty());
    }
    #[test]
    fn capacity_is_bounded_and_a_freed_slot_can_be_rejoined() {
        let mut server = Server::bind("127.0.0.1:0", World::default()).unwrap();
        let mut clients: Vec<_> = (0..MAX_PLAYERS)
            .map(|_| connect(&mut server, "Flummi"))
            .collect();
        let _extra = connect(&mut server, "Overflow");
        settle(&mut server);
        assert_eq!(server.peers.len(), MAX_PLAYERS);
        clients.pop();
        settle(&mut server);
        let _replacement = connect(&mut server, "Replacement");
        assert_eq!(server.peers.len(), MAX_PLAYERS);
        assert!(server.peers.last().unwrap().id > MAX_PLAYERS as u64);
    }
    #[test]
    fn a_fragmented_handshake_waits_for_its_delimiter() {
        let mut server = Server::bind("127.0.0.1:0", World::default()).unwrap();
        let mut stream = TcpStream::connect(server.address().unwrap()).unwrap();
        stream.set_nodelay(true).unwrap();
        stream.write_all(b"Hello(version:1,name:").unwrap();
        settle(&mut server);
        assert!(server.peers[0].name.is_none());
        stream.write_all(b"\"Alice\")\n").unwrap();
        settle(&mut server);
        assert_eq!(server.peers[0].name.as_deref(), Some("Alice"));
    }
}

#[cfg(test)]
mod transport_limits {
    use super::*;
    #[test]
    fn a_full_session_still_answers_health_checks() {
        let mut server = Server::bind("127.0.0.1:0", World::default()).unwrap();
        let mut clients = Vec::new();
        for _ in 0..MAX_PLAYERS {
            let mut client =
                Connection::new(TcpStream::connect(server.address().unwrap()).unwrap()).unwrap();
            client
                .send(&ClientMessage::Hello {
                    version: VERSION,
                    name: "Flummi".into(),
                })
                .unwrap();
            clients.push(client);
        }
        for _ in 0..20 {
            server.tick().unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(server.peers.len(), MAX_PLAYERS);
        let mut probe =
            Connection::new(TcpStream::connect(server.address().unwrap()).unwrap()).unwrap();
        probe.send(&ClientMessage::Ping).unwrap();
        let start = Instant::now();
        loop {
            server.tick().unwrap();
            if probe
                .receive::<ServerMessage>()
                .unwrap()
                .iter()
                .any(|m| matches!(m, ServerMessage::Pong))
            {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(server.peers.len(), MAX_PLAYERS);
    }
    #[test]
    fn an_unterminated_oversized_frame_cannot_grow_the_receive_buffer() {
        let mut server = Server::bind("127.0.0.1:0", World::default()).unwrap();
        let mut stream = TcpStream::connect(server.address().unwrap()).unwrap();
        stream.write_all(&vec![b'x'; MAX_FRAME + 1]).unwrap();
        for _ in 0..20 {
            server.tick().unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(server.peers.is_empty());
    }
}
