//! Installed formation validation. A wagon-only formation is valid static stock,
//! but cannot be selected as the player's powered train. This reports content
//! integrity, not physics parity or support for arbitrary C# scripts.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use openrailsrs_formats::{
    CabControl, CabViewFile, ConsistEntry, ConsistFile, EngineCabView, MstsFile, ShapeFile,
    parse_cab_view_text, parse_msts_file, parse_vehicle_text, read_msts_file_to_string,
    resolve_path_case_insensitive,
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
}

/// Cache shared vehicles, shapes and cab panels across hundreds of `.con` files.
/// Additional TRAINSET roots mirror the viewer's preference for original models
/// over the small project physics fixtures.
#[derive(Clone, Debug, Default)]
pub struct ConsistAuditor {
    pub trainset_roots: Vec<PathBuf>,
    stocks: HashMap<PathBuf, Result<StockAudit, String>>,
    shapes: HashMap<PathBuf, Result<(), String>>,
    cabs: HashMap<(PathBuf, bool), Result<(), String>>,
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
                Err(e) => report.errors.push(format!("{prefix}: {e}")),
                Ok(stock) => {
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
                }
            }
        }
        report
    }
    fn inspect_stock(&mut self, path: &Path) -> Result<StockAudit, String> {
        if !path.is_file() {
            return Err("Falta el archivo ENG/WAG".into());
        }
        let (mass, length, powered, shape, cab) =
            match parse_msts_file(path).map_err(|e| e.to_string())? {
                MstsFile::Engine(e) => (
                    e.mass_kg,
                    e.length_m,
                    e.max_power_w > 0.0 || e.steam.is_some(),
                    e.wagon_shape,
                    Some(e.cab),
                ),
                MstsFile::Wagon(w) => (w.mass_kg, w.length_m, false, w.wagon_shape, None),
                _ => return Err("El archivo no es un vehículo".into()),
            };
        let authored_root = path.parent().unwrap_or(Path::new("."));
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
        };
        if !mass.is_finite() || mass <= 0.0 || !length.is_finite() || length <= 0.0 {
            stock.errors.push("Masa o longitud inválida".into());
        }
        if let Some(shape) = shape {
            match asset(&[root.clone(), root.join("SHAPES")], &shape, false) {
                Some(path) => {
                    if let Err(e) = self.inspect_shape(&path) {
                        stock.errors.push(e);
                    }
                }
                None => stock.errors.push(format!("Falta el modelo {shape}")),
            }
        } else {
            stock
                .warnings
                .push("Sin modelo declarado: se usa geometría genérica".into());
        }
        if let Some(cab) = cab {
            self.inspect_cab(&root, &cab, &mut stock);
        }
        Ok(stock)
    }
    fn inspect_shape(&mut self, path: &Path) -> Result<(), String> {
        if let Some(result) = self.shapes.get(path) {
            return result.clone();
        }
        let result = ShapeFile::from_path(path)
            .map_err(|e| format!("Modelo {}: {e}", path.display()))
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
                for name in &shape.texture_filenames {
                    if asset(&dirs, name, true).is_none() {
                        return Err(format!(
                            "Falta la textura {name} de {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ));
                    }
                }
                Ok(())
            });
        self.shapes.insert(path.into(), result.clone());
        result
    }
    fn inspect_cab(&mut self, root: &Path, cab: &EngineCabView, stock: &mut StockAudit) {
        let dirs = vec![
            root.join("CABVIEW"),
            root.join("CABVIEW3D"),
            root.to_path_buf(),
        ];
        if let Some(name) = &cab.cab_view_file {
            match asset(&dirs, name, false) {
                Some(p) => match self.inspect_cvf(&p, true) {
                    Ok(()) => stock.cab_2d = true,
                    Err(e) => stock.errors.push(e),
                },
                None => stock.errors.push(format!("Falta la cabina {name}")),
            }
        }
        if let Some(name) = &cab.orts_3d_cab_shape {
            match asset(&dirs, name, false) {
                Some(p) => {
                    if let Err(e) = self.inspect_shape(&p) {
                        stock.errors.push(e);
                    } else if let Some(cvf) =
                        resolve_path_case_insensitive(&p.with_extension("cvf"))
                    {
                        match self.inspect_cvf(&cvf, false) {
                            Ok(()) => stock.cab_3d = true,
                            Err(e) => stock.errors.push(e),
                        }
                    } else {
                        stock
                            .errors
                            .push(format!("Faltan los instrumentos CVF de {name}"));
                    }
                }
                None => stock.errors.push(format!("Falta la cabina 3D {name}")),
            }
        } else if let Some(c) = openrailsrs_formats::resolve_cab_assets(root, cab) {
            stock.cab_3d = self.inspect_shape(&c.shape_path).is_ok()
                && self.inspect_cvf(&c.cvf_path, false).is_ok();
        }
        if !stock.cab_2d && !stock.cab_3d && stock.errors.is_empty() {
            stock
                .warnings
                .push("Sin cabina original: se usa cabina genérica".into());
        }
    }
    fn inspect_cvf(&mut self, path: &Path, require_views: bool) -> Result<(), String> {
        if let Some(result) = self.cabs.get(&(path.into(), require_views)) {
            return result.clone();
        }
        let result = read_msts_file_to_string(path)
            .and_then(|t| parse_cab_view_text(&t))
            .and_then(|a| CabViewFile::from_ast(&a))
            .map_err(|e| format!("Cabina {}: {e}", path.display()))
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
                for name in cab
                    .views
                    .iter()
                    .filter(|_| require_views)
                    .map(|v| v.texture_ace.as_str())
                    .chain(graphics)
                    .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
                {
                    if asset(&dirs, name, true).is_none() {
                        return Err(format!("Falta el gráfico de cabina {name}"));
                    }
                }
                Ok(())
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
        assert!(!auditor.inspect(&con).content_valid());
    }
}
