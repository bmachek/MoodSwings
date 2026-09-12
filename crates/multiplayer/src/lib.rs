//! Bounded TCP transport shared by the graphical client and the GPU-free server.
//! Clients own their poses; the server owns membership, IDs and the world clock.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::Duration,
};

pub mod server;
pub const VERSION: u32 = 1;
pub const TICK: Duration = Duration::from_millis(50);
pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_PLAYERS: usize = 16;
const MAX_FRAME: usize = 32 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct World {
    pub seed: u64,
    pub city: String,
    pub hour: f32,
    pub day_seconds: f32,
}
impl Default for World {
    fn default() -> Self {
        Self {
            seed: 42,
            city: "Landshuepf".into(),
            hour: 12.0,
            day_seconds: 600.0,
        }
    }
}
impl World {
    pub fn valid(&self) -> bool {
        [
            "Generisch",
            "Landshuepf",
            "NewDork",
            "Londoof",
            "Minga",
            "Paree",
        ]
        .contains(&self.city.as_str())
            && self.hour.is_finite()
            && (0.0..24.0).contains(&self.hour)
            && self.day_seconds.is_finite()
            && (0.0..=86400.0).contains(&self.day_seconds)
    }
    pub fn hour_after(&self, seconds: f32) -> f32 {
        (self.hour
            + if self.day_seconds > 0.0 {
                seconds * 24.0 / self.day_seconds
            } else {
                0.0
            })
        .rem_euclid(24.0)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub character: String,
    pub mood: f32,
}
impl Pose {
    pub fn valid(&self) -> bool {
        self.position
            .iter()
            .all(|x| x.is_finite() && x.abs() < 100_000.0)
            && self.rotation.iter().all(|x| x.is_finite())
            && (self.rotation.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 0.01
            && self.character.len() <= 48
            && self.character.chars().all(|c| c.is_ascii_alphanumeric())
            && self.mood.is_finite()
            && (-1.0..=1.0).contains(&self.mood)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub id: u64,
    pub name: String,
    pub pose: Pose,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    Ping,
    Hello { version: u32, name: String },
    Update(Option<Pose>),
}
#[derive(Debug, Serialize, Deserialize)]
pub enum ServerMessage {
    Pong,
    Welcome { version: u32, id: u64, world: World },
    Snapshot { hour: f32, players: Vec<Player> },
}
pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 24 && !name.chars().any(char::is_control)
}
pub fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Retains partial reads and writes. Neither a slow peer nor an unbounded line
/// may block the server tick or grow its memory indefinitely.
pub struct Connection {
    stream: TcpStream,
    input: Vec<u8>,
    output: Vec<u8>,
}
impl Connection {
    pub fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            input: Vec::new(),
            output: Vec::new(),
        })
    }
    pub fn send(&mut self, value: &impl Serialize) -> io::Result<()> {
        let bytes = ron::to_string(value).map_err(|e| invalid(&e.to_string()))?;
        if bytes.len() + self.output.len() + 1 > MAX_FRAME * 2 {
            return Err(invalid("outgoing queue exceeded"));
        }
        self.output.extend_from_slice(bytes.as_bytes());
        self.output.push(b'\n');
        self.flush()
    }
    pub fn flush(&mut self) -> io::Result<()> {
        while !self.output.is_empty() {
            match self.stream.write(&self.output) {
                Ok(0) => return Err(invalid("connection closed")),
                Ok(n) => {
                    self.output.drain(..n);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    pub fn receive<T: DeserializeOwned>(&mut self) -> io::Result<Vec<T>> {
        let mut bytes = [0; MAX_FRAME];
        // One bounded read per tick, so a sender cannot monopolise the loop.
        match self.stream.read(&mut bytes) {
            Ok(0) => return Err(invalid("connection closed")),
            Ok(n) => self.input.extend_from_slice(&bytes[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
        if self.input.len() > MAX_FRAME {
            return Err(invalid("frame too large"));
        }
        let mut messages = Vec::new();
        while let Some(end) = self.input.iter().position(|b| *b == b'\n') {
            messages.push(
                ron::de::from_bytes(&self.input[..end]).map_err(|e| invalid(&e.to_string()))?,
            );
            self.input.drain(..=end);
            if messages.len() > 64 {
                return Err(invalid("message rate exceeded"));
            }
        }
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_coordinates_and_names_cannot_enter_the_session() {
        let mut pose = Pose {
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            character: "Everyday".into(),
            mood: 0.5,
        };
        assert!(pose.valid());
        pose.position[0] = f32::NAN;
        assert!(!pose.valid());
        assert!(!valid_name("\nAlice"));
        assert!(!valid_name("   "));
        assert!(valid_name("Jürgen"));
    }
    #[test]
    fn the_session_clock_wraps_and_can_be_frozen() {
        let mut world = World::default();
        assert_eq!(world.hour_after(300.0), 0.0);
        world.day_seconds = 0.0;
        assert_eq!(world.hour_after(300.0), 12.0);
    }
}
