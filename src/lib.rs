//! Overmatch — a realistic 3D multiplayer tank game (single-player vertical slice).
//!
//! Organized one plugin per feature. `GamePlugin` composes them; `main.rs` only supplies
//! the runtime and runs the app. Each feature module owns its components, systems, and its
//! own wiring (a `pub fn plugin(app: &mut App)`), so this list reads as a table of contents.

use avian3d::prelude::{PhysicsInterpolationPlugin, PhysicsLayer, PhysicsPlugins};
use bevy::prelude::*;
use bevy_skein::SkeinPlugin;

mod aim;
mod camera;
#[cfg(debug_assertions)]
mod debug;
mod driving;
mod shooting;
mod state;
mod tank;
mod world;

/// Physics collision layers. Wheel suspension rays filter to `Terrain` only, so they ignore
/// the vehicle's own hull collider (ADR-0005). Shared infra, hence at the crate root.
#[derive(PhysicsLayer, Default, Clone, Copy, Debug)]
pub(crate) enum Layer {
    #[default]
    Default,
    Terrain,
    Vehicle,
}

/// Every gameplay feature, composed. Add to an `App` that already has the runtime plugins
/// (`DefaultPlugins` for the game, `MinimalPlugins` for headless tests).
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            // Avian physics — foundational infra for world/tank/shooting. Runs in
            // FixedPostUpdate by default, consistent with our sim-in-fixed bet (ADR-0004).
            // `interpolate_all` renders bodies at an interpolated pose between fixed steps, so
            // motion stays smooth when the display rate differs from the physics tick rate.
            PhysicsPlugins::default().set(PhysicsInterpolationPlugin::interpolate_all()),
            // Skein: instantiate reflected components authored on the Blender model from glTF
            // extras (ADR-0007). `default()` also serves the registry over BRP for the Blender
            // addon — but only in dev builds; release loads a static glTF with no BRP server.
            SkeinPlugin::default(),
            state::plugin,
            world::plugin,
            tank::plugin,
            driving::plugin,
            camera::plugin,
            aim::plugin,
            shooting::plugin,
        ));

        // Dev-only physics visualization (collider/ray wireframes) + debug toggles. Off in release
        // builds. (Skein's BRP endpoint for Blender authoring is added by `SkeinPlugin` itself,
        // also dev-only — see above.)
        #[cfg(debug_assertions)]
        app.add_plugins((
            avian3d::prelude::PhysicsDebugPlugin::default(),
            debug::plugin,
        ));
    }
}
