//! Loading-gauge declarations require a named source. Missing data is unknown.
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InfrastructureEnvelope {
    pub source: String,
    pub max_width_m: f64,
    pub max_height_m: f64,
    /// Zero explicitly declares straight track; None leaves curvature unknown.
    pub radius_m: Option<f64>,
    #[serde(default)]
    pub raised_collectors_verified: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Assessment {
    Compatible(String),
    Incompatible(String),
    Unknown(String),
}
impl Assessment {
    pub fn text(&self) -> &str {
        match self {
            Self::Compatible(s) | Self::Incompatible(s) | Self::Unknown(s) => s,
        }
    }
}
impl InfrastructureEnvelope {
    pub fn valid(&self) -> bool {
        !self.source.trim().is_empty()
            && self.source.len() <= 512
            && !self.source.chars().any(char::is_control)
            && self.max_width_m.is_finite()
            && (1. ..=20.).contains(&self.max_width_m)
            && self.max_height_m.is_finite()
            && (1. ..=20.).contains(&self.max_height_m)
            && self
                .radius_m
                .is_none_or(|r| r.is_finite() && (r == 0. || r >= 20.))
    }
}
pub fn load(root: &Path) -> Option<InfrastructureEnvelope> {
    let file = root.join("loading-gauge.toml");
    if std::fs::metadata(&file).ok()?.len() > 65536 {
        return None;
    }
    let p: InfrastructureEnvelope = toml::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    p.valid().then_some(p)
}
pub fn compare(
    audit: &openrailsrs_train::ConsistAudit,
    profile: Option<&InfrastructureEnvelope>,
) -> Assessment {
    let Some(profile) = profile else {
        return Assessment::Unknown(
            "Gálibo de infraestructura desconocido; se conserva la selección".into(),
        );
    };
    if !profile.valid() {
        return Assessment::Unknown("Declaración de gálibo inválida".into());
    }
    if audit.compatibility.len() != audit.vehicles {
        return Assessment::Unknown("Falta envolvente de algún vehículo".into());
    }
    let mut curvature_unknown = false;
    for car in &audit.compatibility {
        let Some([width, height, length]) = car.declared.size_m else {
            return Assessment::Unknown("Algún ENG/WAG no declara Size completo".into());
        };
        if width > profile.max_width_m + 0.01 || height > profile.max_height_m + 0.01 {
            return Assessment::Incompatible(format!(
                "Gálibo incompatible: vehículo {} {:.2} × {:.2} m, infraestructura {:.2} × {:.2} m · {}",
                car.vehicle + 1,
                width,
                height,
                profile.max_width_m,
                profile.max_height_m,
                profile.source
            ));
        }
        if car.declared.electric.as_ref().is_some_and(|e| {
            e.pickup == openrailsrs_core::electrification::ElectricPickup::Overhead
        }) && !profile.raised_collectors_verified
        {
            return Assessment::Unknown(
                "El cuerpo cabe, pero falta validar el pantógrafo elevado".into(),
            );
        }
        match profile.radius_m {
            Some(radius) if radius > 0. => {
                let conservative_width = width + length * length / (4. * radius);
                if conservative_width > profile.max_width_m {
                    curvature_unknown = true;
                }
            }
            None => curvature_unknown = true,
            _ => {}
        }
    }
    if curvature_unknown {
        Assessment::Unknown(
            "El cuerpo cabe en recta; falta comprobar la envolvente barrida en curvas".into(),
        )
    } else {
        Assessment::Compatible(format!(
            "Gálibo declarado compatible con {} · no certifica infraestructura sin datos",
            profile.source
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_envelopes_remain_selectable_and_curvature_is_conservative() {
        let ast = openrailsrs_formats::parse_named_stf("Wagon ( Size ( 3m 4m 20m ) )").unwrap();
        let mut metadata = openrailsrs_formats::parse_vehicle_content_metadata(&ast, false);
        metadata.size_m = Some([3., 4., 20.]);
        let mut audit = openrailsrs_train::ConsistAudit {
            vehicles: 1,
            compatibility: vec![openrailsrs_train::consist_audit::VehicleCompatibility {
                vehicle: 0,
                stock_path: "own.wag".into(),
                source_sha256: String::new(),
                declared: metadata,
                limitations: vec![],
            }],
            ..Default::default()
        };
        let mut profile = InfrastructureEnvelope {
            source: "Author dimensions, own test".into(),
            max_width_m: 3.2,
            max_height_m: 4.5,
            radius_m: Some(0.),
            raised_collectors_verified: false,
        };
        assert!(matches!(compare(&audit, None), Assessment::Unknown(_)));
        assert!(matches!(
            compare(&audit, Some(&profile)),
            Assessment::Compatible(_)
        ));
        profile.max_width_m = 2.8;
        assert!(matches!(
            compare(&audit, Some(&profile)),
            Assessment::Incompatible(_)
        ));
        profile.max_width_m = 3.2;
        profile.radius_m = Some(100.);
        assert!(matches!(
            compare(&audit, Some(&profile)),
            Assessment::Unknown(_)
        ));
        audit.compatibility[0].declared.size_m = None;
        assert!(matches!(
            compare(&audit, Some(&profile)),
            Assessment::Unknown(_)
        ));
    }
}
