//! Installed formation validation. A wagon-only formation is valid static stock,
//! but cannot be selected as the player's powered train. This reports content
//! integrity, not physics parity or support for arbitrary C# scripts.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use openrailsrs_formats::{
    CabControl, CabViewFile, ConsistEntry, ConsistFile, EngineCabView, FormatError, MstsFile,
    ScriptSystem, ShapeFile, VehicleContentMetadata, parse_cab_view_text, parse_msts_file,
    parse_named_stf, parse_vehicle_content_metadata, parse_vehicle_text, read_msts_file_to_string,
    resolve_path_case_insensitive, sms_wave_references,
};
use serde::Serialize;

use crate::{consist_asset_root, resolve_consist_entry_path};

#[derive(Clone, Debug, Default, Serialize)]
pub struct ConsistAudit {
    pub path: PathBuf,
    pub vehicles: usize,
    pub powered_vehicles: usize,
    pub length_m: f64,
    pub mass_kg: f64,
    pub cab_2d: bool,
    pub cab_3d: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub missing_resources: Vec<MissingResource>,
    pub compatibility: Vec<VehicleCompatibility>,
}

/// Actual references and search locations from the loader, without guessing a
/// replacement package or parsing filenames out of human-readable errors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MissingResource {
    pub name: String,
    pub referenced_by: PathBuf,
    pub destinations: Vec<PathBuf>,
    pub required: bool,
}
impl MissingResource {
    fn new(reference: &Path, dirs: &[PathBuf], name: &str, required: bool) -> Self {
        Self::with_destinations(
            reference,
            name,
            dirs.iter().map(|p| p.join(name.trim().replace('\\', "/"))),
            required,
        )
    }
    fn with_destinations(
        reference: &Path,
        name: &str,
        paths: impl IntoIterator<Item = PathBuf>,
        required: bool,
    ) -> Self {
        let mut destinations = vec![];
        for path in paths {
            let path = display_path(&path);
            if !destinations.contains(&path) {
                destinations.push(path);
            }
        }
        Self {
            name: name.into(),
            referenced_by: display_path(reference),
            destinations,
            required,
        }
    }
    pub fn guidance(&self) -> String {
        format!(
            "{}: {}\nReferenciado por: {}\nColocar en {}:\n{}",
            if self.required {
                "Falta"
            } else {
                "Falta opcional"
            },
            self.name,
            self.referenced_by.display(),
            if self.destinations.len() == 1 {
                "esta ubicación"
            } else {
                "una de estas ubicaciones"
            },
            self.destinations
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

// Missing leaf files cannot be canonicalized. Resolve the existing prefix,
// preserving the casing of the installation and removing lexical ../ segments.
fn display_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => (),
            _ => normalized.push(component.as_os_str()),
        }
    }
    for ancestor in normalized.ancestors() {
        if let Some(existing) = resolve_path_case_insensitive(ancestor)
            && let Ok(canonical) = existing.canonicalize()
        {
            let suffix = normalized.strip_prefix(ancestor).unwrap();
            return if suffix.as_os_str().is_empty() {
                canonical
            } else {
                canonical.join(suffix)
            };
        }
    }
    normalized
}

#[derive(Clone, Debug, Serialize)]
pub struct VehicleCompatibility {
    pub vehicle: usize,
    pub stock_path: PathBuf,
    pub declared: VehicleContentMetadata,
    pub limitations: Vec<String>,
}

impl ConsistAudit {
    pub fn content_valid(&self) -> bool {
        self.vehicles > 0 && self.errors.is_empty()
    }
    pub fn player_ready(&self) -> bool {
        self.content_valid() && self.powered_vehicles > 0
    }
    pub fn label(&self) -> String {
        if let Some(error) = self.errors.first() {
            format!("Incompleta: {error}")
        } else if self.powered_vehicles == 0 {
            "Sin tracción · material estático".into()
        } else {
            format!(
                "{} vehículos · {:.0} m · cabina {}",
                self.vehicles,
                self.length_m,
                if self.cab_3d {
                    "3D"
                } else if self.cab_2d {
                    "2D"
                } else {
                    "genérica"
                }
            )
        }
    }
    pub fn compatibility_summary(&self) -> String {
        if self.warnings.is_empty() {
            return if self.errors.is_empty() {
                "Recursos completos · sistemas pendientes de validar contra OR"
            } else {
                "Contenido incompleto o inválido · ver diagnóstico"
            }
            .into();
        }
        let warning = self
            .warnings
            .iter()
            .find(|w| w.contains("C#"))
            .unwrap_or(&self.warnings[0]);
        format!(
            "{} avisos · {}",
            self.warnings.len(),
            warning.chars().take(170).collect::<String>()
        )
    }
}

#[derive(Clone, Debug)]
struct StockAudit {
    mass: f64,
    length: f64,
    powered: bool,
    cab_2d: bool,
    cab_3d: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
    missing_resources: Vec<MissingResource>,
    metadata: VehicleContentMetadata,
    limitations: Vec<String>,
}

#[derive(Clone, Debug)]
struct ResourceFailure {
    message: String,
    missing_resources: Vec<MissingResource>,
}
impl ResourceFailure {
    fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            missing_resources: vec![],
        }
    }
    fn missing(message: String, reference: &Path, dirs: &[PathBuf], name: &str) -> Self {
        Self {
            message,
            missing_resources: vec![MissingResource::new(reference, dirs, name, true)],
        }
    }
}
impl From<FormatError> for ResourceFailure {
    fn from(error: FormatError) -> Self {
        let missing_resources = match &error {
            FormatError::MissingInclude {
                containing_file,
                reference,
            } => vec![MissingResource::new(
                containing_file,
                &[containing_file
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_path_buf()],
                reference,
                true,
            )],
            _ => vec![],
        };
        Self {
            message: error.to_string(),
            missing_resources,
        }
    }
}
impl StockAudit {
    fn failure(&mut self, error: ResourceFailure) {
        self.errors.push(error.message);
        self.missing_resources.extend(error.missing_resources);
    }
}

/// Cache shared vehicles, shapes and cab panels across hundreds of `.con` files.
/// Additional TRAINSET roots mirror the viewer's preference for original models
/// over the small project physics fixtures.
#[derive(Clone, Debug, Default)]
pub struct ConsistAuditor {
    pub trainset_roots: Vec<PathBuf>,
    stocks: HashMap<PathBuf, Result<StockAudit, ResourceFailure>>,
    shapes: HashMap<PathBuf, Result<(), ResourceFailure>>,
    cabs: HashMap<(PathBuf, bool), Result<(), ResourceFailure>>,
}

impl ConsistAuditor {
    pub fn new(trainset_roots: Vec<PathBuf>) -> Self {
        Self {
            trainset_roots,
            ..Self::default()
        }
    }
    pub fn inspect(&mut self, con: &Path) -> ConsistAudit {
        let mut report = ConsistAudit {
            path: con.into(),
            ..ConsistAudit::default()
        };
        let parsed = read_msts_file_to_string(con)
            .and_then(|t| parse_vehicle_text(&t))
            .and_then(|a| ConsistFile::from_ast(&a));
        let entries = match parsed {
            Ok(c) => c.entries,
            Err(e) => {
                report.errors.push(e.to_string());
                return report;
            }
        };
        report.vehicles = entries.len();
        if entries.is_empty() {
            report
                .errors
                .push("La formación no contiene vehículos".into());
        }
        for (index, entry) in entries.iter().enumerate() {
            let (rel, engine) = match entry {
                ConsistEntry::Engine { path, .. } => (path, true),
                ConsistEntry::Wagon { path, .. } => (path, false),
            };
            let path = resolve_consist_entry_path(consist_asset_root(con), rel);
            if !path.is_file() {
                report.errors.push(format!(
                    "Vehículo {}: Falta el archivo ENG/WAG {rel}",
                    index + 1
                ));
                let base = consist_asset_root(con);
                let mut destinations = crate::from_ast::consist_entry_candidates(base, rel);
                if base
                    .file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("TRAINS"))
                    && destinations.len() > 1
                {
                    destinations.swap(0, 1);
                }
                let missing = MissingResource::with_destinations(con, rel, destinations, true);
                if !report.missing_resources.contains(&missing) {
                    report.missing_resources.push(missing);
                }
                continue;
            }
            if !self.stocks.contains_key(&path) {
                let audit = self.inspect_stock(&path);
                self.stocks.insert(path.clone(), audit);
            }
            let prefix = format!(
                "Vehículo {} ({})",
                index + 1,
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            match &self.stocks[&path] {
                Err(e) => {
                    report.errors.push(format!("{prefix}: {}", e.message));
                    for missing in &e.missing_resources {
                        if !report.missing_resources.contains(missing) {
                            report.missing_resources.push(missing.clone());
                        }
                    }
                }
                Ok(stock) => {
                    report.compatibility.push(VehicleCompatibility {
                        vehicle: index + 1,
                        stock_path: path.clone(),
                        declared: stock.metadata.clone(),
                        limitations: stock.limitations.clone(),
                    });
                    report.length_m += stock.length;
                    report.mass_kg += stock.mass;
                    report.powered_vehicles += usize::from(engine && stock.powered);
                    if engine && !report.cab_2d && !report.cab_3d {
                        report.cab_2d = stock.cab_2d;
                        report.cab_3d = stock.cab_3d;
                    }
                    report
                        .errors
                        .extend(stock.errors.iter().map(|e| format!("{prefix}: {e}")));
                    report
                        .warnings
                        .extend(stock.warnings.iter().map(|e| format!("{prefix}: {e}")));
                    for missing in &stock.missing_resources {
                        if !report.missing_resources.contains(missing) {
                            report.missing_resources.push(missing.clone());
                        }
                    }
                }
            }
        }
        report
    }
    fn inspect_stock(&mut self, path: &Path) -> Result<StockAudit, ResourceFailure> {
        if !path.is_file() {
            return Err(ResourceFailure::message("Falta el archivo ENG/WAG"));
        }
        let (mass, length, powered, shape, cab) =
            match parse_msts_file(path).map_err(ResourceFailure::from)? {
                MstsFile::Engine(e) => (
                    e.mass_kg,
                    e.length_m,
                    e.max_power_w > 0.0 || e.steam.is_some(),
                    e.wagon_shape,
                    Some(e.cab),
                ),
                MstsFile::Wagon(w) => (w.mass_kg, w.length_m, false, w.wagon_shape, None),
                _ => return Err(ResourceFailure::message("El archivo no es un vehículo")),
            };
        let authored_root = path.parent().unwrap_or(Path::new("."));
        let ast = openrailsrs_formats::read_vehicle_ast(path).map_err(ResourceFailure::from)?;
        let metadata = parse_vehicle_content_metadata(&ast, cab.is_some());
        let root = self
            .trainset_roots
            .iter()
            .find_map(|r| {
                let p = r.join(authored_root.file_name()?);
                resolve_path_case_insensitive(&p).filter(|p| p.is_dir())
            })
            .unwrap_or_else(|| authored_root.into());
        let mut stock = StockAudit {
            mass,
            length,
            powered,
            cab_2d: false,
            cab_3d: false,
            errors: vec![],
            warnings: vec![],
            missing_resources: vec![],
            metadata,
            limitations: vec![],
        };
        self.inspect_subsystems(path, &root, &mut stock);
        if !mass.is_finite() || mass <= 0.0 || !length.is_finite() || length <= 0.0 {
            stock.errors.push("Masa o longitud inválida".into());
        }
        if let Some(shape) = shape {
            match asset(&[root.clone(), root.join("SHAPES")], &shape, false) {
                Some(path) => {
                    if let Err(e) = self.inspect_shape(&path) {
                        stock.failure(e);
                    }
                }
                None => stock.failure(ResourceFailure::missing(
                    format!("Falta el modelo {shape}"),
                    path,
                    &[root.clone(), root.join("SHAPES")],
                    &shape,
                )),
            }
        } else {
            stock
                .warnings
                .push("Sin modelo declarado: se usa geometría genérica".into());
        }
        if let Some(cab) = cab {
            self.inspect_cab(path, &root, &cab, &mut stock);
        }
        Ok(stock)
    }
    fn inspect_subsystems(&self, path: &Path, root: &Path, stock: &mut StockAudit) {
        if let Some(kind) = &stock.metadata.engine_type {
            match kind.to_ascii_lowercase().as_str() {
                "diesel" => {},
                "electric" => stock.limitations.push("Tracción eléctrica parcial: sin paridad de alimentación y protecciones originales".into()),
                "steam" => stock.limitations.push("Vapor parcial: no reproduce todos los subsistemas originales".into()),
                _ => stock.limitations.push(format!("Tipo de motor {kind}: usa el modelo de tracción genérico")),
            }
        }
        if let Some(system) = &stock.metadata.brake_system {
            stock.limitations.push(format!("Frenos {system}: modelo por vehículo; sin certificación de paridad del sistema original"));
        }
        let authored_root = path.parent().unwrap_or(root);
        let script_dirs = [authored_root.join("Script"), root.join("Script")];
        for script in &stock.metadata.scripts {
            if script.built_in {
                continue;
            }
            let file = if Path::new(&script.name).extension().is_none() {
                format!("{}.cs", script.name)
            } else {
                script.name.clone()
            };
            let available = asset(&script_dirs, &file, false).is_some();
            if !available {
                stock.missing_resources.push(MissingResource::new(
                    path,
                    &script_dirs,
                    &file,
                    false,
                ));
            }
            let support = match script.system {
                ScriptSystem::TrainControl => {
                    "TCS C#: requiere selección explícita del host .NET y API compatible"
                }
                ScriptSystem::TrainBrake | ScriptSystem::EngineBrake => {
                    "Controlador de freno C#: no se ejecuta; se usa el modelo Rust"
                }
                ScriptSystem::PowerSupply => {
                    "Alimentación C#: no se ejecuta; se usa el modelo Rust"
                }
            };
            stock.limitations.push(format!(
                "{support} ({}){}",
                script.name,
                if available {
                    ""
                } else {
                    " · falta el archivo"
                }
            ));
        }
        // Sound is optional: missing SMS/WAV is a warning, not a missing train.
        // Search installed global SOUND as well as the vehicle's local SOUND.
        let mut dirs = vec![
            root.to_path_buf(),
            root.join("SOUND"),
            authored_root.to_path_buf(),
            authored_root.join("SOUND"),
        ];
        for vehicle_root in [authored_root, root] {
            if let Some(content) = vehicle_root
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
            {
                dirs.push(content.join("SOUND"));
            }
        }
        for name in &stock.metadata.sounds {
            let Some(sms) = asset(&dirs, name, false) else {
                stock
                    .missing_resources
                    .push(MissingResource::new(path, &dirs, name, false));
                stock
                    .limitations
                    .push(format!("Falta el sonido opcional {name}"));
                continue;
            };
            match read_msts_file_to_string(&sms).and_then(|text| parse_named_stf(&text)) {
                Ok(ast) => {
                    let mut waves = vec![sms.parent().unwrap_or(root).to_path_buf()];
                    waves.extend(dirs.iter().cloned());
                    for file in sms_wave_references(&ast) {
                        if asset(&waves, &file, false).is_none() {
                            stock
                                .missing_resources
                                .push(MissingResource::new(&sms, &waves, &file, false));
                            stock
                                .limitations
                                .push(format!("Falta la muestra opcional {file} de {name}"));
                        }
                    }
                }
                Err(e) => stock
                    .limitations
                    .push(format!("Sonido opcional {name}: {e}")),
            }
        }
        stock.warnings.extend(stock.limitations.iter().cloned());
    }
    fn inspect_shape(&mut self, path: &Path) -> Result<(), ResourceFailure> {
        if let Some(result) = self.shapes.get(path) {
            return result.clone();
        }
        let result = ShapeFile::from_path(path)
            .map_err(|e| ResourceFailure::message(format!("Modelo {}: {e}", path.display())))
            .and_then(|shape| {
                let root = path.parent().unwrap_or(Path::new("."));
                let mut dirs = vec![root.to_path_buf(), root.join("TEXTURES")];
                if root
                    .file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("shapes"))
                    && let Some(parent) = root.parent()
                {
                    dirs.extend([parent.to_path_buf(), parent.join("TEXTURES")]);
                }
                let mut missing_resources = vec![];
                for name in &shape.texture_filenames {
                    if asset(&dirs, name, true).is_none() {
                        let missing = MissingResource::new(path, &dirs, name, true);
                        if !missing_resources.contains(&missing) {
                            missing_resources.push(missing);
                        }
                    }
                }
                if missing_resources.is_empty() {
                    Ok(())
                } else {
                    Err(ResourceFailure {
                        message: missing_resources
                            .iter()
                            .map(|r| {
                                format!(
                                    "Falta la textura {} de {}",
                                    r.name,
                                    path.file_name().unwrap_or_default().to_string_lossy()
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                        missing_resources,
                    })
                }
            });
        self.shapes.insert(path.into(), result.clone());
        result
    }
    fn inspect_cab(
        &mut self,
        reference: &Path,
        root: &Path,
        cab: &EngineCabView,
        stock: &mut StockAudit,
    ) {
        let previous_errors = stock.errors.len();
        let previous_missing = stock.missing_resources.len();
        let dirs = vec![
            root.join("CABVIEW"),
            root.join("CABVIEW3D"),
            root.to_path_buf(),
        ];
        if let Some(name) = &cab.cab_view_file {
            match asset(&dirs, name, false) {
                Some(p) => match self.inspect_cvf(&p, true) {
                    Ok(()) => stock.cab_2d = true,
                    Err(e) => stock.failure(e),
                },
                None => stock.failure(ResourceFailure::missing(
                    format!("Falta la cabina {name}"),
                    reference,
                    &dirs,
                    name,
                )),
            }
        }
        if let Some(name) = &cab.orts_3d_cab_shape {
            match asset(&dirs, name, false) {
                Some(p) => {
                    if let Err(e) = self.inspect_shape(&p) {
                        stock.failure(e);
                    } else if let Some(cvf) =
                        resolve_path_case_insensitive(&p.with_extension("cvf"))
                    {
                        match self.inspect_cvf(&cvf, false) {
                            Ok(()) => stock.cab_3d = true,
                            Err(e) => stock.failure(e),
                        }
                    } else {
                        let cvf = p.with_extension("cvf");
                        stock.failure(ResourceFailure {
                            message: format!("Faltan los instrumentos CVF de {name}"),
                            missing_resources: vec![MissingResource::with_destinations(
                                &p,
                                &cvf.file_name().unwrap_or_default().to_string_lossy(),
                                [cvf.clone()],
                                true,
                            )],
                        });
                    }
                }
                None => stock.failure(ResourceFailure::missing(
                    format!("Falta la cabina 3D {name}"),
                    reference,
                    &dirs,
                    name,
                )),
            }
        } else if let Some(c) = openrailsrs_formats::resolve_cab_assets(root, cab) {
            stock.cab_3d = self.inspect_shape(&c.shape_path).is_ok()
                && self.inspect_cvf(&c.cvf_path, false).is_ok();
        }
        if stock.cab_2d || stock.cab_3d {
            for missing in &mut stock.missing_resources[previous_missing..] {
                missing.required = false;
            }
            // OR can drive a locomotive with either cab mode. Missing optional
            // alternatives remain visible without blocking the working one.
            stock.warnings.extend(
                stock
                    .errors
                    .drain(previous_errors..)
                    .map(|e| format!("Cabina alternativa no disponible: {e}")),
            );
        }
        if !stock.cab_2d && !stock.cab_3d && stock.errors.is_empty() {
            stock
                .warnings
                .push("Sin cabina original: se usa cabina genérica".into());
        }
    }
    fn inspect_cvf(&mut self, path: &Path, require_views: bool) -> Result<(), ResourceFailure> {
        if let Some(result) = self.cabs.get(&(path.into(), require_views)) {
            return result.clone();
        }
        let result = read_msts_file_to_string(path)
            .and_then(|t| parse_cab_view_text(&t))
            .and_then(|a| CabViewFile::from_ast(&a))
            .map_err(|e| ResourceFailure::message(format!("Cabina {}: {e}", path.display())))
            .and_then(|cab| {
                let root = path.parent().unwrap_or(Path::new("."));
                let trainset = root.parent().unwrap_or(root);
                let dirs = vec![
                    root.to_path_buf(),
                    root.join("TEXTURES"),
                    trainset.to_path_buf(),
                    trainset.join("CABVIEW"),
                    trainset.join("CABVIEW3D"),
                ];
                // 3D needles and levers use shape geometry and textures, not
                // their legacy 2D Graphic entries. Native CVFs often contain
                // placeholders such as cab.ace which Open Rails also ignores.
                let graphics =
                    cab.controls
                        .iter()
                        .filter(|_| require_views)
                        .filter_map(|c| match c {
                            CabControl::Lever { graphic, .. }
                            | CabControl::Dial { graphic, .. }
                            | CabControl::Gauge { graphic, .. }
                            | CabControl::MultiStateDisplay { graphic, .. }
                            | CabControl::TwoStateDisplay { graphic, .. }
                            | CabControl::TriStateDisplay { graphic, .. }
                            | CabControl::Screen { graphic, .. } => Some(graphic.as_str()),
                            _ => None,
                        });
                let mut missing_resources = vec![];
                for name in cab
                    .views
                    .iter()
                    .filter(|_| require_views)
                    .map(|v| v.texture_ace.as_str())
                    .chain(graphics)
                    .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
                {
                    if asset(&dirs, name, true).is_none() {
                        let missing = MissingResource::new(path, &dirs, name, true);
                        if !missing_resources.contains(&missing) {
                            missing_resources.push(missing);
                        }
                    }
                }
                if missing_resources.is_empty() {
                    Ok(())
                } else {
                    Err(ResourceFailure {
                        message: missing_resources
                            .iter()
                            .map(|r| format!("Falta el gráfico de cabina {}", r.name))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        missing_resources,
                    })
                }
            });
        self.cabs
            .insert((path.into(), require_views), result.clone());
        result
    }
}

fn asset(dirs: &[PathBuf], name: &str, dds: bool) -> Option<PathBuf> {
    let normalized = name.trim().replace('\\', "/");
    dirs.iter().find_map(|root| {
        let path = root.join(&normalized);
        resolve_path_case_insensitive(&path)
            .filter(|p| p.is_file())
            .or_else(|| {
                dds.then(|| resolve_path_case_insensitive(&path.with_extension("dds")))
                    .flatten()
                    .filter(|p| p.is_file())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_missing_texture_and_cab_graphic_has_its_original_destinations() {
        let tmp = tempfile::tempdir().unwrap();
        let stock = tmp.path().join("TRAINS/TRAINSET/EMU");
        let cons = tmp.path().join("TRAINS/CONSISTS");
        std::fs::create_dir_all(stock.join("SHAPES")).unwrap();
        std::fs::create_dir_all(stock.join("CABVIEW")).unwrap();
        std::fs::create_dir_all(&cons).unwrap();
        let eng = stock.join("Power.eng");
        std::fs::write(&eng, "Wagon ( power Mass ( 40t ) Size ( 3m 4m 20m ) WagonShape ( body.s ) ) Engine ( power MaxPower ( 500kW ) CabView ( front.cvf ) )").unwrap();
        let shape = stock.join("SHAPES/body.s");
        std::fs::write(
            &shape,
            "(shape (texture_filenames 4 body.ace trim.ace body.ace metal.ace))",
        )
        .unwrap();
        // The presence check accepts the loader's DDS substitute for ACE.
        std::fs::write(stock.join("metal.dds"), "DDS file").unwrap();
        let cvf = stock.join("CABVIEW/front.cvf");
        std::fs::write(&cvf, "Tr_CabViewFile ( CabViewControls ( 2 Dial ( Type ( SPEEDOMETER DIAL ) Position ( 0 0 20 20 ) Graphic ( needle.ace ) ScaleRange ( 0 100 ) ) Gauge ( Type ( MAIN_RES GAUGE ) Position ( 0 0 20 20 ) Graphic ( pressure.ace ) ScaleRange ( 0 10 ) ) ) )").unwrap();
        let con = cons.join("emu.con");
        std::fs::write(&con, "Train ( TrainCfg ( emu Engine ( EngineData ( Power EMU ) ) Engine ( EngineData ( Power EMU ) ) ) )").unwrap();
        let report = ConsistAuditor::default().inspect(&con);
        assert!(!report.player_ready());
        assert_eq!(
            report.missing_resources.len(),
            4,
            "{:?}",
            report.missing_resources
        );
        for name in ["body.ace", "trim.ace", "needle.ace", "pressure.ace"] {
            let resource = report
                .missing_resources
                .iter()
                .find(|r| r.name == name)
                .unwrap();
            assert!(resource.required);
            let reference = if name.contains("needle") || name.contains("pressure") {
                &cvf
            } else {
                &shape
            };
            let reference = reference.canonicalize().unwrap();
            assert_eq!(resource.referenced_by, reference);
            assert!(
                resource
                    .destinations
                    .iter()
                    .all(|p| p.is_absolute() && p.starts_with(stock.canonicalize().unwrap()))
            );
            assert!(resource.guidance().contains(name));
            assert!(resource.guidance().contains(reference.to_str().unwrap()));
        }
    }
    #[test]
    fn nested_missing_include_is_reported_relative_to_the_containing_include() {
        let tmp = tempfile::tempdir().unwrap();
        let stock = tmp.path().join("TRAINS/TRAINSET/Stock");
        let common = tmp.path().join("TRAINS/TRAINSET/Common.Include");
        let cons = tmp.path().join("TRAINS/CONSISTS");
        for path in [&stock, &common, &cons] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::write(stock.join("Power.eng"), r#"Wagon ( power Include ( "..\\Common.Include\\base.inc" ) ) Engine ( power MaxPower ( 500kW ) )"#).unwrap();
        let include = common.join("base.inc");
        std::fs::write(&include, "Include ( details/dimensions.inc ) Mass ( 40t )").unwrap();
        let con = cons.join("stock.con");
        std::fs::write(
            &con,
            "Train ( TrainCfg ( test Engine ( EngineData ( Power Stock ) ) ) )",
        )
        .unwrap();
        let report = ConsistAuditor::default().inspect(&con);
        assert!(!report.player_ready());
        assert_eq!(report.missing_resources.len(), 1, "{:?}", report.errors);
        let missing = &report.missing_resources[0];
        assert_eq!(missing.referenced_by, include.canonicalize().unwrap());
        assert_eq!(missing.name, "details/dimensions.inc");
        assert_eq!(
            missing.destinations,
            vec![
                common
                    .canonicalize()
                    .unwrap()
                    .join("details/dimensions.inc")
            ]
        );
    }
    #[test]
    fn intact_models_do_not_hide_scripts_brake_electrical_or_sound_limitations() {
        let tmp = tempfile::tempdir().unwrap();
        let stock = tmp.path().join("TRAINS/TRAINSET/EMU");
        let cons = tmp.path().join("TRAINS/CONSISTS");
        std::fs::create_dir_all(stock.join("SOUND")).unwrap();
        std::fs::create_dir_all(stock.join("Script")).unwrap();
        std::fs::create_dir_all(&cons).unwrap();
        std::fs::write(stock.join("Power.eng"), "Wagon ( power Mass ( 40t ) Type ( Engine ) Size ( 3m 4m 20m ) BrakeSystemType ( EP ) Sound ( motor.sms ) ORTSTrackGauge ( 1676mm ) ) Engine ( power Type ( Electric ) MaxPower ( 500kW ) ORTSTrainControlSystem ( NativeTcs ) ORTSTrainBrakeController ( NativeBrake ) )").unwrap();
        std::fs::write(stock.join("SOUND/motor.sms"), "Tr_SMS ( ScalabiltyGroup ( 5 Streams ( 1 Stream ( Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( absent.wav -1 ) ) ) ) ) ) ) )").unwrap();
        std::fs::write(
            stock.join("Script/NativeTcs.cs"),
            "// present, not implicitly executed",
        )
        .unwrap();
        let con = cons.join("emu.con");
        std::fs::write(
            &con,
            "Train ( TrainCfg ( emu Engine ( EngineData ( Power EMU ) ) ) )",
        )
        .unwrap();
        let report = ConsistAuditor::default().inspect(&con);
        assert!(report.player_ready(), "{:?}", report.errors);
        assert_eq!(report.compatibility.len(), 1);
        assert_eq!(
            report.compatibility[0].declared.curve.track_gauge_m,
            Some(1.676)
        );
        for text in [
            "eléctrica parcial",
            "Frenos EP",
            "selección explícita",
            "Controlador de freno C#",
            "falta el archivo",
            "absent.wav",
        ] {
            assert!(
                report.warnings.iter().any(|w| w.contains(text)),
                "{text}: {:?}",
                report.warnings
            );
        }
        assert!(report.compatibility_summary().contains("C#"));
    }
    #[test]
    fn none_graphic_is_a_valid_control_without_a_sprite() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cab.cvf");
        std::fs::write(&path,"Tr_CabViewFile ( CabViewControls ( 1 Digital ( Type ( SPEEDOMETER DIGITAL ) Position ( 0 0 20 20 ) Graphic ( None ) ScaleRange ( 0 100 ) ) ) )").unwrap();
        assert!(ConsistAuditor::default().inspect_cvf(&path, true).is_ok());
    }
    #[test]
    fn three_dimensional_cvf_does_not_require_unused_two_dimensional_graphics() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cab.cvf");
        std::fs::write(&path, "(Tr_CabViewFile (CabViewType 2) (CabViewFile \"unused-view.ace\") (CabViewControls (Dial (Type (AMMETER DIAL)) (Position (0 0 20 20)) (Graphic \"cab.ace\") (ScaleRange (0 1000)))))").unwrap();
        let mut auditor = ConsistAuditor::default();
        assert!(auditor.inspect_cvf(&path, false).is_ok());
        assert!(auditor.inspect_cvf(&path, true).is_err());
    }
    #[test]
    fn a_working_cab_does_not_require_the_alternative_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let stock = tmp.path().join("TRAINS/TRAINSET/DMU");
        let cons = tmp.path().join("TRAINS/CONSISTS");
        std::fs::create_dir_all(stock.join("CABVIEW")).unwrap();
        std::fs::create_dir_all(&cons).unwrap();
        std::fs::write(stock.join("power.eng"), "Wagon ( power Mass ( 40t ) Size ( 3m 4m 20m ) ) Engine ( power MaxPower ( 500kW ) CabView ( front.cvf ) ORTS3DCabFile ( missing.s ) )").unwrap();
        std::fs::write(stock.join("CABVIEW/front.cvf"), "Tr_CabViewFile ( CabViewControls ( 1 Digital ( Type ( SPEEDOMETER DIGITAL ) Position ( 0 0 20 20 ) Graphic ( None ) ScaleRange ( 0 100 ) ) ) )").unwrap();
        let con = cons.join("dmu.con");
        std::fs::write(
            &con,
            "Train ( TrainCfg ( dmu Engine ( EngineData ( power DMU ) ) ) )",
        )
        .unwrap();
        let report = ConsistAuditor::default().inspect(&con);
        assert!(report.player_ready(), "{:?}", report.errors);
        assert!(report.cab_2d && !report.cab_3d);
        assert!(
            report
                .missing_resources
                .iter()
                .any(|r| r.name == "missing.s" && !r.required)
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("Cabina alternativa"))
        );
        std::fs::remove_file(stock.join("CABVIEW/front.cvf")).unwrap();
        assert!(!ConsistAuditor::default().inspect(&con).player_ready());
    }

    #[test]
    fn native_layout_case_flip_missing_stock_and_unpowered_formations() {
        let tmp = tempfile::tempdir().unwrap();
        let trains = tmp.path().join("TRAINS");
        let stock = trains.join("TRAINSET/DMU");
        let cons = trains.join("CONSISTS");
        std::fs::create_dir_all(&stock).unwrap();
        std::fs::create_dir_all(&cons).unwrap();
        std::fs::write(
            stock.join("Power.ENG"),
            "Wagon ( power Mass ( 40t ) Size ( 3m 4m 20m ) ) Engine ( power MaxPower ( 500kW ) )",
        )
        .unwrap();
        std::fs::write(
            stock.join("Coach.WAG"),
            "Wagon ( coach Mass ( 30t ) Size ( 3m 4m 20m ) )",
        )
        .unwrap();
        let con = cons.join("valid.con");
        std::fs::write(&con, "Train ( TrainCfg ( test Engine ( EngineData ( power dmu ) UiD ( 0 ) Flip ( ) ) Wagon ( WagonData ( coach dmu ) UiD ( 1 ) ) ) )").unwrap();
        let mut auditor = ConsistAuditor::default();
        let valid = auditor.inspect(&con);
        assert!(valid.player_ready(), "{:?}", valid.errors);
        let train = crate::load_consist_with_asset_root(&con, &trains).unwrap();
        assert_eq!(train.vehicles.len(), 2);
        assert!(matches!(&train.vehicles[0], crate::Vehicle::Loco(l) if l.flipped));
        std::fs::write(&con, "(Train (Wagon \"trains/dmu/coach.wag\"))").unwrap();
        let static_stock = auditor.inspect(&con);
        assert!(static_stock.content_valid());
        assert!(!static_stock.player_ready());
        std::fs::write(&con, "(Train (Engine \"trains/dmu/absent.eng\"))").unwrap();
        let missing = auditor.inspect(&con);
        assert!(!missing.content_valid());
        assert!(missing.compatibility_summary().contains("incompleto"));
        assert_eq!(
            missing.missing_resources[0].referenced_by,
            con.canonicalize().unwrap()
        );
        assert_eq!(
            missing.missing_resources[0].destinations[0],
            stock.canonicalize().unwrap().join("absent.eng")
        );
    }
}
