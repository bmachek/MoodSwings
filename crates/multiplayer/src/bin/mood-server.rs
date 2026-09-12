use mood_multiplayer::{TICK, World, server::Server};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut world = World::default();
    let mut bind = "0.0.0.0:7777".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--help" {
            println!(
                "mood-server [--bind 0.0.0.0:7777] [--seed 42] [--city Landshuepf] [--hour 12] [--day-seconds 600]"
            );
            return Ok(());
        }
        let value = args.next().ok_or("missing option value")?;
        match flag.as_str() {
            "--check" => return check(&value),
            "--bind" => bind = value,
            "--seed" => world.seed = value.parse()?,
            "--city" => world.city = value,
            "--hour" => world.hour = value.parse()?,
            "--day-seconds" => world.day_seconds = value.parse()?,
            _ => return Err(format!("unknown option: {flag}").into()),
        }
    }
    let mut server = Server::bind(&bind, world)?;
    println!(
        "Mood Swings exploration server listening on {} (16 players, 20 Hz)",
        server.address()?
    );
    loop {
        let start = std::time::Instant::now();
        server.tick()?;
        std::thread::sleep(TICK.saturating_sub(start.elapsed()));
    }
}

fn check(address: &str) -> Result<(), Box<dyn std::error::Error>> {
    use mood_multiplayer::{ClientMessage, Connection, ServerMessage};
    use std::{
        net::TcpStream,
        time::{Duration, Instant},
    };
    let stream = TcpStream::connect_timeout(&address.parse()?, Duration::from_secs(2))?;
    let mut connection = Connection::new(stream)?;
    connection.send(&ClientMessage::Ping)?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        for message in connection.receive::<ServerMessage>()? {
            if matches!(message, ServerMessage::Pong) {
                return Ok(());
            }
        }
        connection.flush()?;
        std::thread::sleep(TICK);
    }
    Err("server handshake timed out".into())
}
