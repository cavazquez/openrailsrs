//! Authored MSTS / Open Rails couplers, resolved once when loading a formation.
//!
//! OR 1.6.1 stores the first Coupling as the rear end and selects the second
//! when a vehicle is flipped. Advanced stiffness values are *forces*, unlike
//! the legacy Stiffness values, which are N/m. Keep that distinction here.
use std::path::Path;

use openrailsrs_formats::{Ast, Atom, ConsistFile};
use serde::{Deserialize, Serialize};

use crate::TrainError;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CouplerPoint {
    pub displacement_m: f64,
    pub force_n: f64,
}

/// Monotonic loading envelope. Displacement includes the force-free gap.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CouplerCurve {
    pub points: Vec<CouplerPoint>,
}

impl CouplerCurve {
    pub fn valid(&self) -> bool {
        self.points.len() >= 2
            && self.points.len() <= 32
            && self.points[0].force_n == 0.
            && self.points.iter().all(|p| {
                p.displacement_m.is_finite()
                    && p.displacement_m >= 0.
                    && p.displacement_m <= 100.
                    && p.force_n.is_finite()
                    && p.force_n >= 0.
                    && p.force_n <= 1e13
            })
            && self
                .points
                .windows(2)
                .all(|p| p[1].displacement_m > p[0].displacement_m && p[1].force_n >= p[0].force_n)
    }
    pub fn slack_m(&self) -> f64 {
        self.points
            .windows(2)
            .find(|p| p[1].force_n > 0.)
            .map_or_else(|| self.stroke_m(), |p| p[0].displacement_m)
    }
    pub fn stroke_m(&self) -> f64 {
        self.points.last().unwrap().displacement_m
    }
    pub fn max_force_n(&self) -> f64 {
        self.points.last().unwrap().force_n
    }
    pub fn maximum_stiffness_npm(&self) -> f64 {
        self.points
            .windows(2)
            .map(|p| (p[1].force_n - p[0].force_n) / (p[1].displacement_m - p[0].displacement_m))
            .fold(0., f64::max)
    }
    pub fn force_n(&self, displacement: f64) -> f64 {
        if displacement <= self.slack_m() {
            return 0.;
        }
        for p in self.points.windows(2) {
            if displacement <= p[1].displacement_m {
                let t = (displacement - p[0].displacement_m)
                    / (p[1].displacement_m - p[0].displacement_m);
                return p[0].force_n + t * (p[1].force_n - p[0].force_n);
            }
        }
        self.max_force_n()
    }
    pub fn displacement_m(&self, force: f64) -> f64 {
        if force <= 0. {
            return self.slack_m();
        }
        for p in self.points.windows(2) {
            if force <= p[1].force_n {
                let t = (force - p[0].force_n) / (p[1].force_n - p[0].force_n);
                return p[0].displacement_m + t * (p[1].displacement_m - p[0].displacement_m);
            }
        }
        self.stroke_m()
    }
    pub fn elastic_energy_j(&self, displacement: f64) -> f64 {
        let mut energy = 0.;
        for p in self.points.windows(2) {
            let end = displacement.min(p[1].displacement_m);
            if end <= p[0].displacement_m {
                break;
            }
            energy += 0.5 * (p[0].force_n + self.force_n(end)) * (end - p[0].displacement_m);
        }
        energy
    }
    /// Both draft gears carry the same force; their travels add. Do not sum
    /// two spring forces (or add a second buffer force) at a connected joint.
    pub fn in_series(a: &Self, b: &Self) -> Self {
        // A gear with no authored restoring load travels to its hard stop
        // before the other gear carries load. Do not invent a compression
        // spring: MSTSCoupling's default compression forces are both zero.
        let maximum = match (a.max_force_n(), b.max_force_n()) {
            (0., b) => b,
            (a, 0.) => a,
            (a, b) => a.min(b),
        };
        if maximum == 0. {
            return Self {
                points: vec![
                    CouplerPoint {
                        displacement_m: 0.,
                        force_n: 0.,
                    },
                    CouplerPoint {
                        displacement_m: a.stroke_m() + b.stroke_m(),
                        force_n: 0.,
                    },
                ],
            };
        }
        let mut forces = vec![0., maximum];
        forces.extend(
            a.points
                .iter()
                .chain(&b.points)
                .map(|p| p.force_n)
                .filter(|f| *f > 0. && *f < maximum),
        );
        forces.sort_by(f64::total_cmp);
        forces.dedup();
        Self {
            points: forces
                .into_iter()
                .map(|f| CouplerPoint {
                    displacement_m: a.displacement_m(f) + b.displacement_m(f),
                    force_n: f,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CouplerEndProfile {
    pub advanced: bool,
    pub rigid: bool,
    pub zero_length_m: f64,
    pub warning_force_n: f64,
    pub break_force_n: f64,
    pub tension: CouplerCurve,
    pub compression: CouplerCurve,
    /// Original units/values, for diagnostics and DLL comparison.
    pub simple_r0_m: [f64; 2],
    pub simple_stiffness_npm: [f64; 2],
    pub simple_slack_m: f64,
    pub source: String,
}

impl CouplerEndProfile {
    /// TrainCar defaults when there is no authored Coupling. Distinct from a
    /// present MSTSCoupling whose own defaults use R0X=0 and 7*(K1+K2).
    pub fn unauthored() -> Self {
        let curve = CouplerCurve {
            points: vec![
                CouplerPoint {
                    displacement_m: 0.03 / 2.,
                    force_n: 0.,
                },
                CouplerPoint {
                    displacement_m: 0.035 / 2.,
                    force_n: 100_000.,
                },
            ],
        };
        Self {
            advanced: false,
            rigid: false,
            zero_length_m: 0.,
            warning_force_n: 1e10,
            break_force_n: 1e10,
            tension: curve.clone(),
            compression: curve,
            simple_r0_m: [0., 0.],
            simple_stiffness_npm: [1e7, 1e7],
            simple_slack_m: 0.03,
            source: "Open Rails 1.6.1 TrainCar defaults (no Coupling)".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VehicleCouplerProfile {
    pub rear: Option<CouplerEndProfile>,
    pub front: Option<CouplerEndProfile>,
    pub flipped: bool,
}
impl VehicleCouplerProfile {
    pub fn rear_in_consist(&self) -> Option<&CouplerEndProfile> {
        if self.flipped {
            self.front.as_ref().or(self.rear.as_ref())
        } else {
            self.rear.as_ref()
        }
    }
    pub fn front_in_consist(&self) -> Option<&CouplerEndProfile> {
        if self.flipped {
            self.rear.as_ref()
        } else {
            self.front.as_ref().or(self.rear.as_ref())
        }
    }
}

fn blocks<'a>(ast: &'a Ast, name: &str, out: &mut Vec<&'a [Ast]>) {
    if let Ast::List(items) = ast {
        if matches!(items.first(), Some(Ast::Atom(Atom::Symbol(key))) if key.eq_ignore_ascii_case(name))
        {
            out.push(&items[1..]);
        } else {
            for child in items {
                blocks(child, name, out);
            }
        }
    }
}
fn text(atom: &Ast) -> Option<String> {
    match atom {
        Ast::Atom(a) => Some(match a {
            Atom::Symbol(s) | Atom::String(s) => s.clone(),
            Atom::Number(n) => n.to_string(),
            Atom::Integer(n) => n.to_string(),
        }),
        _ => None,
    }
}
fn invalid(field: &str) -> TrainError {
    TrainError::Parse(format!(
        "invalid coupler {field}: expected finite nonnegative quantities with supported units"
    ))
}
fn quantity(raw: &str, kind: &str) -> Option<f64> {
    let raw = raw.trim().to_ascii_lowercase().replace(' ', "");
    let split = raw
        .find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '+' | '-' | 'e'))
        .unwrap_or(raw.len());
    let n = raw[..split].parse::<f64>().ok()?;
    let scale = match (kind, &raw[split..]) {
        (_, "") | ("m", "m") | ("n", "n") | ("npm", "n/m") => 1.,
        ("m", "cm") => 0.01,
        ("m", "mm") => 0.001,
        ("m", "ft") => 0.3048,
        ("m", "in") => 0.0254,
        ("n", "kn") | ("npm", "kn/m") => 1000.,
        ("n", "mn") | ("npm", "mn/m") => 1e6,
        ("n", "lbf") => 4.4482216152605,
        ("npm", "lbf/ft") => 4.4482216152605 / 0.3048,
        _ => return None,
    };
    let value = n * scale;
    (value.is_finite() && value >= 0.).then_some(value)
}
fn pair(values: &[Ast], kind: &str, name: &str) -> Result<[f64; 2], TrainError> {
    let mut raw = Vec::<String>::new();
    for token in values.iter().filter_map(text) {
        if token.starts_with(|c: char| c.is_ascii_alphabetic()) || token.starts_with('/') {
            let Some(last) = raw.last_mut() else {
                return Err(invalid(name));
            };
            last.push_str(&token);
        } else {
            raw.push(token);
        }
    }
    if raw.len() != 2 {
        return Err(invalid(name));
    }
    Ok([
        quantity(&raw[0], kind).ok_or_else(|| invalid(name))?,
        quantity(&raw[1], kind).ok_or_else(|| invalid(name))?,
    ])
}
fn curve(zones: [f64; 3], forces: [f64; 2], field: &str) -> Result<CouplerCurve, TrainError> {
    if !zones
        .iter()
        .chain(&forces)
        .all(|v| v.is_finite() && *v >= 0.)
        || zones[0] >= zones[2]
        || forces[1] < forces[0]
    {
        return Err(invalid(field));
    }
    let mut points = vec![CouplerPoint {
        displacement_m: zones[0],
        force_n: 0.,
    }];
    if zones[1] > zones[0] && zones[1] < zones[2] && forces[1] > forces[0] {
        points.push(CouplerPoint {
            displacement_m: zones[1],
            force_n: forces[0],
        });
    }
    points.push(CouplerPoint {
        displacement_m: zones[2],
        force_n: forces[1],
    });
    let result = CouplerCurve { points };
    result
        .valid()
        .then_some(result)
        .ok_or_else(|| invalid(field))
}
fn parse_end(
    values: &[Ast],
    source: String,
    advanced: bool,
) -> Result<CouplerEndProfile, TrainError> {
    // MSTSCoupling defaults. Process fields in source order, as STFReader does.
    let mut r0 = [0., 0.];
    let mut stiffness = [1e7, 2e7];
    let mut slack = 0.012_f64;
    let mut breaks = [1e10, 1e10];
    let mut rigid = false;
    let mut tension_r0 = [0., 0.];
    let mut compression_r0 = [0., 0.];
    let mut tension_slack = [0., 0.1];
    let mut compression_slack = [0., 0.1];
    let mut tension_force = [1e7, 2e7];
    let mut compression_force = [0., 0.];
    let mut fields = Vec::new();
    for v in values {
        if let Ast::List(items) = v {
            if matches!(items.first(),Some(Ast::Atom(Atom::Symbol(k))) if k.eq_ignore_ascii_case("Spring"))
            {
                fields.extend(&items[1..]);
            } else {
                fields.push(v);
            }
        }
    }
    for field in fields {
        let Ast::List(items) = field else { continue };
        let Some(name) = items.first().and_then(text) else {
            continue;
        };
        let data = &items[1..];
        let key = name.to_ascii_lowercase();
        match key.as_str() {
            "stiffness" => stiffness = pair(data, "npm", &name)?,
            "r0" => {
                r0 = pair(data, "m", &name)?;
                slack = if r0[0] == 0. {
                    (r0[1] / 2. * stiffness[1] / (stiffness[0] + stiffness[1])).clamp(0.001, 0.1)
                } else {
                    0.012
                };
            }
            "break" | "ortsbreak" => {
                breaks = pair(data, "n", &name)?;
                if breaks[1] == 0. {
                    breaks[1] = 2e7;
                }
            }
            "couplinghasrigidconnection" => {
                rigid = data
                    .first()
                    .and_then(text)
                    .is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            }
            "ortstensionstiffness" => tension_force = pair(data, "n", &name)?,
            "ortscompressionstiffness" => compression_force = pair(data, "n", &name)?,
            "ortstensionr0" => tension_r0 = pair(data, "m", &name)?,
            "ortscompressionr0" => compression_r0 = pair(data, "m", &name)?,
            "ortstensionslack" => {
                tension_slack = pair(data, "m", &name)?;
            }
            "ortscompressionslack" => {
                compression_slack = pair(data, "m", &name)?;
            }
            _ => {}
        }
    }
    let (tension, compression) = if advanced {
        let zones = |r0: [f64; 2], s: [f64; 2], compression: bool| {
            if rigid {
                [if compression { 0.00005 } else { 0.00001 }, 0.0001, 0.0002]
            } else {
                [
                    r0[1],
                    r0[1] + s[0],
                    r0[1] + s[0] + if s[1] == 0. { 0.1 } else { s[1] },
                ]
            }
        };
        let scale = if rigid { 10. } else { 1. };
        (
            curve(
                zones(tension_r0, tension_slack, false),
                tension_force.map(|f| f * scale),
                "ORTSTension",
            )?,
            curve(
                zones(compression_r0, compression_slack, true),
                compression_force.map(|f| f * scale),
                "ORTSCompression",
            )?,
        )
    } else {
        let k = if r0[0] == 0. {
            7. * (stiffness[0] + stiffness[1])
        } else {
            2e7
        };
        let gap = if rigid { 0.0001 } else { slack };
        let maximum = if rigid {
            0.0002
        } else {
            0.035_f64.max(gap + 0.0001)
        };
        let c = curve(
            [gap / 2., gap / 2., maximum / 2.],
            [k * (maximum - gap); 2],
            "Stiffness / R0",
        )?;
        (c.clone(), c)
    };
    Ok(CouplerEndProfile {
        advanced,
        rigid,
        zero_length_m: if advanced { r0[0].min(0.5) } else { r0[0] },
        warning_force_n: breaks[0],
        break_force_n: breaks[1],
        tension,
        compression,
        simple_r0_m: r0,
        simple_stiffness_npm: stiffness,
        simple_slack_m: if rigid { 0.0001 } else { slack },
        source,
    })
}

pub fn parse_vehicle_couplers(
    ast: &Ast,
    flipped: bool,
) -> Result<VehicleCouplerProfile, TrainError> {
    let mut found = Vec::new();
    blocks(ast, "Coupling", &mut found);
    let mut out = VehicleCouplerProfile {
        flipped,
        ..Default::default()
    };
    // OR stores this flag on the wagon, not on an individual end. A front
    // override can enable advanced handling of the selected rear coupling.
    let mut advanced_fields = Vec::new();
    blocks(ast, "ORTSTensionSlack", &mut advanced_fields);
    blocks(ast, "ORTSCompressionSlack", &mut advanced_fields);
    let advanced = !advanced_fields.is_empty();
    for (i, values) in found.into_iter().enumerate() {
        let parsed = parse_end(
            values,
            format!("Coupling {} (OR1.6.1 source order)", i + 1),
            advanced,
        )?;
        if i % 2 == 0 {
            out.rear = Some(parsed)
        } else {
            out.front = Some(parsed)
        }
    }
    Ok(out)
}

pub fn load_consist_coupler_profiles(
    consist: &Path,
    base: &Path,
) -> Result<Vec<VehicleCouplerProfile>, TrainError> {
    let ast = openrailsrs_formats::read_vehicle_ast(consist)?;
    ConsistFile::from_ast(&ast)?
        .entries
        .iter()
        .map(|entry| {
            let path = crate::from_ast::resolve_consist_entry_path(base, entry.path());
            if crate::steam_loader::is_toml_eng(&path).unwrap_or(false) {
                return Ok(VehicleCouplerProfile {
                    flipped: entry.flipped(),
                    ..Default::default()
                });
            }
            let mut profile = parse_vehicle_couplers(
                &openrailsrs_formats::read_vehicle_ast(&path)?,
                entry.flipped(),
            )?;
            for end in [&mut profile.rear, &mut profile.front]
                .into_iter()
                .flatten()
            {
                end.source = format!("{}: {}", path.display(), end.source);
            }
            Ok(profile)
        })
        .collect()
}
