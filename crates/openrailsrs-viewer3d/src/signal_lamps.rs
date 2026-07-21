//! Signal lamp quads from `sigcfg.dat` (#37).
//!
//! Spawns emissive discs for WORLD `Signal` heads. Aspect comes from the track
//! graph / live session; signalling logic is not altered.

use bevy::prelude::*;
use openrailsrs_formats::lit_light_indices_for_aspect;
use openrailsrs_or_shader::coordinates::msts_shape_vec3_to_bevy;
use openrailsrs_track::SignalAspect;

use crate::launch::ViewerSceneryMode;
use crate::shapes::RouteAssets;
use crate::track::TrackScene;
use crate::world::{RouteFocus, WorldObject, WorldScene};
// SignalPatch lives on WorldObject.

/// One emissive lamp quad belonging to a WORLD signal head.
#[derive(Component, Debug, Clone)]
pub struct SignalLamp {
    pub tr_item_id: u32,
    pub light_index: u32,
    pub signal_type: String,
}

/// Root marker for a WORLD signal's lamp set (despawn / stream accounting).
#[derive(Component, Debug, Clone)]
pub struct SignalLampRoot {
    pub tile_x: i32,
    pub tile_z: i32,
    pub uid: u32,
}

/// Spawn lamps for Signal objects in `objects` (startup or streamed batch).
#[allow(clippy::too_many_arguments)]
pub fn spawn_signal_lamp_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    objects: &[WorldObject],
    assets: &RouteAssets,
    focus: &RouteFocus,
    cull_center: Option<Vec3>,
) {
    let sigcfg = assets.sigcfg();
    if sigcfg.signal_shapes.is_empty() {
        return;
    }
    let mut spawned = 0usize;
    for obj in objects {
        if obj.kind != "Signal" {
            continue;
        }
        let Some(patch) = obj.signal.as_ref() else {
            continue;
        };
        if let Some(center) = cull_center {
            let dx = obj.position.x - center.x;
            let dz = obj.position.z - center.z;
            if dx * dx + dz * dz > crate::launch::view_radius_m().powi(2) {
                continue;
            }
        }
        let Some(shape_name) = obj.shape_file.as_deref() else {
            continue;
        };
        let Some(shape_def) = sigcfg.signal_shape(shape_name) else {
            continue;
        };
        let base = Transform {
            translation: focus.scenery_to_render(obj.position),
            rotation: obj.rotation,
            scale: obj.scale,
        };
        let root = commands
            .spawn((
                SignalLampRoot {
                    tile_x: obj.tile_x,
                    tile_z: obj.tile_z,
                    uid: obj.uid.unwrap_or(patch.uid),
                },
                Transform::IDENTITY,
                Visibility::default(),
                Name::new(format!("signal-lamps:{}:{}", shape_name, patch.uid)),
            ))
            .id();

        for unit in &patch.units {
            // Only heads installed in the WORLD bitmask (bit i → sub_obj i).
            if patch.signal_sub_obj != 0 && ((patch.signal_sub_obj >> unit.sub_obj) & 1) == 0 {
                continue;
            }
            let sub = shape_def
                .sub_objs
                .iter()
                .find(|s| s.index == unit.sub_obj)
                .or_else(|| shape_def.sub_objs.get(unit.sub_obj as usize));
            let Some(sub) = sub else {
                continue;
            };
            let Some(type_name) = sub.signal_type_name.as_deref() else {
                continue;
            };
            let Some(sig_type) = sigcfg.signal_type(type_name) else {
                continue;
            };
            let aspect = aspect_for_tr_item(assets, unit.tr_item_id);
            let lit = lit_light_indices_for_aspect(sig_type, aspect_to_code(aspect));
            for light in &sig_type.lights {
                let colour = sigcfg
                    .light_colour(&light.colour_name)
                    .map(|c| {
                        let rgb = c.to_linear_rgb();
                        Color::linear_rgb(rgb[0], rgb[1], rgb[2])
                    })
                    .unwrap_or(Color::srgb(1.0, 1.0, 1.0));
                let on = lit.contains(&light.index);
                // OR: Vector3(-X, Y, Z) then Bevy Z-flip → (-X, Y, -Z).
                let local = msts_shape_vec3_to_bevy(Vec3::new(
                    -light.position[0],
                    light.position[1],
                    light.position[2],
                ));
                let radius = light.radius.max(0.05);
                let mesh = meshes.add(Circle::new(radius));
                let material = materials.add(StandardMaterial {
                    base_color: colour,
                    emissive: if on {
                        LinearRgba::from(colour) * 4.0
                    } else {
                        LinearRgba::BLACK
                    },
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    double_sided: true,
                    cull_mode: None,
                    fog_enabled: true,
                    ..default()
                });
                let mut tf = base;
                tf.translation += base.rotation * local;
                // Face along signal forward (−Z local after placement).
                let type_owned = type_name.to_string();
                commands.entity(root).with_children(|parent| {
                    parent.spawn((
                        SignalLamp {
                            tr_item_id: unit.tr_item_id,
                            light_index: light.index,
                            signal_type: type_owned,
                        },
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        tf,
                        Name::new(format!(
                            "signal-lamp:{}:{}:{}",
                            unit.tr_item_id, light.index, light.colour_name
                        )),
                    ));
                });
                spawned += 1;
            }
        }
    }
    if spawned > 0 {
        crate::viewer_log!("openrailsrs-viewer3d: spawned {spawned} signal lamp(s)");
    }
}

/// Startup: lamps for signals already in [`WorldScene`].
#[allow(clippy::too_many_arguments)]
pub fn spawn_signal_lamps(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    assets: Res<RouteAssets>,
    focus: Res<RouteFocus>,
    mode: Res<ViewerSceneryMode>,
) {
    if !mode.loads_msts_scenery() {
        return;
    }
    spawn_signal_lamp_objects(
        &mut commands,
        &mut meshes,
        &mut materials,
        &world.items,
        &assets,
        &focus,
        None,
    );
}

fn aspect_to_code(aspect: SignalAspect) -> u8 {
    match aspect {
        SignalAspect::Stop => 0,
        SignalAspect::Caution => 1,
        SignalAspect::Clear => 2,
    }
}

fn aspect_for_tr_item(assets: &RouteAssets, tr_item_id: u32) -> SignalAspect {
    // Prefer graph signal `sig{id}` when present.
    // TrackScene is not passed here at spawn; use TDB initial aspect as fallback.
    if let Some(tdb) = assets.track_db() {
        if let Some(item) = tdb.items.iter().find(|i| i.id == tr_item_id) {
            if let openrailsrs_formats::TrItemKind::Signal { aspect_initial } = &item.kind {
                return match aspect_initial {
                    openrailsrs_formats::SignalAspectKind::Stop => SignalAspect::Stop,
                    openrailsrs_formats::SignalAspectKind::Caution => SignalAspect::Caution,
                    openrailsrs_formats::SignalAspectKind::Clear => SignalAspect::Clear,
                };
            }
        }
    }
    SignalAspect::Stop
}

/// Update lamp emissive from live / graph aspects.
pub fn update_signal_lamps(
    scene: Res<TrackScene>,
    live: Option<Res<crate::live::LiveDrive>>,
    assets: Res<RouteAssets>,
    lamps: Query<(&SignalLamp, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let sigcfg = assets.sigcfg();
    if sigcfg.signal_types.is_empty() {
        return;
    }
    for (lamp, mat_handle) in &lamps {
        let sig_id = format!("sig{}", lamp.tr_item_id);
        let aspect = live
            .as_ref()
            .and_then(|l| {
                if l.session.assume_signals_clear {
                    Some(SignalAspect::Clear)
                } else {
                    l.session.signal_aspect(&sig_id)
                }
            })
            .or_else(|| scene.graph.signal(&sig_id).map(|s| s.aspect))
            .unwrap_or_else(|| aspect_for_tr_item(&assets, lamp.tr_item_id));

        let Some(sig_type) = sigcfg.signal_type(&lamp.signal_type) else {
            continue;
        };
        let lit = lit_light_indices_for_aspect(sig_type, aspect_to_code(aspect));
        let on = lit.contains(&lamp.light_index);
        let Some(mut mat) = materials.get_mut(mat_handle) else {
            continue;
        };
        let base = mat.base_color;
        mat.emissive = if on {
            LinearRgba::from(base) * 4.0
        } else {
            LinearRgba::BLACK
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_codes_match_sigcfg_helper() {
        assert_eq!(aspect_to_code(SignalAspect::Stop), 0);
        assert_eq!(aspect_to_code(SignalAspect::Caution), 1);
        assert_eq!(aspect_to_code(SignalAspect::Clear), 2);
    }
}
