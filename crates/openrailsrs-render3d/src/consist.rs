//! Consist estatico del jugador (`.con`) en la posicion inicial de la via.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_train::{Vehicle, consist_asset_root, load_consist_with_asset_root};

use crate::objects::ObjectMarker;
use crate::or_scenery_material::OrSceneryMaterial;
use crate::player_spawn::PlayerStartPose;
use crate::textures::TextureEnvironment;
use crate::world_spawn::{
    AssetIndex, ObjectSpawnCtx, TextureLoadStats, spawn_consist_vehicle_shape,
};

/// Un vehiculo del consist listo para spawn 3D.
#[derive(Clone, Debug)]
pub struct ConsistVehicleVisual {
    pub name: String,
    pub shape_file: Option<String>,
    #[allow(dead_code)]
    pub length_m: f32,
    /// Metros detras de la cabeza del tren (negativo hacia cola).
    pub offset_m: f32,
    /// `.con` Flip — giro Y 180°; el orden lead→tail no cambia (#130).
    pub flipped: bool,
}

/// Plan de spawn cargado al arrancar (`--consist` o `.act`).
#[derive(Resource, Clone)]
pub struct StaticConsistPlan {
    pub vehicles: Vec<ConsistVehicleVisual>,
}

#[derive(Component)]
pub struct StaticConsistRoot;

pub fn load_consist_at_path(con_path: &Path) -> Option<Vec<ConsistVehicleVisual>> {
    if !con_path.is_file() {
        return None;
    }
    let asset_root = consist_asset_root(con_path);
    let consist = load_consist_with_asset_root(con_path, asset_root).ok()?;
    let vehicles = vehicles_from_consist(&consist);
    if vehicles.is_empty() {
        None
    } else {
        Some(vehicles)
    }
}

#[allow(dead_code)] // usado en tests
pub fn load_consist_vehicles(
    route_dir: &Path,
    consist_rel: &str,
) -> Option<Vec<ConsistVehicleVisual>> {
    load_consist_at_path(&route_dir.join(consist_rel))
}

fn vehicles_from_consist(consist: &openrailsrs_train::Consist) -> Vec<ConsistVehicleVisual> {
    let lengths: Vec<f32> = consist
        .vehicles
        .iter()
        .map(|vehicle| match vehicle {
            Vehicle::Loco(l) => l.length_m as f32,
            Vehicle::Wagon(w) => w.length_m as f32,
        })
        .collect();
    let offsets = longitudinal_offsets_m(&lengths);

    consist
        .vehicles
        .iter()
        .zip(offsets)
        .map(|(vehicle, offset_m)| match vehicle {
            Vehicle::Loco(l) => ConsistVehicleVisual {
                name: l.name.clone(),
                shape_file: l.wagon_shape.clone(),
                length_m: l.length_m as f32,
                offset_m,
                flipped: l.flipped,
            },
            Vehicle::Wagon(w) => ConsistVehicleVisual {
                name: w.name.clone(),
                shape_file: w.wagon_shape.clone(),
                length_m: w.length_m as f32,
                offset_m,
                flipped: w.flipped,
            },
        })
        .collect()
}

fn longitudinal_offsets_m(lengths: &[f32]) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(lengths.len());
    let mut behind = 0.0_f32;
    for (i, &len) in lengths.iter().enumerate() {
        if i == 0 {
            offsets.push(0.0);
        } else {
            offsets.push(-behind);
        }
        behind += len;
    }
    offsets
}

pub fn resolve_player_consist_path(
    route_dir: &Path,
    cli_consist: Option<&Path>,
    activity_consist: Option<&str>,
) -> Option<PathBuf> {
    if let Some(p) = cli_consist {
        if p.is_file() {
            return Some(p.to_path_buf());
        }
        let rel = p.to_string_lossy();
        let candidate = route_dir.join(rel.as_ref());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    activity_consist
        .filter(|s| !s.is_empty())
        .map(|rel| route_dir.join(rel))
        .filter(|p| p.is_file())
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_static_consist(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    images: &mut Assets<Image>,
    index: &AssetIndex,
    ctx: &mut ObjectSpawnCtx,
    route_dir: &Path,
    msts_root: &Path,
    plan: &StaticConsistPlan,
    pose: PlayerStartPose,
    texture_env: &TextureEnvironment,
    tex_stats: &mut TextureLoadStats,
) -> usize {
    let forward = Vec3::new(pose.yaw_rad.sin(), 0.0, pose.yaw_rad.cos());
    let root = commands
        .spawn((
            StaticConsistRoot,
            Transform::from_translation(pose.position)
                .with_rotation(Quat::from_rotation_y(pose.yaw_rad)),
            Name::new("player_consist"),
        ))
        .id();

    let mut parts_spawned = 0usize;
    for (vi, vehicle) in plan.vehicles.iter().enumerate() {
        let Some(shape_file) = vehicle
            .shape_file
            .as_deref()
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("test.s"))
        else {
            continue;
        };
        let vehicle_pos = pose.position + forward * vehicle.offset_m;
        let mut rotation = Quat::from_rotation_y(pose.yaw_rad);
        if vehicle.flipped {
            rotation *= Quat::from_rotation_y(std::f32::consts::PI);
        }
        let vehicle_tf = Transform {
            translation: vehicle_pos,
            rotation,
            scale: Vec3::ONE,
        };
        let obj = ObjectMarker {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            kind: crate::objects::ObjectKind::Static,
            file_name: Some(shape_file.to_string()),
            section_idx: None,
            dyntrack_sections: Vec::new(),
            forest: None,
            hwater: None,
            transfer: None,
        };
        parts_spawned += spawn_consist_vehicle_shape(
            commands,
            meshes,
            materials,
            or_materials,
            images,
            index,
            ctx,
            route_dir,
            msts_root,
            &obj,
            vehicle_tf,
            pose.position,
            texture_env,
            tex_stats,
            &format!("consist:{vi}:{}", vehicle.name),
        );
    }

    if parts_spawned > 0 {
        info!(
            "consist estatico: {} vehiculo(s), {} parte(s) de malla",
            plan.vehicles.len(),
            parts_spawned
        );
    } else {
        warn!("consist: ningun shape resolvio — revisa rutas SHAPES/trains/");
        commands.entity(root).despawn();
    }

    parts_spawned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_chain_nose_to_tail() {
        assert_eq!(longitudinal_offsets_m(&[18.0, 14.0]), vec![0.0, -18.0]);
    }

    #[test]
    fn smoke_freight_consist_loads() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
        let vehicles = load_consist_vehicles(&route, "consists/freight.con").expect("freight");
        assert_eq!(vehicles.len(), 2);
        assert_eq!(vehicles[1].offset_m, -18.0);
    }
}
