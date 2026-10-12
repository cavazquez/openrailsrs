//! Bounded, asynchronous menu previews from the installed author's assets.
use super::*;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use bevy::render::render_resource::TextureFormat;
use openrailsrs_core::electrification::{ElectricPickup, ElectricSupply};
use openrailsrs_formats::{RouteFile, ShapeFile, resolve_path_case_insensitive};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, mpsc};

#[path = "preview_route.rs"]
mod route;
#[path = "preview_train.rs"]
mod train;

const PREVIEW_LAYER: usize = 31;
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

struct Worker<K, T> {
    tx: mpsc::Sender<(u64, K)>,
    rx: Mutex<mpsc::Receiver<(K, Result<T, String>)>>,
    generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
}
impl<K, T> Worker<K, T> {
    fn cancel(&self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn request(&self, key: K) {
        use std::sync::atomic::Ordering;
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.tx.send((generation, key));
    }
}
impl<K, T> Drop for Worker<K, T> {
    fn drop(&mut self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}
fn worker<K: Send + 'static, T: Send + 'static>(load: fn(&K) -> Result<T, String>) -> Worker<K, T> {
    let (tx, requests) = mpsc::channel::<(u64, K)>();
    let (results, rx) = mpsc::channel();
    let generation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let latest = generation.clone();
    std::thread::spawn(move || {
        while let Ok(mut request) = requests.recv() {
            for newer in requests.try_iter() {
                request = newer;
            }
            let (epoch, key) = request;
            let _scope = openrailsrs_formats::cancellation::Scope::new(latest.clone(), epoch);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load(&key)))
                .unwrap_or_else(|_| {
                    Err("No se pudo interpretar el recurso de la vista previa".into())
                });
            if latest.load(std::sync::atomic::Ordering::Relaxed) != epoch {
                continue;
            }
            if results.send((key, result)).is_err() {
                break;
            }
        }
    });
    Worker {
        tx,
        rx: Mutex::new(rx),
        generation,
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct RouteKey {
    root: PathBuf,
    scenario: Option<PathBuf>,
    imported: PathBuf,
    source: PathBuf,
    native_activity: bool,
    native_timetable: Option<String>,
    path: Option<PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct TrainKey {
    path: PathBuf,
    route: PathBuf,
}
#[derive(Clone, Debug)]
pub(super) struct RailwayInfo {
    pub clearance: Option<crate::clearance::InfrastructureEnvelope>,
    pub gauge_m: f64,
    pub supply: ElectricSupply,
    pub section_supplies: Vec<ElectricSupply>,
    pub missing: Vec<String>,
}
struct RouteResult {
    inventory: crate::resource_estimate::Inventory,
    railway: RailwayInfo,
    image: Option<Image>,
    map: Result<(Image, String), String>,
}
struct TrainResult {
    bytes: crate::resource_estimate::TrainBytes,
    models: Vec<Vec<crate::shapes::LoadedShapePart>>,
    textures: HashMap<String, (Image, u8, bool)>,
    vehicles: Vec<PreviewVehicle>,
    center: Vec3,
    size: Vec3,
    missing: usize,
    truncated: bool,
}
struct PreviewVehicle {
    model: Option<usize>,
    transform: Transform,
    name: String,
    center: Vec3,
    size: Vec3,
}
struct PreviewScene {
    center: Vec3,
    size: Vec3,
    vehicles: Vec<PreviewVehicle>,
    missing: usize,
    truncated: bool,
}
#[derive(Component)]
pub(super) struct PreviewCamera;
#[derive(Default)]
pub(super) struct PreviewPose {
    pub yaw: f32,
    pub zoom: f32,
    pub focus: Option<usize>,
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
    pub route_map: Option<Handle<Image>>,
    pub route_map_summary: String,
    pub show_route_map: bool,
    pub train_image: Option<Handle<Image>>,
    pub railway: Option<RailwayInfo>,
    pub route_status: String,
    pub inventory: Option<crate::resource_estimate::Inventory>,
    pub train_bytes: crate::resource_estimate::TrainBytes,
    pub train_estimate_path: Option<PathBuf>,
    pub train_status: String,
    pub route_missing: HashMap<PathBuf, bool>,
    scene: Option<PreviewScene>,
    pub pose: PreviewPose,
}
impl Default for Previews {
    fn default() -> Self {
        Self {
            route_worker: worker(load_route),
            train_worker: worker(train::load),
            route_key: None,
            train_key: None,
            route_image: None,
            route_map: None,
            route_map_summary: String::new(),
            show_route_map: false,
            train_image: None,
            railway: None,
            route_status: String::new(),
            inventory: None,
            train_bytes: default(),
            train_estimate_path: None,
            train_status: String::new(),
            route_missing: HashMap::new(),
            scene: None,
            pose: default(),
        }
    }
}
fn bounded(path: &Path) -> Result<(), String> {
    openrailsrs_formats::cancellation::check().map_err(|e| e.to_string())?;
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
    openrailsrs_formats::cancellation::check().map_err(|e| e.to_string())?;
    let native = RouteFile::from_route_dir(&key.root).ok();
    let mut railway = RailwayInfo {
        clearance: crate::clearance::load(&key.root),
        gauge_m: native.as_ref().map_or(0., |n| f64::from(n.cant.gauge_m)),
        supply: ElectricSupply {
            kind: if native.as_ref().is_some_and(|n| n.overhead_wire.electrified) {
                ElectricPickup::Overhead
            } else {
                ElectricPickup::None
            },
            voltage_v: if native.as_ref().is_some_and(|n| n.overhead_wire.electrified) {
                native.as_ref().map_or(0., |n| n.max_line_voltage_v)
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
    for folder in ["WORLD", "TILES", "PATHS"]
        .into_iter()
        .filter(|_| native.is_some())
    {
        if !resolve_path_case_insensitive(&key.root.join(folder)).is_some_and(|p| p.is_dir()) {
            railway.missing.push(format!("Falta la carpeta {folder}"));
        }
    }
    let image = native
        .as_ref()
        .and_then(|n| n.loading_screen.as_deref())
        .and_then(|name| route_artwork(&key.root, name))
        .and_then(|path| load_texture(&path).ok().map(|texture| texture.0));
    let map = route::load(key);
    Ok(RouteResult {
        inventory: crate::resource_estimate::Inventory::scan(&key.root)?,
        railway,
        image,
        map,
    })
}
pub(super) fn railway_warning(
    audit: &openrailsrs_train::ConsistAudit,
    railway: &RailwayInfo,
) -> Option<String> {
    if let crate::clearance::Assessment::Incompatible(message) =
        crate::clearance::compare(audit, railway.clearance.as_ref())
    {
        return Some(message);
    }
    if let Some(gauge) = audit
        .compatibility
        .iter()
        .filter_map(|v| v.declared.curve.track_gauge_m)
        .find(|gauge| railway.gauge_m > 0. && (*gauge - railway.gauge_m).abs() > 0.025)
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
    mut menu: ResMut<PlayerLaunchMenu>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<Entity, With<PreviewRoot>>,
    mut cameras: Query<&mut Transform, With<PreviewCamera>>,
) {
    if !matches!(
        ui.panel,
        PlayerPanel::NewGame | PlayerPanel::FormationEditor
    ) {
        if previews.train_key.take().is_some() {
            previews.train_worker.cancel();
        }
        if previews.route_key.take().is_some() {
            previews.route_worker.cancel();
        }
        for root in &roots {
            commands.entity(root).despawn();
        }
        previews.scene = None;
        previews.train_key = None;
        previews.train_image = None;
        return;
    }
    if crate::capture::capture_enabled()
        && std::env::var("OPENRAILSRS_SCREENSHOT_MENU").is_ok_and(|v| v == "1")
        && let Ok(name) = std::env::var("OPENRAILSRS_SCREENSHOT_MENU_CONSIST")
        && let Some(index) = menu
            .consists
            .iter()
            .position(|p| p.file_stem().is_some_and(|s| s.eq_ignore_ascii_case(&name)))
        && menu.consist != index
    {
        choices::select(&mut menu, ChoiceKind::Consist, index);
        ui.choice_search[ChoiceKind::Consist.slot()].query = name;
        ui.rebuild = true;
    }
    if let Some(service) = menu.current() {
        let root = service.scenery_root.as_ref().unwrap_or(&service.route_dir);
        let key = RouteKey {
            root: root.clone(),
            scenario: (!service.native_activity).then(|| service.source.clone()),
            imported: service.route_dir.clone(),
            source: service.source.clone(),
            native_activity: service.native_activity,
            native_timetable: service.native_timetable.clone(),
            path: menu
                .path
                .checked_sub(1)
                .and_then(|i| menu.paths.get(i))
                .cloned(),
        };
        if previews.route_key.as_ref() != Some(&key) {
            previews.inventory = None;
            previews.route_image = None;
            previews.route_map = None;
            previews.route_map_summary = "Preparando el mapa del servicio…".into();
            previews.railway = None;
            previews.route_status = "Cargando imagen de la ruta…".into();
            previews.route_worker.request(key.clone());
            previews.route_key = Some(key);
            ui.rebuild = true;
        }
    } else if previews.route_key.take().is_some() {
        previews.route_image = None;
        previews.route_map = None;
        previews.route_map_summary.clear();
        previews.railway = None;
        previews.route_status = "El escenario no tiene una imagen original instalada".into();
        ui.rebuild = true;
    }
    if crate::capture::capture_enabled()
        && ui.in_start_menu
        && std::env::var("OPENRAILSRS_SCREENSHOT_MENU_PAGE").is_ok_and(|page| page == "route-map")
    {
        previews.show_route_map = true;
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
                match result.map {
                    Ok((map, summary)) => {
                        previews.route_map = Some(images.add(map));
                        previews.route_map_summary = summary;
                    }
                    Err(error) => {
                        previews.route_map_summary = format!("Mapa no disponible: {error}");
                    }
                }
                previews.inventory = Some(result.inventory);
                previews.railway = Some(result.railway);
            }
            Err(error) => {
                previews.route_missing.insert(key.root, true);
                previews.route_status = format!("No se pudo leer la ruta: {error}");
            }
        }
        ui.rebuild = true;
    }
    if ui.new_game_step != NewGameStep::Train && ui.panel != PlayerPanel::FormationEditor {
        if previews.train_key.take().is_some() {
            previews.train_worker.cancel();
        }
        for root in &roots {
            commands.entity(root).despawn();
        }
        previews.scene = None;
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
            previews.scene = None;
            previews.train_status = "Cargando la formación original…".into();
            previews.train_worker.request(key.clone());
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
                previews.train_estimate_path = Some(key.path.clone());
                previews.train_bytes = result.bytes;
                let scene = train::spawn(
                    &mut commands,
                    result,
                    image.clone(),
                    &mut images,
                    &mut meshes,
                    &mut materials,
                );
                previews.train_status = format!(
                    "{} vehículos · modelos y texturas originales{}{}",
                    scene.vehicles.len(),
                    if scene.missing > 0 {
                        format!(" · {} modelos faltantes en gris", scene.missing)
                    } else {
                        String::new()
                    },
                    if scene.truncated {
                        " · limitada a 256 vehículos"
                    } else {
                        ""
                    }
                );
                previews.train_image = Some(image);
                previews.scene = Some(scene);
                previews.pose = default();
            }
        }
        ui.rebuild = true;
    }
    if let Some(scene) = &previews.scene {
        let (center, size) = previews
            .pose
            .focus
            .and_then(|i| scene.vehicles.get(i))
            .map(|v| (v.center, v.size))
            .unwrap_or((scene.center, scene.size));
        let radius = (size.length() * 0.8).max(5.0) * 2.0_f32.powf(previews.pose.zoom);
        let yaw = -1.0 + previews.pose.yaw;
        for mut camera in &mut cameras {
            *camera = Transform::from_translation(
                center + Vec3::new(yaw.sin(), 0.35, yaw.cos()).normalize() * radius,
            )
            .looking_at(center, Vec3::Y);
        }
    }
}

impl Previews {
    pub fn move_focus(&mut self, delta: i32) {
        if let Some(scene) = &self.scene {
            self.pose.focus = Some(cycle(
                self.pose.focus.unwrap_or(0),
                scene.vehicles.len(),
                delta,
            ));
            self.pose.zoom = 0.0;
        }
    }
    pub fn focus_label(&self) -> String {
        self.pose
            .focus
            .and_then(|i| {
                self.scene
                    .as_ref()
                    .and_then(|s| s.vehicles.get(i).map(|v| (i, s, v)))
            })
            .map(|(i, s, v)| {
                format!(
                    "Vehículo {}/{} · {}{}",
                    i + 1,
                    s.vehicles.len(),
                    v.name,
                    if v.model.is_none() {
                        " · falta el modelo"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_else(|| "Formación completa".into())
    }
}
pub(super) fn train_card(p: &mut ChildSpawnerCommands<'_>, preview: &Previews) {
    image_card(p, preview.train_image.as_ref(), &preview.train_status);
    if preview.train_image.is_some() {
        label(p, preview.focus_label(), 12.0, MUTED);
        row(p, |p| {
            button(p, "↶", UiCommand::PreviewRotate(-0.3));
            button(p, "↷", UiCommand::PreviewRotate(0.3));
            button(p, "+", UiCommand::PreviewZoom(-0.25));
            button(p, "−", UiCommand::PreviewZoom(0.25));
            button(p, "Todo", UiCommand::PreviewAll);
        });
        row(p, |p| {
            button(p, "◀ Vehículo", UiCommand::PreviewCar(-1));
            button(p, "Vehículo ▶", UiCommand::PreviewCar(1));
        });
    }
}

pub(super) fn route_card(p: &mut ChildSpawnerCommands<'_>, preview: &Previews) {
    row(p, |p| {
        button(p, "Imagen / Mapa", UiCommand::PreviewRouteView);
    });
    if preview.show_route_map {
        image_card(p, preview.route_map.as_ref(), &preview.route_map_summary);
    } else {
        image_card(p, preview.route_image.as_ref(), &preview.route_status);
        if preview.route_map.is_some() {
            label(
                p,
                "Mapa del servicio disponible · Imagen / Mapa",
                12.0,
                MUTED,
            );
        }
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
                source_sha256: String::new(),
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
            clearance: None,
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
