//! Signal lamp quads from `sigcfg.dat` (#37).
//!
//! Spawns emissive discs for WORLD `Signal` heads. Aspect comes from the track
//! graph / live session; signalling logic is not altered.

use std::collections::HashMap;

use bevy::light::{NotShadowCaster, NotShadowReceiver};
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
    /// Runtime graph/sim key, built once instead of formatting every frame.
    pub signal_id: String,
    /// Whether this physical lamp is lit for Stop / Caution / Clear.
    pub lit_for_aspect: [bool; 3],
    pub fallback_aspect: SignalAspect,
    /// Last state already uploaded to the material asset.
    pub is_on: bool,
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
    origin: &crate::floating_origin::FloatingOrigin,
) {
    let sigcfg = assets.sigcfg();
    if sigcfg.signal_shapes.is_empty() {
        return;
    }
    let mut spawned = 0usize;
    // Signal configs reuse a small palette of colours/radii. Sharing these assets
    // lets Bevy batch visible lamps instead of creating one mesh and material per
    // disc (1k+ at a large station).
    let mut mesh_cache: HashMap<u32, Handle<Mesh>> = HashMap::new();
    let mut material_cache: HashMap<[u32; 4], Handle<StandardMaterial>> = HashMap::new();
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
            if dx * dx + dz * dz > crate::world::shape_mesh_radius_m().powi(2) {
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
            translation: obj.render_position(focus),
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
                Transform::from_translation(-crate::floating_origin::horizontal_shift(
                    origin.shift,
                )),
                crate::world::WorldTileBound::new(obj.tile_x, obj.tile_z),
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
            let lit_by_aspect = [
                lit_light_indices_for_aspect(sig_type, 0),
                lit_light_indices_for_aspect(sig_type, 1),
                lit_light_indices_for_aspect(sig_type, 2),
            ];
            let signal_id = format!("sig{}", unit.tr_item_id);
            for light in &sig_type.lights {
                let colour = sigcfg
                    .light_colour(&light.colour_name)
                    .map(|c| {
                        let rgb = c.to_linear_rgb();
                        Color::linear_rgb(rgb[0], rgb[1], rgb[2])
                    })
                    .unwrap_or(Color::srgb(1.0, 1.0, 1.0));
                let lit_for_aspect =
                    std::array::from_fn(|idx| lit_by_aspect[idx].contains(&light.index));
                let on = lit_for_aspect[aspect_to_code(aspect) as usize];
                // OR: Vector3(-X, Y, Z) then Bevy Z-flip → (-X, Y, -Z).
                let local = msts_shape_vec3_to_bevy(Vec3::new(
                    -light.position[0],
                    light.position[1],
                    light.position[2],
                ));
                let radius = light.radius.max(0.05);
                let mesh = mesh_cache
                    .entry(radius.to_bits())
                    .or_insert_with(|| meshes.add(Circle::new(radius)))
                    .clone();
                let rgba = colour.to_srgba().to_f32_array().map(f32::to_bits);
                let material = material_cache
                    .entry(rgba)
                    .or_insert_with(|| {
                        materials.add(StandardMaterial {
                            base_color: colour,
                            // The physical unlit lens is part of the signal shape.
                            // This overlay exists only for the currently lit aspect.
                            emissive: LinearRgba::from(colour) * 4.0,
                            unlit: true,
                            alpha_mode: AlphaMode::Blend,
                            double_sided: true,
                            cull_mode: None,
                            fog_enabled: true,
                            ..default()
                        })
                    })
                    .clone();
                let mut tf = base;
                tf.translation += base.rotation * local;
                // Face along signal forward (−Z local after placement).
                commands.entity(root).with_children(|parent| {
                    parent.spawn((
                        SignalLamp {
                            signal_id: signal_id.clone(),
                            lit_for_aspect,
                            fallback_aspect: aspect,
                            is_on: on,
                        },
                        NotShadowCaster,
                        NotShadowReceiver,
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        tf,
                        if on {
                            Visibility::Visible
                        } else {
                            Visibility::Hidden
                        },
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
        &crate::floating_origin::FloatingOrigin::default(),
    );
}

fn aspect_to_code(aspect: SignalAspect) -> u8 {
    match aspect {
        SignalAspect::Stop => 0,
        SignalAspect::Caution => 1,
        SignalAspect::Clear => 2,
    }
}

pub(crate) fn aspect_for_tr_item(assets: &RouteAssets, tr_item_id: u32) -> SignalAspect {
    // Prefer graph signal `sig{id}` when present.
    // TrackScene is not passed here at spawn; use TDB initial aspect as fallback.
    if let Some(tdb) = assets.track_db()
        && let Some(item) = tdb.items.iter().find(|i| i.id == tr_item_id)
        && let openrailsrs_formats::TrItemKind::Signal { aspect_initial } = &item.kind
    {
        return match aspect_initial {
            openrailsrs_formats::SignalAspectKind::Stop => SignalAspect::Stop,
            openrailsrs_formats::SignalAspectKind::Caution => SignalAspect::Caution,
            openrailsrs_formats::SignalAspectKind::Clear => SignalAspect::Clear,
        };
    }
    SignalAspect::Stop
}

/// Update lamp emissive from live / graph aspects.
pub fn update_signal_lamps(
    scene: Res<TrackScene>,
    live: Option<Res<crate::live::LiveDrive>>,
    mut lamps: Query<(&mut SignalLamp, &mut Visibility)>,
) {
    for (mut lamp, mut visibility) in &mut lamps {
        let aspect = runtime_aspect(
            &scene,
            live.as_deref(),
            &lamp.signal_id,
            lamp.fallback_aspect,
        );
        let on = lamp.lit_for_aspect[aspect_to_code(aspect) as usize];
        if lamp.is_on == on {
            continue;
        }
        *visibility = if on {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        lamp.is_on = on;
    }
}

pub(crate) fn runtime_aspect(
    scene: &TrackScene,
    live: Option<&crate::live::LiveDrive>,
    signal_id: &str,
    fallback: SignalAspect,
) -> SignalAspect {
    live.and_then(|l| {
        if l.session.assume_signals_clear {
            Some(SignalAspect::Clear)
        } else {
            l.session.signal_aspect(signal_id)
        }
    })
    .or_else(|| scene.graph.signal(signal_id).map(|s| s.aspect))
    .unwrap_or(fallback)
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
