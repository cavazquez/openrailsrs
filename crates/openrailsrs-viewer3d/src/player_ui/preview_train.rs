//! Full formation geometry, loaded once per authored model and shared by cars.
use super::*;

pub(super) fn load(key: &TrainKey) -> Result<TrainResult, String> {
    bounded(&key.path)?;
    let source = crate::rolling_stock::try_load_consist_vehicles(
        key.path.parent().unwrap_or(&key.route),
        key.path.to_string_lossy().as_ref(),
    )
    .ok_or("No se puede cargar la formación: revisá los recursos faltantes")?;
    if source.is_empty() {
        return Err("La formación no tiene vehículos".into());
    }
    let mut models = vec![];
    let mut model_paths = HashMap::new();
    let mut textures = HashMap::new();
    let mut texture_bytes = 0;
    let mut texture_files = std::collections::HashSet::new();
    let mut geometry_bytes = 0usize;
    let mut last_error = String::new();
    let mut vehicles = vec![];
    let mut missing = 0;
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for vehicle in source.iter().take(256) {
        openrailsrs_formats::cancellation::check().map_err(|e| e.to_string())?;
        let transform = Transform::from_xyz(0.0, 0.0, -vehicle.offset_m).with_rotation(
            Quat::from_rotation_y(if vehicle.flipped {
                std::f32::consts::PI
            } else {
                0.0
            }),
        );
        let dirs = vehicle
            .asset_dir
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>();
        let loaded = (|| -> Result<(usize, Vec3, Vec3), String> {
            let path = crate::rolling_stock::resolve_consist_vehicle_shape_path(
                &dirs, vehicle, &key.route,
            )
            .ok_or("Falta el modelo original")?;
            let scope = vehicle
                .asset_dir
                .as_deref()
                .and_then(|dir| {
                    dir.ancestors().find(|p| {
                        p.file_name()
                            .is_some_and(|name| name.eq_ignore_ascii_case("TRAINS"))
                    })
                })
                .or_else(|| {
                    path.ancestors().find(|p| {
                        p.file_name().is_some_and(|name| {
                            name.eq_ignore_ascii_case("TRAINS")
                                || name.eq_ignore_ascii_case("ROUTES")
                        })
                    })
                })
                .and_then(Path::parent)
                .unwrap_or_else(|| path.parent().unwrap_or(&key.route));
            let path = confined_resource(&path, scope).ok_or("El modelo sale de su paquete")?;
            if let Some(&(index, min, max)) = model_paths.get(&path) {
                return Ok((index, min, max));
            }
            bounded(&path)?;
            let shape = ShapeFile::from_path(&path).map_err(|e| e.to_string())?;
            let mut parts = crate::shapes::build_mesh_parts_from_shape_at_distance(&shape, 0.0);
            if parts.is_empty() || parts.len() > 1024 || models.len() >= 64 {
                return Err("Límite de geometría de la vista previa".into());
            }
            let bytes = parts
                .iter()
                .map(|part| {
                    part.mesh.count_vertices().saturating_mul(64)
                        + part
                            .mesh
                            .indices()
                            .map_or(0, |indices| indices.len().saturating_mul(4))
                })
                .sum::<usize>();
            if geometry_bytes.saturating_add(bytes) > 96 * 1024 * 1024 {
                return Err("Geometría fuera del presupuesto de 96 MiB".into());
            }
            geometry_bytes += bytes;
            let (min, max) = crate::shapes::union_meshes_aabb(
                &parts.iter().map(|p| &p.mesh).collect::<Vec<_>>(),
            )
            .filter(|(min, max)| min.is_finite() && max.is_finite())
            .ok_or("Modelo sin dimensiones válidas")?;
            for part in &mut parts {
                openrailsrs_formats::cancellation::check().map_err(|e| e.to_string())?;
                let Some(name) = part.texture_file.as_ref() else {
                    continue;
                };
                let root = path.parent().unwrap_or(&key.route);
                let texture_path = [
                    root.join(name.replace('\\', "/")),
                    root.join("TEXTURES").join(name.replace('\\', "/")),
                ]
                .iter()
                .find_map(|p| confined_resource(p, scope));
                if let Some(path) = &texture_path {
                    texture_files.insert(path.clone());
                }
                let identity = texture_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("{}::{name}", path.display()));
                if !textures.contains_key(&identity)
                    && textures.len() < 128
                    && let Some(texture) = texture_path.and_then(|p| load_texture(&p).ok())
                {
                    let bytes = texture.0.data.as_ref().map_or(0, Vec::len);
                    if texture_bytes + bytes <= 128 * 1024 * 1024 {
                        texture_bytes += bytes;
                        textures.insert(identity.clone(), texture);
                    }
                }
                // Two vehicles may use the same texture filename in different packs.
                part.texture_file = Some(identity);
            }
            let index = models.len();
            models.push(parts);
            model_paths.insert(path, (index, min, max));
            Ok((index, min, max))
        })();
        let (model, min, max) = match loaded {
            Ok((model, min, max)) => (Some(model), min, max),
            Err(error) => {
                last_error = error;
                missing += 1;
                let half = vehicle.length_m.clamp(1.0, 100.0) * 0.5;
                (None, Vec3::new(-1.5, 0.0, -half), Vec3::new(1.5, 4.0, half))
            }
        };
        for x in [min.x, max.x] {
            for y in [min.y, max.y] {
                for z in [min.z, max.z] {
                    let corner = transform.transform_point(Vec3::new(x, y, z));
                    minimum = minimum.min(corner);
                    maximum = maximum.max(corner);
                }
            }
        }
        vehicles.push(PreviewVehicle {
            model,
            transform,
            name: vehicle.name.clone(),
            center: transform.transform_point((min + max) * 0.5),
            size: max - min,
        });
    }
    if models.is_empty() {
        return Err(format!("Sin vista previa 3D: {last_error}"));
    }
    let mut seen = std::collections::HashSet::new();
    let disk = source
        .iter()
        .filter_map(|v| v.stock_file.as_ref())
        .chain(model_paths.keys())
        .chain(texture_files.iter())
        .filter(|p| seen.insert((*p).clone()))
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    Ok(TrainResult {
        bytes: crate::resource_estimate::TrainBytes {
            disk,
            geometry: geometry_bytes as u64,
            textures: texture_bytes as u64,
        },
        models,
        textures,
        vehicles,
        center: (minimum + maximum) * 0.5,
        size: maximum - minimum,
        missing,
        truncated: source.len() > 256,
    })
}

pub(super) fn spawn(
    commands: &mut Commands,
    result: TrainResult,
    image: Handle<Image>,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> PreviewScene {
    let textures = result
        .textures
        .into_iter()
        .map(|(name, (image, alpha, mask))| (name, (images.add(image), alpha, mask)))
        .collect::<HashMap<_, _>>();
    let models = result
        .models
        .into_iter()
        .map(|parts| {
            parts
                .into_iter()
                .flat_map(|part| {
                    let texture = part
                        .texture_file
                        .as_ref()
                        .and_then(|name| textures.get(name))
                        .cloned();
                    let passes = openrailsrs_bevy_scenery::shapes::blend_alpha_passes_from_ace_bits(
                        texture.as_ref().map_or(0, |t| t.1),
                        texture.as_ref().is_some_and(|t| t.2),
                        part.texture_file.as_deref().unwrap_or(""),
                        part.shader_name.as_deref(),
                        part.alpha_test_mode,
                    );
                    let mesh = meshes.add(part.mesh);
                    passes
                        .into_iter()
                        .map(|pass| {
                            let color = part.solid_color.unwrap_or([1.0; 3]);
                            let material = materials.add(StandardMaterial {
                                base_color_texture: texture.as_ref().map(|t| t.0.clone()),
                                base_color: Color::srgb(color[0], color[1], color[2]),
                                alpha_mode: pass.alpha_mode,
                                perceptual_roughness: 0.85,
                                cull_mode: None,
                                double_sided: true,
                                ..default()
                            });
                            (mesh.clone(), material)
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let root = commands
        .spawn((Transform::default(), Visibility::default(), PreviewRoot))
        .id();
    commands.entity(root).with_children(|p| {
        for vehicle in &result.vehicles {
            if let Some(model) = vehicle.model {
                for (mesh, material) in &models[model] {
                    p.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        vehicle.transform,
                        RenderLayers::layer(PREVIEW_LAYER),
                    ));
                }
            } else {
                // A grey placeholder marks an absent model; it never substitutes assets at launch.
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(vehicle.size.x, vehicle.size.y, vehicle.size.z))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(0.18, 0.22, 0.27),
                        ..default()
                    })),
                    Transform::from_translation(vehicle.center),
                    RenderLayers::layer(PREVIEW_LAYER),
                ));
            }
        }
        p.spawn((
            Camera3d::default(),
            Camera {
                order: -10,
                clear_color: Color::srgb(0.07, 0.10, 0.14).into(),
                ..default()
            },
            RenderTarget::Image(image.into()),
            Transform::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: 45.0_f32.to_radians(),
                ..default()
            }),
            RenderLayers::layer(PREVIEW_LAYER),
            Msaa::Off,
            PreviewCamera,
        ));
        let radius = result.size.length().max(10.0);
        p.spawn((
            DirectionalLight {
                illuminance: 15_000.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(result.center + Vec3::new(-radius, radius, -radius))
                .looking_at(result.center, Vec3::Y),
            RenderLayers::layer(PREVIEW_LAYER),
        ));
    });
    PreviewScene {
        center: result.center,
        size: result.size,
        vehicles: result.vehicles,
        missing: result.missing,
        truncated: result.truncated,
    }
}
