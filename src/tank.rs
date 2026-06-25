//! The tank: its rig (structural markers bound by node name), the kinematic `Servo` motor
//! for the turret/gun, and the asset-load binding. The tank declares *structure*; features
//! (aim, shooting) attach their own behavior to these markers reactively.

use std::collections::HashSet;

use avian3d::prelude::{
    ColliderConstructor, ColliderConstructorHierarchy, ColliderDensity, CollisionLayers, LayerMask,
    RayCaster, RigidBody, SpatialQueryFilter,
};
use bevy::asset::LoadState;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use serde::Deserialize;

use crate::Layer;
use crate::spec::{TankSpec, TankSpecHandle};
use crate::state::{AppState, GameplaySet};

// --- Rig markers. Name = the structural contract between the model and the code. ---

#[derive(Component)]
pub struct Turret;

#[derive(Component)]
pub struct Gun;

#[derive(Component)]
pub struct Hull;

/// Marks the vehicle's root entity — the dynamic rigid body (chassis). Suspension/drive forces
/// are applied here; debug x-ray walks its descendants.
#[derive(Component)]
pub struct Tank;

#[derive(Component)]
pub struct Muzzle;

/// The recoiling barrel node (child of `Gun`, parent of `Muzzle`).
#[derive(Component)]
pub struct GunBarrel;

/// Which track a roadwheel drives (for differential thrust). Left wheels sit at −X, right at +X.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrackSide {
    Left,
    Right,
}

/// A load-bearing roadwheel — a suspension/drive contact station, tagged with its track side.
/// Carries a downward [`RayCaster`] (the suspension ray); the sprocket and idler are excluded.
#[derive(Component)]
pub struct Roadwheel {
    pub side: TrackSide,
}

/// The authored centre-of-mass: an Empty (`Center_Of_Mass`) placed in the model. `driving` reads
/// its position and sets the body's centre of mass from it — the model owns the COM.
#[derive(Component)]
pub struct CenterOfMassAnchor;

/// The local axis a servo rotates about. Cardinal-only — tank servos yaw/pitch about a hull axis;
/// a canted mount would add a `Custom(Dir3)` variant. Resolved to a vector in `drive_servos`.
#[derive(Clone, Copy, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn to_vec3(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        }
    }
}

/// Travel limits for a [`ServoSpec`], in **degrees** (the authoring unit).
#[derive(Clone, Copy, Deserialize)]
pub enum Travel {
    Limited { min: f32, max: f32 },
    Continuous,
}

// A 1-DOF kinematic rotational motor (trapezoidal motion profile), split three ways so each
// concern has one owner: per-variant config, the commanded intent, and the live mechanism state.
// `drive_servos` is the behaviour; it reads spec + command and drives state + the transform.

/// Servo config: rotation axis, speed/accel limits, travel range. Per-variant data authored in the
/// tank's `.tank.ron` spec sheet (ADR-0010) and applied to the bound servo node. Angles are in
/// **degrees** — the human-facing authoring unit; `drive_servos` converts to radians (the
/// computed/runtime unit shared with `ServoCommand` and `ServoState`).
#[derive(Component, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServoSpec {
    axis: Axis,
    /// Max slew speed, degrees/second.
    max_speed: f32,
    /// Slew acceleration, degrees/second².
    accel: f32,
    travel: Travel,
}

/// The commanded angle (parent-local) a servo slews toward — the *intent*, written by aiming
/// (and, later, the ROADMAP Phase-2 controls layer). Position-mode for now; a velocity-mode
/// command is a future variant (NOTES.md). Kept separate from state: different writer, different
/// lifecycle.
#[derive(Component, Default)]
pub struct ServoCommand {
    pub target: f32,
}

/// A servo's live mechanism state — current angle and angular velocity of the slew. Owned by
/// `drive_servos`; never authored, never shared.
#[derive(Component, Default)]
pub struct ServoState {
    current: f32,
    velocity: f32,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, load_tank_spec)
        .add_systems(
            Update,
            spawn_tank_when_loaded.run_if(in_state(AppState::Loading)),
        )
        .add_systems(FixedUpdate, drive_servos.in_set(GameplaySet));
}

/// The tank's spec sheet is a *load dependency* (ADR-0011): we kick off its load up front and the
/// tank scene is spawned only once it's ready, so the rig binds with its stats already in hand —
/// no spec-less window. While it loads we sit in `AppState::Loading`.
#[derive(Resource)]
struct PendingTankSpec(Handle<TankSpec>);

fn load_tank_spec(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(PendingTankSpec(
        asset_server.load("tiger_1/tiger_1.tank.ron"),
    ));
}

/// Once the spec has loaded, spawn the tank and enter `Playing`. A *failed* spec load is fatal
/// here (no fallback stats, ADR-0011); a still-loading spec just waits another frame.
fn spawn_tank_when_loaded(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    pending: Option<Res<PendingTankSpec>>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(pending) = pending else {
        return;
    };
    match asset_server.load_state(&pending.0) {
        LoadState::Loaded => {
            commands
                .spawn((
                    WorldAssetRoot(
                        asset_server
                            .load(GltfAssetLabel::Scene(0).from_asset("tiger_1/tiger_1.glb")),
                    ),
                    TankSpecHandle(pending.0.clone()),
                    Transform::from_xyz(10.0, 2.0, 5.0).with_rotation(Quat::from_rotation_z(0.7)),
                    // The hull is a dynamic rigid body — Avian owns its Transform (ADR-0005); its
                    // collider comes from the model's `*_Collider` proxy, bound in on_tank_ready.
                    Tank,
                    RigidBody::Dynamic,
                ))
                .observe(on_tank_ready);
            commands.remove_resource::<PendingTankSpec>();
            next.set(AppState::Playing);
        }
        LoadState::Failed(err) => {
            error!("required tank spec sheet failed to load: {err}");
            panic!("required tank spec sheet failed to load: {err}");
        }
        _ => {}
    }
}

/// Walk the loaded scene and, in one pass, bind structural markers + apply the (already-loaded)
/// per-variant spec to each part — servo configs, the suspension ray, the collider's density — and
/// enforce the rig contract: every node the sim binds behaviour to must exist in the model.
/// Missing structure is an authoring bug — fatal like a bad spec sheet (ADR-0010) — so we panic
/// with the list of what's absent. This is where ADR-0002's "name = the contract" is *enforced*.
fn on_tank_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    handles: Query<&TankSpecHandle>,
    specs: Res<Assets<TankSpec>>,
) {
    // The spec is a load dependency of spawning (ADR-0011): the tank is spawned only once its
    // `.tank.ron` has loaded, so it's guaranteed present here. Its absence would be a bug.
    let spec = handles
        .get(ready.entity)
        .ok()
        .and_then(|handle| specs.get(&handle.0))
        .expect("tank spec must be loaded before the tank is spawned");

    // Hull-level per-variant data (each rig marker below takes its own per-part config).
    commands
        .entity(ready.entity)
        .insert((spec.drivetrain.clone(), spec.suspension.clone()));

    // Record what the walk found, to check against the required contract afterwards.
    let mut found: HashSet<&'static str> = HashSet::new();
    let mut left_wheels = 0u32;
    let mut right_wheels = 0u32;
    let mut colliders = 0u32;

    for entity in children.iter_descendants(ready.entity) {
        // Most descendants are unnamed mesh nodes — skip them quietly.
        let Ok(name) = names.get(entity) else {
            continue;
        };
        let mut entity = commands.entity(entity);
        match name.as_str() {
            // Servos: marker + command/state slots + the per-variant `ServoSpec` (axis, speeds,
            // travel) from the spec sheet (ADR-0010) — never authored in code.
            "Turret" => {
                found.insert("Turret");
                entity.insert((
                    Turret,
                    ServoCommand::default(),
                    ServoState::default(),
                    spec.turret.clone(),
                ));
            }
            "Gun" => {
                found.insert("Gun");
                entity.insert((
                    Gun,
                    ServoCommand::default(),
                    ServoState::default(),
                    spec.gun.clone(),
                ));
            }
            "Hull" => {
                found.insert("Hull");
                entity.insert(Hull);
            }
            "Muzzle" => {
                found.insert("Muzzle");
                entity.insert(Muzzle);
            }
            "Gun_Barrel" => {
                found.insert("Gun_Barrel");
                entity.insert(GunBarrel);
            }
            "Center_Of_Mass" => {
                found.insert("Center_Of_Mass");
                entity.insert(CenterOfMassAnchor);
            }
            // Roadwheels (Wheel_L_0.., Wheel_R_0..): tag the track side + a downward suspension ray
            // sized by the variant's `ray_length`, filtered to Terrain so it skips the tank's own
            // collider. The wheel node has identity rotation, so local −Y is the hull-down axis.
            s if s.starts_with("Wheel_") => {
                let side = if s.starts_with("Wheel_L") {
                    left_wheels += 1;
                    TrackSide::Left
                } else {
                    right_wheels += 1;
                    TrackSide::Right
                };
                entity.insert((
                    Roadwheel { side },
                    RayCaster::new(Vec3::ZERO, Dir3::NEG_Y)
                        .with_max_distance(spec.suspension.ray_length)
                        .with_query_filter(SpatialQueryFilter::from_mask(Layer::Terrain)),
                ));
            }
            // Collision proxies (`*_Collider`, optionally split `_0/_1/...`): a convex-hull collider
            // on the Vehicle layer at the variant's density, hidden (it's physics, not rendering —
            // ADR-0008). The glTF loader puts the mesh on a child primitive, so build over the
            // node's descendants (the hierarchy constructor), not the node itself.
            s if s.contains("_Collider") => {
                colliders += 1;
                entity.insert((
                    ColliderConstructorHierarchy::new(ColliderConstructor::ConvexHullFromMesh)
                        .with_default_layers(CollisionLayers::new([Layer::Vehicle], LayerMask::ALL))
                        .with_default_density(ColliderDensity(spec.hull_density)),
                    Visibility::Hidden,
                ));
            }
            _ => {}
        }
    }

    // Required singletons, plus ≥1 collider (else the body is massless → NaN) and ≥1 roadwheel per
    // side (else a track has no support/thrust). A real Tiger has many wheels; the sim only needs
    // one contact station per side to be non-degenerate, so the contract is per-side presence, not
    // a fixed count (which varies per variant).
    const REQUIRED: [&str; 6] = ["Hull", "Turret", "Gun", "Gun_Barrel", "Muzzle", "Center_Of_Mass"];
    let mut missing: Vec<&str> = REQUIRED.iter().copied().filter(|n| !found.contains(n)).collect();
    if colliders == 0 {
        missing.push("*_Collider");
    }
    if left_wheels == 0 {
        missing.push("Wheel_L*");
    }
    if right_wheels == 0 {
        missing.push("Wheel_R*");
    }
    assert!(
        missing.is_empty(),
        "tank model is missing required rig nodes: {missing:?}"
    );
}

fn drive_servos(
    mut q: Query<(&mut Transform, &ServoSpec, &ServoCommand, &mut ServoState)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (mut transform, spec, command, mut state) in &mut q {
        // `ServoSpec` authors angles in degrees (the human authoring unit); the runtime — the
        // command, the state, and the slew maths below — is radians. Convert the spec's angular
        // quantities once here, at the spec→runtime boundary.
        let max_speed = spec.max_speed.to_radians();
        let accel = spec.accel.to_radians();
        let travel = match spec.travel {
            Travel::Limited { min, max } => Travel::Limited {
                min: min.to_radians(),
                max: max.to_radians(),
            },
            Travel::Continuous => Travel::Continuous,
        };

        let prev = state.current;
        let error = match travel {
            Travel::Limited { .. } => command.target - state.current,
            Travel::Continuous => shortest_angle(command.target - state.current),
        };
        let braking_dist = (state.velocity * state.velocity) / (2.0 * accel);

        if error.abs() <= braking_dist {
            let dv = accel * dt;
            state.velocity = if state.velocity > 0.0 {
                (state.velocity - dv).max(0.0)
            } else {
                (state.velocity + dv).min(0.0)
            };
        } else {
            state.velocity += error.signum() * accel * dt;
            state.velocity = state.velocity.clamp(-max_speed, max_speed);
        }

        state.current += state.velocity * dt;
        if let Travel::Limited { min, max } = travel {
            state.current = state.current.clamp(min, max);
        }

        if error.abs() < 0.001 && state.velocity.abs() < 0.01 {
            state.velocity = 0.0;
            if let Travel::Limited { min, max } = travel {
                state.current = command.target.clamp(min, max);
            }
        }

        let delta = state.current - prev;
        transform.rotate_local(Quat::from_axis_angle(spec.axis.to_vec3(), delta));
    }
}

/// Wrap an angle difference into [-PI, PI] for shortest-path rotation.
fn shortest_angle(diff: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (diff + PI).rem_euclid(TAU) - PI
}
