//! The battlefield: environment lighting now, terrain later. Also home to the ground-plane
//! query that aiming and the camera both use — the seam to swap for an Avian raycast once
//! terrain has colliders.

use avian3d::prelude::{
    Collider, CollisionLayers, LayerMask, RigidBody, SpatialQuery, SpatialQueryFilter,
};
use bevy::prelude::*;

use crate::Layer;

/// Side length of the (square) ground plane, in metres.
const GROUND_SIZE: f32 = 1000.0;
/// Thickness of the ground slab. Only the top face (at y=0) matters; the rest is buried.
const GROUND_THICKNESS: f32 = 1.0;

/// First authored test slope: a thick slab tilted about X. Incline angle, and the slab's run
/// (along the slope, Z), width (X), and thickness (Y) — degrees / metres.
const RAMP_ANGLE_DEG: f32 = 12.0;
const RAMP_RUN: f32 = 16.0;
const RAMP_WIDTH: f32 = 12.0;
const RAMP_THICKNESS: f32 = 2.0;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn_environment);
}

fn spawn_environment(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // The ground: a static slab whose top face sits at y=0 — the same plane the analytic
    // `ground_distance` assumes, so aim/camera are unaffected. A unit cuboid collider scaled
    // by the Transform (the Avian idiom), buried so only the top surface is in play.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.32, 0.42, 0.28))),
        Transform::from_xyz(0.0, -GROUND_THICKNESS / 2.0, 0.0).with_scale(Vec3::new(
            GROUND_SIZE,
            GROUND_THICKNESS,
            GROUND_SIZE,
        )),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
        // Terrain layer: what the wheel suspension rays are allowed to hit.
        CollisionLayers::new([Layer::Terrain], LayerMask::ALL),
    ));

    // A first authored slope: a thick slab tilted about X, on the same Terrain layer as the ground
    // so the wheel rays read it identically. It's sunk so its low edge buries ~1 m under the ground
    // slab — the tilted top surface crosses y=0, giving a flush, step-free entry, then rises to a
    // crest. Purpose: put a real incline under the suspension (articulation, weight transfer), and
    // expose the missing static friction — with velocity-only grip, a parked tank runs away here.
    //
    // Low-edge top y = center_y + (thickness/2)·cosθ − (run/2)·sinθ; solve for center_y at −1 m.
    let (sin, cos) = RAMP_ANGLE_DEG.to_radians().sin_cos();
    let ramp_y = -1.0 - (RAMP_THICKNESS / 2.0) * cos + (RAMP_RUN / 2.0) * sin;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.45, 0.38, 0.28))),
        Transform::from_xyz(0.0, ramp_y, -20.0)
            .with_rotation(Quat::from_rotation_x(RAMP_ANGLE_DEG.to_radians()))
            .with_scale(Vec3::new(RAMP_WIDTH, RAMP_THICKNESS, RAMP_RUN)),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
        CollisionLayers::new([Layer::Terrain], LayerMask::ALL),
    ));
}

/// Distance along `ray` to the terrain, capped at `max`, falling back to `max` when the ray
/// misses (sky / above the horizon). A world raycast against the `Terrain` layer — the shared
/// ground query for aiming and the camera, now that terrain is more than the y=0 plane.
pub fn ground_distance(spatial: &SpatialQuery, ray: Ray3d, max: f32) -> f32 {
    spatial
        .cast_ray(
            ray.origin,
            ray.direction,
            max,
            true,
            &SpatialQueryFilter::from_mask(Layer::Terrain),
        )
        .map(|hit| hit.distance)
        .unwrap_or(max)
}
