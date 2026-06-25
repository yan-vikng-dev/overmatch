//! Firing: spawn a kinematic shell at the muzzle, integrate gravity, raycast the terrain along
//! each step, emit an `Impact`, and recoil the barrel. Ballistics stay hand-integrated — we own
//! the trajectory (muzzle velocity, gravity, later drag/penetration as data + rules); Avian only
//! answers the impact query: what the segment hit, where, and the surface normal.

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::ecs::lifecycle::Add;
use bevy::prelude::*;

use crate::Layer;
use crate::state::GameplaySet;
use crate::tank::{GunBarrel, Muzzle};

/// Muzzle velocity of the 88mm gun (m/s). The world is in meters, so this is literal.
const MUZZLE_SPEED: f32 = 773.0;
/// Reload cooldown before the gun can fire again (s). Placeholder — tune to the gun later.
const RELOAD_SECS: f32 = 3.0;
/// Gravity applied to shells each fixed tick (m/s²).
const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);

/// Backward impulse on firing (m/s along the bore). Higher = harder, longer kick.
const RECOIL_KICK: f32 = 14.0;
/// Spring stiffness pulling the barrel back to battery. Lower = longer stroke + slower return.
const RECOIL_STIFFNESS: f32 = 90.0;
/// Damping; slightly underdamped, so the barrel lumbers home with a small settle.
const RECOIL_DAMPING: f32 = 14.0;

/// A shell in flight. Kinematic — integrated by hand, no physics engine.
#[derive(Component)]
struct Projectile {
    velocity: Vec3,
}

/// Procedural barrel recoil: a 1-DOF damped spring on the barrel. Firing kicks it back along
/// the bore (+local Z); the spring returns it to battery. The translational cousin of `Servo`.
#[derive(Component)]
struct Recoil {
    rest: Vec3,
    offset: f32,
    velocity: f32,
}

/// Preloaded shell scene, cloned per shot rather than loaded each time.
#[derive(Resource)]
struct ProjectileAssets {
    scene: Handle<WorldAsset>,
}

/// Gun reload cooldown: seconds remaining before the next shot. 0 = ready.
#[derive(Resource)]
struct Reload {
    remaining: f32,
}

/// A shell hit something — the seam Phase-2 penetration/armor and impact VFX hang off. The
/// hit's normal and struck entity are available from the raycast; add them here when a feature
/// needs them. Global event (the shell despawns), handled by the `on_impact` observer.
#[derive(Event)]
struct Impact {
    position: Vec3,
}

/// Preloaded mesh+material for the debug impact marker, cloned per hit by `on_impact`.
#[derive(Resource)]
struct ImpactDebug {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub fn plugin(app: &mut App) {
    app.insert_resource(Reload { remaining: 0.0 })
        .add_observer(on_impact)
        .add_systems(Startup, setup_assets)
        // attach_recoil reacts to the barrel binding (observer), so it stays out of the set.
        .add_observer(attach_recoil)
        .add_systems(Update, fire.in_set(GameplaySet))
        .add_systems(
            FixedUpdate,
            (integrate_projectiles, apply_recoil).in_set(GameplaySet),
        );
}

fn setup_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Preload once; firing clones the handle rather than hitting the asset server per shot.
    commands.insert_resource(ProjectileAssets {
        scene: asset_server.load(GltfAssetLabel::Scene(0).from_asset("shell/shell.glb")),
    });
    // Small red sphere reused for every impact marker.
    commands.insert_resource(ImpactDebug {
        mesh: meshes.add(Sphere::new(0.2)),
        material: materials.add(Color::srgb(1.0, 0.3, 0.1)),
    });
}

/// Attach `Recoil` the moment the rig binds `GunBarrel`, capturing its rest (battery) position
/// from the barrel's (parent-local) Transform. Keeps recoil (a shooting concern) out of the rig.
fn attach_recoil(add: On<Add, GunBarrel>, barrels: Query<&Transform>, mut commands: Commands) {
    let Ok(transform) = barrels.get(add.entity) else {
        return;
    };
    commands.entity(add.entity).insert(Recoil {
        rest: transform.translation,
        offset: 0.0,
        velocity: 0.0,
    });
}

fn fire(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut reload: ResMut<Reload>,
    assets: Res<ProjectileAssets>,
    muzzle: Query<&GlobalTransform, With<Muzzle>>,
    mut barrel: Query<&mut Recoil>,
    mut commands: Commands,
) {
    reload.remaining = (reload.remaining - time.delta_secs()).max(0.0);
    if !mouse.just_pressed(MouseButton::Left) || reload.remaining > 0.0 {
        return;
    }
    let Ok(muzzle) = muzzle.single() else {
        return;
    };

    // Spawn at the muzzle, pointing down the bore; velocity is the bore axis * muzzle speed.
    commands.spawn((
        Projectile {
            velocity: muzzle.forward() * MUZZLE_SPEED,
        },
        WorldAssetRoot(assets.scene.clone()),
        muzzle.compute_transform(),
    ));

    // Kick the barrel back; apply_recoil springs it home.
    if let Ok(mut recoil) = barrel.single_mut() {
        recoil.velocity += RECOIL_KICK;
    }
    reload.remaining = RELOAD_SECS;
}

fn integrate_projectiles(
    mut projectiles: Query<(Entity, &mut Transform, &mut Projectile)>,
    spatial: SpatialQuery,
    time: Res<Time>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let filter = SpatialQueryFilter::from_mask(Layer::Terrain);
    for (entity, mut transform, mut projectile) in &mut projectiles {
        let prev = transform.translation;
        // Semi-implicit Euler: update velocity first, then position.
        projectile.velocity += GRAVITY * dt;
        transform.translation += projectile.velocity * dt;

        // Raycast the segment just traversed against the terrain (a ray of the step's length —
        // fast shells can't tunnel). A hit stops the shell and emits the impact at the hit point.
        let step = transform.translation - prev;
        let Ok(direction) = Dir3::new(step) else {
            continue;
        };
        if let Some(hit) = spatial.cast_ray(prev, direction, step.length(), true, &filter) {
            commands.trigger(Impact {
                position: prev + direction * hit.distance,
            });
            commands.entity(entity).despawn();
        }
    }
}

fn apply_recoil(mut barrel: Query<(&mut Transform, &mut Recoil)>, time: Res<Time>) {
    let dt = time.delta_secs();
    for (mut transform, mut recoil) in &mut barrel {
        // Damped spring back to battery: offset'' = -k·offset - c·offset'.
        let accel = -RECOIL_STIFFNESS * recoil.offset - RECOIL_DAMPING * recoil.velocity;
        recoil.velocity += accel * dt;
        recoil.offset += recoil.velocity * dt;
        // Battery stop — the barrel can't return past its rest position.
        if recoil.offset < 0.0 {
            recoil.offset = 0.0;
            recoil.velocity = 0.0;
        }
        // Recoil rides back along the bore (+local Z), measured from the rest position.
        transform.translation = recoil.rest + Vec3::Z * recoil.offset;
    }
}

fn on_impact(impact: On<Impact>, debug: Res<ImpactDebug>, mut commands: Commands) {
    info!("shell impact at {:?}", impact.position);
    // Debug marker for now; Phase-2 penetration/armor and impact VFX hook in here.
    commands.spawn((
        Mesh3d(debug.mesh.clone()),
        MeshMaterial3d(debug.material.clone()),
        Transform::from_translation(impact.position),
    ));
}
