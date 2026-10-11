//! Bounded, asynchronous menu previews from the installed author's assets.
use super::*;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use bevy::render::render_resource::TextureFormat;
use openrailsrs_core::electrification::{ElectricPickup, ElectricSupply};
use openrailsrs_formats::{RouteFile, ShapeFile, resolve_path_case_insensitive};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, mpsc};

const PREVIEW_LAYER: usize = 31;
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

struct Worker<K, T> {
    tx: mpsc::Sender<K>,
    rx: Mutex<mpsc::Receiver<(K, Result<T, String>)>>,
}
fn worker<K: Send + 'static, T: Send + 'static>(load: fn(&K) -> Result<T, String>) -> Worker<K, T> {
    let (tx, requests) = mpsc::channel::<K>();
    let (results, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(mut key) = requests.recv() {
            // Browsing rapidly only queues the latest selection, not many loaders.
            for newer in requests.try_iter() {
                key = newer;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load(&key)))
                .unwrap_or_else(|_| {
                    Err("No se pudo interpretar el recurso de la vista previa".into())
                });
            if results.send((key, result)).is_err() {
                break;
            }
        }
    });
    Worker {
        tx,
        rx: Mutex::new(rx),
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct RouteKey {
    root: PathBuf,
    scenario: Option<PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct TrainKey {
    path: PathBuf,
    route: PathBuf,
}
#[derive(Clone, Debug)]
pub(super) struct RailwayInfo {
    pub gauge_m: f64,
    pub supply: ElectricSupply,
    pub section_supplies: Vec<ElectricSupply>,
    pub missing: Vec<String>,
}
struct RouteResult {
    railway: RailwayInfo,
    image: Option<Image>,
}
struct TrainResult {
    parts: Vec<crate::shapes::LoadedShapePart>,
    textures: HashMap<String, (Image, u8, bool)>,
    center: Vec3,
    size: Vec3,
}
#[derive(Component)]
pub(super) struct PreviewRoot;

#[derive(Resource)]
pub(super) struct Previews {
    route_worker: Worker<RouteKey, RouteResult>,
    train_worker: Worker<TrainKey, TrainResult>,
    route_key: Option<RouteKey>,
    train_key: Option<TrainKey>,
    pub route_image: Option<Handle<Image>>,
    pub train_image: Option<Handle<Image>>,
    pub railway: Option<RailwayInfo>,
    pub route_status: String,
    pub train_status: String,
    pub route_missing: HashMap<PathBuf, bool>,
}
impl Default for Previews {
    fn default() -> Self {
        Self {
            route_worker: worker(load_route),
            train_worker: worker(load_train),
            route_key: None,
            train_key: None,
            route_image: None,
            train_image: None,
            railway: None,
            route_status: String::new(),
            train_status: String::new(),
            route_missing: HashMap::new(),
        }
    }
}
fn bounded(path: &Path) -> Result<(), String> {
    let bytes = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if bytes > MAX_FILE_BYTES {
        return Err("Recurso demasiado grande para la vista previa".into());
    }
    Ok(())
}
fn load_texture(path: &Path) -> Result<(Image, u8, bool), String> {
    bounded(path)?;
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    if extension.eq_ignore_ascii_case("ace") {
        let ace = openrailsrs_ace::read_ace(path).map_err(|e| e.to_string())?;
        if u64::from(ace.width) * u64::from(ace.height) > 4_194_304 {
            return Err("Textura demasiado grande para la vista previa".into());
        }
        Ok((
            crate::shapes::ace_to_image(&ace),
            ace.alpha_bits,
            ace.has_mask_channel,
        ))
    } else if extension.eq_ignore_ascii_case("dds") {
        crate::shapes::decode_dds_to_rgba_image(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map(|image| (image, 8, false))
    } else {
        let data = image::ImageReader::open(path)
            .map_err(|e| e.to_string())?
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .decode()
            .map_err(|e| e.to_string())?;
        Ok((
            Image::from_dynamic(
                data.thumbnail(1024, 512),
                true,
                bevy::asset::RenderAssetUsages::default(),
            ),
            8,
            false,
        ))
    }
}
// Menu artwork is a route-relative reference, not an arbitrary local file reader.
fn route_artwork(root: &Path, name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    if Path::new(&name)
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let path = resolve_path_case_insensitive(&root.join(name))?
        .canonicalize()
        .ok()?;
    path.starts_with(root.canonicalize().ok()?).then_some(path)
}
fn confined_resource(path: &Path, scope: &Path) -> Option<PathBuf> {
    let path = resolve_path_case_insensitive(path)?.canonicalize().ok()?;
    path.starts_with(scope.canonicalize().ok()?).then_some(path)
}
fn load_route(key: &RouteKey) -> Result<RouteResult, String> {
    let native = RouteFile::from_route_dir(&key.root).map_err(|e| e.to_string())?;
    let mut railway = RailwayInfo {
        gauge_m: f64::from(native.cant.gauge_m),
        supply: ElectricSupply {
            kind: if native.overhead_wire.electrified {
                ElectricPickup::Overhead
            } else {
                ElectricPickup::None
            },
            voltage_v: if native.overhead_wire.electrified {
                native.max_line_voltage_v
            } else {
                0.0
            },
        },
        section_supplies: vec![],
        missing: vec![],
    };
    if let Some(scenario) = &key.scenario
        && let Ok(scenario) = openrailsrs_scenarios::load_scenario(scenario)
        && let Some(supply) = scenario.route.electric_supply
    {
        railway.supply = supply.supply;
        railway.section_supplies = supply.sections.into_iter().map(|s| s.supply).collect();
    }
    for folder in ["WORLD", "TILES", "PATHS"] {
        if !resolve_path_case_insensitive(&key.root.join(folder)).is_some_and(|p| p.is_dir()) {
            railway.missing.push(format!("Falta la carpeta {folder}"));
        }
    }
    let image = native
        .loading_screen
        .as_deref()
        .and_then(|name| route_artwork(&key.root, name))
        .and_then(|path| load_texture(&path).ok().map(|texture| texture.0));
    Ok(RouteResult { railway, image })
}
fn load_train(key: &TrainKey) -> Result<TrainResult, String> {
    bounded(&key.path)?;
    let vehicles = crate::rolling_stock::try_load_consist_vehicles(
        key.path.parent().unwrap_or(&key.route),
        key.path.to_string_lossy().as_ref(),
    )
    .ok_or("No se puede cargar la formación: revisá los recursos faltantes")?;
    let vehicle = vehicles.first().ok_or("La formación no tiene vehículos")?;
    let dirs = vehicle
        .asset_dir
        .iter()
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    let shape_path =
        crate::rolling_stock::resolve_consist_vehicle_shape_path(&dirs, vehicle, &key.route)
            .ok_or("Falta el modelo original del primer vehículo")?;
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
            shape_path.ancestors().find(|p| {
                p.file_name().is_some_and(|name| {
                    name.eq_ignore_ascii_case("TRAINS") || name.eq_ignore_ascii_case("ROUTES")
                })
            })
        })
        .and_then(Path::parent)
        .unwrap_or_else(|| shape_path.parent().unwrap_or(&key.route));
    let shape_path = confined_resource(&shape_path, scope)
        .ok_or("El modelo de la vista previa sale de su paquete de contenido")?;
    bounded(&shape_path)?;
    let shape = ShapeFile::from_path(&shape_path).map_err(|e| e.to_string())?;
    let parts = crate::shapes::build_mesh_parts_from_shape_at_distance(&shape, 0.0);
    if parts.is_empty() || parts.len() > 1024 {
        return Err("El modelo no tiene geometría válida para la vista previa".into());
    }
    let (min, max) =
        crate::shapes::union_meshes_aabb(&parts.iter().map(|p| &p.mesh).collect::<Vec<_>>())
            .ok_or("Modelo sin dimensiones válidas")?;
    let mut textures = HashMap::new();
    let mut texture_bytes = 0;
    let root = shape_path.parent().unwrap_or(&key.route);
    for part in &parts {
        if let Some(name) = &part.texture_file
            && textures.len() < 32
            && !textures.contains_key(name)
        {
            let path = [
                root.join(name.replace('\\', "/")),
                root.join("TEXTURES").join(name.replace('\\', "/")),
            ]
            .iter()
            .find_map(|path| confined_resource(path, scope));
            if let Some(texture) = path.and_then(|path| load_texture(&path).ok()) {
                let size = texture.0.data.as_ref().map_or(0, Vec::len);
                if texture_bytes + size <= 64 * 1024 * 1024 {
                    texture_bytes += size;
                    textures.insert(name.clone(), texture);
                }
            }
        }
    }
    Ok(TrainResult {
        parts,
        textures,
        center: (min + max) * 0.5,
        size: max - min,
    })
}
pub(super) fn railway_warning(
    audit: &openrailsrs_train::ConsistAudit,
    railway: &RailwayInfo,
) -> Option<String> {
    if let Some(gauge) = audit
        .compatibility
        .iter()
        .filter_map(|v| v.declared.curve.track_gauge_m)
        .find(|gauge| (*gauge - railway.gauge_m).abs() > 0.025)
    {
        return Some(format!(
            "Trocha incompatible: tren {:.0} mm · ruta {:.0} mm",
            gauge * 1000.0,
            railway.gauge_m * 1000.0
        ));
    }
    let engines = audit
        .compatibility
        .iter()
        .filter(|v| v.declared.engine_type.is_some())
        .collect::<Vec<_>>();
    if !engines.is_empty() && engines.iter().all(|v| v.declared.electric.is_some()) {
        let matching = engines.iter().any(|v| {
            v.declared.electric.as_ref().is_some_and(|engine| {
                std::iter::once(&railway.supply)
                    .chain(&railway.section_supplies)
                    .any(|supply| {
                        supply.kind == engine.pickup
                            && supply.voltage_v >= engine.minimum_voltage_v
                            && engine
                                .maximum_voltage_v
                                .is_none_or(|max| supply.voltage_v <= max)
                    })
            })
        });
        if !matching {
            return Some(format!(
                "Tren eléctrico sin alimentación compatible · ruta: {}",
                railway.supply.kind.label()
            ));
        }
        if railway.supply.kind == ElectricPickup::None {
            return Some("Alimentación eléctrica sólo en algunos tramos del recorrido".into());
        }
    }
    None
}
pub(super) fn update(
    mut commands: Commands,
    mut previews: ResMut<Previews>,
    mut ui: ResMut<PlayerUiState>,
    menu: Res<PlayerLaunchMenu>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<Entity, With<PreviewRoot>>,
) {
    if ui.panel != PlayerPanel::NewGame {
        for root in &roots {
            commands.entity(root).despawn();
        }
        previews.train_key = None;
        previews.train_image = None;
        return;
    }
    if let Some(service) = menu.current()
        && let Some(root) = &service.scenery_root
    {
        let key = RouteKey {
            root: root.clone(),
            scenario: (!service.native_activity).then(|| service.source.clone()),
        };
        if previews.route_key.as_ref() != Some(&key) {
            previews.route_image = None;
            previews.railway = None;
            previews.route_status = "Cargando imagen de la ruta…".into();
            let _ = previews.route_worker.tx.send(key.clone());
            previews.route_key = Some(key);
            ui.rebuild = true;
        }
    } else if previews.route_key.take().is_some() {
        previews.route_image = None;
        previews.railway = None;
        previews.route_status = "El escenario no tiene una imagen original instalada".into();
        ui.rebuild = true;
    }
    let route_results = previews
        .route_worker
        .rx
        .lock()
        .unwrap()
        .try_iter()
        .collect::<Vec<_>>();
    for (key, result) in route_results {
        if previews.route_key.as_ref() != Some(&key) {
            continue;
        }
        match result {
            Ok(result) => {
                previews
                    .route_missing
                    .insert(key.root, !result.railway.missing.is_empty());
                previews.route_status = if result.image.is_some() {
                    "Imagen incluida por el autor"
                } else {
                    "El autor no incluye una imagen de la ruta"
                }
                .into();
                previews.route_image = result.image.map(|image| images.add(image));
                previews.railway = Some(result.railway);
            }
            Err(error) => {
                previews.route_missing.insert(key.root, true);
                previews.route_status = format!("No se pudo leer la ruta: {error}");
            }
        }
        ui.rebuild = true;
    }
    if ui.new_game_step != NewGameStep::Train {
        for root in &roots {
            commands.entity(root).despawn();
        }
        previews.train_key = None;
        previews.train_image = None;
        return;
    }
    if let Some(path) = menu.consists.get(menu.consist) {
        let route = menu
            .current()
            .and_then(|s| s.scenery_root.clone())
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());
        let key = TrainKey {
            path: path.clone(),
            route,
        };
        if previews.train_key.as_ref() != Some(&key) {
            for root in &roots {
                commands.entity(root).despawn();
            }
            previews.train_image = None;
            previews.train_status = "Cargando modelo original…".into();
            let _ = previews.train_worker.tx.send(key.clone());
            previews.train_key = Some(key);
            ui.rebuild = true;
        }
    }
    let train_results = previews
        .train_worker
        .rx
        .lock()
        .unwrap()
        .try_iter()
        .collect::<Vec<_>>();
    for (key, result) in train_results {
        if previews.train_key.as_ref() != Some(&key) {
            continue;
        }
        match result {
            Err(error) => {
                previews.train_status = error;
                previews.train_image = None;
            }
            Ok(result) => {
                let image = images.add(Image::new_target_texture(
                    768,
                    320,
                    TextureFormat::Rgba8Unorm,
                    Some(TextureFormat::Rgba8UnormSrgb),
                ));
                let textures = result
                    .textures
                    .into_iter()
                    .map(|(name, (image, alpha, mask))| (name, (images.add(image), alpha, mask)))
                    .collect::<HashMap<_, _>>();
                let root = commands
                    .spawn((Transform::default(), Visibility::default(), PreviewRoot))
                    .id();
                commands.entity(root).with_children(|p| {
                    for part in result.parts {
                        let texture = part
                            .texture_file
                            .as_ref()
                            .and_then(|name| textures.get(name))
                            .cloned();
                        let passes =
                            openrailsrs_bevy_scenery::shapes::blend_alpha_passes_from_ace_bits(
                                texture.as_ref().map_or(0, |texture| texture.1),
                                texture.as_ref().is_some_and(|texture| texture.2),
                                part.texture_file.as_deref().unwrap_or(""),
                                part.shader_name.as_deref(),
                                part.alpha_test_mode,
                            );
                        let mesh = meshes.add(part.mesh);
                        for pass in passes {
                            let material = materials.add(StandardMaterial {
                                base_color_texture: texture
                                    .as_ref()
                                    .map(|texture| texture.0.clone()),
                                base_color: Color::WHITE,
                                alpha_mode: pass.alpha_mode,
                                perceptual_roughness: 0.85,
                                cull_mode: None,
                                double_sided: true,
                                ..default()
                            });
                            p.spawn((
                                Mesh3d(mesh.clone()),
                                MeshMaterial3d(material),
                                Transform::default(),
                                RenderLayers::layer(PREVIEW_LAYER),
                            ));
                        }
                    }
                    let radius = result.size.length() * 0.65;
                    p.spawn((
                        Camera3d::default(),
                        Camera {
                            order: -10,
                            clear_color: Color::srgb(0.07, 0.10, 0.14).into(),
                            ..default()
                        },
                        RenderTarget::Image(image.clone().into()),
                        Transform::from_translation(
                            result.center
                                + Vec3::new(-1.0, 0.40, -0.65).normalize() * radius.max(5.0),
                        )
                        .looking_at(result.center, Vec3::Y),
                        Projection::Perspective(PerspectiveProjection {
                            fov: 45.0_f32.to_radians(),
                            ..default()
                        }),
                        RenderLayers::layer(PREVIEW_LAYER),
                        Msaa::Off,
                    ));
                    p.spawn((
                        PointLight {
                            intensity: 25_000_000.0,
                            range: radius.max(10.0) * 5.0,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        Transform::from_translation(
                            result.center + Vec3::new(-radius, radius, -radius),
                        ),
                        RenderLayers::layer(PREVIEW_LAYER),
                    ));
                });
                previews.train_image = Some(image);
                previews.train_status =
                    "Vista 3D del primer vehículo · modelo y texturas originales".into();
            }
        }
        ui.rebuild = true;
    }
}

pub(super) fn image_card(
    p: &mut ChildSpawnerCommands<'_>,
    image: Option<&Handle<Image>>,
    status: &str,
) {
    if let Some(image) = image {
        p.spawn((
            ImageNode::new(image.clone()),
            Node {
                width: Val::Percent(100.0),
                max_width: Val::Px(768.0),
                height: Val::Px(200.0),
                flex_shrink: 0.0,
                ..default()
            },
        ));
    }
    if !status.is_empty() {
        label(p, status, 12.0, MUTED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn audit(kind: &str, gauge: Option<f64>) -> openrailsrs_train::ConsistAudit {
        let ast = openrailsrs_formats::parse_vehicle_text(&format!("Wagon ( Test Type ( Engine ) ) Engine ( Test Type ( {kind} ) ORTSRSPickup ( overhead ) )")).unwrap();
        let mut declared = openrailsrs_formats::parse_vehicle_content_metadata(&ast, true);
        declared.curve.track_gauge_m = gauge;
        openrailsrs_train::ConsistAudit {
            vehicles: 1,
            powered_vehicles: 1,
            compatibility: vec![openrailsrs_train::consist_audit::VehicleCompatibility {
                vehicle: 1,
                stock_path: PathBuf::from("Test.eng"),
                declared,
                limitations: vec![],
            }],
            ..default()
        }
    }
    #[test]
    fn railway_badges_distinguish_missing_power_wrong_gauge_and_partial_coverage() {
        let mut route = RailwayInfo {
            gauge_m: 1.435,
            supply: ElectricSupply::default(),
            section_supplies: vec![],
            missing: vec![],
        };
        assert!(
            railway_warning(&audit("Electric", Some(1.435)), &route)
                .unwrap()
                .contains("sin alimentación")
        );
        assert!(railway_warning(&audit("Diesel", Some(1.435)), &route).is_none());
        assert!(
            railway_warning(&audit("Steam", Some(0.6)), &route)
                .unwrap()
                .contains("Trocha incompatible")
        );
        assert!(railway_warning(&audit("Steam", None), &route).is_none());
        route.section_supplies.push(ElectricSupply {
            kind: ElectricPickup::Overhead,
            voltage_v: 25000.0,
        });
        assert!(
            railway_warning(&audit("Electric", Some(1.435)), &route)
                .unwrap()
                .contains("algunos tramos")
        );
        route.supply = route.section_supplies[0].clone();
        assert!(railway_warning(&audit("Electric", Some(1.435)), &route).is_none());
        route.supply.kind = ElectricPickup::ThirdRail;
        route.section_supplies.clear();
        assert!(railway_warning(&audit("Electric", Some(1.435)), &route).is_some());
    }
    #[test]
    fn route_artwork_stays_inside_its_own_package() {
        let temp = tempfile::tempdir().unwrap();
        let route = temp.path().join("Route");
        std::fs::create_dir(&route).unwrap();
        std::fs::write(route.join("Load.ace"), b"example").unwrap();
        assert!(route_artwork(&route, "load.ace").is_some());
        assert!(route_artwork(&route, "../outside.ace").is_none());
        assert!(route_artwork(&route, "/etc/passwd").is_none());
        let models = route.join("Models");
        std::fs::create_dir(&models).unwrap();
        assert!(confined_resource(&models.join("../Load.ace"), &route).is_some());
        let outside = temp.path().join("outside.ace");
        std::fs::write(&outside, b"example").unwrap();
        assert!(confined_resource(&outside, &route).is_none());
        #[cfg(unix)]
        {
            std::fs::write(temp.path().join("outside.ace"), b"example").unwrap();
            std::os::unix::fs::symlink(temp.path().join("outside.ace"), route.join("link.ace"))
                .unwrap();
            assert!(route_artwork(&route, "link.ace").is_none());
        }
    }
}
