//! Cross-parser audit for MSTS `.eng` / `.wag` files (openrailsrs vs OpenBVE catalog).

use std::collections::BTreeSet;
use std::path::Path;

use crate::ast::{Ast, Atom};
use crate::dispatch::{MstsFile, parse_msts_file};
use crate::error::FormatError;
use crate::vehicle_field_catalog::{
    ParserSupport, VehicleFieldSpec, VehicleKind, catalog_for_kind, lookup_field,
};

/// One symbol present in the file, annotated with catalog metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldAuditEntry {
    pub token: String,
    pub category: String,
    pub openrailsrs: ParserSupport,
    pub openbve: ParserSupport,
    pub notes: String,
}

/// Full audit report for a single vehicle file.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleAuditReport {
    pub path: String,
    pub vehicle_kind: VehicleKind,
    pub typed_parse_ok: bool,
    pub typed_parse_error: Option<String>,
    /// Distinct list-head symbols found in the AST.
    pub symbols_in_file: Vec<String>,
    /// Catalog hits for symbols present in the file.
    pub known_fields: Vec<FieldAuditEntry>,
    /// Symbols in file with no catalog row.
    pub unknown_symbols: Vec<String>,
    /// Present in file; OpenBVE parses; openrailsrs does not (actionable gaps).
    pub gaps_openbve_parsed_we_dont: Vec<String>,
    /// Present in file; openrailsrs parses; OpenBVE N/A or NotImplemented (OR-first fields).
    pub orrs_only_in_file: Vec<String>,
    /// Fraction of applicable catalog tokens marked Parsed in openrailsrs (0–1).
    pub openrailsrs_catalog_parsed_ratio: f64,
    /// Fraction of applicable catalog tokens marked Parsed in OpenBVE (0–1).
    pub openbve_catalog_parsed_ratio: f64,
}

impl VehicleAuditReport {
    pub fn format_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("=== Vehicle audit: {} ===\n", self.path));
        out.push_str(&format!(
            "kind={:?} typed_parse_ok={} symbols={}\n",
            self.vehicle_kind,
            self.typed_parse_ok,
            self.symbols_in_file.len()
        ));
        if let Some(err) = &self.typed_parse_error {
            out.push_str(&format!("typed_parse_error: {err}\n"));
        }
        out.push_str(&format!(
            "catalog coverage: openrailsrs {:.0}% parsed | OpenBVE {:.0}% parsed\n",
            self.openrailsrs_catalog_parsed_ratio * 100.0,
            self.openbve_catalog_parsed_ratio * 100.0
        ));

        if !self.gaps_openbve_parsed_we_dont.is_empty() {
            out.push_str("\n--- Gaps (OpenBVE parses, openrailsrs does not) ---\n");
            for token in &self.gaps_openbve_parsed_we_dont {
                if let Some(spec) = lookup_field(token) {
                    out.push_str(&format!(
                        "  {token} [{}/{}] — {}\n",
                        support_label(spec.openbve),
                        support_label(spec.openrailsrs),
                        spec.notes
                    ));
                } else {
                    out.push_str(&format!("  {token}\n"));
                }
            }
        }

        if !self.orrs_only_in_file.is_empty() {
            out.push_str("\n--- OR / openrailsrs-first (in file, not OpenBVE) ---\n");
            for token in &self.orrs_only_in_file {
                out.push_str(&format!("  {token}\n"));
            }
        }

        if !self.unknown_symbols.is_empty() {
            out.push_str("\n--- Unknown symbols (no catalog row) ---\n");
            for token in &self.unknown_symbols {
                out.push_str(&format!("  {token}\n"));
            }
        }

        out.push_str("\n--- Known fields in file ---\n");
        for entry in &self.known_fields {
            out.push_str(&format!(
                "  {:32} ORRS={:12} OBVE={:12} [{}]\n",
                entry.token,
                support_label(entry.openrailsrs),
                support_label(entry.openbve),
                entry.category,
            ));
        }
        out
    }
}

fn support_label(s: ParserSupport) -> &'static str {
    match s {
        ParserSupport::Parsed => "parsed",
        ParserSupport::Partial => "partial",
        ParserSupport::Ignored => "ignored",
        ParserSupport::NotImplemented => "no",
        ParserSupport::NotApplicable => "n/a",
    }
}

fn is_parsed(s: ParserSupport) -> bool {
    matches!(s, ParserSupport::Parsed | ParserSupport::Partial)
}

fn is_openbve_gap(spec: &VehicleFieldSpec) -> bool {
    is_parsed(spec.openbve)
        && matches!(
            spec.openrailsrs,
            ParserSupport::NotImplemented | ParserSupport::Ignored
        )
}

fn is_orrs_only(spec: &VehicleFieldSpec) -> bool {
    is_parsed(spec.openrailsrs)
        && matches!(
            spec.openbve,
            ParserSupport::NotImplemented | ParserSupport::NotApplicable
        )
}

/// Collect list-head symbols from an MSTS AST (case-preserving, deduped).
pub fn collect_msts_list_head_symbols(ast: &Ast) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_symbols_recursive(ast, &mut out);
    out
}

fn collect_symbols_recursive(ast: &Ast, out: &mut BTreeSet<String>) {
    if let Ast::List(items) = ast {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            out.insert(head.clone());
        }
        for item in items {
            collect_symbols_recursive(item, out);
        }
    }
}

fn vehicle_kind_from_path(path: &Path) -> VehicleKind {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("wag") => VehicleKind::Wagon,
        _ => VehicleKind::Engine,
    }
}

fn vehicle_kind_from_ast(ast: &Ast, fallback: VehicleKind) -> VehicleKind {
    let symbols = collect_msts_list_head_symbols(ast);
    let has_engine = symbols.iter().any(|s| s.eq_ignore_ascii_case("Engine"));
    let has_wagon = symbols.iter().any(|s| s.eq_ignore_ascii_case("Wagon"));
    match (has_engine, has_wagon) {
        (true, false) => VehicleKind::Engine,
        (false, true) => VehicleKind::Wagon,
        _ => fallback,
    }
}

/// Audit a vehicle file on disk.
pub fn audit_vehicle_file(path: impl AsRef<Path>) -> Result<VehicleAuditReport, FormatError> {
    let path = path.as_ref();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(ext.as_deref(), Some("eng") | Some("wag")) {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: format!("audit-vehicle expects .eng or .wag, got {}", path.display()),
        });
    }

    let fallback_kind = vehicle_kind_from_path(path);
    let parsed = parse_msts_file(path);
    let (typed_parse_ok, typed_parse_error) = match &parsed {
        Ok(MstsFile::Engine(_)) | Ok(MstsFile::Wagon(_)) => (true, None),
        Ok(other) => (false, Some(format!("unexpected dispatch: {other:?}"))),
        Err(e) => (false, Some(e.to_string())),
    };

    let source = crate::msts_file_text::read_msts_file_decoded(path)?;
    let ast = crate::parser::parse_from_first_paren(&source)?;
    let kind = vehicle_kind_from_ast(&ast, fallback_kind);
    let symbols = collect_msts_list_head_symbols(&ast);

    let mut known_fields = Vec::new();
    let mut unknown_symbols = Vec::new();
    let mut gaps_openbve_parsed_we_dont = Vec::new();
    let mut orrs_only_in_file = Vec::new();
    let mut seen_catalog = BTreeSet::new();

    for symbol in &symbols {
        if let Some(spec) = lookup_field(symbol) {
            if !applies_to_kind(spec, kind) {
                continue;
            }
            let canonical = spec.token.to_string();
            if !seen_catalog.insert(canonical.clone()) {
                continue;
            }
            known_fields.push(FieldAuditEntry {
                token: canonical.clone(),
                category: spec.category.to_string(),
                openrailsrs: spec.openrailsrs,
                openbve: spec.openbve,
                notes: spec.notes.to_string(),
            });
            if is_openbve_gap(spec) {
                gaps_openbve_parsed_we_dont.push(canonical.clone());
            }
            if is_orrs_only(spec) {
                orrs_only_in_file.push(canonical);
            }
        } else if !is_structural_noise(symbol) {
            unknown_symbols.push(symbol.clone());
        }
    }

    known_fields.sort_by(|a, b| a.token.cmp(&b.token));
    gaps_openbve_parsed_we_dont.sort();
    orrs_only_in_file.sort();
    unknown_symbols.sort();

    let (orrs_ratio, obve_ratio) = catalog_coverage_ratios(kind);

    Ok(VehicleAuditReport {
        path: path.display().to_string(),
        vehicle_kind: kind,
        typed_parse_ok,
        typed_parse_error,
        symbols_in_file: symbols.into_iter().collect(),
        known_fields,
        unknown_symbols,
        gaps_openbve_parsed_we_dont,
        orrs_only_in_file,
        openrailsrs_catalog_parsed_ratio: orrs_ratio,
        openbve_catalog_parsed_ratio: obve_ratio,
    })
}

fn applies_to_kind(spec: &VehicleFieldSpec, kind: VehicleKind) -> bool {
    spec.applies_to == VehicleKind::Both || spec.applies_to == kind
}

fn is_structural_noise(symbol: &str) -> bool {
    matches!(
        symbol.to_ascii_uppercase().as_str(),
        "ENGINE"
            | "WAGON"
            | "TYPE"
            | "COMMENT"
            | "DESCRIPTION"
            | "STATE"
            | "STYLE"
            | "SWITCHVAL"
            | "STATES"
            | "POSITIVECOLOUR"
            | "CONTROLCOLOR"
            | "NEGATIVECOLOUR"
            | "DECREASECOLOUR"
            | "ORTSFONT"
            | "SUBOBJECT"
            | "SUBOBJECTS"
    )
}

fn catalog_coverage_ratios(kind: VehicleKind) -> (f64, f64) {
    let specs: Vec<_> = catalog_for_kind(kind).collect();
    if specs.is_empty() {
        return (0.0, 0.0);
    }
    let orrs_parsed = specs.iter().filter(|s| is_parsed(s.openrailsrs)).count();
    let obve_parsed = specs.iter().filter(|s| is_parsed(s.openbve)).count();
    let n = specs.len() as f64;
    (orrs_parsed as f64 / n, obve_parsed as f64 / n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn collect_symbols_from_minimal_eng() {
        let src = std::fs::read_to_string("tests/fixtures/typed_minimal.eng").unwrap();
        let ast = parse_from_first_paren(&src).unwrap();
        let syms = collect_msts_list_head_symbols(&ast);
        assert!(syms.iter().any(|s| s.eq_ignore_ascii_case("Mass")));
        assert!(syms.iter().any(|s| s.eq_ignore_ascii_case("MaxPower")));
    }

    #[test]
    fn audit_minimal_eng_finds_known_fields() {
        let report = audit_vehicle_file("tests/fixtures/typed_minimal.eng").expect("audit");
        assert!(report.typed_parse_ok);
        assert!(
            report
                .known_fields
                .iter()
                .any(|e| e.token.eq_ignore_ascii_case("Mass"))
        );
    }

    #[test]
    fn audit_chiltern_dmbsa_finds_orts_gaps_and_orrs_only() {
        let path = "../../examples/chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let report = audit_vehicle_file(path).expect("audit");
        assert!(report.typed_parse_ok);
        assert!(
            report
                .orrs_only_in_file
                .iter()
                .any(|t| t.contains("ORTSMaxTractiveForce") || t.contains("ORTSDiesel"))
        );
    }
}
