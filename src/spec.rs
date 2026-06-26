//! Per-variant spec sheets as RON data assets (ADR-0010). The Blender model owns geometry and
//! spatial anchors; this owns the tuning numbers — mass + inertia, drivetrain, suspension, servo
//! configs — that differ per tank variant. A `.tank.ron` file deserializes (via serde) straight
//! into the same components the sim reads (`Mass`, `Drivetrain`, `SuspensionParams`, `ServoSpec`), so
//! values stay plain-text, git-diffable, and hot-reloadable, with no recompile and no Blender
//! round-trip. There are **no code defaults** (ADR-0011): a competitive sim never runs on guessed
//! stats, so a failed load is fatal. The spec is a *load dependency* — the tank is spawned only
//! once it's loaded — so `tank::on_tank_ready` binds its values onto the rig in a single pass.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext, LoadState};
use bevy::prelude::*;
use serde::Deserialize;

use crate::driving::{Drivetrain, SuspensionParams};
use crate::tank::{ServoSpec, Tank};

/// One tank variant's spec sheet — the typed contents of a `.tank.ron` file. Its fields *are* the
/// components the sim consumes; `tank::apply_tank_spec` copies them onto the rig once ready.
#[derive(Asset, TypePath, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TankSpec {
    /// Total mass (kg) — authored balance data; the collision proxy contributes none (ADR-0011).
    pub mass: f32,
    /// Hull box full dimensions (x, y, z metres) approximating the angular-inertia distribution.
    pub inertia_extents: (f32, f32, f32),
    pub drivetrain: Drivetrain,
    pub suspension: SuspensionParams,
    pub turret: ServoSpec,
    pub gun: ServoSpec,
}

/// The handle to a tank's spec sheet, carried on its root entity so each tank knows its variant
/// (multi-variant ready). `spawn_tank` loads it alongside the model.
#[derive(Component)]
pub struct TankSpecHandle(pub Handle<TankSpec>);

/// Parses a `.tank.ron` file into a [`TankSpec`]. Tiny by design — the work is serde + RON.
#[derive(TypePath)]
struct TankSpecLoader;

impl AssetLoader for TankSpecLoader {
    type Asset = TankSpec;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<TankSpec, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(ron::de::from_bytes(&bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        &["tank.ron"]
    }
}

pub fn plugin(app: &mut App) {
    app.init_asset::<TankSpec>()
        .register_asset_loader(TankSpecLoader)
        .add_systems(Update, report_failed_spec);
}

/// Surface a failed spec-sheet load instead of swallowing it. The `.tank.ron` is required, in-repo
/// config with **no fallback** (ADR-0011): a competitive sim must never run on guessed stats, so a
/// parse/schema/IO error is fatal — we log the carried `AssetLoadError` and **panic in every
/// build**. (The schema test catches this class pre-ship; this is the runtime backstop for a bad
/// hot-reload or a file that slipped through.)
fn report_failed_spec(asset_server: Res<AssetServer>, tank: Query<&TankSpecHandle, With<Tank>>) {
    for handle in &tank {
        if let LoadState::Failed(err) = asset_server.load_state(&handle.0) {
            error!("required tank spec sheet failed to load: {err}");
            panic!("required tank spec sheet failed to load: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped spec sheet must always deserialize into `TankSpec`. This catches schema drift —
    /// a renamed/removed field, a changed type, a bad enum variant — at `cargo test` time, before
    /// the bad file ever ships (where `report_failed_spec` would catch it at runtime instead, but
    /// only after a player already has it). With `deny_unknown_fields`, a stray/typo'd key fails
    /// here too instead of being silently ignored.
    #[test]
    fn tiger_1_spec_sheet_matches_schema() {
        let ron = include_str!("../assets/tiger_1/tiger_1.tank.ron");
        let spec: TankSpec =
            ron::de::from_str(ron).expect("tiger_1.tank.ron must deserialize into TankSpec");
        // Spot-check values across sections so the test exercises real field wiring, not just "it
        // parsed".
        assert_eq!(spec.mass, 46500.0);
        assert_eq!(spec.inertia_extents, (3.0, 2.0, 6.3));
        assert_eq!(spec.drivetrain.max_thrust, 12500.0);
        assert_eq!(spec.suspension.stiffness, 450_000.0);
    }
}
