//! Building vehicle entities and scattering parked ones around the city.

use avian3d::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use rand::RngExt;

use super::controller::{VehicleInput, VehicleState};
use super::spec::{VehicleClass, VehicleSpec, WHEEL_COUNT};
use crate::core::config::GameConfig;
use crate::core::rng::{stream, stream_for};
use crate::player::interact::DrivenBy;
use crate::world::City;

/// Marks a vehicle the player can get into.
#[derive(Component)]
pub struct Vehicle;

/// A vehicle close enough to matter, and therefore simulated.
///
/// Several hundred parked cars are scattered across the city. Simulating all of
/// them means Avian solving hundreds of dynamic bodies and this module casting
/// four suspension rays each, every tick, for cars nobody can see. Distant ones
/// are held static instead, and because they are parked at exactly their
/// settled ride height, switching back to dynamic causes no visible pop.
#[derive(Component)]
pub struct ActiveVehicle;

/// Vehicles within this distance of the camera are simulated.
pub const ACTIVE_RADIUS: f32 = 140.0;

/// Exempts a vehicle from distance-based deactivation.
///
/// Traffic and police manage their own lifetimes and must keep driving out to
/// their own despawn ranges; freezing them the moment they pass the parked-car
/// radius would strand a pursuit the instant it fell behind.
#[derive(Component)]
pub struct AlwaysSimulated;

/// A wheel mesh, positioned each frame from the suspension state.
#[derive(Component)]
pub struct WheelVisual(pub usize);

/// Where a car stops being worth its fittings, in metres.
///
/// A car is not one mesh. It is bodywork, glazing, a liner, four tyres, four
/// rims, two bumpers, a grille, two mirrors in three pieces each, a tailpipe,
/// two plates, four lamps and a cabin full of seats — a little over thirty
/// meshes, and there are two and a half thousand parked cars in the city. Most
/// of them are far enough away that a mirror shell is under two pixels across.
///
/// So the fittings carry a range and the bodywork does not. What survives to
/// any distance is the silhouette: shell, sill, glazing, liner and tyres. That
/// is the shape of a car, and the shape is all that is left of one at ninety
/// metres anyway.
///
/// The lamps are the one fitting deliberately left off this budget, and the
/// reason is the hour rather than the distance: `vehicle::lights` glows every
/// lens in the city after dusk, so a tail lamp at two hundred metres is not a
/// two-pixel box, it is one of the red dots a night street is made of.
const FITTINGS: f32 = 85.0;

/// And where the *inside* of the cabin stops being worth drawing.
///
/// Much nearer, because seats and a steering wheel are seen through tinted
/// glass at a grazing angle: they are gone as a readable thing long before
/// they are gone as pixels.
///
/// The liner is deliberately not on this budget. It is the surface that stops
/// the near glass being a window straight through the car to the street behind
/// — culling it would not save a mesh so much as knock a hole in every distant
/// car in the row.
const CABIN: f32 = 45.0;

/// A tenth of the distance is the crossfade, which at these ranges is a couple
/// of frames of dither on something a few pixels wide.
fn beyond(distance: f32) -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: distance..(distance * 1.12),
        use_aabb: false,
    }
}

/// The range every bolted-on fitting carries: bumpers, grille, mirrors,
/// tailpipe, plates, rims — and, from `vehicle::lights`, the lamps.
pub fn fittings_range() -> VisibilityRange {
    beyond(FITTINGS)
}

/// The range the cabin's furniture carries.
pub fn cabin_range() -> VisibilityRange {
    beyond(CABIN)
}

/// One archetype's bodywork, as loaded meshes.
struct BodyHandles {
    shell: Handle<Mesh>,
    lower: Handle<Mesh>,
    /// The glazing. `None` on a van, which has none of its own.
    cabin: Option<Handle<Mesh>>,
    /// The pressings over the glazing: roof, headers, pillars.
    frame: Option<Handle<Mesh>>,
    /// Glazing lying on the shell, which is how a van gets a windscreen.
    windows: Option<Handle<Mesh>>,
    /// The cabin seen from within, which is what stops the glazing being a
    /// window onto the street on the far side of the car.
    liner: Option<Handle<Mesh>>,
}

#[derive(Resource)]
pub struct VehicleAssets {
    /// One entry per archetype, built once and shared by every car of that
    /// kind — which is also what lets Bevy batch a street full of them.
    bodies: Vec<(VehicleClass, BodyHandles)>,
    tyre_mesh: Handle<Mesh>,
    rim_mesh: Handle<Mesh>,
    tyre: Handle<StandardMaterial>,
    rim: Handle<StandardMaterial>,
    glass: Handle<StandardMaterial>,
    /// A van's glazing, which cannot be seen through because there is nothing
    /// behind it but the outside of the box it is lying on.
    dark_glass: Handle<StandardMaterial>,
    /// Shared by every car: the flake is a property of automotive paint, not
    /// of one car's paint, and the colour that varies is in the material.
    flake: Handle<Image>,
    /// The finishes a car can come off the line in, built once.
    ///
    /// Every colour in this city comes from one of two closed lists — the
    /// street palette in `paint::stock`, and the five archetypes' own liveries
    /// — so a car almost never needs a material of its own. It used to get one
    /// regardless: two and a half thousand parked cars meant two and a half
    /// thousand `StandardMaterial`s that were bit-for-bit copies of twelve, and
    /// a material handle is what Bevy batches by, so a street of identical
    /// white hatchbacks was a draw call each.
    stock: Vec<(LinearRgba, f32, f32, Handle<StandardMaterial>)>,
    trim: super::trim::TrimKit,
}

impl VehicleAssets {
    fn body(&self, class: VehicleClass) -> &BodyHandles {
        self.bodies
            .iter()
            .find(|(c, _)| *c == class)
            .map(|(_, meshes)| meshes)
            .expect("every archetype gets meshes at startup")
    }

    /// The prebuilt material for a finish, if this is one of the stock ones.
    ///
    /// Matched on the colour rather than on a palette index, so a caller that
    /// tints a car by hand — the capture harness does — still shares a material
    /// when it happens to land on a stock colour, and still gets its own when
    /// it does not.
    fn stock_paint(
        &self,
        color: Color,
        metallic: f32,
        age: f32,
    ) -> Option<&Handle<StandardMaterial>> {
        let wanted = LinearRgba::from(color);
        self.stock
            .iter()
            .find(|(color, stock_metallic, stock_age, _)| {
                same_colour(*color, wanted)
                    && (stock_metallic - metallic).abs() < 1e-4
                    && (stock_age - age).abs() < 1e-4
            })
            .map(|(_, _, _, handle)| handle)
    }
}

/// Whether two colours are the same paint.
///
/// A tolerance rather than equality: the colours make a round trip through
/// sRGB and linear, and a finish that misses its own material by one bit would
/// silently put the per-car materials back.
fn same_colour(a: LinearRgba, b: LinearRgba) -> bool {
    (a.red - b.red).abs() < 1e-4
        && (a.green - b.green).abs() < 1e-4
        && (a.blue - b.blue).abs() < 1e-4
}

/// Fraction of its own radius that a tyre is wide.
const TYRE_WIDTH: f32 = 0.66;

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> VehicleAssets {
    let bodies = VehicleClass::ALL
        .into_iter()
        .map(|class| {
            let built = super::body::build(class, &class.spec());
            // Normal maps need a tangent basis, and mikktspace is the one the
            // shader agrees with.
            let mut add = |mesh| meshes.add(crate::world::buildings::with_tangents(mesh));
            (
                class,
                BodyHandles {
                    shell: add(built.shell),
                    lower: add(built.lower),
                    cabin: built.cabin.map(&mut add),
                    frame: built.frame.map(&mut add),
                    windows: built.windows.map(&mut add),
                    liner: built.liner.map(&mut add),
                },
            )
        })
        .collect();

    // Every finish anything in this city is painted: the street palette, plus
    // the five archetypes' own liveries. Deduplicated, because several classes
    // could pick the same colour and two handles for one paint would put the
    // batching back where it was.
    let flake = images.add(super::paint::flake());
    let mut stock: Vec<(LinearRgba, f32, f32, Handle<StandardMaterial>)> = Vec::new();
    for (color, metallic) in super::paint::stock().chain(
        VehicleClass::ALL
            .into_iter()
            .map(|class| class.spec())
            .map(|spec| (spec.body_color, spec.body_metallic)),
    ) {
        let linear = LinearRgba::from(color);
        // Once per condition. Four times the materials and the same number of
        // *batches* per finish, which is what actually costs anything.
        for age in super::paint::AGES {
            if stock.iter().any(|(had, had_metallic, had_age, _)| {
                same_colour(*had, linear) && *had_metallic == metallic && *had_age == age
            }) {
                continue;
            }
            stock.push((
                linear,
                metallic,
                age,
                materials.add(bodywork(color, metallic, age, &flake)),
            ));
        }
    }

    VehicleAssets {
        bodies,
        tyre_mesh: meshes.add(crate::world::buildings::with_tangents(
            super::body::tyre_mesh(TYRE_WIDTH),
        )),
        rim_mesh: meshes.add(crate::world::buildings::with_tangents(
            super::body::rim_mesh(TYRE_WIDTH),
        )),
        tyre: materials.add(StandardMaterial {
            base_color: Color::srgb(0.34, 0.34, 0.36),
            base_color_texture: Some(images.add(super::paint::tyre())),
            normal_map_texture: Some(images.add(super::paint::tyre_normal())),
            perceptual_roughness: 0.94,
            ..default()
        }),
        rim: materials.add(StandardMaterial {
            base_color: Color::srgb(0.82, 0.83, 0.86),
            base_color_texture: Some(images.add(super::paint::rim())),
            normal_map_texture: Some(images.add(super::paint::rim_normal())),
            metallic_roughness_texture: Some(images.add(super::paint::rim_surface())),
            perceptual_roughness: 1.0,
            metallic: 1.0,
            ..default()
        }),
        glass: materials.add(glazing(0.70)),
        // Opaque, and the alpha is the only difference: a van's windscreen is
        // lying on the front of the box rather than set into a hole in it, so
        // what is behind it is not a cab but the outside of the bodywork.
        dark_glass: materials.add(StandardMaterial {
            alpha_mode: AlphaMode::Opaque,
            // Duller than a car's, and the reason is the same as the opacity:
            // at a windscreen's own roughness an opaque pane lying on a van's
            // raked nose is a mirror pointed at the sky, and comes back as a
            // bright panel indistinguishable from the bodywork around it.
            perceptual_roughness: 0.24,
            ..glazing(1.0)
        }),
        flake: flake.clone(),
        stock,
        trim: super::trim::build_kit(meshes, materials, images),
    }
}

/// One panel of bodywork, as a material.
///
/// Pulled out of the spawner so the same recipe serves both the stock finishes
/// built at startup and the one-off a hand-tinted car still needs.
fn bodywork(color: Color, metallic: f32, age: f32, flake: &Handle<Image>) -> StandardMaterial {
    // Car paint is a coloured base under a clear lacquer, and modelling it that
    // way rather than as "shiny metal" is what makes the highlight sit *on* the
    // panel instead of tinting itself the colour of the car.
    let finish = super::paint::finish(color, metallic, age);
    StandardMaterial {
        base_color: finish.base_color,
        perceptual_roughness: finish.perceptual_roughness,
        metallic: finish.metallic,
        clearcoat: finish.clearcoat,
        clearcoat_perceptual_roughness: 0.08,
        // The facets. See `paint::flake` for why this is a normal map and not
        // the anisotropy the plan asked for.
        normal_map_texture: Some(flake.clone()),
        // The loft's UVs run nought to one over the whole car, so the tile has
        // to be brought down to the size of a hand before it is flake rather
        // than dents.
        uv_transform: bevy::math::Affine2::from_scale(super::paint::FLAKE_TILING),
        ..default()
    }
}

/// Automotive glass.
///
/// Not metal, which is what it used to be here. Glass is a dielectric: at
/// normal incidence four percent of the light bounces and the rest goes
/// through, and at a grazing angle almost all of it bounces. That is what makes
/// a windscreen show the cabin from in front and the sky from the side, and it
/// is exactly what `metallic` destroys — a metal *tints* its reflection with
/// its base colour instead of letting anything past it, so a dark blue metal
/// greenhouse is a dark blue mirror at every angle and a lump at all of them.
fn glazing(alpha: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgba(0.038, 0.044, 0.052, alpha),
        // Blended rather than transmissive on purpose. Refraction through a
        // five-millimetre pane at a windscreen's rake displaces the ray behind
        // it by under two millimetres, which is well under a pixel at any
        // distance a car is ever seen from here; what actually reads as glass
        // is the Fresnel and the cabin behind it, and both of those are cheaper
        // this way than in a transmissive pass with its own copy of the screen.
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.055,
        metallic: 0.0,
        reflectance: 0.5,
        ..default()
    }
}

pub fn spawn_vehicle(
    commands: &mut Commands,
    assets: &VehicleAssets,
    materials: &mut Assets<StandardMaterial>,
    spec: VehicleSpec,
    transform: Transform,
) -> Entity {
    let size = spec.half_extents * 2.0;
    // Off the shelf where the colour is one this city stocks, which it almost
    // always is; a fresh material only for a car somebody tinted by hand.
    let paint = match assets.stock_paint(spec.body_color, spec.body_metallic, spec.body_age) {
        Some(handle) => handle.clone(),
        None => materials.add(bodywork(
            spec.body_color,
            spec.body_metallic,
            spec.body_age,
            &assets.flake,
        )),
    };
    let fittings = fittings_range();

    let anchors = spec.wheel_anchors();
    let wheel_radius = spec.wheel_radius;
    let name = spec.display_name;
    let class = spec.class;
    let body = assets.body(class);
    // The fittings are placed from the spec, and the spec is moved onto the
    // vehicle before the children are spawned.
    let fitted = spec.clone();

    let mut entity = commands.spawn((
        Name::new(name),
        Vehicle,
        transform,
        Visibility::default(),
        // Parked cars start static; `activate_nearby_vehicles` promotes them.
        RigidBody::Static,
        Collider::cuboid(size.x, size.y, size.z),
        Mass(spec.mass),
        // Lowering the centre of mass is what stops it rolling over in corners.
        CenterOfMass(spec.center_of_mass),
        VehicleInput::default(),
        VehicleState::default(),
        super::impact::PreviousVelocity::default(),
        super::impact::Unsettled::default(),
        spec,
    ));

    entity.with_children(|parent| {
        // The shell is already built at this spec's size, so unlike the box it
        // replaced it needs no scaling. It stays separate from the collider:
        // the car collides as the box it always did, and the bodywork is free
        // to be any shape inside that.
        parent.spawn((
            Mesh3d(body.shell.clone()),
            MeshMaterial3d(paint.clone()),
            Transform::IDENTITY,
        ));
        // The sill, in the same paint. Separate only so the shell above it can
        // arch over the wheels without the body pinching in half.
        parent.spawn((
            Mesh3d(body.lower.clone()),
            MeshMaterial3d(paint.clone()),
            Transform::IDENTITY,
        ));
        if let Some(cabin) = &body.cabin {
            parent.spawn((
                Mesh3d(cabin.clone()),
                MeshMaterial3d(assets.glass.clone()),
                Transform::IDENTITY,
            ));
        }
        // The inside: the same loft, built a shade smaller and wound inside
        // out. Without it the near glass is transparent, the far glass is
        // culled, and you see the street straight through the car.
        if let Some(liner) = &body.liner {
            parent.spawn((
                Mesh3d(liner.clone()),
                MeshMaterial3d(assets.trim.liner.clone()),
                Transform::IDENTITY,
                // The greenhouse's shadow is cast by its glass — tinted glass
                // does cast one, and the glass is the outer surface. A shadow
                // map filled from an inside-out liner would record the far wall
                // of the cabin and let the sun in through the roof.
                bevy::light::NotShadowCaster,
            ));
            super::trim::furnish(parent, &assets.trim, class, &fitted, &cabin_range());
        }
        if let Some(frame) = &body.frame {
            parent.spawn((
                Mesh3d(frame.clone()),
                MeshMaterial3d(paint.clone()),
                Transform::IDENTITY,
            ));
        }
        if let Some(windows) = &body.windows {
            parent.spawn((
                Mesh3d(windows.clone()),
                MeshMaterial3d(assets.dark_glass.clone()),
                Transform::IDENTITY,
            ));
        }
        super::trim::fit(
            parent,
            &assets.trim,
            class,
            &fitted,
            &paint,
            transform.translation,
            &fittings,
        );

        for (index, anchor) in anchors.iter().enumerate().take(WHEEL_COUNT) {
            // Wheels are built about the X axis at unit radius, so the whole
            // assembly scales with the spec and needs no laying-on-its-side.
            parent
                .spawn((
                    WheelVisual(index),
                    Mesh3d(assets.tyre_mesh.clone()),
                    MeshMaterial3d(assets.tyre.clone()),
                    Transform::from_translation(*anchor).with_scale(Vec3::splat(wheel_radius)),
                ))
                .with_child((
                    Mesh3d(assets.rim_mesh.clone()),
                    MeshMaterial3d(assets.rim.clone()),
                    // The tyre stays at every distance — it is the wheel's
                    // silhouette — and the face inside it does not.
                    fittings.clone(),
                    // The face belongs on the outside of the car, so the
                    // left-hand wheels wear theirs turned around. A negative
                    // scale would do it too, and would turn every triangle
                    // inside out.
                    Transform::from_rotation(if anchor.x < 0.0 {
                        Quat::from_rotation_y(std::f32::consts::PI)
                    } else {
                        Quat::IDENTITY
                    }),
                ));
        }
    });

    entity.id()
}

/// Yaw that points a vehicle's forward axis along `direction` (an XZ vector).
///
/// Worth its own function and test: Bevy's forward is -Z, and `Vec2::to_angle`
/// measures from +X, so the naive conversion silently parks every car facing
/// across its street instead of along it.
pub fn heading_towards(direction: Vec2) -> f32 {
    // Rotating -Z by yaw gives (-sin yaw, -cos yaw), so invert that.
    (-direction.x).atan2(-direction.y)
}

/// Height the body origin sits at once the springs balance the car's weight.
/// Spawning at exactly this height means a car promoted from static to dynamic
/// neither drops nor pops.
pub fn resting_height(spec: &VehicleSpec) -> f32 {
    let load_per_wheel = spec.mass * 9.81 / WHEEL_COUNT as f32;
    let compression = load_per_wheel / spec.spring_strength;
    (spec.max_ray_length() - compression) - spec.axle_height
}

/// Promotes nearby vehicles to dynamic and demotes distant ones.
pub fn activate_nearby_vehicles(
    mut commands: Commands,
    cameras: Query<&GlobalTransform, With<crate::player::camera::CameraRig>>,
    vehicles: Query<
        (
            Entity,
            &Transform,
            Has<ActiveVehicle>,
            Has<DrivenBy>,
            Has<AlwaysSimulated>,
        ),
        With<Vehicle>,
    >,
) {
    let Ok(camera) = cameras.single() else { return };
    let focus = camera.translation();

    for (entity, transform, active, driven, always) in &vehicles {
        let wanted = driven || always || transform.translation.distance(focus) < ACTIVE_RADIUS;
        match (wanted, active) {
            (true, false) => {
                commands.entity(entity).insert((
                    ActiveVehicle,
                    RigidBody::Dynamic,
                    // A sleeping car ignores its own suspension.
                    SleepingDisabled,
                ));
            }
            (false, true) => {
                commands
                    .entity(entity)
                    .remove::<ActiveVehicle>()
                    .remove::<SleepingDisabled>()
                    .insert(RigidBody::Static);
            }
            _ => {}
        }
    }
}

/// Positions each wheel mesh from its suspension state.
pub fn update_wheel_visuals(
    time: Res<Time>,
    vehicles: Query<(&VehicleState, &VehicleSpec, &Children)>,
    mut wheels: Query<(&WheelVisual, &mut Transform)>,
) {
    // The ray lengths are sampled per 64Hz physics tick. The body is eased
    // between ticks by the interpolation plugin, but these are plain state
    // fields, so drawn raw the meshes closest to the camera stair-step
    // against a smooth body over every bump. Blended at 30/s the lag is a
    // frame or two — invisible under deliberately underdamped springs.
    let blend = 1.0 - (-30.0 * time.delta_secs()).exp();
    for (state, spec, children) in &vehicles {
        let anchors = spec.wheel_anchors();
        for child in children.iter() {
            let Ok((wheel, mut transform)) = wheels.get_mut(child) else {
                continue;
            };
            let index = wheel.0;
            let wheel_state = &state.wheels[index];

            // The mesh hangs below its anchor by however much suspension is extended.
            let drop = wheel_state.ray_length - spec.wheel_radius;
            let target = anchors[index] - Vec3::Y * drop;
            transform.translation = Vec3::new(
                target.x,
                transform.translation.y + (target.y - transform.translation.y) * blend,
                target.z,
            );

            let steer = if VehicleSpec::is_front(index) {
                Quat::from_rotation_y(state.steer_angle)
            } else {
                Quat::IDENTITY
            };
            // The wheel's axle is its own X axis, so rolling is a rotation
            // about that. Negative: driving forwards is -Z, and the contact
            // patch has to travel backwards relative to the car.
            transform.rotation = steer * Quat::from_rotation_x(-state.wheel_spin);
        }
    }
}

/// How much daylight a parked car leaves between itself and the kerb.
///
/// A third of a metre, which is about how badly a real person parks. It has to
/// be more than nothing, because the kerb's collider arrives with the chunk and
/// a body already overlapping one is ejected rather than nudged.
const KERB_CLEARANCE: f32 = 0.34;

/// Scatters parked cars along the kerbs so there is always something to steal.
pub fn spawn_parked_vehicles(
    mut commands: Commands,
    config: Res<GameConfig>,
    city: Res<City>,
    assets: Res<VehicleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut rng = stream_for(config.world_seed, stream::VEHICLE_SPAWNS);
    let mut spawned = 0;

    for edge in city.graph.edges() {
        // Not every segment, or the city reads as a car park — but often
        // enough that there is always one within a short walk, because a crime
        // sandbox where you cannot find a car is not much of a sandbox.
        if rng.random_range(0.0..1.0) > 0.62 {
            continue;
        }

        let a = city.graph.node(edge.a).pos;
        let b = city.graph.node(edge.b).pos;
        let Ok(direction) = Dir2::new(b - a) else {
            continue;
        };
        let normal = Vec2::new(-direction.y, direction.x);

        // Somewhere along the segment, parked against one kerb.
        let along: f32 = rng.random_range(0.25..0.75);
        let side = if rng.random_range(0.0..1.0) < 0.5 {
            1.0
        } else {
            -1.0
        };
        let class = VehicleClass::CIVILIAN[rng.random_range(0..VehicleClass::CIVILIAN.len())];
        let mut spec = class.spec();
        (spec.body_color, spec.body_metallic, spec.body_age) = super::paint::street_paint(&mut rng);
        // Off the kerb by the car's own width, which the fixed metre and a half
        // above was not.
        //
        // Parked cars are startup-resident and the kerbs they are parked
        // against are streamed, so a car overlapping one is not a car sitting
        // in a wall — it is a *static collider appearing inside a dynamic
        // body*, a second later, when the chunk arrives. Avian resolves that
        // the only way it can, and the patrol caught what it looks like: a
        // parked car leaving the ground at thirteen metres a second and coming
        // down from seventy.
        //
        // It went unnoticed for as long as every street was one of six widths
        // from a table. Real widths off the extract run down to four and a
        // half metres, and on one of those a van parked a metre and a half off
        // the centre line has its flank inside the kerb.
        let offset = (edge.width * 0.5 - spec.half_extents.x - KERB_CLEARANCE).max(0.0);
        let position = a + *direction * (edge.length * along) + normal * offset * side;
        // Nose along the street, facing the way traffic on that side runs.
        let facing = if side > 0.0 { *direction } else { -*direction };
        let heading = heading_towards(facing);
        let transform = Transform::from_xyz(position.x, resting_height(&spec), position.y)
            .with_rotation(Quat::from_rotation_y(heading));

        spawn_vehicle(&mut commands, &assets, &mut materials, spec, transform);
        spawned += 1;
    }

    // Guarantee one at the player's start. Relying on the random scatter to
    // put a car within sight of the spawn is a coin flip, and the first thing
    // anyone does is look for something to drive.
    if let Some(start) = city.graph.nearest_node(Vec2::ZERO) {
        let node = city.graph.node(start);
        let along = city.graph.neighbors(start).next().and_then(|(next, edge)| {
            let to = city.graph.node(next).pos - node.pos;
            Dir2::new(to).ok().map(|d| (d, city.graph.edge(edge).width))
        });
        let Some((direction, width)) = along else {
            return;
        };

        let mut spec = VehicleClass::Sedan.spec();
        (spec.body_color, spec.body_metallic, spec.body_age) = super::paint::street_paint(&mut rng);
        let heading = heading_towards(*direction);
        // On the carriageway, a little down the street from the junction.
        let normal = Vec2::new(-direction.y, direction.x);
        let position = node.pos + *direction * 12.0 + normal * (width * 0.25);
        let transform = Transform::from_xyz(position.x, resting_height(&spec), position.y)
            .with_rotation(Quat::from_rotation_y(heading));
        spawn_vehicle(&mut commands, &assets, &mut materials, spec, transform);
        spawned += 1;
    }

    // And into the parking lots, which is what the bays are painted for. This
    // block draws *after* every kerb draw above, so adding it moved no car
    // that was already parked. Cars are startup-resident rather than chunk-
    // streamed, so the lots — which are — get their paint from `world::lots`
    // and their occupants from here, sharing `lots::bays` as the contract.
    for block in &city.0.blocks {
        for vacant in &block.vacants {
            if vacant.purpose != crate::world::citygen::VacantUse::ParkingLot {
                continue;
            }
            for (at, heading) in crate::world::lots::bays(&vacant.rect) {
                // A lot two-thirds full reads as a lot; a full one reads as a
                // wall of cars, and an empty one as a mistake.
                if rng.random_range(0.0..1.0) > 0.62 {
                    continue;
                }
                let class =
                    VehicleClass::CIVILIAN[rng.random_range(0..VehicleClass::CIVILIAN.len())];
                let mut spec = class.spec();
                (spec.body_color, spec.body_metallic, spec.body_age) =
                    super::paint::street_paint(&mut rng);
                // Lots sit on the kerb slab, a step above the carriageway.
                let transform = Transform::from_xyz(
                    at.x,
                    crate::world::buildings::SIDEWALK_HEIGHT + resting_height(&spec),
                    at.y,
                )
                .with_rotation(Quat::from_rotation_y(heading));
                spawn_vehicle(&mut commands, &assets, &mut materials, spec, transform);
                spawned += 1;
            }
        }
    }

    info!("{spawned} vehicles parked around the city");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The forward axis a vehicle ends up with for a given heading.
    fn forward_for(heading: f32) -> Vec2 {
        let f = Quat::from_rotation_y(heading) * Vec3::NEG_Z;
        Vec2::new(f.x, f.z)
    }

    #[test]
    fn heading_points_the_car_along_the_street() {
        for direction in [
            Vec2::new(1.0, 0.0),
            Vec2::new(-1.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(0.0, -1.0),
            Vec2::new(0.6, 0.8),
            Vec2::new(-0.28, 0.96),
        ] {
            let forward = forward_for(heading_towards(direction));
            assert!(
                forward.distance(direction.normalize()) < 1e-4,
                "heading for {direction:?} produced forward {forward:?}"
            );
        }
    }

    #[test]
    fn resting_height_keeps_the_wheels_on_the_ground() {
        for class in [
            VehicleClass::Sedan,
            VehicleClass::Sports,
            VehicleClass::Truck,
        ] {
            let spec = class.spec();
            let height = resting_height(&spec);
            // The body must clear the road, and the wheels must still reach it.
            assert!(
                height > spec.half_extents.y,
                "{} would spawn with its belly in the road",
                spec.display_name
            );
            // The invariant that matters: the suspension ray from each wheel
            // anchor must reach exactly the road surface. The wheel hangs below
            // its anchor by the remaining travel, not by its radius.
            let load_per_wheel = spec.mass * 9.81 / WHEEL_COUNT as f32;
            let compression = load_per_wheel / spec.spring_strength;
            let anchor = height + spec.axle_height;
            let ray_length = spec.max_ray_length() - compression;
            assert!(
                (anchor - ray_length).abs() < 1e-4,
                "{}: wheel contact lands at {:.3}m instead of the road",
                spec.display_name,
                anchor - ray_length
            );
        }
    }

    fn stocked() -> VehicleAssets {
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut images = Assets::<Image>::default();
        build_assets(&mut meshes, &mut materials, &mut images)
    }

    /// The whole point of the paint shop: nothing that spawns off the street
    /// ever asks for a material of its own.
    ///
    /// If this fails, the city quietly goes back to one `StandardMaterial` per
    /// parked car — which costs nothing visible and everything in batching, so
    /// it is exactly the kind of regression that only turns up in a frame time
    /// six months later.
    #[test]
    fn every_colour_a_car_can_be_is_already_on_the_shelf() {
        let assets = stocked();
        let mut rng = crate::core::rng::stream_for(7, stream::VEHICLE_SPAWNS);
        for _ in 0..2000 {
            let (color, metallic, age) = super::super::paint::street_paint(&mut rng);
            assert!(
                assets.stock_paint(color, metallic, age).is_some(),
                "{color:?} at {metallic}, aged {age}, would need a material of its own"
            );
        }
        for class in VehicleClass::ALL {
            let spec = class.spec();
            assert!(
                assets
                    .stock_paint(spec.body_color, spec.body_metallic, spec.body_age)
                    .is_some(),
                "{}'s own livery is not stocked",
                spec.display_name
            );
        }
    }

    #[test]
    fn the_shelf_holds_no_duplicates() {
        // Two handles for one paint is two batches for one colour, which is
        // the bug this whole mechanism exists to avoid.
        let assets = stocked();
        for (i, (color, metallic, age, _)) in assets.stock.iter().enumerate() {
            for (other, other_metallic, other_age, _) in &assets.stock[i + 1..] {
                assert!(
                    !(same_colour(*color, *other)
                        && metallic == other_metallic
                        && age == other_age),
                    "{color:?} at {metallic}, aged {age}, is stocked twice"
                );
            }
        }
    }

    #[test]
    fn a_colour_nobody_stocks_still_gets_painted() {
        // The escape hatch has to stay open: the capture harness tints cars by
        // hand, and a lookup that silently returned the nearest stock colour
        // would repaint them without saying so.
        let assets = stocked();
        assert!(
            assets
                .stock_paint(Color::srgb(0.01, 0.99, 0.42), 0.5, 0.0)
                .is_none()
        );
    }

    /// Bevy asserts on a range whose margins cross, and a range that ends
    /// before it starts is a mesh that is never drawn at all.
    #[test]
    fn the_detail_ranges_are_ordered_and_the_cabin_goes_first() {
        for range in [fittings_range(), cabin_range()] {
            assert!(range.start_margin.end <= range.end_margin.start);
            assert!(range.end_margin.start < range.end_margin.end);
        }
        assert!(
            cabin_range().end_margin.start < fittings_range().end_margin.start,
            "the seats should go before the bumpers do"
        );
        // And both well inside the distance a car is still simulated at, so
        // nothing that has physics done to it is being drawn without fittings
        // in the player's face.
        assert!(cabin_range().end_margin.start < ACTIVE_RADIUS);
    }
}
