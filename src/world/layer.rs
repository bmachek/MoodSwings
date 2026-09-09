//! How high everything lying on the ground is laid, in one table.
//!
//! Nothing here is geometry. It is the order the flat things stack in — grass,
//! yard, carriageway, junction, paint, pavement — and the reason it is a module
//! rather than a constant beside each spawner is that the spawners could not
//! see each other. A junction plate was laid at six millimetres in
//! `streetside` and a building's back yard at six millimetres in `buildings`,
//! written eight hundred lines apart, and where a yard reached a junction the
//! two were *exactly* coplanar.
//!
//! ## Why an exact tie is the whole of the bug
//!
//! The depth buffer is not the problem and never was. The camera is reverse-Z
//! into `Depth32Float` with a near plane of a tenth of a metre, which resolves
//! about two microns at twenty metres and sixty at nine hundred. Two
//! millimetres of separation is a thousand times what it needs.
//!
//! What *is* the problem is a metre being an `f32`. Bevy transforms absolute
//! world positions through one `f32` matrix, and this town runs from −1486 m to
//! +1714 m: the spacing between representable coordinates is 48 µm out at the
//! Altstadt and 204 µm at the edge of the map. The old tiebreak between two
//! carriageways was `(id % 8) * 0.00005` — fifty microns, which is *one* of
//! those steps before the view matrix has cancelled anything. So the ordering
//! it intended did not survive being drawn, and two ribbons came out at the
//! same depth.
//!
//! Two surfaces at the same depth are then decided by the sub-pixel jitter TAA
//! applies to the projection, which is a different jitter every frame. The
//! winner changes at frame rate, the history rejects the change as new
//! geometry and passes it through at full strength, and contrast-adaptive
//! sharpening puts an edge on the result. That is the flicker: not a depth
//! buffer running out of bits, but a tie being broken by a dice roll.
//!
//! So the rule this module exists to keep is: **nothing that can overlap
//! anything may be laid at the same height as it, and no two heights may be
//! closer than a millimetre.** A millimetre is five times the coordinate noise
//! at the far corner of the map and is invisible against a 280 mm kerb.
//!
//! ## The order, and why it is that order
//!
//! Off the ground upward: forecourt, back yard, carriageway, junction, paint.
//! Wherever a yard and a street want the same square metre the street wins,
//! which is the way round that cannot look like a bug — a gravel rectangle
//! over a road is far more obvious than a missing corner of yard. The junction
//! plate sits over the carriageways rather than under them, which is the one
//! reversal from the way this used to work: the plate is now cut to the
//! crossing instead of being a square thrown over it, so what a junction
//! should show is *one* surface, not four ribbons overlapping at angles with
//! a different paving on each.
//!
//! The whole stack is under forty millimetres, which matters because
//! traffic rides on the ground collider rather than on the visible road: a car
//! is drawn with its tyres at the collider's top face, so every millimetre the
//! carriageway is raised is a millimetre of tyre buried in it.

/// How far apart two things in this stack are ever put.
///
/// A millimetre: five times the spacing between representable coordinates at
/// the far corner of this town, twenty times it in the Altstadt, and a
/// twentieth of the depth of the joint between two setts. Nothing at this
/// scale is visible; everything at this scale is decidable.
pub const STEP: f32 = 0.001;

/// Where the grass is. Everything here is measured from it, in whole [`STEP`]s
/// — whole, so that "is this height already taken" is a question about
/// integers rather than about how two floats round.
pub const GROUND: f32 = 0.0;

/// A gravelled forecourt in a hole in the frontage, and how many slots it has.
const FORECOURT_STEP: u32 = 3;
pub const FORECOURT_SLOTS: u32 = 5;

/// The back yard behind a building.
const YARD_STEP: u32 = 9;
pub const YARD_SLOTS: u32 = 4;

/// The lowest a carriageway is laid, how many steps each width band claims
/// over the one below it, and how many slots a band holds.
const ROAD_STEP: u32 = 14;
const ROAD_BAND: u32 = 6;
pub const ROAD_SLOTS: u32 = 6;

/// How many width bands there are.
///
/// Banded rather than continuous, which is the mistake the first version of
/// this made: `rank * ROAD_RANK` over a continuous width put a 7.5 m street
/// and a 7.8 m one a fortieth of a millimetre apart, which is not a separation
/// at all — those two are fifty-nine per cent of the streets in this town.
/// Three bands, each a clear six steps from the next.
const ROAD_BANDS: u32 = 3;

/// The width, in metres, at which a street is in the top band.
pub const WIDEST_STREET: f32 = 18.0;

/// The paving of a crossing, over every arm that meets there.
const JUNCTION_STEP: u32 = 33;

/// Paint on the road.
const PAINT_STEP: u32 = 35;
pub const PAINT_SLOTS: u32 = 5;

/// The top of a kerb: the surface a pedestrian walks on.
const FOOTWAY_STEP: u32 = 4;
pub const FOOTWAY_SLOTS: u32 = 6;

pub const FORECOURT: f32 = FORECOURT_STEP as f32 * STEP;
pub const YARD: f32 = YARD_STEP as f32 * STEP;
pub const ROAD_BED: f32 = ROAD_STEP as f32 * STEP;
pub const JUNCTION: f32 = JUNCTION_STEP as f32 * STEP;
pub const PAINT: f32 = PAINT_STEP as f32 * STEP;
pub const FOOTWAY: f32 = super::buildings::SIDEWALK_HEIGHT + FOOTWAY_STEP as f32 * STEP;

/// Which slot of a layer something takes, so two of the same kind that overlap
/// cannot be laid at the same height.
///
/// The count is deliberately small and the input is deliberately an id rather
/// than a hash: consecutive edges of one polyline street are the pair that
/// overlaps most often — a ribbon runs half its own width past both of its
/// nodes — and consecutive ids land in different slots, which is exactly the
/// case that has to work.
pub fn slot(id: u32, slots: u32) -> f32 {
    (id % slots.max(1)) as f32 * STEP
}

/// Which width band a street is in.
fn band(width: f32) -> u32 {
    let rank = (width / WIDEST_STREET).clamp(0.0, 1.0);
    (rank * (ROAD_BANDS - 1) as f32).round() as u32
}

/// How high one street's carriageway is laid.
///
/// Width decides the band, and it is not a trick to break the tie — it is what
/// a resurfacing gang does: the main road runs through and the side road stops
/// at it. The slot inside the band is the edge's own index, so two streets of
/// the same width still cannot fight.
pub fn carriageway(width: f32, id: u32) -> f32 {
    (ROAD_STEP + band(width) * ROAD_BAND) as f32 * STEP + slot(id, ROAD_SLOTS)
}

/// The tallest a carriageway can be, which is what everything laid over one
/// has to clear.
pub fn carriageway_ceiling() -> f32 {
    (ROAD_STEP + (ROAD_BANDS - 1) * ROAD_BAND + ROAD_SLOTS - 1) as f32 * STEP
}

// The order the layers stack in, checked where it is written down rather than
// in a test: every one of these is a constant, and a constant that is wrong
// should not compile.
const _: () = assert!(FORECOURT_STEP + FORECOURT_SLOTS <= YARD_STEP);
const _: () = assert!(YARD_STEP + YARD_SLOTS <= ROAD_STEP);
const _: () = assert!(ROAD_STEP + (ROAD_BANDS - 1) * ROAD_BAND + ROAD_SLOTS <= JUNCTION_STEP);
const _: () = assert!(JUNCTION_STEP < PAINT_STEP);
// A band has to be wide enough to hold its slots, or the top of one width band
// lands on the bottom of the next.
const _: () = assert!(ROAD_SLOTS <= ROAD_BAND);
// And the whole stack has to stay under four centimetres, because a car rides
// on the ground collider rather than on the road it is drawn over: every
// millimetre the carriageway is raised is a millimetre of tyre buried in it.
const _: () = assert!((PAINT_STEP + PAINT_SLOTS - 1) as f32 * STEP < 0.04);

#[cfg(test)]
mod tests {
    use super::*;

    /// Every height this module can hand out, in whatever order.
    fn every_height() -> Vec<f32> {
        let mut heights = vec![GROUND, JUNCTION];
        heights.extend((0..FORECOURT_SLOTS).map(|i| FORECOURT + slot(i, FORECOURT_SLOTS)));
        heights.extend((0..YARD_SLOTS).map(|i| YARD + slot(i, YARD_SLOTS)));
        heights.extend((0..PAINT_SLOTS).map(|i| PAINT + slot(i, PAINT_SLOTS)));
        // Every width a decimetre apart from nothing to the widest street
        // there is, against every slot.
        for tenths in 0..=200u32 {
            for id in 0..ROAD_SLOTS {
                heights.push(carriageway(tenths as f32 * 0.1, id));
            }
        }
        heights
    }

    /// The layers have to stay in the order the doc comment claims, or a yard
    /// covers a road.
    #[test]
    fn the_stack_is_laid_from_the_ground_up() {
        let forecourt_top = FORECOURT + slot(FORECOURT_SLOTS - 1, FORECOURT_SLOTS);
        let yard_top = YARD + slot(YARD_SLOTS - 1, YARD_SLOTS);
        assert!(
            forecourt_top < YARD,
            "a forecourt at {forecourt_top} reaches the yards at {YARD}"
        );
        assert!(
            yard_top < ROAD_BED,
            "a yard at {yard_top} reaches the road at {ROAD_BED}"
        );
        assert!(
            carriageway_ceiling() < JUNCTION,
            "a carriageway at {} reaches the junction plate at {JUNCTION}",
            carriageway_ceiling()
        );
        assert!(
            PAINT + slot(PAINT_SLOTS - 1, PAINT_SLOTS) < FOOTWAY,
            "the paint is over the kerb"
        );
    }

    /// The rule the module exists for: no two heights in the whole stack are
    /// closer than most of a millimetre, unless they are the same height.
    #[test]
    fn nothing_in_the_stack_lies_within_a_millimetre_of_anything_else() {
        let mut heights = every_height();
        heights.sort_by(f32::total_cmp);
        for pair in heights.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                gap == 0.0 || gap >= STEP * 0.9,
                "{} and {} are {gap} apart, which is inside the coordinate noise",
                pair[0],
                pair[1]
            );
        }
    }

    /// The two commonest widths in Landshut are 7.5 m and 7.8 m, and the first
    /// version of this put them a fortieth of a millimetre apart.
    #[test]
    fn two_streets_of_nearly_the_same_width_do_not_land_on_each_other() {
        for id in 0..ROAD_SLOTS {
            let mine = carriageway(7.5, id);
            let theirs = carriageway(7.8, id);
            assert_eq!(
                mine, theirs,
                "7.5 m and 7.8 m are the same kind of street and should share a band"
            );
        }
        for id in 0..ROAD_SLOTS {
            let mine = carriageway(7.5, id);
            let theirs = carriageway(7.8, id + 1);
            assert!(
                (mine - theirs).abs() >= STEP * 0.9,
                "two streets a slot apart are only {} apart",
                (mine - theirs).abs()
            );
        }
    }

    /// Consecutive segments of one street always overlap: a ribbon runs half
    /// its own width past both of its nodes.
    #[test]
    fn two_segments_of_one_street_are_never_laid_at_the_same_height() {
        for id in 0..64u32 {
            let mine = carriageway(7.5, id);
            let next = carriageway(7.5, id + 1);
            assert!(
                (mine - next).abs() >= STEP * 0.9,
                "segments {id} and {} of one street are {} apart",
                id + 1,
                (mine - next).abs()
            );
        }
    }

    /// And the whole stack has to stay thin, because a car rides on the ground
    /// collider rather than on the road it is drawn over: every millimetre the
    /// carriageway is raised is a millimetre of tyre buried in it.
    #[test]
    fn a_tyre_is_never_buried_in_the_road_it_stands_on() {
        let top = every_height().into_iter().fold(0.0f32, f32::max);
        assert!(
            top < 0.04,
            "the road stack has grown to {top} m, which a wheel would show"
        );
    }
}
