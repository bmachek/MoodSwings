//! Deterministic random streams.
//!
//! City generation must be reproducible: the same seed has to rebuild the exact
//! same city, because chunks are regenerated on demand rather than stored.
//!
//! The trap with a single shared RNG is that *draw order* becomes load-bearing —
//! adding one call in the building generator would silently reshuffle every
//! street downstream. So each subsystem derives its own independent stream from
//! (seed, key) and is free to draw as much as it likes.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Fixed keys for each independent generation stream. Never reuse a value.
pub mod stream {
    pub const ROADS: u64 = 1;
    pub const BLOCKS: u64 = 2;
    pub const BUILDINGS: u64 = 3;
    pub const PROPS: u64 = 4;
    pub const VEHICLE_SPAWNS: u64 = 5;
    pub const PEDESTRIANS: u64 = 6;
    /// Waveform synthesis. Not world state, but the same reproducibility
    /// argument applies: a sound should not change between runs.
    pub const AUDIO: u64 = 7;
    pub const RAIN: u64 = 8;
    /// Cloud cover and wind. Sampled rather than drawn — see `key_for`.
    pub const WEATHER: u64 = 9;
    /// Street trees and park planting. Its own stream rather than sharing the
    /// props one, so that planting a tree cannot shift which bin lands where.
    pub const VEGETATION: u64 = 10;
    /// Manholes, patches, stains and rubber. Separate again, for the same
    /// reason: a street's wear must not depend on how many trees are on it.
    pub const WEAR: u64 = 11;
    /// Temperaments. Its own stream for the usual reason, sharpened: the crowd
    /// is drawn from `PEDESTRIANS`, so taking their tempers from the same
    /// stream would mean that giving somebody a shorter fuse also moves where
    /// the next pedestrian spawns and which street they walk down.
    pub const MOOD: u64 = 12;
    /// Runtime wreckage: geyser spray and whatever chaos comes next. Not
    /// world generation — it depends on what the player crashed into — but it
    /// must still not draw from a generation stream, or knocking a hydrant
    /// over would reshuffle where the props around it spawn.
    pub const MAYHEM: u64 = 13;
    /// The civic zoning pass: which buildings become the town hall, the fire
    /// stations, the parking garages. Its own stream because it runs *after*
    /// the whole layout is drawn — a new civic kind must never reshuffle a
    /// single lot, height or palette that `BUILDINGS` already decided.
    pub const ZONING: u64 = 14;
    /// Who a citizen is: the archetype draw and everything that hangs off it.
    /// Not `PEDESTRIANS` (retuning the cast must not move where anybody
    /// spawns) and not `MOOD` (it must not move anybody's disposition either).
    pub const CROWD: u64 = 15;
    /// The cultural quarters: where the city keeps its Klein-Neapel and its
    /// Fernost-Viertel, and the zone ambience that hangs over them — the use
    /// this key was reserved for all along. Sampled via `key_for`, never
    /// drawn: which wedge of the city smells of basil is a fact about the
    /// seed, and placing a quarter must never reshuffle a single lot.
    pub const QUARTERS: u64 = 16;
    /// The city's event calendar: which day throws which parade. Sampled via
    /// `key_for` like the weather, never drawn — a parade at 14:00 on this
    /// seed must be a fact about the seed, reproducible under `--hour`.
    pub const EVENTS: u64 = 17;
    /// The animals: which pedestrian gets a dog, where the cats prowl.
    pub const ANIMALS: u64 = 18;
    /// Cyclists: where on the network they appear and what they wear. Their
    /// own stream for the usual reason — retuning how many bikes the city
    /// holds must not move a single pedestrian, mood or parked car.
    pub const CYCLISTS: u64 = 19;
    /// The captain. One resident, own wardrobe, own stream: he must be able
    /// to change his coat without a single citizen changing theirs.
    pub const CAPTAIN: u64 = 20;
    /// The canal: which street of the grid is surrendered to water. Sampled
    /// via `key_for` like the weather — where the river runs is a fact about
    /// the seed, and digging it must never move a lot.
    pub const RIVER: u64 = 21;
    /// What a building has put out on the pavement: the pipe, the sandwich
    /// board, the window boxes, the bikes, the terrace. Keyed per footprint
    /// like the roofs are, and on its own stream for the usual reason — the
    /// street's dressing must never move a roof, a lot or a bin.
    pub const FRONTAGE: u64 = 22;
    /// The pigeons: where a flock settles, how many are in it, and which one
    /// is the white one. Its own stream rather than `ANIMALS` — how many birds
    /// are on the paving must not move a cat, and startling a flock must not
    /// change a dog's coat.
    pub const PIGEONS: u64 = 23;
    /// Where the city's rubbish has collected. Its own stream for the reason
    /// every other one has: how dirty a street is must not move the bin at the
    /// end of it, nor the tree, nor the manhole.
    pub const LITTER: u64 = 24;
    /// Which streets are being dug up. Its own stream so that adding a
    /// worksite cannot move a bin, a tree or a piece of rubbish on any of the
    /// streets that are not.
    pub const WORKSITE: u64 = 25;
    /// What is strung across the narrow streets: pennants and washing. Its own
    /// stream so that hanging a line over a lane cannot move anything standing
    /// under it.
    pub const BUNTING: u64 = 26;
    /// Who in the crowd owns an umbrella and what colour it is. Runtime
    /// behaviour rather than world generation — it depends on what the sky is
    /// doing — so it takes its own stream for the reason `MAYHEM` does: a
    /// shower must not reshuffle anything the seed decided.
    pub const UMBRELLAS: u64 = 27;
    /// Which roofs have a chimney going and which gullies are steaming. Its
    /// own stream so that lighting a fire cannot move a lamp post.
    pub const PLUMES: u64 = 28;
    /// Which citizen has stopped to play and where the ring stands round
    /// them. Runtime behaviour, so its own stream: who is busking must not
    /// depend on how many pigeons have gone up.
    pub const BUSKERS: u64 = 29;
    /// Which streets have a van stopped on them and who is unloading it.
    pub const DELIVERIES: u64 = 30;
    /// What fills the ground a real town's frontage left open: the forecourt
    /// across a gap in a terrace and the wall or hedge along the back of the
    /// pavement. Its own stream and not `BUILDINGS`, for the reason at the top
    /// of this file made concrete — the gaps are decided *inside* the marcher's
    /// walk down each street, so a draw taken from the same stream would shift
    /// every frontage after it and rebuild the whole town from the same seed.
    pub const YARDS: u64 = 31;
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// An independent, reproducible stream for one subsystem.
pub fn stream_for(seed: u64, key: u64) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(key_for(seed, key))
}

/// The same derivation, stopping one step short of an RNG.
///
/// Some subsystems do not *draw* randomness, they *sample* it: value noise over
/// a clock needs a fixed key it can hash a position against, and building a
/// ChaCha state per sample would be absurd. They still want the stream keys to
/// stay independent, so the mixing is shared rather than reinvented.
pub fn key_for(seed: u64, key: u64) -> u64 {
    seed ^ key.wrapping_mul(GOLDEN)
}

/// A stream for one chunk of one subsystem, so chunks regenerate identically
/// regardless of the order the player visits them in.
pub fn stream_for_chunk(seed: u64, key: u64, chunk: (i32, i32)) -> ChaCha8Rng {
    let c = (chunk.0 as u64) << 32 | (chunk.1 as u32 as u64);
    ChaCha8Rng::seed_from_u64(seed ^ key.wrapping_mul(GOLDEN) ^ c.wrapping_mul(GOLDEN))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngExt;

    #[test]
    fn same_seed_same_sequence() {
        let a: Vec<u32> = (0..8)
            .map(|_| stream_for(42, stream::ROADS).random::<u32>())
            .collect();
        let b: Vec<u32> = (0..8)
            .map(|_| stream_for(42, stream::ROADS).random::<u32>())
            .collect();
        assert_eq!(a, b);
    }

    #[test]
    fn different_streams_diverge() {
        let roads = stream_for(42, stream::ROADS).random::<u64>();
        let blocks = stream_for(42, stream::BLOCKS).random::<u64>();
        assert_ne!(roads, blocks, "streams must be independent");
    }

    #[test]
    fn chunks_are_order_independent() {
        let first = stream_for_chunk(7, stream::BUILDINGS, (3, -2)).random::<u64>();
        let second = stream_for_chunk(7, stream::BUILDINGS, (3, -2)).random::<u64>();
        assert_eq!(first, second);
        assert_ne!(
            first,
            stream_for_chunk(7, stream::BUILDINGS, (-2, 3)).random::<u64>(),
            "chunk coords must not collide under swap"
        );
    }
}
