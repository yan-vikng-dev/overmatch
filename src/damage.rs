//! Damage consequences: interpret component HP loss as crew death, ammo cookoff, tank knockout, and
//! function loss. Ballistics owns *how* damage is deposited; this module owns what depleted
//! damageable volumes mean.

use avian3d::prelude::{
    AngularInertia, AngularVelocity, LinearVelocity, Mass, NoAutoAngularInertia, NoAutoMass,
    RigidBody,
};
use bevy::prelude::*;
use serde::Deserialize;

use crate::ballistics::ComponentHealth;
use crate::state::GameplaySet;
use crate::tank::{ServoCommand, ServoSpec, ServoState, Turret};

/// Semantic ownership: a ballistic volume belongs to a tank for gameplay aggregation. This is
/// separate from `ChildOf`, which remains the model/transform hierarchy.
#[derive(Component, Debug)]
#[relationship(relationship_target = TankVolumes)]
pub struct VolumeOf(pub Entity);

impl VolumeOf {
    pub fn tank(&self) -> Entity {
        self.0
    }
}

/// Reverse collection for [`VolumeOf`]. Bevy keeps this synchronized; read it, don't mutate it.
#[derive(Component, Debug)]
#[relationship_target(relationship = VolumeOf)]
pub struct TankVolumes(Vec<Entity>);

impl TankVolumes {
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }
}

/// A crew volume's station/function. Reaching 0 HP incapacitates this crewman; the tank is knocked
/// out when fewer than two crew remain living.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum CrewStation {
    Commander,
    Gunner,
    Loader,
    Driver,
    BowGunner,
}

impl CrewStation {
    pub fn label(self) -> &'static str {
        match self {
            CrewStation::Commander => "Commander",
            CrewStation::Gunner => "Gunner",
            CrewStation::Loader => "Loader",
            CrewStation::Driver => "Driver",
            CrewStation::BowGunner => "Bow gunner",
        }
    }
}

/// An ammunition volume. Depletion triggers a cookoff: all crew die immediately.
#[derive(Component)]
pub struct Ammo;

/// A repairable module function served by this volume. Function loss is derived from its HP for now.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum FunctionRole {
    Engine,
    Transmission,
    Breech,
    GunBarrel,
}

impl FunctionRole {
    pub fn label(self) -> &'static str {
        match self {
            FunctionRole::Engine => "Engine",
            FunctionRole::Transmission => "Transmission",
            FunctionRole::Breech => "Breech",
            FunctionRole::GunBarrel => "Gun barrel",
        }
    }
}

/// Latched crew state: this crewman has been incapacitated. Later replacement/recrew mechanics can
/// explicitly clear or replace it; normal play does not.
#[derive(Component)]
pub struct Incapacitated;

/// Latched ammo event: this ammunition volume has cooked off. Prevents repeated cookoff processing.
#[derive(Component)]
pub struct CookedOff;

/// Latched tank terminal state. Normal play never removes this; sandbox reset may.
#[derive(Component)]
pub struct TankKnockedOut {
    pub reason: KnockoutReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnockoutReason {
    CrewLoss,
    Cookoff,
}

impl KnockoutReason {
    pub fn label(self) -> &'static str {
        match self {
            KnockoutReason::CrewLoss => "crew knockout",
            KnockoutReason::Cookoff => "ammo cookoff",
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            process_cookoffs,
            incapacitate_crew,
            knock_out_crewless_tanks,
            launch_turrets_on_cookoff,
        )
            .chain()
            .in_set(GameplaySet),
    );
}

pub fn function_disabled(
    tank_volumes: Option<&TankVolumes>,
    role: FunctionRole,
    functions: &Query<(&FunctionRole, &ComponentHealth)>,
) -> bool {
    let Some(tank_volumes) = tank_volumes else {
        return false;
    };
    tank_volumes.iter().any(|volume| {
        functions
            .get(volume)
            .is_ok_and(|(function, hp)| *function == role && hp.current <= 0.0)
    })
}

fn process_cookoffs(
    ammo: Query<(Entity, &ComponentHealth, &VolumeOf), (With<Ammo>, Without<CookedOff>)>,
    mut crew: Query<(&VolumeOf, &mut ComponentHealth), (With<CrewStation>, Without<Ammo>)>,
    mut commands: Commands,
) {
    for (ammo_entity, hp, owner) in &ammo {
        if hp.current > 0.0 {
            continue;
        }
        commands.entity(ammo_entity).insert(CookedOff);
        commands.entity(owner.tank()).insert(TankKnockedOut {
            reason: KnockoutReason::Cookoff,
        });
        for (crew_owner, mut crew_hp) in &mut crew {
            if crew_owner.tank() == owner.tank() {
                crew_hp.current = 0.0;
            }
        }
    }
}

#[derive(Component)]
pub struct LaunchedTurret;

fn launch_turrets_on_cookoff(
    knocked_out: Query<(Entity, &TankKnockedOut), Added<TankKnockedOut>>,
    turrets: Query<(Entity, &GlobalTransform), (With<Turret>, Without<LaunchedTurret>)>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    for (tank, knocked_out) in &knocked_out {
        if knocked_out.reason != KnockoutReason::Cookoff {
            continue;
        }
        for (turret, global) in &turrets {
            if !is_descendant_of(turret, tank, &parents) {
                continue;
            }
            let pose = global.compute_transform();
            let up = Vec3::from(global.up());
            let right = Vec3::from(global.right());
            let forward = Vec3::from(global.forward());
            const TURRET_MASS: f32 = 8_000.0;
            commands.entity(turret).insert((
                pose,
                RigidBody::Dynamic,
                Mass(TURRET_MASS),
                AngularInertia::from_shape(&Cuboid::new(3.0, 1.2, 2.4), TURRET_MASS),
                NoAutoMass,
                NoAutoAngularInertia,
                LinearVelocity(up * 14.0 + forward * 3.0),
                AngularVelocity(right * 3.0 + up * 1.2),
                LaunchedTurret,
            ));
            commands
                .entity(turret)
                .remove::<(ChildOf, Turret, ServoCommand, ServoState, ServoSpec)>();
        }
    }
}

fn is_descendant_of(mut entity: Entity, ancestor: Entity, parents: &Query<&ChildOf>) -> bool {
    while let Ok(parent) = parents.get(entity) {
        entity = parent.parent();
        if entity == ancestor {
            return true;
        }
    }
    false
}

fn incapacitate_crew(
    crew: Query<(Entity, &ComponentHealth), (With<CrewStation>, Without<Incapacitated>)>,
    mut commands: Commands,
) {
    for (entity, hp) in &crew {
        if hp.current <= 0.0 {
            commands.entity(entity).insert(Incapacitated);
        }
    }
}

fn knock_out_crewless_tanks(
    tanks: Query<(Entity, &TankVolumes), Without<TankKnockedOut>>,
    volumes: Query<(Option<&CrewStation>, Option<&Incapacitated>)>,
    mut commands: Commands,
) {
    for (tank, tank_volumes) in &tanks {
        let mut crew_total = 0;
        let mut crew_living = 0;
        for volume in tank_volumes.iter() {
            let Ok((station, incapacitated)) = volumes.get(volume) else {
                continue;
            };
            if station.is_some() {
                crew_total += 1;
                if incapacitated.is_none() {
                    crew_living += 1;
                }
            }
        }
        if crew_total > 0 && crew_living < 2 {
            commands.entity(tank).insert(TankKnockedOut {
                reason: KnockoutReason::CrewLoss,
            });
        }
    }
}
