//! Collector presentation derived from original animation bounds and declared
//! wire height. Physical voltage remains authoritative and independent of LOD.
use crate::rolling_stock_anim::{RollingStockPartKind, TrainCarTrackOffset, TrainKeyedAnim};
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use openrailsrs_bevy_scenery::shapes::{
    ShapeAnimBinding, animation_pose_matrices, world_baked_anim_transform,
};
use std::collections::HashMap;

#[derive(Clone)]
struct CollectorCurve {
    heights: [f32; 17],
    point: Vec3,
    part_count: usize,
}
impl CollectorCurve {
    fn fraction(&self, height: f32) -> Option<f32> {
        if !height.is_finite()
            || height < self.heights[0]
            || height > self.heights[16] + 0.08
            || self.heights[16] - self.heights[0] < 0.2
            || self.heights.windows(2).any(|w| w[1] < w[0] - 0.02)
        {
            return None;
        }
        for i in 0..16 {
            if height <= self.heights[i + 1] {
                return Some(
                    (i as f32
                        + (height - self.heights[i])
                            / (self.heights[i + 1] - self.heights[i]).max(0.001))
                        / 16.0,
                );
            }
        }
        Some(1.0)
    }
}
#[derive(Default)]
struct Contact {
    clock: Option<f64>,
    voltage: f64,
    loaded: bool,
    last_arc: f64,
    flash_until: f64,
    point: Vec3,
    aligned: bool,
}
#[derive(Resource, Default)]
pub struct ElectricContacts {
    curves: HashMap<Entity, CollectorCurve>,
    targets: HashMap<Entity, f32>,
    contacts: HashMap<Entity, Contact>,
    pub arc_events: u64,
    pub out_of_range: usize,
}
impl ElectricContacts {
    pub fn fraction(&self, car: Entity, physical: f32) -> f32 {
        self.targets.get(&car).copied().unwrap_or(1.0) * physical
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"animated_collectors":self.curves.len(),"aligned_collectors":self.contacts.values().filter(|c|c.aligned).count(),
            "wire_out_of_model_range":self.out_of_range,"arc_events":self.arc_events,
            "contact_source":"physical route voltage, original animation bounds and TRK wire height","supply_independent_of_scenery_lod":true})
    }
}

#[allow(clippy::type_complexity)]
pub fn prepare_geometry(
    wire: Res<crate::overhead_wire::RouteWireConfig>,
    live: Res<crate::live::LiveDrive>,
    meshes: Res<Assets<Mesh>>,
    parts: Query<(&Mesh3d, &ShapeAnimBinding, &TrainKeyedAnim, &ChildOf)>,
    cars: Query<(
        Entity,
        &TrainCarTrackOffset,
        &crate::rolling_stock::ConsistCarIndex,
    )>,
    mut contacts: ResMut<ElectricContacts>,
) {
    contacts.targets.clear();
    contacts.out_of_range = 0;
    if !wire.enabled || !(0.5..30.0).contains(&wire.style.height_m) {
        return;
    }
    let mut active = Vec::new();
    for (entity, offset, index) in &cars {
        let Some(session) = live.session_for_track(offset.track_index) else {
            continue;
        };
        if !session.physics.electric.cars.iter().any(|c| {
            c.vehicle == index.0
                && c.params.pickup == openrailsrs_core::electrification::ElectricPickup::Overhead
        }) {
            continue;
        }
        active.push(entity);
        let part_count = parts
            .iter()
            .filter(|(_, _, k, p)| {
                p.parent() == entity && k.kind == RollingStockPartKind::Pantograph
            })
            .count();
        if contacts
            .curves
            .get(&entity)
            .is_some_and(|c| c.part_count != part_count)
        {
            contacts.curves.remove(&entity);
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = contacts.curves.entry(entity) {
            let mut heights = [f32::NEG_INFINITY; 17];
            let mut point = Vec3::ZERO;
            let mut complete = true;
            for (mesh, binding, keyed, parent) in &parts {
                if parent.parent() != entity
                    || keyed.kind != RollingStockPartKind::Pantograph
                    || binding.frame_count <= 0.0
                {
                    continue;
                }
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) = meshes
                    .get(&mesh.0)
                    .and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION))
                else {
                    complete = false;
                    continue;
                };
                for (i, height) in heights.iter_mut().enumerate() {
                    let pose = animation_pose_matrices(
                        &binding.shape,
                        crate::rolling_stock_anim::key_from_frac(
                            i as f32 / 16.0,
                            binding.frame_count,
                        ),
                    );
                    let transform = world_baked_anim_transform(
                        Transform::IDENTITY,
                        &binding.shape,
                        keyed.matrix_idx,
                        &pose,
                    );
                    // A rotated Aabb can overestimate the collector height by
                    // metres. Use the actual rest-baked vertices, cached once.
                    for vertex in vertices {
                        let vertex = transform
                            .to_matrix()
                            .transform_point3(Vec3::from_array(*vertex));
                        if vertex.y > *height {
                            *height = vertex.y;
                            if i == 16 {
                                point = vertex;
                            }
                        }
                    }
                }
            }
            if complete && heights.iter().all(|h| h.is_finite()) {
                entry.insert(CollectorCurve {
                    heights,
                    point,
                    part_count,
                });
            }
        }
        if let Some(curve) = contacts.curves.get(&entity) {
            if let Some(fraction) = curve.fraction(wire.style.height_m) {
                contacts.targets.insert(entity, fraction);
            } else {
                contacts.out_of_range += 1;
            }
        }
    }
    contacts.curves.retain(|e, _| active.contains(e));
}

fn arc_transition(old: f64, new: f64, previously_loaded: bool, loaded: bool) -> bool {
    (old > 0.0 && new <= 0.0 && previously_loaded) || (old <= 0.0 && new > 0.0 && loaded)
}
#[derive(Component)]
pub struct ContactFlash(Entity);

pub fn spawn_flashes(
    mut commands: Commands,
    cars: Query<(Entity, &TrainCarTrackOffset)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(arc_mesh());
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.7, 0.85, 1.0),
        emissive: LinearRgba::new(15.0, 25.0, 45.0, 1.0),
        unlit: true,
        cull_mode: None,
        ..default()
    });
    for (car, _) in &cars {
        commands.spawn((
            ContactFlash(car),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            Visibility::Hidden,
            NotShadowCaster,
            PointLight {
                color: Color::srgb(0.6, 0.8, 1.0),
                intensity: 0.0,
                range: 3.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Name::new("collector contact arc"),
        ));
    }
}
fn arc_mesh() -> Mesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (i, (x, y)) in [
        (-0.12, 0.0),
        (-0.07, 0.07),
        (-0.025, 0.02),
        (0.03, 0.11),
        (0.065, 0.04),
        (0.13, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        positions.extend([[x, y - 0.008, 0.0], [x, y + 0.008, 0.0]]);
        if i > 0 {
            let n = (i * 2) as u32;
            indices.extend([n - 2, n - 1, n, n - 1, n + 1, n]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vec![[0.0, 0.0, 1.0]; positions.len()],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[allow(clippy::type_complexity)]
pub fn update_contacts(
    live: Res<crate::live::LiveDrive>,
    wire: Res<crate::overhead_wire::RouteWireConfig>,
    mut state: ResMut<ElectricContacts>,
    motion: Option<Res<crate::train_motion::TrainMotion>>,
    cars: Query<
        (
            Entity,
            &GlobalTransform,
            &TrainCarTrackOffset,
            &crate::rolling_stock::ConsistCarIndex,
        ),
        Without<ContactFlash>,
    >,
    mut flashes: Query<(
        &ContactFlash,
        &mut Transform,
        &mut GlobalTransform,
        &mut Visibility,
        &mut PointLight,
    )>,
) {
    let mut events = 0;
    for (entity, transform, offset, index) in &cars {
        let Some(session) = live.session_for_track(offset.track_index) else {
            continue;
        };
        let Some(electric) = session
            .state
            .electric
            .cars
            .iter()
            .find(|c| c.vehicle == index.0)
        else {
            continue;
        };
        let curve = state.curves.get(&entity).cloned();
        let aligned = wire.enabled
            && state.targets.contains_key(&entity)
            && electric.pantograph_fraction > 0.99;
        let contact = state.contacts.entry(entity).or_default();
        let clock = session.time_s();
        let loaded = session.state.throttle > 0.05 && electric.main_power;
        if contact.clock.is_some_and(|previous| clock < previous) {
            *contact = Contact::default();
        }
        if !live.paused
            && contact.clock.is_some()
            && clock > contact.clock.unwrap()
            && curve.is_some()
            && (aligned || contact.aligned)
            && clock - contact.last_arc >= 0.5
            && arc_transition(
                contact.voltage,
                electric.contact_voltage_v,
                contact.loaded,
                loaded,
            )
        {
            contact.last_arc = clock;
            contact.flash_until = clock + 0.08;
            events += 1;
        }
        if let Some(curve) = curve {
            let mut point = curve.point;
            point.y = wire.style.height_m;
            contact.point = motion
                .as_ref()
                .map_or(*transform, |m| m.rigid_transform(entity, *transform))
                .transform_point(point);
        }
        contact.clock = Some(clock);
        contact.voltage = electric.contact_voltage_v;
        contact.loaded = loaded;
        contact.aligned = aligned;
    }
    state.arc_events += events;
    for (flash, mut transform, mut global, mut visibility, mut light) in &mut flashes {
        let visible = state
            .contacts
            .get(&flash.0)
            .is_some_and(|c| c.clock.is_some_and(|t| t < c.flash_until));
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        light.intensity = if visible { 150.0 } else { 0.0 };
        if let Some(contact) = state.contacts.get(&flash.0) {
            transform.translation = contact.point;
        }
        if let Ok((_, car, _, _)) = cars.get(flash.0) {
            transform.rotation = car.rotation();
        }
        // These flashes are roots and update after propagation. Publish their
        // world pose now so a short arc never renders at last frame's origin.
        global.set_if_neq(GlobalTransform::from(*transform));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collector_uses_authored_travel_and_never_stretches_out_of_range() {
        let curve = CollectorCurve {
            heights: std::array::from_fn(|i| 3.0 + i as f32 / 8.0),
            point: Vec3::Y * 5.0,
            part_count: 1,
        };
        assert_eq!(curve.fraction(4.0), Some(0.5));
        assert_eq!(curve.fraction(6.0), None);
        assert_eq!(curve.fraction(2.0), None);
        let mut reversed = curve.clone();
        reversed.heights[10] = 3.0;
        assert_eq!(reversed.fraction(4.0), None);
    }
    #[test]
    fn arcs_require_physical_contact_transition_and_actual_load() {
        assert!(arc_transition(25000.0, 0.0, true, false));
        assert!(arc_transition(0.0, 25000.0, false, true));
        assert!(!arc_transition(25000.0, 25000.0, true, true));
        assert!(!arc_transition(25000.0, 0.0, false, false));
        assert!(!arc_transition(0.0, 25000.0, false, false));
    }
}
