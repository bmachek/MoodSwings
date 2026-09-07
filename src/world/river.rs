//! The canal, its bridges, and the water's opinion of visitors.
//!
//! The city's first water. `citygen::canal_for` surrenders one minor street
//! of the grid to it; this module makes that legible: water between the
//! kerbs, railings where every crossing street becomes a bridge, and the
//! physics of falling in.
//!
//! Three decisions carry the module:
//!
//! * **The kerbs are the quay.** The water is cut wider than the old
//!   carriageway, so it laps two metres into each neighbouring block's kerb
//!   slab. The slab is 28 cm proud of the road; the water sits at 6 cm; the
//!   exposed face reads as an embankment wall and nobody built one.
//! * **A bridge is a road that kept its asphalt.** The water is spawned in
//!   segments *between* crossing streets, so every crossing stays plain
//!   drivable ground at road level — flush, no ramp, no seam — and a pair
//!   of railings is all it takes to make it read as a bridge.
//! * **The water is a trampoline.** A thin static collider floats just
//!   under the surface with restitution turned all the way up: drive in and
//!   the river throws you back out. Rivers here have the same sense of
//!   humour as everything else; drowning is not in this game's register.
//!
//! Spawned once at startup, not streamed: the whole canal is a few dozen
//! entities, and a river that blinks out at the stream radius would be a
//! hole in the world's most legible landmark.

use avian3d::prelude::*;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::buildings::SIDEWALK_HEIGHT;
use super::citygen::CityLayout;

/// The water surface, above the road and below the kerb top.
const WATER_LEVEL: f32 = 0.06;
/// The trampoline's top, just under the surface.
const BOUNCE_LEVEL: f32 = 0.02;
/// How much livelier the water is than the world default. All the way up:
/// the river's whole role is throwing things back.
const WATER_RESTITUTION: f32 = 0.95;
/// The railings either side of a bridge deck.
const RAIL_HEIGHT: f32 = 0.95;
const RAIL_THICK: f32 = 0.14;

/// Raises the canal for a generated layout, if it has one.
pub fn spawn(
    commands: &mut Commands,
    layout: &CityLayout,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let Some(canal) = layout.canal else {
        return;
    };
    let water = materials.add(StandardMaterial {
        base_color: Color::srgba(0.16, 0.34, 0.42, 0.92),
        perceptual_roughness: 0.12,
        metallic: 0.35,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let steel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.40, 0.45),
        perceptual_roughness: 0.5,
        metallic: 0.6,
        ..default()
    });
    let quad = meshes.add(Plane3d::default().mesh().size(1.0, 1.0));
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    // The crossing streets are the other axis' whole list: a grid street
    // runs the full extent, so every one of them crosses the canal.
    let crossings = if canal.along_z {
        &layout.z_streets
    } else {
        &layout.x_streets
    };
    let extent = layout.half_extent;

    // A point on the canal: `along` down the waterway, `across` over it.
    let at = |along: f32, across: f32, y: f32| {
        if canal.along_z {
            Vec3::new(canal.center + across, y, along)
        } else {
            Vec3::new(along, y, canal.center + across)
        }
    };
    let sized = |length: f32, breadth: f32, height: f32| {
        if canal.along_z {
            Vec3::new(breadth, height, length)
        } else {
            Vec3::new(length, height, breadth)
        }
    };

    // Water, segmented between the crossings so each crossing keeps its
    // asphalt — that asphalt *is* the bridge deck.
    let mut shore = -extent;
    let mut segments: Vec<(f32, f32)> = Vec::new();
    for street in crossings {
        let near = street.center - street.width * 0.5;
        if near > shore + 0.5 {
            segments.push((shore, near));
        }
        shore = shore.max(street.center + street.width * 0.5);
    }
    if extent > shore + 0.5 {
        segments.push((shore, extent));
    }
    for (from, to) in &segments {
        let middle = (from + to) * 0.5;
        let length = to - from;
        commands.spawn((
            Mesh3d(quad.clone()),
            MeshMaterial3d(water.clone()),
            Transform::from_translation(at(middle, 0.0, WATER_LEVEL)).with_scale(sized(
                length,
                canal.width,
                1.0,
            )),
            NotShadowCaster,
        ));
    }

    // The trampoline, one collider under the whole run — bridges included,
    // harmlessly: under a bridge it sits beneath the asphalt the cars are
    // actually driving on.
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
        Restitution::new(WATER_RESTITUTION).with_combine_rule(CoefficientCombine::Max),
        Transform::from_translation(at(0.0, 0.0, BOUNCE_LEVEL - 0.5)).with_scale(sized(
            extent * 2.0,
            canal.width,
            1.0,
        )),
    ));

    // Railings: one pair per crossing, spanning the water plus a step of
    // bank on either side. Solid — bouncing off a bridge railing instead of
    // going in is the classic near-miss, and both outcomes are jokes.
    for street in crossings {
        for side in [-1.0f32, 1.0] {
            let offset = side * (street.width * 0.5 + RAIL_THICK * 0.5 + 0.15);
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(steel.clone()),
                Transform::from_translation(at(
                    street.center + offset,
                    0.0,
                    SIDEWALK_HEIGHT + RAIL_HEIGHT * 0.5,
                ))
                .with_scale(sized(RAIL_THICK, canal.width + 2.4, RAIL_HEIGHT)),
                RigidBody::Static,
                Collider::cuboid(1.0, 1.0, 1.0),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::citygen;

    #[test]
    fn the_canal_is_a_minor_street_and_its_edges_are_gone() {
        for seed in [0xA17E_5EED_u64, 2709413613, 7] {
            let layout = citygen::generate(seed, 1000.0, crate::core::config::CityStyle::Generisch);
            let Some(canal) = layout.canal else {
                panic!("seed {seed:#x} dug no canal");
            };
            let list = if canal.along_z {
                &layout.x_streets
            } else {
                &layout.z_streets
            };
            assert!(
                !list[canal.index].arterial,
                "seed {seed:#x} drowned an arterial"
            );
            assert_eq!(list[canal.index].center, canal.center);
            assert!(canal.width > list[canal.index].width);

            // No edge of the graph may run along the canal's centreline.
            for edge in layout.graph.edges() {
                let (a, b) = (layout.graph.node(edge.a).pos, layout.graph.node(edge.b).pos);
                let along = if canal.along_z {
                    (a.x - canal.center).abs() < 0.01 && (b.x - canal.center).abs() < 0.01
                } else {
                    (a.y - canal.center).abs() < 0.01 && (b.y - canal.center).abs() < 0.01
                };
                assert!(!along, "seed {seed:#x} still drives on the water");
            }
        }
    }

    #[test]
    fn the_water_stops_at_every_bridge() {
        // Rebuild the segmentation the spawner uses and hold it against the
        // crossing streets: no water inside any crossing's carriageway.
        let layout = citygen::generate(
            0xA17E_5EED,
            1000.0,
            crate::core::config::CityStyle::Generisch,
        );
        let canal = layout.canal.expect("this seed digs a canal");
        let crossings = if canal.along_z {
            &layout.z_streets
        } else {
            &layout.x_streets
        };
        let mut shore = -layout.half_extent;
        let mut segments: Vec<(f32, f32)> = Vec::new();
        for street in crossings {
            let near = street.center - street.width * 0.5;
            if near > shore + 0.5 {
                segments.push((shore, near));
            }
            shore = shore.max(street.center + street.width * 0.5);
        }
        assert!(
            segments.len() >= crossings.len() / 2,
            "the canal is more bridge than water"
        );
        for street in crossings {
            for (from, to) in &segments {
                assert!(
                    *to <= street.center - street.width * 0.5 + 0.6
                        || *from >= street.center + street.width * 0.5 - 0.6,
                    "water floods the crossing at {}",
                    street.center
                );
            }
        }
    }
}
