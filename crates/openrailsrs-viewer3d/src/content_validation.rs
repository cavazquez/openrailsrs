//! Independent validation dimensions. Resource availability never certifies physics.
use openrailsrs_train::ConsistAudit;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
struct Registry {
    schema: u8,
    reference_version: String,
    reference_commit: String,
    records: Vec<Evidence>,
}
#[derive(Debug, Deserialize)]
pub struct Evidence {
    pub source: String,
    pub source_sha256: String,
    pub scope: String,
    pub points: usize,
    pub evidence: String,
    pub evidence_sha256: String,
    pub provenance: String,
}
impl Registry {
    fn valid(&self) -> bool {
        let reference: toml::Value =
            toml::from_str(include_str!("../../../oracles/openrails-reference.toml"))
                .expect("pinned reference");
        self.schema == 1
            && Some(self.reference_version.as_str())
                == reference.get("version").and_then(toml::Value::as_str)
            && Some(self.reference_commit.as_str())
                == reference.get("commit").and_then(toml::Value::as_str)
    }
    fn evidence(&self, fingerprint: &str) -> Option<&Evidence> {
        if !self.valid() || fingerprint.is_empty() {
            return None;
        }
        self.records.iter().find(|e| {
            e.source_sha256 == fingerprint
                && e.points > 0
                && e.evidence_sha256.len() == 64
                && !e.scope.is_empty()
        })
    }
}
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        serde_json::from_str(include_str!("../../../oracles/content-validation.json"))
            .expect("validated embedded evidence registry")
    })
}
/// Evidence is per exact effective source, including overrides and dependencies.
pub fn evidence(fingerprint: &str) -> Option<&'static Evidence> {
    registry().evidence(fingerprint)
}
pub fn summary(audit: &ConsistAudit) -> String {
    let compared = audit
        .compatibility
        .iter()
        .filter(|v| evidence(&v.source_sha256).is_some())
        .count();
    format!(
        "Validación de recursos: {}\nCirculación: sin ensayo registrado de esta combinación\nSistemas contrastados: {compared}/{} vehículos · Open Rails {} · alcance parcial",
        if audit.content_valid() {
            "completos"
        } else {
            "incompletos o inválidos"
        },
        audit.vehicles,
        registry().reference_version
    )
}
pub fn details(audit: &ConsistAudit) -> String {
    let mut text = summary(audit);
    text += "\nUn ensayo de frenos no certifica cabina, sonido, scripts ni un recorrido completo.";
    for v in &audit.compatibility {
        text += &format!(
            "\n\nCoche {} · {}",
            v.vehicle,
            v.stock_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
        if let Some(e) = evidence(&v.source_sha256) {
            text += &format!(
                "\n{} · {} puntos · OR {}\nEvidencia: {}\nProcedencia: {}\nContenido: {}…",
                e.scope,
                e.points,
                registry().reference_version,
                e.evidence,
                e.provenance,
                &v.source_sha256[..12]
            );
        } else {
            text += "\nSistemas: sin captura coincidente para esta versión del contenido.";
        }
    }
    text
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_completeness_does_not_promote_circulation_or_systems() {
        let audit = ConsistAudit {
            vehicles: 2,
            powered_vehicles: 1,
            ..Default::default()
        };
        assert!(audit.player_ready());
        let text = summary(&audit);
        assert!(text.contains("recursos: completos"));
        assert!(text.contains("sin ensayo"));
        assert!(text.contains("0/2"));
        assert!(evidence("").is_none());
    }
    #[test]
    fn evidence_requires_exact_sources_and_the_pinned_reference() {
        let mut r: Registry =
            serde_json::from_str(include_str!("../../../oracles/content-validation.json")).unwrap();
        let hash = r.records[0].source_sha256.clone();
        assert!(r.evidence(&hash).is_some());
        assert!(r.evidence(&format!("0{}", &hash[1..])).is_none());
        r.reference_version = "unknown".into();
        assert!(r.evidence(&hash).is_none());
    }
    #[test]
    fn records_name_only_profiles_present_in_the_original_capture() {
        let capture: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracles/openrails-brake-power.json"))
                .unwrap();
        for e in &registry().records {
            let profile = ["vacuum", "legacy_ep"]
                .iter()
                .flat_map(|k| capture[*k].as_array().unwrap())
                .find(|p| p["source"].as_str() == Some(&e.source))
                .unwrap();
            assert_eq!(profile["points"].as_array().unwrap().len(), e.points);
        }
    }
}
