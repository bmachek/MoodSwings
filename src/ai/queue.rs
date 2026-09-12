//! Anstehen.
//!
//! `ai::errands` gave the city somewhere to go and the door was the end of
//! it: a citizen walked up to a shopfront and was gone, at whatever rate the
//! doors happened to be reached. That is a city where nobody ever has to
//! *wait* for anything, which is not a city, it is a lobby.
//!
//! A queue is the cheapest social structure there is and it carries more
//! legible intelligence per line of code than anything else in this module.
//! It is ordered, so it can be jumped. It is shared, so somebody at the back
//! is being made to wait by somebody at the front they cannot see. It has a
//! rate, so it can be too slow. And everybody in it is standing perfectly
//! still facing the same way, which is the single most recognisable shape a
//! crowd ever makes — a player reads "there is something in there" from
//! across a junction without one word of UI.
//!
//! Everything a queue then does falls out of systems that already exist. The
//! waiting drains moods, so the faces redden on their own, the contagion
//! spreads it down the line on its own, and a line that has waited too long
//! throws a rage wave without this module knowing rage waves exist. All this
//! has to supply is the shape and the grievance.
//!
//! ## The four things that make it a queue and not a huddle
//!
//! * **It runs along the wall, not out into the road.** The direction comes
//!   from `Shopfront::outward`, which is why that field exists. Which way
//!   along is a fact about the door — derived from its [`PlaceId`] — so both
//!   pavements' worth of queues do not all lean the same way, and the answer
//!   survives the chunk streaming out and coming back.
//! * **It only advances at the front.** The head is served, goes in, and
//!   everybody shuffles up one. Nobody in the line decides anything; they
//!   follow the person ahead, which is the whole of what standing in a queue
//!   is, and it is also why the shuffle reads so strongly from a distance.
//! * **It can be lost.** Patience is the temperament's fuse crossed with the
//!   archetype's reason for being there ([`Archetype::patience`]), so the
//!   Wutbürger gives up first and loudest and the vendor is still there when
//!   the shop shuts. Somebody peeling off the back of a line is the most
//!   human thing the crowd does.
//! * **It can be jumped.** Standing in front of the head is the one thing the
//!   player can do to this city that requires no violence at all, and the
//!   whole line turns round for it. The queue also *stops* while it is being
//!   jumped, because the alternative is an outrage with no teeth. And the
//!   city does it to itself: [`barges`] decides, once, on the way over,
//!   whether somebody walks to the back or to the front, so the joke happens
//!   on streets nobody is standing in.
//!
//! And one thing that makes it funny rather than merely correct: a queue
//! recruits. A line of five people outside a door is the most persuasive
//! advertisement in any city, and [`join_the_fun`] is the entire mechanic —
//! passers-by attach themselves to the back of it at a rate set by how nosy
//! their archetype is, having no idea what is being sold.
//!
//! ## Discipline
//!
//! The randomness is playback jitter, not world generation: it draws from
//! [`AudioRng`] for the same reason `ai::social` does, and nothing here may
//! touch a generation stream. The feel constants are module-level, matching
//! the sibling this module orders itself against — `ai::social` argues about
//! `CHAT_CHANCE` in exactly this register — and the whole line is kept in one
//! resource rather than on the front's entity, because the front is streamed
//! and the queue outside it must not be.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use rand::RngExt;

use super::archetype::Archetype;
use super::errands::{Browsing, Errand, Errands};
use super::figure::Attention;
use super::pedestrian::{Pedestrian, Walking};
use super::social::{Chatting, Composure, Socialising};
use crate::audio::AudioRng;
use crate::bounce::controller::{Bouncer, Launched};
use crate::core::schedule::GameSet;
use crate::mood::feeling::{Mood, Temperament};
use crate::mood::grudge::Grudge;
use crate::world::interior::PlaceId;

/// How far apart two people stand in a line, in metres.
///
/// Wider than the crowd's personal space, because a queue is people who have
/// agreed to be near each other and are still not friends.
const PITCH: f32 = 0.95;

/// How long a line gets before the door gives up recruiting. Six is already
/// five and a half metres of pavement; past that a queue stops being a shape
/// and becomes a wall across the street.
const MAX: usize = 6;

/// How near a slot counts as standing in it, and the shuffle that closes the
/// gap. A shuffle, emphatically: at walking pace the line snaps forward like
/// a chain being yanked, and the whole read is the slow concertina.
const SLOT: f32 = 0.28;
const SHUFFLE: f32 = 0.75;

/// How long the business at the counter takes, either side of the draw.
///
/// The rate at which a line drains, and therefore half of how long a line
/// gets. Long enough that a queue behind somebody is a queue and not a
/// turnstile; short enough that the patience below still clears one.
const SERVICE: (f32, f32) = (4.5, 11.0);

/// Mood lost per second of waiting, before the fuse scales it.
///
/// A drain rather than a jolt, and therefore a number to read through
/// [`souring`] rather than on its own: `feeling::drifted` is an exponential
/// ease back towards the baseline, so a constant drain never accumulates,
/// it finds a level — and the level is the only thing the street ever sees.
const WAIT_STING: f32 = 0.05;

/// And the lift for moving up one. Queueing is misery with punctuation.
const SHUFFLE_CHEER: f32 = 0.05;

/// Seconds of patience everybody has simply for having decided to join, and
/// the span the temperament and the errand then argue over.
///
/// A floor plus a scaled part rather than a bare product, and the floor is
/// not a fudge: at a bare product the Wutbürger's patience came out at four
/// seconds, which is less than the walk from the pavement to the back of the
/// line, so he balked on arrival every single time and the best joke in the
/// module never happened once.
const PATIENCE_FLOOR: f32 = 14.0;
const PATIENCE_SPAN: f32 = 34.0;

/// What giving up costs, on top of everything the wait already took.
const BALK_STING: f32 = 0.12;

/// How often somebody cranes past the person in front to see what the
/// hold-up is, and how long the look lasts.
const CRANE_CHANCE: f32 = 0.22;
const CRANE: f32 = 1.1;

/// The recruiting: how near a queue has to be to be worth joining, how many
/// people it takes before it is persuasive at all, and the base chance per
/// second before [`Archetype::nosiness`] scales it.
///
/// The chance is the rate a line grows at, and [`SERVICE`] is the rate it
/// drains at; a queue's length is the argument between them, and the only
/// place to settle it is a patrol, because two errands landing on the same
/// door within a few seconds of each other is not something that can be
/// reasoned about from a constant.
///
/// It was reasoned about from a constant anyway, and the patrol said no.
/// `LURE_AT` was two, on the sound-sounding ground that one person outside
/// a shop is not a queue yet. A hundred and fifty seconds of patrolled city
/// never got two people to one door at one moment — every reading was one
/// standing in one line — so the threshold was never crossed and the whole
/// recruiting mechanic did not run once. Reaching two *is* the coincidence
/// the lure exists to manufacture; gating the lure on it gates it on
/// itself, and nothing about that failure is visible from inside the game,
/// because a city where every door has one customer looks like a city that
/// is working.
///
/// One, then — which is the truer reading as well. Somebody standing at a
/// door is how every queue in the world starts, and „da steht schon einer"
/// is a better joke than the rule it replaces. The long stop is
/// [`CROWD_SHARE`] rather than anything here.
const LURE_RANGE: f32 = 17.0;
const LURE_AT: usize = 1;
const LURE_CHANCE: f32 = 0.05;
// The guard on a mistake only a patrol could find, because nothing fails
// when it is wrong: above one, the lure never runs, every door shows one
// customer, and the city looks like it is working.
const _: () = assert!(LURE_AT <= 1);

/// And how long a recruit gives the walk over before thinking better of it.
/// Generous, because they have watched people go over there and stay, so
/// going over there and staying is the entire plan.
const LURE_PATIENCE: f32 = 30.0;

/// How much of the crowd may be standing in lines at once.
///
/// The lure is a positive feedback loop — a queue recruits, which makes it
/// more persuasive, which recruits faster — and left alone it does exactly
/// what that describes: within half a minute a fifth of the pavement is
/// standing outside three bakeries and the street is empty. A city where
/// everybody is queueing has no street life left, however good each
/// individual queue is, so the recruiting stops here. The queues that exist
/// carry on; nothing already standing is sent home.
///
/// A quarter rather than the sixth it was first set to. The same patrol
/// that found `LURE_AT` unreachable also showed what the cap has to leave
/// room for: at the default crowd a sixth is nine people, which is one full
/// line and half of another, and two lines is the fewest a street can show
/// and still read as a city that queues rather than as one shop having a
/// moment.
const CROWD_SHARE: f32 = 0.25;

/// Pushing in, when it is one of the city's own doing it.
///
/// A one-shot roll on arrival rather than a rate: somebody either walks to
/// the back like everybody else or they do not, and they decide it once, on
/// the way over. [`Archetype::cheek`] is most of the answer and the mood is
/// the rest — nobody pushes in on a good day, which is what makes it read as
/// a mood rather than as a personality.
const BARGE_ODDS: f32 = 0.5;
const BARGE_MOOD: f32 = -0.1;
/// What watching somebody do it costs everybody standing behind them, all at
/// once. Larger than a second of ordinary waiting by a wide margin: the
/// queue has just been told that the last two minutes were optional.
const BARGE_STING: f32 = 0.22;

/// Pushing in: how near the head of the line counts as being in front of it,
/// how long somebody has to stand there before it stops being a passer-by,
/// how long the line stays outraged once it has decided, and what it costs
/// everybody standing in it per second.
const JUMP_RANGE: f32 = 1.5;
const JUMP_GRACE: f32 = 1.1;
const OUTRAGE: f32 = 3.0;
const JUMP_STING: f32 = 0.30;

/// How far above a body's origin its face is. The same offset `ai::social`
/// writes down, for the same reason: a glare aimed at somebody's chest is
/// not a glare.
const LOOK_AT_FACE: f32 = 0.55;

/// Standing in a line. The place is the identity, not the entity: the front
/// is streamed and the queue outside it is not.
#[derive(Component, Debug)]
pub struct Queueing {
    pub place: PlaceId,
    /// When they joined, for the patience that runs out.
    pub since: f32,
}

/// One line outside one door.
struct Line {
    /// The front's spot on the pavement, where the head stands.
    door: Vec2,
    /// Unit, along the wall, pointing from the head towards the back.
    along: Vec2,
    /// Out of the door towards the road — what the head faces, reversed.
    outward: Vec2,
    /// Head first. The only ordered thing in the whole crowd.
    members: Vec<Entity>,
    /// Seconds left of the head's business at the counter.
    serving: f32,
    /// How long this line has stood without anybody getting in.
    ///
    /// Not a mechanic — nothing reads it to decide anything — but an
    /// instrument: a queue whose head cannot reach the door never advances
    /// and never complains, and a silent wedge outside one shop in a town
    /// of several hundred is precisely the failure a human would not find.
    /// `core::patrol` asks for the worst one every second, which is what
    /// that harness is for.
    stalled: f32,
    /// How long somebody has been standing in front of the head, and how
    /// long the line stays furious about it once it has made up its mind.
    intrusion: f32,
    outrage: f32,
    /// Somebody who has just walked in at the front, waiting to be noticed.
    ///
    /// Parked here rather than acted on where it happens, because the place
    /// it happens is an arrival — no mood query, no transforms, nothing to
    /// glare with. [`mind_the_queue`] takes it on the next frame, which is
    /// also the right beat: a queue does not react instantly, it takes a
    /// moment to believe it.
    jumped: Option<Entity>,
}

impl Line {
    /// Where the citizen standing `index` back from the door stands.
    fn slot(&self, index: usize) -> Vec2 {
        self.door + self.along * (index as f32 * PITCH)
    }
}

/// How long somebody will stand in a line before giving up on whatever is at
/// the front of it.
///
/// The temperament says how they take the waiting; the archetype says what
/// they came for, and therefore how much they are prepared to pay in minutes
/// to get it. The fuse is floored at 0.2 so the least patient disposition in
/// the game still has a patience rather than a countdown.
pub fn patience(temper: &Temperament, archetype: Archetype) -> f32 {
    PATIENCE_FLOOR + PATIENCE_SPAN * (1.4 - temper.fuse).max(0.2) * archetype.patience()
}

/// Whether somebody arriving at a door in this mood pushes in.
///
/// `roll` is a fresh draw in 0..1 — playback jitter, like every other roll in
/// this module, and deliberately not reproducible from the world seed: who
/// pushed in outside which bakery is not a fact about the city, it is
/// something that happened while you were watching.
pub fn barges(archetype: Archetype, mood: f32, roll: f32) -> bool {
    mood < BARGE_MOOD && roll < archetype.cheek() * BARGE_ODDS
}

/// Where standing in a line settles somebody's mood.
///
/// The drain finds a level at `baseline - sting / recovery`, which is the
/// number worth arguing about: at the constants above a serene flummi comes
/// out of a queue very slightly less delighted than it went in, an ordinary
/// one visibly grey, and a Wutbürger flat on the floor of the scale. The
/// spread is a fact about the five recovery rates rather than about the
/// drain, which is why it is written as a function of the temperament and
/// tested as one.
pub fn souring(temper: &Temperament) -> f32 {
    (temper.baseline - WAIT_STING * (0.5 + temper.fuse) / temper.recovery).clamp(-1.0, 1.0)
}

/// Every line in the city that currently has somebody in it.
///
/// A resource rather than a component on the front, because the front is a
/// streamed entity and a queue that dissolved every time its chunk went out
/// of range would never be seen to do anything. Empty lines are dropped, so
/// this stays the size of the queues that exist rather than of the town.
#[derive(Resource, Default)]
pub struct Queues {
    lines: HashMap<PlaceId, Line>,
    tally: Tally,
}

/// Running totals since the city started, for `core::patrol`'s report.
///
/// A queue is a pipeline — an errand aimed at a door, a walk, a place in the
/// line, a turn at the counter — and every stage of it can be empty for a
/// different reason. The standing count alone cannot tell "nobody ever set
/// out" from "everybody set out and nobody arrived", and those want opposite
/// fixes. Two patrols were spent guessing between them from one number; this
/// is the second number, and the one after that.
#[derive(Default, Debug, Clone, Copy)]
pub struct Tally {
    /// Errands handed out by the recruiting, and places actually taken.
    pub lured: u32,
    pub joined: u32,
    /// Turned away because the line was already full.
    pub refused: u32,
    /// Served, and gave up waiting.
    pub served: u32,
    pub balked: u32,
}

impl Queues {
    /// How many are standing outside this door.
    pub fn len_at(&self, place: PlaceId) -> usize {
        self.lines.get(&place).map_or(0, |line| line.members.len())
    }

    /// Everybody standing in a line anywhere, for the dev panel and the tests.
    pub fn standing(&self) -> usize {
        self.lines.values().map(|line| line.members.len()).sum()
    }

    pub fn lines(&self) -> usize {
        self.lines.len()
    }

    /// What the queues have done since the city started.
    pub fn tally(&self) -> Tally {
        self.tally
    }

    /// The longest any line in the city has stood without serving anybody.
    ///
    /// The watch's reading. Ordinary business is a handful of seconds and the
    /// patience below clears a line that is merely slow, so a large number
    /// here means a queue that cannot move rather than one that is busy.
    pub fn longest_stall(&self) -> f32 {
        self.lines
            .values()
            .map(|line| line.stalled)
            .fold(0.0, f32::max)
    }

    /// Joins the line outside a door, opening one if there is none.
    ///
    /// `barge` asks for the front instead of the back, and is honoured only
    /// where there is a front to push into: a line of one is a person, and
    /// walking up beside them is not pushing in. The head itself is never
    /// displaced — they are mid-transaction, and nobody has ever had the
    /// nerve — so a barger goes in at second, which is the real move anyway.
    ///
    /// `false` if the line is already as long as a line gets; the caller's
    /// citizen then thinks better of the whole errand, which is what
    /// everybody does when they see six people outside a bakery.
    fn join(
        &mut self,
        entity: Entity,
        place: PlaceId,
        door: Vec3,
        outward: Vec2,
        barge: bool,
    ) -> bool {
        let line = self.lines.entry(place).or_insert_with(|| {
            // Which way along the wall the line runs is a fact about the
            // door rather than a draw, so a queue that streams out and comes
            // back leans the same way, and the two sides of a street do not
            // all fold towards the same corner.
            let side = if place.0 & 1 == 0 { 1.0 } else { -1.0 };
            let along = Vec2::new(-outward.y, outward.x).normalize_or_zero() * side;
            Line {
                door: door.xz(),
                along,
                outward: outward.normalize_or_zero(),
                members: Vec::new(),
                serving: SERVICE.0,
                stalled: 0.0,
                intrusion: 0.0,
                outrage: 0.0,
                jumped: None,
            }
        });
        if line.members.len() >= MAX || line.members.contains(&entity) {
            return false;
        }
        if barge && line.members.len() >= 2 {
            line.members.insert(1, entity);
            line.jumped = Some(entity);
        } else {
            line.members.push(entity);
        }
        true
    }
}

pub struct QueuePlugin;

impl Plugin for QueuePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Queues>().add_systems(
            Update,
            (
                reconcile_lines,
                join_the_fun,
                mind_the_queue,
                serve_the_head,
                lose_patience,
                hold_the_line,
            )
                .chain()
                .in_set(GameSet::Ai)
                // A fourth writer of `Bouncer::desired`, and the last one:
                // Walking, then Socialising, then Errands, then this. A
                // citizen who is standing in a line is standing in it, and
                // whatever runs later wins — which is the ordering contract
                // `ai::errands` states and the reason it is stated.
                .after(Walking)
                .after(Socialising)
                .after(Errands),
        );
    }
}

/// Keeps the lines honest about who is actually still in them.
///
/// Every way out of a queue goes through here rather than through the system
/// that caused it: a citizen recycled by the population budget, launched
/// through a shop window, panicked by a car or nursing a fresh grudge all
/// leave the same way, and none of the systems that do those things has to
/// know queues exist. That is the same trick `ai::resident` plays on the
/// despawns, and for the same reason — the alternative is one forgotten
/// cleanup away from a line of ghosts outside a bakery.
fn reconcile_lines(
    mut commands: Commands,
    mut rng: ResMut<AudioRng>,
    mut queues: ResMut<Queues>,
    // `With<Queueing>` is not decoration. Without it this walks the whole
    // crowd and hands a `Composure` to every citizen in the city every
    // frame, which quietly switches off chatting and window-shopping
    // everywhere and looks exactly like the street having gone shy.
    standing: Query<
        (
            Entity,
            &Pedestrian,
            Has<Launched>,
            Has<Grudge>,
            Has<Chatting>,
        ),
        With<Queueing>,
    >,
) {
    let fit: HashSet<Entity> = standing
        .iter()
        .filter(|(_, pedestrian, launched, grudge, chatting)| {
            !launched && !grudge && !chatting && pedestrian.panic <= 0.0
        })
        .map(|(entity, ..)| entity)
        .collect();

    for line in queues.lines.values_mut() {
        line.members.retain(|member| fit.contains(member));
    }
    queues.lines.retain(|_, line| !line.members.is_empty());

    // And anybody the lines have let go of stops standing about. Collected
    // first because the release below borrows nothing from the resource.
    let held: HashSet<Entity> = queues
        .lines
        .values()
        .flat_map(|line| line.members.iter().copied())
        .collect();
    for (entity, ..) in &standing {
        if !held.contains(&entity) {
            commands
                .entity(entity)
                .remove::<Queueing>()
                .insert(Composure {
                    left: rng.random_range(6.0..16.0),
                });
        }
    }
}

/// A queue is an advertisement.
///
/// Five people outside a door will sell a passer-by something none of them
/// could describe, and this is the whole of that: a roll per second per
/// nearby citizen, scaled by how nosy their archetype is, handing them an
/// errand aimed at the door. They walk over and join the back like everybody
/// else. Nobody is teleported into a line and nobody is told what is in
/// there, because nobody in a real queue knows either.
///
/// Iterated lines-first rather than citizens-first: there are never more
/// than a handful of lines in the city and the inner loop is a distance
/// check, so the cost is the cost of the queues that exist.
#[allow(clippy::type_complexity)]
fn join_the_fun(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<crate::core::config::GameConfig>,
    mut rng: ResMut<AudioRng>,
    mut queues: ResMut<Queues>,
    candidates: Query<
        (Entity, &Transform, &Pedestrian, &Archetype),
        (
            Without<Errand>,
            Without<Browsing>,
            Without<Queueing>,
            Without<Chatting>,
            Without<Grudge>,
            Without<Launched>,
            // Somebody who has just walked out of a line is not a candidate
            // for walking straight back into it. `lose_patience` hands a
            // balker a long `Composure`, and at a lure threshold of one the
            // door they just left is still an attractor — so without this
            // the recruiting does not merely undo the balk, it turns the
            // most human thing the crowd does into a citizen vibrating on
            // the spot outside a bakery.
            Without<Composure>,
            Without<super::busker::Listening>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    if queues.standing() as f32 > config.crowd.population as f32 * CROWD_SHARE {
        return;
    }
    // Counted up first and added at the end: the loop below borrows the
    // lines out of the very resource the tally lives on.
    let mut lured = 0u32;
    for (place, line) in &queues.lines {
        if line.members.len() < LURE_AT || line.members.len() >= MAX {
            continue;
        }
        // A line at head height in the middle of the pavement; the errand
        // aims at the front marker exactly as a window-shopper's does.
        let at = Vec3::new(line.door.x, 0.0, line.door.y);
        for (entity, transform, pedestrian, archetype) in &candidates {
            if pedestrian.panic > 0.0 {
                continue;
            }
            if transform.translation.xz().distance(line.door) > LURE_RANGE {
                continue;
            }
            if rng.random::<f32>() > LURE_CHANCE * archetype.nosiness() * dt {
                continue;
            }
            lured += 1;
            commands.entity(entity).insert(Errand {
                at: at.with_y(transform.translation.y),
                place: *place,
                inside: true,
                outward: line.outward,
                until: now + LURE_PATIENCE,
            });
        }
    }
    queues.tally.lured += lured;
}

/// The head is served, goes in, and the line shuffles up.
///
/// The going-in is the same despawn `ai::errands` has always done at a
/// doorway — a body parked inside a streamed room is a body standing in a
/// field — so a queue costs the population budget nothing it was not already
/// paying, it only makes it wait.
fn serve_the_head(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<AudioRng>,
    mut queues: ResMut<Queues>,
    mut standing: Query<(&Transform, &mut Mood), With<Queueing>>,
) {
    let dt = time.delta_secs();
    let mut served = 0u32;
    for line in queues.lines.values_mut() {
        line.stalled += dt;
        // Nothing moves while somebody is standing in front of the head.
        // An outrage the queue can simply wait out is not an outrage.
        if line.outrage > 0.0 {
            line.outrage -= dt;
            continue;
        }
        let Some(&head) = line.members.first() else {
            continue;
        };
        // The clock only starts once they are actually at the door: a head
        // still walking up from the pavement is not being served, and
        // without this the second in line is served through the first.
        let Ok((transform, _)) = standing.get(head) else {
            continue;
        };
        if transform.translation.xz().distance(line.slot(0)) > SLOT * 2.0 {
            continue;
        }
        line.serving -= dt;
        if line.serving > 0.0 {
            continue;
        }

        commands.entity(head).try_despawn();
        served += 1;
        line.members.remove(0);
        line.serving = rng.random_range(SERVICE.0..SERVICE.1);
        line.stalled = 0.0;
        // Everybody moves up one, and moving up one is the only good thing
        // that happens to anybody in a queue.
        for member in &line.members {
            if let Ok((_, mut mood)) = standing.get_mut(*member) {
                mood.value = (mood.value + SHUFFLE_CHEER).clamp(-1.0, 1.0);
            }
        }
    }
    queues.tally.served += served;
}

/// The waiting, what it costs, and giving up.
///
/// The head is exempt: their waiting is over, they are being dealt with.
/// Everybody behind them pays by the second at a rate their own fuse sets,
/// which is why a line of ordinary citizens simmers and a line with a
/// Wutbürger in it is a different colour within half a minute.
fn lose_patience(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<AudioRng>,
    mut queues: ResMut<Queues>,
    mut standing: Query<(&mut Mood, &Temperament, &Archetype, &Queueing)>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let mut balked = 0u32;
    for line in queues.lines.values() {
        for (slot, member) in line.members.iter().enumerate() {
            let Ok((mut mood, temper, archetype, queueing)) = standing.get_mut(*member) else {
                continue;
            };
            // The head is exempt from the waiting, not from the deadline.
            // Their waiting really is over — they are being dealt with — but
            // a head who cannot reach the door is being dealt with by
            // nobody, and without a deadline they stand there for ever and
            // the line behind them balks one by one until the shop has a
            // permanent customer nobody is serving.
            if slot > 0 {
                mood.value = (mood.value - WAIT_STING * (0.5 + temper.fuse) * dt).clamp(-1.0, 1.0);

                // Craning past the person in front, which is the one thing a
                // queue does with its head and reads from across a street.
                if rng.random::<f32>() < CRANE_CHANCE * dt {
                    let at = line.slot(0);
                    commands.entity(*member).insert(Attention::to(
                        Vec3::new(at.x, LOOK_AT_FACE, at.y),
                        now,
                        CRANE,
                    ));
                }
            }

            // A short fuse and a thin reason for being here both run out
            // early; either one alone is not enough to walk away over.
            if now - queueing.since > patience(temper, *archetype) {
                balked += 1;
                mood.value = (mood.value - BALK_STING).clamp(-1.0, 1.0);
                // Only the component comes off here. Leaving the line itself
                // to `reconcile_lines` means there is exactly one way out of
                // a queue, and it is the one every other system uses.
                commands
                    .entity(*member)
                    .remove::<Queueing>()
                    .insert(Composure {
                        left: rng.random_range(20.0..45.0),
                    });
            }
        }
    }
    queues.tally.balked += balked;
}

/// Standing in it: the slot, the shuffle and the facing.
///
/// The head faces the door; everybody else faces the back of the person in
/// front. That is not a detail — a huddle of people all facing the door is a
/// crowd, and a column of people all facing one way is a queue, and the
/// difference is entirely in this one branch.
fn hold_the_line(
    queues: Res<Queues>,
    mut standing: Query<(&mut Transform, &mut Bouncer), With<Queueing>>,
) {
    for line in queues.lines.values() {
        for (slot, member) in line.members.iter().enumerate() {
            let Ok((mut transform, mut bouncer)) = standing.get_mut(*member) else {
                continue;
            };
            let want = line.slot(slot);
            let gap = want - transform.translation.xz();
            bouncer.desired = if gap.length() > SLOT {
                gap.normalize_or_zero() * SHUFFLE
            } else {
                Vec2::ZERO
            };
            let facing = if slot == 0 {
                -line.outward
            } else {
                -line.along
            };
            if let Ok(facing) = Dir2::new(facing) {
                transform.rotation =
                    Quat::from_rotation_y(crate::vehicle::spawn::heading_towards(*facing));
            }
        }
    }
}

/// Somebody is pushing in.
///
/// The one thing the player can do to this city that involves no launching,
/// no taunting and no vehicle at all, and it gets a stronger reaction than
/// most of the things that do. A grace period first, because a pavement runs
/// past every door in town and walking down it is not a provocation — the
/// offence is *standing* in front of the head, which is a decision.
///
/// Both `Transform` accesses are reads: the glare is a head turn, inserted
/// through `Commands`, and the bodies are left to `hold_the_line`. Two
/// queries in one system may not both touch a component if either is
/// mutable, and a queue full of people is exactly where that panic would be
/// found at the worst moment.
fn mind_the_queue(
    mut commands: Commands,
    time: Res<Time>,
    mut queues: ResMut<Queues>,
    players: Query<&Transform, (With<crate::player::on_foot::Player>, Without<Pedestrian>)>,
    mut standing: Query<(&Transform, &mut Mood), With<Queueing>>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();

    // First, anybody who walked in at the front since the last frame. This
    // runs whether or not there is a player anywhere near: the city does
    // this to itself, which is the whole point of it being in the cast
    // tables rather than on the controls.
    for line in queues.lines.values_mut() {
        let Some(jumper) = line.jumped.take() else {
            continue;
        };
        let Some(slot) = line.members.iter().position(|member| *member == jumper) else {
            continue;
        };
        let Ok((at, _)) = standing.get(jumper) else {
            continue;
        };
        let face = at.translation.with_y(at.translation.y + LOOK_AT_FACE);
        // Only the people behind them. The head neither lost anything nor
        // noticed, which is exactly how it goes.
        for member in &line.members[slot + 1..] {
            let Ok((_, mut mood)) = standing.get_mut(*member) else {
                continue;
            };
            mood.value = (mood.value - BARGE_STING).clamp(-1.0, 1.0);
            commands
                .entity(*member)
                .insert(Attention::to(face, now, 1.6));
        }
    }

    let Ok(player) = players.single() else {
        for line in queues.lines.values_mut() {
            line.intrusion = 0.0;
        }
        return;
    };
    let here = player.translation.xz();
    let face = player
        .translation
        .with_y(player.translation.y + LOOK_AT_FACE);

    for line in queues.lines.values_mut() {
        // Nobody minds being pushed in front of when they are on their own.
        let barging = line.members.len() >= 2 && here.distance(line.slot(0)) < JUMP_RANGE;
        if barging {
            line.intrusion += dt;
        } else {
            line.intrusion = 0.0;
        }
        if line.intrusion < JUMP_GRACE {
            continue;
        }
        line.outrage = OUTRAGE;
        for member in &line.members {
            let Ok((_, mut mood)) = standing.get_mut(*member) else {
                continue;
            };
            mood.value = (mood.value - JUMP_STING * dt).clamp(-1.0, 1.0);
            commands
                .entity(*member)
                .insert(Attention::to(face, now, 0.8));
        }
    }
}

/// The door's side of an errand that ends in a queue, called from
/// `ai::errands` when somebody arrives at a front meaning to go in.
///
/// Returns whether they got a place in the line. A refusal is not a failure:
/// six people outside a bakery is a reason to keep walking, and the caller
/// treats it as one.
pub fn arrive(
    commands: &mut Commands,
    queues: &mut Queues,
    entity: Entity,
    errand: &Errand,
    now: f32,
    barge: bool,
) -> bool {
    if !queues.join(entity, errand.place, errand.at, errand.outward, barge) {
        queues.tally.refused += 1;
        return false;
    }
    queues.tally.joined += 1;
    commands.entity(entity).insert(Queueing {
        place: errand.place,
        since: now,
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_of(count: usize) -> Queues {
        let mut queues = Queues::default();
        for index in 0..count {
            queues.join(
                Entity::from_raw_u32(index as u32 + 1).unwrap(),
                PlaceId(2),
                Vec3::new(10.0, 2.3, -4.0),
                Vec2::new(0.0, 1.0),
                false,
            );
        }
        queues
    }

    #[test]
    fn a_line_runs_along_the_wall_and_not_out_into_the_road() {
        // The failure this catches builds, runs, and puts five citizens in
        // single file across the carriageway: `along` taken as `outward`.
        let queues = line_of(3);
        let line = &queues.lines[&PlaceId(2)];
        assert!(
            line.along.dot(line.outward).abs() < 1e-5,
            "the queue runs {:?} out of a door facing {:?}",
            line.along,
            line.outward
        );
        assert!((line.along.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_head_stands_at_the_door_and_everybody_else_behind_it() {
        let queues = line_of(4);
        let line = &queues.lines[&PlaceId(2)];
        assert_eq!(line.slot(0), line.door);
        for index in 1..4 {
            let ahead = line.slot(index - 1);
            let here = line.slot(index);
            assert!(
                (here.distance(ahead) - PITCH).abs() < 1e-5,
                "slot {index} stands {} m behind slot {}",
                here.distance(ahead),
                index - 1
            );
            assert!(
                here.distance(line.door) > ahead.distance(line.door),
                "slot {index} is nearer the door than the one in front of it"
            );
        }
    }

    #[test]
    fn a_line_stops_taking_people_and_never_takes_the_same_one_twice() {
        let mut queues = line_of(MAX);
        assert_eq!(queues.standing(), MAX);
        // One more citizen, and the same citizen again.
        assert!(!queues.join(
            Entity::from_raw_u32(99).unwrap(),
            PlaceId(2),
            Vec3::ZERO,
            Vec2::Y,
            false
        ));
        assert!(!queues.join(
            Entity::from_raw_u32(1).unwrap(),
            PlaceId(2),
            Vec3::ZERO,
            Vec2::Y,
            false
        ));
        assert_eq!(queues.standing(), MAX);
    }

    #[test]
    fn two_doors_keep_two_lines() {
        let mut queues = line_of(2);
        queues.join(
            Entity::from_raw_u32(50).unwrap(),
            PlaceId(7),
            Vec3::new(-30.0, 2.3, 8.0),
            Vec2::new(1.0, 0.0),
            false,
        );
        assert_eq!(queues.lines(), 2);
        assert_eq!(queues.len_at(PlaceId(2)), 2);
        assert_eq!(queues.len_at(PlaceId(7)), 1);
        assert_eq!(queues.len_at(PlaceId(11)), 0);
    }

    #[test]
    fn which_way_a_line_leans_is_a_fact_about_the_door() {
        // A queue that re-derived its side from a draw would lean a
        // different way every time its chunk came back, which reads as the
        // whole line teleporting across the shopfront.
        let door = Vec3::new(4.0, 2.3, 9.0);
        let outward = Vec2::new(0.0, -1.0);
        let mut first = Queues::default();
        first.join(
            Entity::from_raw_u32(1).unwrap(),
            PlaceId(4),
            door,
            outward,
            false,
        );
        let mut again = Queues::default();
        again.join(
            Entity::from_raw_u32(8).unwrap(),
            PlaceId(4),
            door,
            outward,
            false,
        );
        assert_eq!(
            first.lines[&PlaceId(4)].along,
            again.lines[&PlaceId(4)].along
        );
        // And the two parities lean opposite ways, so a street does not fold
        // all of its queues towards the same corner.
        let mut odd = Queues::default();
        odd.join(
            Entity::from_raw_u32(1).unwrap(),
            PlaceId(5),
            door,
            outward,
            false,
        );
        assert_eq!(
            first.lines[&PlaceId(4)].along,
            -odd.lines[&PlaceId(5)].along
        );
    }

    #[test]
    fn the_nosiest_archetypes_are_not_the_most_patient_ones() {
        // The joke has to survive a retune: the people a queue attracts are
        // not the people who stay in it. If those two tables ever agree, the
        // queue stops churning and just grows.
        assert!(Archetype::Wutbuerger.nosiness() > Archetype::Everyday.nosiness());
        assert!(Archetype::Wutbuerger.patience() < Archetype::Everyday.patience());
        // And the one archetype that never notices anything stays immune.
        assert_eq!(Archetype::Headphones.nosiness(), 0.0);
    }

    /// The five, in the order their fuses run.
    fn every_temper() -> [Temperament; 5] {
        [
            Temperament::serene(),
            Temperament::easygoing(),
            Temperament::ordinary(),
            Temperament::touchy(),
            Temperament::ragemonger(),
        ]
    }

    #[test]
    fn the_queues_cannot_swallow_the_street() {
        // The lure is positive feedback and the cap is the only thing
        // standing between it and an empty pavement. Sanity, not precision:
        // at the default crowd a handful of full lines must still leave most
        // of the city walking about.
        let population = crate::core::config::GameConfig::default().crowd.population;
        let ceiling = (population as f32 * CROWD_SHARE) as usize;
        assert!(
            ceiling >= MAX,
            "the cap of {ceiling} cannot hold one full line of {MAX}"
        );
        assert!(
            ceiling * 3 < population,
            "{ceiling} of {population} citizens may be standing in lines"
        );
    }

    #[test]
    fn a_barger_goes_in_at_second_and_never_displaces_the_head() {
        // Displacing the head is the one thing nobody has the nerve for, and
        // mechanically it would also serve somebody mid-transaction to the
        // wrong person. Second is the real move anyway.
        let mut queues = line_of(3);
        let cheeky = Entity::from_raw_u32(77).unwrap();
        assert!(queues.join(cheeky, PlaceId(2), Vec3::ZERO, Vec2::Y, true));
        let line = &queues.lines[&PlaceId(2)];
        assert_eq!(line.members[0], Entity::from_raw_u32(1).unwrap());
        assert_eq!(line.members[1], cheeky);
        assert_eq!(line.jumped, Some(cheeky), "nobody noticed");
        assert_eq!(line.members.len(), 4);
    }

    #[test]
    fn there_is_nothing_to_push_in_front_of_in_a_line_of_one() {
        // Walking up beside one person is not pushing in, and a queue that
        // took offence at it would take offence at every second arrival.
        let mut queues = line_of(1);
        let cheeky = Entity::from_raw_u32(77).unwrap();
        assert!(queues.join(cheeky, PlaceId(2), Vec3::ZERO, Vec2::Y, true));
        let line = &queues.lines[&PlaceId(2)];
        assert_eq!(line.members[1], cheeky, "they went to the back");
        assert_eq!(line.jumped, None, "a queue of one took offence");
    }

    #[test]
    fn pushing_in_is_a_mood_and_not_a_personality() {
        // Both halves have to be true at once. The same flummi waits its
        // turn on a good day, and the polite half of the cast never does it
        // however bad a day they are having — a city where anybody might
        // push in is a city with no queues in it.
        assert!(barges(Archetype::Hooligan, -0.8, 0.1));
        assert!(!barges(Archetype::Hooligan, 0.6, 0.1), "on a good day");
        for archetype in [
            Archetype::CaneUser,
            Archetype::Wheelchair,
            Archetype::Missionary,
            Archetype::Shy,
            Archetype::Beggar,
        ] {
            assert!(
                !barges(archetype, -1.0, 0.0),
                "{archetype:?} pushed in at the worst moment of their life"
            );
        }
        // And it stays rare even among the ones who do it: the whole cast at
        // its worst, over a sweep of the roll, must not be most of them.
        let pushy: f32 = Archetype::ALL
            .iter()
            .map(|a| (a.cheek() * BARGE_ODDS).min(1.0))
            .sum();
        assert!(
            pushy < Archetype::ALL.len() as f32 * 0.15,
            "{pushy:.2} of {} archetypes push in",
            Archetype::ALL.len()
        );
    }

    #[test]
    fn nobody_balks_before_the_shop_could_have_served_them() {
        // The two ends of the patience table, against the one number a queue
        // actually takes. Too short and citizens walk away from queues that
        // were about to serve them, which reads as the door not working; too
        // long at the other end and a line never churns at all.
        let full_line = SERVICE.1 * MAX as f32;
        let mut shortest = f32::MAX;
        let mut longest: f32 = 0.0;
        for temper in every_temper() {
            for archetype in Archetype::ALL {
                let waits = patience(&temper, archetype);
                assert!(waits.is_finite() && waits > 0.0, "{archetype:?}");
                shortest = shortest.min(waits);
                longest = longest.max(waits);
            }
        }
        assert!(
            shortest > SERVICE.1 * 1.5,
            "the least patient citizen gives up after {shortest:.0}s, before \
             one turn at the counter has finished"
        );
        assert!(
            longest > full_line,
            "nobody in the city outlasts a full line, which takes {full_line:.0}s"
        );
    }

    #[test]
    fn patience_is_shorter_the_shorter_the_fuse() {
        let calm = patience(&Temperament::serene(), Archetype::Everyday);
        let cross = patience(&Temperament::ragemonger(), Archetype::Wutbuerger);
        assert!(
            calm > cross * 2.5,
            "a serene citizen waits {calm:.0}s and a Wutbürger {cross:.0}s"
        );
    }

    #[test]
    fn a_queue_sours_a_ragemonger_and_leaves_a_serene_citizen_alone() {
        // The whole reason the drain is a drain: five dispositions in the
        // same line come out five different colours, and the spread is the
        // recovery rates rather than anything this module chose. A retune
        // that flattens it turns every queue in the city one shade of grey.
        let settled: Vec<f32> = every_temper().iter().map(souring).collect();
        assert!(
            settled[0] > 0.3,
            "a serene flummi leaves the queue at {:.2}",
            settled[0]
        );
        assert!(
            settled[4] < -0.85,
            "a Wutbürger leaves the queue at {:.2}",
            settled[4]
        );
        for pair in settled.windows(2) {
            assert!(
                pair[0] > pair[1],
                "the queue treats {:.2} and {:.2} the same way",
                pair[0],
                pair[1]
            );
        }
    }
}
