//! Optional exogenous adhesion inputs. Recorded filter outputs, force and
//! motion are deliberately absent from this runtime input format.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
    sync::Arc,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdhesionFilter {
    pub x: [f32; 2],
    pub y: [f32; 2],
}
impl AdhesionFilter {
    pub fn valid(&self) -> bool {
        self.x
            .iter()
            .chain(&self.y)
            .all(|v| v.is_finite() && v.abs() <= 10.)
    }
    /// Orts.Common.IIRFilter.Butterworth(order1, HzToRad(1)), including the
    /// Double divisor in Math.Tan and Single coefficients/accumulation.
    pub fn advance(&mut self, input: f32, dt: f32) -> f32 {
        if dt <= 0. {
            return 0.;
        }
        let cutoff = 2_f32 * std::f32::consts::PI;
        if 1_f32 / dt < cutoff / (2_f32 * std::f32::consts::PI) {
            return input;
        }
        let warp = 4_f32 * (f64::from(cutoff * dt) / 2_f64).tan() as f32;
        let numerator = warp * 0.5_f32;
        let denominator = 2_f32 + numerator;
        let a = numerator / denominator;
        let b = (numerator - 2_f32) / denominator;
        self.x[1] = self.x[0];
        self.y[1] = self.y[0];
        self.x[0] = input;
        self.y[0] = a * self.x[0];
        self.y[0] += a * self.x[1] - b * self.y[1];
        self.y[0]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdhesionInputState {
    pub filter: AdhesionFilter,
    pub cursor: usize,
    pub source_sha256: String,
    pub source_vehicle: usize,
    pub base_factor: f32,
}
impl AdhesionInputState {
    pub fn valid(&self) -> bool {
        self.filter.valid()
            && self.base_factor.is_finite()
            && (0. ..=2.5).contains(&self.base_factor)
            && self.source_sha256.len() == 64
            && self.source_sha256.bytes().all(|c| c.is_ascii_hexdigit())
    }
}

#[derive(Clone, Debug)]
pub struct AdhesionInputSample {
    pub time_s: f64,
    pub input_factor: f32,
    pub base_factor: f32,
}
#[derive(Clone, Debug)]
pub struct AdhesionInputSeries {
    pub sha256: String,
    pub vehicle: usize,
    pub step_s: f64,
    pub initial: AdhesionInputState,
    pub samples: Vec<AdhesionInputSample>,
}
impl AdhesionInputSeries {
    pub fn valid_state(&self, state: &AdhesionInputState, time_s: f64, terminal: bool) -> bool {
        state.valid()
            && state.source_sha256 == self.sha256
            && state.source_vehicle == self.vehicle
            && self.samples.get(state.cursor).is_some_and(|sample| {
                let error = sample.time_s - time_s;
                error.abs() <= 1e-6 || (terminal && (0. ..=self.step_s + 1e-6).contains(&error))
            })
    }
    pub fn next(
        &self,
        state: &AdhesionInputState,
        time_s: f64,
        dt: f64,
    ) -> Result<&AdhesionInputSample, String> {
        if !self.valid_state(state, time_s, false)
            || !dt.is_finite()
            || (dt - self.step_s).abs() > 1e-8
        {
            return Err("Las entradas ambientales requieren su reloj, formación y paso de física originales".into());
        }
        self.samples.get(state.cursor + 1).ok_or_else(||
            "Se agotaron las entradas ambientales registradas; el ensayo no puede continuar sin ellas".into())
    }
}

pub type VehicleAdhesionInputs = Vec<Option<Arc<AdhesionInputSeries>>>;

/// The path is contained within the scenario, including after symlink
/// resolution. Input files cannot read arbitrary paths from downloaded TOML.
pub fn load(
    scenario_dir: &Path,
    relative: &str,
    step_s: f64,
    duration_s: f64,
    vehicle_count: usize,
    locomotive_indices: &[usize],
) -> Result<VehicleAdhesionInputs, String> {
    if locomotive_indices.is_empty()
        || locomotive_indices.iter().any(|i| *i >= vehicle_count)
        || locomotive_indices.iter().collect::<BTreeSet<_>>().len() != locomotive_indices.len()
    {
        return Err("El CSV ambiental requiere índices originales únicos de locomotoras".into());
    }
    let rel = Path::new(relative);
    if relative.is_empty()
        || relative.len() > 1024
        || relative.contains('\\')
        || rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        || !step_s.is_finite()
        || step_s <= 0.
        || !duration_s.is_finite()
        || duration_s < 0.
    {
        return Err("Ruta o paso de entradas ambientales inválidos".into());
    }
    let root = scenario_dir
        .canonicalize()
        .map_err(|e| format!("Carpeta del escenario: {e}"))?;
    let path = root
        .join(rel)
        .canonicalize()
        .map_err(|e| format!("Entradas ambientales: {e}"))?;
    if !path.starts_with(&root)
        || !path.is_file()
        || path.metadata().map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024
    {
        return Err(
            "El CSV ambiental debe ser un archivo de hasta 32 MiB dentro del escenario".into(),
        );
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(bytes.as_slice());
    let header = [
        "time_s",
        "vehicle",
        "input_factor",
        "base_factor",
        "filter_x0",
        "filter_x1",
        "filter_y0",
        "filter_y1",
    ];
    if reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .ne(header)
    {
        return Err("El CSV ambiental debe contener sólo tiempo, vehículo, entrada, base y contexto inicial del filtro".into());
    }
    let mut series: BTreeMap<usize, AdhesionInputSeries> = BTreeMap::new();
    let mut previous_time = 0.;
    for (row_index, row) in reader.records().enumerate() {
        if row_index >= 1_500_000 {
            return Err("Demasiadas filas ambientales".into());
        }
        let row = row.map_err(|e| e.to_string())?;
        let number = |i: usize| -> Result<f64, String> {
            row[i]
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("Valor ambiental inválido en fila {}", row_index + 2))
        };
        let time = number(0)?;
        let vehicle = row[1]
            .parse::<usize>()
            .map_err(|_| "Índice de vehículo ambiental inválido")?;
        let input = number(2)?;
        let base = number(3)?;
        if vehicle >= vehicle_count
            || !locomotive_indices.contains(&vehicle)
            || time < previous_time - 1e-7
            || !(-5. ..=5.).contains(&input)
            || !(0. ..=2.5).contains(&base)
        {
            return Err(format!(
                "Tiempo, vehículo o factor ambiental fuera de rango en fila {}",
                row_index + 2
            ));
        }
        previous_time = time;
        if let std::collections::btree_map::Entry::Vacant(entry) = series.entry(vehicle) {
            if time.abs() > 1e-7 {
                return Err("Cada locomotora debe declarar contexto ambiental en t=0".into());
            }
            let filter = AdhesionFilter {
                x: [number(4)? as f32, number(5)? as f32],
                y: [number(6)? as f32, number(7)? as f32],
            };
            let initial = AdhesionInputState {
                filter,
                cursor: 0,
                source_sha256: sha.clone(),
                source_vehicle: vehicle,
                base_factor: base as f32,
            };
            if !initial.valid() {
                return Err("Contexto inicial del filtro ambiental inválido".into());
            }
            entry.insert(AdhesionInputSeries {
                sha256: sha.clone(),
                vehicle,
                step_s,
                initial,
                samples: Vec::new(),
            });
        } else if row.iter().skip(4).any(|v| !v.is_empty()) {
            return Err("Sólo t=0 puede contener estado del filtro; las salidas observadas posteriores no son entradas".into());
        }
        let s = series.get_mut(&vehicle).unwrap();
        if (time - s.samples.len() as f64 * step_s).abs() > 1e-7 {
            return Err(
                "Las entradas ambientales deben cubrir todos los pasos, sin huecos ni duplicados"
                    .into(),
            );
        }
        s.samples.push(AdhesionInputSample {
            time_s: time,
            input_factor: input as f32,
            base_factor: base as f32,
        });
    }
    if series.len() != locomotive_indices.len() {
        return Err("Faltan locomotoras en el CSV ambiental".into());
    }
    let mut result = vec![None; vehicle_count];
    for (index, s) in series {
        if s.samples
            .last()
            .is_none_or(|r| r.time_s + 1e-7 < duration_s)
        {
            return Err("El CSV ambiental no cubre toda la duración del escenario".into());
        }
        result[index] = Some(Arc::new(s));
    }
    Ok(result)
}
